// S22 Rôles, exclusion, bannissement, départ d'un serveur, transfert de propriété.
import { test, expect, a11y, createGuild, say, messages, API, type User } from '../fixtures';
import type { APIRequestContext, Locator } from '@playwright/test';

type Fixture = { user: (prefix?: string) => Promise<User>; api: (token?: string) => Promise<APIRequestContext> };

/** Invite `who` dans le serveur par l'API ; il le voit apparaître dans son rail. */
async function invite(api: Fixture['api'], owner: User, cid: string, who: User) {
  const { code } = await (await (await api(owner.token)).post(`${API}/channels/${cid}/invites`, { data: {} })).json();
  const res = await (await api(who.token)).post(`${API}/invites/${code}`);
  expect(res.status()).toBe(200);
  return code as string;
}

const members = (u: User) => u.page.getByRole('complementary', { name: 'Membres' });
const row = (u: User, name: string): Locator => members(u).getByRole('listitem').filter({ hasText: name });
const moreFor = (u: User, name: string) => u.page.getByRole('button', { name: `Plus d'actions pour ${name}` });

/** Rôles du serveur, du plus haut au plus bas, sans @everyone. */
async function roleNames(api: Fixture['api'], u: User, gid: string): Promise<string[]> {
  const g = await (await (await api(u.token)).get(`${API}/guilds/${gid}`)).json();
  return g.roles.filter((r: { id: string }) => r.id !== gid).sort((a: { position: number }, b: { position: number }) => b.position - a.position).map((r: { name: string }) => r.name);
}

test('S22-H1 créer un rôle, lui donner une couleur, le donner à un membre : la couleur se voit partout, en direct', async ({ user, api }) => {
  const owner = await user('chef');
  const crew = await user('mousse');
  const [gid, cid] = await createGuild(owner.page, 'Galion');
  await invite(api, owner, cid, crew);
  await owner.page.getByRole('link', { name: 'Serveur Galion' }).click();
  await expect(members(owner).getByText(crew.name)).toBeVisible();

  // Réglages : créer, renommer, colorier, afficher à part.
  await owner.page.getByRole('link', { name: 'Paramètres du serveur' }).click();
  await expect(owner.page.getByRole('dialog', { name: 'Paramètres du serveur' })).toBeVisible();
  await owner.page.getByRole('button', { name: 'Créer un rôle' }).click();
  const name = owner.page.getByLabel('Nom du rôle');
  await expect(name).toBeFocused();
  await name.fill('Capitaines');
  await owner.page.getByRole('button', { name: 'Renommer' }).click();
  await expect(owner.page.getByText('Enregistré.')).toBeVisible();
  await owner.page.getByRole('radio', { name: 'Bleu' }).click();
  await expect(owner.page.getByRole('radio', { name: 'Bleu' })).toHaveAttribute('aria-checked', 'true');
  await owner.page.getByRole('switch', { name: 'Afficher à part dans la liste des membres' }).check();
  // Un droit, avec sa description en français.
  const kick = owner.page.getByRole('switch', { name: 'Expulser des membres' });
  await expect(owner.page.getByText('Faire sortir un membre, qui peut revenir avec une invitation.')).toBeVisible();
  await kick.check();
  await expect(kick).toBeChecked();
  await a11y(owner.page);
  expect(await roleNames(api, owner, gid)).toEqual(['Capitaines']);
  await owner.page.keyboard.press('Escape');
  await expect(owner.page.getByRole('dialog', { name: 'Paramètres du serveur' })).toHaveCount(0);

  // Donner le rôle depuis le menu du membre : sa fiche.
  await moreFor(owner, crew.name).click();
  await owner.page.getByRole('button', { name: 'Profil et rôles' }).click();
  const fiche = owner.page.getByRole('dialog');
  await fiche.getByLabel('Capitaines').check();
  await expect(fiche.getByLabel('Capitaines')).toBeChecked();
  await fiche.getByRole('button', { name: 'Fermer' }).click();

  // Liste des membres : rubrique du rôle, nom coloré avec la teinte du jeton, chez les deux.
  for (const u of [owner, crew]) {
    await u.page.getByRole('link', { name: 'Serveur Galion' }).click();
    await expect(members(u).getByRole('heading', { name: 'Capitaines — 1', level: 3 })).toBeVisible();
    const colored = row(u, crew.name).locator('[data-role-color="6"]');
    await expect(colored).toHaveText(crew.name);
    const tint = await u.page.evaluate(() => {
      const probe = document.createElement('i');
      probe.style.color = 'var(--avatar-6-text)';
      document.body.append(probe);
      const c = getComputedStyle(probe).color;
      probe.remove();
      return c;
    });
    await expect(colored).toHaveCSS('color', tint);
  }

  // Ses messages portent la couleur dans le fil.
  await say(crew.page, 'général', 'Terre en vue !');
  await expect(messages(owner.page).last().locator('b[data-role-color="6"]')).toHaveText(crew.name);

  // Retirer le rôle : plus de rubrique, plus de couleur, en direct chez le membre.
  await moreFor(owner, crew.name).click();
  await owner.page.getByRole('button', { name: 'Profil et rôles' }).click();
  await owner.page.getByRole('dialog').getByLabel('Capitaines').uncheck();
  await owner.page.getByRole('dialog').getByRole('button', { name: 'Fermer' }).click();
  await expect(members(crew).getByRole('heading', { level: 3 })).toHaveCount(0);
  await expect(row(crew, crew.name).locator('[data-role-color]')).toHaveCount(0);
});

