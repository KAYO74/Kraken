//! Images reçues (fichiers joints, images d'aperçus de liens) : décodage borné, réduction et
//! ré-encodage, ce qui efface EXIF, position GPS et autres métadonnées de la copie servie.

use std::io::Cursor;
use std::sync::atomic::{AtomicUsize, Ordering};

use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use tokio::sync::Semaphore;

/// Mémoire qu'on laisse au décodage d'une image entière.
const DECODE_ALLOC: u64 = 256 * 1024 * 1024;
/// Mémoire qu'on laisse à la lecture de l'en-tête. Des blocs de métadonnées (profil de couleur
/// `iCCP`, texte `zTXt`) sont compressés : quelques Mo peuvent s'annoncer en gigaoctets. Passé cette
/// limite, la décompression s'arrête et l'image est refusée.
const HEADER_ALLOC: u64 = 64 * 1024 * 1024;

/// Décodages d'image menés en même temps sur tout le serveur (photos, bannières, pièces jointes,
/// images d'aperçus) : les autres attendent leur tour.
pub const MAX_DECODES: usize = 2;
static DECODES: Semaphore = Semaphore::const_new(MAX_DECODES);
static RUNNING: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

/// Le plus grand nombre de décodages vus en même temps depuis le démarrage (pour les tests).
#[doc(hidden)]
pub fn peak_decodes() -> usize {
    PEAK.load(Ordering::SeqCst)
}

struct Running;

impl Running {
    fn enter() -> Self {
        PEAK.fetch_max(RUNNING.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
        Self
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        RUNNING.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Exécute un calcul lourd hors du runtime, derrière le portail des décodages : le permis voyage
/// avec le calcul, qui ne le rend qu'en finissant, même si la requête est abandonnée entre-temps.
pub async fn gated<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let permit = DECODES.acquire().await.ok()?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let _running = Running::enter();
        work()
    })
    .await
    .ok()
}

/// Les dimensions qu'on accepte de décoder : un côté, une surface.
#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub side: u32,
    pub pixels: u64,
}

impl Bounds {
    pub fn fits(self, w: u32, h: u32) -> bool {
        w <= self.side && h <= self.side && u64::from(w) * u64::from(h) <= self.pixels
    }

    /// « 4096 pixels de côté et 16 millions de pixels au plus ».
    pub fn describe(self) -> String {
        format!("{} pixels de côté et {} millions de pixels au plus", self.side, self.pixels / 1_000_000)
    }
}

/// Pièces jointes et images d'aperçus : jusqu'à 8192 pixels de côté et 40 millions de pixels.
pub const MEDIA: Bounds = Bounds { side: 8192, pixels: 40_000_000 };

#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Pas une image que l'on sait lire (ou métadonnées démesurées).
    NotImage,
    /// Lisible, mais plus grande que les [`Bounds`] : refusée d'après son seul en-tête.
    TooBig,
}

/// Formats d'image que le navigateur affiche sans risque, et que l'on sait lire.
pub fn safe_format(raw: &[u8]) -> Option<(ImageFormat, &'static str)> {
    match image::guess_format(raw).ok()? {
        ImageFormat::Png => Some((ImageFormat::Png, "image/png")),
        ImageFormat::Jpeg => Some((ImageFormat::Jpeg, "image/jpeg")),
        ImageFormat::WebP => Some((ImageFormat::WebP, "image/webp")),
        ImageFormat::Gif => Some((ImageFormat::Gif, "image/gif")),
        _ => None,
    }
}

