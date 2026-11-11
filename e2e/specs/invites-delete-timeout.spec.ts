// Invitations (liste, copie, révocation, liens morts), suppression d'un serveur, exclusion
// temporaire d'un membre.
import { test, expect, a11y, createGuild, API, type User } from '../fixtures';
import type { APIRequestContext, Locator, Page } from '@playwright/test';

type Fixture = { user: (prefix?: string) => Promise<User>; api: (token?: string) => Promise<APIRequestContext> };

// Bits des droits (crates/kraken-core/src/perms.rs).
const TIMEOUT = 1 << 24;
const ADMINISTRATOR = 1 << 30;

/** Invite `who` dans le serveur par l'API ; il le voit apparaître dans son rail. */
async function invite(api: Fixture['api'], owner: User, cid: string, who: User) {
  const { code } = await (await (await api(owner.token)).post(`${API}/channels/${cid}/invites`, { data: {} })).json();
  const res = await (await api(who.token)).post(`${API}/invites/${code}`);
  expect(res.status()).toBe(200);
  return code as string;
}

const reglages = (u: User) => u.page.getByRole('dialog', { name: 'Paramètres du serveur' });
const invitations = (u: User) => reglages(u).getByRole('list', { name: 'Invitations' }).getByRole('listitem');
const members = (u: User) => u.page.getByRole('complementary', { name: 'Membres' });
const moreFor = (u: User, name: string) => u.page.getByRole('button', { name: `Plus d'actions pour ${name}` });
const composer = (u: User) => u.page.getByRole('textbox', { name: /^(Écrire dans|Exclu jusqu)/ });
const EXCLU = /Exclu jusqu.à /;

/** La fenêtre est entière à l'écran et rien ne la recouvre (le rail et le tiroir passent dessous). */
async function fullyShown(dialog: Locator) {
  await expect(dialog).toBeVisible();
  const box = (await dialog.boundingBox())!;
  const { width, height } = dialog.page().viewportSize()!;
  expect(box.x).toBeGreaterThanOrEqual(0);
  expect(box.x + box.width).toBeLessThanOrEqual(width);
  expect(box.y).toBeGreaterThanOrEqual(0);
  expect(box.y + box.height).toBeLessThanOrEqual(height);
  const covered = await dialog.evaluate((el) => {
    const r = el.getBoundingClientRect();
    const title = el.querySelector('h2, h3')!.getBoundingClientRect();
    const pts: [number, number][] = [
      [r.left + 4, r.top + 4], [r.right - 4, r.top + 4], [r.left + 4, r.bottom - 4], [r.right - 4, r.bottom - 4],
      [title.left + 2, title.top + title.height / 2], [title.right - 2, title.top + title.height / 2],
    ];
    return pts.filter(([x, y]) => !el.contains(document.elementFromPoint(x, y))).length;
  });
  expect(covered, 'des éléments recouvrent la fenêtre').toBe(0);
}

async function noScroll(page: Page) {
  const w = page.viewportSize()!.width;
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(w);
}

