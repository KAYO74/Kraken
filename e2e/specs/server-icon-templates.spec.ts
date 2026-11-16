// Créer un serveur à partir d'un modèle ; icône du serveur (envoi, rail, retrait) ; renommer. Les images sont
// fabriquées sur place (e2e/images.ts), rien de binaire n'est dans le dépôt.
import { test, expect, a11y, createGuild, API, type User } from '../fixtures';
import { png, animatedGif } from '../images';
import type { APIRequestContext, Page } from '@playwright/test';

type Api = (token?: string) => Promise<APIRequestContext>;
type Chan = { name: string; kind: string; parent_id: string | null; position: number };

const settings = (p: Page) => p.getByRole('dialog', { name: 'Paramètres du serveur' });
const rail = (p: Page, name: string) => p.getByRole('navigation', { name: 'Serveurs' }).getByRole('link', { name: `Serveur ${name}` });
const styleOf = (l: ReturnType<Page['locator']>, prop: string) => l.evaluate((e, p) => getComputedStyle(e).getPropertyValue(p), prop);

async function invite(api: Api, owner: User, cid: string, who: User) {
  const { code } = await (await (await api(owner.token)).post(`${API}/channels/${cid}/invites`, { data: {} })).json();
  expect((await (await api(who.token)).post(`${API}/invites/${code}`)).status()).toBe(200);
}

/** La structure d'un serveur : chaque catégorie suivie de ses salons, dans l'ordre. */
async function structure(api: Api, u: User, gid: string): Promise<[string, string][]> {
  const g = await (await (await api(u.token)).get(`${API}/guilds/${gid}`)).json();
  const all: (Chan & { id: string })[] = g.channels;
  const order = (a: Chan, b: Chan) => a.position - b.position;
  const out: [string, string][] = [];
  const kids = (parent: string | null) => all.filter((c) => c.kind !== 'category' && c.parent_id === parent).sort(order);
  for (const c of kids(null)) out.push([c.kind, c.name]);
  for (const cat of all.filter((c) => c.kind === 'category').sort(order)) {
    out.push(['category', cat.name]);
    for (const c of all.filter((x) => x.parent_id === cat.id).sort(order)) out.push([c.kind, c.name]);
  }
  return out;
}

const iconKey = async (api: Api, u: User, gid: string) => (await (await (await api(u.token)).get(`${API}/guilds/${gid}`)).json()).guild.icon as string | null;

