// Journal d'audit : Réglages › Journal d'audit (lecture seule). L'onglet n'existe que pour qui a « Voir le
// journal d'audit » ; chaque action réelle (rôle, salon, emoji, exclusion, expulsion, bannissement…) y devient
// une phrase lisible, avec le motif, l'heure et l'avant / après quand il y en a un ; filtres, pagination par
// « Charger plus » (focus sur la première ligne arrivée), états vides et d'erreur, texte jamais pris pour du HTML,
// 390 px, contraste élevé, texte 20 et zoom 150 %, clavier seul.
import { test, expect, a11y, createGuild, API, type User } from '../fixtures';
import { png } from '../images';
import type { APIRequestContext, Locator, Page } from '@playwright/test';

type Api = (t?: string) => Promise<APIRequestContext>;

const VIEW_AUDIT_LOG = 1 << 26;
const MANAGE_CHANNELS = 1 << 18;
const MANAGE_EMOJIS = 1 << 20;

const settings = (p: Page) => p.getByRole('dialog', { name: 'Paramètres du serveur' });
const nav = (p: Page) => settings(p).getByRole('navigation').getByRole('link');
const rows = (p: Page) => settings(p).getByRole('list', { name: 'Événements du journal' }).getByRole('listitem');
const status = (p: Page) => settings(p).getByRole('status');
const row = (p: Page, text: string | RegExp) => rows(p).filter({ hasText: text });
const noOverflow = (p: Page) => p.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth);
const open = async (p: Page, gid: string) => {
  await p.goto(`/guilds/${gid}/settings/audit`);
  await expect(settings(p).getByRole('heading', { name: "Journal d'audit", level: 2 })).toBeVisible();
};

/** Crée un serveur par l'API (à 390 px le rail est dans le tiroir) ; rend son id et celui de #général. */
async function makeGuild(as: APIRequestContext, name: string): Promise<[string, string]> {
  const res = await as.post(`${API}/guilds`, { data: { name } });
  expect(res.status(), await res.text()).toBe(201);
  const g = await res.json();
  return [g.guild.id, g.channels.find((c: { kind: string }) => c.kind === 'text').id];
}

/** `guest` rejoint le serveur du salon `cid`, invité par `owner`. */
async function invite(api: Api, owner: User, cid: string, guest: User) {
  const { code } = await (await (await api(owner.token)).post(`${API}/channels/${cid}/invites`, { data: {} })).json();
  expect((await (await api(guest.token)).post(`${API}/invites/${code}`)).status()).toBe(200);
}

async function role(as: APIRequestContext, gid: string, name: string, permissions = 0) {
  const res = await as.post(`${API}/guilds/${gid}/roles`, { data: { name, permissions } });
  expect(res.status(), await res.text()).toBe(201);
  return (await res.json()) as { id: string };
}

const give = async (as: APIRequestContext, gid: string, who: User, r: { id: string }) =>
  expect((await as.put(`${API}/guilds/${gid}/members/${who.id}/roles/${r.id}`)).status()).toBe(200);

async function channel(as: APIRequestContext, gid: string, name: string) {
  const res = await as.post(`${API}/guilds/${gid}/channels`, { data: { name, kind: 'text' } });
  expect(res.status(), await res.text()).toBe(201);
  return (await res.json()) as { id: string };
}

