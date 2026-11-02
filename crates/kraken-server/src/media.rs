//! Images reçues (fichiers joints, images d'aperçus de liens) : décodage borné, réduction et
//! ré-encodage, ce qui efface EXIF, position GPS et autres métadonnées de la copie servie.

use std::io::Cursor;

use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};

/// Côté au-delà duquel on refuse de décoder (une image minuscule en octets peut réclamer des
/// gigaoctets de mémoire).
const MAX_SIDE: u32 = 8192;

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

/// Décode, avec les limites de taille et de mémoire, en appliquant l'orientation EXIF.
pub fn decode(raw: &[u8]) -> Option<DynamicImage> {
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
    (img.width() > 0 && img.height() > 0).then_some(img)
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