test('modeles-H1 créer un serveur « Études » au clavier : les salons du modèle, et le premier s’ouvre', async ({ user, api }) => {
  const a = await user('lise');
  await a.page.getByRole('link', { name: 'Créer ou rejoindre un serveur' }).click();
  await expect(a.page.getByRole('heading', { name: 'Crée ton serveur', level: 1 })).toBeVisible();
  const group = a.page.getByRole('radiogroup', { name: 'Modèle' });
  await expect(group.getByRole('radio')).toHaveCount(5);
  await expect(group.getByRole('radio', { name: /Vide/ })).toBeChecked();
  await expect(a.page.getByRole('radio', { name: /Créateurs/ })).toHaveCount(0);
  await a11y(a.page);

  // Le nom est proposé d'après le pseudo ; la liste « Sera créé » suit le choix. Flèches : une seule étape de Tab.
  await expect(a.page.getByLabel('Nom du serveur')).toHaveValue(new RegExp(`^Serveur de `));
  await group.getByRole('radio', { name: /Vide/ }).focus();
  for (let i = 0; i < 3; i++) await a.page.keyboard.press('ArrowRight');
  const etudes = group.getByRole('radio', { name: /Études/ });
  await expect(etudes).toBeChecked();
  await expect(etudes).toBeFocused();
  const made = a.page.locator('.app-ng-made');
  await expect(made).toContainText('Sera créé :');
  await expect(made.getByRole('listitem')).toHaveCount(7);
  await expect(made).toContainText('devoirs');
  await expect(made).toContainText('Salle de travail');
  await expect(group.locator('[tabindex="0"], input:not([tabindex])')).toHaveCount(5); // natifs : le navigateur n'en garde qu'un dans l'ordre de Tab
  await a11y(a.page);

  await a.page.getByLabel('Nom du serveur').fill('Révisions du soir');
  await a.page.getByRole('button', { name: 'Créer le serveur', exact: true }).click();
  await a.page.waitForURL(/\/channels\/[0-9a-f-]{36}\/[0-9a-f-]{36}$/);
  await expect(a.page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
  const gid = new URL(a.page.url()).pathname.split('/')[2];

  // Les salons du modèle, rangés par catégorie.
  expect(await structure(api, a, gid)).toEqual([
    ['category', 'Travail'],
    ['text', 'général'],
    ['text', 'cours'],
    ['text', 'devoirs'],
    ['text', 'ressources'],
    ['category', 'Salons vocaux'],
    ['voice', 'Salle de travail'],
    ['voice', 'Révisions en groupe'],
    ['voice', 'Pause'],
  ]);
  const side = a.page.getByRole('navigation', { name: 'Salons' });
  for (const n of ['général', 'cours', 'devoirs', 'ressources']) await expect(side.getByRole('link', { name: new RegExp(n) }).first()).toBeVisible();
  await expect(rail(a.page, 'Révisions du soir')).toBeVisible();
});

test('modeles-H2 le modèle par défaut est « Vide », et l’API refuse un modèle inconnu', async ({ user, api }) => {
  const a = await user('ines');
  const http = await api(a.token);
  const bad = await http.post(`${API}/guilds`, { data: { name: 'Quelque chose', template: 'n-importe-quoi' } });
  expect(bad.status()).toBe(400);
  // Tabulation, saut de ligne, U+2028 et NUL dans un nom : refusés avec un message clair, à la création comme au renommage.
  for (const name of ['Le\tRécif', 'Le\nRécif', 'Le\u2028Récif', 'Le\u0000Récif']) {
    const refused = await http.post(`${API}/guilds`, { data: { name } });
    expect(refused.status(), JSON.stringify(name)).toBe(400);
    expect((await refused.json()).message).toBe('Le nom du serveur contient un caractère interdit (tabulation, saut de ligne ou autre caractère de contrôle).');
  }
  const [gid] = await createGuild(a.page, 'Sans modèle choisi');
  const renamed = await http.patch(`${API}/guilds/${gid}`, { data: { name: 'Le\u0000Récif' } });
  expect(renamed.status()).toBe(400);
  expect((await renamed.json()).code).toBe('invalid_chars');
  expect(await structure(api, a, gid)).toEqual([
    ['category', 'Salons textuels'],
    ['text', 'général'],
    ['category', 'Salons vocaux'],
    ['voice', 'Général'],
  ]);
  const made = await http.post(`${API}/guilds`, { data: { name: 'Communauté test', template: 'communaute' } });
  expect(made.status()).toBe(201);
  const comm = (await made.json()).guild.id as string;
  expect((await structure(api, a, comm)).map((x) => x[1])).toEqual(['Accueil', 'annonces', 'règles', 'Discussions', 'général', 'aide', 'Salons vocaux', 'Discussion libre']);
});

test('modeles-E1 la page tient sur un téléphone : une colonne, pas de débordement, cibles de 44 px', async ({ user }) => {
  const a = await user('theo');
  await a.page.setViewportSize({ width: 390, height: 844 });
  await a.page.goto('/guilds/new');
  const cards = a.page.locator('.app-tpl');
  await expect(cards).toHaveCount(5);
  const boxes = await cards.evaluateAll((els) => els.map((e) => e.getBoundingClientRect().toJSON()));
  expect(new Set(boxes.map((b) => Math.round(b.left))).size).toBe(1);
  for (const b of boxes) expect(b.height).toBeGreaterThanOrEqual(44);
  expect(await a.page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)).toBeLessThanOrEqual(0);
  const create = a.page.getByRole('button', { name: 'Créer le serveur', exact: true });
  expect((await create.boundingBox())!.height).toBeGreaterThanOrEqual(44);
  await a11y(a.page);
  // Entre 640 et 1023 px : deux colonnes ; en large : trois.
  await a.page.setViewportSize({ width: 800, height: 900 });
  expect(new Set((await cards.evaluateAll((els) => els.map((e) => Math.round(e.getBoundingClientRect().left))))).size).toBe(2);
  await a.page.setViewportSize({ width: 1280, height: 900 });
  expect(new Set((await cards.evaluateAll((els) => els.map((e) => Math.round(e.getBoundingClientRect().left))))).size).toBe(3);
});

