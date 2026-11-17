// Langues de l'interface, lot « serveurs » (PR #31) : créer un serveur avec un modèle, puis les Réglages du serveur
// (Vue d'ensemble, Rôles, Salons, Emojis, Invitations, Bannis, Journal d'audit, Propriété), les réglages d'un salon,
// l'exclusion et la fiche d'un membre, en anglais. Fichiers migrés : guild.rs, channels.rs, audit.rs, emoji_settings.rs,
// new_guild.rs, overview.rs, icon_picker.rs. Chaque test garde un œil sur le français : aucune des phrases françaises du
// catalogue « guilds » ne doit rester à l'écran, texte ou nom accessible.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test as base, expect, a11y, watch, API, PASSWORD, unique } from '../fixtures';
import { animatedGif, png } from '../images';
import type { APIRequestContext, BrowserContext, Page } from '@playwright/test';

type Account = { page: Page; context: BrowserContext; id: string; name: string; token: string };
type Api = (t?: string) => Promise<APIRequestContext>;

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
      const name = unique('i18ng');
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

const catalog = readFileSync(join(__dirname, '../../crates/kraken-client/i18n/en/guilds.tsv'), 'utf8').split('\n');
const split = catalog.findIndex((l) => l.startsWith('# Modèles de serveur'));
const pairs = (lines: string[]) =>
  lines
    .filter((l) => l && !l.startsWith('#'))
    .map((l) => l.split('\t') as [string, string])
    .filter(([fr, en]) => fr !== en && !fr.includes('{') && fr.length >= 8)
    .map(([fr]) => fr);
/** Les phrases françaises du catalogue « guilds » : ce qui ne doit plus se lire en anglais. */
const FRENCH = pairs(catalog.slice(0, split));
/** Les noms français et anglais des catégories et des salons des modèles : une seule table, dans `kraken-core`
 *  (`group("Accueil", "Welcome", …)`, `text("annonces", "announcements")`). */
