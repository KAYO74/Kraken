// Langues de l'interface, lot « application » (PR #31) : l'accueil Amis (liste, demandes, ajout, blocage), le choix d'amis
// pour un MP ou un groupe, les Réglages restants (compte, mon profil, qui peut t'écrire, notifications, sections à venir),
// la carte de profil, le recadrage de la photo et de la bannière, les pages légales, les écrans pas encore construits et la
// mise à jour de l'appli de bureau, en anglais. Fichiers migrés : home.rs, social.rs, settings.rs, profile.rs, avatar.rs,
// legal.rs, update.rs, app.rs. Chaque test garde un œil sur le français : aucune phrase française du catalogue ne doit
// rester à l'écran, texte ou nom accessible.
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { test as base, expect, a11y, watch, API, PASSWORD, unique } from '../fixtures';
import { animatedGif, png } from '../images';
import type { APIRequestContext, BrowserContext, Page } from '@playwright/test';

type Account = { page: Page; context: BrowserContext; id: string; name: string; token: string };
type Api = (t?: string) => Promise<APIRequestContext>;
type Who = { id: string; name: string; token: string };

/**
 * `signedIn(locale?, viewport?)` : un compte neuf, connecté dans un navigateur de cette langue, sans attendre un
 * titre de page (la fixture `user` attend « Amis », écrit en français).
 */
const test = base.extend<{ signedIn: (locale?: string, viewport?: { width: number; height: number }) => Promise<Account> }>({
  signedIn: async ({ browser, baseURL, issues, locale: browserLocale }, use) => {
    const contexts: BrowserContext[] = [];
    await use(async (locale = browserLocale ?? 'fr-FR', viewport = { width: 1440, height: 900 }) => {
      const context = await browser.newContext({ baseURL, locale, viewport });
      contexts.push(context);
      const name = unique('i18na');
      const res = await context.request.post(`${API}/auth/register`, { data: { username: name, email: `${name}@example.org`, password: PASSWORD } });
      expect(res.status(), await res.text()).toBeLessThan(300);
      const { token, user } = await res.json();
      const page = await context.newPage();
      watch(page, issues);
      await page.goto('/login');
      await page.evaluate(() => localStorage.setItem('kraken.signed_in', '1'));
      await page.goto('/channels/@me');
      await expect(page.locator('#main')).toBeVisible();
      return { page, context, id: user.id, name, token };
    });
    for (const c of contexts) await c.close();
  },
});

test.use({ locale: 'en-US' });

// ───────── outils ─────────

/** Tous les catalogues anglais : (français, anglais) quand les deux diffèrent. */
const CATALOG_DIR = join(__dirname, '../../crates/kraken-client/i18n/en');
const entries = readdirSync(CATALOG_DIR)
  .filter((f) => f.endsWith('.tsv'))
  .flatMap((f) =>
    readFileSync(join(CATALOG_DIR, f), 'utf8')
      .split('\n')
      .filter((l) => l && !l.startsWith('#'))
      .map((l) => l.split('\t') as [string, string]),
  )
  .filter(([fr, en]) => fr && en && fr !== en && !fr.includes('\\'));
/** Les phrases françaises du catalogue : ce qui ne doit plus se lire en anglais. */
const FRENCH = [...new Set(entries.filter(([fr]) => !fr.includes('{') && fr.length >= 8).map(([fr]) => fr))];
/** Les morceaux fixes des phrases à variables (`Retirer {nom} de tes amis ?` : « de tes amis ? » est trop court, mais pas « Cette personne ne verra plus »). */
const FRAGMENTS = [
  ...new Set(
    entries
      .filter(([fr]) => fr.includes('{'))
      .flatMap(([fr]) => fr.split(/\{[^}]+\}/))
      .map((s) => s.trim())
      .filter((s) => s.length >= 14),
  ),
];
/** L'inverse, pour la page en français : les phrases anglaises du seul groupe « app », qui ne doivent pas s'y trouver. */
const ENGLISH_APP = readFileSync(join(CATALOG_DIR, 'app.tsv'), 'utf8')
  .split('\n')
  .filter((l) => l && !l.startsWith('#'))
  .map((l) => l.split('\t') as [string, string])
  .filter(([fr, en]) => fr !== en && !en.includes('{') && en.length >= 14)
  .map(([, en]) => en);

/** Des mots français courts, trop courts pour la liste ci-dessus : cherchés comme mots entiers. */
const SHORT = [
  'Amis', 'Annuler', 'Enregistrer', 'Supprimer', 'Fermer', 'Retirer', 'Bloquer', 'Débloquer', 'Bloqués', 'Appeler', 'Appliquer',
  'Accepter', 'Refuser', 'Quitter', 'Rôles', 'Pronoms', 'Chercher', 'Aperçu', 'Déconnexion', 'Confidentialité', 'Réglages',
  'Ajouter', 'Changer', 'Nouveautés', 'Règles', 'Modération',
];

/** Tout ce qu'on lit ou entend sur la page : le texte visible et les noms accessibles (aria-label, title, placeholder, alt). */
const everything = (p: Page) =>
  p.evaluate(() => {
    const names = [...document.querySelectorAll('[aria-label],[title],[placeholder],[alt]')].flatMap((e) =>
      ['aria-label', 'title', 'placeholder', 'alt'].map((a) => e.getAttribute(a) ?? ''),
    );
    return document.body.innerText + '\n' + names.join('\n');
  });

/** Aucune phrase française du catalogue n'est à l'écran. */
async function noFrench(p: Page) {
  expect(FRENCH.length, 'les catalogues sont lus').toBeGreaterThan(500);
  expect(FRAGMENTS.length, 'les morceaux de phrases sont lus').toBeGreaterThan(100);
  const seen = await everything(p);
  const words = SHORT.filter((w) => new RegExp(`(^|[^\\p{L}])${w}($|[^\\p{L}])`, 'u').test(seen));
  expect({ phrases: FRENCH.filter((k) => seen.includes(k)), fragments: FRAGMENTS.filter((k) => seen.includes(k)), words }).toEqual({
    phrases: [],
    fragments: [],
    words: [],
  });
}

