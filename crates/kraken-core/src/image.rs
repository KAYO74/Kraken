//! Reconnaissance des images de profil d'après leurs premiers octets, sans décodage : le
//! navigateur (pour décider de recadrer ou d'envoyer tel quel) et le serveur s'en servent de la
//! même façon. Un GIF ou un WebP **animé** est gardé tel quel ; seules ses métadonnées en sont
//! retirées ([`strip_animated`]).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageKind {
    Png,
    Jpeg,
    Gif,
    Webp,
}

impl ImageKind {
    pub fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::Webp => "image/webp",
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Gif => "gif",
            Self::Webp => "webp",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sniffed {
    pub kind: ImageKind,
    /// GIF de plusieurs images, ou WebP qui se déclare animé.
    pub animated: bool,
}

/// Le format d'après les octets, et s'il est animé. `None` : ce n'est pas un format accepté.
pub fn sniff(bytes: &[u8]) -> Option<Sniffed> {
    let still = |kind| Some(Sniffed { kind, animated: false });
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return still(ImageKind::Png);
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return still(ImageKind::Jpeg);
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        // Illisible : on ne le dit pas animé, le décodage le refusera ensuite.
        let animated = gif_blocks(bytes).is_some_and(|g| g.frames >= 2);
        return Some(Sniffed { kind: ImageKind::Gif, animated });
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        let animated = webp_chunks(bytes)
            .and_then(|c| c.into_iter().next())
            .is_some_and(|c| &c.id == b"VP8X" && c.data.first().is_some_and(|f| f & WEBP_ANIMATION != 0));
        return Some(Sniffed { kind: ImageKind::Webp, animated });
    }
    None
}

/// Une copie sans commentaires, texte, XMP ni autre extension qui n'est pas nécessaire à
/// l'animation (GIF), sans EXIF ni XMP (WebP, dont la position GPS). `None` si le fichier est
/// mal formé.
pub fn strip_animated(bytes: &[u8], kind: ImageKind) -> Option<Vec<u8>> {
    match kind {
        ImageKind::Gif => strip_gif(bytes),
        ImageKind::Webp => strip_webp(bytes),
        _ => None,
    }
}

// ───────── GIF ─────────

struct Gif {
    frames: usize,
}

/// Un bloc d'extension GIF : où il commence et finit dans le fichier, son étiquette et si
/// c'est l'application dont l'animation a besoin (NETSCAPE : le nombre de boucles).
struct Walk<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Walk<'a> {
    fn byte(&mut self) -> Option<u8> {
        let v = *self.b.get(self.at)?;
        self.at += 1;
        Some(v)
    }

    fn skip(&mut self, n: usize) -> Option<()> {
        self.at = self.at.checked_add(n).filter(|a| *a <= self.b.len())?;
        Some(())
    }

    /// Saute des sous-blocs jusqu'au bloc vide.
    fn sub_blocks(&mut self) -> Option<()> {
        loop {
            let n = usize::from(self.byte()?);
            if n == 0 {
                return Some(());
            }
            self.skip(n)?;
        }
    }
}

/// Taille de la palette d'après l'octet de paquets, 0 s'il n'y en a pas.
fn palette(packed: u8) -> usize {
    if packed & 0x80 != 0 { 3 * (2usize << (packed & 7)) } else { 0 }
}

fn gif_blocks(b: &[u8]) -> Option<Gif> {
    let mut w = Walk { b, at: 6 };
    w.skip(4)?; // largeur et hauteur
    let packed = w.byte()?;
    w.skip(2)?; // couleur de fond, rapport
    w.skip(palette(packed))?;
    let mut frames = 0;
    loop {
        match w.byte()? {
            0x3B => return Some(Gif { frames }),
            0x21 => {
                w.skip(1)?;
                w.sub_blocks()?;
            }
            0x2C => {
                w.skip(8)?;
                let packed = w.byte()?;
                w.skip(palette(packed))?;
                w.skip(1)?; // taille minimale du code LZW
                w.sub_blocks()?;
                frames += 1;
            }
            _ => return None,
        }
    }
}

