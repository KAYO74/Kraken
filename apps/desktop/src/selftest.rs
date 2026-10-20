//! `--self-test` (pour la CI) : démarre PostgreSQL et KRAKEN dans un dossier temporaire,
//! vérifie l'API, le client web et une inscription, arrête tout. Aucune fenêtre n'est ouverte.

use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::time::Duration;

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
    let result = result.and_then(|()| db_not_on_network(&server));
    server.stop();
    result
}

async fn checks(addr: SocketAddr) -> Result<(), String> {
    let (status, _) = http(addr, "GET", "/api/v1/health", None).await?;
    expect("GET /api/v1/health", status, 200)?;
    let (status, body) = http(addr, "GET", "/", None).await?;
    expect("GET /", status, 200)?;
    if !body.contains("boot.js") {
        return Err("GET / ne renvoie pas le client web (index.html)".into());
    }
    let body = r#"{"username":"selftest","email":"selftest@example.invalid","password":"kraken-self-test"}"#;
    let (status, reply) = http(addr, "POST", "/api/v1/auth/register", Some(body)).await?;
    expect("POST /api/v1/auth/register", status, 201)?;
    if !reply.contains("\"token\"") {
        return Err("inscription sans jeton de session".into());
    }
    eprintln!("self-test : API, client web et inscription OK");
    Ok(())
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

/// Un client HTTP/1.1 minimal : une requête par connexion.
async fn http(addr: SocketAddr, method: &str, path: &str, body: Option<&str>) -> Result<(u16, String), String> {
    let exchange = async {
        let mut stream = tokio::net::TcpStream::connect(addr).await?;
        let body = body.unwrap_or("");
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nUser-Agent: kraken-self-test\r\nConnection: close\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
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
