//! Mode « héberger » : un serveur KRAKEN complet dans l'appli, avec son propre PostgreSQL.
//!
//! PostgreSQL (binaires embarqués par `build.rs`) n'écoute que sur 127.0.0.1 ; KRAKEN écoute
//! sur toutes les interfaces, pour que les personnes du même réseau puissent le rejoindre. Le
//! serveur a son propre runtime tokio : l'arrêter coupe aussi ses tâches de fond et les
//! WebSockets ouvertes. Les données (`<dossier>/pgdata`) restent d'un lancement à l'autre.

use std::collections::HashMap;
use std::fmt::Display;
use std::fs::{self, File};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use kraken_server::{AppState, Config};
use postgresql_commands::initdb::InitDbBuilder;
use postgresql_commands::{AsyncCommandExecutor, CommandBuilder};
use postgresql_embedded::{PostgreSQL, Settings, VersionReq};
use rand::distr::{Alphanumeric, SampleString};
use serde::Serialize;
use sqlx::Connection;
use tokio::sync::oneshot;
use url::{Host, Url};

const PG_VERSION: &str = env!("KRAKEN_PG_VERSION");
/// Archive zonky (`.jar`) pour la cible compilée ; vide si la cible n'en a pas.
static PG_ARCHIVE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/postgresql.jar"));
/// Windows : les DLL du runtime Visual C++, posées à côté des binaires de PostgreSQL (vide ailleurs).
static VC_RUNTIME: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/runtime.rs"));
const PG_USER: &str = postgresql_embedded::BOOTSTRAP_SUPERUSER;
const DB_NAME: &str = "kraken";
/// Port de KRAKEN au premier lancement, s'il est libre.
const DEFAULT_PORT: u16 = 8080;

/// Ce que la page locale affiche.
#[derive(Clone, Default, Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum Status {
    #[default]
    Idle,
    Starting {
        step: &'static str,
    },
    Ready {
        url: String,
        lan: bool,
        /// Aucun compte pour l'instant.
        first: bool,
        /// L'adresse n'est plus celle du lancement précédent : à redonner.
        fresh: bool,
        /// L'ordinateur a changé d'adresse depuis le démarrage : à redémarrer.
        stale: bool,
    },
    Failed {
        message: String,
        details: String,
    },
}

/// Un échec expliqué : `message` pour tout le monde, `details` pour dépanner.
#[derive(Debug)]
pub struct Failure {
    pub message: String,
    pub details: String,
}

impl Failure {
    fn new(message: &str, details: impl Display) -> Self {
        Self { message: message.into(), details: details.to_string() }
    }
}

trait OrFail<T> {
    fn or_fail(self, message: &str) -> Result<T, Failure>;
}

impl<T, E: Display> OrFail<T> for Result<T, E> {
    fn or_fail(self, message: &str) -> Result<T, Failure> {
        self.map_err(|e| Failure::new(message, e))
    }
}

/// Un serveur qui tourne. À arrêter avec [`Server::stop`] ; à défaut, PostgreSQL s'arrête
/// quand la valeur est détruite.
pub struct Server {
    rt: tokio::runtime::Runtime,
    pg: PostgreSQL,
    state: AppState,
    stop_http: oneshot::Sender<()>,
    http: tokio::task::JoinHandle<()>,
    /// Une seule appli à la fois sur ces données.
    _lock: File,
    /// L'adresse à partager : celle de cet ordinateur sur le réseau local.
    pub url: String,
    /// Faux si aucune adresse réseau n'a été trouvée (l'adresse est alors 127.0.0.1).
    pub lan: bool,
    /// Vrai si l'adresse a changé depuis le lancement précédent.
    pub fresh: bool,
    pub port: u16,
}