/// Décode une image dans ces [`Bounds`], en appliquant l'orientation EXIF.
///
/// L'en-tête est lu d'abord, avec une mémoire bornée à [`HEADER_ALLOC`] : c'est lui qui dit la
/// taille, et une image trop grande est refusée avant que le moindre pixel soit décodé. Un PNG de
/// 215 Ko en 8192 × 8192 pèse plus de 400 Mo décodé.
pub fn decode_within(raw: &[u8], bounds: Bounds) -> Result<DynamicImage, Refusal> {
    let mut reader = ImageReader::new(Cursor::new(raw)).with_guessed_format().map_err(|_| Refusal::NotImage)?;
    let mut header = Limits::no_limits();
    header.max_alloc = Some(HEADER_ALLOC);
    reader.limits(header);
    let mut decoder = reader.into_decoder().map_err(|_| Refusal::NotImage)?;
    let (w, h) = decoder.dimensions();
    if w == 0 || h == 0 {
        return Err(Refusal::NotImage);
    }
    if !bounds.fits(w, h) {
        return Err(Refusal::TooBig);
    }
    let mut limits = Limits::no_limits();
    limits.max_image_width = Some(bounds.side);
    limits.max_image_height = Some(bounds.side);
    limits.max_alloc = Some(DECODE_ALLOC);
    decoder.set_limits(limits).map_err(|_| Refusal::NotImage)?;
    let orientation = decoder.orientation().ok();
    // Un GIF animé ne livre que sa première image.
    let mut img = DynamicImage::from_decoder(decoder).map_err(|_| Refusal::NotImage)?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    if img.width() == 0 || img.height() == 0 { Err(Refusal::NotImage) } else { Ok(img) }
}

/// Une image réduite pour tenir dans `max` × `max` (jamais agrandie), ré-encodée.
pub struct Encoded {
    pub bytes: Vec<u8>,
    /// `jpg` (sans transparence) ou `png`.
    pub ext: &'static str,
    pub width: u32,
    pub height: u32,
}

pub fn shrink(img: &DynamicImage, max: u32) -> Option<Encoded> {
    let small =
        if img.width() > max || img.height() > max { img.resize(max, max, FilterType::Lanczos3) } else { img.clone() };
    let (width, height) = (small.width(), small.height());
    let mut bytes = Vec::new();
    let ext = if small.color().has_alpha() {
        DynamicImage::ImageRgba8(small.to_rgba8()).write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png).ok()?;
        "png"
    } else {
        DynamicImage::ImageRgb8(small.to_rgb8()).write_to(&mut Cursor::new(&mut bytes), ImageFormat::Jpeg).ok()?;
        "jpg"
    };
    Some(Encoded { bytes, ext, width, height })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
    use std::time::Duration;

    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn at_most_two_decodes_run_at_the_same_time() {
        static NOW: AtomicUsize = AtomicUsize::new(0);
        static MOST: AtomicUsize = AtomicUsize::new(0);
        let jobs = (0..8).map(|_| {
            tokio::spawn(gated(|| {
                let now = NOW.fetch_add(1, SeqCst) + 1;
                MOST.fetch_max(now, SeqCst);
                std::thread::sleep(Duration::from_millis(40));
                NOW.fetch_sub(1, SeqCst);
            }))
        });
        for job in jobs.collect::<Vec<_>>() {
            assert!(job.await.unwrap().is_some());
        }
        assert_eq!(
            MOST.load(SeqCst),
            MAX_DECODES,
            "jamais plus de {MAX_DECODES} à la fois, et les deux places servent"
        );
        assert!(peak_decodes() <= MAX_DECODES);
        assert_eq!(DECODES.available_permits(), MAX_DECODES, "tous les permis sont rendus");
    }

    #[test]
    fn bounds_limit_the_side_and_the_surface() {
        assert!(MEDIA.fits(8192, 4882) && MEDIA.fits(8192, 16) && MEDIA.fits(6324, 6324));
        assert!(!MEDIA.fits(8193, 1) && !MEDIA.fits(1, 8193) && !MEDIA.fits(8192, 8192) && !MEDIA.fits(7000, 6000));
        assert_eq!(MEDIA.describe(), "8192 pixels de côté et 40 millions de pixels au plus");
    }
}