/** Contraste WCAG entre deux couleurs CSS `rgb(r, g, b)`. */
function ratio(a: string, b: string) {
  const lum = (c: string) => {
    const [r, g, bl] = (c.match(/[\d.]+/g) ?? ['0', '0', '0']).slice(0, 3).map((v) => {
      const x = Number(v) / 255;
      return x <= 0.04045 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * r + 0.7152 * g + 0.0722 * bl;
  };
  const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/** Couleur de texte et fond effectif (le premier non transparent en remontant) d'un élément. */
const colors = (loc: Locator) =>
  loc.evaluate((el) => {
    let bg = 'rgba(0, 0, 0, 0)';
    for (let n: Element | null = el; n; n = n.parentElement) {
      const c = getComputedStyle(n).backgroundColor;
      if (!c.startsWith('rgba(0, 0, 0, 0')) {
        bg = c;
        break;
      }
    }
    return { fg: getComputedStyle(el).color, bg };
  });

test('audit-H1 l’onglet n’existe que pour qui peut lire le journal ; l’API refuse les autres', async ({ user, api }) => {
  const owner = await user('chef');
  const greffier = await user('greffier');
  const plain = await user('simple');
  const out = await user('dehors');
  const [gid, cid] = await createGuild(owner.page, 'Archives');
  await invite(api, owner, cid, greffier);
  await invite(api, owner, cid, plain);
  const asOwner = await api(owner.token);
  await give(asOwner, gid, greffier, await role(asOwner, gid, 'Greffiers', VIEW_AUDIT_LOG));

  // Le propriétaire : l'onglet vient juste après « Bannis ».
  await open(owner.page, gid);
  await expect(nav(owner.page)).toHaveText(['Rôles', 'Salons', 'Emojis', 'Invitations', 'Bannis', "Journal d'audit", 'Propriété']);
  await expect(nav(owner.page).filter({ hasText: "Journal d'audit" })).toHaveAttribute('aria-current', 'page');
  await a11y(owner.page);

  // Un rôle qui n'a que ce droit y accède, et ne voit rien d'autre à régler que les invitations de tout le monde.
  await open(greffier.page, gid);
  await expect(nav(greffier.page)).toHaveText(['Invitations', "Journal d'audit"]);

  // Sans le droit : pas d'onglet, l'adresse directe tombe sur la première section permise, l'API dit 403 ; hors du serveur, 404.
  await plain.page.goto(`/guilds/${gid}/settings/audit`);
  await expect(settings(plain.page).getByRole('heading', { name: 'Invitations', level: 2 })).toBeVisible();
  await expect(nav(plain.page).filter({ hasText: "Journal d'audit" })).toHaveCount(0);
  await expect(settings(plain.page).getByRole('list', { name: 'Événements du journal' })).toHaveCount(0);
  const refused = await (await api(plain.token)).get(`${API}/guilds/${gid}/audit-log`);
  expect(refused.status()).toBe(403);
  expect((await refused.json()).code).toBe('forbidden');
  expect((await (await api(out.token)).get(`${API}/guilds/${gid}/audit-log`)).status()).toBe(404);
  expect((await (await api()).get(`${API}/guilds/${gid}/audit-log`)).status()).toBe(401);

  // Retirer le rôle retire l'accès : la page suivante n'offre plus l'onglet.
  const gr = await (await asOwner.get(`${API}/guilds/${gid}`)).json();
  const rid = gr.roles.find((r: { name: string }) => r.name === 'Greffiers').id;
  expect((await asOwner.delete(`${API}/guilds/${gid}/members/${greffier.id}/roles/${rid}`)).status()).toBe(200);
  expect((await (await api(greffier.token)).get(`${API}/guilds/${gid}/audit-log`)).status()).toBe(403);
});

test('audit-H2 les actions réelles deviennent des phrases lisibles, avec motif, heure et avant / après ; rien n’est pris pour du HTML', async ({ user, api }) => {
  const owner = await user('chef');
  const sam = await user('sam');
  const moussa = await user('moussa');
  const lea = await user('lea');
  const [gid, cid] = await createGuild(owner.page, 'Galion');
  for (const u of [sam, moussa, lea]) await invite(api, owner, cid, u);
  const as = await api(owner.token);

  const modos = await role(as, gid, 'Modos');
  await give(as, gid, sam, modos);
  expect((await as.patch(`${API}/guilds/${gid}/roles/${modos.id}`, { data: { name: 'Modérateurs', color: 3 } })).status()).toBe(200);
  const chan = await channel(as, gid, 'annonces');
  expect((await as.patch(`${API}/channels/${chan.id}`, { data: { name: 'infos', slowmode_s: 30 } })).status()).toBe(200);
  const emoji = await (await as.post(`${API}/guilds/${gid}/emojis?name=poulpe`, { data: png(32, 32), headers: { 'content-type': 'application/octet-stream' } })).json();
  expect((await as.patch(`${API}/guilds/${gid}/emojis/${emoji.id}`, { data: { name: 'pieuvre' } })).status()).toBe(200);
  expect((await as.put(`${API}/guilds/${gid}/members/${sam.id}/timeout`, { data: { seconds: 3600, reason: 'pause café' } })).status()).toBe(200);
  expect((await as.delete(`${API}/guilds/${gid}/members/${moussa.id}?reason=hors%20sujet`)).status()).toBe(204);
  expect((await as.put(`${API}/guilds/${gid}/bans/${lea.id}`, { data: { reason: 'spam <b>gras</b> & <script>x</script>' } })).status()).toBe(200);
  expect((await as.delete(`${API}/guilds/${gid}/bans/${lea.id}`)).status()).toBe(204);
  await role(as, gid, '<i>Chef</i>');
  // Un rôle supprimé après avoir reçu des droits dans un salon : le sujet de la surcharge n'existe plus.
  const gone = await role(as, gid, 'Éphémère');
  expect((await as.put(`${API}/channels/${chan.id}/permissions/${gone.id}`, { data: { kind: 'role', allow: 1 << 1, deny: 1 << 6 } })).status()).toBe(200);
  expect((await as.delete(`${API}/guilds/${gid}/roles/${gone.id}`)).status()).toBe(204);
  expect((await as.delete(`${API}/channels/${chan.id}`)).status()).toBe(204);

  const { page } = owner;
  await open(page, gid);
  const me = owner.name;
  // Le plus récent d'abord : la suppression du salon.
  await expect(rows(page).first()).toContainText(`${me} a supprimé le salon #infos`);
  await expect(settings(page).getByRole('heading', { name: "Aujourd'hui", level: 3 })).toBeVisible();
  await expect(status(page)).toHaveText(/^\d+ événements$/);

  // Rôles, membres.
  // Les noms se lisent maintenant : le rôle renommé « Modérateurs » s'appelle ainsi dans toute la phrase.
  await expect(row(page, 'a créé le rôle Modérateurs')).toContainText(`${me} a créé le rôle Modérateurs`);
  await expect(row(page, 'a ajouté le rôle')).toContainText(`${me} a ajouté le rôle Modos à ${sam.name}`);
  await expect(row(page, 'a modifié le rôle')).toContainText(`${me} a modifié le rôle Modérateurs`);
  await expect(row(page, 'a exclu')).toContainText(`${me} a exclu ${sam.name} pendant 1 h`);
  await expect(row(page, 'a exclu')).toContainText('Raison : pause café');
  await expect(row(page, 'a expulsé')).toContainText(`${me} a expulsé ${moussa.name}`);
  await expect(row(page, 'a expulsé')).toContainText('Raison : hors sujet');
  await expect(row(page, 'a banni')).toContainText(`${me} a banni ${lea.name}`);
  await expect(row(page, 'a levé le bannissement')).toContainText(`${me} a levé le bannissement de ${lea.name}`);
  // Salons, emojis, invitations.
  await expect(row(page, 'a créé le salon')).toContainText(`${me} a créé le salon #annonces`);
  await expect(row(page, 'a modifié le salon')).toContainText(`${me} a modifié le salon #infos`);
  await expect(row(page, "a ajouté l'emoji")).toContainText(`${me} a ajouté l'emoji :pieuvre:`);
  await expect(row(page, "a renommé l'emoji")).toContainText(`${me} a renommé l'emoji :poulpe: en :pieuvre:`);
  await expect(row(page, 'a créé une invitation').first()).toContainText(`${me} a créé une invitation pour le salon #général`);
  await expect(row(page, 'a créé une invitation').first()).toContainText('Sans date limite, utilisations illimitées');
  // La cible d'une surcharge qui n'existe plus : « (supprimé) ».
  await expect(row(page, 'a modifié les droits')).toContainText(`${me} a modifié les droits du rôle (supprimé) dans le salon #infos`);
  await expect(row(page, 'a modifié les droits')).toContainText('Autorisé : Écrire des messages');

  // Des noms et des motifs sont du texte : ni gras, ni italique, ni script ne sortent de ce qu'on a saisi.
  await expect(row(page, 'a banni')).toContainText('Raison : spam <b>gras</b> & <script>x</script>');
  await expect(row(page, 'a banni').locator('small b')).toHaveCount(0);
  await expect(row(page, 'a créé le rôle <i>Chef</i>')).toContainText(`${me} a créé le rôle <i>Chef</i>`);
  await expect(rows(page).locator('i, script')).toHaveCount(0);

  // L'heure : relative, avec la date complète en infobulle et pour un lecteur d'écran.
  const when = rows(page).first().locator('time');
  await expect(when).toHaveAttribute('datetime', /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}/);
  await expect(when.locator('[aria-hidden="true"]')).toHaveText(/^(à l'instant|il y a \d+ min)$/);
  const title = await when.getAttribute('title');
  expect(title).toMatch(new RegExp(`${new Date().getFullYear()} à \\d{1,2}:\\d{2}`));
  await expect(when.locator('.k-sr')).toHaveText(title!);

  // Avant / après : un bouton seulement sur les lignes qui en ont, replié au départ, déplié dans la ligne.
  const renamed = row(page, 'a modifié le salon');
  const toggle = renamed.getByRole('button', { name: 'Voir les changements' });
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
  await expect(renamed.getByText('Avant').first()).toBeHidden();
  await toggle.click();
  const less = renamed.getByRole('button', { name: 'Masquer les changements' });
  await expect(less).toHaveAttribute('aria-expanded', 'true');
  await expect(renamed.getByText('annonces', { exact: true })).toBeVisible();
  await expect(renamed.getByText('infos', { exact: true })).toBeVisible();
  await expect(renamed.getByText('Mode lent')).toBeVisible();
  await expect(renamed.getByText('30 secondes')).toBeVisible();
  await expect(renamed.getByText('Désactivé')).toBeVisible();
  await expect(row(page, "a renommé l'emoji").getByRole('button', { name: 'Voir les changements' })).toBeVisible();
  await expect(row(page, 'a modifié le rôle').getByRole('button', { name: 'Voir les changements' })).toBeVisible();
  await expect(row(page, 'a banni').getByRole('button')).toHaveCount(0);
  await expect(row(page, 'a expulsé').getByRole('button')).toHaveCount(0);
  await a11y(page);
  await less.click();
  await expect(renamed.getByText('Avant').first()).toBeHidden();
  // La liste est en lecture seule : ni ⋯ ni action au survol.
  await expect(settings(page).getByRole('button', { name: /Plus d'actions|Actions pour/ })).toHaveCount(0);
});

test('audit-H3 filtres « Action » et « Membre », « Réinitialiser » seulement quand un filtre est actif, « Actualiser »', async ({ user, api }) => {
  const owner = await user('chef');
  const sam = await user('sam');
  const lea = await user('lea');
  const [gid, cid] = await createGuild(owner.page, 'Récif');
  await invite(api, owner, cid, sam);
  await invite(api, owner, cid, lea);
  const as = await api(owner.token);
  await give(as, gid, sam, await role(as, gid, 'Bâtisseurs', MANAGE_CHANNELS | MANAGE_EMOJIS));
  await role(as, gid, 'Rôle de chef');
  await channel(as, gid, 'cartes');
  await channel(await api(sam.token), gid, 'quai');
  expect((await as.put(`${API}/guilds/${gid}/bans/${lea.id}`, { data: {} })).status()).toBe(200);
  const { page } = owner;
  await open(page, gid);

  const action = settings(page).getByLabel('Action', { exact: true });
  const member = settings(page).getByLabel('Membre', { exact: true });
  const reset = settings(page).getByRole('button', { name: 'Réinitialiser' });
  await expect(settings(page).getByRole('search', { name: "Filtrer le journal d'audit" })).toBeVisible();
  expect(await action.locator('option').allTextContents()).toEqual(['Toutes', 'Salons', 'Rôles', 'Membres', 'Invitations', 'Serveur', 'Emojis']);
  const names = [owner.name, sam.name].sort((a, b) => a.toLowerCase().localeCompare(b.toLowerCase()));
  await expect.poll(() => member.locator('option').allTextContents()).toEqual(['Tous', ...names]);
  await expect(reset).toHaveCount(0);
  const all = await rows(page).count();
  expect(all).toBeGreaterThanOrEqual(7);

  // Par type : seulement des salons.
  await action.selectOption('salons');
  await expect(reset).toBeVisible();
  await expect(rows(page)).toHaveCount(2);
  await expect(rows(page).nth(0)).toContainText(`${sam.name} a créé le salon #quai`);
  await expect(rows(page).nth(1)).toContainText(`${owner.name} a créé le salon #cartes`);
  await expect(status(page)).toHaveText('2 événements');
  await action.selectOption('membres');
  await expect(rows(page)).toHaveCount(2); // le rôle donné à sam, le bannissement de lea
  await expect(row(page, 'a banni')).toHaveCount(1);
  // Par personne.
  await action.selectOption('');
  await member.selectOption({ label: sam.name });
  await expect(rows(page)).toHaveCount(1);
  await expect(rows(page).first()).toContainText(`${sam.name} a créé le salon #quai`);
  // Les deux : rien ne correspond, l'état vide propose de réinitialiser.
  await action.selectOption('emojis');
  await expect(settings(page).locator('.k-empty')).toContainText('Aucun événement ne correspond');
  await expect(status(page)).toHaveText('Aucun événement');
  await expect(settings(page).getByRole('button', { name: 'Réinitialiser' })).toHaveCount(2);
  await a11y(page);
  await settings(page).locator('.k-empty').getByRole('button', { name: 'Réinitialiser' }).click();
  await expect(rows(page)).toHaveCount(all);
  await expect(reset).toHaveCount(0);
  await expect(action).toHaveValue('');
  await expect(member).toHaveValue('');
  await expect(action).toBeFocused();

  // Actualiser : une action faite entre-temps apparaît, en tête.
  await channel(as, gid, 'nouveau');
  await expect(row(page, '#nouveau')).toHaveCount(0);
  await settings(page).getByRole('button', { name: 'Actualiser' }).click();
  await expect(rows(page).first()).toContainText(`${owner.name} a créé le salon #nouveau`);
  expect(await rows(page).count()).toBe(all + 1);
});

test('audit-H4 « Charger plus » : 50 lignes puis la suite, le focus va à la première nouvelle, 100 lignes et plus restent fluides', async ({ user, api }) => {
  test.setTimeout(120_000);
  const owner = await user('chef');
  const [gid] = await createGuild(owner.page, 'Chronique');
  const as = await api(owner.token);
  // 95 rôles puis 30 salons : 125 événements.
  for (let i = 0; i < 95; i += 5) await Promise.all([0, 1, 2, 3, 4].map((k) => role(as, gid, `rôle ${i + k}`)));
  for (let i = 0; i < 30; i += 5) await Promise.all([0, 1, 2, 3, 4].map((k) => channel(as, gid, `salon-${i + k}`)));
  const { page } = owner;
  await open(page, gid);

  await expect(rows(page)).toHaveCount(50);
  await expect(status(page)).toHaveText('50 événements affichés');
  const more = settings(page).getByRole('button', { name: 'Charger plus' });
  await expect(more).toBeVisible();
  const ids = await rows(page).evaluateAll((l) => l.map((x) => x.getAttribute('data-audit')));
  expect(new Set(ids).size).toBe(50);

  // Au clavier : Entrée charge les 50 suivantes, le focus va sur la première arrivée.
  await more.focus();
  const t0 = Date.now();
  await page.keyboard.press('Enter');
  await expect(rows(page)).toHaveCount(100);
  expect(Date.now() - t0, 'afficher 50 lignes de plus reste rapide').toBeLessThan(3000);
  await expect(status(page)).toContainText('50 événements de plus');
  const first = rows(page).nth(50);
  await expect(first).toBeFocused();
  expect(await first.getAttribute('data-audit')).not.toBeNull();
  // Un lecteur d'écran n'est pas perdu : la ligne focalisée est la première nouvelle, pas celle du haut de la liste.
  expect(ids).not.toContain(await first.getAttribute('data-audit'));

  // La dernière page est plus courte, le bouton disparaît, le décompte est le total.
  await settings(page).getByRole('button', { name: 'Charger plus' }).click();
  await expect(rows(page)).toHaveCount(125);
  await expect(settings(page).getByRole('button', { name: 'Charger plus' })).toHaveCount(0);
  await expect(status(page)).toContainText('125 événements');
  await expect(status(page)).toContainText('25 événements de plus');
  // Jamais deux fois la même ligne, du plus récent au plus ancien.
  const all = await rows(page).evaluateAll((l) => l.map((x) => x.getAttribute('data-audit')));
  expect(new Set(all).size).toBe(125);
  const api1 = await (await as.get(`${API}/guilds/${gid}/audit-log?limit=100`)).json();
  expect(all.slice(0, 100)).toEqual(api1.entries.map((e: { id: string }) => e.id));
  // Un seul titre de jour pour tout cela.
  await expect(settings(page).getByRole('main').getByRole('heading', { level: 3 })).toHaveCount(1);
  await a11y(page);
});

test('audit-H5 états : chargement, vide, erreur avec « Réessayer »', async ({ user, allow }) => {
  allow.push(/status of 403/);
  const owner = await user('chef');
  const [gid] = await createGuild(owner.page, 'Vide');
  const { page } = owner;

  // Chargement : une zone d'état annonce, cinq lignes d'attente, puis la réponse.
  let release: () => void = () => {};
  const gate = new Promise<void>((r) => (release = r));
  await page.route('**/api/v1/guilds/*/audit-log*', async (route) => {
    await gate;
    await route.continue();
  });
  await page.goto(`/guilds/${gid}/settings/audit`);
  await expect(status(page)).toHaveText('Chargement du journal…');
  await expect(settings(page).locator('.k-skeleton')).toHaveCount(5);
  release();
  // Vide : un message, pas de bouton « Charger plus », ni de « Réinitialiser ».
  await expect(settings(page).locator('.k-empty')).toContainText('Aucun événement');
  await expect(status(page)).toHaveText('Aucun événement');
  await expect(settings(page).getByRole('button', { name: 'Charger plus' })).toHaveCount(0);
  await expect(settings(page).getByRole('button', { name: 'Réinitialiser' })).toHaveCount(0);
  await expect(settings(page).locator('.k-skeleton')).toHaveCount(0);
  await a11y(page);

  // Erreur : le message du serveur dans une alerte, « Réessayer » relance.
  await page.unroute('**/api/v1/guilds/*/audit-log*');
  let fail = true;
  await page.route('**/api/v1/guilds/*/audit-log*', (route) =>
    fail
      ? route.fulfill({ status: 403, contentType: 'application/json', body: JSON.stringify({ code: 'forbidden', message: 'Tu n’as pas la permission de faire ça.' }) })
      : route.continue(),
  );
  await settings(page).getByRole('button', { name: 'Actualiser' }).click();
  const alert = settings(page).getByRole('alert');
  await expect(alert).toContainText('Tu n’as pas la permission de faire ça.');
  await expect(settings(page).getByRole('button', { name: 'Réessayer' })).toBeVisible();
  await a11y(page);
  fail = false;
  await settings(page).getByRole('button', { name: 'Réessayer' }).click();
  await expect(alert).toHaveCount(0);
  await expect(settings(page).locator('.k-empty')).toContainText('Aucun événement');
});

test('audit-H6 l’heure suit la préférence 12 / 24 h (infobulle et texte accessible)', async ({ user, api }) => {
  const owner = await user('chef');
  const [gid] = await createGuild(owner.page, 'Horloge');
  await role(await api(owner.token), gid, 'Un rôle');
  const { page } = owner;
  for (const [pref, pattern] of [['12', / à \d{1,2}:\d{2} (AM|PM)$/], ['24', / à \d{2}:\d{2}$/]] as const) {
    await page.evaluate((p) => localStorage.setItem('kraken.clock', p), pref);
    await open(page, gid);
    const time = rows(page).first().locator('time');
    await expect(time).toHaveAttribute('title', pattern);
    await expect(time.locator('.k-sr')).toHaveText(pattern);
  }
});

test('audit-H7 clavier seul : filtres, bouton « Voir les changements » et « Charger plus » s’atteignent et se manient sans souris', async ({ user, api }) => {
  const owner = await user('chef');
  const [gid] = await createGuild(owner.page, 'Clavier');
  const as = await api(owner.token);
  const chan = await channel(as, gid, 'avant');
  expect((await as.patch(`${API}/channels/${chan.id}`, { data: { name: 'apres' } })).status()).toBe(200);
  for (let i = 0; i < 52; i += 4) await Promise.all([0, 1, 2, 3].map((k) => role(as, gid, `r${i + k}`)));
  const { page } = owner;
  await open(page, gid);
  await expect(rows(page)).toHaveCount(50);

  const action = settings(page).getByLabel('Action', { exact: true });
  await action.focus();
  await page.keyboard.press('Tab');
  await expect(settings(page).getByLabel('Membre', { exact: true })).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(settings(page).getByRole('button', { name: 'Actualiser' })).toBeFocused();
  // Une seule fois par ligne : les lignes elles-mêmes ne sont pas des arrêts de tabulation.
  await page.keyboard.press('Tab');
  const stop = await page.evaluate(() => document.activeElement?.tagName);
  expect(stop).toBe('BUTTON');
  // Le filtre se choisit au clavier.
  await action.focus();
  await page.keyboard.press('ArrowDown');
  await expect(action).toHaveValue('salons');
  await expect(rows(page)).toHaveCount(2);
  await expect(settings(page).getByRole('button', { name: 'Réinitialiser' })).toBeVisible();
  // L'avant / après s'ouvre à Entrée et se referme à Espace.
  const toggle = row(page, 'a modifié le salon').getByRole('button', { name: 'Voir les changements' });
  await toggle.focus();
  await page.keyboard.press('Enter');
  await expect(row(page, 'a modifié le salon').getByRole('button', { name: 'Masquer les changements' })).toHaveAttribute('aria-expanded', 'true');
  await expect(row(page, 'a modifié le salon').getByText('apres', { exact: true })).toBeVisible();
  await page.keyboard.press('Space');
  await expect(row(page, 'a modifié le salon').getByRole('button', { name: 'Voir les changements' })).toHaveAttribute('aria-expanded', 'false');
  // Aucun piège : Tab continue jusqu'à la fin de la fenêtre.
  await settings(page).getByRole('button', { name: 'Réinitialiser' }).click();
  await expect(rows(page)).toHaveCount(50);
  await settings(page).getByRole('button', { name: 'Charger plus' }).focus();
  await page.keyboard.press('Space');
  await expect(rows(page)).toHaveCount(54);
  await expect(rows(page).nth(50)).toBeFocused();
});

async function bigText({ user, api }: { user: (p?: string) => Promise<User>; api: Api }) {
  const owner = await user('chef');
  const as = await api(owner.token);
  const [gid] = await makeGuild(as, 'Grand');
  const chan = await channel(as, gid, 'avant');
  expect((await as.patch(`${API}/channels/${chan.id}`, { data: { name: 'apres', topic: 'Un sujet assez long pour passer à la ligne' } })).status()).toBe(200);
  await role(as, gid, 'Un rôle');
  const { page } = owner;
  await page.evaluate(() => {
    localStorage.setItem('kraken.zoom', '150');
    localStorage.setItem('kraken.text', '20');
  });
  await open(page, gid);
  await expect(page.locator('html')).toHaveAttribute('data-text', '20');
  await expect(rows(page).first()).toBeVisible();
  expect(await noOverflow(page)).toBe(true);
  for (const r of await rows(page).all()) {
    const ok = await r.locator('p').first().evaluate((el) => el.scrollWidth <= el.clientWidth + 1 && getComputedStyle(el).textOverflow !== 'ellipsis');
    expect(ok).toBe(true);
  }
  const toggle = row(page, 'a modifié le salon').getByRole('button', { name: 'Voir les changements' });
  await toggle.scrollIntoViewIfNeeded();
  await toggle.click();
  await expect(row(page, 'a modifié le salon').getByText('Sujet', { exact: true })).toBeVisible();
  expect(await noOverflow(page)).toBe(true);
  await a11y(page);
}

test.describe('largeur mobile', () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test('audit-N1 à 390 px : filtres pleine largeur et empilés, lignes sans débordement, cibles de 36 px, phrase jamais tronquée', async ({ user, api }) => {
    const owner = await user('chef');
    const sam = await user('sam');
    const as = await api(owner.token);
    const [gid, cid] = await makeGuild(as, 'Poche');
    await invite(api, owner, cid, sam);
    const long = 'Un rôle au nom très long pour vérifier que la phrase passe à la ligne sans jamais être coupée';
    expect(long.length).toBeLessThanOrEqual(100);
    await role(as, gid, long);
    const chan = await channel(as, gid, 'avant');
    expect((await as.patch(`${API}/channels/${chan.id}`, { data: { name: 'apres' } })).status()).toBe(200);
    expect((await as.put(`${API}/guilds/${gid}/bans/${sam.id}`, { data: { reason: 'x'.repeat(120) } })).status()).toBe(200);
    const { page } = owner;
    await open(page, gid);
    await expect(rows(page).first()).toBeVisible();

    const box = async (l: Locator) => (await l.boundingBox())!;
    const action = await box(settings(page).getByLabel('Action', { exact: true }));
    const member = await box(settings(page).getByLabel('Membre', { exact: true }));
    expect(member.y, 'empilés').toBeGreaterThan(action.y + action.height - 1);
    expect(Math.abs(member.width - action.width), 'même largeur').toBeLessThan(2);
    const container = await box(settings(page).locator('.app-audit'));
    expect(action.width, 'pleine largeur').toBeGreaterThan(container.width - 2);
    for (const name of ['Actualiser']) expect((await box(settings(page).getByRole('button', { name }))).height).toBeGreaterThanOrEqual(36);
    expect((await box(row(page, 'a modifié le salon').getByRole('button', { name: 'Voir les changements' }))).height).toBeGreaterThanOrEqual(36);
    expect(await noOverflow(page)).toBe(true);
    // La phrase passe à la ligne : rien de coupé, rien qui déborde de sa ligne.
    const sentence = row(page, long).locator('p').first();
    await expect(sentence).toContainText(`${owner.name} a créé le rôle ${long}`);
    const fits = await sentence.evaluate((el) => {
      const s = getComputedStyle(el);
      return { ellipsis: s.textOverflow, clip: el.scrollWidth <= el.clientWidth + 1, nowrap: s.whiteSpace };
    });
    expect(fits).toEqual({ ellipsis: 'clip', clip: true, nowrap: 'normal' });
    const line = await box(row(page, long));
    expect(line.x + line.width).toBeLessThanOrEqual(390);
    await row(page, 'a banni').scrollIntoViewIfNeeded();
    await expect(row(page, 'a banni')).toContainText(`Raison : ${'x'.repeat(120)}`);
    await a11y(page);
  });

  test('audit-N2 à 390 px, texte 20 px : rien ne se coupe, la page ne déborde pas (le zoom est gelé sous 640 px)', async ({ user, api }) => {
    await bigText({ user, api });
  });
});

test.describe('fenêtre de 1280 × 720', () => {
  test.use({ viewport: { width: 1280, height: 720 } });

  test('audit-N3 à 1280 × 720, zoom 150 % (plafonné) et texte 20 px : rien ne se coupe, la page ne déborde pas', async ({ user, api }) => {
    await bigText({ user, api });
  });
});

test.describe('contraste élevé', () => {
  test('audit-C1 la phrase, le motif, l’heure et l’avant / après passent 7:1 dans les trois thèmes', async ({ user, api }) => {
    const owner = await user('chef');
    const sam = await user('sam');
    const [gid, cid] = await createGuild(owner.page, 'Contraste');
    await invite(api, owner, cid, sam);
    const as = await api(owner.token);
    const chan = await channel(as, gid, 'avant');
    expect((await as.patch(`${API}/channels/${chan.id}`, { data: { name: 'apres' } })).status()).toBe(200);
    expect((await as.put(`${API}/guilds/${gid}/bans/${sam.id}`, { data: { reason: 'pas gentil' } })).status()).toBe(200);
    expect((await as.delete(`${API}/channels/${chan.id}`)).status()).toBe(204);
    const { page } = owner;
    for (const theme of ['dark', 'light', 'black']) {
      await page.evaluate((t) => {
        localStorage.setItem('kraken.contrast', 'high');
        localStorage.setItem('kraken.theme', t);
      }, theme);
      await open(page, gid);
      await expect(page.locator('html')).toHaveAttribute('data-contrast', 'high');
      await row(page, 'a modifié le salon').getByRole('button', { name: 'Voir les changements' }).click();
      const probes: [string, Locator][] = [
        ['phrase', row(page, 'a banni').locator('p').first()],
        ['nom', row(page, 'a banni').locator('b').first()],
        ['motif', row(page, 'a banni').getByText('Raison : pas gentil')],
        ['heure', row(page, 'a banni').locator('time [aria-hidden="true"]')],
        ['jour', settings(page).getByRole('main').getByRole('heading', { level: 3 }).first()],
        ['avant', row(page, 'a modifié le salon').getByText('Avant', { exact: true })],
        ['valeur', row(page, 'a modifié le salon').getByText('apres', { exact: true })],
        ['décompte', status(page)],
      ];
      for (const [what, loc] of probes) {
        const c = await colors(loc);
        expect(ratio(c.fg, c.bg), `${theme} ${what} : ${c.fg} sur ${c.bg}`).toBeGreaterThanOrEqual(7);
      }
      // L'icône de suppression est doublée par la phrase ; sa couleur reste lisible (3:1) sur sa pastille.
      const icon = row(page, 'a supprimé le salon').locator('.app-audit-icon');
      const c = await colors(icon);
      expect(ratio(c.fg, c.bg), `${theme} icône`).toBeGreaterThanOrEqual(3);
      await a11y(page);
    }
  });
});