const noScroll = async (p: Page) =>
  expect(await p.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(p.viewportSize()!.width);
/** Une zone `status` avec ce texte (il y en a beaucoup : on filtre toujours). */
const said = (p: Page | ReturnType<Page['locator']>, text: string | RegExp) => p.getByRole('status').filter({ hasText: text });
const settings = (p: Page) => p.getByRole('dialog', { name: 'Settings' });
const sections = (p: Page) => settings(p).getByRole('navigation', { name: 'Settings sections' });
const tab = (p: Page, name: string | RegExp) => p.getByRole('tablist', { name: 'Filter friends' }).getByRole('tab', { name });
const open = async (p: Page, section: string, heading: string) => {
  await p.goto(`/settings/${section}`);
  await expect(settings(p).getByRole('heading', { name: heading, level: 2 })).toBeVisible();
};

async function register(api: Api, prefix: string): Promise<Who> {
  const name = unique(prefix);
  const res = await (await api()).post(`${API}/auth/register`, { data: { username: name, email: `${name}@example.org`, password: PASSWORD } });
  expect(res.status(), await res.text()).toBeLessThan(300);
  const { token, user } = await res.json();
  return { id: user.id as string, name, token: token as string };
}

/** Une demande d'ami de `from` à `to`, par l'API. */
async function ask(api: Api, from: Who, to: Who) {
  const res = await (await api(from.token)).post(`${API}/users/@me/relationships`, { data: { username: to.name } });
  expect(res.status(), await res.text()).toBe(201);
}

/** `a` et `b` deviennent amis, par l'API. */
async function befriend(api: Api, a: Who, b: Who) {
  await ask(api, a, b);
  const res = await (await api(b.token)).put(`${API}/users/@me/relationships/${a.id}`, { data: { kind: 'friend' } });
  expect(res.status(), await res.text()).toBe(200);
}

/** Un serveur (par l'API) : son id et celui de son premier salon textuel. */
async function guildOf(as: APIRequestContext, name: string) {
  const res = await as.post(`${API}/guilds`, { data: { name } });
  expect(res.status(), await res.text()).toBeLessThan(300);
  const g = await res.json();
  const text = g.channels.find((c: { kind: string }) => c.kind === 'text');
  return { gid: g.guild.id as string, cid: text.id as string };
}

async function joinWith(api: Api, owner: Who, cid: string, guest: Who) {
  const inv = await (await api(owner.token)).post(`${API}/channels/${cid}/invites`, { data: {} });
  expect(inv.status(), await inv.text()).toBeLessThan(300);
  const { code } = await inv.json();
  expect((await (await api(guest.token)).post(`${API}/invites/${code}`)).status()).toBe(200);
}

// ───────── Amis ─────────

test('I18N-A1 Amis en anglais : titre, onglets au clavier, états vides, colonne « Where your friends are », ligne d’un ami et son menu ; rien de français', async ({ signedIn, api }) => {
  const a = await signedIn('en-US');
  const p = a.page;

  await expect(p.getByRole('heading', { name: 'Friends', level: 1 })).toBeVisible();
  await expect(p.getByRole('tablist', { name: 'Filter friends' }).getByRole('tab')).toHaveText(['Online', 'All', 'Pending', 'Blocked', 'Add a friend']);
  await expect(tab(p, 'Online')).toHaveAttribute('aria-selected', 'true');

  // Chaque onglet a son état vide, en deux phrases.
  const panel = (name: string) => p.getByRole('tabpanel', { name });
  await expect(panel('Online').getByRole('heading', { name: 'Online 0', level: 2 })).toBeVisible();
  await expect(panel('Online').getByText('Nobody online right now')).toBeVisible();
  await expect(panel('Online').getByText('Your friends will show up here as soon as they sign in.')).toBeVisible();
  const search = panel('Online').getByRole('searchbox', { name: 'Search your friends' });
  await expect(search).toHaveAttribute('placeholder', 'Search');
  await noFrench(p);

  // Au clavier : les flèches déplacent le choix et le focus.
  await tab(p, 'Online').focus();
  await p.keyboard.press('ArrowRight');
  await expect(tab(p, 'All')).toHaveAttribute('aria-selected', 'true');
  await expect(tab(p, 'All')).toBeFocused();
  await expect(panel('All').getByRole('heading', { name: 'All 0', level: 2 })).toBeVisible();
  await expect(panel('All').getByText('No friends yet')).toBeVisible();
  await expect(panel('All').getByText('Add someone with their username to get started.')).toBeVisible();
  await p.keyboard.press('ArrowRight');
  await expect(panel('Pending').getByRole('heading', { name: 'Pending 0', level: 2 })).toBeVisible();
  await expect(panel('Pending').getByText('No pending requests')).toBeVisible();
  await expect(panel('Pending').getByText("Requests you've received and sent appear here.")).toBeVisible();
  await p.keyboard.press('ArrowRight');
  await expect(panel('Blocked').getByRole('heading', { name: 'Blocked 0', level: 2 })).toBeVisible();
  await expect(panel('Blocked').getByText("You haven't blocked anyone")).toBeVisible();
  await expect(panel('Blocked').getByText('A blocked account can no longer message you privately or send you a friend request.')).toBeVisible();
  await p.keyboard.press('End');
  await expect(tab(p, 'Add a friend')).toHaveAttribute('aria-selected', 'true');
  await p.keyboard.press('Home');
  await expect(tab(p, 'Online')).toHaveAttribute('aria-selected', 'true');
  await noFrench(p);

  // La colonne d'activité.
  const side = p.getByRole('complementary', { name: 'Activity' });
  await expect(side.getByRole('heading', { name: 'Where your friends are', level: 2 })).toBeVisible();
  await expect(side.getByText('No voice channels in use')).toBeVisible();
  await expect(side.getByText('Voice channels where your friends are talking will show up here, so you can join them in one click.')).toBeVisible();

  // Un ami (hors ligne) : sa ligne, ses actions et leur nom, le menu « ⋯ ».
  const b = await register(api, 'bea');
  await befriend(api, a, b);
  await tab(p, 'All').click();
  await expect(panel('All').getByRole('heading', { name: 'All 1', level: 2 })).toBeVisible();
  const row = p.getByRole('listitem').filter({ hasText: `@${b.name}` });
  await expect(row).toContainText(`@${b.name} · offline`);
  await expect(row.getByRole('button', { name: `Message ${b.name}` })).toHaveAttribute('title', 'Message');
  await expect(row.getByRole('button', { name: `Call ${b.name}` })).toHaveAttribute('title', 'Call');
  await row.getByRole('button', { name: `More actions for ${b.name}` }).click();
  await expect(p.getByRole('button', { name: 'Remove from friends' })).toBeVisible();
  await expect(p.getByRole('button', { name: `Block ${b.name}` })).toHaveText('Block');
  await noFrench(p);
  await a11y(p);
  await p.keyboard.press('Escape');
  await expect(p.getByRole('button', { name: 'Remove from friends' })).toHaveCount(0);
});

test('I18N-A2 demandes reçues et envoyées, accepter, annuler ; ajouter un ami : réponses et erreurs du serveur en anglais ; rien de français', async ({ signedIn, api, allow }) => {
  allow.push(/status of 40[0-9]/);
  const a = await signedIn('en-US');
  const p = a.page;
  const [c, d, e, f] = await Promise.all(['cam', 'dee', 'eli', 'fay'].map((n) => register(api, n)));
  await ask(api, c, a);
  await ask(api, a, d);

  // Deux demandes en attente, dont une à accepter : le badge et son texte caché le disent.
  await expect(tab(p, 'Pending 1 to accept or decline')).toBeVisible();
  await tab(p, /^Pending/).click();
  const pending = p.getByRole('tabpanel', { name: /^Pending/ });
  await expect(pending.getByRole('heading', { name: 'Pending 2', level: 2 })).toBeVisible();
  const incoming = pending.getByRole('listitem').filter({ hasText: c.name });
  const outgoing = pending.getByRole('listitem').filter({ hasText: d.name });
  await expect(incoming).toContainText(`@${c.name} · Request received`);
  await expect(outgoing).toContainText(`@${d.name} · Request sent`);
  await expect(incoming.getByRole('button', { name: `Accept ${c.name}'s request` })).toHaveAttribute('title', 'Accept');
  await expect(incoming.getByRole('button', { name: `Decline ${c.name}'s request` })).toHaveAttribute('title', 'Decline');
  await expect(incoming.getByRole('button', { name: `Block ${c.name}` })).toHaveAttribute('title', 'Block');
  await expect(outgoing.getByRole('button', { name: `Cancel request to ${d.name}` })).toHaveAttribute('title', 'Cancel request');
  await noFrench(p);
  await a11y(p);

  // Accepter l'une, annuler l'autre.
  await incoming.getByRole('button', { name: `Accept ${c.name}'s request` }).click();
  await expect(pending.getByRole('heading', { name: 'Pending 1', level: 2 })).toBeVisible();
  await expect(tab(p, 'Pending')).toBeVisible();
  await outgoing.getByRole('button', { name: `Cancel request to ${d.name}` }).click();
  await expect(pending.getByText('No pending requests')).toBeVisible();
  await tab(p, 'All').click();
  await expect(p.getByRole('listitem').filter({ hasText: `@${c.name} · offline` })).toBeVisible();

  // Ajouter un ami : la consigne, le champ, le bouton.
  await tab(p, 'Add a friend').click();
  const add = p.getByRole('tabpanel', { name: 'Add a friend' });
  await expect(add.getByRole('heading', { name: 'Add a friend', level: 2 })).toBeVisible();
  await expect(add.getByText('Type their KRAKEN username, the one that starts with @ under their name.')).toBeVisible();
  const field = add.getByLabel("Your friend's username");
  await expect(field).toHaveAttribute('placeholder', 'username');
  await expect(field).toHaveAccessibleDescription('Type their KRAKEN username, the one that starts with @ under their name.');
  const send = add.getByRole('button', { name: 'Send request' });
  await expect(send).toBeDisabled();
  await noFrench(p);
  await a11y(p);

  // Les réponses : le sien (erreur du serveur traduite), une demande envoyée puis redoublée, déjà amis, et une demande déjà reçue.
  await field.fill(a.name);
  await send.click();
  await expect(add.getByRole('alert')).toHaveText("That's your own username.");
  await expect(field).toHaveAttribute('aria-invalid', 'true');
  await field.fill(`@${e.name.toUpperCase()}`);
  await expect(add.getByRole('alert')).toHaveCount(0);
  await send.click();
  await expect(said(add, `Request sent to ${e.name} (@${e.name}). It appears under "Pending".`)).toBeVisible();
  await expect(field).toHaveValue('');
  await field.fill(e.name);
  await send.click();
  await expect(add.getByRole('alert')).toHaveText('Your request was already sent and is waiting for a reply under "Pending".');
  await field.fill(c.name);
  await send.click();
  await expect(add.getByRole('alert')).toHaveText("You're already friends.");
  await noFrench(p);
  await ask(api, f, a);
  await field.fill(f.name);
  await send.click();
  await expect(said(add, `${f.name} had already sent you a request: you're now friends.`)).toBeVisible();
  await noFrench(p);
  await a11y(p);
});

test('I18N-A3 retirer un ami, bloquer, débloquer : confirmations, onglet Blocked et Réglages › Who can message you ; rien de français', async ({ signedIn, api }) => {
  const a = await signedIn('en-US');
  const p = a.page;
  const [b, c] = await Promise.all([register(api, 'bea'), register(api, 'cyd')]);
  await befriend(api, a, b);
  await befriend(api, a, c);
  await tab(p, 'All').click();
  await expect(p.getByRole('heading', { name: 'All 2', level: 2 })).toBeVisible();
  const more = (who: Who) => p.getByRole('button', { name: `More actions for ${who.name}` });

  // Retirer : Échap annule d'abord, Annuler est le bouton ciblé par défaut.
  await more(b).click();
  await p.getByRole('button', { name: 'Remove from friends' }).click();
  const remove = p.getByRole('alertdialog', { name: `Remove ${b.name} from your friends?` });
  await expect(remove).toBeVisible();
  await expect(remove.getByText('Your direct messages stay. To become friends again, one of you will have to send a new request.')).toBeVisible();
  await expect(remove.getByRole('button', { name: 'Cancel' })).toBeFocused();
  await noFrench(p);
  await a11y(p);
  await p.keyboard.press('Escape');
  await expect(remove).toHaveCount(0);
  await more(b).click();
  await p.getByRole('button', { name: 'Remove from friends' }).click();
  await remove.getByRole('button', { name: 'Remove', exact: true }).click();
  await expect(p.getByRole('heading', { name: 'All 1', level: 2 })).toBeVisible();

  // Bloquer : la confirmation dit ce qui change.
  await more(c).click();
  await p.getByRole('button', { name: `Block ${c.name}` }).click();
  const block = p.getByRole('alertdialog', { name: `Block ${c.name}?` });
  await expect(block).toBeVisible();
  await expect(
    block.getByText(
      "You'll no longer be friends. This account won't be able to message you privately or send you a friend request, and its messages will be collapsed in the channels you share. It isn't notified.",
    ),
  ).toBeVisible();
  await expect(block.getByRole('button', { name: 'Cancel' })).toBeVisible();
  await noFrench(p);
  await a11y(p);
  await block.getByRole('button', { name: 'Block', exact: true }).click();
  await expect(block).toHaveCount(0);
  await tab(p, 'Blocked').click();
  await expect(p.getByRole('heading', { name: 'Blocked 1', level: 2 })).toBeVisible();
  const blocked = p.getByRole('listitem').filter({ hasText: c.name });
  await expect(blocked).toContainText(`@${c.name} · Blocked`);
  await expect(blocked.getByRole('button', { name: `Unblock ${c.name}` })).toHaveText('Unblock');
  await noFrench(p);

  // Réglages › Who can message you : la liste des comptes bloqués, le déblocage.
  await open(p, 'messaging', 'Who can message you');
  await expect(settings(p).getByRole('heading', { name: 'Blocked accounts', level: 3 })).toBeVisible();
  await expect(
    settings(p).getByText(
      "A blocked account can no longer message you privately or send you a friend request; its messages are collapsed in the channels you share. It isn't notified of anything.",
    ),
  ).toBeVisible();
  await expect(settings(p).getByRole('button', { name: `Unblock ${c.name}` })).toHaveText('Unblock');
  await noFrench(p);
  await a11y(p);
  await settings(p).getByRole('button', { name: `Unblock ${c.name}` }).click();
  await expect(settings(p).getByText('Nobody is blocked')).toBeVisible();
  await expect(settings(p).getByText('Block someone from your friends list or from their requests.')).toBeVisible();
  await noFrench(p);
});

// ───────── nouveau MP, groupes ─────────

test('I18N-A4 nouvelle conversation et groupes : choix d’amis, compteur, nom, ajout, renommer, retirer, quitter, « Empty group » ; rien de français', async ({ signedIn, api }) => {
  test.setTimeout(90_000);
  const a = await signedIn('en-US');
  const p = a.page;
  const [b, c, d] = await Promise.all([register(api, 'bea'), register(api, 'cyd'), register(api, 'dax')]);
  const newChat = p.getByRole('button', { name: 'New conversation or group' });

  // Sans ami : la consigne et le renvoi vers l'onglet d'ajout.
  await newChat.click();
  const picker = p.getByRole('dialog', { name: 'New conversation' });
  await expect(picker.getByText('One friend for a direct message, several for a group (up to 9).')).toBeVisible();
  await expect(picker.getByText('Add some friends first, from the "Add a friend" tab.')).toBeVisible();
  await expect(said(picker, '0 of 9 selected')).toBeVisible();
  await expect(picker.getByRole('button', { name: 'Open conversation' })).toBeDisabled();
  await noFrench(p);
  await a11y(p);
  await picker.getByRole('button', { name: 'Cancel' }).click();
  await expect(picker).toHaveCount(0);

  // Avec trois amis : recherche, cases, compteur, bouton qui change, nom facultatif.
  await befriend(api, a, b);
  await befriend(api, a, c);
  await befriend(api, a, d);
  await newChat.click();
  await expect(picker.getByRole('group', { name: 'Your friends' })).toBeVisible();
  await expect(picker.getByRole('checkbox')).toHaveCount(3);
  const find = picker.getByRole('searchbox', { name: 'Search your friends' });
  await expect(find).toHaveAttribute('placeholder', 'Search for a friend');
  await find.fill('zzzz');
  await expect(picker.getByText("No friend matches, or they're all in this group already.")).toBeVisible();
  await find.fill('');
  await picker.getByRole('checkbox', { name: new RegExp(b.name) }).check();
  await expect(said(picker, '1 of 9 selected')).toBeVisible();
  await expect(picker.getByRole('button', { name: 'Open conversation' })).toBeEnabled();
  await expect(picker.getByLabel('Group name (optional)')).toHaveCount(0);
  await picker.getByRole('checkbox', { name: new RegExp(c.name) }).check();
  await expect(said(picker, '2 of 9 selected')).toBeVisible();
  const groupName = picker.getByLabel('Group name (optional)');
  await expect(groupName).toHaveAttribute('placeholder', "Members' first names, if left empty");
  await groupName.fill('Crew');
  await noFrench(p);
  await a11y(p);
  await picker.getByRole('button', { name: 'Create group' }).click();
  await p.waitForURL(/\/channels\/@me\/[0-9a-f-]{36}$/);
  await expect(p.getByRole('heading', { name: 'Crew', level: 1 })).toBeVisible();
  const members = p.getByRole('complementary', { name: 'Group members' });
  await expect(members.getByRole('listitem')).toHaveCount(3);

  // Ajouter au groupe : la place restante.
  await p.getByRole('button', { name: 'Add friends to the group' }).click();
  const adding = p.getByRole('dialog', { name: 'Add friends to the group' });
  await expect(adding.getByText('A group can have up to 10 people: 7 spot(s) left.')).toBeVisible();
  await expect(adding.getByRole('checkbox')).toHaveCount(1);
  await adding.getByRole('checkbox', { name: new RegExp(d.name) }).check();
  await expect(said(adding, '1 of 7 selected')).toBeVisible();
  await noFrench(p);
  await adding.getByRole('button', { name: 'Add to group' }).click();
  await expect(adding).toHaveCount(0);
  await expect(members.getByRole('listitem')).toHaveCount(4);

  // Renommer.
  await p.getByRole('button', { name: 'Rename group' }).click();
  const rename = p.getByRole('dialog', { name: 'Rename group' });
  await expect(rename.getByLabel('Group name')).toHaveValue('Crew');
  await expect(rename.getByText("Leave empty to show the members' first names. The whole group sees the change.")).toBeVisible();
  await expect(rename.getByRole('button', { name: 'Cancel' })).toBeVisible();
  await noFrench(p);
  await a11y(p);
  await rename.getByLabel('Group name').fill('Crew 2');
  await rename.getByRole('button', { name: 'Save' }).click();
  await expect(p.getByRole('heading', { name: 'Crew 2', level: 1 })).toBeVisible();

  // Retirer un membre, quitter : les deux confirmations nomment le groupe.
  await members.getByRole('button', { name: `More actions for ${d.name}` }).click();
  await members.getByRole('button', { name: 'Remove from group' }).click();
  const removing = p.getByRole('alertdialog', { name: `Remove ${d.name} from the group?` });
  await expect(removing.getByText('This person will no longer see "Crew 2" or its messages. You can add them again later.')).toBeVisible();
  await expect(removing.getByRole('button', { name: 'Remove from group' })).toBeVisible();
  await noFrench(p);
  await a11y(p);
  await p.keyboard.press('Escape');
  await expect(removing).toHaveCount(0);
  await p.getByRole('button', { name: 'Leave group' }).click();
  const leaving = p.getByRole('alertdialog', { name: 'Leave "Crew 2"?' });
  await expect(leaving.getByText("You'll no longer see this group or its messages. To come back, a member will have to add you again.")).toBeVisible();
  await expect(leaving.getByRole('button', { name: 'Leave group' })).toBeVisible();
  await noFrench(p);
  await p.keyboard.press('Escape');

  // Un groupe dont tous les autres sont partis n'a plus de nom à montrer.
  const asA = await api(a.token);
  const lonely = await (await asA.post(`${API}/users/@me/channels`, { data: { recipients: [b.id, c.id] } })).json();
  for (const who of [b, c]) expect((await (await api(who.token)).delete(`${API}/channels/${lonely.id}/recipients/@me`)).status()).toBeLessThan(300);
  await p.goto(`/channels/@me/${lonely.id}`);
  await expect(p.getByRole('heading', { name: 'Empty group', level: 1 })).toBeVisible();
  await expect(p.getByRole('navigation', { name: 'Direct messages' }).getByRole('link', { name: /Empty group/ })).toBeVisible();
  await noFrench(p);
});

// ───────── Réglages ─────────

test('I18N-A5 Réglages › My account : pseudo, e-mail, mot de passe, déconnexions, suppression du compte ; rien de français', async ({ signedIn, api, allow }) => {
  allow.push(/status of 40[0-9]/);
  const a = await signedIn('en-US');
  const p = a.page;
  await open(p, 'account', 'My account');
  await expect(sections(p).getByRole('link', { name: 'My account' })).toHaveAttribute('aria-current', 'page');
  await noFrench(p);
  await a11y(p);

  // Pseudo : la consigne, l'erreur du serveur (mot de passe), puis le succès.
  const username = p.getByRole('form', { name: 'Username' });
  await expect(username.getByText(`Your current username is @${a.name}. It's what your friends type to add you: lowercase letters, numbers, dots and underscores. You can change it 2 times per hour at most.`)).toBeVisible();
  const change = async (name: string, password: string) => {
    await username.getByLabel('New username').fill(name);
    await username.getByLabel('Current password').fill(password);
    await username.getByRole('button', { name: 'Change username' }).click();
  };
  await change(unique('i18nb'), 'not-the-password');
  await expect(username.getByRole('alert')).toHaveText('Incorrect password.');
  await expect(username.getByLabel('Current password')).toHaveAttribute('aria-invalid', 'true');
  await a11y(p);
  const renamed = unique('i18nb');
  await change(renamed, PASSWORD);
  await expect(said(username, `Your username is now @${renamed}.`)).toBeVisible();
  await expect(username.getByText(`Your current username is @${renamed}.`)).toBeVisible();
  await noFrench(p);

  // E-mail : l'adresse, une erreur, puis la confirmation à faire (la réponse du serveur est simulée : cette instance n'envoie pas d'e-mail).
  const email = p.getByRole('form', { name: 'Email address' });
  await expect(email.getByText(`Your current address is ${a.name}@example.org (not confirmed yet).`)).toBeVisible();
  const mail = async (address: string, password: string) => {
    await email.getByLabel('New email address').fill(address);
    await email.getByLabel('Current password').fill(password);
    await email.getByRole('button', { name: 'Change address' }).click();
  };
  const fresh = `${unique('fresh')}@example.org`;
  await mail(fresh, 'not-the-password');
  await expect(email.getByRole('alert')).toHaveText('Incorrect password.');
  await mail(fresh, PASSWORD);
  await expect(said(email, 'Address saved.')).toBeVisible();
  await expect(email.getByText(`Your current address is ${fresh} (not confirmed yet).`)).toBeVisible();
  await p.route('**/api/v1/users/@me/email', (route) =>
    route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ email: fresh, email_verified: true, pending_email: 'next@example.org', mail: true }) }),
  );
  await mail('next@example.org', PASSWORD);
  await expect(said(email, 'A confirmation message has been sent to next@example.org. Your current address stays valid until you open the link.')).toBeVisible();
  await expect(
    email.getByText('Waiting for confirmation: next@example.org. Open the link sent to that address; until then, the old one stays valid.'),
  ).toBeVisible();
  await expect(email.getByText(`Your current address is ${fresh}.`)).toBeVisible();
  await noFrench(p);
  await a11y(p);

  // Mot de passe.
  const password = p.getByRole('form', { name: 'Password' });
  await expect(password.getByText('At least 8 characters. Your other devices will be signed out; this one stays signed in.')).toBeVisible();
  await password.getByLabel('Current password').fill('not-the-password');
  await password.getByLabel('New password').fill('another-long-password');
  await password.getByRole('button', { name: 'Change password' }).click();
  await expect(password.getByRole('alert')).toHaveText('Incorrect password.');
  await password.getByLabel('Current password').fill(PASSWORD);
  await password.getByRole('button', { name: 'Change password' }).click();
  await expect(said(password, 'Password changed. Your other devices have been signed out.')).toBeVisible();
  await noFrench(p);

  // Les trois blocs du bas.
  await expect(settings(p).getByText('Signs you out on this device.')).toBeVisible();
  // Le libellé relié au bouton en est le nom accessible ; le texte du bouton est ce qu'on voit.
  await expect(settings(p).getByRole('button', { name: 'Sign out everywhere' })).toHaveText('All devices');
  await expect(settings(p).getByRole('button', { name: 'Sign out', exact: true })).toHaveText('Sign out');
  await expect(settings(p).getByText('Signs you out of all your sessions, including this one. Useful after losing a device.')).toBeVisible();
  await expect(settings(p).getByText('Your account is closed right away and erased after 14 days. Your messages stay, signed "deleted account".')).toBeVisible();
  await expect(settings(p).getByRole('button', { name: 'Delete account' })).toHaveText('Delete');
  await settings(p).getByRole('button', { name: 'Delete account' }).click();
  const del = p.getByRole('alertdialog', { name: 'Delete your account?' });
  await expect(del.getByText("You won't be able to sign in anymore. Confirm with your password.")).toBeVisible();
  await expect(del.getByRole('button', { name: 'Cancel' })).toBeFocused();
  await expect(del.getByRole('button', { name: 'Delete permanently' })).toBeVisible();
  await del.getByLabel('Password').fill('not-the-password');
  await del.getByRole('button', { name: 'Delete permanently' }).click();
  await expect(del.getByRole('alert')).toHaveText('Incorrect password.');
  await noFrench(p);
  await a11y(p);
  await del.getByRole('button', { name: 'Cancel' }).click();
  await expect(del).toHaveCount(0);
  expect((await (await api(a.token)).get(`${API}/users/@me`)).status()).toBe(200);

  // Se déconnecter : on retombe sur la connexion, en anglais.
  await settings(p).getByRole('button', { name: 'Sign out', exact: true }).click();
  await p.waitForURL('**/login');
  await expect(p.getByRole('heading', { name: 'Welcome back', level: 1 })).toBeVisible();
  await noFrench(p);
});