test('S22-H2 exclure : la personne est renvoyée à l’accueil et peut revenir ; sans droit, le menu ne propose rien', async ({ user, api }) => {
  const owner = await user('capit');
  const crew = await user('matelot');
  const other = await user('novice');
  const [gid, cid] = await createGuild(owner.page, 'Barque');
  await invite(api, owner, cid, crew);
  const code = await invite(api, owner, cid, other);
  await crew.page.getByRole('link', { name: 'Serveur Barque' }).click();
  await expect(members(crew).getByText(other.name)).toBeVisible();

  // Sans le droit, ni « Expulser » ni « Bannir » dans le menu d'un membre.
  await moreFor(crew, other.name).click();
  await expect(crew.page.getByRole('button', { name: 'Profil et rôles' })).toBeVisible();
  await expect(crew.page.getByRole('button', { name: 'Expulser' })).toHaveCount(0);
  await expect(crew.page.getByRole('button', { name: 'Bannir' })).toHaveCount(0);
  await crew.page.keyboard.press('Escape');

  // Le propriétaire ne se sanctionne pas lui-même, ni n'a de menu qui le propose.
  await moreFor(owner, owner.name).click();
  await expect(owner.page.getByRole('button', { name: 'Expulser' })).toHaveCount(0);
  await owner.page.keyboard.press('Escape');

  await moreFor(owner, crew.name).click();
  await owner.page.getByRole('button', { name: 'Expulser' }).click();
  const dialog = owner.page.getByRole('alertdialog', { name: `Expulser ${crew.name} ?` });
  await dialog.getByLabel('Raison (facultatif)').fill('trop de bruit');
  await dialog.getByRole('button', { name: 'Expulser', exact: true }).click();
  await expect(dialog).toHaveCount(0);
  await expect(members(owner).getByText(crew.name)).toHaveCount(0);

  // L'exclu perd le serveur en direct, sans recharger, et la page quitte le serveur.
  await expect(crew.page.getByRole('link', { name: 'Serveur Barque' })).toHaveCount(0);
  await expect(crew.page.getByRole('heading', { name: 'Amis', level: 1 })).toBeVisible();
  expect((await (await api(crew.token)).get(`${API}/guilds/${gid}`)).status()).toBe(404);
  // Les autres membres ne le voient plus non plus.
  await expect(members(other).getByText(crew.name)).toHaveCount(0);
  // Une exclusion n'est pas un bannissement : retour par invitation.
  expect((await (await api(crew.token)).post(`${API}/invites/${code}`)).status()).toBe(200);
});

test('S22-H3 bannir puis débannir : plus de retour par invitation, liste des bannis dans les réglages', async ({ user, api }) => {
  const owner = await user('amiral');
  const crew = await user('pirate');
  const [gid, cid] = await createGuild(owner.page, 'Corsaires');
  const code = await invite(api, owner, cid, crew);
  await crew.page.getByRole('link', { name: 'Serveur Corsaires' }).click();
  await say(crew.page, 'général', 'pub pub pub');
  await expect(messages(owner.page).filter({ hasText: 'pub pub pub' })).toHaveCount(1);

  await moreFor(owner, crew.name).click();
  await owner.page.getByRole('button', { name: 'Bannir' }).click();
  const dialog = owner.page.getByRole('alertdialog', { name: `Bannir ${crew.name} ?` });
  await a11y(owner.page);
  await dialog.getByLabel('Raison (facultatif)').fill('publicité');
  await dialog.getByLabel('Messages à effacer').selectOption('24');
  await dialog.getByRole('button', { name: 'Bannir', exact: true }).click();
  await expect(dialog).toHaveCount(0);
  await expect(members(owner).getByText(crew.name)).toHaveCount(0);
  // Ses messages des dernières 24 h ont disparu du fil, en direct.
  await expect(messages(owner.page).filter({ hasText: 'pub pub pub' })).toHaveCount(0);
  await expect(crew.page.getByRole('link', { name: 'Serveur Corsaires' })).toHaveCount(0);

  // Il ne peut plus rejoindre, même avec une invitation valable.
  const refused = await (await api(crew.token)).post(`${API}/invites/${code}`);
  expect(refused.status()).toBe(403);
  expect((await refused.json()).code).toBe('banned');

  // Liste des bannis : le motif, et débannir.
  await owner.page.getByRole('link', { name: 'Paramètres du serveur' }).click();
  await owner.page.getByRole('link', { name: 'Bannis' }).click();
  const ban = owner.page.getByRole('listitem').filter({ hasText: crew.name });
  await expect(ban).toContainText('Raison : publicité');
  await a11y(owner.page);
  await owner.page.getByRole('button', { name: `Débannir ${crew.name}` }).click();
  await expect(owner.page.getByText('Personne n\'est banni')).toBeVisible();
  expect((await (await api(crew.token)).post(`${API}/invites/${code}`)).status()).toBe(200);
  await expect(crew.page.getByRole('link', { name: 'Serveur Corsaires' })).toBeVisible();
  void gid;
});

