// S23 Gérer les salons : créer (salon, vocal, catégorie), régler (nom, sujet, mode lent), ranger au
// clavier, supprimer, salons privés et droits par rôle et par membre. Tout se voit en direct chez
// les autres membres.
import { test, expect, a11y, createGuild, say, messages, API, type User } from '../fixtures';
import type { APIRequestContext, Page } from '@playwright/test';

type Fixture = { user: (prefix?: string) => Promise<User>; api: (token?: string) => Promise<APIRequestContext> };

type Chan = { id: string; name: string; kind: string; parent_id: string | null; position: number; private?: boolean; slowmode_s?: number };

// Bits des droits (crates/kraken-core/src/perms.rs).
const VIEW_CHANNEL = 1 << 0;
const SEND_MESSAGES = 1 << 1;
const MANAGE_CHANNELS = 1 << 18;

/** Invite `who` dans le serveur par l'API ; il le voit apparaître dans son rail. */
async function invite(api: Fixture['api'], owner: User, cid: string, who: User) {
  const { code } = await (await (await api(owner.token)).post(`${API}/channels/${cid}/invites`, { data: {} })).json();
  const res = await (await api(who.token)).post(`${API}/invites/${code}`);
  expect(res.status()).toBe(200);
}

/** Les salons du serveur, tels que `u` les voit, du premier au dernier. */
async function channels(api: Fixture['api'], u: User, gid: string): Promise<Chan[]> {
  const g = await (await (await api(u.token)).get(`${API}/guilds/${gid}`)).json();
  return g.channels;
}

/** Crée un salon par l'API ; rend son id. */
async function make(api: Fixture['api'], owner: User, gid: string, name: string, kind = 'text', parent: string | null = null) {
  const res = await (await api(owner.token)).post(`${API}/guilds/${gid}/channels`, { data: { name, kind, parent_id: parent } });
  expect(res.status(), await res.text()).toBe(201);
  return (await res.json()).id as string;
}

const sidebar = (p: Page) => p.getByRole('navigation', { name: 'Salons' });
const settings = (p: Page) => p.getByRole('dialog', { name: 'Paramètres du salon' });