impl Server {
    /// Prépare (la première fois : extraction de PostgreSQL et création de la base) puis
    /// démarre tout. `port` : celui voulu pour KRAKEN s'il est libre, 0 pour n'importe lequel.
    pub fn start(data: &Path, web: &Path, port: u16, progress: &dyn Fn(&'static str)) -> Result<Self, Failure> {
        if PG_ARCHIVE.is_empty() {
            return Err(Failure::new("L'hébergement n'est pas disponible sur ce système.", std::env::consts::ARCH));
        }
        if !web.join("index.html").is_file() {
            return Err(Failure::new("Des fichiers de KRAKEN manquent : réinstalle l'appli.", web.display()));
        }
        fs::create_dir_all(data).or_fail("Impossible de créer le dossier du serveur.")?;
        let lock = File::create(data.join("lock")).or_fail("Impossible de créer le dossier du serveur.")?;
        if lock.try_lock().is_err() {
            return Err(Failure::new("Ce serveur tourne déjà dans une autre fenêtre de KRAKEN.", data.display()));
        }
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("kraken-serveur")
            .build()
            .or_fail("Impossible de démarrer le serveur.")?;
        let Running { pg, state, stop_http, http, port } = rt.block_on(prepare(data, web, port, progress))?;
        let url = state.cfg.public_url.clone();
        let lan = !url.starts_with("http://127.0.0.1:");
        let _ = fs::write(data.join("port"), port.to_string());
        // Seulement les adresses réseau : sans réseau, la prochaine fois sera sans doute comme avant.
        let previous = fs::read_to_string(data.join("address")).unwrap_or_default();
        let fresh = lan && !previous.is_empty() && previous != url;
        if lan {
            let _ = fs::write(data.join("address"), &url);
        }
        Ok(Self { rt, pg, state, stop_http, http, _lock: lock, url, lan, fresh, port })
    }

    /// Arrête KRAKEN puis PostgreSQL. Les données restent.
    pub fn stop(self) {
        let Self { rt, pg, state, stop_http, http, .. } = self;
        rt.block_on(async {
            let _ = stop_http.send(());
            let _ = tokio::time::timeout(Duration::from_secs(3), http).await;
            let _ = tokio::time::timeout(Duration::from_secs(2), state.db.pool.close()).await;
        });
        drop(state);
        // Coupe ce qui reste (tâches de fond, WebSockets) avant d'arrêter la base.
        rt.shutdown_timeout(Duration::from_secs(2));
        // Le Drop de PostgreSQL l'arrête (pg_ctl stop -m fast, avec attente), sans runtime.
        drop(pg);
    }

    /// Vrai tant que personne n'a créé de compte.
    pub fn first(&self) -> bool {
        let pool = &self.state.db.pool;
        let query = async {
            let q = sqlx::query_scalar::<_, bool>("select not exists (select 1 from users)").fetch_one(pool);
            matches!(tokio::time::timeout(Duration::from_secs(2), q).await, Ok(Ok(true)))
        };
        // Sur un fil à part : `block_on` refuse de tourner depuis un autre runtime.
        thread::scope(|s| s.spawn(|| self.rt.block_on(query)).join().unwrap_or(false))
    }

    /// Vrai si l'ordinateur a maintenant une autre adresse sur le réseau (nouveau Wi-Fi, réseau
    /// revenu après le démarrage…) : l'adresse partagée ne mène plus ici.
    pub fn stale(&self) -> bool {
        lan_ip().is_some_and(|ip| !self.url.starts_with(&format!("http://{ip}:")))
    }

    /// Port de PostgreSQL (sur 127.0.0.1 seulement).
    pub fn pg_port(&self) -> u16 {
        self.pg.settings().port
    }

    pub fn block_on<F: Future>(&self, f: F) -> F::Output {
        self.rt.block_on(f)
    }
}

struct Running {
    pg: PostgreSQL,
    state: AppState,
    stop_http: oneshot::Sender<()>,
    http: tokio::task::JoinHandle<()>,
    port: u16,
}

/// PostgreSQL démarré, base à jour, KRAKEN en écoute.
async fn prepare(data: &Path, web: &Path, port: u16, progress: &dyn Fn(&'static str)) -> Result<Running, Failure> {
    let install = install(data, progress).await?;
    let settings = settings(data, install)?;
    let pgdata = settings.data_dir.clone();
    if !pgdata.join("PG_VERSION").is_file() {
        progress("Création de la base de données…");
        initdb(&settings).await?;
    }
    let major = fs::read_to_string(pgdata.join("PG_VERSION")).unwrap_or_default();
    if Some(major.trim()) != PG_VERSION.split('.').next() {
        return Err(Failure::new(
            "Tes données viennent d'une autre version de PostgreSQL : cette version de KRAKEN ne sait pas les ouvrir.",
            format!("données : PostgreSQL {}, appli : {PG_VERSION}", major.trim()),
        ));
    }
    progress("Démarrage de la base de données…");
    recover(&settings).await?;
    let pg = start_pg(settings).await?;
    let exists = pg.database_exists(DB_NAME).await.or_fail("La base de données ne répond pas.")?;
    if !exists {
        pg.create_database(DB_NAME).await.or_fail("La base de données n'a pas pu être créée.")?;
    }

    progress("Mise à jour de la base de données…");
    let listener = listen(port).or_fail("Aucun port réseau n'est libre pour KRAKEN.")?;
    let port = listener.local_addr().or_fail("Aucun port réseau n'est libre pour KRAKEN.")?.port();
    let listener = tokio::net::TcpListener::from_std(listener).or_fail("Aucun port réseau n'est libre pour KRAKEN.")?;
    // Les liens d'invitation reprennent l'adresse de la fenêtre : celle du réseau local.
    let ip = lan_ip().unwrap_or(Ipv4Addr::LOCALHOST);
    let mut cfg = Config::from_env();
    cfg.addr = format!("0.0.0.0:{port}");
    cfg.public_url = format!("http://{ip}:{port}");
    cfg.web_dir = web.to_string_lossy().into_owned();
    cfg.database_url = pg.settings().url(DB_NAME);
    cfg.seed = false;
    cfg.trust_proxy = false;
    let state = AppState::new(cfg).await.or_fail("La base de données n'a pas pu être mise à jour.")?;
    progress("Démarrage de KRAKEN…");
    kraken_server::spawn_workers(&state);
    let app = kraken_server::app(state.clone()).into_make_service_with_connect_info::<SocketAddr>();
    let (stop_http, stopped) = oneshot::channel::<()>();
    let http = tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await;
    });
    Ok(Running { pg, state, stop_http, http, port })
}