test('S22-H4 quitter un serveur : confirmation ; le propriétaire doit d’abord le transférer', async ({ user, api }) => {
  const owner = await user('doyen');
  const crew = await user('recrue');
  const [gid, cid] = await createGuild(owner.page, 'Phare');
  await invite(api, owner, cid, crew);
  await crew.page.getByRole('link', { name: 'Serveur Phare' }).click();
  await expect(crew.page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();

  // Le propriétaire n'a pas « Quitter » : il a « Transférer la propriété ».
  await owner.page.getByRole('button', { name: 'Plus d\'actions pour ce serveur' }).click();
  await expect(owner.page.getByRole('button', { name: 'Quitter le serveur' })).toHaveCount(0);
  await expect(owner.page.getByRole('link', { name: 'Transférer la propriété' })).toBeVisible();
  await owner.page.keyboard.press('Escape');
  const refused = await (await api(owner.token)).delete(`${API}/guilds/${gid}/members/@me`);
  expect(refused.status()).toBe(403);
  expect((await refused.json()).code).toBe('owner_leave');

  // Le membre annule d'abord (Échap), puis confirme.
  await crew.page.getByRole('button', { name: 'Plus d\'actions pour ce serveur' }).click();
  await crew.page.getByRole('button', { name: 'Quitter le serveur' }).click();
  const dialog = crew.page.getByRole('alertdialog', { name: 'Quitter « Phare » ?' });
  await expect(dialog).toBeVisible();
  await crew.page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  await expect(crew.page.getByRole('link', { name: 'Serveur Phare' })).toBeVisible();
  await crew.page.getByRole('button', { name: 'Plus d\'actions pour ce serveur' }).click();
  await crew.page.getByRole('button', { name: 'Quitter le serveur' }).click();
  await dialog.getByRole('button', { name: 'Quitter le serveur' }).click();
  await expect(crew.page.getByRole('heading', { name: 'Amis', level: 1 })).toBeVisible();
  await expect(crew.page.getByRole('link', { name: 'Serveur Phare' })).toHaveCount(0);
  await expect(members(owner).getByText(crew.name)).toHaveCount(0);

  // Transfert : le nouveau propriétaire reçoit la couronne, l'ancien peut partir.
  await invite(api, owner, cid, crew);
  await owner.page.getByRole('button', { name: 'Plus d\'actions pour ce serveur' }).click();
  await owner.page.getByRole('link', { name: 'Transférer la propriété' }).click();
  await owner.page.getByLabel('Nouveau propriétaire').selectOption({ label: `${crew.name} (@${crew.name})` });
  await owner.page.getByRole('button', { name: 'Transférer…' }).click();
  await owner.page.getByRole('alertdialog').getByRole('button', { name: 'Transférer la propriété' }).click();
  await expect(owner.page.getByRole('alertdialog')).toHaveCount(0);
  await owner.page.keyboard.press('Escape');
  await owner.page.getByRole('button', { name: 'Plus d\'actions pour ce serveur' }).click();
  await expect(owner.page.getByRole('button', { name: 'Quitter le serveur' })).toBeVisible();
  await crew.page.getByRole('link', { name: 'Serveur Phare' }).click();
  await crew.page.getByRole('button', { name: 'Plus d\'actions pour ce serveur' }).click();
  await expect(crew.page.getByRole('link', { name: 'Transférer la propriété' })).toBeVisible();
});

test('S22-K1 réordonner les rôles au clavier : boutons Monter et Descendre, le focus reste sur le bouton', async ({ user, api }) => {
  const owner = await user('maitre');
  const [gid] = await createGuild(owner.page, 'Escadre');
  const http = await api(owner.token);
  for (const name of ['Alpha', 'Beta', 'Gamma']) {
    expect((await http.post(`${API}/guilds/${gid}/roles`, { data: { name } })).status()).toBe(201);
  }
  // Le dernier créé est le plus bas.
  expect(await roleNames(api, owner, gid)).toEqual(['Gamma', 'Beta', 'Alpha']);
  await owner.page.goto(`/guilds/${gid}/settings/roles`);
  const list = owner.page.getByRole('list', { name: 'Rôles, du plus haut au plus bas' });
  const order = () => list.getByRole('listitem').evaluateAll((els) => els.map((e) => e.querySelector('.app-role-pick')!.textContent!.trim()));
  await expect.poll(order).toEqual(['Gamma', 'Beta', 'Alpha', '@everyone']);
  await a11y(owner.page);

  // Aux extrémités, le bouton est inactif.
  await expect(owner.page.getByRole('button', { name: 'Monter le rôle Gamma' })).toBeDisabled();
  await expect(owner.page.getByRole('button', { name: 'Descendre le rôle Alpha' })).toBeDisabled();

  const down = owner.page.getByRole('button', { name: 'Descendre le rôle Gamma' });
  await down.focus();
  await owner.page.keyboard.press('Enter');
  await expect.poll(order).toEqual(['Beta', 'Gamma', 'Alpha', '@everyone']);
  await expect(down).toBeFocused();
  await owner.page.keyboard.press('Space');
  await expect.poll(order).toEqual(['Beta', 'Alpha', 'Gamma', '@everyone']);
  // Arrivé en bas, le focus passe sur « Monter » plutôt que de se perdre.
  await expect(owner.page.getByRole('button', { name: 'Monter le rôle Gamma' })).toBeFocused();
  await owner.page.keyboard.press('Enter');
  await expect.poll(order).toEqual(['Beta', 'Gamma', 'Alpha', '@everyone']);
  expect(await roleNames(api, owner, gid)).toEqual(['Beta', 'Gamma', 'Alpha']);

  // Glisser-déposer : Alpha tout en haut.
  await list.getByRole('listitem').filter({ hasText: 'Alpha' }).dragTo(list.getByRole('listitem').filter({ hasText: 'Beta' }));
  await expect.poll(order).toEqual(['Alpha', 'Beta', 'Gamma', '@everyone']);
  expect(await roleNames(api, owner, gid)).toEqual(['Alpha', 'Beta', 'Gamma']);
});

test('S22-N1 hiérarchie : un modérateur ne touche ni au propriétaire ni à un rang égal ou supérieur', async ({ user, api }) => {
  const owner = await user('roi');
  const mod = await user('garde');
  const peer = await user('garde2');
  const [gid, cid] = await createGuild(owner.page, 'Citadelle');
  await invite(api, owner, cid, mod);
  await invite(api, owner, cid, peer);
  const http = await api(owner.token);
  const perms = (1 << 22) | (1 << 23) | (1 << 19); // KICK | BAN | MANAGE_ROLES
  const role = await (await http.post(`${API}/guilds/${gid}/roles`, { data: { name: 'Gardes', permissions: perms } })).json();
  for (const u of [mod, peer]) expect((await http.put(`${API}/guilds/${gid}/members/${u.id}/roles/${role.id}`)).status()).toBe(200);

  await mod.page.getByRole('link', { name: 'Serveur Citadelle' }).click();
  await expect(members(mod).getByText(peer.name)).toBeVisible();
  // Menu : rien pour le propriétaire ni pour le pair ; la fiche refuse de changer ses rôles.
  for (const target of [owner.name, peer.name]) {
    await moreFor(mod, target).click();
    await expect(mod.page.getByRole('button', { name: 'Expulser' })).toHaveCount(0);
    await expect(mod.page.getByRole('button', { name: 'Bannir' })).toHaveCount(0);
    await mod.page.keyboard.press('Escape');
  }
  // Côté serveur, c'est refusé aussi (403), et le serveur n'a pas bougé.
  const asMod = await api(mod.token);
  for (const [method, path] of [
    ['delete', `/guilds/${gid}/members/${owner.id}`],
    ['delete', `/guilds/${gid}/members/${peer.id}`],
    ['put', `/guilds/${gid}/bans/${peer.id}`],
  ] as const) {
    const res = await asMod[method](`${API}${path}`);
    expect(res.status(), path).toBe(403);
    expect((await res.json()).code).toBe('hierarchy');
  }
  // Un simple membre n'a accès à aucune section des réglages.
  const plain = await user('simple');
  await invite(api, owner, cid, plain);
  await plain.page.goto(`/guilds/${gid}/settings`);
  await expect(plain.page.getByText('Tes rôles ne te donnent accès à aucun réglage de ce serveur.')).toBeVisible();
  expect((await (await api(plain.token)).get(`${API}/guilds/${gid}/bans`)).status()).toBe(403);
});
