//! Pont HTTP vers le vocal natif synthétique, pour les tests de bout en bout du client web
//! (Playwright), sans appli de bureau, sans Tauri et sans carte son.
//!
//! Il expose un `Controller::synthetic` (micro = sinusoïde, haut-parleurs = puits) sous les noms des
//! commandes Tauri de l'appli de bureau, avec les mêmes corps et les mêmes réponses :
//!
//! | Requête | Corps (JSON) | Réponse (JSON) |
//! |---|---|---|
//! | `POST /native_voice_available` | | `true` |
//! | `POST /native_voice_start` | `{"opts": {"mic_device", "speaker_device", "mic_gain", "output_volume", "self_mute", "self_deaf"}}` (tout est facultatif) | `{"sdp", "mids": {"mic", "screen", "screen_audio", "camera"}, "self_mute", "self_deaf"}` |
//! | `POST /native_voice_answer` | `{"sdp": "…"}` | `null` |
//! | `POST /native_voice_set` | `{"patch": {"self_mute", "self_deaf", "mic_gain", "output_volume"}}` | `null` |
//! | `POST /native_voice_poll` | | l'état (`state`, `participants`, `speaking`, niveaux…) ou `null` sans appel |
//! | `POST /native_voice_stop` | | `null` |
//! | `POST /native_voice_devices` | | `{"inputs": [...], "outputs": [...]}` |
//! | `GET /stats?freqs=440,880` | | ce que le puits a entendu : `samples`, `window`, `rms`, `tones: [{hz, share}]`, ou `null` sans appel |
//!
//! Les corps sont les arguments de l'`invoke` Tauri (une clé par paramètre de la commande), les
//! réponses ce qu'il rend. Une commande qui échoue répond 400 avec le message (une chaîne JSON, ce
//! que la promesse de `invoke` rejette). CORS permissif : `Access-Control-Allow-Origin` reprend
//! l'origine de la requête, le prévol `OPTIONS` aussi. C'est un outil de test : il n'écoute que
//! 127.0.0.1 et ne sert à rien d'autre.
//!
//! ```text
//! cargo build --release --example bridge -p kraken-voice-native --no-default-features
//! target/release/examples/bridge 8801            # port en argument, ou BRIDGE_PORT ; 0 : au choix du système
//! target/release/examples/bridge 8801 880        # fréquence du micro synthétique (Hz), ou BRIDGE_FREQ ; 440 sinon
//! ```
//!
//! `BRIDGE_IPS` (adresses IPv4 séparées par des virgules) remplace les adresses annoncées à WebRTC
//! (celles des interfaces réseau par défaut) : `127.0.0.1` pour un serveur sur la même machine.
//! Au démarrage, une ligne `bridge: http://127.0.0.1:<port> …` dit que c'est prêt.

use std::io::Read;
use std::net::Ipv4Addr;
use std::sync::Arc;

use kraken_voice_native::control::{Controller, StartOpts, StatePatch};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tiny_http::{Header, Method, Request, Response, Server};

/// Fréquences de `GET /stats` quand la requête n'en donne pas : celles des micros de 440 et 880 Hz.
const DEFAULT_FREQS: [f32; 2] = [440.0, 880.0];
/// Corps de requête accepté au plus : une réponse SDP fait quelques ko.
const MAX_BODY: u64 = 1 << 20;
/// Fils qui servent les requêtes : un `start` ou un `stop` lent ne bloque pas un `poll`.
const WORKERS: usize = 4;

fn main() {
    let arg = |n: usize| std::env::args().nth(n);
    let port: u16 = arg(1)
        .or_else(|| std::env::var("BRIDGE_PORT").ok())
        .map_or(8799, |p| p.parse().unwrap_or_else(|_| fail(&format!("port invalide : {p}"))));
    let freq: f32 = arg(2)
        .or_else(|| std::env::var("BRIDGE_FREQ").ok())
        .map_or(440.0, |f| f.parse().unwrap_or_else(|_| fail(&format!("fréquence invalide : {f}"))));
    let ips: Vec<Ipv4Addr> = std::env::var("BRIDGE_IPS")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().parse().unwrap_or_else(|_| fail(&format!("adresse IPv4 invalide : {s}"))))
        .collect();

    let server = Arc::new(Server::http(("127.0.0.1", port)).unwrap_or_else(|e| fail(&format!("port {port} : {e}"))));
    let addr = server.server_addr().to_ip().unwrap_or_else(|| fail("adresse d'écoute inconnue"));
    let controller = Arc::new(Controller::synthetic(freq).with_ips(ips));
    println!("bridge: http://{addr} (micro synthétique de {freq} Hz)");

    let workers: Vec<_> = (0..WORKERS)
        .map(|_| {
            let (server, controller) = (server.clone(), controller.clone());
            std::thread::spawn(move || {
                while let Ok(request) = server.recv() {
                    handle(&controller, request);
                }
            })
        })
        .collect();
    for w in workers {
        let _ = w.join();
    }
}

fn fail(message: &str) -> ! {
    eprintln!("bridge: {message}");
    std::process::exit(2);
}