fn strip_gif(b: &[u8]) -> Option<Vec<u8>> {
    let mut w = Walk { b, at: 6 };
    w.skip(4)?;
    let packed = w.byte()?;
    w.skip(2)?;
    w.skip(palette(packed))?;
    let mut out = b[..w.at].to_vec();
    loop {
        let start = w.at;
        match w.byte()? {
            0x3B => {
                out.push(0x3B);
                return Some(out);
            }
            0x21 => {
                let label = w.byte()?;
                // Le premier sous-bloc d'une extension d'application dit laquelle c'est.
                let first = (w.b.get(w.at).copied()? as usize, w.at + 1);
                w.sub_blocks()?;
                let keep = match label {
                    0xF9 => true,
                    0xFF => matches!(w.b.get(first.1..first.1 + first.0), Some(b"NETSCAPE2.0" | b"ANIMEXTS1.0")),
                    // 0xFE commentaire, 0x01 texte, et toute autre : retirés.
                    _ => false,
                };
                if keep {
                    out.extend_from_slice(&w.b[start..w.at]);
                }
            }
            0x2C => {
                w.skip(8)?;
                let packed = w.byte()?;
                w.skip(palette(packed))?;
                w.skip(1)?;
                w.sub_blocks()?;
                out.extend_from_slice(&w.b[start..w.at]);
            }
            _ => return None,
        }
    }
}

// ───────── WebP ─────────

const WEBP_EXIF: u8 = 0x08;
const WEBP_XMP: u8 = 0x04;
const WEBP_ANIMATION: u8 = 0x02;

struct Chunk<'a> {
    id: [u8; 4],
    data: &'a [u8],
}

/// Les blocs RIFF d'un WebP, dans l'ordre. `None` si la taille d'un bloc dépasse le fichier.
fn webp_chunks(b: &[u8]) -> Option<Vec<Chunk<'_>>> {
    let mut out = Vec::new();
    let mut at = 12;
    while at < b.len() {
        let id: [u8; 4] = b.get(at..at + 4)?.try_into().ok()?;
        let size = u32::from_le_bytes(b.get(at + 4..at + 8)?.try_into().ok()?) as usize;
        let end = at.checked_add(8)?.checked_add(size)?;
        out.push(Chunk { id, data: b.get(at + 8..end)? });
        // Les blocs sont alignés sur un nombre pair d'octets.
        at = end + (size & 1);
    }
    Some(out)
}