test('F06-H2 invitations : créer avec une durée et des usages, copier le lien, révoquer ; le lien révoqué dit pourquoi', async ({ user, allow }) => {
  allow.push(/status of 404/);
  const owner = await user('chef');
  const guest = await user('invite');
  await owner.context.grantPermissions(['clipboard-read', 'clipboard-write']);
  const [, cid] = await createGuild(owner.page, 'Relais');
  void cid;
  await owner.page.getByRole('link', { name: 'Paramètres du serveur' }).click();
  await reglages(owner).getByRole('link', { name: 'Invitations' }).click();
  await expect(reglages(owner).getByRole('heading', { name: 'Invitations', level: 2 })).toBeVisible();
  await expect(reglages(owner).getByText('Aucune invitation')).toBeVisible();

  // Le formulaire : sept durées (7 jours par défaut) et sept nombres d'utilisations (illimité par défaut).
  const age = reglages(owner).getByLabel('Expire après');
  const uses = reglages(owner).getByLabel("Nombre d'utilisations");
  await expect(age).toHaveValue('604800');
  expect(await age.locator('option').allTextContents()).toEqual(['30 minutes', '1 heure', '6 heures', '12 heures', '1 jour', '7 jours', 'Jamais']);
  await expect(uses).toHaveValue('0');
  expect(await uses.locator('option').allTextContents()).toEqual(['Illimité', '1 utilisation', '5 utilisations', '10 utilisations', '25 utilisations', '50 utilisations', '100 utilisations']);
  await age.selectOption({ label: '1 heure' });
  await uses.selectOption({ label: '10 utilisations' });
  await reglages(owner).getByRole('button', { name: 'Créer une invitation' }).click();

  const row = invitations(owner).first();
  await expect(row).toBeVisible();
  const code = (await row.locator('code').innerText()).trim();
  expect(code).toMatch(/^[A-Za-z0-9]{6,}$/);
  await expect(reglages(owner).getByText(`Invitation ${code} créée.`)).toBeVisible();
  await expect(row).toContainText(`Créée par ${owner.name}`);
  await expect(row.getByText('0 / 10', { exact: true })).toBeVisible();
  await expect(row).toContainText(/expire dans (1 h|60 min)/);
  // Un code en police à chasse fixe.
  expect(await row.locator('code').evaluate((el) => getComputedStyle(el).fontFamily)).toMatch(/mono/i);
  await a11y(owner.page);

  // Copier : « Copié » annoncé dans un role=status de la ligne, et le presse-papiers porte le lien.
  await row.getByRole('button', { name: `Copier le lien de l'invitation ${code}` }).click();
  await expect(row.getByRole('status')).toHaveText('Copié');
  const link = await owner.page.evaluate(() => navigator.clipboard.readText());
  expect(link).toMatch(new RegExp(`/invite/${code}$`));

  // Le lien marche : l'invité rejoint, le compteur passe à 1 / 10 au rechargement de l'onglet.
  await guest.page.goto(`/invite/${code}`);
  await guest.page.getByRole('button', { name: 'Rejoindre le serveur' }).click();
  await expect(guest.page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
  await owner.page.reload();
  await expect(reglages(owner).getByText('1 / 10', { exact: true })).toBeVisible();

  // Révoquer : l'entrée danger du menu ⋯, la ligne part, le lien cesse de marcher aussitôt.
  const more = invitations(owner).first().getByRole('button', { name: `Plus d'actions pour l'invitation ${code}` });
  await more.click();
  const revoke = owner.page.getByRole('button', { name: 'Révoquer' });
  await expect(revoke).toHaveClass(/is-danger/);
  await revoke.click();
  await expect(invitations(owner)).toHaveCount(0);
  await expect(reglages(owner).getByText(`Invitation ${code} révoquée.`)).toBeVisible();
  const late = await user('retard');
  await late.page.goto(`/invite/${code}`);
  await expect(late.page.getByRole('heading', { name: 'Invitation invalide', level: 1 })).toBeVisible();
  await expect(late.page.getByText('a été révoquée')).toBeVisible();
  await late.page.getByRole('link', { name: "Retour à l'accueil" }).click();
  await expect(late.page.getByRole('heading', { name: 'Amis', level: 1 })).toBeVisible();
});

test('F06-E4 invitations mortes : « Épuisée » et « Expirée » en texte, ligne discrète sans bouton Copier, révocable', async ({ user, api }) => {
  const owner = await user('chef');
  const [gid, cid] = await createGuild(owner.page, 'Vieux phare');
  const one = await (await (await api(owner.token)).post(`${API}/channels/${cid}/invites`, { data: { max_uses: 1 } })).json();
  const other = await user('seul');
  expect((await (await api(other.token)).post(`${API}/invites/${one.code}`)).status()).toBe(200);
  // Aucune invitation n'expire en moins de 30 minutes : on fait répondre une liste où l'une a expiré.
  await owner.page.route(`**/api/v1/guilds/${gid}/invites`, async (route) => {
    const res = await route.fetch();
    const rows = (await res.json()) as Record<string, unknown>[];
    const old = { ...rows[0], code: 'EXPIRE01', uses: 0, max_uses: 0, state: 'expired', expires_at: Date.now() - 3_600_000 };
    await route.fulfill({ response: res, json: [...rows, old] });
  });
  await owner.page.getByRole('link', { name: 'Paramètres du serveur' }).click();
  await reglages(owner).getByRole('link', { name: 'Invitations' }).click();

  const spent = invitations(owner).filter({ hasText: one.code });
  await expect(spent.getByText('Épuisée')).toBeVisible();
  await expect(spent.getByText('1 / 1', { exact: true })).toBeVisible();
  await expect(spent.getByRole('button', { name: /^Copier/ })).toHaveCount(0);
  const dead = invitations(owner).filter({ hasText: 'EXPIRE01' });
  await expect(dead.getByText('Expirée')).toBeVisible();
  await expect(dead).toContainText('a expiré');
  await expect(dead.getByRole('button', { name: /^Copier/ })).toHaveCount(0);
  // Plus discrètes que les vivantes (par la couleur), mais lisibles : axe vérifie le contraste.
  await expect(dead).toHaveAttribute('data-dead', '');
  await a11y(owner.page);
  // Une morte se révoque pour faire le ménage (la vraie : l'expirée est un décor de ce test).
  await spent.getByRole('button', { name: `Plus d'actions pour l'invitation ${one.code}` }).click();
  await owner.page.getByRole('button', { name: 'Révoquer' }).click();
  await expect(reglages(owner).getByText(`Invitation ${one.code} révoquée.`)).toBeVisible();
  await expect(spent).toHaveCount(0);
});

test('F06-N3 liens morts : épuisé, expiré, révoqué ou inconnu, chacun dit pourquoi avec un retour à l’accueil ; l’API répond 404 avec un code', async ({ user, api, allow, page }) => {
  allow.push(/status of 404/);
  const owner = await user('chef');
  const [, cid] = await createGuild(owner.page, 'Quai');
  const http = await api(owner.token);
  const one = await (await http.post(`${API}/channels/${cid}/invites`, { data: { max_uses: 1 } })).json();
  const first = await user('premier');
  expect((await (await api(first.token)).post(`${API}/invites/${one.code}`)).status()).toBe(200);

  // Épuisée : 404 `invite_exhausted`, en lecture comme en adhésion.
  const second = await user('second');
  const http2 = await api(second.token);
  for (const call of [() => http2.get(`${API}/invites/${one.code}`), () => http2.post(`${API}/invites/${one.code}`)]) {
    const res = await call();
    expect(res.status()).toBe(404);
    expect((await res.json()).code).toBe('invite_exhausted');
  }
  await second.page.goto(`/invite/${one.code}`);
  await expect(second.page.getByRole('heading', { name: 'Invitation invalide', level: 1 })).toBeVisible();
  await expect(second.page.getByText('déjà servi autant de fois que permis')).toBeVisible();
  await expect(second.page.getByRole('link', { name: "Retour à l'accueil" })).toHaveAttribute('href', '/channels/@me');
  await a11y(second.page);

  // Révoquée : l'API dit « inconnue » (elle a disparu), et la page le dit.
  const live = await (await http.post(`${API}/channels/${cid}/invites`, { data: {} })).json();
  expect((await http.delete(`${API}/invites/${live.code}`)).status()).toBe(204);
  const res = await http2.get(`${API}/invites/${live.code}`);
  expect([res.status(), (await res.json()).code]).toEqual([404, 'invite_unknown']);
  expect((await http2.delete(`${API}/invites/${live.code}`)).status()).toBe(404);

  // Expirée : la page reprend le message du serveur.
  await page.route('**/api/v1/invites/EXPIREE1', (route) =>
    route.fulfill({ status: 404, json: { code: 'invite_expired', message: 'Cette invitation a expiré. Demande-en une nouvelle.' } }));
  await page.goto('/invite/EXPIREE1');
  await expect(page.getByRole('heading', { name: 'Invitation invalide', level: 1 })).toBeVisible();
  await expect(page.getByText('Cette invitation a expiré.')).toBeVisible();
  await a11y(page);
});

test('F06-N4 invitations : durée au-delà de 7 jours et nombre d’usages démesuré refusés ; qui ne gère rien ne révoque pas celle d’un autre', async ({ user, api }) => {
  const owner = await user('chef');
  const member = await user('membre');
  const [gid, cid] = await createGuild(owner.page, 'Écluse');
  await invite(api, owner, cid, member);
  const http = await api(owner.token);
  for (const bad of [{ max_age: 7 * 86400 + 1 }, { max_uses: 1001 }]) {
    const res = await http.post(`${API}/channels/${cid}/invites`, { data: bad });
    expect(res.status(), JSON.stringify(bad)).toBe(400);
  }
  expect((await http.post(`${API}/channels/${cid}/invites`, { data: { max_age: 7 * 86400, max_uses: 0 } })).status()).toBe(201);
  expect((await http.post(`${API}/channels/${cid}/invites`, { data: { max_age: null } })).status()).toBe(201);

  // Le membre ne voit que les siennes ; il ne révoque pas celle du propriétaire.
  const mine = await (await (await api(member.token)).post(`${API}/channels/${cid}/invites`, { data: {} })).json();
  const theirs = await (await http.post(`${API}/channels/${cid}/invites`, { data: {} })).json();
  const list = await (await (await api(member.token)).get(`${API}/guilds/${gid}/invites`)).json();
  expect(list.map((i: { code: string }) => i.code)).toEqual([mine.code]);
  expect((await (await api(member.token)).delete(`${API}/invites/${theirs.code}`)).status()).toBe(403);
  // Le gérant les voit toutes, et révoque celle du membre.
  const all = await (await http.get(`${API}/guilds/${gid}/invites`)).json();
  expect(all.length).toBeGreaterThanOrEqual(4);
  expect((await http.delete(`${API}/invites/${mine.code}`)).status()).toBe(204);

  // Dans l'interface, le membre n'a que l'onglet Invitations, et un texte qui le dit.
  await member.page.getByRole('link', { name: 'Serveur Écluse' }).click();
  await member.page.getByRole('link', { name: 'Paramètres du serveur' }).click();
  await expect(reglages(member).getByRole('link')).toHaveText(['Invitations']);
  await expect(reglages(member).getByText('Tu ne vois que les invitations que tu as créées.')).toBeVisible();
  await a11y(member.page);
});

test('S22-H5 supprimer un serveur : zone dangereuse, nom à retaper, Échap ne ferme que la fenêtre du dessus ; il disparaît chez tous', async ({ user, api }) => {
  const owner = await user('chef');
  const crew = await user('matelot');
  const [gid, cid] = await createGuild(owner.page, 'Épave');
  await invite(api, owner, cid, crew);
  await crew.page.getByRole('link', { name: 'Serveur Épave' }).click();
  await expect(crew.page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();

  await owner.page.getByRole('link', { name: 'Paramètres du serveur' }).click();
  await reglages(owner).getByRole('link', { name: 'Propriété' }).click();
  const zone = reglages(owner).getByRole('region', { name: 'Zone dangereuse' });
  await expect(zone).toContainText('salons, ses messages, ses rôles et ses invitations');
  await a11y(owner.page);

  await zone.getByRole('button', { name: 'Supprimer le serveur…' }).click();
  const dialog = owner.page.getByRole('alertdialog', { name: 'Supprimer « Épave » ?' });
  await expect(dialog).toBeVisible();
  for (const what of ['les salons', 'les rôles', 'les invitations', 'irréversible']) await expect(dialog).toContainText(what);
  // Le focus part sur « Annuler », pas sur l'action.
  await expect(dialog.getByRole('button', { name: 'Annuler' })).toBeFocused();
  const go = dialog.getByRole('button', { name: 'Supprimer le serveur', exact: true });
  await expect(go).toBeDisabled();
  await a11y(owner.page);
  // Échap ferme la fenêtre du dessus seulement ; le focus revient à son déclencheur.
  await owner.page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  await expect(reglages(owner)).toBeVisible();
  await expect(zone.getByRole('button', { name: 'Supprimer le serveur…' })).toBeFocused();

  // Le bouton ne s'active qu'au nom exact (les espaces autour ne comptent pas).
  await zone.getByRole('button', { name: 'Supprimer le serveur…' }).click();
  const name = dialog.getByLabel('Tape le nom du serveur');
  await name.fill('epave');
  await expect(go).toBeDisabled();
  await name.fill('Épav');
  await expect(go).toBeDisabled();
  await name.fill(' Épave ');
  await expect(go).toBeEnabled();
  await go.click();

  // Pour le propriétaire : retour à l'accueil, plus de serveur dans le rail. Pour l'autre aussi, en direct.
  for (const u of [owner, crew]) {
    await expect(u.page.getByRole('heading', { name: 'Amis', level: 1 })).toBeVisible();
    await expect(u.page.getByRole('link', { name: 'Serveur Épave' })).toHaveCount(0);
    expect((await (await api(u.token)).get(`${API}/guilds/${gid}`)).status()).toBe(404);
    expect((await (await api(u.token)).get(`${API}/channels/${cid}/messages`)).status()).toBe(404);
  }
});

test('S22-N2 supprimer un serveur : refusé à qui n’en est pas propriétaire ; le nom doit correspondre côté serveur', async ({ user, api }) => {
  const owner = await user('chef');
  const crew = await user('adjoint');
  const [gid, cid] = await createGuild(owner.page, 'Citadelle');
  await invite(api, owner, cid, crew);
  const http = await api(owner.token);
  // Même avec tous les droits, un administrateur n'est pas le propriétaire.
  const role = await (await http.post(`${API}/guilds/${gid}/roles`, { data: { name: 'Admins', permissions: ADMINISTRATOR, color: 1, hoist: false } })).json();
  expect((await http.put(`${API}/guilds/${gid}/members/${crew.id}/roles/${role.id}`)).status()).toBe(200);
  const other = await api(crew.token);
  expect((await other.delete(`${API}/guilds/${gid}`, { data: { name: 'Citadelle' } })).status()).toBe(403);
  for (const data of [{ name: 'citadel' }, { name: '' }, undefined]) {
    const res = await http.delete(`${API}/guilds/${gid}`, data ? { data } : {});
    expect(res.status(), JSON.stringify(data)).toBe(400);
    expect((await res.json()).code).toBe('name_mismatch');
  }
  expect((await http.get(`${API}/guilds/${gid}`)).status()).toBe(200);

  // Dans l'interface, l'onglet « Propriété » (et sa zone dangereuse) n'existe que pour le propriétaire.
  await crew.page.getByRole('link', { name: 'Serveur Citadelle' }).click();
  await crew.page.getByRole('link', { name: 'Paramètres du serveur' }).click();
  await expect(reglages(crew).getByRole('link', { name: 'Propriété' })).toHaveCount(0);
  await expect(reglages(crew).getByText('Zone dangereuse')).toHaveCount(0);
  await crew.page.goto(`/guilds/${gid}/settings/owner`);
  await expect(reglages(crew).getByText('Zone dangereuse')).toHaveCount(0);
});

test('S22-H6 exclusion temporaire : durées et raison, pastille dans la liste, saisie grisée chez l’exclu, lecture permise, levée en direct', async ({ user, api }) => {
  const owner = await user('chef');
  const crew = await user('matelot');
  const [gid, cid] = await createGuild(owner.page, 'Brume');
  await invite(api, owner, cid, crew);
  await crew.page.getByRole('link', { name: 'Serveur Brume' }).click();
  await expect(members(crew).getByText(owner.name)).toBeVisible();
  await expect(members(owner).getByText(crew.name)).toBeVisible();

  // « Exclure temporairement… » est distinct d'Expulser et de Bannir, et ouvre une petite fenêtre.
  await moreFor(owner, crew.name).click();
  await expect(owner.page.getByRole('button', { name: 'Expulser' })).toBeVisible();
  await owner.page.getByRole('button', { name: 'Exclure temporairement…' }).click();
  const dialog = owner.page.getByRole('dialog', { name: `Exclure ${crew.name} temporairement` });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole('button', { name: 'Annuler' })).toBeFocused();
  const group = dialog.getByRole('radiogroup', { name: 'Durée' });
  const labels = ['60 secondes', '5 minutes', '10 minutes', '1 heure', '1 jour', '1 semaine'];
  for (const l of labels) await expect(group.getByRole('radio', { name: l, exact: true })).toBeVisible();
  await expect(group.getByRole('radio', { name: '60 secondes' })).toBeChecked();
  await group.getByRole('radio', { name: '10 minutes' }).check();
  await expect(group.getByRole('radio', { name: '60 secondes' })).not.toBeChecked();
  await dialog.getByLabel('Raison (facultatif)').fill('trop de bruit');
  await a11y(owner.page);
  // Échap annule sans rien faire.
  await owner.page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  await expect(moreFor(owner, crew.name)).toBeFocused();
  await expect(composer(crew)).toBeEnabled();

  await moreFor(owner, crew.name).click();
  await owner.page.getByRole('button', { name: 'Exclure temporairement…' }).click();
  await dialog.getByRole('radio', { name: '10 minutes' }).check();
  await dialog.getByLabel('Raison (facultatif)').fill('trop de bruit');
  await dialog.getByRole('button', { name: 'Exclure', exact: true }).click();
  await expect(dialog).toHaveCount(0);

  // La liste des membres : une pastille « Exclu jusqu'à HH:MM » chez le propriétaire comme chez l'exclu.
  const badge = (u: User) => members(u).getByRole('listitem').filter({ hasText: crew.name }).getByText(EXCLU);
  await expect(badge(owner)).toBeVisible();
  await expect(badge(crew)).toBeVisible();
  // Chez l'exclu : la zone de saisie est grisée et dit jusqu'à quand ; pas de compte à rebours.
  await expect(composer(crew)).toBeDisabled();
  await expect(composer(crew)).toHaveAccessibleName(EXCLU);
  for (const b of ['Envoyer', 'Insérer un emoji']) await expect(crew.page.getByRole('button', { name: b })).toBeDisabled();
  await expect(crew.page.getByText('Tu peux lire, pas écrire.')).toBeVisible();
  await a11y(crew.page);
  await a11y(owner.page);

  // Le serveur refuse pour de bon, avec la fin ; lire reste possible.
  const refused = await (await api(crew.token)).post(`${API}/channels/${cid}/messages`, { data: { content: 'encore moi' } });
  expect(refused.status()).toBe(403);
  const body = await refused.json();
  expect(body.code).toBe('timed_out');
  expect(body.timeout_until).toBeGreaterThan(Date.now() + 9 * 60_000);
  const sent = await (await api(owner.token)).post(`${API}/channels/${cid}/messages`, { data: { content: 'silence à bord' } });
  expect(sent.status()).toBe(201);
  await expect(crew.page.getByRole('log').getByText('silence à bord')).toBeVisible();
  expect((await (await api(crew.token)).get(`${API}/channels/${cid}/messages`)).status()).toBe(200);

  // Lever l'exclusion : la saisie se rouvre tout de suite chez lui.
  await moreFor(owner, crew.name).click();
  await expect(owner.page.getByRole('button', { name: 'Exclure temporairement…' })).toHaveCount(0);
  await owner.page.getByRole('button', { name: "Lever l'exclusion" }).click();
  await expect(badge(owner)).toHaveCount(0);
  await expect(badge(crew)).toHaveCount(0);
  await expect(composer(crew)).toBeEnabled();
  await expect(composer(crew)).toHaveAccessibleName('Écrire dans #général');
  await composer(crew).fill('merci');
  await composer(crew).press('Enter');
  await expect(owner.page.getByRole('log').getByText('merci')).toBeVisible();
  void gid;
});

test('S22-E1 exclusion temporaire : la saisie se rouvre toute seule à l’heure dite, sans rechargement', async ({ user, api }) => {
  const owner = await user('chef');
  const crew = await user('matelot');
  const [, cid] = await createGuild(owner.page, 'Marée');
  await invite(api, owner, cid, crew);
  await crew.page.getByRole('link', { name: 'Serveur Marée' }).click();
  await expect(composer(crew)).toBeEnabled();
  // Horloge du navigateur de l'exclu sous contrôle : une minute passe d'un coup.
  await crew.page.clock.install();
  const res = await (await api(owner.token)).put(`${API}/guilds/${new URL(crew.page.url()).pathname.split('/')[2]}/members/${crew.id}/timeout`, { data: { seconds: 60 } });
  expect(res.status()).toBe(200);
  await expect(composer(crew)).toBeDisabled();
  await expect(composer(crew)).toHaveAccessibleName(EXCLU);
  await crew.page.clock.fastForward('01:10');
  await expect(composer(crew)).toBeEnabled();
  await expect(composer(crew)).toHaveAccessibleName('Écrire dans #général');
  await expect(members(crew).getByText(EXCLU)).toHaveCount(0);
});

test('S22-N3 exclusion temporaire : ni soi-même, ni le propriétaire, ni un rang égal ou supérieur, ni sans le droit', async ({ user, api }) => {
  const owner = await user('chef');
  const mod = await user('moderateur');
  const plain = await user('simple');
  const [gid, cid] = await createGuild(owner.page, 'Digue');
  await invite(api, owner, cid, mod);
  await invite(api, owner, cid, plain);
  const http = await api(owner.token);
  const role = await (await http.post(`${API}/guilds/${gid}/roles`, { data: { name: 'Modération', permissions: TIMEOUT, color: 2, hoist: true } })).json();
  expect((await http.put(`${API}/guilds/${gid}/members/${mod.id}/roles/${role.id}`)).status()).toBe(200);
  for (const u of [mod, plain]) await u.page.getByRole('link', { name: 'Serveur Digue' }).click();
  await expect(members(mod).getByText(plain.name)).toBeVisible();

  // Le modérateur peut exclure le simple membre, mais le menu du propriétaire et du sien ne le propose pas.
  await moreFor(mod, owner.name).click();
  await expect(mod.page.getByRole('button', { name: 'Profil et rôles' })).toBeVisible();
  await expect(mod.page.getByRole('button', { name: 'Exclure temporairement…' })).toHaveCount(0);
  await mod.page.keyboard.press('Escape');
  await moreFor(mod, mod.name).click();
  await expect(mod.page.getByRole('button', { name: 'Exclure temporairement…' })).toHaveCount(0);
  await mod.page.keyboard.press('Escape');
  await moreFor(mod, plain.name).click();
  await expect(mod.page.getByRole('button', { name: 'Exclure temporairement…' })).toBeVisible();
  await mod.page.keyboard.press('Escape');
  // Sans le droit, rien non plus.
  await moreFor(plain, mod.name).click();
  await expect(plain.page.getByRole('button', { name: 'Exclure temporairement…' })).toHaveCount(0);
  await plain.page.keyboard.press('Escape');

  // Le serveur reste le juge.
  const modApi = await api(mod.token);
  const put = (who: User, seconds = 60) => modApi.put(`${API}/guilds/${gid}/members/${who.id}/timeout`, { data: { seconds } });
  expect((await put(owner)).status()).toBe(403);
  expect((await put(mod)).status()).toBe(400);
  expect((await put(plain, 59)).status()).toBe(400);
  expect((await put(plain, 28 * 86400 + 1)).status()).toBe(400);
  expect((await (await api(plain.token)).put(`${API}/guilds/${gid}/members/${mod.id}/timeout`, { data: { seconds: 60 } })).status()).toBe(403);
  expect((await put(plain, 28 * 86400)).status()).toBe(200);
  // Un administrateur reste intouchable, même par un rang au-dessus de lui.
  const adm = await user('admin');
  await invite(api, owner, cid, adm);
  const admins = await (await http.post(`${API}/guilds/${gid}/roles`, { data: { name: 'Admins', permissions: ADMINISTRATOR, color: 3, hoist: false } })).json();
  expect((await http.put(`${API}/guilds/${gid}/members/${adm.id}/roles/${admins.id}`)).status()).toBe(200);
  const refused = await http.put(`${API}/guilds/${gid}/members/${adm.id}/timeout`, { data: { seconds: 60 } });
  expect(refused.status()).toBe(403);
  // Réagir, joindre un fichier et parler sont aussi refusés à celui qui est exclu.
  const msg = await (await http.post(`${API}/channels/${cid}/messages`, { data: { content: 'bonjour' } })).json();
  const plainApi = await api(plain.token);
  const react = await plainApi.put(`${API}/channels/${cid}/messages/${msg.id}/reactions/${encodeURIComponent('👍')}/@me`);
  expect([react.status(), (await react.json()).code]).toEqual([403, 'timed_out']);
  const up = await plainApi.post(`${API}/channels/${cid}/attachments?filename=a.txt`, { headers: { 'content-type': 'text/plain' }, data: 'salut' });
  expect([up.status(), (await up.json()).code]).toEqual([403, 'timed_out']);
});

test.describe('largeur mobile', () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test('S22-E2 à 390 px : invitations, suppression du serveur et exclusion temporaire tiennent à l’écran, devant le rail', async ({ user, api }) => {
    const owner = await user('chef');
    const crew = await user('matelot');
    const res = await (await api(owner.token)).post(`${API}/guilds`, { data: { name: 'Poche' } });
    const g = await res.json();
    const gid = g.guild.id as string;
    const cid = g.channels.find((c: { kind: string }) => c.kind === 'text').id as string;
    await invite(api, owner, cid, crew);
    const page = owner.page;

    // Invitations : le formulaire et une ligne, sans défilement horizontal.
    await (await api(owner.token)).post(`${API}/channels/${cid}/invites`, { data: { max_uses: 5 } });
    await page.goto(`/guilds/${gid}/settings/invites`);
    await expect(reglages(owner).getByLabel('Expire après')).toBeVisible();
    await expect(invitations(owner).first()).toBeVisible();
    await noScroll(page);
    await a11y(page);

    // La fenêtre de suppression : entière, devant tout, avec le champ et les deux boutons à portée.
    await page.goto(`/guilds/${gid}/settings/owner`);
    await page.getByRole('button', { name: 'Supprimer le serveur…' }).click();
    const del = page.getByRole('alertdialog', { name: 'Supprimer « Poche » ?' });
    await fullyShown(del);
    await expect(del.getByLabel('Tape le nom du serveur')).toBeVisible();
    for (const b of [del.getByRole('button', { name: 'Annuler' }), del.getByRole('button', { name: 'Supprimer le serveur', exact: true })]) {
      const box = (await b.boundingBox())!;
      expect(box.height).toBeGreaterThanOrEqual(35.5);
    }
    await noScroll(page);
    await a11y(page);
    await page.keyboard.press('Escape');
    await expect(del).toHaveCount(0);

    // L'exclusion : depuis la liste des membres, qui est un tiroir par-dessus le fil.
    await page.goto(`/channels/${gid}/${cid}`);
    await page.getByRole('button', { name: 'Liste des membres' }).click();
    await moreFor(owner, crew.name).click();
    await page.getByRole('button', { name: 'Exclure temporairement…' }).click();
    const dialog = page.getByRole('dialog', { name: `Exclure ${crew.name} temporairement` });
    await fullyShown(dialog);
    await noScroll(page);
    await a11y(page);
    await dialog.getByRole('radio', { name: '5 minutes' }).check();
    await dialog.getByRole('button', { name: 'Exclure', exact: true }).click();
    await expect(dialog).toHaveCount(0);
    await expect(members(owner).getByText(EXCLU)).toBeVisible();
    await noScroll(page);
    await a11y(page);
  });
});
