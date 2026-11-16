// Messages épinglés (v0.20.0) : le bouton de la barre d'outils (droit « Gérer les messages » dans un serveur, tout le
// monde en MP), le repère dans l'en-tête, le panneau « Messages épinglés » (états, aller au message, désépingler,
// focus), le temps réel entre deux clients, la limite de 50, le clavier, axe et 390 px.
import { test, expect, a11y, createGuild, messages, API, type User } from '../fixtures';
import type { APIRequestContext, Locator, Page } from '@playwright/test';

type Api = (t?: string) => Promise<APIRequestContext>;

/** `a` crée un serveur, `b` le rejoint par invitation ; les deux sont sur #général. */
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

/** Écrit un message par l'API et rend son id. */
async function post(as: APIRequestContext, cid: string, content: string): Promise<string> {
  const res = await as.post(`${API}/channels/${cid}/messages`, { data: { content } });
  expect(res.status(), await res.text()).toBe(201);
  return (await res.json()).id;
}

async function befriend(api: Api, a: User, b: User) {
  expect((await (await api(a.token)).post(`${API}/users/@me/relationships`, { data: { username: b.name } })).status()).toBe(201);
  expect((await (await api(b.token)).put(`${API}/users/@me/relationships/${a.id}`, { data: { kind: 'friend' } })).status()).toBe(200);
}

/** Le message qui contient `text`, une fois confirmé par le serveur. */
async function sent(page: Page, text: string) {
  const m = messages(page).filter({ hasText: text });
  await expect(m).toHaveCount(1);
  await expect(m).not.toHaveAttribute('data-pending', '');
  return m;
}

const toolbar = (m: Locator) => m.getByRole('toolbar', { name: 'Actions sur le message' });
/** Les noms des boutons de la barre d'outils, dans l'ordre. */
const actions = (m: Locator) => toolbar(m).getByRole('button').evaluateAll((els) => els.map((e) => e.getAttribute('aria-label')));
/** Survole le message et clique l'action de sa barre d'outils. */
async function act(page: Page, text: string, action: string) {
  const m = await sent(page, text);
  await m.hover();
  await toolbar(m).getByRole('button', { name: action, exact: true }).click();
}
/** La zone `status` de la coquille, avec ce texte (il y a d'autres zones `status`). */
const said = (page: Page, text: string) => page.getByRole('status').filter({ hasText: text });
const opener = (page: Page) => page.getByRole('button', { name: 'Messages épinglés', exact: true });
const panel = (page: Page) => page.getByRole('complementary', { name: 'Messages épinglés' });
const sheet = (page: Page) => page.getByRole('dialog', { name: 'Messages épinglés' });
/** Les lignes du panneau (ou de la feuille) ouvert, de la plus récente à la plus ancienne. */
const rows = (p: Locator) => p.getByRole('listitem');
const noOverflow = (p: Page) => p.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth);
/** Plus aucun envoi en cours : les lignes confirmées sont redessinées, on attend qu'elles aient fini. */
const settled = (p: Page) => expect(p.locator('article[data-pending]')).toHaveCount(0);

/** La couleur calculée d'un jeton du thème. */
const token = (page: Page, name: string) =>
  page.evaluate((n) => {
    const probe = document.createElement('i');
    probe.style.color = `var(${n})`;
    document.body.append(probe);
    const c = getComputedStyle(probe).color;
    probe.remove();
    return c;
  }, name);