test('I18N-A6 Réglages : toutes les sections, notifications (permission refusée, bloquée, non prises en charge), sections à venir ; rien de français', async ({ signedIn }) => {
  const a = await signedIn('en-US');
  const p = a.page;
  // Le navigateur sans écran répond toujours « denied » : on joue la permission (« default », puis refusée) à la main.
  await a.context.addInitScript(() => {
    if (sessionStorage.getItem('i18n-no-notifications')) {
      delete (window as unknown as Record<string, unknown>).Notification;
      return;
    }
    class Stub {
      static get permission() {
        return sessionStorage.getItem('i18n-perm') ?? 'default';
      }
      static requestPermission() {
        sessionStorage.setItem('i18n-perm', 'denied');
        return Promise.resolve('denied');
      }
    }
    (window as unknown as Record<string, unknown>).Notification = Stub;
  });
  await open(p, 'notifications', 'Notifications');
  await expect(sections(p).locator('h3')).toHaveText(['You', 'Preferences']);
  await expect(sections(p).getByRole('link')).toHaveText([
    'My account', 'My profile', 'Your data', 'Who can message you', 'Linked apps', 'Notifications', 'Voice and video', 'Appearance',
    'Accessibility', 'Keyboard shortcuts', 'Language and time',
  ]);
  await expect(settings(p).getByRole('button', { name: 'Close settings (Esc)' })).toBeVisible();

  const toggle = settings(p).getByRole('switch', { name: 'Desktop notifications' });
  await expect(toggle).not.toBeChecked();
  await expect(toggle).toHaveAccessibleDescription(
    'When KRAKEN isn\'t in the foreground: your mentions, your direct messages, and any channel set to "All messages". One click opens the conversation. This setting is specific to this device.',
  );
  await expect(settings(p).getByRole('heading', { name: 'Per server and per channel', level: 3 })).toBeVisible();
  await expect(settings(p).getByText('Right-click a server, a channel or a conversation (or Shift+F10 on the keyboard): "Mark as read", mute from 15 minutes to indefinitely')).toBeVisible();
  await expect(settings(p).getByText('The number of pending mentions also appears in the tab title. Alt+Shift+↑ and Alt+Shift+↓ jump to the previous or next unread channel.')).toBeVisible();
  await noFrench(p);
  await a11y(p);

  // Le navigateur refuse : la phrase dit quoi faire ; au rechargement, la permission est bloquée pour ce site.
  await toggle.click();
  await expect(settings(p).getByRole('alert')).toHaveText('Your browser denied the request. Allow notifications for this site in its settings, then try again.');
  await expect(toggle).not.toBeChecked();
  await noFrench(p);
  await p.reload();
  await expect(settings(p).getByRole('alert')).toHaveText('Notifications are blocked for this site in your browser.');
  await noFrench(p);
  await a11y(p);

  // Sans prise en charge : l'aide change et l'interrupteur est grisé.
  await p.evaluate(() => sessionStorage.setItem('i18n-no-notifications', '1'));
  await p.reload();
  await expect(settings(p).getByText("This browser (or app) can't show system notifications. Badges and the tab counter still work.")).toBeVisible();
  await expect(settings(p).getByRole('switch', { name: 'Desktop notifications' })).toBeDisabled();
  await noFrench(p);

  // Les sections pas encore construites.
  for (const [section, title, id] of [
    ['privacy', 'Your data', 'S07'],
    ['apps', 'Linked apps', 'S15'],
  ]) {
    await open(p, section, title);
    await expect(settings(p).getByText('Coming soon')).toBeVisible();
    await expect(settings(p).getByText(`Section ${id} is planned but not built yet.`)).toBeVisible();
    await noFrench(p);
    await a11y(p);
  }
});

