//! Fichiers joints aux messages. Envoi en deux temps : `POST /channels/{id}/attachments` reçoit
//! le fichier (le corps de la requête est le fichier lui-même, son nom dans `?filename=`) et
//! rend son id ; le message le rattache ensuite (`CreateMessage::attachments`). Les fichiers
//! vivent dans `KRAKEN_DATA_DIR/attachments` sous une clé aléatoire (l'original, et pour une
//! image lisible un aperçu `.p` ré-encodé, donc sans EXIF ni GPS).
//!
//! Lecture : seulement pour qui voit le salon et en lit l'historique, avec la session. Une
//! balise `<img>` ou un lien du même site portent le cookie de session `HttpOnly` sans autre
//! mécanisme : pas d'URL signée à fabriquer, périmer ni laisser fuiter dans un historique de
//! navigation ou un journal. Toute autre demande (pas de session, pas de droit, mauvais
//! salon) ne livre rien : 401 sans session, 404 sinon (le salon et le fichier restent
//! inconnus).

use std::path::PathBuf;

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use futures_util::StreamExt;
use kraken_core::perms;
use kraken_proto::*;
use serde::Deserialize;
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;

use crate::AppState;
use crate::api::{channel_perms, require_channel};
use crate::auth::Auth;
use crate::error::{ApiResult, AppError};
use crate::media;

/// Envois par minute et par client.
pub const UPLOADS_PER_MIN: u32 = 120;
/// Envois en attente de message au plus, par personne (les autres sont refusés jusqu'au ménage).
const MAX_PENDING: i64 = 40;
/// Heures après lesquelles un envoi jamais rattaché est supprimé.
pub const ORPHAN_HOURS: i32 = 24;
/// Côté au plus de l'aperçu d'une image.
const PREVIEW_SIDE: u32 = 1600;

pub fn new_key() -> String {
    rand::random::<[u8; 16]>().iter().map(|b| format!("{b:02x}")).collect()
}

fn valid_key(key: &str) -> bool {
    key.len() == 32 && key.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn data(state: &AppState) -> PathBuf {
    PathBuf::from(&state.cfg.data_dir)
}

/// Supprime des fichiers du dossier de données (chemins relatifs rendus par `kraken-db`).
/// Seuls `attachments/…` et `embeds/…`, sans `..`, sont admis : un chemin qui sortirait de ces
/// dossiers est ignoré.
pub async fn remove_files(state: &AppState, paths: Vec<String>) {
    for p in paths {
        let ok = ["attachments/", "embeds/"].iter().any(|d| {
            p.strip_prefix(d).is_some_and(|name| {
                !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.') && !name.contains("..")
            })
        });
        if ok {
            tokio::fs::remove_file(data(state).join(&p)).await.ok();
        }
    }
}

fn too_large(max: u64) -> AppError {
    AppError(
        StatusCode::PAYLOAD_TOO_LARGE,
        "too_large",
        format!("Ce fichier pèse trop lourd : {} Mo au plus.", max.div_ceil(1024 * 1024)),
    )
}

fn internal() -> AppError {
    AppError(StatusCode::INTERNAL_SERVER_ERROR, "internal", "Erreur du serveur, réessaie.".into())
}

/// Nom de fichier sûr : sans dossier, sans caractère de contrôle, 255 caractères au plus.
fn clean_name(raw: &str) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let s: String = base
        .chars()
        .filter(|c| !c.is_control() && !"\u{202e}\u{202d}\u{2066}\u{2067}\u{2068}\u{2069}".contains(*c))
        .collect();
    let s = s.trim();
    let s: String = s.chars().take(255).collect();
    if s.is_empty() || s == "." || s == ".." { "fichier".into() } else { s }
}

/// Type annoncé par le client pour un fichier quelconque : jamais servi tel quel, gardé pour
/// l'affichage (icône). Un type mal formé devient `application/octet-stream`.
fn claimed_mime(headers: &HeaderMap) -> String {
    let raw = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("");
    let m = raw.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    let token = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$&-^_.+".contains(&b));
    match m.split_once('/') {
        // Un fichier qui n'a pas passé l'examen d'image ne se déclare pas image.
        Some(("image", _)) => "application/octet-stream".into(),
        Some((a, b)) if m.len() <= 100 && token(a) && token(b) => m,
        _ => "application/octet-stream".into(),
    }
}

