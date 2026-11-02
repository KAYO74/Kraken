//! Photos de profil : reçues telles quelles, décodées, recadrées en carré, réduites à
//! 256 × 256 et ré-encodées en PNG (ce qui efface EXIF et autres métadonnées), puis gardées
//! sur disque sous une clé aléatoire dans `KRAKEN_DATA_DIR/avatars`. La clé est celle de
//! `users.avatar_key` ; elle ne se devine pas, et change à chaque envoi (cache sans fin).

use std::io::Cursor;
use std::path::PathBuf;

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::IntoResponse;
use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use kraken_proto::*;

use crate::AppState;
use crate::auth::{Auth, audience_of};
use crate::error::{ApiResult, AppError};

/// Dimensions au-delà desquelles on refuse de décoder (une image minuscule en octets peut
/// réclamer des gigaoctets de mémoire).
const MAX_SIDE: u32 = 8192;

fn dir(state: &AppState) -> PathBuf {
    PathBuf::from(&state.cfg.data_dir).join("avatars")
}

fn valid_key(key: &str) -> bool {
    key.len() == 32 && key.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn new_key() -> String {
    rand::random::<[u8; 16]>().iter().map(|b| format!("{b:02x}")).collect()
}

fn not_image() -> AppError {
    AppError(
        StatusCode::BAD_REQUEST,
        "not_image",
        "Ce fichier n'est pas une image lisible : choisis un PNG, un JPEG, un WebP ou un GIF.".into(),
    )
}

/// Décode, recadre au centre en carré, réduit, ré-encode en PNG. `None` si ce n'est pas une
/// image que l'on sait lire (ou si elle est démesurée).
fn normalize(raw: Vec<u8>) -> Option<Vec<u8>> {
    let mut reader = ImageReader::new(Cursor::new(raw)).with_guessed_format().ok()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().ok()?;
    let orientation = decoder.orientation().ok();
    // Un GIF animé ne livre que sa première image.
    let mut img = DynamicImage::from_decoder(decoder).ok()?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    let (w, h) = (img.width(), img.height());
    let side = w.min(h);
    if side == 0 {
        return None;
    }
    let square = img.crop_imm((w - side) / 2, (h - side) / 2, side, side);
    let small =
        DynamicImage::ImageRgba8(square.resize_exact(AVATAR_SIZE, AVATAR_SIZE, FilterType::Lanczos3).to_rgba8());
    let mut out = Vec::new();
    small.write_to(&mut Cursor::new(&mut out), ImageFormat::Png).ok()?;
    Some(out)
}

/// Prévient l'intéressé et ceux qui le voient.
async fn announce(state: &AppState, user: &User) -> ApiResult<()> {
    let audience = audience_of(state, user.id).await?;
    state.hub.send(&audience, Event::UserUpdate(user.clone()));
    Ok(())
}

/// Remplacer sa photo de profil : le corps de la requête est l'image elle-même.
pub async fn set(State(state): State<AppState>, auth: Auth, headers: HeaderMap, body: Body) -> ApiResult<Json<User>> {
    let too_big = || {
        AppError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "too_large",
            format!("Cette image pèse trop lourd : {} Mo au plus.", MAX_AVATAR_BYTES / (1024 * 1024)),
        )
    };
    let declared =
        headers.get(header::CONTENT_LENGTH).and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<usize>().ok());
    if declared.is_some_and(|n| n > MAX_AVATAR_BYTES) {
        return Err(too_big());
    }
    let raw = axum::body::to_bytes(body, MAX_AVATAR_BYTES).await.map_err(|_| too_big())?;
    if raw.is_empty() {
        return Err(not_image());
    }
    let png = tokio::task::spawn_blocking(move || normalize(raw.into())).await.ok().flatten().ok_or_else(not_image)?;

    let key = new_key();
    let dir = dir(&state);
    let file = dir.join(format!("{key}.png"));
    let stored = async {
        tokio::fs::create_dir_all(&dir).await?;
        tokio::fs::write(&file, &png).await
    };
    if let Err(e) = stored.await {
        tracing::error!("avatar : écriture de {} : {e}", file.display());
        return Err(AppError(StatusCode::INTERNAL_SERVER_ERROR, "internal", "Erreur du serveur, réessaie.".into()));
    }
    let (user, old) = match state.db.set_avatar(auth.user.id, Some(&key)).await {
        Ok(r) => r,
        Err(e) => {
            tokio::fs::remove_file(&file).await.ok();
            return Err(e.into());
        }
    };
    forget(&state, old).await;
    announce(&state, &user).await?;
    Ok(Json(user))
}

/// Retirer sa photo de profil : retour aux initiales.
pub async fn remove(State(state): State<AppState>, auth: Auth) -> ApiResult<Json<User>> {
    let (user, old) = state.db.set_avatar(auth.user.id, None).await?;
    forget(&state, old).await;
    announce(&state, &user).await?;
    Ok(Json(user))
}

/// Supprime le fichier d'une ancienne photo.
pub async fn forget(state: &AppState, key: Option<String>) {
    if let Some(key) = key.filter(|k| valid_key(k)) {
        tokio::fs::remove_file(dir(state).join(format!("{key}.png"))).await.ok();
    }
}

/// La photo, pour n'importe quel navigateur (une balise `<img>` n'envoie pas d'en-tête
/// d'autorisation). Sa clé aléatoire fait office d'adresse secrète ; elle ne change jamais de
/// contenu, d'où le cache sans fin.
pub async fn get(State(state): State<AppState>, Path(key): Path<String>) -> ApiResult<impl IntoResponse> {
    let key = key.strip_suffix(".png").unwrap_or(&key);
    if !valid_key(key) {
        return Err(AppError::not_found());
    }
    let bytes = tokio::fs::read(dir(&state).join(format!("{key}.png"))).await.map_err(|_| AppError::not_found())?;
    Ok((
        [
            (header::CONTENT_TYPE, HeaderValue::from_static("image/png")),
            (header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=31536000, immutable")),
        ],
        bytes,
    ))
}