// ───────── profil ─────────

test('I18N-A7 Réglages › My profile : formulaire, compteurs, aperçu de la carte, enregistrement ; rien de français', async ({ signedIn }) => {
  const a = await signedIn('en-US');
  const p = a.page;
  await open(p, 'profiles', 'My profile');
  const form = p.getByRole('form', { name: 'What others see' });
  await expect(form.getByLabel('Display name')).toHaveAttribute('placeholder', a.name);
  await expect(form.getByText(`Your unique username @${a.name} doesn't change here: it stays written below.`)).toBeVisible();
  await expect(form.getByLabel('Pronouns')).toHaveAttribute('placeholder', 'she/her, he/him, they/them…');
  await expect(form.getByText('Optional. 0 / 40')).toBeVisible();
  await expect(form.getByLabel('About you')).toHaveAttribute('placeholder', 'A few words about you, your games, how you like to chat…');
  await expect(form.getByText('0 / 190')).toBeVisible();
  await expect(settings(p).getByRole('heading', { name: 'Photo and banner', level: 3 })).toBeVisible();
  const preview = settings(p).getByRole('region', { name: 'Preview of your card' });
  await expect(preview).toBeVisible();
  await noFrench(p);
  await a11y(p);

  // L'aperçu suit la saisie, avant l'enregistrement.
  await form.getByLabel('Display name').fill('Pearl of the deep');
  await form.getByLabel('Pronouns').fill('she/her');
  await form.getByLabel('About you').fill('Collects shipwrecks.');
  await expect(form.getByText('Optional. 7 / 40')).toBeVisible();
  await expect(form.getByText('20 / 190')).toBeVisible();
  await expect(preview.getByText('Pearl of the deep', { exact: true })).toBeVisible();
  await expect(preview.getByRole('region', { name: 'About' })).toContainText('Collects shipwrecks.');
  await expect(preview.locator('dt')).toHaveText(['Member since']);
  await expect(preview.getByText('Online', { exact: true })).toBeVisible();
  await expect(preview.getByRole('button', { name: 'Play animation', includeHidden: true })).toHaveCount(1);
  await noFrench(p);
  await form.getByRole('button', { name: 'Save' }).click();
  await expect(said(form, 'Profile saved.')).toBeVisible();
  await a11y(p);
});