fn handle(controller: &Controller, mut request: Request) {
    let origin = header(&request, "Origin");
    let url = request.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    if *request.method() == Method::Options {
        let mut response = Response::empty(204);
        cors(&mut response, origin.as_deref());
        if let Some(h) = header(&request, "Access-Control-Request-Headers") {
            add(&mut response, "Access-Control-Allow-Headers", &h);
        } else {
            add(&mut response, "Access-Control-Allow-Headers", "content-type");
        }
        add(&mut response, "Access-Control-Allow-Methods", "GET, POST, OPTIONS");
        add(&mut response, "Access-Control-Max-Age", "600");
        if header(&request, "Access-Control-Request-Private-Network").is_some() {
            add(&mut response, "Access-Control-Allow-Private-Network", "true");
        }
        let _ = request.respond(response);
        return;
    }
    let method = request.method().clone();
    let (status, body) = route(controller, &method, path, query, &mut request);
    let mut response = Response::from_string(body.to_string()).with_status_code(status);
    add(&mut response, "Content-Type", "application/json");
    add(&mut response, "Cache-Control", "no-store");
    cors(&mut response, origin.as_deref());
    let _ = request.respond(response);
}

fn header(request: &Request, name: &str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str().to_string())
}

fn add<R: Read>(response: &mut Response<R>, name: &str, value: &str) {
    if let Ok(h) = Header::from_bytes(name.as_bytes(), value.as_bytes()) {
        response.add_header(h);
    }
}

/// `Access-Control-Allow-Origin` reprend l'origine de la requête (sans origine, ce n'est pas une
/// requête CORS : rien à ajouter).
fn cors<R: Read>(response: &mut Response<R>, origin: Option<&str>) {
    if let Some(origin) = origin {
        add(response, "Access-Control-Allow-Origin", origin);
        add(response, "Vary", "Origin");
    }
}

/// Code HTTP et corps JSON de la réponse.
fn route(c: &Controller, method: &Method, path: &str, query: &str, request: &mut Request) -> (u16, Value) {
    let known = path == "/stats" || path.starts_with("/native_voice_");
    match (method, path) {
        (Method::Get, "/stats") => (200, to_json(&c.stats(&freqs(query)))),
        (Method::Post, p) if p.starts_with("/native_voice_") => {
            let mut raw = Vec::new();
            if request.as_reader().take(MAX_BODY).read_to_end(&mut raw).is_err() {
                return (400, json!("Corps de requête illisible."));
            }
            let body: Value = if raw.iter().all(u8::is_ascii_whitespace) {
                Value::Null
            } else {
                match serde_json::from_slice(&raw) {
                    Ok(v) => v,
                    Err(_) => return (400, json!("Corps JSON illisible.")),
                }
            };
            match command(c, &p[1..], &body) {
                Ok(Some(reply)) => (200, reply),
                Ok(None) => (404, json!("Commande inconnue.")),
                Err(message) => (400, json!(message)),
            }
        }
        _ if known => (405, json!("Méthode non permise.")),
        _ => (404, json!("Introuvable.")),
    }
}

/// Une commande Tauri : `Ok(None)` si elle n'existe pas.
fn command(c: &Controller, name: &str, body: &Value) -> Result<Option<Value>, String> {
    Ok(Some(match name {
        "native_voice_available" => json!(c.available()),
        "native_voice_start" => to_json(&c.start(arg::<StartOpts>(body, "opts")?)?),
        "native_voice_answer" => {
            let sdp = body.get("sdp").and_then(Value::as_str).ok_or("Argument « sdp » manquant.")?;
            c.answer(sdp)?;
            Value::Null
        }
        "native_voice_set" => {
            c.set(arg::<StatePatch>(body, "patch")?)?;
            Value::Null
        }
        "native_voice_poll" => to_json(&c.poll()),
        "native_voice_stop" => {
            c.stop();
            Value::Null
        }
        "native_voice_devices" => to_json(&c.devices()?),
        _ => return Ok(None),
    }))
}

/// L'argument `key` de la commande ; absent ou `null` : la valeur par défaut (comme un `Option` Tauri).
fn arg<T: DeserializeOwned + Default>(body: &Value, key: &str) -> Result<T, String> {
    match body.get(key) {
        None | Some(Value::Null) => Ok(T::default()),
        Some(v) => serde_json::from_value(v.clone()).map_err(|_| format!("Argument « {key} » invalide.")),
    }
}

fn to_json<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// `freqs=440,880` de la requête ; 440 et 880 Hz sans elle (au plus 16, valeurs positives).
fn freqs(query: &str) -> Vec<f32> {
    let list = query.split('&').find_map(|kv| kv.strip_prefix("freqs=")).map(|v| {
        v.split(',').filter_map(|f| f.parse::<f32>().ok()).filter(|f| f.is_finite() && *f > 0.0).take(16).collect()
    });
    list.filter(|l: &Vec<f32>| !l.is_empty()).unwrap_or_else(|| DEFAULT_FREQS.to_vec())
}