test('icone-H1 envoyer, voir, renommer, retirer : le rail et les membres suivent en direct, le journal en garde la trace', async ({ user, api, allow }) => {
  allow.push(/status of 400/); // le nom d'une lettre, refusé par le serveur
  const a = await user('ondine');
  const b = await user('maelle');
  const [gid, cid] = await createGuild(a.page, 'Récif');
  await invite(api, a, cid, b);
  await b.page.goto(`/channels/${gid}/${cid}`);
  await expect(rail(b.page, 'Récif')).toBeVisible();
  await expect(rail(a.page, 'Récif').locator('img')).toHaveCount(0);

  // Réglages : « Vue d'ensemble » est la première rubrique.
  await a.page.getByRole('link', { name: 'Paramètres du serveur' }).click();
  const dlg = settings(a.page);
  await expect(dlg.getByRole('navigation').getByRole('link').first()).toHaveText("Vue d'ensemble");
  await expect(dlg.getByRole('heading', { name: "Vue d'ensemble", level: 2 })).toBeVisible();
  await expect(dlg.getByRole('button', { name: "Retirer l'icône" })).toHaveCount(0);
  await a11y(a.page);

  // Une image fixe passe par le recadrage.
  await dlg.getByLabel('Choisir une image').setInputFiles({ name: 'recif.png', mimeType: 'image/png', buffer: png(300, 180) });
  const crop = a.page.getByRole('dialog', { name: "Recadrer l'icône" });
  await expect(crop).toBeVisible();
  await a11y(a.page);
  await crop.getByRole('button', { name: 'Appliquer' }).click();
  await expect(crop).toHaveCount(0);
  await expect(dlg.getByRole('status').filter({ hasText: 'Icône mise à jour' })).toBeVisible();
  const key = await iconKey(api, a, gid);
  expect(key).toMatch(/^[0-9a-f]{32}$/);

  // Le fichier est public, mis en cache pour longtemps, carré de 256 px.
  const served = await (await api()).get(`${API}/guild-icons/${key}`);
  expect(served.status()).toBe(200);
  expect(served.headers()['content-type']).toBe('image/png');
  expect(served.headers()['cache-control']).toContain('immutable');

  // Le rail : l'image remplace les initiales, alt vide ; chez l'autre membre aussi, sans recharger.
  for (const u of [a, b]) {
    const img = rail(u.page, 'Récif').locator('img');
    await expect(img).toHaveCount(1);
    await expect(img).toHaveAttribute('alt', '');
    await expect(img).toHaveAttribute('src', `${API}/guild-icons/${key}`);
    await expect.poll(() => img.evaluate((i: HTMLImageElement) => i.naturalWidth)).toBe(256);
  }
  await expect(dlg.getByRole('button', { name: "Retirer l'icône" })).toBeVisible();
  await a11y(a.page);

  // Renommer : le rail, le titre des réglages et l'autre membre suivent.
  const field = dlg.getByLabel('Nom du serveur');
  await expect(dlg.getByRole('button', { name: 'Enregistrer' })).toBeDisabled();
  // Les messages de réussite (un par bloc) ne restent pas à côté d'une erreur ni d'un message de l'autre bloc.
  const notes = dlg.locator('.app-ov-note');
  await expect(notes).toHaveText(['Icône mise à jour.', '']);
  await field.fill('R');
  await expect(notes).toHaveText(['', '']); // modifier le nom efface la note de l'icône
  await field.press('Enter');
  await expect(a.page.locator('#ov-name-err')).toBeVisible();
  await expect(notes).toHaveText(['', '']);
  await field.fill('Grand Récif');
  await field.press('Enter');
  await expect(dlg.getByRole('status').filter({ hasText: 'Nom du serveur enregistré' })).toBeVisible();
  await expect(notes).toHaveText(['', 'Nom du serveur enregistré.']);
  await expect(rail(a.page, 'Grand Récif')).toBeVisible();
  await expect(rail(b.page, 'Grand Récif')).toBeVisible();
  await expect(rail(b.page, 'Grand Récif').locator('img')).toHaveCount(1);

  // Retirer : retour aux initiales, le fichier n'est plus servi.
  await dlg.getByRole('button', { name: "Retirer l'icône" }).click();
  await expect(dlg.getByRole('status').filter({ hasText: 'Icône mise à jour' })).toBeVisible();
  await expect(notes).toHaveText(['Icône mise à jour.', '']); // l'action sur l'icône efface la note du nom
  await expect(rail(a.page, 'Grand Récif').locator('img')).toHaveCount(0);
  await expect(rail(b.page, 'Grand Récif').locator('img')).toHaveCount(0);
  await expect(rail(b.page, 'Grand Récif')).toContainText('GR');
  expect(await iconKey(api, a, gid)).toBeNull();
  expect((await (await api()).get(`${API}/guild-icons/${key}`)).status()).toBe(404);

  // Un fichier refusé : l'erreur, et plus aucune note de réussite à côté.
  await dlg.getByLabel('Choisir une image').setInputFiles({ name: 'notes.txt', mimeType: 'text/plain', buffer: Buffer.from('pas une image') });
  await expect(dlg.getByRole('alert').filter({ hasText: 'pas une image lisible' })).toBeVisible();
  await expect(notes).toHaveText(['', '']);

  // Le journal d'audit : qui a fait quoi.
  await dlg.getByRole('link', { name: "Journal d'audit" }).click();
  const rows = dlg.getByRole('list', { name: 'Événements du journal' }).getByRole('listitem');
  await expect(rows.filter({ hasText: "a retiré l'icône du serveur" })).toHaveCount(1);
  await expect(rows.filter({ hasText: "a changé l'icône du serveur" })).toHaveCount(1);
  await expect(rows.filter({ hasText: 'a renommé le serveur « Récif » en « Grand Récif »' })).toHaveCount(1);
  // Échap ferme les réglages, même après être passé d'une rubrique à l'autre.
  await a.page.keyboard.press('Escape');
  await expect(dlg).toHaveCount(0);
});