test('I18N-A8 la carte de profil d’un autre et la sienne : à propos, dates, rôles, ajouter en ami, bloquer, accepter ; rien de français', async ({ signedIn, api }) => {
  test.setTimeout(90_000);
  const a = await signedIn('en-US');
  const p = a.page;
  const [b, c] = await Promise.all([register(api, 'bea'), register(api, 'cyd')]);
  const asA = await api(a.token);
  const { gid, cid } = await guildOf(asA, 'Harbor');
  await joinWith(api, a, cid, b);
  await joinWith(api, a, cid, c);
  expect((await (await api(b.token)).patch(`${API}/users/@me`, { data: { display_name: 'Pearl of the deep', bio: 'Collects shipwrecks.', pronouns: 'she/her' } })).status()).toBe(200);
  const divers = await (await asA.post(`${API}/guilds/${gid}/roles`, { data: { name: 'Divers', color: 3, hoist: false, permissions: 0 } })).json();
  expect((await asA.put(`${API}/guilds/${gid}/members/${b.id}/roles/${divers.id}`)).status()).toBe(200);
  await p.goto(`/channels/${gid}/${cid}`);
  const list = p.getByRole('complementary', { name: 'Members' });
  const opener = (who: Who | Account) => list.getByRole('button', { name: new RegExp(`@${who.name}`) });

  // La carte d'un autre : ce qu'on y lit, et ses actions.
  await opener(b).click();
  const card = p.getByRole('dialog', { name: "Pearl of the deep's profile" });
  await expect(card).toBeVisible();
  await expect(card.getByRole('region', { name: 'About' })).toContainText('Collects shipwrecks.');
  await expect(card).toContainText('she/her');
  await expect(card.locator('dt')).toHaveText(['Member since', 'On this server since']);
  await expect(card.locator('dd')).toHaveText([/^[A-Z][a-z]+ \d{1,2}, \d{4}$/, /^[A-Z][a-z]+ \d{1,2}, \d{4}$/]);
  await expect(card.getByRole('region', { name: 'Roles' }).getByRole('listitem')).toHaveText(['Divers']);
  await expect(card.getByText('Offline', { exact: true })).toBeVisible();
  await expect(card.getByRole('button', { name: 'Send a message' })).toBeVisible();
  await expect(card.getByRole('button', { name: 'Add friend' })).toBeVisible();
  await expect(card.getByRole('button', { name: 'Block', exact: true })).toBeVisible();
  await expect(card.getByRole('button', { name: 'Play animation', includeHidden: true })).toHaveCount(1);
  await noFrench(p);
  await a11y(p);

  // Ajouter en ami, puis bloquer et débloquer depuis la carte.
  await card.getByRole('button', { name: 'Add friend' }).click();
  await expect(said(card, 'Friend request sent.')).toBeVisible();
  await expect(card.getByRole('button', { name: 'Request sent' })).toBeDisabled();
  await card.getByRole('button', { name: 'Block', exact: true }).click();
  const confirm = p.getByRole('alertdialog', { name: 'Block Pearl of the deep?' });
  await expect(confirm).toBeVisible();
  await confirm.getByRole('button', { name: 'Block', exact: true }).click();
  await expect(confirm).toHaveCount(0);
  await expect(card.getByRole('button', { name: 'Unblock' })).toBeVisible();
  await expect(card.getByRole('button', { name: 'Send a message' })).toHaveCount(0);
  await card.getByRole('button', { name: 'Unblock' }).click();
  await expect(said(card, 'Account unblocked.')).toBeVisible();
  await expect(card.getByRole('button', { name: 'Add friend' })).toBeVisible();
  await noFrench(p);
  await p.getByRole('button', { name: 'Close profile (Esc)' }).click();
  await expect(card).toHaveCount(0);

  // Une demande reçue : accepter depuis la carte.
  await ask(api, c, a);
  await opener(c).click();
  const other = p.getByRole('dialog', { name: `${c.name}'s profile` });
  await other.getByRole('button', { name: 'Accept request' }).click();
  await expect(said(other, "You're now friends.")).toBeVisible();
  await expect(other.getByText('Friend', { exact: true })).toBeVisible();
  await noFrench(p);
  await p.keyboard.press('Escape');
  await expect(other).toHaveCount(0);

  // La sienne : un lien vers Mon profil.
  await opener(a).click();
  const mine = p.getByRole('dialog', { name: `${a.name}'s profile` });
  await expect(mine.getByRole('link', { name: 'Edit my profile' })).toHaveAttribute('href', '/settings/profiles');
  await expect(mine.getByRole('button', { name: 'Send a message' })).toHaveCount(0);
  await noFrench(p);
});