test('pins-H1 la barre d’outils épingle et désépingle, le repère suit, un simple membre n’a pas le bouton', async ({ user, api }) => {
  const a = await user('chef');
  const b = await user('lec');
  const [, cid] = await together(a, b, 'Atelier des épingles');
  const id = await post(await api(a.token), cid, 'Règle numéro un');
  const mine = await sent(a.page, 'Règle numéro un');
  await expect(await sent(b.page, 'Règle numéro un')).toBeVisible();

  // Le bouton vient après « Répondre » et avant « Modifier » et « Supprimer » (le danger reste en dernier).
  await mine.hover();
  expect(await actions(mine)).toEqual(['Ajouter une réaction', 'Répondre', 'Épingler le message', 'Modifier le message', 'Supprimer le message']);
  // Le propriétaire de serveur ne voit pas de repère tant que rien n'est épinglé.
  await expect(mine.getByText('Épinglé', { exact: true })).toHaveCount(0);

  await toolbar(mine).getByRole('button', { name: 'Épingler le message', exact: true }).click();
  await expect(said(a.page, 'Message épinglé.')).toBeVisible();
  // Le repère : une icône dans l'en-tête, à côté de l'heure, dite aux lecteurs d'écran.
  const pinned = await sent(a.page, 'Règle numéro un');
  const mark = pinned.locator('header .app-pinned');
  await expect(mark).toHaveCount(1);
  await expect(mark.getByText('Épinglé', { exact: true })).toHaveCount(1);
  const icon = await mark.locator('svg').boundingBox();
  expect([Math.round(icon!.width), Math.round(icon!.height)]).toEqual([16, 16]);
  await expect(mark).toHaveCSS('color', await token(a.page, '--text-muted'));
  // Le libellé change (pas d'aria-pressed) ; le clavier garde sa place : le focus revient sur le message.
  await expect(pinned).toBeFocused();
  await expect(toolbar(pinned).getByRole('button', { name: 'Désépingler le message', exact: true })).toHaveCount(1);
  await expect(toolbar(pinned).getByRole('button', { name: 'Désépingler le message', exact: true })).not.toHaveAttribute('aria-pressed', /.*/);
  // L'autre le voit en direct, mais n'a pas le bouton : il est absent, pas grisé.
  const theirs = await sent(b.page, 'Règle numéro un');
  await expect(theirs.locator('header .app-pinned')).toHaveCount(1);
  await theirs.hover();
  expect(await actions(theirs)).toEqual(['Ajouter une réaction', 'Répondre']);
  const refused = await (await api(b.token)).put(`${API}/channels/${cid}/pins/${id}`);
  expect(refused.status()).toBe(403);

  // Modifier le texte garde l'épingle.
  await act(a.page, 'Règle numéro un', 'Modifier le message');
  const box = a.page.getByRole('textbox', { name: 'Modifier le message' });
  await box.fill('Règle numéro un, corrigée');
  await box.press('Enter');
  const edited = await sent(a.page, 'corrigée');
  await expect(edited.locator('header .app-pinned')).toHaveCount(1);

  // Désépingler : le repère part chez les deux, le status le dit.
  await act(a.page, 'corrigée', 'Désépingler le message');
  await expect(said(a.page, 'Message désépinglé.')).toBeVisible();
  await expect(edited.locator('header .app-pinned')).toHaveCount(0);
  await expect(messages(b.page).filter({ hasText: 'corrigée' }).locator('header .app-pinned')).toHaveCount(0);
  await settled(a.page);
  await a11y(a.page);
});

