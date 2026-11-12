//! Emojis perso d'un serveur. Le corps de `POST /guilds/{id}/emojis?name=…` est l'image elle-même
//! (comme pour une photo de profil). Elle est reconnue à ses premiers octets, jamais à son
//! extension ni au type annoncé, puis gardée sur disque sous une clé aléatoire dans
//! `KRAKEN_DATA_DIR/emojis` (`clé.png`, `clé.gif` ou `clé.webp`, nom repris dans `emojis.image_key`).
//! L'image ne change jamais (renommer ne la touche pas) : elle est servie avec un cache sans fin.
//!
//! * 256 Kio au plus (413 `too_large`), PNG, JPEG, WebP ou GIF (400 `not_image`).
//! * Image fixe : même garde que les photos de profil (4096 pixels de côté, 16 millions de pixels,
//!   jugés sur l'en-tête, 2 décodages à la fois sur tout le serveur), recadrée au centre en carré,
//!   réduite à 128 × 128 (jamais agrandie) et ré-encodée en PNG, ce qui efface EXIF et métadonnées.
//! * GIF ou WebP **animé** : gardé tel quel (sans ses métadonnées), après la même vérification que
//!   pour une photo animée ; son rapport largeur / hauteur ne dépasse pas 2 pour 1 (400
//!   `bad_ratio`) puisqu'il n'est pas recadré.
//!
//! Lecture : `GET /emojis/{id}/image`, avec une session (une balise `<img>` porte le cookie). Pas
//! besoin d'être membre du serveur : un emoji s'utilise partout, et un message qui le cite doit
//! s'afficher chez tous ceux qui le lisent. L'id est un UUID, qui ne se devine pas.

use std::io::Cursor;
use std::path::PathBuf;

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::IntoResponse;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat};
use kraken_core::image::{ImageKind, sniff};
use kraken_proto::*;
use serde::Deserialize;

use crate::AppState;
use crate::auth::Auth;
use crate::avatar::{self, Refused, STILL, Stored};
use crate::error::{ApiResult, AppError};
use crate::media::{self, Refusal, gated};

/// Ajouts, renommages et retraits par minute et par client (un ajout coûte un décodage d'image).
pub const WRITES_PER_MIN: u32 = 60;

fn dir(state: &AppState) -> PathBuf {
    PathBuf::from(&state.cfg.data_dir).join("emojis")
}

/// `clé.ext` : le nom d'un fichier que ce module a écrit. Rend le type de l'image.
fn file_kind(image_key: &str) -> Option<(&str, &'static str)> {
    let (key, ext) = image_key.split_once('.')?;
    let mime = match ext {
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => return None,
    };
    avatar::valid_key(key).then_some((image_key, mime))
}

fn too_large() -> AppError {
    AppError(
        StatusCode::PAYLOAD_TOO_LARGE,
        "too_large",
        format!("Cette image pèse trop lourd : {} Ko au plus.", MAX_EMOJI_BYTES / 1024),
    )
}

fn bad_ratio() -> AppError {
    AppError(
        StatusCode::BAD_REQUEST,
        "bad_ratio",
        format!(
            "Cette animation est trop allongée : elle doit rester presque carrée ({MAX_EMOJI_RATIO} pour 1 au plus)."
        ),
    )
}

fn internal() -> AppError {
    AppError(StatusCode::INTERNAL_SERVER_ERROR, "internal", "Erreur du serveur, réessaie.".into())
}

enum Bad {
    Image(Refused),
    Ratio,
}

impl From<Refused> for Bad {
    fn from(r: Refused) -> Self {
        Self::Image(r)
    }
}

impl From<Bad> for AppError {
    fn from(b: Bad) -> Self {
        match b {
            Bad::Image(Refused::NotImage) => avatar::not_image(),
            Bad::Image(Refused::BigAnimation) => avatar::big_animation(),
            Bad::Image(Refused::BigImage) => avatar::big_image(),
            Bad::Ratio => bad_ratio(),
        }
    }
}

/// Image fixe : recadrée au centre en carré, réduite à [`EMOJI_SIZE`] (jamais agrandie), en PNG.
fn normalize_still(raw: &[u8]) -> Result<Stored, Refused> {
    let img = media::decode_within(raw, STILL).map_err(|e| match e {
        Refusal::NotImage => Refused::NotImage,
        Refusal::TooBig => Refused::BigImage,
    })?;
    let side = img.width().min(img.height());
    if side == 0 {
        return Err(Refused::NotImage);
    }
    let square = img.crop_imm((img.width() - side) / 2, (img.height() - side) / 2, side, side);
    let out_side = side.min(EMOJI_SIZE);
    let small = if out_side < side { square.resize_exact(out_side, out_side, FilterType::Lanczos3) } else { square };
    let mut bytes = Vec::new();
    DynamicImage::ImageRgba8(small.to_rgba8())
        .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .map_err(|_| Refused::NotImage)?;
    Ok(Stored { bytes, ext: "png" })
}

/// Le fichier à garder, et s'il est animé.
fn normalize(raw: &[u8]) -> Result<(Stored, bool), Bad> {
    let found = sniff(raw).ok_or(Refused::NotImage)?;
    if found.animated && matches!(found.kind, ImageKind::Gif | ImageKind::Webp) {
        let stored = avatar::normalize_animated(raw, found.kind)?;
        let (w, h) = media::header_size(Cursor::new(raw)).ok_or(Refused::NotImage)?;
        if w.max(h) > MAX_EMOJI_RATIO * w.min(h) {
            return Err(Bad::Ratio);
        }
        Ok((stored, true))
    } else {
        Ok((normalize_still(raw)?, false))
    }
}

