//! Récupération d'une page ou d'une image pour un aperçu de lien, protégée contre le SSRF
//! (faire joindre au serveur son réseau interne par un lien posté dans un message).
//!
//! Le client HTTP est écrit à la main sur hyper : on résout le nom nous-mêmes, on vérifie
//! CHAQUE adresse obtenue, puis on se connecte à cette adresse précise (jamais à un nom que le
//! système résoudrait une seconde fois : pas de « DNS rebinding »). Chaque redirection repart
//! de zéro, adresse par adresse. Refusés : tout schéma autre que http/https, tout port autre
//! que 80 et 443, les identifiants dans l'adresse, et les adresses de [`ip_allowed`].

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper::header::{
    ACCEPT, ACCEPT_ENCODING, CONNECTION, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, HOST, LOCATION,
};
use hyper::{Request, StatusCode};
use hyper_util::rt::TokioIo;
use rustls::pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use url::{Host, Position, Url};

pub const USER_AGENT: &str = "KRAKEN-link-preview";
/// Redirections suivies au plus.
pub const MAX_REDIRECTS: usize = 3;
/// Durée accordée à chaque étape (résolution, connexion, réponse et corps).
pub const TIMEOUT: Duration = Duration::from_secs(5);
/// Octets d'une page lus au plus (le reste est ignoré).
pub const MAX_HTML: usize = 1024 * 1024;
/// Octets d'une image d'aperçu au plus (au-delà, elle est refusée).
pub const MAX_IMAGE: usize = 4 * 1024 * 1024;

/// Pourquoi une adresse est refusée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    Scheme,
    Port,
    Credentials,
    Host,
    /// Adresse interne (loopback, privée, lien local…).
    Address(IpAddr),
    NoAddress,
    TooManyRedirects,
    BadRedirect,
}

/// Réglage du garde. `allow_private` ne sert qu'aux tests (`Config::preview_allow_private`,
/// éteint par défaut) : il lève l'interdiction des adresses internes et des ports.
#[derive(Debug, Clone, Copy, Default)]
pub struct Policy {
    pub allow_private: bool,
}

/// Cette adresse est-elle joignable depuis Internet ? Non pour : 0.0.0.0/8, 10/8, 100.64/10
/// (CGNAT), 127/8, 169.254/16, 172.16/12, 192.0.0/24, 192.0.2/24, 192.168/16, 198.18/15,
/// 198.51.100/24, 203.0.113/24, multidiffusion et réservé 224/3 (dont 255.255.255.255) ; en
/// IPv6 : ::, ::1, ff00::/8, fc00::/7 (ULA), fe80::/10 et fec0::/10, 2001:db8::/32, 100::/64,
/// Teredo, et les formes qui en cachent une IPv4 (mappée, 6to4, NAT64) si celle-ci est refusée.
pub fn ip_allowed(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4_allowed(v4),
        IpAddr::V6(v6) => v6_allowed(v6),
    }
}

fn v4_allowed(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(a == 0
        || a == 10
        || a == 127
        || (a == 100 && (64..128).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..32).contains(&b))
        || (a == 192 && b == 0 && (c == 0 || c == 2))
        || (a == 192 && b == 168)
        || (a == 198 && (b == 18 || b == 19))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
        || a >= 224)
}

fn v6_allowed(ip: Ipv6Addr) -> bool {
    if ip.is_unspecified() || ip.is_loopback() || ip.is_multicast() {
        return false;
    }
    // ::ffff:a.b.c.d et l'ancien ::a.b.c.d : l'IPv4 qu'elles cachent décide.
    if let Some(v4) = ip.to_ipv4() {
        return v4_allowed(v4);
    }
    let s = ip.segments();
    // fc00::/7 (ULA), fe80::/10 (lien local), fec0::/10 (site local, abandonné).
    if (s[0] & 0xfe00) == 0xfc00 || (s[0] & 0xffc0) == 0xfe80 || (s[0] & 0xffc0) == 0xfec0 {
        return false;
    }
    // 2001:db8::/32 (documentation), 2001::/32 (Teredo), 100::/64 (trou noir).
    if (s[0] == 0x2001 && (s[1] == 0x0db8 || s[1] == 0)) || (s[0] == 0x0100 && s[1] == 0 && s[2] == 0 && s[3] == 0) {
        return false;
    }
    // 64:ff9b::/96 (NAT64) et 2002::/16 (6to4) portent une IPv4 : elle décide aussi.
    if s[0] == 0x64 && s[1] == 0xff9b && s[2..6].iter().all(|x| *x == 0) {
        return v4_allowed(Ipv4Addr::new((s[6] >> 8) as u8, s[6] as u8, (s[7] >> 8) as u8, s[7] as u8));
    }
    if s[0] == 0x2002 {
        return v4_allowed(Ipv4Addr::new((s[1] >> 8) as u8, s[1] as u8, (s[2] >> 8) as u8, s[2] as u8));
    }
    true
}