#[derive(Deserialize)]
pub struct UploadQuery {
    filename: Option<String>,
}

/// Résultat de l'examen d'une image reçue.
struct Inspected {
    mime: &'static str,
    width: u32,
    height: u32,
    preview: Vec<u8>,
}

fn inspect(path: PathBuf) -> Option<Inspected> {
    let raw = std::fs::read(path).ok()?;
    let (_, mime) = media::safe_format(&raw)?;
    let img = media::decode(&raw)?;
    let enc = media::shrink(&img, PREVIEW_SIDE)?;
    Some(Inspected { mime, width: img.width(), height: img.height(), preview: enc.bytes })
}

pub async fn upload(
    State(state): State<AppState>,
    auth: Auth,
    Path(cid): Path<Id>,
    Query(q): Query<UploadQuery>,
    headers: HeaderMap,
    body: Body,
) -> ApiResult<(StatusCode, Json<Attachment>)> {
    let channel = require_channel(&state, auth.user.id, cid, perms::SEND_MESSAGES | perms::ATTACH_FILES).await?;
    if !channel.kind.is_textual() {
        return Err(AppError::not_found());
    }
    let max = state.cfg.max_upload_bytes;
    let declared =
        headers.get(header::CONTENT_LENGTH).and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
    if declared.is_some_and(|n| n > max) {
        return Err(too_large(max));
    }
    if state.db.pending_attachments(auth.user.id).await? >= MAX_PENDING {
        return Err(AppError(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_pending",
            "Trop de fichiers en attente : envoie un message ou retire-en avant d'en ajouter.".into(),
        ));
    }
    let filename = clean_name(q.filename.as_deref().unwrap_or(""));
    let key = new_key();
    let dir = data(&state).join("attachments");
    let file = dir.join(&key);
    // Le corps arrive en flux, écrit au fur et à mesure : jamais 25 Mio en mémoire par envoi.
    let written = async {
        tokio::fs::create_dir_all(&dir).await.map_err(|_| internal())?;
        let mut out = tokio::fs::File::create(&file).await.map_err(|_| internal())?;
        let mut size = 0u64;
        let mut stream = body.into_data_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| AppError::invalid("Envoi interrompu."))?;
            size += chunk.len() as u64;
            if size > max {
                return Err(too_large(max));
            }
            out.write_all(&chunk).await.map_err(|_| internal())?;
        }
        out.flush().await.map_err(|_| internal())?;
        Ok(size)
    };
    let size = match written.await {
        Ok(0) => {
            tokio::fs::remove_file(&file).await.ok();
            return Err(AppError::invalid("Ce fichier est vide."));
        }
        Ok(n) => n,
        Err(e) => {
            tokio::fs::remove_file(&file).await.ok();
            return Err(e);
        }
    };
    // Une image lisible garde ses dimensions et reçoit un aperçu propre ; tout le reste est
    // un fichier ordinaire.
    let image = tokio::task::spawn_blocking({
        let file = file.clone();
        move || inspect(file)
    })
    .await
    .ok()
    .flatten();
    let preview = image.is_some();
    if let Some(i) = &image {
        let wrote = tokio::fs::write(dir.join(format!("{key}.p")), &i.preview).await;
        if wrote.is_err() {
            tokio::fs::remove_file(&file).await.ok();
            return Err(internal());
        }
    }
    let mime = image.as_ref().map_or_else(|| claimed_mime(&headers), |i| i.mime.to_string());
    let dims = image.as_ref().map(|i| (i.width, i.height));
    let id = state
        .db
        .create_attachment(kraken_db::NewAttachment {
            channel_id: cid,
            uploader_id: auth.user.id,
            filename: &filename,
            mime: &mime,
            size,
            storage_key: &key,
            dims,
            preview,
        })
        .await;
    let id = match id {
        Ok(id) => id,
        Err(e) => {
            remove_files(&state, kraken_db::attachment_paths(&key, preview)).await;
            return Err(e.into());
        }
    };
    let a = Attachment { id, filename, mime, size, width: dims.map(|d| d.0), height: dims.map(|d| d.1), alt: None };
    Ok((StatusCode::CREATED, Json(a)))
}

