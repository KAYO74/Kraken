// F04/F03 Appels des MP et des groupes, sans serveur (S19) : l'appel sonne chez les autres, on
// décroche ou on refuse, on s'entend, on partage son écran et on le contrôle comme dans un salon
// vocal ; la sonnerie s'arrête seule, l'appel finit au départ du dernier et laisse sa trace dans
// la conversation. Micro et écran factices de Chromium, vrai son à travers le SFU.
import { test, expect, a11y, API, type User } from '../fixtures';
import type { APIRequestContext, Page } from '@playwright/test';

test.use({ launchOptions: { args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'] } });

const tiles = (page: Page) => page.getByRole('list', { name: 'Participants' }).getByRole('listitem');
const bar = (page: Page) => page.getByRole('group', { name: "Commandes de l'appel" });
const voicePanel = (page: Page) => page.getByRole('region', { name: 'Connexion vocale' });
const incoming = (page: Page, from: string) =>
  page.getByRole('region', { name: 'Appels entrants' }).getByRole('alertdialog', { name: `Appel de ${from}` });
const callArea = (page: Page) => page.getByRole('region', { name: 'Appel', exact: true });
const notes = (page: Page) => page.getByRole('log').getByRole('article');

type Api = (t?: string) => Promise<APIRequestContext>;

/** `a` et `b` deviennent amis (par l'API). */
async function befriend(api: Api, a: User, b: User) {
  const asA = await api(a.token);
  expect((await asA.post(`${API}/users/@me/relationships`, { data: { username: b.name } })).status()).toBe(201);
  const asB = await api(b.token);
  expect((await asB.put(`${API}/users/@me/relationships/${a.id}`, { data: { kind: 'friend' } })).status()).toBe(200);
}

async function inCall(u: User) {
  await expect(voicePanel(u.page).getByRole('status')).toHaveText('En appel', { timeout: 20_000 });
}

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

/** L'appli de bureau, vue de la page : `remote_begin` accepte, chaque appel est noté. */
async function fakeDesktop(u: User) {
  await u.context.addInitScript(() => {
    const w = window as unknown as Record<string, unknown>;
    const calls: { cmd: string; args: Record<string, unknown> }[] = [];
    w.__calls = calls;
    w.__TAURI_INTERNALS__ = {
      invoke: (cmd: string, args: Record<string, unknown>) => {
        calls.push({ cmd, args: JSON.parse(JSON.stringify(args ?? {})) });
        return Promise.resolve(cmd === 'remote_begin' ? `jeton-${calls.length}` : null);
      },
    };
  });
  await u.page.reload();
}

test('F04-C1 appel en MP depuis Amis : sonnerie, décrocher, s’entendre, écran regardé et contrôlé, raccrocher', async ({
  user,
  api,
}) => {
  test.setTimeout(150_000);
  const a = await user('appelant');
  const b = await user('appele');
  await befriend(api, a, b);
  await fakeDesktop(a);
  await expect(a.page.getByRole('heading', { name: 'Amis', level: 1 })).toBeVisible();

  // A appelle B depuis sa ligne d'ami : A arrive dans le MP, l'appel lancé.
  await a.page.getByRole('tab', { name: 'Tous' }).click();
  await a.page.getByRole('button', { name: `Appeler ${b.name}` }).click();
  await a.page.waitForURL(/\/channels\/@me\/[0-9a-f-]{36}$/);
  const dm = a.page.url().split('/').pop()!;
  await inCall(a);
  await expect(voicePanel(a.page)).toContainText(`Appel avec ${b.name}`);
  await expect(callArea(a.page).getByRole('status').filter({ hasText: `Ça sonne chez ${b.name}…` })).toBeVisible();
  await expect(notes(a.page).filter({ hasText: 'Tu as lancé un appel.' })).toBeVisible();

  // Chez B, sur l'accueil : l'appel sonne, avec Rejoindre et Refuser ; le MP montre l'appel.
  const ring = incoming(b.page, a.name);
  await expect(ring).toBeVisible();
  await expect(ring).toContainText("t'appelle");
  await expect(
    b.page.getByRole('navigation', { name: 'Messages privés' }).getByRole('img', { name: 'Appel en cours' }),
  ).toBeVisible();
  await a11y(b.page);
  await ring.getByRole('button', { name: 'Rejoindre' }).click();
  await b.page.waitForURL(`**/channels/@me/${dm}`);
  await inCall(b);
  await expect(ring).toHaveCount(0);
  await expect(callArea(a.page).getByText('Ça sonne chez')).toHaveCount(0);

  for (const [me, other] of [
    [a, b],
    [b, a],
  ]) {
    await expect(tiles(me.page)).toHaveCount(2);
    await expect(tiles(me.page).filter({ hasText: `${me.name} (toi)` })).toBeVisible();
    await expect(tiles(me.page).filter({ hasText: other.name })).toBeVisible();
    // Dans la barre latérale, sous la conversation : qui est dans l'appel.
    const side = me.page.getByRole('navigation', { name: 'Messages privés' }).getByRole('list', { name: `Dans ${other.name}` });
    await expect(side.getByRole('listitem')).toHaveCount(2);
  }
  await expect.poll(() => loudness(b.page), { timeout: 15_000 }).toBeGreaterThan(0.05);
  await expect.poll(() => loudness(a.page), { timeout: 15_000 }).toBeGreaterThan(0.05);

  // Micro coupé chez A : B le voit.
  const tileA = tiles(b.page).filter({ hasText: a.name });
  await bar(a.page).getByRole('button', { name: 'Couper le micro' }).click();
  await expect(tileA.getByRole('img', { name: 'micro coupé' })).toBeVisible();
  await bar(a.page).getByRole('button', { name: 'Couper le micro' }).click();
  await expect(tileA.getByRole('img', { name: 'micro coupé' })).toHaveCount(0);

  // A partage son écran, B le regarde : de vraies images.
  await bar(a.page).getByRole('button', { name: 'Partager ton écran' }).click();
  await expect(bar(a.page).getByRole('button', { name: 'Arrêter le partage' })).toBeVisible({ timeout: 15_000 });
  await tileA.getByRole('button', { name: `Regarder l'écran de ${a.name}` }).click();
  const screen = b.page.getByLabel(`Écran partagé par ${a.name}`);
  await expect.poll(() => screen.evaluate((v: HTMLVideoElement) => v.videoWidth), { timeout: 20_000 }).toBeGreaterThan(0);
  const t0 = await screen.evaluate((v: HTMLVideoElement) => v.currentTime);
  await expect
    .poll(() => screen.evaluate((v: HTMLVideoElement) => (v.paused ? -1 : v.currentTime)), { timeout: 10_000 })
    .toBeGreaterThan(t0 + 0.5);
  await a11y(b.page);

  // Contrôle à distance de l'écran de A (appli de bureau simulée) : demandé, accepté, une touche.
  await b.page.getByRole('button', { name: `Demander le contrôle de l'écran de ${a.name}` }).click();
  const control = a.page.getByRole('region', { name: 'Contrôle à distance de ton écran' });
  await control.getByRole('button', { name: 'Accepter' }).click();
  await expect(b.page.getByText(`Tu contrôles l'écran de ${a.name}.`)).toBeVisible();
  await b.page.keyboard.press('a');
  await expect
    .poll(() =>
      a.page.evaluate(() =>
        (window as unknown as { __calls: { cmd: string; args: { events?: object[] } }[] }).__calls
          .filter((c) => c.cmd === 'remote_input')
          .flatMap((c) => c.args.events ?? []),
      ),
    )
    .toEqual(expect.arrayContaining([{ k: 'key', code: 'KeyA', down: true }]));
  await b.page.keyboard.press('Escape');
  await bar(a.page).getByRole('button', { name: 'Arrêter le partage' }).click();
  await expect(screen).toHaveCount(0);

  // B raccroche : A reste, seul ; B peut revenir tant que l'appel dure.
  await bar(b.page).getByRole('button', { name: 'Raccrocher' }).click();
  await expect(voicePanel(b.page)).toHaveCount(0);
  await expect(tiles(a.page)).toHaveCount(1);
  await callArea(b.page).getByRole('button', { name: "Rejoindre l'appel" }).click();
  await inCall(b);
  await expect(tiles(a.page)).toHaveCount(2);
  await bar(b.page).getByRole('button', { name: 'Raccrocher' }).click();
  await expect(tiles(a.page)).toHaveCount(1);

  // A raccroche depuis le panneau : l'appel est fini pour les deux, avec sa trace dans le fil.
  await voicePanel(a.page).getByRole('button', { name: 'Raccrocher' }).click();
  for (const u of [a, b]) {
    await expect(callArea(u.page)).toHaveCount(0);
    await expect(u.page.getByRole('button', { name: 'Démarrer un appel' })).toBeVisible();
  }
  await expect(notes(a.page).filter({ hasText: "Tu as lancé un appel qui a duré moins d'une minute." })).toBeVisible();
  await expect(notes(b.page).filter({ hasText: `${a.name} a lancé un appel qui a duré moins d'une minute.` })).toBeVisible();
  await a11y(b.page);
});

test('F04-C2 appel de groupe à trois : deux décrochent, un refuse puis rejoint', async ({ user, api }) => {
  test.setTimeout(150_000);
  const a = await user('chef');
  const b = await user('second');
  const c = await user('tiers');
  await befriend(api, a, b);
  await befriend(api, a, c);
  const res = await (await api(a.token)).post(`${API}/users/@me/channels`, { data: { recipients: [b.id, c.id] } });
  expect(res.status()).toBe(201);
  const group = (await res.json()).id;

  await a.page.goto(`/channels/@me/${group}`);
  await a.page.getByRole('button', { name: 'Démarrer un appel' }).click();
  await inCall(a);
  const ringing = callArea(a.page).getByRole('status').filter({ hasText: 'Ça sonne chez' });
  await expect(ringing).toContainText(b.name);
  await expect(ringing).toContainText(c.name);

  // Ça sonne chez B et chez C, en disant où.
  for (const u of [b, c]) await expect(incoming(u.page, a.name)).toContainText(`t'appelle dans`);
  // C refuse : plus de sonnerie chez lui, A ne l'attend plus.
  await incoming(c.page, a.name).getByRole('button', { name: 'Refuser' }).click();
  await expect(incoming(c.page, a.name)).toHaveCount(0);
  await expect(ringing).toHaveText(`Ça sonne chez ${b.name}…`);
  // B décroche.
  await incoming(b.page, a.name).getByRole('button', { name: 'Rejoindre' }).click();
  await inCall(b);
  await expect(ringing).toHaveCount(0);
  for (const u of [a, b]) await expect(tiles(u.page)).toHaveCount(2);
  await expect.poll(() => loudness(b.page), { timeout: 15_000 }).toBeGreaterThan(0.05);

  // C ouvre le groupe : l'appel en cours, qui y est, et « Rejoindre l'appel ».
  await c.page.goto(`/channels/@me/${group}`);
  const lobby = callArea(c.page);
  await expect(lobby.getByRole('list', { name: "Dans l'appel" }).getByRole('listitem')).toHaveCount(2);
  await expect(c.page.getByRole('button', { name: 'Démarrer un appel' })).toHaveCount(0);
  await a11y(c.page);
  await lobby.getByRole('button', { name: "Rejoindre l'appel" }).click();
  await inCall(c);
  for (const u of [a, b, c]) await expect(tiles(u.page)).toHaveCount(3);
  await expect.poll(() => loudness(c.page), { timeout: 15_000 }).toBeGreaterThan(0.05);

  // Tous raccrochent : l'appel finit.
  for (const u of [c, b, a]) await bar(u.page).getByRole('button', { name: 'Raccrocher' }).click();
  for (const u of [a, b, c]) await expect(u.page.getByRole('button', { name: 'Démarrer un appel' })).toBeVisible();
  await expect(notes(c.page).filter({ hasText: `${a.name} a lancé un appel qui a duré` })).toBeVisible();
});

test('F04-C3 sonnerie sans réponse : elle s’arrête seule, « Appel manqué » ; blocage et inconnus refusés', async ({
  user,
  api,
  allow,
}) => {
  test.setTimeout(120_000);
  allow.push(/status of 40[34]/);
  const a = await user('sonne');
  const b = await user('absent');
  const stranger = await user('intrus');
  await befriend(api, a, b);
  const asA = await api(a.token);
  const dm = (await (await asA.post(`${API}/users/@me/channels`, { data: { recipients: [b.id] } })).json()).id;

  await a.page.goto(`/channels/@me/${dm}`);
  await a.page.getByRole('button', { name: 'Démarrer un appel' }).click();
  await inCall(a);
  await expect(incoming(b.page, a.name)).toBeVisible();
  // Personne ne répond : après 30 s, ça ne sonne plus nulle part.
  await expect(incoming(b.page, a.name)).toHaveCount(0, { timeout: 40_000 });
  await expect(callArea(a.page).getByText('Ça sonne chez')).toHaveCount(0);
  await voicePanel(a.page).getByRole('button', { name: 'Raccrocher' }).click();
  await expect(notes(a.page).filter({ hasText: "Personne n'a répondu à ton appel." })).toBeVisible();
  await b.page.goto(`/channels/@me/${dm}`);
  await expect(notes(b.page).filter({ hasText: `Appel manqué de ${a.name}.` })).toBeVisible();
  await a11y(b.page);

  // Hors de la conversation : rien à rejoindre ni à refuser.
  const join = { sdp: 'v=0', mids: { mic: '0', screen: '1', screen_audio: '2' } };
  const asStranger = await api(stranger.token);
  expect((await asStranger.post(`${API}/channels/${dm}/voice`, { data: join })).status()).toBe(404);
  expect((await asStranger.post(`${API}/channels/${dm}/call/decline`)).status()).toBe(404);

  // B bloque A : plus d'appel possible entre eux, dans un sens comme dans l'autre.
  const asB = await api(b.token);
  expect((await asB.put(`${API}/users/@me/relationships/${a.id}`, { data: { kind: 'blocked' } })).status()).toBe(200);
  const refused = await asA.post(`${API}/channels/${dm}/voice`, { data: join });
  expect(refused.status()).toBe(403);
  expect((await refused.json()).code).toBe('call_refused');
  expect((await (await asB.post(`${API}/channels/${dm}/voice`, { data: join })).json()).code).toBe('call_blocked');
  await expect(b.page.getByRole('button', { name: 'Démarrer un appel' })).toBeDisabled();
});
