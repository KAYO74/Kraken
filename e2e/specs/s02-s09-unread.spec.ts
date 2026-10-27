// S02, S09, S25 : non-lus et mentions en direct (rail, salons, MP, accueil, titre de l'onglet),
// ligne « Nouveaux messages », lecture synchronisée entre deux onglets, sourdine et niveaux par
// salon, menu au clavier, notifications sur le bureau.
import { test, expect, a11y, createGuild, say, messages, API, type User } from '../fixtures';
import type { BrowserContext, Page } from '@playwright/test';

/** `a` crée un serveur, `b` le rejoint par invitation ; les deux sont sur #général. */
async function together(a: User, b: User, name: string): Promise<[string, string]> {
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

/** Un salon textuel de plus, créé par le propriétaire. */
async function channel(owner: User, gid: string, name: string): Promise<string> {
  const res = await owner.context.request.post(`${API}/guilds/${gid}/channels`, {
    data: { name, kind: 'text', parent_id: null },
    headers: { Authorization: `Session ${owner.token}` },
  });
  expect(res.status()).toBe(201);
  return (await res.json()).id;
}

/** Envoie un message par l'API, comme un autre appareil. */
async function post(u: User, cid: string, content: string) {
  const res = await u.context.request.post(`${API}/channels/${cid}/messages`, {
    data: { content },
    headers: { Authorization: `Session ${u.token}` },
  });
  expect(res.status()).toBe(201);
}

const rail = (p: Page) => p.getByRole('navigation', { name: 'Serveurs' });
const sidebar = (p: Page) => p.getByRole('navigation', { name: 'Salons' });
const server = (p: Page, name: string) => rail(p).getByRole('link', { name: `Serveur ${name}` });
const chan = (p: Page, name: string) => sidebar(p).getByRole('link', { name: new RegExp(`^${name}`) });
/** Nom accessible : le libellé, puis « , non lu », « , 2 mentions »… (texte masqué, espaces libres). */
const said = (...parts: string[]) => new RegExp(parts.map((p) => p.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('\\s*, ') + '$');

test('S02-H1 non-lu et mention en direct, ligne « Nouveaux messages », lecture partagée par deux onglets', async ({ user }) => {
  const a = await user('ecr');
  const b = await user('lec');
  const [gid, general] = await together(a, b, 'Récif');
  const calm = await channel(a, gid, 'calme');
  // b lit un premier message dans #général, puis part dans #calme.
  await say(a.page, 'général', 'Bienvenue au récif');
  await expect(messages(b.page).filter({ hasText: 'Bienvenue au récif' })).toHaveCount(1);
  await chan(b.page, 'calme').click();
  await expect(b.page.getByRole('heading', { name: 'calme', level: 1 })).toBeVisible();
  await expect(chan(b.page, 'général')).not.toHaveAttribute('data-unread');
  await expect(b.page).toHaveTitle('KRAKEN');

  // Un message ordinaire : #général en gras, sans badge ; le serveur garde son point.
  await post(a, general, 'Coucou la compagnie');
  await expect(chan(b.page, 'général')).toHaveAttribute('data-unread', '');
  await expect(chan(b.page, 'général')).toHaveAccessibleName(said('général', 'non lu'));
  await expect(server(b.page, 'Récif')).toHaveAttribute('data-unread', '');

  // Une mention : badge sur le salon et le serveur, compteur dans le titre.
  await post(a, general, `@${b.name} regarde ça`);
  await expect(chan(b.page, 'général')).toHaveAccessibleName(said('général', 'non lu', '1 mention'));
  await expect(server(b.page, 'Récif')).toHaveAccessibleName(said('non lu', '1 mention'));
  await expect(b.page).toHaveTitle('(1) KRAKEN');
  // Second onglet du même compte : il voit le même état.
  const tab2 = await b.context.newPage();
  await tab2.goto(`/channels/${gid}/${calm}`);
  await expect(chan(tab2, 'général')).toHaveAccessibleName(said('général', 'non lu', '1 mention'));
  await expect(tab2).toHaveTitle('(1) KRAKEN');

  // b ouvre #général : la ligne se place avant le premier message non lu, puis tout est lu.
  await chan(b.page, 'général').click();
  const divider = b.page.getByRole('log').getByRole('separator', { name: 'Nouveaux messages' });
  await expect(divider).toBeVisible();
  await expect(b.page.getByRole('log').locator('.k-newline + article')).toContainText('Coucou la compagnie');
  await expect(chan(b.page, 'général')).not.toHaveAttribute('data-unread');
  await expect(b.page).toHaveTitle('KRAKEN');
  // L'autre onglet suit, sans recharger.
  await expect(chan(tab2, 'général')).not.toHaveAttribute('data-unread');
  await expect(chan(tab2, 'général')).toHaveAccessibleName('général');
  await expect(server(tab2, 'Récif')).not.toHaveAttribute('data-unread');
  await expect(tab2).toHaveTitle('KRAKEN');
  // La ligne reste tant que le salon est ouvert, et disparaît quand on écrit.
  await say(b.page, 'général', 'Me voilà');
  await expect(divider).toHaveCount(0);
  await a11y(b.page);
});

test('S09-H1 sourdine et niveaux par salon : plus de gras, la mention compte, sauf avec « Rien » ; gardés après rechargement', async ({ user }) => {
  const a = await user('bav');
  const b = await user('cal');
  const [gid] = await together(a, b, 'Brouhaha');
  const noisy = await channel(a, gid, 'bruit');
  await expect(chan(b.page, 'bruit')).toBeVisible();

  // Clic droit sur #bruit : menu, puis sourdine d'une heure dans la boîte « Notifications ».
  await chan(b.page, 'bruit').click({ button: 'right' });
  const menu = b.page.getByRole('menu', { name: 'Actions pour #bruit' });
  await expect(menu.getByRole('menuitem').first()).toBeFocused();
  await a11y(b.page);
  await menu.getByRole('menuitem', { name: 'Mettre en sourdine…' }).click();
  const dialog = b.page.getByRole('dialog', { name: 'Notifications de #bruit' });
  await dialog.getByLabel('Sourdine').selectOption({ label: 'Pendant 1 heure' });
  await expect(chan(b.page, 'bruit')).toHaveAttribute('data-muted', '');
  await expect(dialog.getByLabel('Sourdine')).toContainText("En sourdine jusqu'à");
  await a11y(b.page);
  await dialog.getByRole('button', { name: 'Fermer' }).click();
  await expect(dialog).toHaveCount(0);

  // Un message dans le salon en sourdine : pas de gras, pas de point sur le serveur.
  await post(a, noisy, 'blabla sans intérêt');
  await post(a, noisy, '@everyone grande nouvelle');
  await expect(chan(b.page, 'bruit')).toHaveAccessibleName(said('bruit', 'en sourdine', '1 mention'));
  await expect(chan(b.page, 'bruit')).not.toHaveAttribute('data-unread');
  await expect(server(b.page, 'Brouhaha')).not.toHaveAttribute('data-unread');
  await expect(server(b.page, 'Brouhaha')).toHaveAccessibleName(/Brouhaha, 1 mention$/);

  // Niveau « Rien » : plus de compteur ; réglage gardé après rechargement (côté serveur).
  await b.page.getByRole('button', { name: 'Notifications du serveur' }).click();
  await expect(b.page.getByRole('dialog', { name: 'Notifications de Brouhaha' })).toBeVisible();
  await b.page.keyboard.press('Escape');
  await chan(b.page, 'bruit').click({ button: 'right' });
  await menu.getByRole('menuitem', { name: 'Notifications…' }).click();
  await dialog.getByRole('radio', { name: /^Rien/ }).check();
  await expect(chan(b.page, 'bruit')).toHaveAccessibleName(said('bruit', 'en sourdine'));
  await dialog.getByRole('button', { name: 'Fermer' }).click();
  await b.page.reload();
  await expect(chan(b.page, 'bruit')).toHaveAttribute('data-muted', '');
  await expect(chan(b.page, 'bruit')).toHaveAccessibleName(said('bruit', 'en sourdine'));
  await expect(b.page).toHaveTitle('KRAKEN');

  // Retirer la sourdine au clavier (Maj+F10) : #bruit redevient gras.
  await chan(b.page, 'bruit').focus();
  await b.page.keyboard.press('Shift+F10');
  await expect(menu.getByRole('menuitem', { name: 'Marquer comme lu' })).toBeFocused();
  await b.page.keyboard.press('ArrowDown');
  await expect(menu.getByRole('menuitem', { name: 'Retirer la sourdine' })).toBeFocused();
  await b.page.keyboard.press('Enter');
  await expect(chan(b.page, 'bruit')).toHaveAttribute('data-unread', '');
  await expect(chan(b.page, 'bruit')).toBeFocused();

  // « Marquer comme lu » sur le serveur, au clavier.
  await server(b.page, 'Brouhaha').focus();
  await b.page.keyboard.press('Shift+F10');
  await b.page.getByRole('menu', { name: 'Actions pour Brouhaha' }).getByRole('menuitem', { name: 'Marquer comme lu' }).press('Enter');
  await expect(chan(b.page, 'bruit')).not.toHaveAttribute('data-unread');
  await expect(server(b.page, 'Brouhaha')).not.toHaveAttribute('data-unread');
});

/** Remplace l'API Notification par un enregistreur (permission accordée au contexte). */
async function recordNotifications(ctx: BrowserContext) {
  await ctx.grantPermissions(['notifications']);
  await ctx.addInitScript(() => {
    const w = window as unknown as { __notes: { title: string; body?: string }[]; __last: { onclick: (() => void) | null } };
    w.__notes = [];
    const Real = window.Notification;
    class Recorder {
      // Chromium sans écran répond « denied » ici même quand le contexte a la permission.
      static get permission() {
        return 'granted';
      }
      static requestPermission() {
        return Real.requestPermission().then(() => 'granted');
      }
      onclick: (() => void) | null = null;
      constructor(title: string, opts?: NotificationOptions) {
        w.__notes.push({ title, body: opts?.body });
        w.__last = this;
      }
      close() {}
    }
    (window as unknown as { Notification: unknown }).Notification = Recorder;
  });
}

test('S09-H2 MP : badge sur l’accueil et la conversation, notification sur le bureau fenêtre inactive, clic qui ouvre la conversation', async ({ user }) => {
  const a = await user('exp');
  const b = await user('dst');
  await together(a, b, 'Port');
  await recordNotifications(b.context);

  // Activer depuis les réglages (la question du navigateur ne vient qu'au clic).
  await b.page.goto('/settings/notifications');
  const toggle = b.page.getByRole('switch', { name: 'Notifications sur le bureau' });
  await expect(toggle).not.toBeChecked();
  await toggle.check();
  await expect(toggle).toBeChecked();
  await a11y(b.page);
  await b.page.goto('/channels/@me');

  const res = await a.context.request.post(`${API}/users/@me/channels`, {
    data: { recipients: [b.id] },
    headers: { Authorization: `Session ${a.token}` },
  });
  const dm = (await res.json()).id;
  // Fenêtre au premier plan : pas de notification, mais le badge.
  await post(a, dm, 'Premier mot');
  const home = rail(b.page).getByRole('link', { name: /^Accueil et messages privés/ });
  await expect(home).toHaveAccessibleName('Accueil et messages privés, 1 mention');
  const row = b.page.getByRole('navigation', { name: 'Messages privés' }).getByRole('link', { name: new RegExp(a.name) });
  await expect(row).toHaveAccessibleName(said(a.name, 'non lu', '1 mention'));
  expect(await b.page.evaluate(() => (window as unknown as { __notes: unknown[] }).__notes.length)).toBe(0);

  // Fenêtre inactive : notification avec l'auteur et le texte.
  await b.page.evaluate(() => window.dispatchEvent(new Event('blur')));
  await post(a, dm, 'Tu es là ?');
  await expect(home).toHaveAccessibleName('Accueil et messages privés, 2 mentions');
  await expect(b.page).toHaveTitle('(2) KRAKEN');
  await expect
    .poll(() => b.page.evaluate(() => (window as unknown as { __notes: unknown[] }).__notes))
    .toEqual([{ title: a.name, body: 'Tu es là ?' }]);

  // Le clic ouvre la conversation, qui se lit une fois la fenêtre revenue.
  await b.page.evaluate(() => (window as unknown as { __last: { onclick: () => void } }).__last.onclick());
  await expect(b.page).toHaveURL(new RegExp(`/channels/@me/${dm}$`));
  await b.page.evaluate(() => window.dispatchEvent(new Event('focus')));
  await expect(home).toHaveAccessibleName('Accueil et messages privés');
  await expect(b.page).toHaveTitle('KRAKEN');
  await expect(messages(b.page).filter({ hasText: 'Tu es là ?' })).toHaveCount(1);
});

test('S13-H1 Alt+Maj+↓ et ↑ mènent au salon non lu suivant ou précédent', async ({ user }) => {
  const a = await user('nav');
  const b = await user('saut');
  const [gid] = await together(a, b, 'Archipel');
  const first = await channel(a, gid, 'aval');
  const second = await channel(a, gid, 'amont');
  await expect(chan(b.page, 'amont')).toBeVisible();
  await post(a, first, 'un message en aval');
  await post(a, second, 'un message en amont');
  await expect(chan(b.page, 'amont')).toHaveAttribute('data-unread', '');
  // Les salons hors catégorie viennent en tête : depuis #général, ils sont plus haut.
  await b.page.keyboard.press('Alt+Shift+ArrowUp');
  await expect(b.page.getByRole('heading', { name: 'amont', level: 1 })).toBeVisible();
  await expect(chan(b.page, 'amont')).not.toHaveAttribute('data-unread');
  await b.page.keyboard.press('Alt+Shift+ArrowUp');
  await expect(b.page.getByRole('heading', { name: 'aval', level: 1 })).toBeVisible();
  // Plus rien de non lu : la touche ne fait rien.
  await b.page.keyboard.press('Alt+Shift+ArrowDown');
  await expect(b.page.getByRole('heading', { name: 'aval', level: 1 })).toBeVisible();
});
