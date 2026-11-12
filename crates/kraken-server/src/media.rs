//! Images reçues (fichiers joints, images d'aperçus de liens) : décodage borné, réduction et
//! ré-encodage, ce qui efface EXIF, position GPS et autres métadonnées de la copie servie.

use std::fs::File;
use std::io::{BufRead, BufReader, Cursor, Seek};
use std::path::PathBuf;
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
/// Lectures d'en-tête menées en même temps (voir [`peek_size`]) : chacune peut aller jusqu'à
/// [`HEADER_ALLOC`], aucune ne décode de pixel.
const MAX_PEEKS: usize = 8;
static PEEKS: Semaphore = Semaphore::const_new(MAX_PEEKS);
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
/// Une pièce jointe plus grande reste un fichier ordinaire, sans vignette ; une image d'aperçu
/// plus grande est écartée.
pub const MEDIA: Bounds = Bounds { side: 8192, pixels: 40_000_000 };

#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Pas une image que l'on sait lire (ou métadonnées démesurées).
    NotImage,
    /// Lisible, mais plus grande que les [`Bounds`] : jugée d'après son seul en-tête.
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

/// Les limites de la lecture d'un en-tête : de la mémoire, pas de limite de dimensions (c'est
/// justement lui qui les dit).
fn header_limits() -> Limits {
    let mut limits = Limits::no_limits();
    limits.max_alloc = Some(HEADER_ALLOC);
    limits
}

/// Les dimensions que l'en-tête d'un PNG, JPEG, WebP ou GIF annonce, sans décoder un pixel.
/// `None` : un autre format, ou un en-tête illisible.
pub fn header_size<R: BufRead + Seek>(source: R) -> Option<(u32, u32)> {
    let mut reader = ImageReader::new(source).with_guessed_format().ok()?;
    if !matches!(reader.format()?, ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP | ImageFormat::Gif) {
        return None;
    }
    reader.limits(header_limits());
    let (w, h) = reader.into_decoder().ok()?.dimensions();
    (w > 0 && h > 0).then_some((w, h))
}

/// [`header_size`] d'un fichier posé sur le disque, hors du runtime, sans le lire en entier et
/// sans permis de décodage.
pub async fn peek_size(path: PathBuf) -> Option<(u32, u32)> {
    let permit = PEEKS.acquire().await.ok()?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        header_size(BufReader::new(File::open(path).ok()?))
    })
    .await
    .ok()
    .flatten()
}

/// Décode une image dans ces [`Bounds`], en appliquant l'orientation EXIF.
///
/// L'en-tête est lu d'abord, avec une mémoire bornée à [`HEADER_ALLOC`] : c'est lui qui dit la
/// taille, et une image trop grande est refusée avant que le moindre pixel soit décodé. Un PNG de
/// 215 Ko en 8192 × 8192 pèse plus de 400 Mo décodé.
pub fn decode_within(raw: &[u8], bounds: Bounds) -> Result<DynamicImage, Refusal> {
    let mut reader = ImageReader::new(Cursor::new(raw)).with_guessed_format().map_err(|_| Refusal::NotImage)?;
    reader.limits(header_limits());
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

    use image::{GrayImage, RgbImage};

    use super::*;

    /// Les tests qui comptent ou occupent les permis de décodage ne tournent pas ensemble.
    static PERMITS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn encoded(img: &DynamicImage, format: ImageFormat) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, format).unwrap();
        out.into_inner()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn at_most_two_decodes_run_at_the_same_time() {
        let _alone = PERMITS.lock().await;
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

    #[test]
    fn the_header_gives_the_size_of_the_four_safe_formats_and_nothing_else() {
        let gray = DynamicImage::ImageLuma8(GrayImage::new(8200, 16));
        let rgb = DynamicImage::ImageRgb8(RgbImage::new(40, 30));
        for (format, what) in [(ImageFormat::Png, "png"), (ImageFormat::Gif, "gif"), (ImageFormat::WebP, "webp")] {
            let img = if format == ImageFormat::Png { &gray } else { &rgb };
            let (w, h) = (img.width(), img.height());
            let got = header_size(Cursor::new(encoded(img, format)));
            assert_eq!(got, Some((w, h)), "{what}");
        }
        assert_eq!(header_size(Cursor::new(encoded(&rgb, ImageFormat::Jpeg))), Some((40, 30)));
        // Un BMP, du texte, un fichier vide ou coupé : pas lus.
        let mut bmp = b"BM".to_vec();
        bmp.resize(64, 0);
        assert_eq!(header_size(Cursor::new(bmp)), None);
        assert_eq!(header_size(Cursor::new(b"<html>pas un png</html>".to_vec())), None);
        assert_eq!(header_size(Cursor::new(Vec::new())), None);
        let png = encoded(&gray, ImageFormat::Png);
        assert_eq!(header_size(Cursor::new(png[..12].to_vec())), None);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn reading_a_header_needs_no_decode_permit() {
        let _alone = PERMITS.lock().await;
        let dir = std::env::temp_dir().join(format!("kraken-peek-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("large.png");
        std::fs::write(&path, encoded(&DynamicImage::ImageLuma8(GrayImage::new(8200, 16)), ImageFormat::Png)).unwrap();
        // Les deux places de décodage sont prises : la lecture de l'en-tête passe quand même.
        let held = DECODES.acquire_many(MAX_DECODES as u32).await.unwrap();
        let size = tokio::time::timeout(Duration::from_secs(5), peek_size(path.clone())).await;
        drop(held);
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(size.expect("la lecture d'un en-tête attend un permis de décodage"), Some((8200, 16)));
        assert_eq!(PEEKS.available_permits(), MAX_PEEKS, "tous les permis sont rendus");
    }
}