test('icone-H2 une animation reste fixe dans le rail, s’anime dans l’aperçu, et reste fixe en mouvement réduit', async ({ user, api }) => {
  const a = await user('flore');
  const [gid] = await createGuild(a.page, 'Méduses');
  await a.page.goto(`/guilds/${gid}/settings/overview`);
  const dlg = settings(a.page);
  // Une animation part telle quelle, sans recadrage.
  await dlg.getByLabel('Choisir une image').setInputFiles({ name: 'meduse.gif', mimeType: 'image/gif', buffer: animatedGif(3, 16) });
  await expect(dlg.getByRole('status').filter({ hasText: 'Icône mise à jour' })).toBeVisible();
  await expect(a.page.getByRole('dialog', { name: "Recadrer l'icône" })).toHaveCount(0);
  const key = await iconKey(api, a, gid);
  const served = await (await api()).get(`${API}/guild-icons/${key}`);
  expect(served.headers()['content-type']).toBe('image/gif');

  const preview = dlg.locator('.app-gicon-preview');
  await expect(preview.locator('img.k-live')).toHaveCount(1);
  expect(await styleOf(preview.locator('img.k-live'), 'display')).toBe('block');
  expect(await styleOf(preview.locator('img.k-still'), 'visibility')).toBe('hidden');
  expect(await preview.evaluate((e) => Math.round(e.getBoundingClientRect().width))).toBe(96);

  const tile = rail(a.page, 'Méduses');
  await expect(tile.locator('img.k-still')).toHaveCount(1);
  expect(await styleOf(tile.locator('img.k-live'), 'display')).toBe('none');
  // Les réglages couvrent le rail : on les ferme pour le survoler, puis on les rouvre.
  await a.page.keyboard.press('Escape');
  await expect(dlg).toHaveCount(0);
  await tile.hover();
  expect(await styleOf(tile.locator('img.k-live'), 'display')).toBe('none');
  expect(await styleOf(tile.locator('img.k-still'), 'visibility')).toBe('visible');

  await a.page.emulateMedia({ reducedMotion: 'reduce' });
  await expect(a.page.locator('html')).toHaveAttribute('data-motion', 'reduce');
  await a.page.goto(`/guilds/${gid}/settings/overview`);
  expect(await styleOf(preview.locator('img.k-live'), 'display')).toBe('none');
  expect(await styleOf(preview.locator('img.k-still'), 'visibility')).toBe('visible');
});