/// Binaires de PostgreSQL, extraits une fois pour toutes (et une fois par version).
async fn install(data: &Path, progress: &dyn Fn(&'static str)) -> Result<PathBuf, Failure> {
    let root = data.join("postgresql");
    let dir = root.join(PG_VERSION);
    if !dir.join("bin").join(exe("pg_ctl")).is_file() {
        progress("Préparation de PostgreSQL…");
        // Reste d'une extraction interrompue : l'extraction passe par un dossier temporaire.
        let _ = fs::remove_dir_all(&dir);
        let archive = PG_ARCHIVE.to_vec();
        postgresql_archive::extract(postgresql_archive::configuration::zonky::URL, &archive, &dir)
            .await
            .or_fail("PostgreSQL n'a pas pu être installé.")?;
    }
    for (name, bytes) in VC_RUNTIME {
        let path = dir.join("bin").join(name);
        if fs::metadata(&path).map(|m| m.len()).ok() != Some(bytes.len() as u64) {
            fs::write(&path, bytes).or_fail("PostgreSQL n'a pas pu être installé.")?;
        }
    }
    // Les anciennes versions mineures ne servent plus (même format de données).
    let major = PG_VERSION.split('.').next().unwrap_or_default();
    for entry in fs::read_dir(&root).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name != PG_VERSION && name.split('.').next() == Some(major) && name.split('.').count() == 3 {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
    Ok(dir)
}

fn exe(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

fn settings(data: &Path, install: PathBuf) -> Result<Settings, Failure> {
    let password_file = data.join("pgpass");
    let password = password(&password_file).or_fail("Impossible de créer le mot de passe de la base.")?;
    let mut configuration = HashMap::from([("listen_addresses".to_string(), "127.0.0.1".to_string())]);
    // Pas de socket Unix : seulement TCP, sur 127.0.0.1.
    if cfg!(unix) {
        configuration.insert("unix_socket_directories".into(), "''".into());
    }
    Ok(Settings {
        releases_url: postgresql_archive::configuration::zonky::URL.into(),
        version: VersionReq::parse(&format!("={PG_VERSION}")).or_fail("Version de PostgreSQL invalide.")?,
        installation_dir: install,
        password_file,
        data_dir: data.join("pgdata"),
        host: "127.0.0.1".into(),
        port: 0,
        username: PG_USER.into(),
        password,
        temporary: false,
        // initdb peut être long sur un disque lent.
        timeout: Some(Duration::from_secs(180)),
        configuration,
        trust_installation_dir: true,
        socket_dir: None,
    })
}

/// Mot de passe de la base : tiré au hasard la première fois, lisible par l'utilisateur seul.
fn password(path: &Path) -> std::io::Result<String> {
    if let Ok(p) = fs::read_to_string(path)
        && !p.trim().is_empty()
    {
        return Ok(p.trim().to_string());
    }
    let p = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    // Windows : le dossier de données local de l'utilisateur n'est lisible que par lui.
    std::io::Write::write_all(&mut options.open(path)?, p.as_bytes())?;
    Ok(p)
}

/// initdb dans un dossier à part, renommé une fois fini : une création interrompue ne
/// laisse pas de base à moitié faite.
async fn initdb(settings: &Settings) -> Result<(), Failure> {
    let target = &settings.data_dir;
    let tmp = target.with_file_name("pgdata.init");
    let _ = fs::remove_dir_all(&tmp);
    let result = InitDbBuilder::from(settings)
        .pgdata(&tmp)
        .username(PG_USER)
        .auth("scram-sha-256")
        .pwfile(&settings.password_file)
        .encoding("UTF8")
        // Locale « C » : ne dépend pas des langues installées sur l'ordinateur.
        .no_locale()
        .build_tokio()
        .execute(settings.timeout)
        .await;
    if let Err(e) = result {
        let _ = fs::remove_dir_all(&tmp);
        let e = e.to_string();
        let message = if e.contains("cannot be run as root") {
            "KRAKEN ne peut pas héberger de serveur quand il est lancé en administrateur (root)."
        } else {
            "La base de données n'a pas pu être créée."
        };
        return Err(Failure::new(message, e));
    }
    // Un dossier vide peut rester d'un essai précédent ; un dossier plein n'est jamais écrasé.
    let _ = fs::remove_dir(target);
    fs::rename(&tmp, target).or_fail("La base de données n'a pas pu être créée.")
}

/// Après un plantage, PostgreSQL peut encore tourner (pg_ctl le détache de l'appli) ou avoir
/// laissé son `postmaster.pid`. Un serveur à nous est arrêté proprement ; un fichier périmé
/// (son processus est mort, ou son numéro a été repris par un autre) est effacé, sauf si
/// quelque chose écoute encore sur son port.
async fn recover(settings: &Settings) -> Result<(), Failure> {
    let pid_file = settings.data_dir.join("postmaster.pid");
    let Ok(text) = fs::read_to_string(&pid_file) else { return Ok(()) };
    // 4e ligne : le port du serveur.
    let port = text.lines().nth(3).and_then(|l| l.trim().parse::<u16>().ok());
    if let Some(port) = port
        && answers(settings, port).await
    {
        let mut old = settings.clone();
        old.port = port;
        let _ = PostgreSQL::new(old).stop().await;
        if pid_file.exists() {
            return Err(Failure::new(
                "Une ancienne copie du serveur tourne encore : redémarre l'ordinateur, puis relance KRAKEN.",
                pid_file.display(),
            ));
        }
        return Ok(());
    }
    // Un serveur qui répond sans connaître notre mot de passe : on n'efface rien.
    if let Some(port) = port
        && listening(port).await
    {
        return Err(Failure::new(
            "Une ancienne copie de la base de données tourne peut-être encore : redémarre l'ordinateur, puis relance KRAKEN.",
            format!("{} (port {port})", pid_file.display()),
        ));
    }
    let _ = fs::remove_file(&pid_file);
    Ok(())
}

async fn listening(port: u16) -> bool {
    let connect = tokio::net::TcpStream::connect((Ipv4Addr::LOCALHOST, port));
    matches!(tokio::time::timeout(Duration::from_secs(2), connect).await, Ok(Ok(_)))
}

/// Vrai si un PostgreSQL à nous (il connaît notre mot de passe) répond sur ce port.
async fn answers(settings: &Settings, port: u16) -> bool {
    let options = sqlx::postgres::PgConnectOptions::new()
        .host("127.0.0.1")
        .port(port)
        .username(PG_USER)
        .password(&settings.password)
        .database("postgres");
    match tokio::time::timeout(Duration::from_secs(5), sqlx::PgConnection::connect_with(&options)).await {
        Ok(Ok(c)) => {
            let _ = c.close().await;
            true
        }
        // En train de démarrer ou de s'arrêter : c'est bien un serveur en route.
        Ok(Err(e)) => e.as_database_error().and_then(|d| d.code()).is_some_and(|c| c == "57P03"),
        Err(_) => false,
    }
}

/// Démarre PostgreSQL sur un port libre de 127.0.0.1 (nouvel essai si un autre programme le
/// prend entre-temps).
async fn start_pg(mut settings: Settings) -> Result<PostgreSQL, Failure> {
    let log = settings.data_dir.join("start.log");
    // Le journal de PostgreSQL s'allonge à chaque lancement : on repart de zéro s'il est gros.
    if fs::metadata(&log).is_ok_and(|m| m.len() > 10 << 20) {
        let _ = fs::remove_file(&log);
    }
    let mut last = String::new();
    for _ in 0..3 {
        settings.port = free_port(Ipv4Addr::LOCALHOST).or_fail("Aucun port n'est libre pour la base de données.")?;
        let mut pg = PostgreSQL::new(settings.clone());
        match pg.start().await {
            Ok(()) => return Ok(pg),
            Err(e) => last = e.to_string(),
        }
    }
    // Les dernières lignes du journal, dans l'ordre.
    let log = fs::read_to_string(&log).unwrap_or_default();
    let lines: Vec<&str> = log.lines().collect();
    let tail = lines[lines.len().saturating_sub(8)..].join("\n");
    Err(Failure::new("La base de données ne démarre pas.", format!("{last}\n{tail}")))
}

fn free_port(ip: Ipv4Addr) -> std::io::Result<u16> {
    Ok(TcpListener::bind((ip, 0))?.local_addr()?.port())
}

/// Le port voulu s'il est libre, sinon un autre. Il doit l'être aussi sur 127.0.0.1, qu'un
/// autre programme peut occuper seul.
fn listen(port: u16) -> std::io::Result<TcpListener> {
    let listener = if port != 0
        && TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_ok()
        && let Ok(l) = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))
    {
        l
    } else {
        TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0))?
    };
    listener.set_nonblocking(true)?;
    Ok(listener)
}