#[derive(Deserialize)]
pub struct DownloadQuery {
    /// `1` : télécharger même une image que le navigateur afficherait.
    download: Option<String>,
}

/// L'attachement, si cette personne peut le lire dans ce salon.
async fn readable(state: &AppState, uid: Id, cid: Id, id: Id) -> ApiResult<kraken_db::StoredAttachment> {
    let p = state.db.channel_perms(uid, cid).await?;
    if p & (perms::VIEW_CHANNEL | perms::READ_HISTORY) != (perms::VIEW_CHANNEL | perms::READ_HISTORY) {
        return Err(AppError::not_found());
    }
    let a = state.db.attachment(id).await?.filter(|a| a.channel_id == cid).ok_or_else(AppError::not_found)?;
    // Pas encore dans un message : seul son auteur le voit.
    if a.message_id.is_none() && a.uploader_id != Some(uid) {
        return Err(AppError::not_found());
    }
    Ok(a)
}

/// Nom pour `Content-Disposition` : une version ASCII sûre, et la vraie en UTF-8 (RFC 6266).
fn disposition(kind: &str, filename: &str) -> String {
    let ascii: String = filename
        .chars()
        .map(|c| if c.is_ascii_graphic() || c == ' ' { c } else { '_' })
        .map(|c| if c == '"' || c == '\\' || c == '%' { '_' } else { c })
        .collect();
    let mut pct = String::new();
    for b in filename.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            pct.push(b as char);
        } else {
            pct.push_str(&format!("%{b:02X}"));
        }
    }
    format!("{kind}; filename=\"{ascii}\"; filename*=UTF-8''{pct}")
}

/// Types que le navigateur peut afficher sans danger dans la page : jamais de HTML ni de SVG.
fn inline_safe(mime: &str) -> bool {
    matches!(mime, "image/png" | "image/jpeg" | "image/webp" | "image/gif")
}

fn file_response(kind: &str, mime: &str, name: &str, len: Option<u64>, body: Body) -> ApiResult<Response> {
    let mut r = Response::new(body);
    let h = r.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_str(mime).map_err(|_| internal())?);
    h.insert(header::CONTENT_DISPOSITION, HeaderValue::from_str(&disposition(kind, name)).map_err(|_| internal())?);
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, max-age=3600"));
    if let Some(n) = len {
        h.insert(header::CONTENT_LENGTH, HeaderValue::from(n));
    }
    Ok(r)
}

/// L'original. Une image sûre s'affiche (`inline`) ; tout le reste se télécharge
/// (`attachment`, `application/octet-stream`) : un fichier HTML ou SVG envoyé ne s'exécute
/// jamais sur le site.
pub async fn get(
    State(state): State<AppState>,
    auth: Auth,
    Path((cid, id)): Path<(Id, Id)>,
    Query(q): Query<DownloadQuery>,
) -> ApiResult<Response> {
    let a = readable(&state, auth.user.id, cid, id).await?;
    if !valid_key(&a.storage_key) {
        return Err(AppError::not_found());
    }
    let file = tokio::fs::File::open(data(&state).join("attachments").join(&a.storage_key))
        .await
        .map_err(|_| AppError::not_found())?;
    let len = file.metadata().await.ok().map(|m| m.len());
    let show = a.preview && inline_safe(&a.mime) && q.download.as_deref() != Some("1");
    let (kind, mime) = if show { ("inline", a.mime.as_str()) } else { ("attachment", "application/octet-stream") };
    file_response(kind, mime, &a.filename, len, Body::from_stream(ReaderStream::new(file)))
}

/// Retirer de la saisie un fichier envoyé mais pas encore joint à un message.
pub async fn remove(
    State(state): State<AppState>,
    auth: Auth,
    Path((cid, id)): Path<(Id, Id)>,
) -> ApiResult<StatusCode> {
    let a = state.db.attachment(id).await?.filter(|a| a.channel_id == cid).ok_or_else(AppError::not_found)?;
    let paths = state.db.delete_pending_attachment(a.id, auth.user.id).await?.ok_or_else(AppError::not_found)?;
    remove_files(&state, paths).await;
    Ok(StatusCode::NO_CONTENT)
}