const coreSource = readFileSync(join(__dirname, '../../crates/kraken-core/src/templates.rs'), 'utf8');
const NAMES = [...coreSource.matchAll(/\b(?:text|voice|group)\(\s*"([^"]+)"\s*,\s*"([^"]+)"/g)].map((m) => [m[1], m[2]] as [string, string]);
/** Les textes français des modèles (nom et description de la carte, noms des catégories et des salons) : traduits sur la page
 *  de création ; sur les pages d'un serveur né d'un modèle en français, ce sont les noms de ses salons, des données du serveur. */
const TEMPLATES = [
  ...catalog
    .slice(split)
    .filter((l) => l && !l.startsWith('#'))
    .map((l) => l.split('\t') as [string, string]),
  ...NAMES,
]
  .filter(([fr, en]) => fr !== en && fr.length >= 5 && !fr.includes('{'))
  .map(([fr]) => fr);

/** Des mots français courts, trop courts pour la liste ci-dessus : cherchés comme mots entiers. */
const SHORT = ['Rôles', 'Invitations', 'Bannis', 'Propriété', 'Annuler', 'Enregistrer', 'Supprimer', 'Fermer', 'Retirer', 'Ajouter', 'Copier', 'Révoquer', 'Réinitialiser', 'Actualiser', 'Membres', 'Rejoindre', 'Modèle'];

/** Tout ce qu'on lit ou entend sur la page : le texte visible et les noms accessibles (aria-label, title, placeholder, alt). */
const everything = (p: Page) =>
  p.evaluate(() => {
    const names = [...document.querySelectorAll('[aria-label],[title],[placeholder],[alt]')].flatMap((e) =>
      ['aria-label', 'title', 'placeholder', 'alt'].map((a) => e.getAttribute(a) ?? ''),
    );
    return document.body.innerText + '\n' + names.join('\n');
  });

/** Aucune phrase française de ce lot n'est à l'écran (avec `templates`, pas non plus les textes des modèles). */
async function noFrench(p: Page, templates = false) {
  expect(FRENCH.length, 'le catalogue « guilds » est lu').toBeGreaterThan(200);
  expect(TEMPLATES.length, 'les modèles du catalogue sont lus').toBeGreaterThan(15);
  const seen = await everything(p);
  const words = SHORT.filter((w) => new RegExp(`(^|[^\\p{L}])${w}($|[^\\p{L}])`, 'u').test(seen));
  expect({ phrases: FRENCH.filter((k) => seen.includes(k)), words, templates: templates ? TEMPLATES.filter((k) => seen.includes(k)) : [] }).toEqual({
    phrases: [],
    words: [],
    templates: [],
  });
}

const noScroll = async (p: Page) =>
  expect(await p.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(p.viewportSize()!.width);
/** Une zone `status` avec ce texte (il y en a beaucoup : on filtre toujours). */
const said = (p: Page, text: string | RegExp) => p.getByRole('status').filter({ hasText: text });
const settings = (p: Page) => p.getByRole('dialog', { name: 'Server settings' });
const nav = (p: Page) => settings(p).getByRole('navigation', { name: 'Server settings sections' }).getByRole('link');
const SECTIONS = ['Overview', 'Roles', 'Channels', 'Emojis', 'Invites', 'Bans', 'Audit log', 'Ownership'];
const open = async (p: Page, gid: string, section: string, heading: string) => {
  await p.goto(`/guilds/${gid}/settings/${section}`);
  await expect(settings(p).getByRole('heading', { name: heading, level: 2 })).toBeVisible();
};

async function register(api: Api, prefix: string) {
  const name = unique(prefix);
  const res = await (await api()).post(`${API}/auth/register`, { data: { username: name, email: `${name}@example.org`, password: PASSWORD } });
  expect(res.status(), await res.text()).toBeLessThan(300);
  const { token, user } = await res.json();
  return { id: user.id as string, name, token: token as string };
}

/** Un serveur (par l'API), éventuellement d'après un modèle : son id, celui de son premier salon textuel, et son salon vocal. */
async function guildOf(as: APIRequestContext, name: string, template?: string) {
  const res = await as.post(`${API}/guilds`, { data: { name, template } });
  expect(res.status(), await res.text()).toBeLessThan(300);
  const g = await res.json();
  const text = g.channels.find((c: { kind: string }) => c.kind === 'text');
  return { gid: g.guild.id as string, cid: text.id as string, general: text.name as string };
}

const joinWith = async (api: Api, owner: { token: string }, cid: string, guest: { token: string }) => {
  const inv = await (await api(owner.token)).post(`${API}/channels/${cid}/invites`, { data: {} });
  expect(inv.status(), await inv.text()).toBeLessThan(300);
  const { code } = await inv.json();
  expect((await (await api(guest.token)).post(`${API}/invites/${code}`)).status()).toBe(200);
  return code as string;
};

async function role(as: APIRequestContext, gid: string, name: string, permissions = 0) {
  const res = await as.post(`${API}/guilds/${gid}/roles`, { data: { name, permissions } });
  expect(res.status(), await res.text()).toBe(201);
  return (await res.json()) as { id: string };
}

async function channel(as: APIRequestContext, gid: string, name: string, kind = 'text') {
  const res = await as.post(`${API}/guilds/${gid}/channels`, { data: { name, kind } });
  expect(res.status(), await res.text()).toBe(201);
  return (await res.json()) as { id: string };
}

const give = async (as: APIRequestContext, gid: string, who: { id: string }, r: { id: string }) =>
  expect((await as.put(`${API}/guilds/${gid}/members/${who.id}/roles/${r.id}`)).status()).toBe(200);

const MANAGE_CHANNELS = 1 << 18;
const MANAGE_MESSAGES = 1 << 8;
const BAN = 1 << 23;
const SEND_MESSAGES = 1 << 1;

// ───────── créer un serveur ─────────

test('I18N-G1 créer un serveur avec un modèle : cartes, liste « Will be created », nom proposé, invitation ; rien de français', async ({ signedIn, api }) => {
  const a = await signedIn('en-US');
  const p = a.page;
  await p.goto('/guilds/new');
  await expect(p.getByRole('heading', { name: 'Create your server', level: 1 })).toBeVisible();
  await expect(p.getByText('A space for your crew, with text and voice channels.')).toBeVisible();
  await expect(p.getByRole('heading', { name: 'Template', level: 2 })).toBeVisible();

  // Les cinq modèles : nom et description en anglais, la même liste de salons que le serveur bâtira.
  const group = p.getByRole('radiogroup', { name: 'Template' });
  await expect(group.getByRole('radio')).toHaveCount(5);
  for (const [name, text] of [
    ['Empty', 'One text channel and one voice channel, ready for you to build on.'],
    ['Community', 'Announcements, rules and support, for a club or a project.'],
    ['Gaming', 'Find teammates and talk during the game.'],
    ['Study', 'Classes, homework and voice study rooms.'],
    ['Friends', 'Chat, share photos and hang out in voice.'],
  ]) {
    await expect(group.getByRole('radio', { name: new RegExp(`^${name}`) })).toBeAttached();
    await expect(group.getByText(text)).toBeVisible();
  }
  const made = p.locator('.app-ng-made');
  await expect(made.getByText('Will be created:')).toBeVisible();
  await expect(made.locator('li')).toHaveText(['Text channel general', 'Voice channel General']);
  await group.getByRole('radio', { name: /^Study/ }).check({ force: true });
  await expect(made.locator('li')).toHaveText([
    'Text channel general',
    'Text channel classes',
    'Text channel homework',
    'Text channel resources',
    'Voice channel Study room',
    'Voice channel Group study',
    'Voice channel Break',
  ]);
  await group.getByRole('radio', { name: /^Community/ }).check({ force: true });
  await expect(made.locator('li').first()).toHaveText('Text channel announcements');
  await group.getByRole('radio', { name: /^Study/ }).check({ force: true });
  await noFrench(p, true);

  // Le nom proposé est dans la langue de l'interface ; l'icône est facultative.
  const nameField = p.getByLabel('Server name');
  await expect(nameField).toHaveValue(`${a.name}'s server`);
  await expect(p.getByRole('heading', { name: 'Icon (optional)', level: 2 })).toBeVisible();
  await expect(p.getByRole('img', { name: "No icon: the server's initials" })).toBeVisible();
  await expect(p.getByText('Choose an image')).toBeVisible();
  await expect(p.getByText('PNG, JPEG, WebP or GIF. A still image is cropped to a square;')).toBeVisible();
  await expect(p.getByLabel('Already have an invite?')).toBeVisible();
  await a11y(p);

  // Rejoindre sans rien coller : le message est en anglais, relié au champ.
  await p.getByRole('button', { name: 'Join', exact: true }).click();
  await expect(p.getByRole('alert').filter({ hasText: 'Paste an invite link or code.' })).toBeVisible();
  await expect(p.getByRole('button', { name: 'Creating…' })).toHaveCount(0);

  // Créer : le serveur suit le modèle choisi (l'identifiant envoyé n'est pas traduit), on arrive dans son premier salon.
  await nameField.fill('Quarter');
  await p.getByRole('button', { name: 'Create server' }).click();
  await p.waitForURL(/\/channels\/[0-9a-f-]{36}\/[0-9a-f-]{36}$/);
  const gid = new URL(p.url()).pathname.split('/')[2];
  const g = await (await (await api(a.token)).get(`${API}/guilds/${gid}`)).json();
  expect(g.guild.name).toBe('Quarter');
  // Le serveur est bâti en anglais, comme l'aperçu : la langue de l'interface part avec la demande.
  expect(shapeOf(g)).toEqual({ cats: EN.etudes.cats, channels: EN.etudes.channels });
  expect(await sidebarOf(p)).toEqual({ cats: EN.etudes.cats, channels: EN.etudes.channels });
});

/** Ce que chaque modèle bâtit, dans l'ordre d'affichage : le nom de la carte, les catégories et tous les salons. */
type Built = { card: string; cats: string[]; channels: string[] };
const EN: Record<string, Built> = {
  vide: { card: 'Empty', cats: ['Text channels', 'Voice channels'], channels: ['general', 'General'] },
  communaute: {
    card: 'Community',
    cats: ['Welcome', 'Discussions', 'Voice channels'],
    channels: ['announcements', 'rules', 'general', 'help', 'Open chat'],
  },
  jeux: {
    card: 'Gaming',
    cats: ['Text channels', 'Voice channels'],
    channels: ['general', 'looking-for-players', 'tips', 'Game 1', 'Game 2', 'Break'],
  },
  etudes: {
    card: 'Study',
    cats: ['Study', 'Voice channels'],
    channels: ['general', 'classes', 'homework', 'resources', 'Study room', 'Group study', 'Break'],
  },
  amis: {
    card: 'Friends',
    cats: ['Text channels', 'Voice channels'],
    channels: ['general', 'photos', 'outings', 'The couch', 'Movie night'],
  },
};
const FR: Record<string, Built> = {
  vide: { card: 'Vide', cats: ['Salons textuels', 'Salons vocaux'], channels: ['général', 'Général'] },
  communaute: {
    card: 'Communauté',
    cats: ['Accueil', 'Discussions', 'Salons vocaux'],
    channels: ['annonces', 'règles', 'général', 'aide', 'Discussion libre'],
  },
  jeux: {
    card: 'Jeux',
    cats: ['Salons textuels', 'Salons vocaux'],
    channels: ['général', 'recherche-de-joueurs', 'astuces', 'Partie 1', 'Partie 2', 'Pause'],
  },
  etudes: {
    card: 'Études',
    cats: ['Travail', 'Salons vocaux'],
    channels: ['général', 'cours', 'devoirs', 'ressources', 'Salle de travail', 'Révisions en groupe', 'Pause'],
  },
  amis: {
    card: 'Amis',
    cats: ['Salons textuels', 'Salons vocaux'],
    channels: ['général', 'photos', 'sorties', 'Le canapé', 'Soirée film'],
  },
};

/** Les catégories, puis tous les salons (catégorie par catégorie), d'un serveur tel que l'API le décrit. */
function shapeOf(g: { channels: { id: string; name: string; kind: string; parent_id: string | null; position: number }[] }) {
  const byPosition = (a: { position: number }, b: { position: number }) => a.position - b.position;
  const cats = g.channels.filter((c) => c.kind === 'category').sort(byPosition);
  return {
    cats: cats.map((c) => c.name),
    channels: cats.flatMap((cat) => g.channels.filter((c) => c.parent_id === cat.id).sort(byPosition).map((c) => c.name)),
  };
}
/** La barre latérale du serveur ouvert : ses catégories et ses salons, dans l'ordre de la page. */
const sidebarOf = (p: Page) =>
  p.locator('.app-sidebar').evaluate((root) => ({
    cats: [...root.querySelectorAll('.k-category')].map((e) => (e as HTMLElement).innerText.trim()),
    channels: [...root.querySelectorAll('.app-chan .app-ellipsis')].map((e) => (e as HTMLElement).innerText.trim()),
  }));
/** La liste « Sera créé » : le nom de chaque salon, sans le mot caché qui en dit le genre. */
const previewOf = (p: Page) =>
  p.locator('.app-ng-made li').evaluateAll((els) =>
    els.map((e) => (e.textContent ?? '').replace(/^\s*(Text channel|Voice channel|Salon textuel|Salon vocal)\s+/, '').trim()),
  );

/** Crée un serveur d'après chaque modèle, par la page : l'aperçu, le serveur bâti et la barre latérale disent les mêmes noms. */
async function createFromEveryTemplate(a: Account, api: Api, names: Record<string, Built>, labels: { name: string; create: string; group: string }) {
  const p = a.page;
  for (const [id, want] of Object.entries(names)) {
    await p.goto('/guilds/new');
    await p.getByRole('radiogroup', { name: labels.group }).getByRole('radio', { name: new RegExp(`^${want.card}`) }).check({ force: true });
    await expect(p.locator('.app-ng-made li')).toHaveCount(want.channels.length);
    expect(await previewOf(p), `aperçu de ${id}`).toEqual(want.channels);
    await p.getByLabel(labels.name).fill(`Harbor ${id}`);
    await p.getByRole('button', { name: labels.create }).click();
    await p.waitForURL(/\/channels\/[0-9a-f-]{36}\/[0-9a-f-]{36}$/);
    const gid = new URL(p.url()).pathname.split('/')[2];
    const g = await (await (await api(a.token)).get(`${API}/guilds/${gid}`)).json();
    expect(shapeOf(g), `serveur ${id}`).toEqual({ cats: want.cats, channels: want.channels });
    await expect(p.locator('.app-sidebar .app-chan').first()).toBeVisible();
    expect(await sidebarOf(p), `barre latérale de ${id}`).toEqual({ cats: want.cats, channels: want.channels });
    // Le premier salon textuel s'ouvre.
    await expect(p.getByRole('heading', { name: want.channels[0], level: 1 })).toBeVisible();
  }
}

test('I18N-G1b créer un serveur en anglais : pour chaque modèle, l’aperçu, le serveur bâti et la barre latérale ont les mêmes noms anglais', async ({ signedIn, api }) => {
  test.setTimeout(120_000);
  const a = await signedIn('en-US');
  await createFromEveryTemplate(a, api, EN, { name: 'Server name', create: 'Create server', group: 'Template' });
  // Aucun nom français de modèle dans la barre latérale du dernier serveur.
  const seen = await everything(a.page);
  expect(TEMPLATES.filter((k) => seen.includes(k))).toEqual([]);
});

test('I18N-G1c créer un serveur en français : rien ne change, l’aperçu, le serveur et la barre latérale gardent les noms français', async ({ signedIn, api }) => {
  test.setTimeout(120_000);
  const a = await signedIn('fr-FR');
  await createFromEveryTemplate(a, api, FR, { name: 'Nom du serveur', create: 'Créer le serveur', group: 'Modèle' });
});

test('I18N-G1d l’API nomme le serveur dans la langue demandée ; sans langue ou avec une langue inconnue, en français', async ({ signedIn, api }) => {
  const a = await signedIn('en-US');
  const as = await api(a.token);
  const make = async (data: Record<string, unknown>) => {
    const res = await as.post(`${API}/guilds`, { data: { name: 'Harbor', template: 'communaute', ...data } });
    expect(res.status(), await res.text()).toBe(201);
    return shapeOf(await res.json());
  };
  for (const lang of ['en', 'en-GB', 'EN_us']) expect(await make({ lang }), lang).toEqual({ cats: EN.communaute.cats, channels: EN.communaute.channels });
  for (const lang of ['fr', 'fr-CA', 'de', '', 'english', undefined]) expect(await make({ lang }), String(lang)).toEqual({ cats: FR.communaute.cats, channels: FR.communaute.channels });
  // Sans modèle non plus : la structure d'avant, dans la langue demandée.
  const plain = await as.post(`${API}/guilds`, { data: { name: 'Harbor', lang: 'en' } });
  expect(shapeOf(await plain.json())).toEqual({ cats: EN.vide.cats, channels: EN.vide.channels });
  expect((await as.post(`${API}/guilds`, { data: { name: 'Harbor', lang: 3 } })).status()).toBe(422);
});

// ───────── Réglages du serveur : Vue d'ensemble, Rôles ─────────

test('I18N-G2 Vue d’ensemble et Rôles : sections, icône, nom ; liste, onglets, couleurs, droits, recherche, membres ; rien de français', async ({ signedIn, api }) => {
  const a = await signedIn('en-US');
  const sam = await register(api, 'sam');
  const as = await api(a.token);
  const { gid, cid } = await guildOf(as, 'Harbor', 'communaute');
  await joinWith(api, a, cid, sam);
  await role(as, gid, 'Helpers');
  const p = a.page;

  await open(p, gid, 'overview', 'Overview');
  await expect(nav(p)).toHaveText(SECTIONS);
  await expect(nav(p).first()).toHaveAttribute('aria-current', 'page');
  await expect(settings(p).getByRole('button', { name: 'Close settings (Esc)' })).toBeVisible();
  await expect(settings(p).getByRole('heading', { name: 'Server icon', level: 3 })).toBeVisible();
  await expect(settings(p).getByText("It replaces the server's initials in the list on the left, for all its members.")).toBeVisible();
  await expect(settings(p).getByRole('img', { name: "No icon: the server's initials" })).toBeVisible();
  await expect(settings(p).getByText('Choose an image')).toBeVisible();
  await expect(settings(p).getByRole('button', { name: 'Remove icon' })).toHaveCount(0);
  await noFrench(p);
  await a11y(p);

  // Une icône : l'aperçu et le bouton changent de nom, la zone d'état le dit.
  expect((await as.put(`${API}/guilds/${gid}/icon`, { headers: { 'content-type': 'image/png' }, data: png(32, 32) })).status()).toBe(200);
  await p.reload();
  await expect(settings(p).getByRole('img', { name: 'Current server icon' })).toBeVisible();
  await settings(p).getByRole('button', { name: 'Remove icon' }).click();
  await expect(said(p, 'Icon updated.')).toBeVisible();
  await expect(settings(p).getByRole('img', { name: "No icon: the server's initials" })).toBeVisible();
  // Le nom.
  const name = settings(p).getByLabel('Server name');
  await expect(name).toHaveValue('Harbor');
  await name.fill('Harbor Light');
  await settings(p).getByRole('button', { name: 'Save', exact: true }).click();
  await expect(said(p, 'Server name saved.')).toBeVisible();
  await noFrench(p);

  // Rôles : la liste, l'éditeur, les trois onglets.
  await nav(p).filter({ hasText: 'Roles' }).click();
  await expect(settings(p).getByRole('heading', { name: 'Roles', level: 2 })).toBeVisible();
  await expect(settings(p).getByText('The higher a role is in the list, the more powerful it is.')).toBeVisible();
  const list = settings(p).getByRole('list', { name: 'Roles, highest to lowest' });
  await expect(list.getByRole('button', { name: /^@everyone/ })).toBeVisible();
  await settings(p).getByRole('button', { name: 'Create a role' }).click();
  await expect(settings(p).getByRole('heading', { name: 'New role', level: 3 })).toBeVisible();
  await expect(settings(p).getByRole('tablist', { name: 'Role settings' }).getByRole('tab')).toHaveText(['Display', 'Permissions', 'Members']);
  const roleName = settings(p).getByLabel('Role name');
  await expect(roleName).toBeFocused();
  await roleName.fill('Moderators');
  const colors = settings(p).getByRole('radiogroup', { name: 'Name color' }).getByRole('radio');
  await expect(colors).toHaveCount(9);
  await expect(colors.evaluateAll((els) => els.map((e) => e.getAttribute('aria-label')))).resolves.toEqual([
    'Default', 'Red', 'Gold', 'Lime', 'Green', 'Turquoise', 'Blue', 'Purple', 'Pink',
  ]);
  await expect(colors.nth(6)).toHaveAttribute('title', 'Blue');
  await colors.nth(6).click();
  await settings(p).getByRole('switch', { name: 'Show separately in the member list' }).check();
  await expect(settings(p).getByText('Members with this role get their own section, above the others.')).toBeVisible();
  const bar = settings(p).getByRole('region', { name: 'Role changes' });
  await expect(bar.getByText('Unsaved changes')).toBeVisible();
  await expect(list.getByRole('button', { name: /^New role.*unsaved changes/ })).toBeVisible();
  await noFrench(p);
  await a11y(p);

  // Droits : catégories, noms, aide, droits sensibles, recherche.
  await settings(p).getByRole('tab', { name: 'Permissions' }).click();
  await expect(settings(p).locator('.app-perms legend')).toHaveText(['General', 'Members', 'Text Channels', 'Voice', 'Advanced']);
  await expect(settings(p).getByRole('switch', { name: 'Manage Messages' })).toBeVisible();
  await expect(settings(p).getByText("Delete other people's messages.")).toBeVisible();
  await expect(settings(p).getByRole('switch', { name: /Ban Members/ })).toBeVisible();
  await expect(settings(p).getByRole('switch', { name: 'Connect' })).toBeVisible();
  await expect(settings(p).locator('.app-sensitive').first()).toHaveAttribute('title', 'Sensitive permission');
  const search = settings(p).getByRole('searchbox', { name: 'Search permissions' });
  await expect(search).toHaveAttribute('placeholder', 'Search permissions');
  await search.fill('ban');
  await expect(settings(p).getByRole('switch')).toHaveCount(1);
  await expect(settings(p).getByRole('switch', { name: /Ban Members/ })).toBeVisible();
  await search.fill('nothing like this');
  await expect(said(p, 'No permission matches this search.')).toBeVisible();
  await search.fill('manage messages');
  await settings(p).getByRole('switch', { name: 'Manage Messages' }).check();
  await a11y(p);
  await search.fill('');
  await bar.getByRole('button', { name: 'Save' }).click();
  await expect(bar).toHaveCount(0);
  await expect(list.getByRole('button', { name: /^Moderators/ })).toBeVisible();

  // Membres du rôle : l'ajout, le décompte, le retrait.
  await settings(p).getByRole('tab', { name: 'Members' }).click();
  await expect(settings(p).getByText('Nobody has this role yet.')).toBeVisible();
  await settings(p).getByLabel('Add a member to this role').selectOption({ label: `${sam.name} (@${sam.name})` });
  await settings(p).getByRole('button', { name: 'Add', exact: true }).click();
  const holders = settings(p).getByRole('list', { name: 'Members with this role' });
  await expect(holders.getByText(sam.name).first()).toBeVisible();
  await expect(list.getByRole('button', { name: /^Moderators.*1 member/ })).toBeVisible();
  await expect(list.getByRole('button', { name: /^@everyone.*2 members/ })).toBeVisible();
  await expect(settings(p).getByRole('option', { name: 'Pick a member' })).toBeAttached();
  await noFrench(p);
  await a11y(p);
  await holders.getByRole('button', { name: `Remove ${sam.name} from this role` }).click();
  await expect(settings(p).getByText('Nobody has this role yet.')).toBeVisible();

  // Réordonner : le nom accessible dit quel rôle, la zone d'état le confirme.
  const up = list.getByRole('button', { name: 'Move role Moderators up' });
  const down = list.getByRole('button', { name: 'Move role Moderators down' });
  await expect(up).toBeAttached();
  await ((await up.isEnabled()) ? up : down).click();
  await expect(said(p, 'Role order changed.')).toBeVisible();

  // @everyone : ni nom ni couleur à changer.
  await list.getByRole('button', { name: /^@everyone/ }).click();
  await expect(settings(p).getByRole('heading', { name: '@everyone', level: 3 })).toBeVisible();
  await expect(settings(p).getByText('Everyone has this role automatically, so it has no name to change and no color.')).toBeVisible();
  await noFrench(p);
});

// ───────── Salons ─────────

test('I18N-G3 Salons : liste, création, rangement, réglages d’un salon, droits par rôle ; rien de français', async ({ signedIn, api }) => {
  const a = await signedIn('en-US');
  const as = await api(a.token);
  const sam = await signedIn('en-US');
  const { gid, cid } = await guildOf(as, 'Docks', 'jeux');
  await joinWith(api, a, cid, sam);
  const ideas = await channel(as, gid, 'ideas');
  await role(as, gid, 'Helpers');
  const p = a.page;

  await open(p, gid, 'channels', 'Channels');
  await expect(settings(p).getByText('Channels are shown in this order.')).toBeVisible();
  await expect(settings(p).getByRole('button', { name: 'Create a channel' })).toBeVisible();
  await expect(settings(p).getByRole('button', { name: 'Create a category' })).toBeVisible();
  const list = settings(p).getByRole('list', { name: 'Channels and categories, in display order' });
  await expect(list.getByRole('button', { name: 'Move channel ideas up' })).toBeAttached();
  await expect(list.getByRole('link', { name: 'Settings for channel ideas' })).toBeVisible();
  await expect(list.getByRole('button', { name: /^Move category .* up$/ }).first()).toBeAttached();
  await noFrench(p);
  await a11y(p);

  // La fenêtre de création : types, aide, espace réservé, erreur de nom.
  await settings(p).getByRole('button', { name: 'Create a channel' }).click();
  // Le nom de la fenêtre suit le type choisi (« Create a channel » / « Create a category ») : on la retrouve par son groupe « Type ».
  const dialog = p.getByRole('dialog').filter({ has: p.getByRole('group', { name: 'Type' }) });
  await expect(dialog.getByRole('radio')).toHaveCount(3);
  await expect(dialog.getByRole('radio').evaluateAll((els) => els.map((e) => e.closest('label')?.textContent))).resolves.toEqual([
    'Text channelFor writing messages and sharing images and files.',
    'Voice channelFor talking and seeing each other live.',
    'CategoryFor grouping several channels under one heading.',
  ]);
  const field = dialog.getByLabel('Channel name');
  await expect(field).toHaveAttribute('placeholder', 'new-channel');
  await expect(dialog.getByText('Capital letters and spaces are replaced: "Test channel" becomes #test-channel.')).toBeVisible();
  await expect(dialog.getByLabel('Category', { exact: true })).toBeVisible();
  await expect(dialog.getByRole('option', { name: 'No category' })).toBeAttached();
  await dialog.getByRole('button', { name: 'Create channel' }).click();
  await expect(dialog.getByRole('alert')).toHaveText('Give the channel a name.');
  await dialog.getByRole('radio', { name: /^Voice channel/ }).check();
  await expect(field).toHaveAttribute('placeholder', 'Voice channel');
  await expect(dialog.getByText('The name can contain spaces and capital letters.')).toBeVisible();
  await dialog.getByRole('radio', { name: /^Category/ }).check();
  await expect(dialog.getByLabel('Category name')).toHaveAttribute('placeholder', 'New category');
  await expect(dialog.getByRole('heading', { name: 'Create a category' })).toBeVisible();
  await dialog.getByRole('button', { name: 'Create category' }).click();
  await expect(dialog.getByRole('alert')).toHaveText('Give the category a name.');
  await noFrench(p);
  await a11y(p);
  await dialog.getByRole('radio', { name: /^Text channel/ }).check();
  await dialog.getByLabel('Channel name').fill('Test channel');
  await dialog.getByRole('button', { name: 'Create channel' }).click();
  await expect(dialog).toHaveCount(0);
  await expect(list.getByRole('link', { name: 'Settings for channel test-channel' })).toBeVisible();
  await list.getByRole('button', { name: 'Move channel test-channel up' }).click();
  await expect(said(p, '"test-channel" moved.')).toBeVisible();

  // Réglages d'un salon : aperçu, mode lent, privé, droits.
  await list.getByRole('link', { name: 'Settings for channel ideas' }).click();
  const chan = p.getByRole('dialog', { name: 'Channel settings' });
  await expect(chan.getByRole('navigation', { name: 'Channel settings sections' }).getByRole('link')).toHaveText(['Overview', 'Permissions']);
  await expect(chan.getByRole('heading', { name: 'Overview', level: 2 })).toBeVisible();
  await expect(chan.getByLabel('Channel name')).toHaveValue('ideas');
  await expect(chan.getByText('Capital letters and spaces are replaced: "Test channel" becomes #test-channel.')).toBeVisible();
  await expect(chan.getByLabel('Channel topic')).toBeVisible();
  await expect(chan.getByText('One line about what’s discussed here'.replace('’', "'"), { exact: false })).toBeVisible();
  const slow = chan.getByLabel('Slow mode');
  expect(await slow.locator('option').allTextContents()).toEqual(['Off', '5 seconds', '10 seconds', '30 seconds', '1 minute', '5 minutes', '15 minutes', '1 hour']);
  await expect(chan.getByRole('switch', { name: 'Private channel' })).toBeVisible();
  await expect(chan.getByText('Only the roles and members you add under "Permissions" can see this channel.')).toBeVisible();
  await expect(chan.getByRole('button', { name: 'Delete channel' })).toBeVisible();
  await noFrench(p);
  await a11y(p);
  await slow.selectOption({ label: '30 seconds' });
  await chan.getByLabel('Channel topic').fill('Plans and sketches');
  const bar = chan.getByRole('region', { name: 'Channel changes' });
  await expect(bar.getByText('Unsaved changes')).toBeVisible();
  await bar.getByRole('button', { name: 'Save' }).click();
  await expect(said(p, 'Changes saved.')).toBeAttached();
  await expect(bar).toHaveCount(0);
  const saved = (await (await as.get(`${API}/guilds/${gid}`)).json()).channels.find((c: { id: string }) => c.id === ideas.id);
  expect(saved.slowmode_s).toBe(30);

  // Les droits : le rôle ajouté, les trois états, l'enregistrement.
  await chan.getByRole('link', { name: 'Permissions' }).click();
  await expect(chan.getByRole('heading', { name: 'Permissions', level: 2 })).toBeVisible();
  await expect(chan.getByText('For each permission: "Allow" or "Deny" is added to the server\'s permissions in this channel')).toBeVisible();
  await expect(chan.getByRole('list', { name: 'Roles and members of this channel' }).getByRole('button', { name: /^@everyone/ })).toBeVisible();
  await expect(chan.getByLabel('Add a role or a member')).toBeVisible();
  await expect(chan.getByRole('option', { name: 'Pick from the list' })).toBeAttached();
  await expect(chan.locator('optgroup[label="Roles"]')).toBeAttached();
  await expect(chan.locator('optgroup[label="Members"]')).toBeAttached();
  await chan.getByLabel('Add a role or a member').selectOption({ label: 'Helpers' });
  await chan.getByRole('button', { name: 'Add', exact: true }).click();
  await expect(chan.getByRole('heading', { name: 'Helpers', level: 3 })).toBeVisible();
  await expect(chan.locator('.app-perms legend')).toHaveText(['General', 'Text Channels']);
  const send = chan.getByRole('radiogroup', { name: /Send Messages/ }).first();
  await expect(send.getByRole('radio')).toHaveCount(3);
  await expect(send.locator('label').evaluateAll((els) => els.map((e) => e.getAttribute('title')))).resolves.toEqual(['Allow', 'Inherit', 'Deny']);
  await send.getByRole('radio', { name: 'Allow' }).check({ force: true });
  await expect(chan.getByRole('button', { name: 'Reset everything to "Inherit"' })).toBeEnabled();
  const permBar = chan.getByRole('region', { name: 'Permission changes' });
  await expect(permBar.getByText('Unsaved changes')).toBeVisible();
  await noFrench(p);
  await a11y(p);
  await permBar.getByRole('button', { name: 'Save' }).click();
  await expect(said(p, 'Permissions saved.')).toBeAttached();
  await expect(chan.getByRole('button', { name: /^Helpers.*configured/ })).toBeVisible();

  // Le mode lent se lit dans le salon, dans la langue : « 30 seconds ».
  // (le propriétaire n'attend jamais : un autre membre le lit.)
  await sam.page.goto(`/channels/${gid}/${ideas.id}`);
  await expect(sam.page.getByText('Slow mode: 30 seconds between messages')).toBeVisible();
});

// ───────── Emojis, Invitations, Bannis ─────────

test('I18N-G4 Emojis : liste, ajout (animation), nom invalide ou pris, renommer, supprimer, sans le droit ; rien de français', async ({ signedIn, api }) => {
  const a = await signedIn('en-US');
  const sam = await signedIn('en-US');
  const as = await api(a.token);
  const { gid, cid } = await guildOf(as, 'Reef');
  await joinWith(api, a, cid, sam);
  const emoji = await (await as.post(`${API}/guilds/${gid}/emojis?name=octopus`, { data: png(32, 32), headers: { 'content-type': 'application/octet-stream' } })).json();
  expect(emoji.name).toBe('octopus');
  const p = a.page;

  await open(p, gid, 'emojis', 'Emojis');
  const s = settings(p);
  await expect(s.getByText('Add images to this server: members type them in their messages with')).toBeVisible();
  await expect(s.locator('p.app-hint code')).toHaveText(':nom:');
  await expect(s.getByText('1 / 50 slots')).toBeVisible();
  await expect(s.getByRole('button', { name: 'Add an emoji' })).toBeVisible();
  await expect(s.getByText('PNG, JPEG, WebP or GIF. A still image is cropped to a square and shrunk to 128 px;')).toBeVisible();
  const list = s.getByRole('list', { name: 'Server emojis' });
  await expect(list.getByRole('listitem')).toContainText([`:octopus:added by ${a.name}`]);
  await noFrench(p);
  await a11y(p);

  // Ajouter une animation : elle passe directement à la fenêtre d'ajout.
  await s.locator('#emoji-file').setInputFiles({ name: 'party-parrot.gif', mimeType: 'image/gif', buffer: animatedGif(2, 8) });
  const add = p.getByRole('dialog', { name: 'Add an emoji' });
  await expect(add).toBeVisible();
  await expect(add.getByLabel('Name')).toHaveValue('party_parrot');
  await expect(add.getByText('In a message:')).toBeVisible();
  await expect(add.locator('code')).toHaveText(':party_parrot:');
  await expect(add.getByRole('img', { name: 'Emoji preview' }).first()).toBeAttached();
  await add.getByLabel('Name').fill('a');
  await add.getByRole('button', { name: 'Add emoji' }).click();
  await expect(add.getByRole('alert')).toHaveText('The name must be 2 to 32 characters: letters without accents, digits and _.');
  await add.getByLabel('Name').fill('Octopus');
  await add.getByRole('button', { name: 'Add emoji' }).click();
  await expect(add.getByRole('alert')).toHaveText('This name is already taken in this server.');
  await noFrench(p);
  await a11y(p);
  await add.getByRole('button', { name: 'Cancel' }).click();
  await expect(add).toHaveCount(0);

  // Renommer sur place.
  await s.getByRole('button', { name: 'Actions for :octopus:' }).click();
  await p.getByRole('button', { name: 'Rename' }).click();
  const rename = s.getByLabel('New name for :octopus:');
  await expect(rename).toBeFocused();
  await expect(s.getByText('Enter to save, Esc to cancel.')).toBeVisible();
  await rename.fill('!');
  await rename.press('Enter');
  await expect(s.getByRole('alert')).toHaveText('The name must be 2 to 32 characters: letters without accents, digits and _.');
  await rename.press('Escape');
  await expect(rename).toHaveCount(0);

  // Supprimer : la fenêtre dit ce qui reste, puis l'état vide.
  await s.getByRole('button', { name: 'Actions for :octopus:' }).click();
  await p.getByRole('button', { name: 'Delete' }).click();
  const del = p.getByRole('alertdialog', { name: 'Delete :octopus:?' });
  await expect(del.getByText("The emoji disappears from the server and the picker.")).toBeVisible();
  await noFrench(p);
  await a11y(p);
  await del.getByRole('button', { name: 'Delete emoji' }).click();
  await expect(said(p, 'Emoji :octopus: deleted.')).toBeVisible();
  await expect(s.getByText('No emojis yet')).toBeVisible();
  await expect(s.getByText("The server's emojis will show up here, and in its members' emoji picker.")).toBeVisible();
  await expect(s.getByRole('button', { name: 'Add an emoji' })).toBeVisible();

  // Sans « Gérer les émojis » (mais avec « Gérer les salons » : la rubrique est visible), la liste se lit sans se changer.
  await as.post(`${API}/guilds/${gid}/emojis?name=squid`, { data: png(32, 32), headers: { 'content-type': 'application/octet-stream' } });
  await give(as, gid, sam, await role(as, gid, 'Builders', MANAGE_CHANNELS));
  await open(sam.page, gid, 'emojis', 'Emojis');
  await expect(settings(sam.page).getByText('You need the "Manage Emojis" permission to add, rename or remove an emoji.')).toBeVisible();
  await expect(settings(sam.page).getByRole('button', { name: 'Add an emoji' })).toHaveCount(0);
  await noFrench(sam.page);
  await a11y(sam.page);
});

test('I18N-G5 Invitations et Bannis : durées, utilisations, états, copie, révocation ; liste des bannis ; rien de français', async ({ signedIn, api }) => {
  const a = await signedIn('en-US');
  const sam = await signedIn('en-US');
  const lea = await register(api, 'lea');
  const moussa = await register(api, 'moussa');
  const as = await api(a.token);
  const { gid, cid } = await guildOf(as, 'Pier');
  await joinWith(api, a, cid, sam);
  const p = a.page;

  // Une invitation à usage unique, épuisée par lea.
  const single = await (await as.post(`${API}/channels/${cid}/invites`, { data: { max_uses: 1 } })).json();
  expect((await (await api(lea.token)).post(`${API}/invites/${single.code}`)).status()).toBe(200);
  await open(p, gid, 'invites', 'Invites');
  const s = settings(p);
  await expect(s.getByText('Expired or used-up invites stay listed for a week.')).toBeVisible();
  const form = s.getByRole('form', { name: 'New invite' });
  expect(await form.getByLabel('Expires after').locator('option').allTextContents()).toEqual(['30 minutes', '1 hour', '6 hours', '12 hours', '1 day', '7 days', 'Never']);
  expect(await form.getByLabel('Number of uses').locator('option').allTextContents()).toEqual(['Unlimited', '1 use', '5 uses', '10 uses', '25 uses', '50 uses', '100 uses']);
  const rows = s.getByRole('list', { name: 'Invites' }).getByRole('listitem');
  const used = rows.filter({ hasText: single.code });
  await expect(used).toContainText('Used up');
  await expect(used).toContainText('no uses left');
  await expect(used).toContainText(`Created by ${a.name}`);
  await expect(used.getByText('1 use out of 1')).toBeAttached();
  await expect(used.getByRole('button', { name: /^Copy/ })).toHaveCount(0);
  await noFrench(p);
  await a11y(p);

  // Créer : durée et nombre au choix, la zone d'état dit le code.
  await form.getByLabel('Expires after').selectOption({ label: '1 day' });
  await form.getByLabel('Number of uses').selectOption({ label: '5 uses' });
  await form.getByRole('button', { name: 'Create invite' }).click();
  const fresh = rows.filter({ hasText: 'expires in' });
  await expect(fresh).toHaveCount(1);
  const code = (await fresh.locator('code').textContent())!;
  await expect(said(p, `Invite ${code} created.`)).toBeVisible();
  await expect(fresh).toContainText('expires in 1 d');
  await expect(fresh.getByText('0 uses out of 5')).toBeAttached();
  await expect(fresh.getByRole('button', { name: `Copy the link to invite ${code}` })).toBeVisible();
  await fresh.getByRole('button', { name: `Copy the link to invite ${code}` }).click();
  await expect(fresh.getByText(/^(Copied|Couldn't copy)$/)).toBeVisible();
  await fresh.getByRole('button', { name: `More actions for invite ${code}` }).click();
  await p.getByRole('button', { name: 'Revoke' }).click();
  await expect(said(p, `Invite ${code} revoked.`)).toBeVisible();
  await expect(rows.filter({ hasText: code })).toHaveCount(0);
  await noFrench(p);

  // Qui peut seulement créer des invitations ne voit que les siennes.
  await open(sam.page, gid, 'invites', 'Invites');
  await expect(nav(sam.page)).toHaveText(['Invites']);
  await expect(settings(sam.page).getByText('You only see the invites you created.')).toBeVisible();
  await expect(settings(sam.page).getByText('No invites')).toBeVisible();
  await expect(settings(sam.page).getByText('The ones you create show up here.')).toBeVisible();
  await noFrench(sam.page);
  await a11y(sam.page);

  // Bannis : avec et sans motif, puis la liste vide.
  await joinWith(api, a, cid, moussa);
  expect((await as.put(`${API}/guilds/${gid}/bans/${lea.id}`, { data: { reason: 'spam' } })).status()).toBe(200);
  expect((await as.put(`${API}/guilds/${gid}/bans/${moussa.id}`, { data: {} })).status()).toBe(200);
  await open(p, gid, 'bans', 'Bans');
  await expect(s.getByText("A banned person can't rejoin the server with an invite.")).toBeVisible();
  const banned = s.getByRole('list', { name: 'Banned people' }).getByRole('listitem');
  await expect(banned).toHaveCount(2);
  await expect(banned.filter({ hasText: lea.name })).toContainText('Reason: spam');
  await expect(banned.filter({ hasText: moussa.name })).toContainText('No reason given');
  await expect(banned.filter({ hasText: lea.name }).getByRole('button', { name: `Unban ${lea.name}` })).toHaveText('Unban');
  await noFrench(p);
  await a11y(p);
  await banned.filter({ hasText: lea.name }).getByRole('button', { name: 'Unban' }).click();
  await banned.filter({ hasText: moussa.name }).getByRole('button', { name: 'Unban' }).click();
  await expect(s.getByText('Nobody is banned')).toBeVisible();
  await expect(s.getByText('Bans will show up here.')).toBeVisible();
  await a11y(p);
});

// ───────── Journal d'audit ─────────

test('I18N-G6 Journal d’audit : chaque action est une phrase anglaise entière, avec motif, précisions, avant / après et filtres', async ({ signedIn, api }) => {
  test.setTimeout(90_000);
  const a = await signedIn('en-US');
  const sam = await register(api, 'sam');
  const lea = await register(api, 'lea');
  const moussa = await register(api, 'moussa');
  const noa = await register(api, 'noa');
  const ivy = await register(api, 'ivy');
  const as = await api(a.token);
  const { gid, cid, general } = await guildOf(as, 'Galleon');
  for (const u of [sam, lea, moussa, noa, ivy]) await joinWith(api, a, cid, u);

  const mods = await role(as, gid, 'Mods');
  await give(as, gid, sam, mods);
  expect((await as.patch(`${API}/guilds/${gid}/roles/${mods.id}`, { data: { name: 'Moderators', color: 6, permissions: MANAGE_MESSAGES | BAN } })).status()).toBe(200);
  // Un second auteur dans le journal : sam, modérateur, bannit ivy (avant d'être exclu lui-même).
  expect((await (await api(sam.token)).put(`${API}/guilds/${gid}/bans/${ivy.id}`, { data: { reason: 'trolling' } })).status()).toBe(200);
  const plans = await channel(as, gid, 'plans');
  expect((await as.patch(`${API}/channels/${plans.id}`, { data: { name: 'ideas', slowmode_s: 30 } })).status()).toBe(200);
  const cafe = await channel(as, gid, 'Cafe', 'voice');
  await channel(as, gid, 'Archive', 'category');
  expect((await as.delete(`${API}/channels/${cafe.id}`)).status()).toBe(204);
  const emoji = await (await as.post(`${API}/guilds/${gid}/emojis?name=octopus`, { data: png(32, 32), headers: { 'content-type': 'application/octet-stream' } })).json();
  expect((await as.patch(`${API}/guilds/${gid}/emojis/${emoji.id}`, { data: { name: 'squid' } })).status()).toBe(200);
  expect((await as.put(`${API}/guilds/${gid}/members/${sam.id}/timeout`, { data: { seconds: 3600, reason: 'coffee break' } })).status()).toBe(200);
  expect((await as.delete(`${API}/guilds/${gid}/members/${moussa.id}?reason=off%20topic`)).status()).toBe(204);
  expect((await as.put(`${API}/guilds/${gid}/bans/${lea.id}`, { data: { reason: 'spam' } })).status()).toBe(200);
  expect((await as.delete(`${API}/guilds/${gid}/bans/${lea.id}`)).status()).toBe(204);
  // Droits propres à un salon : un rôle, un membre, un rôle disparu, puis retirés.
  expect((await as.put(`${API}/channels/${plans.id}/permissions/${mods.id}`, { data: { kind: 'role', allow: SEND_MESSAGES, deny: MANAGE_MESSAGES } })).status()).toBe(200);
  expect((await as.put(`${API}/channels/${plans.id}/permissions/${sam.id}`, { data: { kind: 'member', allow: SEND_MESSAGES, deny: 0 } })).status()).toBe(200);
  const gone = await role(as, gid, 'Ephemeral');
  expect((await as.put(`${API}/channels/${plans.id}/permissions/${gone.id}`, { data: { kind: 'role', allow: 0, deny: MANAGE_MESSAGES } })).status()).toBe(200);
  expect((await as.delete(`${API}/guilds/${gid}/roles/${gone.id}`)).status()).toBe(204);
  expect((await as.put(`${API}/channels/${plans.id}/permissions/${mods.id}`, { data: { kind: 'role', allow: 0, deny: 0 } })).status()).toBe(200);
  // Invitations, messages, serveur.
  const inv = await (await as.post(`${API}/channels/${cid}/invites`, { data: { max_age: 3600, max_uses: 1 } })).json();
  expect((await as.delete(`${API}/invites/${inv.code}`)).status()).toBeLessThan(300);
  const posted = await (await api(noa.token)).post(`${API}/channels/${cid}/messages`, { data: { content: 'hello there' } });
  expect(posted.status(), await posted.text()).toBe(201);
  const said1 = await posted.json();
  expect((await as.put(`${API}/channels/${cid}/pins/${said1.id}`)).status()).toBeLessThan(300);
  expect((await as.delete(`${API}/channels/${cid}/pins/${said1.id}`)).status()).toBeLessThan(300);
  expect((await as.delete(`${API}/channels/${cid}/messages/${said1.id}`)).status()).toBeLessThan(300);
  expect((await as.put(`${API}/guilds/${gid}/icon`, { headers: { 'content-type': 'image/png' }, data: png(32, 32) })).status()).toBe(200);
  expect((await as.delete(`${API}/guilds/${gid}/icon`)).status()).toBe(200);
  expect((await as.patch(`${API}/guilds/${gid}`, { data: { name: 'Galleon II' } })).status()).toBe(200);

  const p = a.page;
  await open(p, gid, 'audit', 'Audit log');
  const s = settings(p);
  await expect(s.getByText('Management and moderation actions on this server, newest first.')).toBeVisible();
  const rows = s.getByRole('list', { name: 'Audit log events' }).getByRole('listitem');
  const row = (text: string | RegExp) => rows.filter({ hasText: text });
  const me = a.name;
  await expect(rows.first()).toContainText(`${me} renamed the server "Galleon" to "Galleon II"`);
  await expect(s.getByRole('heading', { name: 'Today', level: 3 })).toBeVisible();
  await expect(s.getByRole('status').filter({ hasText: / events$/ })).toHaveText(/^\d+ events$/);

  const sentences = [
    `${me} added the role Mods to ${sam.name}`,
    `${me} created the role Moderators`,
    `${me} edited the role Moderators`,
    `${me} deleted the role Ephemeral`,
    `${me} created the channel #ideas`,
    `${me} created the role Ephemeral`,
    `${me} edited the channel #ideas`,
    `${me} created the voice channel Cafe`,
    `${me} deleted the voice channel Cafe`,
    `${me} created the category Archive`,
    `${me} added the emoji :squid:`,
    `${me} renamed the emoji :octopus: to :squid:`,
    `${me} timed out ${sam.name} for 1 h`,
    `${me} kicked ${moussa.name}`,
    `${me} banned ${lea.name}`,
    `${sam.name} banned ${ivy.name}`,
    `${me} unbanned ${lea.name}`,
    `${me} changed the permissions of the role Moderators in the channel #ideas`,
    `${me} changed the permissions of ${sam.name} in the channel #ideas`,
    `${me} changed the permissions of the role (deleted) in the channel #ideas`,
    `${me} removed the permission overrides for the role Moderators in the channel #ideas`,
    `${me} created an invite for the channel #${general}`,
    `${me} revoked an invite created by ${me}`,
    `${me} deleted a message by ${noa.name} in the channel #${general}`,
    `${me} pinned a message in #${general}`,
    `${me} unpinned a message in #${general}`,
    `${me} changed the server icon`,
    `${me} removed the server icon`,
  ];
  for (const sentence of sentences) await expect(row(sentence).first(), sentence).toBeVisible();
  // Motifs, précisions : « Reason: », « Valid for … », « Allowed: … ».
  await expect(row('timed out')).toContainText('Reason: coffee break');
  await expect(row('kicked')).toContainText('Reason: off topic');
  await expect(row(`${me} banned ${lea.name}`)).toContainText('Reason: spam');
  await expect(row('created an invite').filter({ hasText: 'Valid for 1 h, at most 1 use' })).toHaveCount(1);
  await expect(row('the role Moderators in the channel').first()).toBeVisible();
  await expect(row(`changed the permissions of ${sam.name}`)).toContainText('Allowed: Send Messages');
  await expect(row('the role (deleted)')).toContainText('Denied: Manage Messages');
  await noFrench(p);
  await a11y(p);

  // L'heure : relative, avec la date complète en infobulle et pour un lecteur d'écran.
  const when = rows.first().locator('time');
  await expect(when.locator('[aria-hidden="true"]')).toHaveText(/^(just now|\d+ min ago)$/);
  const title = await when.getAttribute('title');
  expect(title).toMatch(new RegExp(`${new Date().getFullYear()} at \\d{1,2}:\\d{2}`));

  // Avant / après : le salon renommé (nom, mode lent), le rôle (nom, couleur, droits).
  const renamed = row(`edited the channel #ideas`);
  await expect(renamed.getByText('Before').first()).toBeHidden();
  await renamed.getByRole('button', { name: 'View changes' }).click();
  await expect(renamed.getByRole('button', { name: 'Hide changes' })).toHaveAttribute('aria-expanded', 'true');
  for (const text of ['Name', 'plans', 'ideas', 'Slow mode', '30 seconds', 'Off', 'Before', 'After']) await expect(renamed.getByText(text, { exact: true }).first()).toBeVisible();
  const edited = row('edited the role Moderators');
  await edited.getByRole('button', { name: 'View changes' }).click();
  for (const text of ['Color', 'None', 'Blue', 'Permissions', 'Added', 'Ban Members, Manage Messages']) await expect(edited.getByText(text, { exact: true }).first()).toBeVisible();
  await a11y(p);
  await renamed.getByRole('button', { name: 'Hide changes' }).click();
  await expect(renamed.getByText('Before').first()).toBeHidden();

  // Filtres : types d'action, personne, « Reset », « Refresh », état vide.
  const action = s.getByLabel('Action', { exact: true });
  const member = s.getByLabel('Member', { exact: true });
  await expect(s.getByRole('search', { name: 'Filter the audit log' })).toBeVisible();
  expect(await action.locator('option').allTextContents()).toEqual(['All', 'Channels', 'Roles', 'Members', 'Invites', 'Server', 'Emojis']);
  await expect.poll(() => member.locator('option').first().textContent()).toBe('All');
  await expect(s.getByRole('button', { name: 'Reset' })).toHaveCount(0);
  await action.selectOption('emojis');
  await expect(rows).toHaveCount(2);
  await expect(s.getByRole('status').filter({ hasText: / events$/ })).toHaveText('2 events');
  await member.selectOption({ label: sam.name });
  await expect(s.locator('.k-empty')).toContainText('No matching events');
  await expect(s.locator('.k-empty')).toContainText('Try another type of action or another member.');
  await expect(s.getByRole('button', { name: 'Reset' })).toHaveCount(2);
  await a11y(p);
  await s.locator('.k-empty').getByRole('button', { name: 'Reset' }).click();
  await expect(action).toBeFocused();
  await s.getByRole('button', { name: 'Refresh' }).click();
  await expect(rows.first()).toContainText('renamed the server');
  await noFrench(p);
});

// ───────── Propriété, exclusion, fiche d'un membre ─────────

test('I18N-G7 Propriété, suppression du serveur, exclusion temporaire et fiche d’un membre ; rien de français', async ({ signedIn, api }) => {
  test.setTimeout(90_000);
  const a = await signedIn('en-US');
  const sam = await register(api, 'sam');
  const as = await api(a.token);
  const { gid, cid } = await guildOf(as, 'Harbor', 'amis');
  await joinWith(api, a, cid, sam);
  await give(as, gid, sam, await role(as, gid, 'Helpers'));
  const p = a.page;

  await open(p, gid, 'owner', 'Ownership');
  const s = settings(p);
  await expect(s.getByText('The owner has every permission and can\'t be sanctioned.')).toBeVisible();
  await expect(s.getByLabel('New owner')).toBeVisible();
  await expect(s.getByRole('option', { name: 'Pick a member' })).toBeAttached();
  await expect(s.getByRole('button', { name: 'Transfer…' })).toBeDisabled();
  await expect(s.getByRole('heading', { name: 'Danger zone', level: 3 })).toBeVisible();
  await expect(s.getByText("Deleting the server erases its channels, messages, roles and invites, for everyone.")).toBeVisible();
  await noFrench(p);
  await a11y(p);
  await s.getByRole('button', { name: 'Delete server…' }).click();
  const del = p.getByRole('alertdialog', { name: 'Delete "Harbor"?' });
  await expect(del.getByText("All of this disappears for everyone, immediately:")).toBeVisible();
  await expect(del.getByRole('listitem')).toHaveText(['the channels, their messages and their files,', 'the roles and permissions,', 'the invites.']);
  await expect(del.getByText("This can't be undone.")).toBeVisible();
  await expect(del.getByText('People in voice are disconnected.')).toBeVisible();
  await expect(del.getByText('To confirm, type: Harbor')).toBeVisible();
  await expect(del.getByLabel('Type the server name')).toBeVisible();
  await expect(del.getByRole('button', { name: 'Delete server' })).toBeDisabled();
  await del.getByLabel('Type the server name').fill('Harbor');
  await expect(del.getByRole('button', { name: 'Delete server' })).toBeEnabled();
  await noFrench(p);
  await a11y(p);
  await del.getByRole('button', { name: 'Cancel' }).click();
  await expect(del).toHaveCount(0);
  await p.keyboard.press('Escape');
  await expect(settings(p)).toHaveCount(0);

  // Exclusion temporaire (menu ⋯ d'un membre) : la fenêtre, les durées, la raison.
  await p.goto(`/channels/${gid}/${cid}`);
  await p.getByRole('button', { name: `More actions for ${sam.name}` }).click();
  await p.getByRole('button', { name: 'Time out…' }).click();
  const to = p.getByRole('dialog', { name: `Time out ${sam.name}` });
  await expect(to).toBeVisible();
  await expect(to.getByText('During this time, this person can read the server')).toBeVisible();
  await expect(to.getByRole('radiogroup', { name: 'Duration' }).getByRole('radio')).toHaveCount(6);
  await expect(to.getByRole('radiogroup', { name: 'Duration' }).locator('label')).toHaveText(['60 seconds', '5 minutes', '10 minutes', '1 hour', '1 day', '1 week']);
  await expect(to.getByLabel('Reason (optional)')).toBeVisible();
  await noFrench(p);
  await a11y(p);
  await to.getByLabel('10 minutes').check();
  await to.getByLabel('Reason (optional)').fill('cooling off');
  await to.getByRole('button', { name: 'Time out', exact: true }).click();
  await expect(to).toHaveCount(0);
  await expect.poll(async () => (await (await as.get(`${API}/guilds/${gid}`)).json()).members.find((m: { user: { id: string } }) => m.user.id === sam.id)?.timeout_until ?? null).not.toBeNull();

  // La fiche d'un membre : ses rôles, cochés ou non.
  await p.getByRole('button', { name: `More actions for ${sam.name}` }).click();
  await expect(p.getByRole('button', { name: 'Remove timeout' })).toBeVisible();
  await p.getByRole('button', { name: 'Profile and roles' }).click();
  const card = p.getByRole('dialog').filter({ has: p.getByRole('heading', { name: 'Roles', level: 3 }) });
  await expect(card).toBeVisible();
  await expect(card.getByText(`@${sam.name} · member since`)).toBeVisible();
  await expect(card.getByLabel('Helpers')).toBeChecked();
  await expect(card.getByRole('button', { name: 'Close' })).toBeVisible();
  await noFrench(p);
  await a11y(p);
  await card.getByRole('button', { name: 'Close' }).click();
  await expect(card).toHaveCount(0);
});

// ───────── 390 px ─────────

test('I18N-G8 sur 390 px : création, rôles, salons, emojis, invitations, bannis, journal et propriété tiennent sans débordement, axe propre', async ({ signedIn, api }) => {
  test.setTimeout(120_000);
  const a = await signedIn('en-US', { width: 390, height: 844 });
  const sam = await register(api, 'sam');
  const as = await api(a.token);
  const { gid, cid } = await guildOf(as, 'Harbor', 'etudes');
  await joinWith(api, a, cid, sam);
  await channel(as, gid, 'ideas');
  await role(as, gid, 'Helpers');
  await as.post(`${API}/guilds/${gid}/emojis?name=octopus`, { data: png(32, 32), headers: { 'content-type': 'application/octet-stream' } });
  expect((await as.put(`${API}/guilds/${gid}/bans/${sam.id}`, { data: { reason: 'spam' } })).status()).toBe(200);
  const p = a.page;

  await p.goto('/guilds/new');
  await expect(p.getByRole('heading', { name: 'Create your server', level: 1 })).toBeVisible();
  await noFrench(p, true);
  await noScroll(p);
  await a11y(p);

  for (const [section, heading] of [
    ['overview', 'Overview'],
    ['roles', 'Roles'],
    ['channels', 'Channels'],
    ['emojis', 'Emojis'],
    ['invites', 'Invites'],
    ['bans', 'Bans'],
    ['audit', 'Audit log'],
    ['owner', 'Ownership'],
  ]) {
    await open(p, gid, section, heading);
    await expect(nav(p).filter({ hasText: heading })).toHaveAttribute('aria-current', 'page');
    await noFrench(p);
    await noScroll(p);
    await a11y(p);
  }
  await open(p, gid, 'roles', 'Roles');
  await settings(p).getByRole('tab', { name: 'Permissions' }).click();
  await expect(settings(p).getByRole('switch', { name: 'Manage Messages' })).toBeVisible();
  await noFrench(p);
  await noScroll(p);
  await a11y(p);
  await open(p, gid, 'audit', 'Audit log');
  await expect(settings(p).getByRole('list', { name: 'Audit log events' }).getByRole('listitem').first()).toContainText('banned');
  await settings(p).getByRole('button', { name: 'Create a channel' }).count();
});
