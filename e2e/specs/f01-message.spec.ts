// F01 Envoyer un message dans un salon.
import { test, expect, a11y, createGuild, say, messages, API } from '../fixtures';

const MAX = 4000;

test('F01-H1 Entrée envoie, le message reste après rechargement', async ({ user }) => {
  const { page } = await user('max');
  await createGuild(page, 'Banc de sardines');
  await expect(page.getByText("C'est le tout début de #général")).toBeVisible();
  await a11y(page);
  await say(page, 'général', 'Premier message au fond.');
  await expect(messages(page).filter({ hasText: 'Premier message au fond.' })).toHaveCount(1);
  await expect(page.getByLabel('Écrire dans #général')).toHaveValue('');
  await page.reload();
  await expect(messages(page).filter({ hasText: 'Premier message au fond.' })).toHaveCount(1);
  await a11y(page);
});

test('F01-E1 accents, emoji et retours à la ligne arrivent intacts', async ({ user }) => {
  const { page } = await user('noe');
  await createGuild(page, 'Récif corallien');
  const text = 'Ça déménage ici 🐙🦑 — où est la pieuvre ? «ñ»';
  const box = page.getByLabel('Écrire dans #général');
  await box.fill('ligne une');
  await box.press('Shift+Enter');
  await box.pressSequentially('ligne deux');
  await box.press('Enter');
  await say(page, 'général', text);
  await expect(messages(page).filter({ hasText: text })).toHaveCount(1);
  await page.reload();
  await expect(messages(page).filter({ hasText: text })).toHaveCount(1);
  await expect(messages(page).filter({ hasText: 'ligne une' })).toContainText('ligne deux');
});

test('F01-E2 message vide ou fait d’espaces : rien n’est envoyé', async ({ user }) => {
  const { page } = await user('oda');
  await createGuild(page, 'Lagon calme');
  await say(page, 'général', '    ');
  await page.getByRole('button', { name: 'Envoyer' }).click();
  await page.waitForTimeout(500);
  await expect(messages(page)).toHaveCount(0);
});

test('F01-E3 limite de longueur : 4000 passe, 4001 est bloqué avec un compteur', async ({ user }) => {
  const { page } = await user('pia');
  await createGuild(page, 'Fosse longue');
  const box = page.getByLabel('Écrire dans #général');
  await box.fill('a'.repeat(MAX + 1));
  await expect(page.getByRole('alert')).toContainText(`${MAX + 1} caractères sur ${MAX}`);
  await box.press('Enter');
  await expect(box).toHaveValue('a'.repeat(MAX + 1));
  await expect(messages(page)).toHaveCount(0);
  await box.press('Backspace');
  await box.press('Enter');
  await expect(messages(page)).toHaveCount(1);
  // Un mot de 4000 caractères ne fait pas déborder la page.
  const wide = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
  expect(wide).toBeLessThanOrEqual(0);
});

test('F01-E4 double clic sur Envoyer : un seul message', async ({ user }) => {
  const { page } = await user('quentin');
  await createGuild(page, 'Courant rapide');
  await page.getByLabel('Écrire dans #général').fill('Une seule fois');
  await page.getByRole('button', { name: 'Envoyer' }).dblclick();
  await page.waitForTimeout(500);
  await expect(messages(page).filter({ hasText: 'Une seule fois' })).toHaveCount(1);
});