/// Contrôles qui ne demandent pas le réseau : schéma, identifiants, port, et adresse si le
/// serveur est donné par son IP. `Ok(Some(ip))` : l'adresse écrite dans l'URL, déjà vérifiée ;
/// `Ok(None)` : un nom, à résoudre puis à vérifier avec [`vet_addrs`].
pub fn check_url(url: &Url, policy: Policy) -> Result<Option<IpAddr>, Refused> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(Refused::Scheme);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Refused::Credentials);
    }
    let port = url.port_or_known_default().ok_or(Refused::Port)?;
    if !policy.allow_private && port != 80 && port != 443 {
        return Err(Refused::Port);
    }
    match url.host().ok_or(Refused::Host)? {
        Host::Domain("") => Err(Refused::Host),
        Host::Domain(_) => Ok(None),
        Host::Ipv4(ip) => literal(IpAddr::V4(ip), policy),
        Host::Ipv6(ip) => literal(IpAddr::V6(ip), policy),
    }
}

fn literal(ip: IpAddr, policy: Policy) -> Result<Option<IpAddr>, Refused> {
    if policy.allow_private || ip_allowed(ip) { Ok(Some(ip)) } else { Err(Refused::Address(ip)) }
}

/// Ce que la résolution d'un nom a donné : toutes les adresses doivent être publiques (un nom
/// qui en mêle une interne est refusé en entier), sinon rien n'est joint.
pub fn vet_addrs(found: &[SocketAddr], policy: Policy) -> Result<Vec<SocketAddr>, Refused> {
    if found.is_empty() {
        return Err(Refused::NoAddress);
    }
    if !policy.allow_private
        && let Some(bad) = found.iter().find(|a| !ip_allowed(a.ip()))
    {
        return Err(Refused::Address(bad.ip()));
    }
    Ok(found.to_vec())
}

/// La destination d'une redirection `Location`, relative ou absolue, contrôlée comme une
/// première adresse. `hops` : redirections déjà suivies.
pub fn next_hop(from: &Url, location: &str, hops: usize, policy: Policy) -> Result<Url, Refused> {
    if hops >= MAX_REDIRECTS {
        return Err(Refused::TooManyRedirects);
    }
    let to = from.join(location.trim()).map_err(|_| Refused::BadRedirect)?;
    check_url(&to, policy)?;
    Ok(to)
}

/// Ce qu'on attend en réponse.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Want {
    Html,
    Image,
}

#[derive(Debug)]
pub struct Fetched {
    /// Adresse finale, après redirections (base des liens relatifs de la page).
    pub url: Url,
    pub body: Vec<u8>,
}

/// Récupère `url` (redirections suivies, au plus [`MAX_REDIRECTS`]).
pub async fn fetch(url: &str, want: Want, policy: Policy) -> Result<Fetched, String> {
    let mut url = Url::parse(url).map_err(|_| "adresse illisible".to_string())?;
    for hops in 0..=MAX_REDIRECTS {
        check_url(&url, policy).map_err(|r| format!("{r:?}"))?;
        match tokio::time::timeout(TIMEOUT, hop(&url, want, policy)).await {
            Err(_) => return Err("délai dépassé".into()),
            Ok(Err(e)) => return Err(e),
            Ok(Ok(Hop::Done(body))) => return Ok(Fetched { url, body }),
            Ok(Ok(Hop::Redirect(loc))) => url = next_hop(&url, &loc, hops, policy).map_err(|r| format!("{r:?}"))?,
        }
    }
    Err(format!("{:?}", Refused::TooManyRedirects))
}

enum Hop {
    Redirect(String),
    Done(Vec<u8>),
}

fn tls() -> Arc<rustls::ClientConfig> {
    static CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let mut roots = rustls::RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let cfg = rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .expect("versions TLS")
                .with_root_certificates(roots)
                .with_no_client_auth();
            Arc::new(cfg)
        })
        .clone()
}