test('pins-H2 le panneau : états vide, chargement et erreur, liste récente d’abord, focus, Échap, temps réel et droits', async ({ user, api }) => {
  const a = await user('chef');
  const b = await user('lec');
  const [, cid] = await together(a, b, 'Atelier du panneau');
  const asA = await api(a.token);
  const one = await post(asA, cid, 'Premier message à retenir');
  const two = await post(asA, cid, 'Deuxième message à retenir');
  await sent(a.page, 'Deuxième message');
  await sent(b.page, 'Deuxième message');

  // Vide : une icône, un titre, et de quoi faire ; sans le droit, une autre phrase.
  const ba = opener(a.page);
  await expect(ba).toHaveAttribute('aria-expanded', 'false');
  await expect(ba).not.toHaveAttribute('aria-controls', /.+/);
  await ba.click();
  await expect(ba).toHaveAttribute('aria-expanded', 'true');
  await expect(ba).toHaveAttribute('aria-controls', 'pins-panel');
  const pa = panel(a.page);
  await expect(pa.getByRole('heading', { name: 'Messages épinglés', level: 2 })).toBeFocused();
  await expect(pa.getByText('Aucun message épinglé')).toBeVisible();
  await expect(pa.getByText('Épingle un message depuis ses actions pour le retrouver ici.')).toBeVisible();
  await expect(pa.getByText('0 / 50')).toBeVisible();
  await opener(b.page).click();
  const pb = panel(b.page);
  await expect(pb.getByText('Aucun message épinglé')).toBeVisible();
  await expect(pb.getByText('Les modérateurs peuvent en épingler.')).toBeVisible();
  await expect(pb.getByText('Épingle un message depuis ses actions')).toHaveCount(0);
  await a11y(a.page);

  // Épingler en direct : les deux panneaux ouverts reçoivent la ligne, le plus récent d'abord.
  const pinPath = (id: string) => `${API}/channels/${cid}/pins/${id}`;
  expect((await asA.put(pinPath(one))).status()).toBe(200);
  await expect(rows(pb)).toHaveCount(1);
  expect((await asA.put(pinPath(two))).status()).toBe(200);
  await expect(rows(pa)).toHaveCount(2);
  await expect(rows(pb)).toHaveCount(2);
  await expect(rows(pa).nth(0)).toContainText('Deuxième message à retenir');
  await expect(rows(pa).nth(1)).toContainText('Premier message à retenir');
  await expect(pa.getByText('2 / 50')).toBeVisible();
  await expect(rows(pa).nth(0)).toContainText(a.name);
  // Un simple membre : « Aller au message », pas « Désépingler ».
  await expect(rows(pb).nth(0).getByRole('button', { name: `Aller au message de ${a.name}` })).toBeVisible();
  await expect(pb.getByRole('button', { name: /Désépingler/ })).toHaveCount(0);
  await expect(rows(pa).nth(0).getByRole('button', { name: `Désépingler le message de ${a.name}` })).toBeVisible();
  await settled(a.page);
  await a11y(a.page);

  // Une modification du texte arrive dans la ligne.
  expect((await asA.patch(`${API}/channels/${cid}/messages/${one}`, { data: { content: 'Premier message, corrigé' } })).status()).toBe(200);
  await expect(rows(pb).nth(1)).toContainText('Premier message, corrigé');
  await expect(pb.getByText('2 / 50')).toBeVisible();

  // Désépingler depuis le panneau : la ligne part chez les deux, le focus passe à la suivante.
  await rows(pa).nth(0).getByRole('button', { name: /^Désépingler/ }).click();
  await expect(said(a.page, 'Message désépinglé.')).toBeVisible();
  await expect(rows(pa)).toHaveCount(1);
  await expect(rows(pb)).toHaveCount(1);
  await expect(rows(pa).nth(0).getByRole('button', { name: /^Aller au message/ })).toBeFocused();
  // La dernière : le focus va au titre.
  await rows(pa).nth(0).getByRole('button', { name: /^Désépingler/ }).click();
  await expect(pa.getByText('Aucun message épinglé')).toBeVisible();
  await expect(pa.getByRole('heading', { name: 'Messages épinglés', level: 2 })).toBeFocused();
  // Le message lui-même ne porte plus le repère.
  await expect(messages(a.page).filter({ hasText: 'Premier message' }).locator('header .app-pinned')).toHaveCount(0);

  // Supprimer un message épinglé le retire du panneau d'un autre ; son focus passe à la ligne suivante, ou au titre.
  expect((await asA.put(pinPath(one))).status()).toBe(200);
  expect((await asA.put(pinPath(two))).status()).toBe(200);
  await expect(rows(pb)).toHaveCount(2);
  await rows(pb).nth(0).getByRole('button', { name: /^Aller au message/ }).focus();
  expect((await asA.delete(`${API}/channels/${cid}/messages/${two}`)).status()).toBe(204);
  await expect(rows(pb)).toHaveCount(1);
  await expect(pb.getByText('1 / 50')).toBeVisible();
  await expect(rows(pb).nth(0).getByRole('button', { name: /^Aller au message/ })).toBeFocused();
  await rows(pb).nth(0).getByRole('button', { name: /^Aller au message/ }).focus();
  expect((await asA.delete(pinPath(one))).status()).toBe(200);
  await expect(pb.getByText('Aucun message épinglé')).toBeVisible();
  await expect(pb.getByRole('heading', { name: 'Messages épinglés', level: 2 })).toBeFocused();

  // Échap ferme et rend le focus au bouton ; le bouton ferme aussi, et rouvre sur une liste relue.
  await a.page.keyboard.press('Escape');
  await expect(pa).toHaveCount(0);
  await expect(ba).toBeFocused();
  await expect(ba).toHaveAttribute('aria-expanded', 'false');
  await ba.click();
  await expect(panel(a.page)).toBeVisible();
  await ba.click();
  await expect(panel(a.page)).toHaveCount(0);
  // L'un ou l'autre : ouvrir les épingles ferme la liste des membres.
  const members = a.page.getByRole('button', { name: 'Liste des membres' });
  await members.click();
  await expect(a.page.getByRole('complementary', { name: 'Membres' })).toBeVisible();
  await ba.click();
  await expect(a.page.getByRole('complementary', { name: 'Membres' })).toHaveCount(0);
  await expect(panel(a.page)).toBeVisible();
  await members.click();
  await expect(panel(a.page)).toHaveCount(0);
  await expect(a.page.getByRole('complementary', { name: 'Membres' })).toBeVisible();
});

