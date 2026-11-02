// Photo de profil : envoi depuis Réglages > Compte (recadrage dans le navigateur), affichage
// partout, retrait, et refus d'un fichier trop gros ou qui n'est pas une image.
import { deflateSync } from 'node:zlib';
import { test, expect, a11y, API, type User } from '../fixtures';
import type { APIRequestContext } from '@playwright/test';

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
function png(w: number, h: number) {
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

const me = async (api: (t?: string) => Promise<APIRequestContext>, u: User) =>
  (await (await api(u.token)).get(`${API}/users/@me`)).json();

test('profil-H1 envoyer une photo : recadrage, affichage, puis retrait', async ({ user, api }) => {
  const a = await user('lena');
  const b = await user('malo');
  const asA = await api(a.token);
  expect((await asA.post(`${API}/users/@me/relationships`, { data: { username: b.name } })).status()).toBe(201);
  expect((await (await api(b.token)).put(`${API}/users/@me/relationships/${a.id}`, { data: { kind: 'friend' } })).status()).toBe(200);

  await a.page.goto('/settings/account');
  await expect(a.page.getByRole('button', { name: 'Retirer', exact: true })).toHaveCount(0);
  await a11y(a.page);
  await a.page.getByLabel('Changer la photo').setInputFiles({ name: 'photo.png', mimeType: 'image/png', buffer: png(300, 180) });
  const crop = a.page.getByRole('dialog', { name: 'Recadrer ta photo' });
  await expect(crop).toBeVisible();
  await expect(crop.getByRole('button', { name: 'Appliquer' })).toBeEnabled();
  await crop.getByRole('slider', { name: 'Zoom' }).fill('2');
  await crop.getByRole('application').press('ArrowLeft');
  await a11y(a.page);
  await crop.getByRole('button', { name: 'Appliquer' }).click();
  await expect(crop).toHaveCount(0);

  // Dans Réglages : l'image remplace les initiales (alt vide, le nom est sur le conteneur).
  const big = a.page.getByRole('dialog', { name: 'Réglages' }).locator('.k-avatar--xl img');
  await expect(big).toBeVisible();
  await expect.poll(() => big.evaluate((i: HTMLImageElement) => i.naturalWidth)).toBe(256);
  await expect(a.page.getByRole('status').filter({ hasText: 'Photo enregistrée.' })).toBeVisible();
  const key = (await me(api, a)).avatar as string;
  expect(key).toMatch(/^[0-9a-f]{32}$/);
  const served = await asA.get(`${API}/avatars/${key}`);
  expect(served.status()).toBe(200);
  expect(served.headers()['content-type']).toBe('image/png');

  // L'ami la voit sans recharger, dans sa liste.
  await b.page.getByRole('link', { name: 'Amis' }).first().click();
  await expect(b.page.locator(`img[src="${API}/avatars/${key}"]`).first()).toBeVisible();

  // Retirer : retour aux initiales, le fichier n'est plus servi.
  await a.page.getByRole('button', { name: 'Retirer', exact: true }).click();
  await expect(big).toHaveCount(0);
  await expect(a.page.getByRole('status').filter({ hasText: 'Photo retirée.' })).toBeVisible();
  expect((await asA.get(`${API}/avatars/${key}`)).status()).toBe(404);
  expect((await me(api, a)).avatar).toBeNull();
});

test('profil-E1 pas une image, ou trop lourde : message clair, rien d’enregistré', async ({ user, api }) => {
  const a = await user('nina');
  await a.page.goto('/settings/account');
  const input = a.page.getByLabel('Changer la photo');
  await input.setInputFiles({ name: 'notes.txt', mimeType: 'text/plain', buffer: Buffer.from('ceci n’est pas une image') });
  await expect(a.page.getByRole('alert').filter({ hasText: 'pas une image lisible' })).toBeVisible();
  await expect(a.page.getByRole('dialog', { name: 'Recadrer ta photo' })).toHaveCount(0);

  await input.setInputFiles({ name: 'enorme.png', mimeType: 'image/png', buffer: Buffer.alloc(9 * 1024 * 1024) });
  await expect(a.page.getByRole('alert').filter({ hasText: '8 Mo au plus' })).toBeVisible();
  expect((await me(api, a)).avatar).toBeNull();
});
