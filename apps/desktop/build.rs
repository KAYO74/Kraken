//! Compile les pages locales (Tauri) et embarque PostgreSQL pour le mode « héberger ».
//!
//! L'archive vient des binaires zonky (Maven Central) : autonomes, leurs bibliothèques sont
//! incluses (Linux : seulement la glibc ; macOS : binaire universel, macOS 13 et plus). Elle est
//! choisie pour la cible compilée, pas pour la machine qui compile : le build universel macOS
//! compile x86_64 sur une machine arm64. `KRAKEN_PG_ARCHIVE` donne un fichier déjà téléchargé
//! (build hors ligne).
//!
//! Sous Windows, ces binaires ont aussi besoin du runtime Visual C++, absent de bien des PC : ses
//! DLL (redistribuables) sont embarquées avec, depuis les outils de build Visual Studio ou le
//! dossier donné par `KRAKEN_VC_RUNTIME_DIR` (`…\x64\Microsoft.VC143.CRT`).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

/// Changer de version demande de changer les empreintes ci-dessous.
const PG_VERSION: &str = "18.4.0";

/// Cible Rust, plateforme zonky, SHA-256 de l'archive.
const ARCHIVES: &[(&str, &str, &str)] = &[
    ("x86_64-unknown-linux-gnu", "linux-amd64", "401d4e69baba9072d3772607710df0d47b39a7632c6b01b2a5f5d85a9514e212"),
    ("aarch64-unknown-linux-gnu", "linux-arm64v8", "a9ec284923b9a7d2db41509bfb88c3d0eeb9f4aa5e2b29ea0aac5e8dbaf4c335"),
    ("x86_64-pc-windows-msvc", "windows-amd64", "1f71b4d67eeee94034ffbea7a9fae68ab34ff1e31ac01837acb6f56984350ece"),
    ("x86_64-apple-darwin", "darwin-amd64", "68381ed1488edd337345c6c4bc6ae0c823832a4dd7af8b04656ee5c218c291b5"),
    ("aarch64-apple-darwin", "darwin-arm64v8", "ab698c4486a795d2aa8a158b3a41e1201a77e7ff72a6255094cd5900080853c2"),
];

/// DLL du runtime Visual C++ que demandent initdb, pg_ctl, postgres et ICU.
const VC_RUNTIME: &[&str] = &["vcruntime140.dll", "vcruntime140_1.dll", "msvcp140.dll"];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=KRAKEN_PG_ARCHIVE");
    println!("cargo:rerun-if-env-changed=KRAKEN_VC_RUNTIME_DIR");
    embed_postgresql();
    embed_vc_runtime();
    // Les commandes de l'appli passent par l'ACL : seule la page locale a droit aux quatre
    // premières (capabilities/default.json) ; celles du contrôle à distance, seulement la page de
    // l'instance (capacité ajoutée au lancement, `remote.rs`).
    let manifest = tauri_build::AppManifest::new().commands(&[
        "join",
        "host_start",
        "host_status",
        "host_open",
        "remote_begin",
        "remote_input",
        "remote_end",
    ]);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest)).expect("tauri-build");
}

