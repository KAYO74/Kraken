//! `--self-test` (pour la CI) : démarre PostgreSQL et KRAKEN dans un dossier temporaire,
//! vérifie l'API, le client web, une inscription et le vocal, arrête tout. Aucune fenêtre n'est
//! ouverte.

use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use kraken_proto::{AuthResp, ChannelKind, GuildState, VOICE_DATA_CHANNEL, VoiceAnswer, VoiceJoin, VoiceMids};
use str0m::change::SdpAnswer;
use str0m::media::{Direction, MediaKind};
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, Input, Output, Rtc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::host::Server;

/// Code de sortie : 0 si tout va bien.
pub fn run(web: &Path) -> i32 {
    let dir = std::env::temp_dir().join(format!("kraken-self-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let result = test(&dir, web);
    let _ = std::fs::remove_dir_all(&dir);
    match result {
        Ok(()) => {
            eprintln!("self-test : tout fonctionne");
            0
        }
        Err(e) => {
            eprintln!("self-test : ÉCHEC : {e}");
            1
        }
    }
}

fn test(dir: &Path, web: &Path) -> Result<(), String> {
    eprintln!("self-test : client web dans {}", web.display());
    let server = Server::start(dir, web, 0, &|step| eprintln!("self-test : {step}"))
        .map_err(|f| format!("{} ({})", f.message, f.details))?;
    eprintln!("self-test : KRAKEN sur {} (PostgreSQL sur 127.0.0.1:{})", server.url, server.pg_port());
    let local = SocketAddr::from((Ipv4Addr::LOCALHOST, server.port));
    let result = server.block_on(checks(local));
    let result = result.and_then(|token| voice(&server, local, &token));
    let result = result.and_then(|()| db_not_on_network(&server));
    server.stop();
    result
}

/// Renvoie le jeton de session du compte créé.
async fn checks(addr: SocketAddr) -> Result<String, String> {
    let (status, _) = http(addr, "GET", "/api/v1/health", None, None).await?;
    expect("GET /api/v1/health", status, 200)?;
    let (status, body) = http(addr, "GET", "/", None, None).await?;
    expect("GET /", status, 200)?;
    if !body.contains("boot.js") {
        return Err("GET / ne renvoie pas le client web (index.html)".into());
    }
    let body = r#"{"username":"selftest","email":"selftest@example.invalid","password":"kraken-self-test"}"#;
    let (status, reply) = http(addr, "POST", "/api/v1/auth/register", Some(body), None).await?;
    expect("POST /api/v1/auth/register", status, 201)?;
    let auth: AuthResp = parse("POST /api/v1/auth/register", &reply)?;
    eprintln!("self-test : API, client web et inscription OK");
    Ok(auth.token)
}

/// Le vocal comme le client web : un « navigateur » str0m rejoint le salon vocal d'un nouveau
/// serveur par l'API, puis en UDP (ICE, DTLS, canal de données), et le quitte.
fn voice(server: &Server, api: SocketAddr, token: &str) -> Result<(), String> {
    let port = server.voice_port().ok_or("le vocal n'a pas démarré : aucun port UDP")?;
    let call =
        |method: &str, path: &str, body: Option<&str>| server.block_on(http(api, method, path, body, Some(token)));
    let (status, body) = call("POST", "/api/v1/guilds", Some(r#"{"name":"Self-test"}"#))?;
    expect("POST /api/v1/guilds", status, 201)?;
    let guild: GuildState = parse("POST /api/v1/guilds", &body)?;
    let room =
        guild.channels.iter().find(|c| c.kind == ChannelKind::Voice).ok_or("nouveau serveur sans salon vocal")?;

    // Sur l'adresse que le serveur annonce en premier : celle du réseau local (ou 127.0.0.1).
    let ip = server.url.strip_prefix("http://").and_then(|r| r.split(':').next()).and_then(|ip| ip.parse().ok());
    let ip: IpAddr = ip.unwrap_or(Ipv4Addr::LOCALHOST.into());
    let socket = UdpSocket::bind((ip, 0)).map_err(|e| format!("vocal : prise UDP sur {ip} : {e}"))?;
    let local = socket.local_addr().map_err(|e| e.to_string())?;
    let mut rtc =
        Rtc::builder().set_crypto_provider(Arc::new(str0m::crypto::from_feature_flags())).build(Instant::now());
    rtc.add_local_candidate(Candidate::host(local, "udp").map_err(|e| e.to_string())?);
    // Micro, image et son de l'écran, puis le canal `kraken` : l'offre du client web.
    let mut sdp = rtc.sdp_api();
    let [mic, screen, screen_audio] = [MediaKind::Audio, MediaKind::Video, MediaKind::Audio]
        .map(|kind| sdp.add_media(kind, Direction::SendOnly, None, None, None).to_string());
    sdp.add_channel(VOICE_DATA_CHANNEL.into());
    let (offer, pending) = sdp.apply().ok_or("vocal : offre WebRTC impossible")?;
    let join = VoiceJoin {
        sdp: offer.to_sdp_string(),
        mids: VoiceMids { mic, screen, screen_audio },
        self_mute: true,
        self_deaf: false,
    };
    let path = format!("/api/v1/channels/{}/voice", room.id);
    let (status, body) = call("POST", &path, Some(&serde_json::to_string(&join).map_err(|e| e.to_string())?))?;
    expect(&format!("POST {path}"), status, 200)?;
    let answer: VoiceAnswer = parse(&path, &body)?;
    let answer = SdpAnswer::from_sdp_string(&answer.sdp).map_err(|e| format!("vocal : réponse illisible : {e}"))?;
    rtc.sdp_api().accept_answer(pending, answer).map_err(|e| format!("vocal : réponse refusée : {e}"))?;

    let deadline = Instant::now() + Duration::from_secs(15);
    let mut buf = vec![0; 2000];
    let mut open = false;
    while !open {
        if Instant::now() > deadline {
            return Err(format!("vocal : pas de connexion avec le serveur (UDP {port}, depuis {local})"));
        }
        let next = loop {
            match rtc.poll_output().map_err(|e| format!("vocal : {e}"))? {
                Output::Timeout(t) => break t,
                Output::Transmit(t) => {
                    let _ = socket.send_to(&t.contents, t.destination);
                }
                Output::Event(Event::ChannelOpen(_, label)) if label == VOICE_DATA_CHANNEL => open = true,
                Output::Event(_) => {}
            }
        };
        let wait =
            next.saturating_duration_since(Instant::now()).clamp(Duration::from_millis(1), Duration::from_millis(50));
        socket.set_read_timeout(Some(wait)).map_err(|e| e.to_string())?;
        let input = match socket.recv_from(&mut buf) {
            Ok((n, from)) => match Receive::new(Protocol::Udp, from, local, &buf[..n]) {
                Ok(r) => Input::Receive(Instant::now(), r),
                Err(_) => continue,
            },
            // Windows signale ainsi un envoi précédent resté sans destinataire.
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::ConnectionReset) => {
                Input::Timeout(Instant::now())
            }
            Err(e) => return Err(format!("vocal : réception UDP : {e}")),
        };
        rtc.handle_input(input).map_err(|e| format!("vocal : {e}"))?;
    }

    let path = format!("/api/v1/guilds/{}", guild.guild.id);
    let (status, body) = call("GET", &path, None)?;
    expect(&format!("GET {path}"), status, 200)?;
    let now: GuildState = parse(&path, &body)?;
    if !now.voice_states.iter().any(|s| s.channel_id == Some(room.id) && s.self_mute) {
        return Err("vocal : connecté, mais absent du salon pour les autres".into());
    }
    let (status, _) = call("DELETE", "/api/v1/voice", None)?;
    expect("DELETE /api/v1/voice", status, 204)?;
    eprintln!("self-test : vocal OK (UDP {port}, canal de données ouvert depuis {local})");
    Ok(())
}

fn parse<T: serde::de::DeserializeOwned>(what: &str, body: &str) -> Result<T, String> {
    serde_json::from_str(body).map_err(|e| format!("{what} : réponse illisible ({e})"))
}

/// PostgreSQL ne doit répondre que sur 127.0.0.1, pas sur l'adresse réseau.
fn db_not_on_network(server: &Server) -> Result<(), String> {
    let Some(ip) = server.url.strip_prefix("http://").and_then(|r| r.split(':').next()) else { return Ok(()) };
    let Ok(ip) = ip.parse::<Ipv4Addr>() else { return Ok(()) };
    if ip.is_loopback() {
        return Ok(());
    }
    let addr = SocketAddr::from((ip, server.pg_port()));
    match TcpStream::connect_timeout(&addr, Duration::from_secs(2)) {
        Ok(_) => Err(format!("PostgreSQL est joignable depuis le réseau ({addr})")),
        Err(_) => {
            eprintln!("self-test : PostgreSQL injoignable sur {addr}, comme prévu");
            Ok(())
        }
    }
}

fn expect(what: &str, got: u16, want: u16) -> Result<(), String> {
    if got == want { Ok(()) } else { Err(format!("{what} : statut {got}, attendu {want}")) }
}

/// Un client HTTP/1.1 minimal : une requête par connexion, avec un jeton de session ou non.
async fn http(
    addr: SocketAddr,
    method: &str,
    path: &str,
    body: Option<&str>,
    token: Option<&str>,
) -> Result<(u16, String), String> {
    let exchange = async {
        let mut stream = tokio::net::TcpStream::connect(addr).await?;
        let body = body.unwrap_or("");
        let auth = token.map(|t| format!("Authorization: Session {t}\r\n")).unwrap_or_default();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nUser-Agent: kraken-self-test\r\nConnection: close\r\n\
             {auth}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).await?;
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await?;
        Ok::<_, std::io::Error>(raw)
    };
    let raw = tokio::time::timeout(Duration::from_secs(30), exchange)
        .await
        .map_err(|_| format!("{method} {path} : pas de réponse"))?
        .map_err(|e| format!("{method} {path} : {e}"))?;
    let text = String::from_utf8_lossy(&raw);
    let status =
        text.split(' ').nth(1).and_then(|c| c.parse().ok()).ok_or(format!("{method} {path} : réponse illisible"))?;
    let body = text.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default();
    Ok((status, body))
}