test('icone-E1 pas une image, trop petite, ou trop lourde : message clair, rien d’enregistré', async ({ user, api }) => {
  const a = await user('gaspard');
  const [gid] = await createGuild(a.page, 'Phare');
  await a.page.goto(`/guilds/${gid}/settings/overview`);
  const dlg = settings(a.page);
  const input = dlg.getByLabel('Choisir une image');

  await input.setInputFiles({ name: 'notes.txt', mimeType: 'text/plain', buffer: Buffer.from('ceci n’est pas une image') });
  await expect(dlg.getByRole('alert').filter({ hasText: 'pas une image lisible' })).toBeVisible();
  await expect(input).toHaveAttribute('aria-invalid', 'true');
  await expect(a.page.getByRole('dialog', { name: "Recadrer l'icône" })).toHaveCount(0);

  await input.setInputFiles({ name: 'enorme.png', mimeType: 'image/png', buffer: Buffer.alloc(9 * 1024 * 1024) });
  await expect(dlg.getByRole('alert').filter({ hasText: 'Mo au plus' })).toBeVisible();

  // Trop petite : refusée avec le même message par le serveur (même sans passer par la page).
  const tiny = await (await api(a.token)).put(`${API}/guilds/${gid}/icon`, { headers: { 'content-type': 'image/png' }, data: png(4, 4) });
  expect(tiny.status()).toBe(400);
  expect((await tiny.json()).message).toContain('trop petite');
  const notImage = await (await api(a.token)).put(`${API}/guilds/${gid}/icon`, { headers: { 'content-type': 'image/png' }, data: Buffer.from('GIF8 pas du tout') });
  expect(notImage.status()).toBe(400);
  expect(await iconKey(api, a, gid)).toBeNull();
  await a11y(a.page);
});