test('pins-H3 chargement sans saut puis erreur avec « Réessayer »', async ({ user, api, allow }) => {
  allow.push(/status of 403/);
  const a = await user('chef');
  const [, cid] = await (async () => {
    const [gid, cid] = await createGuild(a.page, 'Atelier des états');
    return [gid, cid];
  })();
  const asA = await api(a.token);
  const id = await post(asA, cid, 'Un message sous réserve');
  expect((await asA.put(`${API}/channels/${cid}/pins/${id}`)).status()).toBe(200);
  await sent(a.page, 'Un message sous réserve');

  // Chargement : la ligne status est réservée (même hauteur avant et après), la zone est occupée.
  let release!: () => void;
  const held = new Promise<void>((r) => (release = r));
  await a.page.route(`**/channels/${cid}/pins`, async (route) => {
    await held;
    await route.continue();
  });
  await opener(a.page).click();
  const pa = panel(a.page);
  const status = pa.getByRole('status');
  await expect(status).toHaveText('Chargement des messages épinglés…');
  await expect(pa.locator('.app-pins-body')).toHaveAttribute('aria-busy', 'true');
  const during = (await status.boundingBox())!;
  await a11y(a.page);
  release();
  await expect(rows(pa)).toHaveCount(1);
  await expect(pa.locator('.app-pins-body')).toHaveAttribute('aria-busy', 'false');
  await expect(status).not.toHaveText('Chargement des messages épinglés…');
  const after = (await status.boundingBox())!;
  expect([Math.round(after.y), Math.round(after.height)]).toEqual([Math.round(during.y), Math.round(during.height)]);
  await a.page.unroute(`**/channels/${cid}/pins`);

  // Erreur : un message en role alert et « Réessayer », qui relit la liste.
  await a.page.keyboard.press('Escape');
  await a.page.route(`**/channels/${cid}/pins`, (route) => route.fulfill({ status: 403, contentType: 'application/json', body: JSON.stringify({ code: 'forbidden', message: 'Tu n’as pas la permission de faire ça.' }) }));
  await opener(a.page).click();
  const failure = panel(a.page).getByRole('alert');
  await expect(failure).toContainText('Impossible de charger les messages épinglés');
  await expect(failure).toContainText('Tu n’as pas la permission de faire ça.');
  await a11y(a.page);
  await a.page.unroute(`**/channels/${cid}/pins`);
  await failure.getByRole('button', { name: 'Réessayer' }).click();
  await expect(rows(panel(a.page))).toHaveCount(1);
  await expect(panel(a.page).getByRole('heading', { name: 'Messages épinglés', level: 2 })).toBeFocused();
});