test('S23-H1 créer un salon, un vocal et une catégorie : depuis « + », le menu du serveur et les réglages ; en direct chez les autres', async ({ user, api }) => {
  const owner = await user('chef');
  const crew = await user('mousse');
  const [gid, cid] = await createGuild(owner.page, 'Phare');
  await invite(api, owner, cid, crew);
  await crew.page.getByRole('link', { name: 'Serveur Phare' }).click();
  await expect(sidebar(crew.page).getByRole('link', { name: 'général', exact: true })).toBeVisible();
  // Un simple membre ne voit aucune commande de gestion.
  await expect(sidebar(crew.page).getByRole('button', { name: 'Créer un salon' })).toHaveCount(0);
  await expect(sidebar(crew.page).getByRole('link', { name: /^Paramètres (du salon|de la catégorie)/ })).toHaveCount(0);

  // « + » d'une catégorie : la fenêtre s'ouvre sur le nom, un nom vide est refusé sur place.
  const plus = sidebar(owner.page).getByRole('button', { name: 'Créer un salon' }).first();
  await plus.click();
  const dialog = owner.page.getByRole('dialog', { name: 'Créer un salon' });
  const name = dialog.getByLabel('Nom du salon');
  await expect(name).toBeFocused();
  await dialog.getByRole('button', { name: 'Créer le salon' }).click();
  await expect(dialog.getByRole('alert')).toHaveText('Donne un nom au salon.');
  await expect(name).toBeFocused();
  await expect(name).toHaveAttribute('aria-invalid', 'true');
  await a11y(owner.page);
  // Échap ferme la fenêtre, le focus revient sur « + ».
  await owner.page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  await expect(plus).toBeFocused();

  // Un nom avec majuscules et espaces : « salon-de-test », rangé dans la catégorie, focus dans la saisie.
  await plus.click();
  await name.fill('Salon de Test');
  await name.press('Enter');
  await expect(owner.page.getByRole('heading', { name: 'salon-de-test', level: 1 })).toBeVisible();
  await expect(owner.page.getByLabel('Écrire dans #salon-de-test')).toBeFocused();
  const list = await channels(api, owner, gid);
  const textual = list.find((c) => c.kind === 'category' && c.name === 'Salons textuels')!;
  expect(list.find((c) => c.name === 'salon-de-test')?.parent_id).toBe(textual.id);
  // En direct chez l'autre membre, sans rechargement.
  await expect(sidebar(crew.page).getByRole('link', { name: 'salon-de-test' })).toBeVisible();

  // Menu du serveur : une catégorie ; le focus revient sur le bouton du menu.
  const menu = owner.page.getByRole('button', { name: "Plus d'actions pour ce serveur" });
  await menu.click();
  await owner.page.getByRole('button', { name: 'Créer une catégorie' }).click();
  const cat = owner.page.getByRole('dialog', { name: 'Créer une catégorie' });
  await expect(cat.getByLabel('Nom de la catégorie')).toBeFocused();
  await expect(cat.getByRole('radio', { name: /Catégorie/ })).toBeChecked();
  await expect(cat.getByLabel('Catégorie', { exact: true })).toHaveCount(0);
  await cat.getByLabel('Nom de la catégorie').fill('Archives');
  await cat.getByRole('button', { name: 'Créer la catégorie' }).click();
  await expect(sidebar(owner.page).getByRole('button', { name: 'Archives' })).toBeVisible();
  await expect(sidebar(crew.page).getByRole('button', { name: 'Archives' })).toBeVisible();
  await expect(menu).toBeFocused();

  // Réglages › Salons : un vocal dans la nouvelle catégorie (le nom garde ses majuscules).
  await owner.page.goto(`/guilds/${gid}/settings/channels`);
  const admin = owner.page.getByRole('dialog', { name: 'Paramètres du serveur' });
  await admin.getByRole('button', { name: 'Créer un salon' }).click();
  const made = owner.page.getByRole('dialog', { name: 'Créer un salon' });
  await made.getByRole('radio', { name: /Salon vocal/ }).check();
  await expect(made.getByRole('button', { name: 'Créer le salon' })).toBeVisible();
  await made.getByLabel('Nom du salon').fill('Salle des Machines');
  await made.getByLabel('Catégorie', { exact: true }).selectOption({ label: 'Archives' });
  await made.getByRole('button', { name: 'Créer le salon' }).click();
  await expect(made).toHaveCount(0);
  await expect(admin.getByRole('list', { name: /Salons et catégories/ }).getByText('Salle des Machines')).toBeVisible();
  await expect(admin.getByRole('button', { name: 'Créer un salon' })).toBeFocused();
  const after = await channels(api, owner, gid);
  const archives = after.find((c) => c.name === 'Archives')!;
  const voice = after.find((c) => c.name === 'Salle des Machines')!;
  expect(voice.kind).toBe('voice');
  expect(voice.parent_id).toBe(archives.id);
  await expect(sidebar(crew.page).getByRole('link', { name: /Salle des Machines/ })).toBeVisible();
});