test('icone-N1 sans « Gérer le serveur » : ni rubrique, ni droit de changer le nom ou l’icône', async ({ user, api }) => {
  const owner = await user('chef');
  const crew = await user('mousse');
  const [gid, cid] = await createGuild(owner.page, 'Galère');
  await invite(api, owner, cid, crew);
  const asCrew = await api(crew.token);

  await crew.page.goto(`/guilds/${gid}/settings/overview`);
  await expect(settings(crew.page).getByRole('link', { name: "Vue d'ensemble" })).toHaveCount(0);
  await expect(settings(crew.page).getByRole('heading', { name: "Vue d'ensemble" })).toHaveCount(0);
  const put = await asCrew.put(`${API}/guilds/${gid}/icon`, { headers: { 'content-type': 'image/png' }, data: png(32, 32) });
  expect(put.status()).toBe(403);
  expect((await asCrew.delete(`${API}/guilds/${gid}/icon`)).status()).toBe(403);
  expect((await asCrew.patch(`${API}/guilds/${gid}`, { data: { name: 'Pirates' } })).status()).toBe(403);
  expect(await iconKey(api, owner, gid)).toBeNull();

  // Un rôle « Gérer le serveur » (1 << 25) ouvre la rubrique, sans rien d'autre.
  const asOwner = await api(owner.token);
  const role = await (await asOwner.post(`${API}/guilds/${gid}/roles`, { data: { name: 'Régisseurs', permissions: 1 << 25 } })).json();
  expect((await asOwner.put(`${API}/guilds/${gid}/members/${crew.id}/roles/${role.id}`)).status()).toBe(200);
  await crew.page.goto(`/guilds/${gid}/settings`);
  await expect(settings(crew.page).getByRole('heading', { name: "Vue d'ensemble", level: 2 })).toBeVisible();
  await expect(settings(crew.page).getByRole('navigation').getByRole('link')).toHaveText(["Vue d'ensemble", 'Invitations']);
  const ok = await asCrew.patch(`${API}/guilds/${gid}`, { data: { name: 'Pirates' } });
  expect(ok.status()).toBe(200);
  await expect(rail(owner.page, 'Pirates')).toBeVisible();
});