fn embed_postgresql() {
    let target = env::var("TARGET").expect("TARGET");
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("postgresql.jar");
    println!("cargo:rustc-env=KRAKEN_PG_VERSION={PG_VERSION}");
    let Some(&(_, platform, sha)) = ARCHIVES.iter().find(|(t, _, _)| *t == target) else {
        // Cible sans binaires : l'appli se construit, sans le mode « héberger ».
        println!("cargo:warning=pas de PostgreSQL embarqué pour {target} : le mode « héberger » sera indisponible");
        fs::write(&out, b"").expect("OUT_DIR");
        return;
    };
    if digest(&out).as_deref() == Some(sha) {
        return;
    }
    let part = out.with_extension("part");
    if let Some(local) = env::var_os("KRAKEN_PG_ARCHIVE") {
        if let Err(e) = fs::copy(&local, &part) {
            panic!("KRAKEN_PG_ARCHIVE ({}) : {e}", Path::new(&local).display());
        }
    } else {
        let name = format!("embedded-postgres-binaries-{platform}-{PG_VERSION}.jar");
        let url = format!(
            "https://repo1.maven.org/maven2/io/zonky/test/postgres/embedded-postgres-binaries-{platform}/{PG_VERSION}/{name}"
        );
        // curl est présent sur les trois systèmes (Windows 10 et plus compris).
        let ok = Command::new("curl")
            .args(["-fsSL", "--retry", "5", "--retry-delay", "10", "-o"])
            .arg(&part)
            .arg(&url)
            .status()
            .is_ok_and(|s| s.success());
        assert!(
            ok,
            "téléchargement de PostgreSQL impossible ({url}) ; hors ligne, donne le fichier avec KRAKEN_PG_ARCHIVE"
        );
    }
    let got = digest(&part).unwrap_or_default();
    assert!(got == sha, "archive PostgreSQL altérée : SHA-256 {got}, attendu {sha}");
    fs::rename(&part, &out).expect("OUT_DIR");
}

/// Écrit `runtime.rs` : la liste (nom, contenu) des DLL à poser à côté de PostgreSQL, vide hors
/// de Windows.
fn embed_vc_runtime() {
    let target = env::var("TARGET").expect("TARGET");
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let mut list = String::from("&[");
    if target == "x86_64-pc-windows-msvc" {
        let dir = vc_runtime_dir().unwrap_or_else(|| {
            panic!(
                "runtime Visual C++ introuvable : installe les outils de build Visual Studio (C++), ou donne le dossier \
                 x64\\Microsoft.VC14x.CRT avec KRAKEN_VC_RUNTIME_DIR"
            )
        });
        for name in VC_RUNTIME {
            let (from, to) = (dir.join(name), out.join(name));
            if let Err(e) = fs::copy(&from, &to) {
                panic!("{} : {e}", from.display());
            }
            list += &format!("({name:?}, include_bytes!({:?})),", to.display().to_string());
        }
    }
    list += "]";
    fs::write(out.join("runtime.rs"), list).expect("OUT_DIR");
}

fn vc_runtime_dir() -> Option<PathBuf> {
    if let Some(dir) = env::var_os("KRAKEN_VC_RUNTIME_DIR") {
        return Some(dir.into());
    }
    // Invite de commandes Visual Studio : VCToolsRedistDir ; sinon, vswhere trouve l'installation.
    let versions = match env::var_os("VCToolsRedistDir") {
        Some(dir) => vec![PathBuf::from(dir)],
        None => {
            let vswhere =
                PathBuf::from(env::var_os("ProgramFiles(x86)")?).join(r"Microsoft Visual Studio\Installer\vswhere.exe");
            let found = Command::new(vswhere)
                .args(["-latest", "-products", "*", "-utf8", "-property", "installationPath"])
                .args(["-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64"])
                .output()
                .ok()?;
            let install = PathBuf::from(String::from_utf8_lossy(&found.stdout).trim());
            // VC\Redist\MSVC\<version>, la plus récente d'abord.
            let mut versions: Vec<PathBuf> = fs::read_dir(install.join(r"VC\Redist\MSVC"))
                .ok()?
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(|c: char| c.is_ascii_digit()))
                })
                .collect();
            versions.sort();
            versions.reverse();
            versions
        }
    };
    versions.iter().find_map(|v| {
        fs::read_dir(v.join("x64")).ok()?.flatten().map(|e| e.path()).find(|p| {
            let name = p.file_name().unwrap_or_default().to_string_lossy();
            name.starts_with("Microsoft.VC") && name.ends_with(".CRT") && VC_RUNTIME.iter().all(|f| p.join(f).is_file())
        })
    })
}

fn digest(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    Some(Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect())
}