test('S23-H2 régler un salon : nom, sujet dans l’en-tête, mode lent appliqué par le serveur avec décompte', async ({ user, api }) => {
  const owner = await user('chef');
  const crew = await user('mousse');
  const [, cid] = await createGuild(owner.page, 'Vigie');
  await invite(api, owner, cid, crew);
  await crew.page.getByRole('link', { name: 'Serveur Vigie' }).click();
  await expect(crew.page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
  const draft = crew.page.getByLabel('Écrire dans #général');
  await draft.fill('brouillon en cours');

  // Le premier engrenage « salon » de la liste est celui de #général.
  await sidebar(owner.page).getByRole('link', { name: 'Paramètres du salon' }).first().click();
  const dlg = settings(owner.page);
  await expect(dlg).toBeVisible();
  await expect(dlg.getByLabel('Nom du salon')).toHaveValue('général');
  await dlg.getByLabel('Nom du salon').fill('Bord de Mer');
  await dlg.getByLabel('Sujet du salon').fill('Tout ce qui flotte');
  await dlg.getByLabel('Mode lent').selectOption({ label: '5 secondes' });
  const bar = dlg.getByRole('region', { name: 'Modifications du salon' });
  await expect(bar.getByText('Modifications non enregistrées')).toBeVisible();
  await a11y(owner.page);
  // Annuler défait tout ; on recommence.
  await bar.getByRole('button', { name: 'Annuler' }).click();
  await expect(bar).toHaveCount(0);
  await expect(dlg.getByLabel('Nom du salon')).toHaveValue('général');
  await dlg.getByLabel('Nom du salon').fill('Bord de Mer');
  await dlg.getByLabel('Sujet du salon').fill('Tout ce qui flotte');
  await dlg.getByLabel('Mode lent').selectOption({ label: '5 secondes' });
  await bar.getByRole('button', { name: 'Enregistrer' }).click();
  await expect(bar).toHaveCount(0);
  await expect(dlg.getByLabel('Nom du salon')).toHaveValue('bord-de-mer');

  // Chez l'autre membre : titre, sujet et liste suivent, la vue n'est pas reconstruite (le brouillon reste).
  await expect(crew.page.getByRole('heading', { name: 'bord-de-mer', level: 1 })).toBeVisible();
  await expect(crew.page.getByText('Tout ce qui flotte')).toBeVisible();
  await expect(sidebar(crew.page).getByRole('link', { name: 'bord-de-mer' })).toBeVisible();
  await expect(crew.page.getByLabel('Écrire dans #bord-de-mer')).toHaveValue('brouillon en cours');
  await expect(crew.page.getByText('Mode lent : 5 secondes entre deux messages')).toBeVisible();

  // Le premier message passe, le suivant est refusé tout de suite, avec le décompte.
  await say(crew.page, 'bord-de-mer', 'Premier');
  await expect(messages(crew.page).filter({ hasText: 'Premier' })).toHaveCount(1);
  await expect(crew.page.getByText(/Mode lent : encore \d s avant ton prochain message/)).toBeVisible();
  await say(crew.page, 'bord-de-mer', 'Trop tôt');
  await expect(crew.page.getByRole('alert').filter({ hasText: 'Mode lent : attends encore' })).toBeVisible();
  await expect(messages(crew.page).filter({ hasText: 'Trop tôt' })).toHaveCount(0);
  // Le serveur applique la règle lui-même (autre appareil), sauf pour qui gère les messages ou les salons.
  const early = await (await api(crew.token)).post(`${API}/channels/${cid}/messages`, { data: { content: 'par l’API' } });
  expect(early.status()).toBe(429);
  expect((await early.json()).code).toBe('slowmode');
  const asOwner = await api(owner.token);
  for (const content of ['un', 'deux']) {
    expect((await asOwner.post(`${API}/channels/${cid}/messages`, { data: { content } })).status()).toBe(201);
  }
  // Le délai passé, il peut écrire de nouveau.
  await expect(crew.page.getByText('Mode lent : 5 secondes entre deux messages')).toBeVisible({ timeout: 10_000 });
  await say(crew.page, 'bord-de-mer', 'Plus tard');
  await expect(messages(crew.page).filter({ hasText: 'Plus tard' })).toHaveCount(1);

  // Le propriétaire n'attend jamais : pas de bandeau, deux messages de suite.
  await owner.page.keyboard.press('Escape');
  await expect(dlg).toHaveCount(0);
  await expect(owner.page.getByLabel('Écrire dans #bord-de-mer')).toBeVisible();
  await expect(owner.page.getByText(/Mode lent :/)).toHaveCount(0);
  await say(owner.page, 'bord-de-mer', 'Vite');
  await say(owner.page, 'bord-de-mer', 'Et vite');
  await expect(messages(owner.page).filter({ hasText: /vite/i })).toHaveCount(2);
});

test('S23-H3 supprimer un salon : confirmation, les autres le voient partir ; le dernier salon d’écriture reste', async ({ user, api, allow }) => {
  allow.push(/status of 404/, /status of 409/);
  const owner = await user('chef');
  const crew = await user('mousse');
  const [gid, cid] = await createGuild(owner.page, 'Épave');
  await invite(api, owner, cid, crew);
  const gone = await make(api, owner, gid, 'éphémère');
  await (await api(owner.token)).post(`${API}/channels/${gone}/messages`, { data: { content: 'Je vais disparaître' } });
  await crew.page.getByRole('link', { name: 'Serveur Épave' }).click();
  await sidebar(crew.page).getByRole('link', { name: 'éphémère' }).click();
  await expect(messages(crew.page).filter({ hasText: 'Je vais disparaître' })).toHaveCount(1);

  await owner.page.goto(`/channels/${gid}/${gone}/settings`);
  const dlg = settings(owner.page);
  await dlg.getByRole('button', { name: 'Supprimer le salon' }).click();
  const confirm = owner.page.getByRole('alertdialog', { name: 'Supprimer #éphémère ?' });
  await expect(confirm.getByRole('button', { name: 'Annuler' })).toBeFocused();
  await owner.page.keyboard.press('Escape');
  await expect(confirm).toHaveCount(0);
  await expect(dlg).toBeVisible(); // Échap n'a fermé que la confirmation
  await dlg.getByRole('button', { name: 'Supprimer le salon' }).click();
  await confirm.getByRole('button', { name: 'Supprimer le salon' }).click();
  await expect(owner.page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
  await expect(sidebar(owner.page).getByRole('link', { name: 'éphémère' })).toHaveCount(0);
  // Chez l'autre membre qui le regardait : renvoyé vers le serveur, le salon n'est plus dans la liste.
  await expect(crew.page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
  await expect(sidebar(crew.page).getByRole('link', { name: 'éphémère' })).toHaveCount(0);
  expect((await (await api(owner.token)).get(`${API}/channels/${gone}/messages`)).status()).toBe(404);

  // #général est maintenant le seul salon d'écriture : il reste, ici comme sur le serveur.
  await owner.page.goto(`/channels/${gid}/${cid}/settings`);
  await expect(dlg.getByRole('button', { name: 'Supprimer le salon' })).toBeDisabled();
  await expect(dlg.getByText(/dernier salon d.écriture du serveur/)).toBeVisible();
  const res = await (await api(owner.token)).delete(`${API}/channels/${cid}`);
  expect(res.status()).toBe(409);
  expect((await res.json()).code).toBe('last_text');
  await a11y(owner.page);
});

test('S23-K1 ranger les salons au clavier : Monter et Descendre, d’une catégorie à l’autre, le focus reste sur le bouton', async ({ user, api }) => {
  const owner = await user('chef');
  const [gid] = await createGuild(owner.page, 'Archipel');
  const g = await channels(api, owner, gid);
  const textual = g.find((c) => c.name === 'Salons textuels')!;
  const voice = g.find((c) => c.name === 'Salons vocaux')!;
  await make(api, owner, gid, 'alpha', 'text', textual.id);
  await make(api, owner, gid, 'beta', 'text', textual.id);

  await owner.page.goto(`/guilds/${gid}/settings/channels`);
  const list = owner.page.getByRole('list', { name: /Salons et catégories/ });
  const order = () => list.getByRole('listitem').evaluateAll((els) => els.map((e) => e.querySelector('.app-chan-name')!.textContent!.trim()));
  await expect.poll(order).toEqual(['Salons textuels', 'général', 'alpha', 'beta', 'Salons vocaux', 'Général']);
  await a11y(owner.page);
  // Aux extrémités, le bouton est inactif.
  await expect(owner.page.getByRole('button', { name: 'Monter la catégorie Salons textuels' })).toBeDisabled();
  await expect(owner.page.getByRole('button', { name: 'Descendre le salon Général', exact: true })).toBeDisabled();

  // Descendre « général » au clavier : le focus reste sur le bouton, même après plusieurs pressions.
  const down = owner.page.getByRole('button', { name: 'Descendre le salon général', exact: true });
  await down.focus();
  await owner.page.keyboard.press('Enter');
  await expect.poll(order).toEqual(['Salons textuels', 'alpha', 'général', 'beta', 'Salons vocaux', 'Général']);
  await expect(down).toBeFocused();
  await owner.page.keyboard.press('Space');
  await expect.poll(order).toEqual(['Salons textuels', 'alpha', 'beta', 'général', 'Salons vocaux', 'Général']);
  await expect(down).toBeFocused();
  // Au bas de sa catégorie, descendre le range dans la catégorie suivante, tout en haut.
  await owner.page.keyboard.press('Enter');
  await expect.poll(order).toEqual(['Salons textuels', 'alpha', 'beta', 'Salons vocaux', 'général', 'Général']);
  await expect(down).toBeFocused();
  await expect.poll(async () => (await channels(api, owner, gid)).find((c) => c.name === 'général')!.parent_id).toBe(voice.id);
  // Et il remonte dans la première.
  const up = owner.page.getByRole('button', { name: 'Monter le salon général', exact: true });
  await up.focus();
  await owner.page.keyboard.press('Enter');
  await expect.poll(order).toEqual(['Salons textuels', 'alpha', 'beta', 'général', 'Salons vocaux', 'Général']);
  await expect(up).toBeFocused();
  expect((await channels(api, owner, gid)).find((c) => c.name === 'général')!.parent_id).toBe(textual.id);

  // Une catégorie ne bouge qu'entre catégories ; elle emmène ses salons.
  const catDown = owner.page.getByRole('button', { name: 'Descendre la catégorie Salons textuels' });
  await catDown.focus();
  await owner.page.keyboard.press('Enter');
  await expect.poll(order).toEqual(['Salons vocaux', 'Général', 'Salons textuels', 'alpha', 'beta', 'général']);
  await expect(owner.page.getByRole('button', { name: 'Descendre la catégorie Salons textuels' })).toBeDisabled();
  // Le résultat est celui du serveur, pas seulement de l'écran.
  const final = await channels(api, owner, gid);
  expect(final.filter((c) => c.kind === 'category').sort((a, b) => a.position - b.position).map((c) => c.name)).toEqual(['Salons vocaux', 'Salons textuels']);
  // Dans la barre latérale aussi, après rechargement.
  await owner.page.goto(`/channels/${gid}`);
  await expect(sidebar(owner.page).getByRole('button', { expanded: true })).toHaveText(['Salons vocaux', 'Salons textuels']);
});

test('S23-H4 salon privé : il disparaît en direct pour qui ne le voit plus, revient avec un droit donné à un membre, refus d’écrire', async ({ user, api, allow }) => {
  allow.push(/status of 404/, /status of 403/);
  const owner = await user('chef');
  const crew = await user('mousse');
  const [gid, cid] = await createGuild(owner.page, 'Soute');
  await invite(api, owner, cid, crew);
  const secret = await make(api, owner, gid, 'secret');
  await crew.page.getByRole('link', { name: 'Serveur Soute' }).click();
  await sidebar(crew.page).getByRole('link', { name: 'secret' }).click();
  await expect(crew.page.getByRole('heading', { name: 'secret', level: 1 })).toBeVisible();

  // Le propriétaire le rend privé : il quitte la liste de l'autre membre, qui est renvoyé au serveur.
  await owner.page.goto(`/channels/${gid}/${secret}/settings`);
  const dlg = settings(owner.page);
  const priv = dlg.getByRole('switch', { name: 'Salon privé' });
  await priv.check();
  await dlg.getByRole('region', { name: 'Modifications du salon' }).getByRole('button', { name: 'Enregistrer' }).click();
  await expect(priv).toBeChecked();
  await expect(crew.page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
  await expect(sidebar(crew.page).getByRole('link', { name: /secret/ })).toHaveCount(0);
  expect((await (await api(crew.token)).get(`${API}/channels/${secret}/messages`)).status()).toBe(404);
  // Chez le propriétaire : un cadenas, dit aux lecteurs d'écran.
  await owner.page.keyboard.press('Escape');
  await expect(sidebar(owner.page).getByRole('link', { name: /salon privé secret/ })).toBeVisible();

  // Permissions : donner à l'autre membre de voir le salon (et lui refuser d'y écrire).
  await owner.page.goto(`/channels/${gid}/${secret}/settings/permissions`);
  await expect(dlg.getByRole('list', { name: 'Rôles et membres de ce salon' }).getByRole('button', { name: '@tout le monde' })).toBeVisible();
  await a11y(owner.page);
  await dlg.getByLabel('Ajouter un rôle ou un membre').selectOption({ label: `${crew.name} (@${crew.name})` });
  await dlg.getByRole('button', { name: 'Ajouter', exact: true }).click();
  await expect(dlg.getByRole('heading', { name: crew.name })).toBeVisible();
  await dlg.getByRole('radiogroup', { name: 'Voir les salons' }).getByRole('radio', { name: 'Autoriser' }).check();
  await dlg.getByRole('radiogroup', { name: 'Écrire des messages' }).getByRole('radio', { name: 'Refuser' }).check();
  const bar = dlg.getByRole('region', { name: 'Modifications des permissions' });
  await expect(bar.getByText('Modifications non enregistrées')).toBeVisible();
  // Rien n'est enregistré avant « Enregistrer » : l'autre membre ne le voit toujours pas.
  await expect(sidebar(crew.page).getByRole('link', { name: /secret/ })).toHaveCount(0);
  await a11y(owner.page);
  await bar.getByRole('button', { name: 'Enregistrer' }).click();
  await expect(bar).toHaveCount(0);
  await expect(dlg.getByRole('radiogroup', { name: 'Voir les salons' }).getByRole('radio', { name: 'Autoriser' })).toBeChecked();
  // Il le voit revenir tout de suite, lit, mais ne peut pas écrire (le serveur refuse aussi).
  await expect(sidebar(crew.page).getByRole('link', { name: /secret/ })).toBeVisible();
  const asCrew = await api(crew.token);
  expect((await asCrew.get(`${API}/channels/${secret}/messages`)).status()).toBe(200);
  const denied = await asCrew.post(`${API}/channels/${secret}/messages`, { data: { content: 'bonjour' } });
  expect(denied.status()).toBe(403);
  // Un membre sans droit de gérer ne voit pas les réglages du salon.
  expect((await asCrew.get(`${API}/channels/${secret}/permissions`)).status()).toBe(403);

  // Tout remettre sur « Hériter » retire son réglage : le salon redevient invisible pour lui.
  await dlg.getByRole('button', { name: 'Tout remettre sur « Hériter »' }).click();
  await bar.getByRole('button', { name: 'Enregistrer' }).click();
  await expect(bar).toHaveCount(0);
  await expect(sidebar(crew.page).getByRole('link', { name: /secret/ })).toHaveCount(0);
  // Repasser en public : tout le monde le revoit.
  await owner.page.goto(`/channels/${gid}/${secret}/settings`);
  await priv.uncheck();
  await dlg.getByRole('region', { name: 'Modifications du salon' }).getByRole('button', { name: 'Enregistrer' }).click();
  await expect(sidebar(crew.page).getByRole('link', { name: 'secret' })).toBeVisible();
});

test('S23-N1 hiérarchie et accès : on ne donne que les droits qu’on a, on ne se retire pas l’accès, un membre ne gère rien', async ({ user, api, allow }) => {
  allow.push(/status of 40[34]/);
  const owner = await user('roi');
  const mod = await user('garde');
  const plain = await user('simple');
  const outsider = await user('passant');
  const [gid, cid] = await createGuild(owner.page, 'Citadelle');
  await invite(api, owner, cid, mod);
  await invite(api, owner, cid, plain);
  const http = await api(owner.token);
  // Les gardes gèrent les salons et voient tout, mais n'ont pas le droit de mentionner tout le monde.
  const role = await (await http.post(`${API}/guilds/${gid}/roles`, { data: { name: 'Gardes', permissions: MANAGE_CHANNELS | VIEW_CHANNEL | SEND_MESSAGES } })).json();
  expect((await http.put(`${API}/guilds/${gid}/members/${mod.id}/roles/${role.id}`)).status()).toBe(200);

  // Un simple membre : ni commande, ni réglages ; le serveur refuse, un étranger ne sait même pas que le salon existe.
  await plain.page.getByRole('link', { name: 'Serveur Citadelle' }).click();
  await expect(sidebar(plain.page).getByRole('link', { name: 'général', exact: true })).toBeVisible();
  await expect(sidebar(plain.page).getByRole('button', { name: /Créer un salon/ })).toHaveCount(0);
  await plain.page.goto(`/channels/${gid}/${cid}/settings`);
  await expect(plain.page.getByText('Il faut le droit « Gérer les salons » pour changer ce salon.')).toBeVisible();
  const asPlain = await api(plain.token);
  for (const [method, path, data] of [
    ['post', `/guilds/${gid}/channels`, { name: 'x', kind: 'text', parent_id: null }],
    ['patch', `/channels/${cid}`, { name: 'x' }],
    ['delete', `/channels/${cid}`, undefined],
    ['put', `/channels/${cid}/position`, { parent_id: null, index: 0 }],
    ['get', `/channels/${cid}/permissions`, undefined],
  ] as const) {
    const res = await asPlain[method](`${API}${path}`, data ? { data } : undefined);
    expect(res.status(), `${method} ${path}`).toBe(403);
  }
  const asOutsider = await api(outsider.token);
  expect((await asOutsider.patch(`${API}/channels/${cid}`, { data: { name: 'x' } })).status()).toBe(404);
  expect((await asOutsider.get(`${API}/channels/${cid}/permissions`)).status()).toBe(404);

  // Le garde règle les droits d'un salon : @tout le monde, oui ; mais pas un droit qu'il n'a pas lui-même.
  await mod.page.goto(`/channels/${gid}/${cid}/settings/permissions`);
  const dlg = settings(mod.page);
  const everyone = dlg.getByRole('radiogroup', { name: 'Mentionner tout le monde' }).getByRole('radio', { name: 'Autoriser' });
  await expect(everyone).toBeDisabled();
  await expect(dlg.getByRole('radiogroup', { name: 'Écrire des messages' }).getByRole('radio', { name: 'Refuser' })).toBeEnabled();
  const asMod = await api(mod.token);
  const grant = await asMod.put(`${API}/channels/${cid}/permissions/${gid}`, { data: { kind: 'role', allow: 1 << 7, deny: 0 } });
  expect(grant.status()).toBe(403);
  // Il ne peut pas se fermer la porte : refuser « Voir les salons » à @tout le monde le priverait de #général.
  const lock = await asMod.put(`${API}/channels/${cid}/permissions/${gid}`, { data: { kind: 'role', allow: 0, deny: VIEW_CHANNEL | MANAGE_CHANNELS } });
  expect(lock.status()).toBe(403);
  expect((await lock.json()).code).toBe('lockout');
  // Dans l'interface, le message du serveur s'affiche.
  await dlg.getByRole('radiogroup', { name: 'Droit sensible : Gérer les salons' }).getByRole('radio', { name: 'Refuser' }).check();
  await dlg.getByRole('region', { name: 'Modifications des permissions' }).getByRole('button', { name: 'Enregistrer' }).click();
  await expect(dlg.getByRole('alert')).toContainText("te retirerait l'accès");
  // Le salon n'a pas bougé.
  expect((await asMod.get(`${API}/channels/${cid}/messages`)).status()).toBe(200);
});

test.describe('largeur mobile', () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test('S23-E1 à 390 px : créer un salon et le régler depuis le tiroir, sans défilement horizontal', async ({ user, api }) => {
    const owner = await user('chef');
    // Le rail des serveurs est dans le tiroir : le serveur est créé par l'API, puis ouvert.
    const res = await (await api(owner.token)).post(`${API}/guilds`, { data: { name: 'Poche' } });
    expect(res.status(), await res.text()).toBe(201);
    const g = await res.json();
    const first = g.channels.find((c: Chan) => c.kind === 'text');
    await owner.page.goto(`/channels/${g.guild.id}/${first.id}`);
    await expect(owner.page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
    const noScroll = async () => expect(await owner.page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
    await owner.page.getByRole('button', { name: 'Ouvrir la navigation' }).click();
    await sidebar(owner.page).getByRole('button', { name: 'Créer un salon' }).first().click();
    const dialog = owner.page.getByRole('dialog', { name: 'Créer un salon' });
    await expect(dialog.getByLabel('Nom du salon')).toBeFocused();
    await noScroll();
    await a11y(owner.page);
    await dialog.getByLabel('Nom du salon').fill('mobile');
    await dialog.getByRole('button', { name: 'Créer le salon' }).click();
    await expect(owner.page.getByRole('heading', { name: 'mobile', level: 1 })).toBeVisible();

    // Paramètres du salon, onglet Permissions : les trois choix restent à portée, rien ne déborde.
    await owner.page.getByRole('button', { name: 'Ouvrir la navigation' }).click();
    await sidebar(owner.page).getByRole('link', { name: 'Paramètres du salon' }).nth(1).click();
    const dlg = settings(owner.page);
    await expect(dlg.getByLabel('Nom du salon')).toHaveValue('mobile');
    await noScroll();
    await a11y(owner.page);
    await dlg.getByRole('link', { name: 'Permissions' }).click();
    await expect(dlg.getByRole('radiogroup', { name: 'Voir les salons' })).toBeVisible();
    await noScroll();
    await a11y(owner.page);
  });
});