test('I18N-A9 photo et bannière : fichier qui n’est pas une image, trop lourd, recadrage, enregistrement, retrait ; rien de français', async ({ signedIn, api }) => {
  test.setTimeout(90_000);
  const a = await signedIn('en-US');
  const p = a.page;
  await open(p, 'profiles', 'My profile');
  const me = async () => (await (await (await api(a.token)).get(`${API}/users/@me`)).json()) as { avatar: string | null; banner: string | null };

  // Photo : les consignes, deux refus.
  await expect(settings(p).getByText("PNG, JPEG, WebP or GIF, 8 MB max. An animated GIF or WebP stays animated (it isn't cropped).")).toBeVisible();
  await expect(settings(p).getByRole('button', { name: 'Remove', exact: true })).toHaveCount(0);
  const photo = p.getByLabel('Change photo');
  await photo.setInputFiles({ name: 'notes.txt', mimeType: 'text/plain', buffer: Buffer.from('this is not an image') });
  await expect(p.getByRole('alert').filter({ hasText: "This file isn't a readable image: choose a PNG, JPEG, WebP or GIF." })).toBeVisible();
  await expect(p.getByRole('dialog', { name: 'Crop your photo' })).toHaveCount(0);
  await noFrench(p);
  await photo.setInputFiles({ name: 'huge.png', mimeType: 'image/png', buffer: Buffer.alloc(9 * 1024 * 1024) });
  await expect(p.getByRole('alert').filter({ hasText: 'This image is too large: 8 MB at most.' })).toBeVisible();
  await noFrench(p);

  // Recadrage de la photo : titre, zone, curseur, aperçus, boutons.
  await photo.setInputFiles({ name: 'photo.png', mimeType: 'image/png', buffer: png(300, 180) });
  const crop = p.getByRole('dialog', { name: 'Crop your photo' });
  await expect(crop).toBeVisible();
  await expect(crop.getByRole('button', { name: 'Apply' })).toBeEnabled();
  await expect(crop.getByRole('application', { name: 'Crop area: the arrow keys move the image, + and − zoom.' })).toBeVisible();
  await expect(crop.getByRole('slider', { name: 'Zoom' })).toBeVisible();
  await expect(crop.getByText('Drag the image to position it, or use the arrow keys. The frame shows what will be kept.')).toBeVisible();
  await expect(crop.locator('figcaption')).toHaveText(['Large', 'Medium', 'Small']);
  await noFrench(p);
  await a11y(p);
  await crop.getByRole('button', { name: 'Cancel' }).click();
  await expect(crop).toHaveCount(0);
  await photo.setInputFiles({ name: 'photo.png', mimeType: 'image/png', buffer: png(300, 180) });
  await crop.getByRole('button', { name: 'Apply' }).click();
  await expect(crop).toHaveCount(0);
  await expect(said(settings(p), 'Photo saved.')).toBeVisible();
  expect((await me()).avatar).toMatch(/^[0-9a-f]{32}$/);
  await settings(p).getByRole('button', { name: 'Remove', exact: true }).click();
  await expect(said(settings(p), 'Photo removed.')).toBeVisible();
  expect((await me()).avatar).toBeNull();

  // Un GIF animé n'est pas recadré : il part tel quel.
  await photo.setInputFiles({ name: 'move.gif', mimeType: 'image/gif', buffer: animatedGif() });
  await expect(said(settings(p), 'Photo saved.')).toBeVisible();
  await expect(p.getByRole('dialog', { name: 'Crop your photo' })).toHaveCount(0);

  // Bannière : même parcours, forme large.
  await expect(settings(p).getByText('No banner')).toBeVisible();
  await expect(settings(p).getByText("PNG, JPEG, WebP or GIF, 8 MB max, wide shape (3 to 1). An animated GIF or WebP stays animated (it isn't cropped).")).toBeVisible();
  const banner = p.getByLabel('Change banner');
  await banner.setInputFiles({ name: 'notes.txt', mimeType: 'text/plain', buffer: Buffer.from('this is not an image') });
  await expect(p.getByRole('alert').filter({ hasText: "This file isn't a readable image" })).toBeVisible();
  await banner.setInputFiles({ name: 'banner.png', mimeType: 'image/png', buffer: png(900, 300) });
  const wide = p.getByRole('dialog', { name: 'Crop your banner' });
  await expect(wide).toBeVisible();
  await expect(wide.locator('figcaption')).toHaveText(['Preview']);
  await noFrench(p);
  await a11y(p);
  await wide.getByRole('button', { name: 'Apply' }).click();
  await expect(wide).toHaveCount(0);
  await expect(said(settings(p), 'Banner saved.')).toBeVisible();
  await expect(settings(p).getByRole('img', { name: 'Your current banner' })).toBeVisible();
  await expect(settings(p).getByText('No banner')).toHaveCount(0);
  await noFrench(p);
  await settings(p).getByRole('button', { name: 'Remove banner' }).click();
  await expect(said(settings(p), 'Banner removed.')).toBeVisible();
  expect((await me()).banner ?? null).toBeNull();
});

// ───────── pages légales, écrans à venir, mise à jour ─────────

