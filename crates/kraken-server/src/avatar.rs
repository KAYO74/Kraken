//! Photos de profil et bannières : reçues telles quelles, vérifiées, puis gardées sur disque
//! sous une clé aléatoire dans `KRAKEN_DATA_DIR/avatars` (les deux ensemble : la clé est celle de
//! `users.avatar_key` ou de `users.banner_key` ; elle ne se devine pas, et change à chaque envoi,
//! d'où un cache sans fin).
//!
//! * Image fixe (PNG, JPEG, WebP, GIF d'une seule image) : décodée, recadrée au centre (carré
//!   pour une photo, 3 pour 1 pour une bannière), réduite et ré-encodée en PNG, ce qui efface EXIF
//!   et autres métadonnées.
//!   Avant de décoder, l'en-tête dit la taille : au-delà de 4096 pixels de côté ou de 16 millions
//!   de pixels, l'image est refusée (`image_too_big`), et deux décodages au plus tournent à la fois
//!   sur tout le serveur (un PNG de 215 Ko en 8192 × 8192 pèse plus de 400 Mo décodé).
//! * GIF ou WebP **animé** : gardé animé, donc pas ré-encodé. Il est décodé image par image pour
//!   vérifier qu'il est lisible et borné (côté, nombre d'images, surface totale), puis recopié sans
//!   ses commentaires, XMP et EXIF. Le navigateur le recadre au centre à l'affichage.

use std::io::Cursor;
use std::path::PathBuf;

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::IntoResponse;
use image::imageops::FilterType;
use image::{AnimationDecoder, DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use kraken_core::image::{ImageKind, sniff, strip_animated};
use kraken_proto::*;
use tokio::sync::Semaphore;

use crate::AppState;
use crate::auth::{Auth, audience_of};
use crate::error::{ApiResult, AppError};

/// Dimensions au-delà desquelles on refuse de décoder une animation (une image minuscule en octets
/// peut réclamer des gigaoctets de mémoire).
const MAX_SIDE: u32 = 8192;
/// Une image fixe se décode en entier avant d'être réduite : un PNG de 215 Ko en 8192 × 8192 pèse
/// plus de 400 Mo décodé. Au-delà de ce côté, ou de cette surface, on la refuse d'après son seul
/// en-tête, sans rien décoder.
const MAX_STILL_SIDE: u32 = 4096;
const MAX_STILL_PIXELS: u64 = 16_000_000;
/// Décodages d'image (photo ou bannière, fixe ou animée) menés en même temps sur tout le serveur ;
/// les autres envois attendent leur tour.
const MAX_DECODES: usize = 2;
static DECODES: Semaphore = Semaphore::const_new(MAX_DECODES);
/// Surface totale (pixels × images) qu'on accepte de décoder pour vérifier une animation.
const MAX_ANIMATED_PIXELS: u64 = 128 * 1024 * 1024;
/// Les extensions sous lesquelles un fichier peut être gardé.
const EXTS: [&str; 3] = ["png", "gif", "webp"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Avatar,
    Banner,
}

impl Kind {
    /// Largeur et hauteur d'une image fixe telle que le serveur la garde.
    fn size(self) -> (u32, u32) {
        match self {
            Self::Avatar => (AVATAR_SIZE, AVATAR_SIZE),
            Self::Banner => (BANNER_WIDTH, BANNER_HEIGHT),
        }
    }

    fn max_bytes(self) -> usize {
        match self {
            Self::Avatar => MAX_AVATAR_BYTES,
            Self::Banner => MAX_BANNER_BYTES,
        }
    }
}

/// Un fichier prêt à être gardé.
struct Stored {
    bytes: Vec<u8>,
    ext: &'static str,
}

enum Refused {
    NotImage,
    BigAnimation,
    BigImage,
}

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

fn big_animation() -> AppError {
    AppError(
        StatusCode::BAD_REQUEST,
        "animation_too_big",
        format!(
            "Cette animation est trop lourde à afficher : {MAX_ANIMATED_SIDE} pixels de côté et {MAX_ANIMATED_FRAMES} images au plus."
        ),
    )
}

fn big_image() -> AppError {
    AppError(
        StatusCode::BAD_REQUEST,
        "image_too_big",
        format!(
            "Cette image est trop grande : {MAX_STILL_SIDE} pixels de côté et {} millions de pixels au plus. Réduis-la avant de l'envoyer.",
            MAX_STILL_PIXELS / 1_000_000
        ),
    )
}

/// Une image fixe de ces dimensions peut-elle être décodée ?
fn still_fits(w: u32, h: u32) -> bool {
    w <= MAX_STILL_SIDE && h <= MAX_STILL_SIDE && u64::from(w) * u64::from(h) <= MAX_STILL_PIXELS
}

/// Exécute un calcul lourd hors du runtime, derrière le portail des décodages : le permis voyage
/// avec le calcul, qui ne le rend qu'en finissant, même si la requête est abandonnée entre-temps.
async fn gated<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let permit = DECODES.acquire().await.ok()?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        work()
    })
    .await
    .ok()
}