/// L'aperçu d'une image : ré-encodé par le serveur, sans métadonnées.
pub async fn preview(
    State(state): State<AppState>,
    auth: Auth,
    Path((cid, id)): Path<(Id, Id)>,
) -> ApiResult<Response> {
    let a = readable(&state, auth.user.id, cid, id).await?;
    if !a.preview || !valid_key(&a.storage_key) {
        return Err(AppError::not_found());
    }
    let bytes = tokio::fs::read(data(&state).join("attachments").join(format!("{}.p", a.storage_key)))
        .await
        .map_err(|_| AppError::not_found())?;
    let mime = match media::safe_format(&bytes) {
        Some((_, m)) if inline_safe(m) => m,
        _ => return Err(AppError::not_found()),
    };
    let len = bytes.len() as u64;
    file_response("inline", mime, &a.filename, Some(len), Body::from(bytes))
}

/// Image d'un aperçu de lien (`/channels/{id}/embeds/{clé}`), pour qui voit le salon.
pub async fn embed_image(
    State(state): State<AppState>,
    auth: Auth,
    Path((cid, key)): Path<(Id, String)>,
) -> ApiResult<Response> {
    let (stem, ext) = key.split_once('.').ok_or_else(AppError::not_found)?;
    let mime = match ext {
        "jpg" => "image/jpeg",
        "png" => "image/png",
        _ => return Err(AppError::not_found()),
    };
    if !valid_key(stem) {
        return Err(AppError::not_found());
    }
    channel_perms(&state, auth.user.id, cid, perms::READ_HISTORY).await.map_err(|_| AppError::not_found())?;
    if !state.db.embed_file_in_channel(cid, &key).await? {
        return Err(AppError::not_found());
    }
    let bytes = tokio::fs::read(data(&state).join("embeds").join(&key)).await.map_err(|_| AppError::not_found())?;
    let len = bytes.len() as u64;
    file_response("inline", mime, "apercu", Some(len), Body::from(bytes))
}

/// Ménage des envois jamais rattachés à un message.
pub async fn sweep(state: &AppState) {
    match state.db.purge_orphan_attachments(ORPHAN_HOURS).await {
        Ok(paths) => {
            if !paths.is_empty() {
                tracing::info!("{} fichier(s) joint(s) jamais envoyé(s) supprimé(s)", paths.len());
            }
            remove_files(state, paths).await;
        }
        Err(e) => tracing::error!("ménage des fichiers joints : {e:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("../../etc/passwd"), "passwd");
        assert_eq!(clean_name("C:\\Users\\moi\\photo.png"), "photo.png");
        assert_eq!(clean_name("  \u{0}a\nb.txt "), "ab.txt");
        assert_eq!(clean_name(""), "fichier");
        assert_eq!(clean_name(".."), "fichier");
        assert_eq!(clean_name(&"é".repeat(400)).chars().count(), 255);
        // Un nom ne peut pas se faire passer pour une autre extension par inversion de texte.
        assert!(!clean_name("photo\u{202e}gpj.exe").contains('\u{202e}'));
    }

    #[test]
    fn disposition_is_header_safe() {
        let d = disposition("attachment", "re\"çu 100%.pdf");
        assert_eq!(d, "attachment; filename=\"re__u 100_.pdf\"; filename*=UTF-8''re%22%C3%A7u%20100%25.pdf");
        assert!(HeaderValue::from_str(&d).is_ok());
        assert!(inline_safe("image/png") && !inline_safe("image/svg+xml") && !inline_safe("text/html"));
    }

    #[test]
    fn only_data_dirs_are_deleted() {
        // La liste blanche de `remove_files`, vérifiée sur des chemins pièges.
        let ok = |p: &str| {
            ["attachments/", "embeds/"].iter().any(|d| {
                p.strip_prefix(d).is_some_and(|n| {
                    !n.is_empty() && n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.') && !n.contains("..")
                })
            })
        };
        assert!(ok("attachments/0123456789abcdef0123456789abcdef.p") && ok("embeds/ab.jpg"));
        assert!(!ok("../etc/passwd") && !ok("attachments/../x") && !ok("attachments/a/b") && !ok("avatars/ab"));
    }
}
