// F04 Caméra en appel (S18) : deux navigateurs dans le même salon vocal, caméra factice de
// Chromium, vraies images à travers le SFU (UDP sur 127.0.0.1). La caméra allumée remplace
// l'avatar dans la tuile, chez soi (en miroir) comme chez les autres, sans « Regarder ».
import { test, expect, a11y, API, type User } from '../fixtures';
import type { APIRequestContext, Page } from '@playwright/test';

// Micro (bip) et caméra (mire animée) factices, sans fenêtre d'autorisation.
test.use({ launchOptions: { args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'] } });

type Api = (token?: string) => Promise<APIRequestContext>;

const tiles = (page: Page) => page.getByRole('list', { name: 'Participants' }).getByRole('listitem');
const side = (page: Page, channel: string) =>
  page.getByRole('navigation', { name: 'Salons' }).getByRole('list', { name: `Dans ${channel}` });
const bar = (page: Page) => page.getByRole('group', { name: "Commandes de l'appel" });
/** Le bouton de la caméra : son nom ne change pas, c'est `aria-pressed` qui dit s'il est allumé. */
const cameraButton = (page: Page) => bar(page).getByRole('button', { name: 'Caméra', exact: true });
const videoWidth = (v: HTMLVideoElement) => v.videoWidth;

/** Deux comptes dans un salon vocal tout neuf, chacun sur la page du salon ; `a` et `b` y sont entrés. */
async function room(user: (prefix?: string) => Promise<User>, api: Api) {
  const a = await user('cam');
  const b = await user('lens');
  const owner = await api(a.token);
  const created = await owner.post(`${API}/guilds`, { data: { name: 'Chambre noire' } });
  expect(created.status()).toBe(201);
  const g = await created.json();
  const text = g.channels.find((c: { kind: string }) => c.kind === 'text').id;
  const inv = await (await owner.post(`${API}/channels/${text}/invites`, { data: {} })).json();
  expect((await (await api(b.token)).post(`${API}/invites/${inv.code}`)).status()).toBe(200);
  for (const u of [a, b]) {
    await u.page.goto(`/channels/${g.guild.id}/${text}`);
    await u.page.getByRole('link', { name: 'salon vocal Général', exact: true }).click();
    await expect(u.page.getByRole('heading', { name: 'Général', level: 1 })).toBeVisible();
    await u.page.getByRole('button', { name: 'Rejoindre', exact: true }).click();
    await expect(u.page.getByRole('region', { name: 'Connexion vocale' }).getByRole('status')).toHaveText('En appel', {
      timeout: 20_000,
    });
  }
  for (const u of [a, b]) await expect(tiles(u.page)).toHaveCount(2);
  return { a, b };
}

/** Pas de défilement horizontal de la page. */
const fits = (page: Page) =>
  page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth);

test('F04-CAM1 allumer la caméra : aperçu en miroir chez soi, image chez l’autre, extinction', async ({ user, api }) => {
  test.setTimeout(120_000);
  const { a, b } = await room(user, api);
  const button = cameraButton(a.page);

  // Éteinte au départ : un bouton à bascule, des avatars, aucune image.
  await expect(button).toHaveAttribute('aria-pressed', 'false');
  await expect(a.page.locator('video[data-camera]')).toHaveCount(0);
  await expect(b.page.locator('video[data-camera]')).toHaveCount(0);

  // A l'allume : le bouton le dit, l'aperçu apparaît (miroir, sans son), la légende est « Toi ».
  await button.click();
  await expect(button).toHaveAttribute('aria-pressed', 'true');
  await expect(button).toHaveAccessibleName('Caméra');
  const preview = a.page.getByLabel('Aperçu de ta caméra');
  await expect(preview).toBeVisible();
  await expect(preview).toHaveCSS('transform', 'matrix(-1, 0, 0, 1, 0, 0)');
  expect(await preview.evaluate((v: HTMLVideoElement) => v.muted)).toBe(true);
  await expect.poll(() => preview.evaluate(videoWidth), { timeout: 15_000 }).toBeGreaterThan(0);
  const mine = tiles(a.page).filter({ has: preview });
  await expect(mine).toContainText('Toi');

  // B la voit sans rien demander : une vraie image (taille connue, lecture qui avance), pas en miroir.
  const seen = b.page.getByLabel(`Caméra de ${a.name}`);
  await expect(seen).toBeVisible({ timeout: 20_000 });
  await expect.poll(() => seen.evaluate(videoWidth), { timeout: 20_000 }).toBeGreaterThan(0);
  const t0 = await seen.evaluate((v: HTMLVideoElement) => v.currentTime);
  await expect
    .poll(() => seen.evaluate((v: HTMLVideoElement) => (v.paused ? -1 : v.currentTime)), { timeout: 10_000 })
    .toBeGreaterThan(t0 + 0.3);
  const flags = await seen.evaluate((v: HTMLVideoElement) => ({
    muted: v.muted,
    autoplay: v.autoplay,
    inline: v.playsInline,
    fit: getComputedStyle(v).objectFit,
    transform: getComputedStyle(v).transform,
  }));
  expect(flags).toEqual({ muted: true, autoplay: true, inline: true, fit: 'cover', transform: 'none' });
  const tileA = tiles(b.page).filter({ hasText: a.name });
  await expect(tileA.getByLabel(`Caméra de ${a.name}`)).toBeVisible();
  await expect(tileA).toContainText(a.name);
  // Ni bouton ni « En direct » : une caméra n'est pas un écran à regarder.
  await expect(tileA.getByRole('button')).toHaveCount(0);
  await expect(tileA.getByText('En direct')).toHaveCount(0);
  // La liste du salon, dans la barre latérale, le dit aussi.
  const sideA = side(b.page, 'Général').getByRole('listitem').filter({ hasText: a.name });
  await expect(sideA.getByRole('img', { name: 'caméra allumée' })).toBeVisible();
  await a11y(b.page);

  // B n'a pas allumé la sienne : sa tuile garde son avatar.
  await expect(tiles(b.page).filter({ hasText: b.name }).locator('video')).toHaveCount(0);

  // La caméra de B arrive aussi chez A.
  await cameraButton(b.page).click();
  await expect(cameraButton(b.page)).toHaveAttribute('aria-pressed', 'true');
  const seenByA = a.page.getByLabel(`Caméra de ${b.name}`);
  await expect.poll(() => seenByA.evaluate(videoWidth), { timeout: 20_000 }).toBeGreaterThan(0);

  // À 390 px : rien ne déborde, le bouton reste une cible de 44 px au moins, la tuile reste dans la page.
  for (const u of [a, b]) await u.page.setViewportSize({ width: 390, height: 844 });
  for (const u of [a, b]) expect(await fits(u.page)).toBe(true);
  const box = await button.boundingBox();
  expect(box!.width).toBeGreaterThanOrEqual(44);
  expect(box!.height).toBeGreaterThanOrEqual(44);
  const tile = await tiles(b.page).filter({ hasText: a.name }).boundingBox();
  expect(tile!.x).toBeGreaterThanOrEqual(0);
  expect(tile!.x + tile!.width).toBeLessThanOrEqual(390);
  await expect(seen).toBeVisible();
  await a11y(b.page);
  for (const u of [a, b]) await u.page.setViewportSize({ width: 1440, height: 900 });

  // A l'éteint : l'aperçu et l'image disparaissent, l'avatar revient, plus d'icône dans la liste.
  await button.click();
  await expect(button).toHaveAttribute('aria-pressed', 'false');
  await expect(preview).toHaveCount(0);
  await expect(seen).toHaveCount(0);
  await expect(tileA.locator('.k-avatar')).toBeVisible();
  await expect(sideA.getByRole('img', { name: 'caméra allumée' })).toHaveCount(0);
  // Celle de B continue.
  await expect(seenByA).toBeVisible();

  // Rallumée, puis A quitte : sa caméra disparaît chez B, et reste éteinte quand A rejoint de nouveau.
  await button.click();
  await expect(seen).toBeVisible({ timeout: 20_000 });
  await a.page.getByRole('button', { name: 'Quitter le salon vocal' }).click();
  await expect(seen).toHaveCount(0);
  await expect(tiles(b.page)).toHaveCount(1);
  await a.page.getByRole('button', { name: 'Rejoindre', exact: true }).click();
  await expect(a.page.getByRole('region', { name: 'Connexion vocale' }).getByRole('status')).toHaveText('En appel', {
    timeout: 20_000,
  });
  await expect(cameraButton(a.page)).toHaveAttribute('aria-pressed', 'false');
  await expect(preview).toHaveCount(0);
  // La caméra de B, toujours allumée, revient chez A avec sa nouvelle connexion.
  await expect.poll(() => seenByA.evaluate(videoWidth), { timeout: 20_000 }).toBeGreaterThan(0);
});

test('F04-CAM2 caméra refusée : un message sous la barre, sans fenêtre, puis on peut réessayer', async ({ user, api }) => {
  test.setTimeout(120_000);
  const { a, b } = await room(user, api);
  // Le navigateur refuse la caméra (pas le micro, déjà pris).
  await a.page.evaluate(() => {
    const media = navigator.mediaDevices;
    const original = media.getUserMedia.bind(media);
    (window as unknown as { __allow: () => void }).__allow = () => {
      media.getUserMedia = original;
    };
    media.getUserMedia = (c?: MediaStreamConstraints) =>
      c?.video ? Promise.reject(new DOMException('refusé', 'NotAllowedError')) : original(c);
  });
  const button = cameraButton(a.page);
  await button.click();
  const alert = a.page.getByRole('alert').filter({ hasText: "Le navigateur a refusé l'accès à la caméra" });
  await expect(alert).toBeVisible();
  await expect(a.page.getByRole('dialog')).toHaveCount(0);
  await expect(button).toHaveAttribute('aria-pressed', 'false');
  await expect(tiles(b.page).locator('video')).toHaveCount(0);
  await a11y(a.page);

  // Autorisée à nouveau : le message part, la caméra s'allume.
  await a.page.evaluate(() => (window as unknown as { __allow: () => void }).__allow());
  await button.click();
  await expect(button).toHaveAttribute('aria-pressed', 'true');
  await expect(alert).toHaveCount(0);
  await expect(b.page.getByLabel(`Caméra de ${a.name}`)).toBeVisible({ timeout: 20_000 });

  // Caméra débranchée : la piste s'arrête d'elle-même, la caméra s'éteint proprement chez les deux.
  await a.page.evaluate(() => {
    for (const el of document.querySelectorAll<HTMLVideoElement>('video[data-camera]')) {
      const stream = el.srcObject;
      if (stream instanceof MediaStream) {
        for (const t of stream.getVideoTracks()) {
          t.stop();
          t.dispatchEvent(new Event('ended'));
        }
      }
    }
  });
  await expect(button).toHaveAttribute('aria-pressed', 'false');
  await expect(a.page.getByRole('alert').filter({ hasText: "La caméra s'est arrêtée" })).toBeVisible();
  await expect(b.page.getByLabel(`Caméra de ${a.name}`)).toHaveCount(0);
});