async fn hop(url: &Url, want: Want, policy: Policy) -> Result<Hop, String> {
    let port = url.port_or_known_default().ok_or("port")?;
    let addrs = match check_url(url, policy).map_err(|r| format!("{r:?}"))? {
        Some(ip) => vec![SocketAddr::new(ip, port)],
        None => {
            let host = url.host_str().ok_or("hôte")?;
            let found: Vec<SocketAddr> =
                tokio::net::lookup_host((host, port)).await.map_err(|_| "DNS".to_string())?.collect();
            vet_addrs(&found, policy).map_err(|r| format!("{r:?}"))?
        }
    };
    // Connexion à l'adresse vérifiée, et à elle seule.
    let mut stream = None;
    for a in addrs {
        if let Ok(s) = TcpStream::connect(a).await {
            stream = Some(s);
            break;
        }
    }
    let stream = stream.ok_or("connexion impossible")?;
    if url.scheme() == "https" {
        let name = match url.host().ok_or("hôte")? {
            Host::Domain(d) => ServerName::try_from(d.to_string()).map_err(|_| "nom TLS")?,
            Host::Ipv4(ip) => ServerName::IpAddress(IpAddr::V4(ip).into()),
            Host::Ipv6(ip) => ServerName::IpAddress(IpAddr::V6(ip).into()),
        };
        let io = tokio_rustls::TlsConnector::from(tls()).connect(name, stream).await.map_err(|_| "TLS".to_string())?;
        exchange(io, url, want).await
    } else {
        exchange(stream, url, want).await
    }
}