test('modeles-E2 la création échoue : seul un nom fautif est marqué, le focus va au bon endroit, « Réessayer » crée', async ({ user, allow }) => {
  allow.push(/ERR_FAILED|status of 400/);
  const a = await user('rene');
  await a.page.goto('/guilds/new');
  const name = a.page.getByLabel('Nom du serveur');
  await expect(name).toHaveValue(/^Serveur de /);

  // Erreur réseau : le champ Nom n'est pas marqué, le focus arrive sur « Réessayer », rien n'est créé.
  await a.page.route('**/api/v1/guilds', (r) => (r.request().method() === 'POST' ? r.abort('failed') : r.fallback()));
  await a.page.getByRole('button', { name: 'Créer le serveur', exact: true }).click();
  const alert = a.page.getByRole('alert').filter({ has: a.page.getByRole('button', { name: 'Réessayer' }) });
  await expect(alert).toBeVisible();
  await expect(name).not.toHaveAttribute('aria-invalid', 'true');
  await expect(name).not.toHaveAttribute('aria-describedby', /.+/);
  await expect(a.page.getByRole('button', { name: 'Réessayer' })).toBeFocused();
  await expect(a.page.getByRole('button', { name: 'Créer le serveur', exact: true })).toBeEnabled();
  await expect(a.page).toHaveURL(/\/guilds\/new$/);
  await a11y(a.page);

  // « Réessayer » une fois le réseau revenu : le serveur est créé, la page s'ouvre dessus.
  await a.page.unroute('**/api/v1/guilds');
  await a.page.getByRole('button', { name: 'Réessayer' }).click();
  await a.page.waitForURL(/\/channels\/[0-9a-f-]{36}\/[0-9a-f-]{36}$/);
  await expect(a.page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();

  // Un nom refusé par le serveur : lui seul est marqué (et relié à l'erreur), et c'est lui qui prend le focus.
  await a.page.goto('/guilds/new');
  await expect(name).toHaveValue(/^Serveur de /);
  await name.fill('x');
  await a.page.getByRole('button', { name: 'Créer le serveur', exact: true }).click();
  await expect(a.page.getByRole('alert').filter({ hasText: 'de 2 à 100 caractères' })).toBeVisible();
  await expect(name).toHaveAttribute('aria-invalid', 'true');
  await expect(name).toHaveAttribute('aria-describedby', 'ng-err');
  await expect(name).toBeFocused();
});

test('modeles-E3 l’icône ne part pas après la création : le serveur existe, une information le dit (zone status)', async ({ user, allow }) => {
  allow.push(/status of 400/);
  const a = await user('sacha');
  await a.page.goto('/guilds/new');
  await expect(a.page.getByLabel('Nom du serveur')).toHaveValue(/^Serveur de /);
  await a.page.getByLabel('Nom du serveur').fill('Archipel');
  await a.page.getByLabel('Choisir une image').setInputFiles({ name: 'archipel.png', mimeType: 'image/png', buffer: png(64, 64) });
  await a.page.getByRole('dialog', { name: "Recadrer l'icône" }).getByRole('button', { name: 'Appliquer' }).click();
  await expect(a.page.getByRole('img', { name: 'Icône choisie' })).toBeVisible();
  await a.page.route('**/api/v1/guilds/*/icon', (r) =>
    r.fulfill({ status: 400, contentType: 'application/json', body: JSON.stringify({ code: 'invalid', message: 'Cette image est refusée.' }) }),
  );
  await a.page.getByRole('button', { name: 'Créer le serveur', exact: true }).click();
  await a.page.waitForURL(/\/channels\/[0-9a-f-]{36}\/[0-9a-f-]{36}$/);
  await expect(rail(a.page, 'Archipel')).toBeVisible();
  const info = a.page.getByRole('status').filter({ hasText: "Le serveur est créé, mais son icône n'a pas pu être envoyée" });
  await expect(info).toBeVisible();
  await expect(info).toContainText('Vue d\'ensemble');
  await a11y(a.page);
  await info.getByRole('button', { name: "Fermer l'information" }).click();
  await expect(info).toHaveCount(0);
});

/** Crée un serveur dont l'icône ne part pas (500) : la page s'ouvre dessus, avec l'information dans la zone `status`. */
async function iconFailure(a: { page: Page }) {
  await a.page.goto('/guilds/new');
  await expect(a.page.getByLabel('Nom du serveur')).toHaveValue(/^Serveur de /);
  await a.page.getByLabel('Nom du serveur').fill('Archipel');
  await a.page.getByLabel('Choisir une image').setInputFiles({ name: 'archipel.png', mimeType: 'image/png', buffer: png(64, 64) });
  await a.page.getByRole('dialog', { name: "Recadrer l'icône" }).getByRole('button', { name: 'Appliquer' }).click();
  await expect(a.page.getByRole('img', { name: 'Icône choisie' })).toBeVisible();
  await a.page.route('**/api/v1/guilds/*/icon', (r) => r.fulfill({ status: 500, contentType: 'application/json', body: '{"code":"internal","message":"Erreur."}' }));
  await a.page.getByRole('button', { name: 'Créer le serveur', exact: true }).click();
  await a.page.waitForURL(/\/channels\/[0-9a-f-]{36}\/[0-9a-f-]{36}$/);
  return a.page.getByRole('status').filter({ hasText: "Le serveur est créé, mais son icône n'a pas pu être envoyée" });
}

/** L'information tient dans la fenêtre : texte entier (il passe à la ligne), « Fermer » à portée, rien ne déborde, sous l'en-tête. */
async function fitsTheWindow(page: Page, info: ReturnType<Page['locator']>, width: number) {
  await expect(info).toBeVisible();
  await expect(info).toContainText("Vue d'ensemble");
  const banner = info.locator('.k-banner');
  const box = (await banner.boundingBox())!;
  expect(box.x).toBeGreaterThanOrEqual(0);
  expect(box.x + box.width).toBeLessThanOrEqual(width);
  const text = banner.locator('span');
  expect(await text.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true);
  const lines = await text.evaluate((e) => (e.getBoundingClientRect().height / parseFloat(getComputedStyle(e).lineHeight)));
  if (width < 640) expect(Math.round(lines)).toBeGreaterThanOrEqual(3);
  else expect(Math.round(lines)).toBeGreaterThanOrEqual(1);
  const close = info.getByRole('button', { name: "Fermer l'information" });
  await expect(close).toBeInViewport({ ratio: 1 });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth)).toBe(true);
  const header = (await page.locator('.app-main .app-header').boundingBox())!;
  expect(box.y).toBeGreaterThanOrEqual(header.y + header.height);
  await a11y(page);
  await close.click();
  await expect(info).toHaveCount(0);
}