/// Dit aux membres du serveur quels emojis il a maintenant.
async fn announce(state: &AppState, gid: Id) -> ApiResult<Vec<Emoji>> {
    let emojis = state.db.guild_emojis(gid).await?;
    let audience = state.db.member_ids(gid).await?;
    state.hub.send(&audience, Event::EmojisUpdate { guild_id: gid, emojis: emojis.clone() });
    Ok(emojis)
}

/// `GET /guilds/{id}/emojis` : les emojis du serveur, pour ses membres.
pub async fn list(State(state): State<AppState>, auth: Auth, Path(gid): Path<Id>) -> ApiResult<Json<Vec<Emoji>>> {
    Ok(Json(state.db.member_guild_emojis(gid, auth.user.id).await?))
}

/// `GET /users/@me/emojis` : tous ceux que je peux utiliser, c'est-à-dire ceux des serveurs dont je suis membre.
pub async fn mine(State(state): State<AppState>, auth: Auth) -> ApiResult<Json<Vec<Emoji>>> {
    Ok(Json(state.db.user_emojis(auth.user.id).await?))
}

#[derive(Deserialize)]
pub struct NewEmoji {
    name: Option<String>,
}

/// `POST /guilds/{id}/emojis?name=…` : le corps est l'image.
pub async fn create(
    State(state): State<AppState>,
    auth: Auth,
    Path(gid): Path<Id>,
    Query(q): Query<NewEmoji>,
    headers: HeaderMap,
    body: Body,
) -> ApiResult<(StatusCode, Json<Emoji>)> {
    let name = q.name.unwrap_or_default();
    // Droit, nom, place et nom libre sont jugés avant de lire, de décoder ou d'écrire quoi que ce soit.
    state.db.check_new_emoji(gid, auth.user.id, &name).await?;
    let declared =
        headers.get(header::CONTENT_LENGTH).and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<usize>().ok());
    if declared.is_some_and(|n| n > MAX_EMOJI_BYTES) {
        return Err(too_large());
    }
    let raw = axum::body::to_bytes(body, MAX_EMOJI_BYTES).await.map_err(|_| too_large())?;
    if raw.is_empty() {
        return Err(avatar::not_image());
    }
    let (stored, animated) = gated(move || normalize(&raw)).await.ok_or_else(avatar::not_image)??;

    let file_name = format!("{}.{}", avatar::new_key(), stored.ext);
    let (dir, file) = (dir(&state), dir(&state).join(&file_name));
    let written = async {
        tokio::fs::create_dir_all(&dir).await?;
        tokio::fs::write(&file, &stored.bytes).await
    };
    if let Err(e) = written.await {
        tracing::error!("emoji : écriture de {} : {e}", file.display());
        return Err(internal());
    }
    let emoji = match state.db.add_emoji(gid, auth.user.id, &name, animated, &file_name).await {
        Ok(e) => e,
        Err(e) => {
            tokio::fs::remove_file(&file).await.ok();
            return Err(e.into());
        }
    };
    announce(&state, gid).await?;
    Ok((StatusCode::CREATED, Json(emoji)))
}

/// `PATCH /guilds/{id}/emojis/{eid}` : renommer.
pub async fn rename(
    State(state): State<AppState>,
    auth: Auth,
    Path((gid, eid)): Path<(Id, Id)>,
    Json(req): Json<RenameEmoji>,
) -> ApiResult<Json<Emoji>> {
    let before = state.db.guild_emojis(gid).await?.into_iter().find(|e| e.id == eid);
    let emoji = state.db.rename_emoji(gid, eid, auth.user.id, &req.name).await?;
    if before.as_ref() != Some(&emoji) {
        announce(&state, gid).await?;
    }
    Ok(Json(emoji))
}

/// `DELETE /guilds/{id}/emojis/{eid}` : retirer. Les messages et les réactions qui le citent restent
/// dans la base ; le client les montre comme du texte.
pub async fn remove(
    State(state): State<AppState>,
    auth: Auth,
    Path((gid, eid)): Path<(Id, Id)>,
) -> ApiResult<StatusCode> {
    let key = state.db.delete_emoji(gid, eid, auth.user.id).await?;
    crate::attach::remove_files(&state, vec![format!("emojis/{key}")]).await;
    announce(&state, gid).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /emojis/{eid}/image` : l'image, pour toute session ouverte.
pub async fn image(State(state): State<AppState>, _auth: Auth, Path(eid): Path<Id>) -> ApiResult<impl IntoResponse> {
    let key = state.db.emoji_image(eid).await?.ok_or_else(AppError::not_found)?;
    let (name, mime) = file_kind(&key).ok_or_else(AppError::not_found)?;
    let bytes = tokio::fs::read(dir(&state).join(name)).await.map_err(|_| AppError::not_found())?;
    Ok((
        [
            (header::CONTENT_TYPE, HeaderValue::from_static(mime)),
            // Privé : la lecture demande une session, un cache partagé ne doit pas la resservir à d'autres.
            (header::CACHE_CONTROL, HeaderValue::from_static("private, max-age=31536000, immutable")),
        ],
        bytes,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_files_written_here_are_served() {
        let key = "0123456789abcdef0123456789abcdef";
        assert_eq!(file_kind(&format!("{key}.png")).map(|f| f.1), Some("image/png"));
        assert_eq!(file_kind(&format!("{key}.gif")).map(|f| f.1), Some("image/gif"));
        assert_eq!(file_kind(&format!("{key}.webp")).map(|f| f.1), Some("image/webp"));
        for bad in [
            format!("{key}.svg"),
            format!("{key}.png.png"),
            format!("../{key}.png"),
            "x.png".to_string(),
            key.to_string(),
            String::new(),
        ] {
            assert!(file_kind(&bad).is_none(), "{bad}");
        }
    }
}
