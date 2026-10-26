// F04 Salon vocal et partage d'écran (S18) : deux navigateurs dans le même salon, micro et
// écran factices de Chromium, vrai son et vraies images à travers le SFU (UDP sur 127.0.0.1).
import { test, expect, a11y, API, type User } from '../fixtures';
import type { Page } from '@playwright/test';

// Micro (bip régulier) et écran (mire animée) factices, sans fenêtre de choix ni d'autorisation :
// marche aussi sans affichage. Ce fichier seul en a besoin, d'où un navigateur lancé pour lui.
test.use({ launchOptions: { args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'] } });

const JOIN = { sdp: 'v=0', mids: { mic: '0', screen: '1', screen_audio: '2' } };

/** Les tuiles de l'appel affiché. */
const tiles = (page: Page) => page.getByRole('list', { name: 'Participants' }).getByRole('listitem');
/** Qui est dans ce salon vocal, d'après la barre latérale. */
const side = (page: Page, channel: string) =>
  page.getByRole('navigation', { name: 'Salons' }).getByRole('list', { name: `Dans ${channel}` });
const bar = (page: Page) => page.getByRole('group', { name: "Commandes de l'appel" });

/** Le plus fort niveau entendu sur les micros reçus pendant une seconde (0 : silence). */
const loudness = (page: Page) =>
  page.evaluate(async () => {
    let best = 0;
    for (const el of document.querySelectorAll('audio')) {
      if (!(el.srcObject instanceof MediaStream)) continue;
      const ctx = new AudioContext();
      await ctx.resume();
      const probe = ctx.createAnalyser();
      ctx.createMediaStreamSource(el.srcObject).connect(probe);
      const buf = new Float32Array(probe.fftSize);
      for (let i = 0; i < 20; i++) {
        await new Promise((r) => setTimeout(r, 50));
        probe.getFloatTimeDomainData(buf);
        for (const x of buf) best = Math.max(best, Math.abs(x));
      }
      await ctx.close();
    }
    return best;
  });

/** Entre dans le salon vocal affiché et attend que la connexion soit établie. */
async function join(u: User) {
  await u.page.getByRole('button', { name: 'Rejoindre', exact: true }).click();
  await expect(u.page.getByRole('region', { name: 'Connexion vocale' }).getByRole('status')).toHaveText('Vocal connecté', {
    timeout: 20_000,
  });
}

test('F04-H1 rejoindre, s’entendre, partager l’écran, regarder, couper le micro, quitter', async ({ user, api }) => {
  test.setTimeout(120_000);
  const a = await user('vox');
  const b = await user('echo');
  // Un serveur neuf à chaque passe : les comptes de démo servent à d'autres tests en parallèle,
  // et une seconde connexion vocale du même compte remplacerait la première.
  const owner = await api(a.token);
  const created = await owner.post(`${API}/guilds`, { data: { name: 'Cloche de plongée' } });
  expect(created.status()).toBe(201);
  const g = await created.json();
  const gid: string = g.guild.id;
  const text = g.channels.find((c: { kind: string }) => c.kind === 'text').id;
  const voice = g.channels.find((c: { kind: string }) => c.kind === 'voice');
  expect(voice.name).toBe('Général');
  const inv = await (await owner.post(`${API}/channels/${text}/invites`, { data: {} })).json();
  expect((await (await api(b.token)).post(`${API}/invites/${inv.code}`)).status()).toBe(200);

  // Avant d'entrer : le salon, personne, « Rejoindre ».
  for (const u of [a, b]) {
    await u.page.goto(`/channels/${gid}/${text}`);
    await u.page.getByRole('link', { name: 'salon vocal Général', exact: true }).click();
    await expect(u.page.getByRole('heading', { name: 'Général', level: 1 })).toBeVisible();
    await expect(u.page.getByText('Personne pour l\'instant.')).toBeVisible();
  }
  await a11y(b.page);

  // A entre : B le voit dans l'aperçu et dans la barre latérale, puis entre à son tour.
  await join(a);
  await expect(b.page.getByText('1 personne dans le salon')).toBeVisible();
  await expect(side(b.page, 'Général').getByText(a.name)).toBeVisible();
  await join(b);
  for (const [me, other] of [
    [a, b],
    [b, a],
  ]) {
    await expect(tiles(me.page)).toHaveCount(2);
    await expect(tiles(me.page).filter({ hasText: `${me.name} (toi)` })).toBeVisible();
    await expect(tiles(me.page).filter({ hasText: other.name })).toBeVisible();
    await expect(side(me.page, 'Général').getByRole('listitem')).toHaveCount(2);
    await expect(side(me.page, 'Général').getByText(other.name)).toBeVisible();
    await expect(me.page.getByRole('region', { name: 'Connexion vocale' })).toContainText('Général / Cloche de plongée');
  }
  // Le micro de chacun arrive chez l'autre, avec du son (le micro factice bipe).
  await expect.poll(() => loudness(b.page), { timeout: 15_000 }).toBeGreaterThan(0.05);
  await expect.poll(() => loudness(a.page), { timeout: 15_000 }).toBeGreaterThan(0.05);

  // A partage son écran : aperçu chez lui, « En direct » chez B, qui regarde.
  await bar(a.page).getByRole('button', { name: 'Partager ton écran' }).click();
  await expect(bar(a.page).getByRole('button', { name: 'Arrêter le partage' })).toBeVisible({ timeout: 15_000 });
  await expect(a.page.getByLabel('Aperçu de ton écran partagé')).toBeVisible();
  const tileA = tiles(b.page).filter({ hasText: a.name });
  await expect(tileA.getByText('En direct')).toBeVisible();
  const sideA = side(b.page, 'Général').getByRole('listitem').filter({ hasText: a.name });
  await expect(sideA).toContainText('En direct');
  const watch = tileA.getByRole('button', { name: `Regarder l'écran de ${a.name}` });
  await watch.click();
  const screen = b.page.getByLabel(`Écran partagé par ${a.name}`);
  await expect(screen).toBeVisible();
  // De vraies images : une taille connue, et la lecture avance.
  await expect.poll(() => screen.evaluate((v: HTMLVideoElement) => v.videoWidth), { timeout: 20_000 }).toBeGreaterThan(0);
  const t0 = await screen.evaluate((v: HTMLVideoElement) => v.currentTime);
  await expect
    .poll(() => screen.evaluate((v: HTMLVideoElement) => (v.paused ? -1 : v.currentTime)), { timeout: 10_000 })
    .toBeGreaterThan(t0 + 0.5);
  await expect(tileA.getByRole('button', { name: 'Arrêter de regarder' })).toHaveAttribute('aria-pressed', 'true');
  await a11y(b.page);

  // A coupe son micro, puis le son : B voit les deux icônes, et A n'entend plus rien.
  const mic = bar(a.page).getByRole('button', { name: 'Couper le micro' });
  await mic.click();
  await expect(mic).toHaveAttribute('aria-pressed', 'true');
  await expect(tileA.getByRole('img', { name: 'micro coupé' })).toBeVisible();
  await expect(sideA.getByRole('img', { name: 'micro coupé' })).toBeVisible();
  const deaf = bar(a.page).getByRole('button', { name: 'Couper le son' });
  await deaf.click();
  await expect(tileA.getByRole('img', { name: 'son coupé' })).toBeVisible();
  const heard = await a.page.evaluate(() => [...document.querySelectorAll('audio')].map((e) => e.muted));
  expect(heard.length).toBeGreaterThan(0);
  expect(heard.every(Boolean)).toBe(true);
  await deaf.click();
  await expect(tileA.getByRole('img', { name: 'son coupé' })).toHaveCount(0);
  await expect(tileA.getByRole('img', { name: 'micro coupé' })).toBeVisible();
  await mic.click();
  await expect(mic).toHaveAttribute('aria-pressed', 'false');
  await expect(tileA.getByRole('img', { name: 'micro coupé' })).toHaveCount(0);

  // A arrête le partage : l'écran disparaît chez B, plus de « Regarder » ni d'« En direct ».
  await bar(a.page).getByRole('button', { name: 'Arrêter le partage' }).click();
  await expect(screen).toHaveCount(0);
  await expect(tileA.getByRole('button')).toHaveCount(0);
  await expect(tileA.getByText('En direct')).toHaveCount(0);
  await expect(bar(a.page).getByRole('button', { name: 'Partager ton écran' })).toBeVisible();

  // B quitte : A ne le voit plus, ni dans l'appel ni dans la barre latérale.
  await bar(b.page).getByRole('button', { name: 'Quitter', exact: true }).click();
  await expect(b.page.getByRole('button', { name: 'Rejoindre', exact: true })).toBeVisible();
  await expect(b.page.getByRole('region', { name: 'Connexion vocale' })).toHaveCount(0);
  await expect(tiles(a.page)).toHaveCount(1);
  await expect(tiles(a.page).filter({ hasText: b.name })).toHaveCount(0);
  await expect(side(a.page, 'Général').getByText(b.name)).toHaveCount(0);
  const states = (await (await owner.get(`${API}/guilds/${gid}`)).json()).voice_states;
  expect(states.map((s: { user_id: string }) => s.user_id)).toEqual([a.id]);

  // A quitte depuis le panneau « Vocal connecté » : le salon est vide pour les deux.
  await a.page.getByRole('button', { name: 'Quitter le salon vocal' }).click();
  for (const u of [a, b]) await expect(u.page.getByText('Personne pour l\'instant.')).toBeVisible();
  expect((await (await owner.get(`${API}/guilds/${gid}`)).json()).voice_states).toEqual([]);
});

test('F04-N1 sans accès : salon d’un autre serveur, salon caché, salon textuel, sans session', async ({ demo, api }) => {
  // Le salon vocal des données de démo (Abysse) : ilan n'est pas membre, noa ne voit pas #staff.
  const lou = await demo('lou');
  const asLou = await api(lou.token);
  const href = await lou.page.getByRole('link', { name: 'Serveur Abysse' }).getAttribute('href');
  const gid = href!.split('/')[2];
  const { channels } = await (await asLou.get(`${API}/guilds/${gid}`)).json();
  const id = (name: string): string => channels.find((c: { name: string }) => c.name === name).id;
  const url = (name: string) => `${API}/channels/${id(name)}/voice`;

  const ilan = await demo('ilan');
  expect((await (await api(ilan.token)).post(url('Salle vocale'), { data: JOIN })).status()).toBe(404);
  // Par l'adresse du salon, il ne trouve rien à rejoindre.
  await ilan.page.goto(`/channels/${gid}/${id('Salle vocale')}`);
  await expect(ilan.page.getByRole('heading', { name: 'Introuvable', level: 1 })).toBeVisible();
  await expect(ilan.page.getByRole('button', { name: 'Rejoindre', exact: true })).toHaveCount(0);
  const noa = await demo('noa');
  expect((await (await api(noa.token)).post(url('staff'), { data: JOIN })).status()).toBe(404);
  expect((await asLou.post(url('général'), { data: JOIN })).status()).toBe(400);
  expect((await (await api()).post(url('Salle vocale'), { data: JOIN })).status()).toBe(401);
  // Ces refus n'ont mis personne en vocal.
  const states: { user_id: string }[] = (await (await asLou.get(`${API}/guilds/${gid}`)).json()).voice_states;
  expect(states.filter((x) => [lou.id, noa.id].includes(x.user_id))).toEqual([]);
});