test('modeles-E5 l’information d’icône échouée tient dans la fenêtre à 1440 px : texte entier, « Fermer » visible', async ({ user, allow }) => {
  allow.push(/500/);
  const a = await user('sacha');
  await fitsTheWindow(a.page, await iconFailure(a), 1440);
});

test.describe('information d’icône à 390 px', () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test('modeles-E6 l’information d’icône échouée passe à la ligne à 390 px : texte entier, « Fermer » dans la fenêtre', async ({ user, allow }) => {
    allow.push(/500/);
    const a = await user('sacha');
    await fitsTheWindow(a.page, await iconFailure(a), 390);
  });
});

test('modeles-E4 l’aperçu de l’icône reste un carré de 96 px, à 390 px comme à 1440 px, à côté des boutons en large', async ({ user }) => {
  const a = await user('tania');
  const box = (l: ReturnType<Page['locator']>) => l.evaluate((e) => e.getBoundingClientRect().toJSON());
  await a.page.setViewportSize({ width: 390, height: 844 });
  await a.page.goto('/guilds/new');
  const preview = a.page.locator('.app-gicon-preview');
  await expect(preview).toBeVisible();
  const small = await box(preview);
  expect([small.width, small.height]).toEqual([96, 96]);
  // Les boutons, eux, prennent la largeur.
  const pick = await box(a.page.locator('label.k-btn', { hasText: 'Choisir une image' }));
  expect(pick.width).toBeGreaterThan(300);
  expect(await a.page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)).toBeLessThanOrEqual(0);

  await a.page.setViewportSize({ width: 1440, height: 900 });
  await a.page.goto('/guilds/new');
  await expect(preview).toBeVisible();
  const wide = await box(preview);
  expect([wide.width, wide.height]).toEqual([96, 96]);
  const btn = await box(a.page.locator('label.k-btn', { hasText: 'Choisir une image' }));
  expect(btn.left).toBeGreaterThanOrEqual(wide.right); // côte à côte, pas empilés
  expect(Math.abs(btn.top - wide.top)).toBeLessThan(wide.height);

  // Pareil dans Réglages › Vue d'ensemble.
  const [gid] = await createGuild(a.page, 'Cap Vert');
  await a.page.goto(`/guilds/${gid}/settings/overview`);
  const p2 = await box(settings(a.page).locator('.app-gicon-preview'));
  const b2 = await box(settings(a.page).locator('label.k-btn', { hasText: 'Choisir une image' }));
  expect([p2.width, p2.height]).toEqual([96, 96]);
  expect(b2.left).toBeGreaterThanOrEqual(p2.right);
});

test('modeles-E5 la page d’invitation laisse de l’air sous « 1 membre », avant les boutons', async ({ user, api }) => {
  const a = await user('uriel');
  const b = await user('vera');
  const [, cid] = await createGuild(a.page, 'Phalène');
  const { code } = await (await (await api(a.token)).post(`${API}/channels/${cid}/invites`, { data: {} })).json();
  await b.page.goto(`/invite/${code}`);
  const count = b.page.getByText('1 membre');
  await expect(count).toBeVisible();
  const text = await count.evaluate((e) => e.getBoundingClientRect().bottom);
  const foot = await b.page.locator('.app-auth-card > footer').evaluate((e) => e.getBoundingClientRect().top);
  expect(foot - text).toBeGreaterThanOrEqual(15);
});
