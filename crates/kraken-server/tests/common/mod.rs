//! Aides communes aux tests HTTP : une bombe de décompression dans un PNG minuscule, et la
//! mémoire qu'a prise le processus (serveur compris, puisque les tests le lancent en interne).
#![allow(dead_code)]

use std::io::Cursor;

use image::{ImageFormat, Rgb, RgbImage};

/// Écrit des bits dans l'ordre d'un flux deflate : les champs de poids faible d'abord, les codes
/// de Huffman en commençant par leur bit de poids fort.
struct Bits {
    out: Vec<u8>,
    used: u32,
}

impl Bits {
    fn bit(&mut self, b: bool) {
        if self.used == 0 {
            self.out.push(0);
        }
        if b {
            *self.out.last_mut().unwrap() |= 1 << self.used;
        }
        self.used = (self.used + 1) % 8;
    }

    fn field(&mut self, v: u32, n: u32) {
        for i in 0..n {
            self.bit(v >> i & 1 == 1);
        }
    }

    fn code(&mut self, v: u32, n: u32) {
        for i in (0..n).rev() {
            self.bit(v >> i & 1 == 1);
        }
    }

    fn zeros(&mut self, mut n: u64) {
        while n > 0 && self.used != 0 {
            self.bit(false);
            n -= 1;
        }
        self.out.resize(self.out.len() + (n / 8) as usize, 0);
        for _ in 0..n % 8 {
            self.bit(false);
        }
    }
}

/// Un flux zlib de quelques Mo qui se décompresse en `1 + 258 × runs` zéros (environ 1 pour 1000) :
/// un bloc de Huffman dynamique où « copier 258 octets, distance 1 » ne coûte que deux bits. Écrit
/// directement, sans jamais produire ces zéros : `runs = 12_480_000` donne 3 Gio pour 3 Mo.
pub fn zlib_zeros(runs: u64) -> Vec<u8> {
    let mut b = Bits { out: Vec::new(), used: 0 };
    b.field(1, 1); // dernier bloc
    b.field(2, 2); // Huffman dynamique
    b.field(29, 5); // 286 symboles littéral/longueur
    b.field(0, 5); // 1 code de distance
    b.field(14, 4); // 18 longueurs de codes de longueurs
    for len in [0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 2] {
        b.field(len, 3);
    }
    b.code(0b11, 2); // littéral 0 : longueur 2
    b.code(0, 1);
    b.field(127, 7); // 138 zéros
    b.code(0, 1);
    b.field(106, 7); // 117 zéros
    b.code(0b11, 2); // fin de bloc (256) : longueur 2
    b.code(0, 1);
    b.field(17, 7); // 28 zéros
    b.code(0b10, 2); // longueur 258 (285) : longueur 1
    b.code(0b10, 2); // distance 1 : longueur 1
    b.code(0b10, 2); // le littéral 0
    b.zeros(2 * runs); // `runs` fois : longueur 258 (« 0 »), distance 1 (« 0 »)
    b.code(0b11, 2); // fin de bloc
    let total = 1 + 258 * runs;
    let adler = (((total % 65521) as u32) << 16) | 1;
    let mut z = vec![0x78, 0x01];
    z.extend(b.out);
    z.extend(adler.to_be_bytes());
    z
}

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

/// Un PNG de 64 × 64 pixels, lisible, dont le bloc `iCCP` (profil de couleur) se décompresse en
/// `1 + 258 × runs` octets de zéros.
pub fn icc_bomb_png(runs: u64) -> Vec<u8> {
    let mut ihdr = 64u32.to_be_bytes().to_vec();
    ihdr.extend(64u32.to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    let mut icc = b"icc\0\0".to_vec();
    icc.extend(zlib_zeros(runs));
    // Les pixels : 64 lignes d'un octet de filtre et de 192 octets nuls, en blocs deflate « stockés ».
    let raw = vec![0u8; 64 * (1 + 64 * 3)];
    let mut idat = vec![0x78, 0x01, 0x01];
    idat.extend((raw.len() as u16).to_le_bytes());
    idat.extend((!(raw.len() as u16)).to_le_bytes());
    idat.extend(&raw);
    idat.extend((((raw.len() as u32) << 16) | 1).to_be_bytes()); // Adler-32 d'octets nuls : n << 16 | 1
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    png.extend(chunk(b"IHDR", &ihdr));
    png.extend(chunk(b"iCCP", &icc));
    png.extend(chunk(b"IDAT", &idat));
    png.extend(chunk(b"IEND", &[]));
    png
}

/// La plus grande mémoire résidente du processus depuis son démarrage, en Mio (Linux).
pub fn peak_rss_mib() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let kb = status
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.split_whitespace().next())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    kb / 1024
}

/// Une image unie de `w × h` pixels : minuscule en octets, énorme une fois décodée.
pub fn flat(w: u32, h: u32, format: ImageFormat) -> Vec<u8> {
    let mut out = Vec::new();
    match format {
        ImageFormat::Png => {
            image::GrayImage::from_pixel(w, h, image::Luma([7])).write_to(&mut Cursor::new(&mut out), format)
        }
        _ => RgbImage::from_pixel(w, h, Rgb([7, 7, 7])).write_to(&mut Cursor::new(&mut out), format),
    }
    .unwrap();
    out
}