test('I18N-A10 pages Privacy et Terms : titres, paragraphes, contact de l’exploitant, pied de page ; rien de français', async ({ page }) => {
  await page.goto('/privacy');
  const article = page.getByRole('article');
  await expect(page.getByRole('heading', { name: 'Privacy', level: 1 })).toBeVisible();
  await expect(article.getByRole('heading', { level: 2 })).toHaveText([
    'Who handles your data', 'What is stored', 'What is not stored', 'Cookies', 'Service providers', 'Retention and deletion', 'Your rights',
  ]);
  await expect(article.getByText('The operator of this KRAKEN instance, meaning the person or organization that hosts it. Contact: not provided by the operator.')).toBeVisible();
  await expect(article.getByText('Your email address, your username, your display name, your password in hashed form (argon2id, never in plain text), your servers and your messages, and your sessions (browser and sign-in date).')).toBeVisible();
  await expect(article.getByText('Just one, and it\'s essential: the one that keeps your session open.')).toBeVisible();
  await expect(article.getByText('The server host and, if emails are enabled, the email delivery service, both chosen by the operator.')).toBeVisible();
  await expect(article.getByText('signed "deleted account".')).toBeVisible();
  await expect(article.getByText('Access, rectification, erasure, portability, objection: write to the contact above.')).toBeVisible();
  const footer = article.locator('footer');
  await expect(footer).toHaveText('Privacy · Terms · Sign in');
  await expect(footer.getByRole('link', { name: 'Terms' })).toHaveAttribute('href', '/terms');
  await noFrench(page);
  await a11y(page);

  await footer.getByRole('link', { name: 'Terms' }).click();
  await expect(page).toHaveURL(/\/terms$/);
  await expect(page.getByRole('heading', { name: 'Terms of use', level: 1 })).toBeVisible();
  await expect(article.getByRole('heading', { level: 2 })).toHaveText(['The service', 'Free software', 'Rules', 'Moderation', 'Contact']);
  await expect(article.getByText('KRAKEN is free, with no paid plan. It is provided as is, with no guarantee of availability.')).toBeVisible();
  await expect(article.getByText('KRAKEN is released under the AGPL-3.0 license.')).toBeVisible();
  await expect(article.getByRole('link', { name: 'Source code of this instance' })).toHaveAttribute('href', /^https:\/\//);
  await expect(article.getByText('No illegal content, harassment, threats, spam, or sharing other people\'s data without their consent.')).toBeVisible();
  await expect(article.getByText('Each server is moderated by its owners.')).toBeVisible();
  await expect(article.getByText('Not provided by the operator.')).toBeVisible();
  await noFrench(page);
  await a11y(page);

  // Quand l'exploitant renseigne son contact et ses prestataires, ses mots (donnée de l'instance) passent tels quels.
  await page.route('**/api/v1/instance', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ contact: 'Harbor Hosting, via the contact form', processors: 'Example Cloud (hosting) and Example Mail (delivery)', source_url: 'https://example.org/source', public_url: 'http://127.0.0.1', max_upload_bytes: 1 }),
    }),
  );
  await page.goto('/privacy');
  await expect(article.getByText('Contact: Harbor Hosting, via the contact form.')).toBeVisible();
  await expect(article.getByText('Example Cloud (hosting) and Example Mail (delivery)', { exact: true })).toBeVisible();
  await page.goto('/terms');
  await expect(article.getByText('Harbor Hosting, via the contact form', { exact: true })).toBeVisible();
  await expect(article.getByRole('link', { name: 'Source code of this instance' })).toHaveAttribute('href', 'https://example.org/source');
  await noFrench(page);
});

test('I18N-A11 écrans pas encore construits : titre, numéro d’écran, retour à l’accueil ; rien de français', async ({ signedIn, api }) => {
  const a = await signedIn('en-US');
  const p = a.page;
  const { gid, cid } = await guildOf(await api(a.token), 'Harbor');
  for (const [path, title, id] of [
    ['/inbox', 'Inbox', 'S25'],
    ['/search', 'Message search', 'S26'],
    ['/discover', 'Explore servers', 'S30'],
    [`/users/${a.id}`, 'Profile', 'S20'],
    [`/guilds/${gid}/events`, 'Events', 'S27'],
    [`/channels/${gid}/${cid}/threads`, 'Threads and forum', 'S24'],
  ]) {
    await p.goto(path);
    await expect(p.getByRole('heading', { name: title, level: 1 })).toBeVisible();
    const stub = p.locator('.app-fill');
    await expect(stub.locator('b')).toHaveText(title);
    await expect(stub.getByText(`Screen ${id} is planned but not built yet.`)).toBeVisible();
    await expect(stub.getByRole('link', { name: 'Back to home' })).toHaveAttribute('href', '/channels/@me');
    await noFrench(p);
    await a11y(p);
  }
});

/** L'appli de bureau, vue de la page : `update_status` annonce 9.9.9, `update_install` avance par étapes. */
async function fakeDesktop(a: Account, mode: 'install' | 'download' = 'install') {
  await a.context.addInitScript((mode) => {
    const w = window as unknown as Record<string, unknown>;
    const state = { phase: 'idle', downloaded: 0, total: null as number | null };
    w.__fail = null as string | null;
    w.__TAURI_INTERNALS__ = {
      invoke: (cmd: string) => {
        if (cmd === 'update_status') {
          return Promise.resolve({ current: '0.8.0', available: '9.9.9', notes: null, notes_url: 'https://example.org/releases/v9.9.9', mode, ...state });
        }
        if (cmd === 'update_install') {
          if (w.__fail) return Promise.reject(w.__fail);
          if (mode === 'download') return Promise.resolve(null);
          Object.assign(state, { phase: 'downloading', downloaded: 42, total: 100 });
          return new Promise((done) => setTimeout(() => done(null), 1500));
        }
        return Promise.resolve(null);
      },
    };
  }, mode);
  await a.page.reload();
}

test('I18N-A12 mise à jour de l’appli de bureau : tuile, carte, nouveautés, progression, erreur et « Retry », téléchargement, « My account » ; rien de français', async ({ signedIn }) => {
  test.setTimeout(90_000);
  const a = await signedIn('en-US');
  const p = a.page;
  await fakeDesktop(a);
  const tile = p.getByRole('button', { name: 'Update v9.9.9 available' });
  await expect(tile).toBeVisible();
  await expect(tile).toHaveAttribute('title', 'Update v9.9.9 available');
  await noFrench(p);
  await a11y(p);
  await tile.click();
  const card = p.getByRole('group', { name: 'Update', exact: true });
  await expect(card.getByText('KRAKEN v9.9.9 is available')).toBeVisible();
  await expect(card.getByRole('link', { name: "What's new" })).toHaveAttribute('href', /releases\/v9\.9\.9$/);
  await expect(card.getByRole('button', { name: 'Later' })).toBeVisible();
  await noFrench(p);
  await a11y(p);

  // Une erreur de l'appli (son texte passe tel quel), puis « Retry » et la progression.
  await p.evaluate(() => ((window as unknown as { __fail: string }).__fail = 'The update could not be installed.'));
  await card.getByRole('button', { name: 'Install and restart' }).click();
  await expect(card.getByRole('alert')).toHaveText('The update could not be installed.');
  await p.evaluate(() => ((window as unknown as { __fail: null }).__fail = null));
  await card.getByRole('button', { name: 'Retry' }).click();
  await expect(card.getByRole('progressbar', { name: 'Update progress' })).toHaveAttribute('aria-valuenow', '42');
  await expect(card.getByText('Downloading… 42%')).toBeVisible();
  await expect(card.getByRole('button', { name: 'Later' })).toBeDisabled();
  await noFrench(p);
  await expect(card.getByText('Restarting…')).toBeVisible();

  // Sans paquet signé : « Download » ; la carte « My account » propose aussi la mise à jour.
  await fakeDesktop(a, 'download');
  await p.getByRole('button', { name: 'My account' }).click();
  await p.getByRole('button', { name: 'Update to v9.9.9' }).click();
  await expect(card.getByRole('button', { name: 'Download' })).toBeVisible();
  await expect(card.getByRole('button', { name: 'Install and restart' })).toHaveCount(0);
  await noFrench(p);
});

// ───────── 390 px ─────────

