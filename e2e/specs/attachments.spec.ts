// Fichiers joints : plateau, envoi, vignettes et cartes, visionneuse au clavier, collage, refus
// d'un fichier trop lourd. Les aperçus de liens sont couverts par les tests du serveur
// (crates/kraken-server/tests/files.rs) : leur garde SSRF refuse les adresses locales, et ouvrir
// le serveur de test à une page locale demanderait un réglage réservé aux tests.
import { deflateSync } from 'node:zlib';
import { test, expect, a11y, createGuild, say, messages, API, type User } from '../fixtures';
import { flatPng } from '../images';

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
/** Un PNG dégradé de `w` × `h` pixels, fabriqué sur place. */
function png(w: number, h: number, tint = 140) {
  const head = Buffer.alloc(13);
  head.writeUInt32BE(w, 0);
  head.writeUInt32BE(h, 4);
  head.set([8, 2, 0, 0, 0], 8);
  const raw = Buffer.alloc((w * 3 + 1) * h);
  for (let y = 0; y < h; y++)
    for (let x = 0; x < w; x++) raw.set([(x * 255) / w, (y * 255) / h, tint], y * (w * 3 + 1) + 1 + x * 3);
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', head),
    chunk('IDAT', deflateSync(raw)),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

async function together(a: User, b: User, name: string) {
  const [gid, cid] = await createGuild(a.page, name);
  const res = await a.context.request.post(`${API}/channels/${cid}/invites`, {
    data: {},
    headers: { Authorization: `Session ${a.token}` },
  });
  const { code } = await res.json();
  await b.page.goto(`/invite/${code}`);
  await b.page.getByRole('button', { name: 'Rejoindre le serveur' }).click();
  await expect(b.page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
  return [gid, cid];
}

test('PJ-H1 joindre une image et un fichier texte : plateau, vignette, carte, visionneuse au clavier', async ({ user }) => {
  const a = await user('pj');
  const b = await user('lec');
  await together(a, b, 'Atelier des pièces jointes');
  const page = a.page;

  await page.locator('input[type=file]').setInputFiles([
    { name: 'plage.png', mimeType: 'image/png', buffer: png(640, 480) },
    { name: 'notes.txt', mimeType: 'text/plain', buffer: Buffer.from('Rendez-vous à la criée.') },
    { name: 'phare.png', mimeType: 'image/png', buffer: png(300, 500, 40) },
  ]);
  // Plateau : une tuile par fichier, un champ de description pour les images seulement.
  const tray = page.getByRole('list', { name: 'Fichiers à joindre' });
  await expect(tray.getByRole('listitem')).toHaveCount(3);
  await expect(tray.getByText('notes.txt')).toBeVisible();
  await expect(tray.getByLabel('Texte alternatif de plage.png')).toBeVisible();
  await expect(tray.getByLabel('Texte alternatif de notes.txt')).toHaveCount(0);
  await tray.getByLabel('Texte alternatif de plage.png').fill('Une plage au lever du jour');
  await a11y(page);

  // Retirer une tuile.
  await tray.getByRole('button', { name: 'Retirer phare.png' }).click();
  await expect(tray.getByRole('listitem')).toHaveCount(2);
  // Un fichier peut aussi être collé depuis le presse-papiers.
  await page.getByLabel('Écrire dans #général').focus();
  await page.evaluate(() => {
    const dt = new DataTransfer();
    dt.items.add(new File([new Uint8Array([137, 80, 78, 71])], 'collee.bin', { type: 'application/octet-stream' }));
    document.activeElement!.dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true }));
  });
  await expect(tray.getByRole('listitem')).toHaveCount(3);
  await tray.getByRole('button', { name: 'Retirer collee.bin' }).click();
  await expect(tray.getByRole('listitem')).toHaveCount(2);

  // Les envois sont terminés avant que le message parte.
  await expect(tray.getByRole('progressbar')).toHaveCount(0);
  await page.getByLabel('Écrire dans #général').fill('Voilà pour demain');
  await page.getByLabel('Envoyer').click();
  await expect(tray).toHaveCount(0);

  const msg = messages(page).filter({ hasText: 'Voilà pour demain' });
  await expect(msg).toHaveCount(1);
  await expect(msg).not.toHaveAttribute('data-pending', '');
  const thumb = msg.getByRole('button', { name: "Ouvrir l'image plage.png" });
  await expect(thumb).toBeVisible();
  const img = thumb.getByRole('img', { name: 'Une plage au lever du jour' });
  await expect(img).toBeVisible();
  await expect.poll(() => img.evaluate((i: HTMLImageElement) => i.naturalWidth)).toBeGreaterThan(0);
  // Carte de fichier : nom, taille, lien de téléchargement qui rend le contenu envoyé.
  const card = msg.locator('.k-attachment');
  await expect(card).toContainText('notes.txt');
  await expect(card).toContainText('25 o');
  const link = card.getByRole('link', { name: 'Télécharger notes.txt' });
  const res = await page.request.get((await link.getAttribute('href'))!);
  expect(res.status()).toBe(200);
  expect(res.headers()['content-disposition']).toContain('attachment');
  expect(res.headers()['x-content-type-options']).toBe('nosniff');
  expect(await res.text()).toBe('Rendez-vous à la criée.');
  // L'autre membre voit la même chose.
  await expect(messages(b.page).filter({ hasText: 'Voilà pour demain' }).locator('.k-attachment')).toContainText('notes.txt');

  // Visionneuse : s'ouvre au clic, Échap la ferme et rend le focus à la vignette.
  await thumb.focus();
  await thumb.press('Enter');
  const viewer = page.getByRole('dialog', { name: "Visionneuse d'images" });
  await expect(viewer).toBeVisible();
  await expect(viewer.getByRole('link', { name: 'Ouvrir l’original' }).or(viewer.getByRole('link', { name: "Ouvrir l'original" }))).toBeVisible();
  await expect(viewer.getByRole('link', { name: 'Télécharger' })).toHaveAttribute('href', /download=1$/);
  await expect(viewer.getByRole('img')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(viewer).toHaveCount(0);
  await expect(thumb).toBeFocused();
});