test('pins-H4 « Aller au message » charge l’ancien, défile, met le contour 2 s et prend le focus', async ({ user, api }) => {
  const a = await user('chef');
  const [, cid] = await (async () => createGuild(a.page, 'Atelier du saut'))();
  const asA = await api(a.token);
  const old = await post(asA, cid, 'Le tout premier message épinglé');
  expect((await asA.put(`${API}/channels/${cid}/pins/${old}`)).status()).toBe(200);
  // Assez de messages derrière lui pour qu'il ne soit plus dans la première page de l'historique (50).
  for (let i = 0; i < 130; i++) await post(asA, cid, `remplissage ${i}`);
  await a.page.reload();
  await expect(a.page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
  await expect(messages(a.page).filter({ hasText: 'remplissage 129' })).toHaveCount(1);
  await expect(messages(a.page).filter({ hasText: 'Le tout premier' })).toHaveCount(0);

  await opener(a.page).click();
  const pa = panel(a.page);
  await expect(rows(pa)).toHaveCount(1);
  await rows(pa).getByRole('button', { name: `Aller au message de ${a.name}` }).click();
  const target = a.page.locator(`#msg-${old}`);
  await expect(target).toBeFocused();
  await expect(target).toHaveAttribute('data-flash', '');
  await expect(target).toHaveCSS('outline-width', '2px');
  await expect(target).toBeInViewport();
  // Le contour part après 2 secondes.
  await expect(target).not.toHaveAttribute('data-flash', '', { timeout: 4_000 });
  // Le panneau reste ouvert à côté du fil (grand écran).
  await expect(pa).toBeVisible();

  // Un message déjà affiché : même chose, sans rien charger.
  const recent = await post(asA, cid, 'Un message récent épinglé');
  expect((await asA.put(`${API}/channels/${cid}/pins/${recent}`)).status()).toBe(200);
  await expect(rows(pa)).toHaveCount(2);
  await rows(pa).nth(0).getByRole('button', { name: /^Aller au message/ }).click();
  await expect(a.page.locator(`#msg-${recent}`)).toBeFocused();
});

test('pins-H5 mouvement réduit : le contour reste, sans animation', async ({ api, browser, baseURL }) => {
  const context = await browser.newContext({ baseURL, locale: 'fr-FR', reducedMotion: 'reduce', viewport: { width: 1440, height: 900 } });
  const page = await context.newPage();
  const name = `rm_${Date.now().toString(36).slice(-6)}`;
  const reg = await context.request.post(`${API}/auth/register`, { data: { username: name, email: `${name}@example.org`, password: 'motdepasse-solide' } });
  const { token } = await reg.json();
  await page.goto('/login');
  await page.evaluate(() => localStorage.setItem('kraken.signed_in', '1'));
  await page.goto('/channels/@me');
  const [, cid] = await createGuild(page, 'Atelier du mouvement réduit');
  const asA = await api(token);
  const id = await post(asA, cid, 'Un message au contour');
  expect((await asA.put(`${API}/channels/${cid}/pins/${id}`)).status()).toBe(200);
  await opener(page).click();
  await rows(panel(page)).getByRole('button', { name: /^Aller au message/ }).click();
  const target = page.locator(`#msg-${id}`);
  await expect(target).toHaveAttribute('data-flash', '');
  const motion = await target.evaluate((e) => {
    const c = getComputedStyle(e);
    return { animation: c.animationName, transition: c.transitionDuration, outline: c.outlineWidth };
  });
  expect(motion.animation).toBe('none');
  expect(motion.outline).toBe('2px');
  await context.close();
});

test('pins-H6 la limite de 50 : le bouton reste actif et l’erreur est lue', async ({ user, api, allow }) => {
  allow.push(/status of 400/);
  const a = await user('chef');
  const [, cid] = await (async () => createGuild(a.page, 'Atelier de la limite'))();
  const asA = await api(a.token);
  const ids: string[] = [];
  for (let i = 0; i < 51; i++) ids.push(await post(asA, cid, `message ${String(i).padStart(2, '0')} de la pile`));
  for (const id of ids.slice(0, 50)) expect((await asA.put(`${API}/channels/${cid}/pins/${id}`)).status()).toBe(200);
  await expect(messages(a.page).filter({ hasText: 'message 50 de la pile' })).toHaveCount(1);

  // Depuis la barre d'outils : l'alerte dit pourquoi, rien n'est grisé, rien n'est épinglé.
  const last = await sent(a.page, 'message 50 de la pile');
  await last.hover();
  const pin = toolbar(last).getByRole('button', { name: 'Épingler le message', exact: true });
  await expect(pin).toBeEnabled();
  await pin.click();
  const alert = a.page.getByRole('alert').filter({ hasText: 'Ce salon a déjà 50 messages épinglés : désépingles-en un d\'abord.' });
  await expect(alert).toBeVisible();
  await expect(a.page.getByText('Message épinglé.')).toHaveCount(0);
  await expect(last.locator('header .app-pinned')).toHaveCount(0);
  await last.hover();
  await expect(toolbar(last).getByRole('button', { name: 'Épingler le message', exact: true })).toBeEnabled();
  // Le panneau compte jusqu'à 50.
  await opener(a.page).click();
  await expect(rows(panel(a.page))).toHaveCount(50);
  await expect(panel(a.page).getByText('50 / 50')).toBeVisible();
  // Désépingler une ligne libère une place : la barre d'outils réussit.
  await rows(panel(a.page)).nth(49).getByRole('button', { name: /^Désépingler/ }).click();
  await expect(rows(panel(a.page))).toHaveCount(49);
  await last.hover();
  await toolbar(last).getByRole('button', { name: 'Épingler le message', exact: true }).click();
  await expect(rows(panel(a.page))).toHaveCount(50);
  await expect(rows(panel(a.page)).nth(0)).toContainText('message 50 de la pile');
});

test('pins-H7 un rôle « Gérer les messages » donne le bouton, le retirer le reprend', async ({ user, api }) => {
  const a = await user('chef');
  const b = await user('lec');
  const [gid, cid] = await together(a, b, 'Atelier des rôles');
  const asA = await api(a.token);
  await post(asA, cid, 'Un message du chef');
  const theirs = await sent(b.page, 'Un message du chef');
  await theirs.hover();
  expect(await actions(theirs)).toEqual(['Ajouter une réaction', 'Répondre']);

  const role = await (await asA.post(`${API}/guilds/${gid}/roles`, { data: { name: 'Gérants', permissions: 1 << 8 } })).json();
  expect((await asA.put(`${API}/guilds/${gid}/members/${b.id}/roles/${role.id}`)).status()).toBe(200);
  // Le droit arrive par le gateway : le bouton apparaît sans recharger la page.
  await theirs.hover();
  await expect(toolbar(theirs).getByRole('button', { name: 'Épingler le message', exact: true })).toBeVisible();
  await toolbar(theirs).getByRole('button', { name: 'Épingler le message', exact: true }).click();
  await expect(said(b.page, 'Message épinglé.')).toBeVisible();
  await expect(messages(a.page).filter({ hasText: 'Un message du chef' }).locator('header .app-pinned')).toHaveCount(1);
  // Le panneau montre « Désépingler » à celui qui a le droit.
  await opener(b.page).click();
  await expect(rows(panel(b.page)).getByRole('button', { name: /^Désépingler/ })).toHaveCount(1);
});

test('pins-H8 en MP, chacun des deux épingle et désépingle, sans journal ; un groupe aussi', async ({ user, api }) => {
  const a = await user('alix');
  const b = await user('bruno');
  const c = await user('chloe');
  await befriend(api, a, b);
  await befriend(api, a, c);
  const asA = await api(a.token);
  const asB = await api(b.token);
  const dm = await (await asA.post(`${API}/users/@me/channels`, { data: { recipients: [b.id] } })).json();
  await post(asA, dm.id, 'Le code est 4242');
  await post(asB, dm.id, 'Merci, je note');
  await a.page.goto(`/channels/@me/${dm.id}`);
  await b.page.goto(`/channels/@me/${dm.id}`);
  const mineA = await sent(a.page, 'Le code est 4242');
  const mineB = await sent(b.page, 'Le code est 4242');

  // Le message de l'autre : bruno épingle celui d'alix, et lui-même (les deux ont le bouton).
  await mineB.hover();
  expect(await actions(mineB)).toEqual(['Ajouter une réaction', 'Répondre', 'Épingler le message']);
  await toolbar(mineB).getByRole('button', { name: 'Épingler le message', exact: true }).click();
  await expect(said(b.page, 'Message épinglé.')).toBeVisible();
  await expect(mineA.locator('header .app-pinned')).toHaveCount(1);
  // Le panneau : même phrase d'aide pour les deux (tous les deux peuvent), désépingler pour les deux.
  await opener(a.page).click();
  const pa = panel(a.page);
  await expect(rows(pa)).toHaveCount(1);
  await expect(rows(pa).getByRole('button', { name: /^Désépingler/ })).toHaveCount(1);
  await rows(pa).getByRole('button', { name: /^Désépingler/ }).click();
  await expect(said(a.page, 'Message désépinglé.')).toBeVisible();
  await expect(pa.getByText('Épingle un message depuis ses actions pour le retrouver ici.')).toBeVisible();
  await expect(mineB.locator('header .app-pinned')).toHaveCount(0);
  await settled(a.page);
  await a11y(a.page);

  // Un groupe : un participant quelconque.
  const group = await (await asA.post(`${API}/users/@me/channels`, { data: { recipients: [b.id, c.id], name: 'Les trois' } })).json();
  await post(asA, group.id, 'Rendez-vous samedi');
  await c.page.goto(`/channels/@me/${group.id}`);
  const inGroup = await sent(c.page, 'Rendez-vous samedi');
  await inGroup.hover();
  await toolbar(inGroup).getByRole('button', { name: 'Épingler le message', exact: true }).click();
  await expect(said(c.page, 'Message épinglé.')).toBeVisible();
  await expect(inGroup.locator('header .app-pinned')).toHaveCount(1);
});

test('pins-H9 au clavier : barre d’outils, bouton d’en-tête, panneau, Tab et Échap', async ({ user, api }) => {
  const a = await user('chef');
  const [, cid] = await (async () => createGuild(a.page, 'Atelier du clavier'))();
  const asA = await api(a.token);
  await post(asA, cid, 'Un message au clavier');
  const m = await sent(a.page, 'Un message au clavier');

  // Le message prend le focus : sa barre d'outils s'affiche, le bouton s'atteint puis s'active à Entrée.
  await m.focus();
  const pin = toolbar(m).getByRole('button', { name: 'Épingler le message', exact: true });
  await pin.focus();
  await a.page.keyboard.press('Enter');
  await expect(said(a.page, 'Message épinglé.')).toBeVisible();
  await expect(m.locator('header .app-pinned')).toHaveCount(1);
  // Le focus revient sur le message (la ligne a été redessinée), pas sur le corps de la page.
  await expect(m).toBeFocused();

  // Le bouton d'en-tête s'ouvre à Entrée ; le focus va au titre, Tab au bouton « Fermer », puis à la ligne.
  await opener(a.page).focus();
  await a.page.keyboard.press('Enter');
  const pa = panel(a.page);
  const title = pa.getByRole('heading', { name: 'Messages épinglés', level: 2 });
  await expect(title).toBeFocused();
  await a.page.keyboard.press('Tab');
  await expect(pa.getByRole('button', { name: 'Fermer les messages épinglés' })).toBeFocused();
  await a.page.keyboard.press('Tab');
  await expect(rows(pa).getByRole('button', { name: /^Aller au message/ })).toBeFocused();
  await a.page.keyboard.press('Tab');
  await expect(rows(pa).getByRole('button', { name: /^Désépingler/ })).toBeFocused();
  // « Aller au message » au clavier : le message prend le focus.
  await rows(pa).getByRole('button', { name: /^Aller au message/ }).focus();
  await a.page.keyboard.press('Enter');
  await expect(m).toBeFocused();
  // Échap depuis le panneau le ferme et rend le focus au bouton.
  await rows(pa).getByRole('button', { name: /^Désépingler/ }).focus();
  await a.page.keyboard.press('Escape');
  await expect(pa).toHaveCount(0);
  await expect(opener(a.page)).toBeFocused();
  // Rouvrir au clavier, fermer par le bouton « Fermer » : même retour.
  await a.page.keyboard.press('Enter');
  await expect(panel(a.page)).toBeVisible();
  await panel(a.page).getByRole('button', { name: 'Fermer les messages épinglés' }).click();
  await expect(panel(a.page)).toHaveCount(0);
  await expect(opener(a.page)).toBeFocused();
});

test('pins-H10 le journal d’audit écrit « a épinglé un message dans #salon », une seule fois', async ({ user, api }) => {
  const a = await user('chef');
  const [gid, cid] = await (async () => createGuild(a.page, 'Atelier du journal'))();
  const asA = await api(a.token);
  const id = await post(asA, cid, 'Un secret très secret');
  expect((await asA.put(`${API}/channels/${cid}/pins/${id}`)).status()).toBe(200);
  expect((await asA.put(`${API}/channels/${cid}/pins/${id}`)).status()).toBe(200);
  expect((await asA.delete(`${API}/channels/${cid}/pins/${id}`)).status()).toBe(200);
  expect((await asA.delete(`${API}/channels/${cid}/pins/${id}`)).status()).toBe(200);

  await a.page.goto(`/guilds/${gid}/settings/audit`);
  const log = a.page.getByRole('dialog', { name: 'Paramètres du serveur' });
  const list = log.getByRole('list', { name: 'Événements du journal' });
  await expect(list.getByRole('listitem').filter({ hasText: 'a épinglé un message dans #général' })).toHaveCount(1);
  await expect(list.getByRole('listitem').filter({ hasText: 'a désépinglé un message dans #général' })).toHaveCount(1);
  await expect(list).not.toContainText('secret');
});

test.describe('à 390 px', () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test('pins-M1 l’en-tête garde des cibles de 44 px sans déborder, la feuille est pleine page, fermable, et mène au message', async ({ user, api }) => {
    const a = await user('chef');
    const asA = await api(a.token);
    const made = await (await asA.post(`${API}/guilds`, { data: { name: 'Abysse' } })).json();
    const gid: string = made.guild.id;
    const cid: string = made.channels.find((c: { kind: string }) => c.kind === 'text').id;
    const first = await post(asA, cid, 'Un message à retrouver');
    expect((await asA.put(`${API}/channels/${cid}/pins/${first}`)).status()).toBe(200);
    for (let i = 0; i < 25; i++) await post(asA, cid, `remplissage ${i}`);
    await a.page.goto(`/channels/${gid}/${cid}`);
    await expect(a.page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
    await settled(a.page);

    // Chaque bouton de l'en-tête : 44 px au moins, dans la page, sans défilement horizontal.
    const tools = a.page.locator('.app-header-tools').locator('a, button');
    expect(await tools.count()).toBe(6);
    for (const box of await tools.evaluateAll((els) => els.map((e) => { const r = e.getBoundingClientRect(); return { x: r.x, w: r.width, h: r.height, right: r.right }; }))) {
      expect(box.w).toBeGreaterThanOrEqual(44);
      expect(box.h).toBeGreaterThanOrEqual(44);
      expect(box.x).toBeGreaterThanOrEqual(0);
      expect(box.right).toBeLessThanOrEqual(390);
    }
    // Le titre reste lisible, sur sa ligne.
    await expect(a.page.getByRole('heading', { name: 'général', level: 1 })).toBeInViewport();
    expect(await noOverflow(a.page)).toBe(true);
    await a11y(a.page);

    // L'ordre : épingles avant membres.
    const names = await tools.evaluateAll((els) => els.map((e) => e.getAttribute('aria-label')));
    expect(names.indexOf('Messages épinglés')).toBeGreaterThan(-1);
    expect(names.indexOf('Messages épinglés')).toBeLessThan(names.indexOf('Liste des membres'));

    // La feuille : pleine page, dans un dialogue, avec sa fermeture visible.
    await opener(a.page).click();
    const s = sheet(a.page);
    await expect(s).toBeVisible();
    const frame = (await s.boundingBox())!;
    expect([Math.round(frame.x), Math.round(frame.y), Math.round(frame.width), Math.round(frame.height)]).toEqual([0, 0, 390, 844]);
    await expect(s.getByRole('heading', { name: 'Messages épinglés', level: 2 })).toBeFocused();
    const close = s.getByRole('button', { name: 'Fermer les messages épinglés' });
    await expect(close).toBeVisible();
    const cb = (await close.boundingBox())!;
    expect(cb.width).toBeGreaterThanOrEqual(44);
    expect(cb.height).toBeGreaterThanOrEqual(44);
    await expect(rows(s)).toHaveCount(1);
    for (const b of await rows(s).getByRole('button').all()) expect((await b.boundingBox())!.height).toBeGreaterThanOrEqual(44);
    expect(await noOverflow(a.page)).toBe(true);
    await settled(a.page);
    await a11y(a.page);
    // La page de dessous n'est plus au clavier : Tab reste dans la feuille.
    await a.page.keyboard.press('Shift+Tab');
    expect(await s.evaluate((el) => el.contains(document.activeElement))).toBe(true);

    // Échap ferme et rend le focus au bouton.
    await a.page.keyboard.press('Escape');
    await expect(s).toHaveCount(0);
    await expect(opener(a.page)).toBeFocused();

    // « Aller au message » ferme la feuille, défile jusqu'au message, lui donne le focus et le contour.
    await opener(a.page).click();
    await rows(sheet(a.page)).getByRole('button', { name: /^Aller au message/ }).click();
    await expect(sheet(a.page)).toHaveCount(0);
    const target = a.page.locator(`#msg-${first}`);
    await expect(target).toBeFocused();
    await expect(target).toHaveAttribute('data-flash', '');
    await expect(target).toBeInViewport();
    expect(await noOverflow(a.page)).toBe(true);

    // Le bouton « Fermer » ferme aussi.
    await opener(a.page).click();
    await sheet(a.page).getByRole('button', { name: 'Fermer les messages épinglés' }).click();
    await expect(sheet(a.page)).toHaveCount(0);
    await expect(opener(a.page)).toBeFocused();
  });

  test('pins-M2 un MP et un groupe : l’en-tête ne déborde pas, cibles de 44 px', async ({ user, api }) => {
    const a = await user('alix');
    const b = await user('bruno');
    const c = await user('chloe');
    await befriend(api, a, b);
    await befriend(api, a, c);
    const asA = await api(a.token);
    const dm = await (await asA.post(`${API}/users/@me/channels`, { data: { recipients: [b.id] } })).json();
    const group = await (await asA.post(`${API}/users/@me/channels`, { data: { recipients: [b.id, c.id], name: 'Les trois' } })).json();
    // Un MP entre amis : appel, groupe, notifications, recherche, boîte, plus les épingles.
    for (const [channel, count] of [[dm, 6], [group, 9]] as const) {
      await a.page.goto(`/channels/@me/${channel.id}`);
      await expect(a.page.getByRole('heading', { level: 1 })).toBeVisible();
      const tools = a.page.locator('.app-header-tools').locator('a, button');
      await expect(tools).toHaveCount(count);
      for (const box of await tools.evaluateAll((els) => els.map((e) => { const r = e.getBoundingClientRect(); return { x: r.x, w: r.width, h: r.height, right: r.right }; }))) {
        expect(box.w).toBeGreaterThanOrEqual(44);
        expect(box.h).toBeGreaterThanOrEqual(44);
        expect(box.x).toBeGreaterThanOrEqual(0);
        expect(box.right).toBeLessThanOrEqual(390);
      }
      expect(await noOverflow(a.page)).toBe(true);
      await opener(a.page).click();
      await expect(sheet(a.page).getByText('Aucun message épinglé')).toBeVisible();
      expect(await noOverflow(a.page)).toBe(true);
      await a.page.keyboard.press('Escape');
      await expect(sheet(a.page)).toHaveCount(0);
    }
  });
});
