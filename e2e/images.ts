// Images fabriquées sur place pour les tests : un PNG dégradé et un petit GIF animé.
import { deflateSync } from 'node:zlib';

const crcTable = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
const crc = (b: Buffer) => {
  let c = 0xffffffff;
  for (const x of b) c = crcTable[(c ^ x) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
};
const chunk = (type: string, data: Buffer) => {
  const body = Buffer.concat([Buffer.from(type), data]);
  const out = Buffer.alloc(12 + data.length);
  out.writeUInt32BE(data.length, 0);
  body.copy(out, 4);
  out.writeUInt32BE(crc(body), 8 + data.length);
  return out;
};

/** Un PNG dégradé de `w` × `h` pixels. */
export function png(w: number, h: number) {
  const head = Buffer.alloc(13);
  head.writeUInt32BE(w, 0);
  head.writeUInt32BE(h, 4);
  head.set([8, 2, 0, 0, 0], 8);
  const raw = Buffer.alloc((w * 3 + 1) * h);
  for (let y = 0; y < h; y++)
    for (let x = 0; x < w; x++) raw.set([(x * 255) / w, (y * 255) / h, 140], y * (w * 3 + 1) + 1 + x * 3);
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', head),
    chunk('IDAT', deflateSync(raw)),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

/**
 * Un GIF animé de `frames` images de 4 × 4 pixels, chacune d'une couleur unie (deux couleurs en
 * alternance). Le flux LZW n'est pas compressé : un code d'effacement tous les deux pixels garde la
 * largeur des codes à 3 bits, ce qui suffit à n'importe quel décodeur.
 */
export function animatedGif(frames = 2) {
  const size = 4;
  const lzw = (index: number) => {
    const bits: number[] = [];
    const emit = (code: number) => {
      for (let i = 0; i < 3; i++) bits.push((code >> i) & 1);
    };
    for (let i = 0; i < size * size; i += 2) {
      emit(4); // effacement
      emit(index);
      emit(index);
    }
    emit(4);
    emit(5); // fin
    const bytes: number[] = [];
    for (let i = 0; i < bits.length; i += 8) {
      let v = 0;
      for (let j = 0; j < 8 && i + j < bits.length; j++) v |= bits[i + j] << j;
      bytes.push(v);
    }
    return bytes;
  };
  const out: number[] = [...Buffer.from('GIF89a'), size, 0, size, 0, 0x80, 0, 0];
  out.push(255, 0, 0, 0, 0, 255); // palette : rouge, bleu
  out.push(0x21, 0xff, 0x0b, ...Buffer.from('NETSCAPE2.0'), 3, 1, 0, 0, 0); // boucle sans fin
  for (let f = 0; f < frames; f++) {
    out.push(0x21, 0xf9, 4, 0, 10, 0, 0, 0); // délai de 100 ms
    out.push(0x2c, 0, 0, 0, 0, size, 0, size, 0, 0);
    const data = lzw(f % 2);
    out.push(2, data.length, ...data, 0);
  }
  out.push(0x3b);
  return Buffer.from(out);
}