/// L'adresse IPv4 de cet ordinateur sur son réseau (celle de la route par défaut). Aucun
/// paquet n'est envoyé : `connect` sur de l'UDP choisit seulement l'interface.
fn lan_ip() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() && !ip.is_link_local() => Some(ip),
        _ => None,
    }
}

/// Le serveur de l'appli, piloté depuis la page locale : un démarrage en arrière-plan à la
/// fois, un arrêt qui ne bloque pas la fenêtre.
#[derive(Default)]
pub struct Hosting {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    status: Status,
    server: Option<Server>,
    /// Numéro du dernier démarrage demandé : un démarrage annulé entre-temps s'arrête de lui-même.
    run: u64,
    starting: Option<JoinHandle<()>>,
    stopping: Option<JoinHandle<()>>,
}

impl Hosting {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn status(&self) -> Status {
        let mut inner = self.lock();
        let inner = &mut *inner;
        if let (Status::Ready { first, stale, .. }, Some(server)) = (&mut inner.status, &inner.server) {
            // Le premier compte peut avoir été créé depuis (ça ne va que dans ce sens).
            *first = *first && server.first();
            *stale = server.stale();
        }
        inner.status.clone()
    }

    /// Démarre en arrière-plan, sauf si c'est déjà fait ou en cours. Si l'ordinateur a changé
    /// d'adresse, redémarre pour partager la bonne.
    pub fn start(&self, data: PathBuf, web: PathBuf) {
        let mut inner = self.lock();
        if matches!(inner.status, Status::Starting { .. }) || inner.server.as_ref().is_some_and(|s| !s.stale()) {
            return;
        }
        inner.halt();
        let run = inner.run;
        inner.status = Status::Starting { step: "Préparation…" };
        // Un démarrage annulé ou un arrêt en cours doit d'abord avoir libéré la base.
        let before = [inner.starting.take(), inner.stopping.take()];
        let shared = self.inner.clone();
        inner.starting = Some(thread::spawn(move || {
            for h in before.into_iter().flatten() {
                let _ = h.join();
            }
            let lock = || shared.lock().unwrap_or_else(|e| e.into_inner());
            let port = fs::read_to_string(data.join("port")).ok().and_then(|p| p.trim().parse().ok());
            let result = Server::start(&data, &web, port.unwrap_or(DEFAULT_PORT), &|step| {
                let mut inner = lock();
                if inner.run == run {
                    inner.status = Status::Starting { step };
                }
            });
            let first = result.as_ref().is_ok_and(Server::first);
            let mut inner = lock();
            match result {
                Ok(server) if inner.run == run => {
                    let (url, lan, fresh) = (server.url.clone(), server.lan, server.fresh);
                    inner.status = Status::Ready { url, lan, first, fresh, stale: false };
                    inner.server = Some(server);
                }
                // Arrêt demandé pendant le démarrage.
                Ok(server) => {
                    drop(inner);
                    server.stop();
                }
                Err(f) if inner.run == run => inner.status = Status::Failed { message: f.message, details: f.details },
                Err(_) => {}
            }
        }));
    }