fn strip_webp(b: &[u8]) -> Option<Vec<u8>> {
    let mut body = b"WEBP".to_vec();
    for c in webp_chunks(b)? {
        if &c.id == b"EXIF" || &c.id == b"XMP " {
            continue;
        }
        body.extend_from_slice(&c.id);
        body.extend_from_slice(&(c.data.len() as u32).to_le_bytes());
        let flags_at = body.len();
        body.extend_from_slice(c.data);
        if &c.id == b"VP8X" && !c.data.is_empty() {
            body[flags_at] &= !(WEBP_EXIF | WEBP_XMP);
        }
        if c.data.len() & 1 == 1 {
            body.push(0);
        }
    }
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un GIF de `frames` images de 1 × 1 pixel, avec un commentaire et le bloc de boucle.
    fn gif(frames: usize, comment: bool) -> Vec<u8> {
        let mut g = b"GIF89a".to_vec();
        g.extend_from_slice(&[1, 0, 1, 0, 0x80, 0, 0]); // 1 × 1, palette de 2 couleurs
        g.extend_from_slice(&[0, 0, 0, 255, 255, 255]);
        g.extend_from_slice(&[0x21, 0xFF, 11]);
        g.extend_from_slice(b"NETSCAPE2.0");
        g.extend_from_slice(&[3, 1, 0, 0, 0]);
        if comment {
            g.extend_from_slice(&[0x21, 0xFE, 4]);
            g.extend_from_slice(b"note");
            g.push(0);
        }
        for _ in 0..frames {
            g.extend_from_slice(&[0x21, 0xF9, 4, 0, 10, 0, 0, 0]);
            g.extend_from_slice(&[0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0]);
            g.extend_from_slice(&[2, 2, 0x44, 0x01, 0]);
        }
        g.push(0x3B);
        g
    }

    fn riff(chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut body = b"WEBP".to_vec();
        for (id, data) in chunks {
            body.extend_from_slice(*id);
            body.extend_from_slice(&(data.len() as u32).to_le_bytes());
            body.extend_from_slice(data);
            if data.len() & 1 == 1 {
                body.push(0);
            }
        }
        let mut out = b"RIFF".to_vec();
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn formats_are_recognised_from_their_first_bytes() {
        let k = |b: &[u8]| sniff(b).map(|s| (s.kind, s.animated));
        assert_eq!(k(&[0x89, b'P', b'N', b'G', 13, 10]), Some((ImageKind::Png, false)));
        assert_eq!(k(&[0xFF, 0xD8, 0xFF, 0xE0]), Some((ImageKind::Jpeg, false)));
        assert_eq!(k(&gif(1, false)), Some((ImageKind::Gif, false)));
        assert_eq!(k(&gif(3, false)), Some((ImageKind::Gif, true)));
        assert_eq!(k(b"<svg xmlns='http://www.w3.org/2000/svg'/>"), None);
        assert_eq!(k(b""), None);
        // Un GIF coupé n'est pas annoncé animé.
        let g = gif(3, false);
        assert_eq!(k(&g[..g.len() - 8]), Some((ImageKind::Gif, false)));
    }

    #[test]
    fn webp_animation_flag_is_read_from_the_extended_header() {
        let ext =
            |flags: u8| riff(&[(b"VP8X", vec![flags, 0, 0, 0, 0, 0, 0, 0, 0, 0]), (b"VP8L", vec![0x2F, 0, 0, 0, 0])]);
        let animated = sniff(&ext(WEBP_ANIMATION)).unwrap();
        assert_eq!((animated.kind, animated.animated), (ImageKind::Webp, true));
        assert!(!sniff(&ext(0)).unwrap().animated);
        assert!(!sniff(&riff(&[(b"VP8L", vec![0x2F, 0, 0, 0, 0])])).unwrap().animated);
        // Un bloc plus long que le fichier : pas animé, pas de panique.
        let mut cut = ext(WEBP_ANIMATION);
        cut.truncate(cut.len() - 3);
        assert!(!sniff(&cut).unwrap().animated);
    }

    #[test]
    fn gif_loses_its_comments_and_keeps_the_animation() {
        let with = gif(2, true);
        let out = strip_animated(&with, ImageKind::Gif).unwrap();
        assert!(out.len() < with.len());
        assert!(!out.windows(4).any(|w| w == b"note"));
        assert!(out.windows(11).any(|w| w == b"NETSCAPE2.0"));
        let s = sniff(&out).unwrap();
        assert_eq!((s.kind, s.animated), (ImageKind::Gif, true));
        assert_eq!(strip_animated(&out, ImageKind::Gif).as_deref(), Some(out.as_slice()), "idempotent");
        assert_eq!(strip_animated(&with[..with.len() - 5], ImageKind::Gif), None);
        assert_eq!(strip_animated(&with, ImageKind::Png), None);
    }

    #[test]
    fn webp_loses_exif_and_xmp() {
        let flags = WEBP_ANIMATION | WEBP_EXIF | WEBP_XMP;
        let img = riff(&[
            (b"VP8X", vec![flags, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            (b"ANIM", vec![0; 6]),
            (b"VP8L", vec![0x2F, 0, 0, 0, 0]),
            (b"EXIF", b"gps:48.85,2.35".to_vec()),
            (b"XMP ", b"<x/>".to_vec()),
        ]);
        let out = strip_animated(&img, ImageKind::Webp).unwrap();
        assert!(!out.windows(3).any(|w| w == b"gps"));
        assert!(!out.windows(4).any(|w| w == b"EXIF" || w == b"XMP "));
        let s = sniff(&out).unwrap();
        assert!(s.animated, "l'animation reste");
        let chunks = webp_chunks(&out).unwrap();
        assert_eq!(chunks[0].data[0], WEBP_ANIMATION);
        assert_eq!(chunks.len(), 3);
        // La taille du fichier annoncée est exacte.
        assert_eq!(u32::from_le_bytes(out[4..8].try_into().unwrap()) as usize, out.len() - 8);
        assert_eq!(strip_animated(&img[..img.len() - 3], ImageKind::Webp), None);
    }
}