test('F01-E5 deux onglets du même compte : le message apparaît dans les deux', async ({ user }) => {
  const u = await user('rose');
  const [gid, cid] = await createGuild(u.page, 'Deux hublots');
  const tab2 = await u.context.newPage();
  await tab2.goto(`/channels/${gid}/${cid}`);
  await expect(tab2.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
  await say(u.page, 'général', 'Vu des deux côtés');
  await expect(messages(tab2).filter({ hasText: 'Vu des deux côtés' })).toHaveCount(1);
});

test('F01-E6 hors ligne : échec visible, Réessayer envoie une seule fois', async ({ user, allow }) => {
  allow.push(/ERR_INTERNET_DISCONNECTED|WebSocket/);
  const u = await user('sol');
  await createGuild(u.page, 'Zone blanche');
  const { page } = u;
  await u.context.setOffline(true);
  await say(page, 'général', 'Envoyé sans réseau');
  await expect(page.getByRole('alert')).toBeVisible();
  await u.context.setOffline(false);
  await page.getByRole('button', { name: 'Réessayer' }).click();
  // Le bandeau de reconnexion peut rester quelques secondes : on attend l'envoi lui-même.
  await expect(page.getByRole('button', { name: 'Réessayer' })).toHaveCount(0, { timeout: 10_000 });
  await expect(messages(page).filter({ hasText: 'Envoyé sans réseau' })).not.toHaveAttribute('data-pending', '');
  await page.reload();
  await expect(messages(page).filter({ hasText: 'Envoyé sans réseau' })).toHaveCount(1, { timeout: 10_000 });
});

test('F01-E7 réponse perdue puis Réessayer : pas de doublon', async ({ user, allow }) => {
  allow.push(/ERR_FAILED/);
  const u = await user('tao');
  const [, cid] = await createGuild(u.page, 'Écho');
  const { page } = u;
  // Les événements du gateway sont retenus et la réponse au POST se perd : le serveur a
  // pourtant bien enregistré le message.
  let mute = false;
  await page.routeWebSocket(/\/gateway/, (ws) => {
    const server = ws.connectToServer();
    server.onMessage((m) => !mute && ws.send(m));
    ws.onMessage((m) => server.send(m));
  });
  await page.reload();
  await expect(page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
  mute = true;
  let first = true;
  await page.route(`**${API}/channels/${cid}/messages`, async (route) => {
    if (route.request().method() !== 'POST' || !first) return route.continue();
    first = false;
    await route.fetch();
    await route.abort('failed');
  });
  await say(page, 'général', 'Une seule copie');
  await page.getByRole('button', { name: 'Réessayer' }).click();
  await expect(page.getByRole('alert')).toHaveCount(0);
  await page.unroute(`**${API}/channels/${cid}/messages`);
  const res = await u.context.request.get(`${API}/channels/${cid}/messages`);
  const all = (await res.json()) as { content: string }[];
  expect(all.filter((m) => m.content === 'Une seule copie')).toHaveLength(1);
});

test('F01-E8 clavier seul : du lien d’évitement au message envoyé', async ({ user }) => {
  const { page } = await user('ugo');
  await createGuild(page, 'Clavier sous-marin');
  await page.keyboard.press('Tab');
  await expect(page.getByRole('link', { name: 'Aller au contenu' })).toBeFocused();
  await page.keyboard.press('Enter');
  for (let i = 0; i < 40; i++) {
    if (await page.getByLabel('Écrire dans #général').evaluate((el) => el === document.activeElement)) break;
    await page.keyboard.press('Tab');
  }
  await expect(page.getByLabel('Écrire dans #général')).toBeFocused();
  await page.keyboard.type('Sans souris');
  await page.keyboard.press('Enter');
  await expect(messages(page).filter({ hasText: 'Sans souris' })).toHaveCount(1);
});

test('F01-N1 un non-membre ne voit ni le salon ni ses messages', async ({ user, api, allow }) => {
  allow.push(/status of 404/);
  const a = await user('vera');
  const b = await user('wil');
  const [gid, cid] = await createGuild(a.page, 'Grotte secrète');
  await say(a.page, 'général', 'Secret des profondeurs');
  await expect(messages(a.page)).toHaveCount(1);

  const asB = await api(b.token);
  expect((await asB.get(`${API}/channels/${cid}/messages`)).status()).toBe(404);
  expect((await asB.post(`${API}/channels/${cid}/messages`, { data: { content: 'intrus' } })).status()).toBe(404);
  expect((await asB.get(`${API}/guilds/${gid}`)).status()).toBe(404);
  expect((await asB.post(`${API}/channels/${cid}/invites`, { data: {} })).status()).toBe(404);

  await b.page.goto(`/channels/${gid}/${cid}`);
  await expect(b.page.getByText('Secret des profondeurs')).toHaveCount(0);
  await expect(b.page.getByRole('heading', { level: 1 })).not.toHaveText('général');
});

test('F01-N2 salon privé de l’équipe invisible pour un nouveau membre', async ({ user, api, demo }) => {
  const lou = await demo('lou');
  const asLou = await api(lou.token);
  // Le serveur Abysse appartient à lou ; on prend son salon #général pour inviter.
  await lou.page.getByRole('link', { name: 'Serveur Abysse' }).click();
  await lou.page.getByRole('link', { name: 'général' }).click();
  const [, , gid, cid] = new URL(lou.page.url()).pathname.split('/');
  const inv = await (await asLou.post(`${API}/channels/${cid}/invites`, { data: { max_uses: 1 } })).json();
  const newcomer = await user('xia');
  await newcomer.page.goto(`/invite/${inv.code}`);
  await newcomer.page.getByRole('button', { name: 'Rejoindre le serveur' }).click();
  await expect(newcomer.page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
  const channels = newcomer.page.getByRole('navigation', { name: 'Salons' });
  await expect(channels.getByRole('link', { name: 'général' })).toBeVisible();
  await expect(channels.getByRole('link', { name: 'staff' })).toHaveCount(0);
  await expect(lou.page.getByRole('navigation', { name: 'Salons' }).getByRole('link', { name: 'staff' })).toBeVisible();
  const asNew = await api(newcomer.token);
  const state = await (await asNew.get(`${API}/guilds/${gid}`)).json();
  expect(state.channels.map((c: { name: string }) => c.name)).not.toContain('staff');
});

test.describe('largeur mobile', () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test('F01-E9 à 390 px : tiroir, choix du salon, envoi', async ({ demo }) => {
    const { page } = await demo('demo');
    await page.getByRole('button', { name: 'Ouvrir la navigation' }).click();
    await page.getByRole('link', { name: 'Serveur Abysse' }).click();
    await page.getByRole('link', { name: 'général' }).click();
    await expect(page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
    await expect(page.getByRole('navigation', { name: 'Salons' })).toBeHidden();
    await say(page, 'général', `Depuis un téléphone ${Date.now()}`);
    await expect(messages(page).last()).toContainText('Depuis un téléphone');
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
    await a11y(page);
  });
});