test('I18N-A13 sur 390 px : Amis, choix d’amis, Réglages, carte, recadrage, pages légales et écrans à venir tiennent sans débordement, axe propre', async ({ signedIn, api }) => {
  test.setTimeout(120_000);
  const a = await signedIn('en-US', { width: 390, height: 844 });
  const p = a.page;
  const [b, c] = await Promise.all([register(api, 'bea'), register(api, 'cyd')]);
  await befriend(api, a, b);
  await ask(api, c, a);
  const { gid, cid } = await guildOf(await api(a.token), 'Harbor');
  await joinWith(api, a, cid, b);
  const check = async () => {
    await noFrench(p);
    await noScroll(p);
    await a11y(p);
  };

  await expect(p.getByRole('heading', { name: 'Friends', level: 1 })).toBeVisible();
  await check();
  await tab(p, 'All').click();
  await expect(p.getByRole('heading', { name: 'All 1', level: 2 })).toBeVisible();
  await check();
  await tab(p, /^Pending/).click();
  await expect(p.getByRole('heading', { name: 'Pending 1', level: 2 })).toBeVisible();
  await check();
  await tab(p, 'Add a friend').click();
  await expect(p.getByRole('heading', { name: 'Add a friend', level: 2 })).toBeVisible();
  await check();

  // Choisir des amis et une confirmation.
  await p.getByRole('button', { name: 'Open navigation' }).click();
  await p.getByRole('button', { name: 'New conversation or group' }).click();
  const picker = p.getByRole('dialog', { name: 'New conversation' });
  await expect(picker.getByRole('checkbox')).toHaveCount(1);
  await check();
  await picker.getByRole('button', { name: 'Cancel' }).click();
  await expect(picker).toHaveCount(0);
  await p.goto('/channels/@me');
  await tab(p, 'All').click();
  await p.getByRole('button', { name: `More actions for ${b.name}` }).click();
  await p.getByRole('button', { name: `Block ${b.name}` }).click();
  await expect(p.getByRole('alertdialog', { name: `Block ${b.name}?` })).toBeVisible();
  await check();
  await p.keyboard.press('Escape');

  // Réglages.
  for (const [section, heading] of [
    ['account', 'My account'],
    ['profiles', 'My profile'],
    ['messaging', 'Who can message you'],
    ['notifications', 'Notifications'],
    ['privacy', 'Your data'],
  ]) {
    await open(p, section, heading);
    await check();
  }

  // Recadrage de la photo.
  await open(p, 'profiles', 'My profile');
  await p.getByLabel('Change photo').setInputFiles({ name: 'photo.png', mimeType: 'image/png', buffer: png(300, 180) });
  const crop = p.getByRole('dialog', { name: 'Crop your photo' });
  await expect(crop.getByRole('button', { name: 'Apply' })).toBeEnabled();
  await check();
  await crop.getByRole('button', { name: 'Cancel' }).click();

  // La carte d'un membre.
  await p.goto(`/channels/${gid}/${cid}`);
  await p.getByRole('button', { name: 'Member list' }).click();
  await p.getByRole('complementary', { name: 'Members' }).getByRole('button', { name: new RegExp(`@${b.name}`) }).click();
  await expect(p.getByRole('dialog', { name: `${b.name}'s profile` })).toBeVisible();
  await check();
  await p.keyboard.press('Escape');

  // Pages légales et écran à venir.
  for (const [path, heading] of [
    ['/privacy', 'Privacy'],
    ['/terms', 'Terms of use'],
    ['/inbox', 'Inbox'],
  ]) {
    await p.goto(path);
    await expect(p.getByRole('heading', { name: heading, level: 1 })).toBeVisible();
    await check();
  }
});

// ───────── le français ne bouge pas ─────────

test.describe('navigateur en français', () => {
  test.use({ locale: 'fr-FR' });

  test('I18N-A14 les mêmes écrans en français : les textes d’origine, ni anglais ni espace perdu', async ({ signedIn, api, page, allow }) => {
    allow.push(/status of 40[0-9]/);
    const a = await signedIn('fr-FR');
    const p = a.page;
    const b = await register(api, 'bea');
    await befriend(api, a, b);
    await expect(p.getByRole('heading', { name: 'Amis', level: 1 })).toBeVisible();
    await expect(p.getByRole('tablist', { name: 'Filtrer les amis' }).getByRole('tab')).toHaveText(['En ligne', 'Tous', 'En attente', 'Bloqués', 'Ajouter un ami']);
    await expect(p.getByRole('heading', { name: 'En ligne 0', level: 2 })).toBeVisible();
    await expect(p.getByText("Personne en ligne pour l'instant")).toBeVisible();
    await expect(p.getByRole('complementary', { name: 'Activité' }).getByText('Aucun salon vocal occupé')).toBeVisible();
    await p.getByRole('tab', { name: 'Tous' }).click();
    const row = p.getByRole('listitem').filter({ hasText: `@${b.name}` });
    await expect(row).toContainText(`@${b.name} · hors ligne`);
    await expect(row.getByRole('button', { name: `Écrire à ${b.name}` })).toHaveAttribute('title', 'Message');
    await expect(row.getByRole('button', { name: `Appeler ${b.name}` })).toHaveAttribute('title', 'Appeler');
    await row.getByRole('button', { name: `Plus d'actions pour ${b.name}` }).click();
    await p.getByRole('button', { name: `Bloquer ${b.name}` }).click();
    const block = p.getByRole('alertdialog', { name: `Bloquer ${b.name} ?` });
    await expect(block.getByText("Vous ne serez plus amis. Ce compte ne pourra plus t'écrire en privé ni te demander en ami, et ses messages seront repliés dans les salons que vous partagez. Il n'en est pas prévenu.")).toBeVisible();
    await expect(block.getByRole('button', { name: 'Annuler' })).toBeVisible();
    await p.keyboard.press('Escape');
    await p.getByRole('tab', { name: 'Ajouter un ami' }).click();
    await expect(p.getByLabel('Pseudo de ton ami')).toHaveAttribute('placeholder', 'pseudo');
    await p.getByLabel('Pseudo de ton ami').fill(a.name);
    await p.getByRole('button', { name: 'Envoyer la demande' }).click();
    await expect(p.getByRole('alert')).toHaveText("C'est ton propre pseudo.");

    await p.goto('/settings/account');
    await expect(p.getByRole('form', { name: 'Pseudo' }).getByText(`Ton pseudo actuel est @${a.name}. C'est celui que tes amis tapent pour t'ajouter : minuscules, chiffres, points et tirets bas. Tu peux le changer 2 fois par heure au plus.`)).toBeVisible();
    await expect(p.getByRole('form', { name: 'Adresse e-mail' }).getByText(`Ton adresse actuelle est ${a.name}@example.org (pas encore confirmée).`)).toBeVisible();
    await expect(p.getByText("Ton compte est fermé tout de suite et effacé au bout de 14 jours. Tes messages restent, signés « compte supprimé ».")).toBeVisible();
    await p.goto('/inbox');
    await expect(p.getByRole('heading', { name: 'Boîte de réception', level: 1 })).toBeVisible();
    await expect(p.getByText('Écran S25 prévu, pas encore construit.')).toBeVisible();

    await page.goto('/privacy');
    await expect(page.getByRole('heading', { name: 'Confidentialité', level: 1 })).toBeVisible();
    await expect(page.getByText("L'exploitant de cette instance KRAKEN, c'est-à-dire la personne ou l'organisation qui l'héberge. Contact : non renseigné par l'exploitant.")).toBeVisible();
    await expect(page.getByText("Un seul, indispensable : celui qui garde ta session ouverte.")).toBeVisible();
    await expect(page.locator('footer').last()).toHaveText('Confidentialité · Conditions · Connexion');
    await page.goto('/terms');
    await expect(page.getByRole('heading', { name: "Conditions d'utilisation", level: 1 })).toBeVisible();
    await expect(page.getByText('KRAKEN est publié sous licence AGPL-3.0. Code source de cette instance.')).toBeVisible();

    // Aucune phrase du catalogue anglais sur ces pages.
    expect(ENGLISH_APP.length).toBeGreaterThan(100);
    const seen = (await everything(page)) + (await everything(p));
    expect(ENGLISH_APP.filter((e) => seen.includes(e))).toEqual([]);
  });
});