async fn exchange<T>(io: T, url: &Url, want: Want) -> Result<Hop, String>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(io)).await.map_err(|_| "HTTP")?;
    let driver = tokio::spawn(async move {
        let _ = conn.await;
    });
    let host = &url[Position::BeforeHost..Position::AfterPort];
    let accept = if want == Want::Html { "text/html,application/xhtml+xml" } else { "image/*" };
    let req = Request::builder()
        .method("GET")
        .uri(&url[Position::BeforePath..Position::AfterQuery])
        .header(HOST, host)
        .header("user-agent", USER_AGENT)
        .header(ACCEPT, accept)
        .header(ACCEPT_ENCODING, "identity")
        .header(CONNECTION, "close")
        .body(Empty::<Bytes>::new())
        .map_err(|_| "requête")?;
    let result = async {
        let resp = sender.send_request(req).await.map_err(|_| "HTTP".to_string())?;
        let status = resp.status();
        if status.is_redirection() {
            let loc = resp.headers().get(LOCATION).and_then(|v| v.to_str().ok()).ok_or("redirection sans Location")?;
            return Ok(Hop::Redirect(loc.to_string()));
        }
        if status != StatusCode::OK {
            return Err(format!("statut {status}"));
        }
        let h = resp.headers();
        if h.get(CONTENT_ENCODING).is_some_and(|v| !v.as_bytes().eq_ignore_ascii_case(b"identity")) {
            return Err("réponse compressée".into());
        }
        let ctype = h.get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
        let max = match want {
            Want::Html if ctype.starts_with("text/html") || ctype.starts_with("application/xhtml+xml") => MAX_HTML,
            Want::Image if ctype.starts_with("image/") => MAX_IMAGE,
            _ => return Err(format!("type refusé : {ctype}")),
        };
        let declared = h.get(CONTENT_LENGTH).and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<usize>().ok());
        if want == Want::Image && declared.is_some_and(|n| n > max) {
            return Err("image trop lourde".into());
        }
        let mut body = resp.into_body();
        let mut out = Vec::new();
        while let Some(frame) = body.frame().await {
            let frame = frame.map_err(|_| "lecture".to_string())?;
            if let Ok(data) = frame.into_data() {
                out.extend_from_slice(&data);
                if out.len() >= max {
                    // Une page tronquée se lit encore ; une image tronquée est inutilisable.
                    if want == Want::Image && out.len() > max {
                        return Err("image trop lourde".into());
                    }
                    out.truncate(max);
                    break;
                }
            }
        }
        Ok(Hop::Done(out))
    }
    .await;
    driver.abort();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    const NO: Policy = Policy { allow_private: false };

    #[test]
    fn internal_addresses_are_refused() {
        for bad in [
            "127.0.0.1",
            "127.255.0.9",
            "10.0.0.1",
            "10.255.255.255",
            "172.16.0.1",
            "172.31.255.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "100.127.255.254",
            "0.0.0.0",
            "0.1.2.3",
            "224.0.0.1",
            "239.1.1.1",
            "255.255.255.255",
            "240.0.0.1",
            "192.0.2.5",
            "198.18.0.1",
            "::1",
            "::",
            "fe80::1",
            "fc00::1",
            "fd12:3456::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:10.1.2.3",
            "::ffff:169.254.169.254",
            "64:ff9b::7f00:1",
            "2002:7f00:1::1",
            "2001:db8::1",
        ] {
            assert!(!ip_allowed(ip(bad)), "{bad} devrait être refusée");
        }
        for good in [
            "93.184.216.34",
            "8.8.8.8",
            "172.15.0.1",
            "172.32.0.1",
            "100.63.0.1",
            "2606:2800:220:1::1",
            "::ffff:8.8.8.8",
        ] {
            assert!(ip_allowed(ip(good)), "{good} devrait passer");
        }
    }

    #[test]
    fn guard_refuses_literal_targets() {
        for bad in [
            "http://127.0.0.1/",
            "http://10.1.2.3/x",
            "http://169.254.169.254/latest/meta-data/",
            "http://[::1]/",
            "http://[fd00::1]:80/",
            "http://[::ffff:7f00:1]/",
            // Écritures détournées que le navigateur et le système acceptent.
            "http://2130706433/",
            "http://0x7f.0.0.1/",
            "http://127.1/",
            "http://0/",
        ] {
            assert!(matches!(check_url(&url(bad), NO), Err(Refused::Address(_))), "{bad}");
        }
        assert_eq!(check_url(&url("http://93.184.216.34/"), NO), Ok(Some(ip("93.184.216.34"))));
        assert_eq!(check_url(&url("https://exemple.org/page"), NO), Ok(None));
        assert_eq!(check_url(&url("ftp://exemple.org/"), NO), Err(Refused::Scheme));
        assert_eq!(check_url(&url("file:///etc/passwd"), NO), Err(Refused::Scheme));
        assert_eq!(check_url(&url("https://exemple.org:8443/"), NO), Err(Refused::Port));
        assert_eq!(check_url(&url("http://exemple.org:22/"), NO), Err(Refused::Port));
        assert_eq!(check_url(&url("https://exemple.org:443/"), NO), Ok(None));
        assert_eq!(check_url(&url("http://exemple.org:80/"), NO), Ok(None));
        assert_eq!(check_url(&url("http://user:mdp@exemple.org/"), NO), Err(Refused::Credentials));
        // Le réglage de test lève les interdits d'adresse et de port, rien d'autre.
        let open = Policy { allow_private: true };
        assert_eq!(check_url(&url("http://127.0.0.1:8123/"), open), Ok(Some(ip("127.0.0.1"))));
        assert_eq!(check_url(&url("gopher://127.0.0.1/"), open), Err(Refused::Scheme));
    }

    #[test]
    fn a_name_resolving_inside_is_refused() {
        let sa = |s: &str| SocketAddr::new(ip(s), 443);
        assert_eq!(vet_addrs(&[], NO), Err(Refused::NoAddress));
        assert!(vet_addrs(&[sa("93.184.216.34"), sa("2606:2800:220:1::1")], NO).is_ok());
        assert_eq!(vet_addrs(&[sa("127.0.0.1")], NO), Err(Refused::Address(ip("127.0.0.1"))));
        // Une seule adresse interne parmi des publiques suffit à tout refuser.
        assert_eq!(vet_addrs(&[sa("93.184.216.34"), sa("10.0.0.7")], NO), Err(Refused::Address(ip("10.0.0.7"))));
        assert!(vet_addrs(&[sa("127.0.0.1")], Policy { allow_private: true }).is_ok());
    }

    #[test]
    fn redirects_are_checked_at_every_hop() {
        let from = url("https://exemple.org/a");
        // Vers l'intérieur, en absolu ou par une écriture détournée.
        for bad in [
            "http://127.0.0.1/admin",
            "http://169.254.169.254/",
            "http://[::1]:80/",
            "http://10.0.0.1/",
            "http://2130706433/",
        ] {
            assert!(matches!(next_hop(&from, bad, 0, NO), Err(Refused::Address(_))), "{bad}");
        }
        assert_eq!(next_hop(&from, "ftp://exemple.org/", 0, NO), Err(Refused::Scheme));
        assert_eq!(next_hop(&from, "https://exemple.org:9000/", 0, NO), Err(Refused::Port));
        // Relative : résolue contre la page, donc soumise aux mêmes contrôles.
        assert_eq!(next_hop(&from, "/b?x=1", 0, NO).unwrap().as_str(), "https://exemple.org/b?x=1");
        assert_eq!(
            next_hop(&from, "//cdn.exemple.org/i.png", 1, NO).unwrap().as_str(),
            "https://cdn.exemple.org/i.png"
        );
        // Trois redirections au plus.
        assert!(next_hop(&from, "/b", 2, NO).is_ok());
        assert_eq!(next_hop(&from, "/b", 3, NO), Err(Refused::TooManyRedirects));
    }

    /// Un petit serveur HTTP local : chemin → (statut, en-têtes, corps).
    type Route = (&'static str, u16, Vec<(&'static str, String)>, Vec<u8>);

    async fn serve(routes: Vec<Route>) -> u16 {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = listener.accept().await else { return };
                let routes = routes.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    let head = String::from_utf8_lossy(&buf[..n]).to_string();
                    let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let ua_ok = head.to_lowercase().contains("user-agent: kraken-link-preview");
                    let (status, headers, body) = match routes.iter().find(|r| r.0 == path) {
                        Some(r) if ua_ok => (r.1, r.2.clone(), r.3.clone()),
                        _ => (404, vec![], vec![]),
                    };
                    let mut out =
                        format!("HTTP/1.1 {status} X\r\ncontent-length: {}\r\nconnection: close\r\n", body.len());
                    for (k, v) in headers {
                        out.push_str(&format!("{k}: {v}\r\n"));
                    }
                    out.push_str("\r\n");
                    let _ = s.write_all(out.as_bytes()).await;
                    let _ = s.write_all(&body).await;
                });
            }
        });
        port
    }

    fn html() -> Vec<(&'static str, String)> {
        vec![("content-type", "text/html; charset=utf-8".into())]
    }

    #[tokio::test]
    async fn fetch_follows_redirects_and_limits() {
        let open = Policy { allow_private: true };
        let big = vec![b'a'; MAX_HTML + 5000];
        let port = serve(vec![
            ("/ok", 200, html(), b"<title>Salut</title>".to_vec()),
            ("/r1", 302, vec![("location", "/r2".into())], vec![]),
            ("/r2", 302, vec![("location", "/r3".into())], vec![]),
            ("/r3", 301, vec![("location", "/ok".into())], vec![]),
            ("/r4", 302, vec![("location", "/r1".into())], vec![]),
            ("/inside", 302, vec![("location", "http://169.254.169.254/latest".into())], vec![]),
            ("/json", 200, vec![("content-type", "application/json".into())], b"{}".to_vec()),
            ("/big", 200, html(), big),
            ("/png", 200, vec![("content-type", "image/png".into())], vec![0; 10]),
        ])
        .await;
        let at = |p: &str| format!("http://127.0.0.1:{port}{p}");
        let f = fetch(&at("/ok"), Want::Html, open).await.unwrap();
        assert_eq!(f.body, b"<title>Salut</title>");
        // Trois redirections : acceptées ; quatre : refusées.
        assert!(fetch(&at("/r1"), Want::Html, open).await.is_ok());
        assert!(fetch(&at("/r4"), Want::Html, open).await.unwrap_err().contains("TooManyRedirects"));
        // Un seul octet de trop en type : refusé. Une page trop longue est tronquée à 1 Mio.
        assert!(fetch(&at("/json"), Want::Html, open).await.unwrap_err().contains("type refusé"));
        assert_eq!(fetch(&at("/big"), Want::Html, open).await.unwrap().body.len(), MAX_HTML);
        assert!(fetch(&at("/png"), Want::Html, open).await.is_err());
        assert!(fetch(&at("/png"), Want::Image, open).await.is_ok());
        // Redirection vers une adresse interne, même quand le garde est ouvert aux tests
        // pour 127.0.0.1 : le contrôle des adresses de redirection reste celui du réglage.
        let strict = fetch(&at("/ok"), Want::Html, NO).await.unwrap_err();
        assert!(strict.contains("Address") || strict.contains("Port"), "{strict}");
        // Sans le réglage, le même serveur local n'est joignable ni directement ni par redirection.
        assert!(fetch(&at("/inside"), Want::Html, NO).await.is_err());
    }
}