fn decode_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(256 * 1024 * 1024);
    limits
}

/// Une animation lisible et bornée ? On la décode en entier, image par image.
fn check_animation<'a, D: AnimationDecoder<'a> + ImageDecoder>(mut decoder: D) -> Result<(), Refused> {
    decoder.set_limits(decode_limits()).map_err(|_| Refused::NotImage)?;
    let (w, h) = decoder.dimensions();
    if w == 0 || h == 0 {
        return Err(Refused::NotImage);
    }
    if w > MAX_ANIMATED_SIDE || h > MAX_ANIMATED_SIDE {
        return Err(Refused::BigAnimation);
    }
    let area = u64::from(w) * u64::from(h);
    let mut frames = 0usize;
    for frame in decoder.into_frames() {
        frame.map_err(|_| Refused::NotImage)?;
        frames += 1;
        if frames > MAX_ANIMATED_FRAMES || area * frames as u64 > MAX_ANIMATED_PIXELS {
            return Err(Refused::BigAnimation);
        }
    }
    // Une « animation » sans image n'en est pas une.
    if frames == 0 { Err(Refused::NotImage) } else { Ok(()) }
}

fn normalize_animated(raw: &[u8], kind: ImageKind) -> Result<Stored, Refused> {
    match kind {
        ImageKind::Gif => {
            check_animation(image::codecs::gif::GifDecoder::new(Cursor::new(raw)).map_err(|_| Refused::NotImage)?)?
        }
        _ => check_animation(image::codecs::webp::WebPDecoder::new(Cursor::new(raw)).map_err(|_| Refused::NotImage)?)?,
    }
    let bytes = strip_animated(raw, kind).ok_or(Refused::NotImage)?;
    Ok(Stored { bytes, ext: kind.ext() })
}

/// Décode, recadre au centre au format de `kind`, réduit, ré-encode en PNG.
fn normalize_still(raw: Vec<u8>, kind: Kind) -> Result<Stored, Refused> {
    let mut reader = ImageReader::new(Cursor::new(raw)).with_guessed_format().map_err(|_| Refused::NotImage)?;
    // L'en-tête d'abord, sans limite : c'est lui qui dit si l'image est trop grande, avant que
    // le moindre pixel soit décodé.
    reader.no_limits();
    let mut decoder = reader.into_decoder().map_err(|_| Refused::NotImage)?;
    let (hw, hh) = decoder.dimensions();
    if hw == 0 || hh == 0 {
        return Err(Refused::NotImage);
    }
    if !still_fits(hw, hh) {
        return Err(Refused::BigImage);
    }
    decoder.set_limits(decode_limits()).map_err(|_| Refused::NotImage)?;
    let orientation = decoder.orientation().ok();
    // Un GIF fixe ne livre qu'une image ; un GIF animé n'arrive pas ici.
    let mut img = DynamicImage::from_decoder(decoder).map_err(|_| Refused::NotImage)?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return Err(Refused::NotImage);
    }
    let (tw, th) = kind.size();
    // Le plus grand rectangle de la forme voulue (tw : th), au centre.
    let (cw, ch) = if u64::from(w) * u64::from(th) >= u64::from(h) * u64::from(tw) {
        (((u64::from(h) * u64::from(tw)) / u64::from(th)).max(1) as u32, h)
    } else {
        (w, ((u64::from(w) * u64::from(th)) / u64::from(tw)).max(1) as u32)
    };
    let cropped = img.crop_imm((w - cw) / 2, (h - ch) / 2, cw, ch);
    let small = DynamicImage::ImageRgba8(cropped.resize_exact(tw, th, FilterType::Lanczos3).to_rgba8());
    let mut out = Vec::new();
    small.write_to(&mut Cursor::new(&mut out), ImageFormat::Png).map_err(|_| Refused::NotImage)?;
    Ok(Stored { bytes: out, ext: "png" })
}