    /// Arrête le serveur sans attendre ; les données restent.
    pub fn stop(&self) {
        self.lock().halt();
    }

    /// Arrête et attend que tout soit fini : à la fermeture de l'appli.
    pub fn stop_and_wait(&self) {
        self.stop();
        let (starting, stopping) = {
            let mut inner = self.lock();
            (inner.starting.take(), inner.stopping.take())
        };
        // Un démarrage en cours finit, voit qu'il est annulé et s'arrête.
        for h in [starting, stopping].into_iter().flatten() {
            let _ = h.join();
        }
    }

    /// L'adresse à ouvrir dans la fenêtre : l'inscription tant qu'il n'y a aucun compte.
    pub fn open_url(&self) -> Option<String> {
        let inner = self.lock();
        let server = inner.server.as_ref()?;
        Some(format!("{}{}", server.url, if server.first() { "/register" } else { "/" }))
    }

    /// Si `link` mène au serveur hébergé ici (par son adresse réseau ou par 127.0.0.1), le même
    /// lien sur son adresse réseau, celle que reprennent les liens d'invitation.
    pub fn own_link(&self, link: &Url) -> Option<Url> {
        let inner = self.lock();
        let server = inner.server.as_ref()?;
        let mut own = Url::parse(&server.url).ok()?;
        let here = match link.host()? {
            Host::Ipv4(ip) => ip.is_loopback() || own.host() == Some(Host::Ipv4(ip)),
            Host::Ipv6(ip) => ip.is_loopback(),
            Host::Domain(d) => d == "localhost",
        };
        if !here || link.port_or_known_default() != Some(server.port) {
            return None;
        }
        own.set_path(link.path());
        own.set_query(link.query());
        own.set_fragment(link.fragment());
        Some(own)
    }
}

impl Inner {
    /// Arrête le serveur en arrière-plan et annule un démarrage en cours.
    fn halt(&mut self) {
        self.run += 1;
        self.status = Status::Idle;
        if let Some(server) = self.server.take() {
            let previous = self.stopping.take();
            self.stopping = Some(thread::spawn(move || {
                if let Some(h) = previous {
                    let _ = h.join();
                }
                server.stop();
            }));
        }
    }
}