test('PJ-H2 plusieurs images : grille, flèches entre les images du message, Échap', async ({ user }) => {
  const a = await user('galerie');
  await createGuild(a.page, 'Galerie du port');
  const page = a.page;
  await page.locator('input[type=file]').setInputFiles([
    { name: 'un.png', mimeType: 'image/png', buffer: png(200, 100, 10) },
    { name: 'deux.png', mimeType: 'image/png', buffer: png(100, 200, 90) },
  ]);
  const tray = page.getByRole('list', { name: 'Fichiers à joindre' });
  await expect(tray.getByRole('listitem')).toHaveCount(2);
  // Entrée pendant l'envoi est refusée (« Patiente ») : attendre la fin des téléversements.
  await expect(tray.getByRole('progressbar')).toHaveCount(0);
  // Un message peut ne porter que des fichiers.
  await page.getByLabel('Écrire dans #général').press('Enter');
  // L'envoi grisé porte déjà les vignettes ; il est remplacé par le vrai message, qui garde le focus de retour.
  await expect(page.locator('article[data-pending]')).toHaveCount(0);
  const msg = messages(page).filter({ has: page.locator('.app-media') });
  await expect(msg).toHaveCount(1);
  await expect(msg.locator('.app-media')).toHaveAttribute('data-multi', '');
  const first = msg.getByRole('button', { name: "Ouvrir l'image un.png" });
  await first.click();
  const viewer = page.getByRole('dialog', { name: "Visionneuse d'images" });
  await expect(viewer.getByText('un.png')).toBeVisible();
  await expect(viewer).toContainText('1 sur 2');
  await page.keyboard.press('ArrowRight');
  await expect(viewer.getByText('deux.png')).toBeVisible();
  await expect(viewer).toContainText('2 sur 2');
  await page.keyboard.press('ArrowLeft');
  await expect(viewer).toContainText('1 sur 2');
  await viewer.getByRole('button', { name: 'Image suivante' }).click();
  await expect(viewer).toContainText('2 sur 2');
  await viewer.getByRole('button', { name: 'Image précédente' }).click();
  await expect(viewer).toContainText('1 sur 2');
  await expect(viewer).not.toContainText('on:click');
  await page.keyboard.press('Escape');
  await expect(viewer).toHaveCount(0);
  await expect(first).toBeFocused();
});

test('PJ-H3 fichier trop lourd : refusé avant tout envoi, le plateau reste vide', async ({ user }) => {
  const a = await user('lourd');
  await createGuild(a.page, 'Quai de chargement');
  const page = a.page;
  await page.locator('input[type=file]').setInputFiles({
    name: 'enorme.bin',
    mimeType: 'application/octet-stream',
    buffer: Buffer.alloc(26 * 1024 * 1024, 1),
  });
  await expect(page.getByRole('alert')).toContainText('enorme.bin');
  await expect(page.getByRole('alert')).toContainText('25,0 Mo au plus');
  await expect(page.getByRole('list', { name: 'Fichiers à joindre' })).toHaveCount(0);
  await say(page, 'général', 'rien ne part en plus');
  await expect(messages(page).filter({ hasText: 'rien ne part en plus' })).toHaveCount(1);
});

test('PJ-E1 image trop grande : refusée avec son message, retirable du plateau', async ({ user, allow }) => {
  allow.push(/status of 4\d\d/);
  const a = await user('geante');
  await createGuild(a.page, 'Quai des géantes');
  const page = a.page;
  // Quelques Ko, mais 8200 pixels de large : le serveur la refuse d'après son en-tête, sans la décoder.
  await page.locator('input[type=file]').setInputFiles({ name: 'panorama.png', mimeType: 'image/png', buffer: flatPng(8200, 16) });
  await expect(page.getByRole('alert')).toContainText('Cette image est trop grande');
  await expect(page.getByRole('alert')).toContainText('8192 pixels de côté et 40 millions de pixels au plus');
  // La vignette en échec reste visible avec son message, jusqu'à ce qu'on la retire.
  await page.getByRole('button', { name: 'Retirer panorama.png' }).click();
  await expect(page.getByRole('list', { name: 'Fichiers à joindre' })).toHaveCount(0);
  // Une image dans la limite part normalement.
  await page.locator('input[type=file]').setInputFiles({ name: 'bande.png', mimeType: 'image/png', buffer: flatPng(8192, 16) });
  await expect(page.getByRole('list', { name: 'Fichiers à joindre' })).toBeVisible();
});