fn normalize(raw: Vec<u8>, kind: Kind) -> Result<Stored, Refused> {
    match sniff(&raw) {
        Some(s) if s.animated => normalize_animated(&raw, s.kind),
        _ => normalize_still(raw, kind),
    }
}

/// Prévient l'intéressé et ceux qui le voient.
async fn announce(state: &AppState, user: &User) -> ApiResult<()> {
    let audience = audience_of(state, user.id).await?;
    state.hub.send(&audience, Event::UserUpdate(user.clone()));
    Ok(())
}

/// Remplacer sa photo de profil : le corps de la requête est l'image elle-même.
pub async fn set(State(state): State<AppState>, auth: Auth, headers: HeaderMap, body: Body) -> ApiResult<Json<User>> {
    put(state, auth, headers, body, Kind::Avatar).await
}

/// Remplacer sa bannière.
pub async fn set_banner(
    State(state): State<AppState>,
    auth: Auth,
    headers: HeaderMap,
    body: Body,
) -> ApiResult<Json<User>> {
    put(state, auth, headers, body, Kind::Banner).await
}

async fn put(state: AppState, auth: Auth, headers: HeaderMap, body: Body, kind: Kind) -> ApiResult<Json<User>> {
    let max = kind.max_bytes();
    let too_big = || {
        AppError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "too_large",
            format!("Cette image pèse trop lourd : {} Mo au plus.", max / (1024 * 1024)),
        )
    };
    let declared =
        headers.get(header::CONTENT_LENGTH).and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<usize>().ok());
    if declared.is_some_and(|n| n > max) {
        return Err(too_big());
    }
    let raw = axum::body::to_bytes(body, max).await.map_err(|_| too_big())?;
    if raw.is_empty() {
        return Err(not_image());
    }
    let stored = gated(move || normalize(raw.into(), kind)).await.ok_or_else(not_image)?.map_err(|e| match e {
        Refused::NotImage => not_image(),
        Refused::BigAnimation => big_animation(),
        Refused::BigImage => big_image(),
    })?;

    let key = new_key();
    let dir = dir(&state);
    let file = dir.join(format!("{key}.{}", stored.ext));
    let written = async {
        tokio::fs::create_dir_all(&dir).await?;
        tokio::fs::write(&file, &stored.bytes).await
    };
    if let Err(e) = written.await {
        tracing::error!("avatar : écriture de {} : {e}", file.display());
        return Err(AppError(StatusCode::INTERNAL_SERVER_ERROR, "internal", "Erreur du serveur, réessaie.".into()));
    }
    let saved = match kind {
        Kind::Avatar => state.db.set_avatar(auth.user.id, Some(&key)).await,
        Kind::Banner => state.db.set_banner(auth.user.id, Some(&key)).await,
    };
    let (user, old) = match saved {
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

/// Retirer sa bannière.
pub async fn remove_banner(State(state): State<AppState>, auth: Auth) -> ApiResult<Json<User>> {
    let (user, old) = state.db.set_banner(auth.user.id, None).await?;
    forget(&state, old).await;
    announce(&state, &user).await?;
    Ok(Json(user))
}

/// Supprime le fichier d'une ancienne photo ou bannière, quel que soit son format.
pub async fn forget(state: &AppState, key: Option<String>) {
    if let Some(key) = key.filter(|k| valid_key(k)) {
        for ext in EXTS {
            tokio::fs::remove_file(dir(state).join(format!("{key}.{ext}"))).await.ok();
        }
    }
}

/// L'image, pour n'importe quel navigateur (une balise `<img>` n'envoie pas d'en-tête
/// d'autorisation). Sa clé aléatoire fait office d'adresse secrète ; elle ne change jamais de
/// contenu, d'où le cache sans fin. Photos et bannières ont la même route de lecture.
pub async fn get(State(state): State<AppState>, Path(key): Path<String>) -> ApiResult<impl IntoResponse> {
    let key = EXTS.iter().find_map(|e| key.strip_suffix(&format!(".{e}"))).unwrap_or(&key);
    if !valid_key(key) {
        return Err(AppError::not_found());
    }
    for (ext, mime) in [("png", "image/png"), ("gif", "image/gif"), ("webp", "image/webp")] {
        if let Ok(bytes) = tokio::fs::read(dir(&state).join(format!("{key}.{ext}"))).await {
            return Ok((
                [
                    (header::CONTENT_TYPE, HeaderValue::from_static(mime)),
                    (header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=31536000, immutable")),
                ],
                bytes,
            ));
        }
    }
    Err(AppError::not_found())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
    use std::time::Duration;

    use super::*;

    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for b in bytes {
            crc ^= u32::from(*b);
            for _ in 0..8 {
                crc = if crc & 1 == 1 { 0xEDB8_8320 ^ (crc >> 1) } else { crc >> 1 };
            }
        }
        !crc
    }

    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut body = kind.to_vec();
        body.extend(data);
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend(&body);
        out.extend(crc32(&body).to_be_bytes());
        out
    }

    /// Un PNG qui annonce `w × h` pixels mais n'en contient aucun : son en-tête est valide, ses
    /// données non. Le décoder échouerait ; seule une lecture de l'en-tête peut le refuser pour sa taille.
    fn header_only_png(w: u32, h: u32) -> Vec<u8> {
        let mut ihdr = w.to_be_bytes().to_vec();
        ihdr.extend(h.to_be_bytes());
        ihdr.extend([8, 2, 0, 0, 0]);
        let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        out.extend(chunk(b"IHDR", &ihdr));
        out.extend(chunk(b"IDAT", &[0x78, 0x01, 0xFF, 0xFF]));
        out.extend(chunk(b"IEND", &[]));
        out
    }

    #[test]
    fn a_still_image_is_judged_on_its_header_before_any_pixel_is_decoded() {
        // Le cas d'un PNG de 215 Ko qui pèse plus de 400 Mo une fois décodé.
        for (w, h) in [(8192, 8192), (4097, 1), (1, 4097), (4096, 4096), (20_000, 20_000)] {
            assert!(
                matches!(normalize_still(header_only_png(w, h), Kind::Avatar), Err(Refused::BigImage)),
                "{w} × {h} aurait dû être refusée d'après son en-tête"
            );
        }
        // À la limite, l'image passe l'en-tête : c'est alors son contenu (absent ici) qui échoue.
        for (w, h) in [(4096, 1), (4000, 4000), (1, 1)] {
            assert!(
                matches!(normalize_still(header_only_png(w, h), Kind::Banner), Err(Refused::NotImage)),
                "{w} × {h} ne doit pas être refusée pour sa taille"
            );
        }
    }

    #[test]
    fn side_and_surface_limits() {
        assert!(still_fits(4096, 3906) && still_fits(4000, 4000) && still_fits(1, 1));
        assert!(!still_fits(4097, 1) && !still_fits(1, 4097) && !still_fits(4096, 4096) && !still_fits(4000, 4001));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn at_most_two_decodes_run_at_the_same_time() {
        static NOW: AtomicUsize = AtomicUsize::new(0);
        static PEAK: AtomicUsize = AtomicUsize::new(0);
        let jobs = (0..8).map(|_| {
            tokio::spawn(gated(|| {
                let now = NOW.fetch_add(1, SeqCst) + 1;
                PEAK.fetch_max(now, SeqCst);
                std::thread::sleep(Duration::from_millis(40));
                NOW.fetch_sub(1, SeqCst);
            }))
        });
        for job in jobs.collect::<Vec<_>>() {
            assert!(job.await.unwrap().is_some());
        }
        assert_eq!(
            PEAK.load(SeqCst),
            MAX_DECODES,
            "jamais plus de {MAX_DECODES} à la fois, et les deux places servent"
        );
        assert_eq!(DECODES.available_permits(), MAX_DECODES, "tous les permis sont rendus");
    }
}
