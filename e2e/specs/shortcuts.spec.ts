// Raccourcis clavier et Quick Switcher (S13, S29) : Ctrl+K (opérateurs, accents, récents, MP, personnes),
// Alt+↑/↓ et Ctrl+Alt+↑/↓, aide (Ctrl+/ et Réglages), micro et son au clavier, touches seules libres dans la saisie,
// et le sélecteur d'emojis à un seul arrêt de Tab (tabindex itinérant).
import { test, expect, a11y, API, type User } from '../fixtures';
import type { APIRequestContext, Page } from '@playwright/test';

// Micro factice, sans fenêtre d'autorisation : pour les raccourcis micro et son dans un salon vocal.
test.use({ launchOptions: { args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'] } });

type Api = (t?: string) => Promise<APIRequestContext>;
type Guild = { guild: { id: string }; channels: { id: string; kind: string; name: string }[] };

async function guild(http: APIRequestContext, name: string): Promise<Guild> {
  const res = await http.post(`${API}/guilds`, { data: { name } });
  expect(res.status(), await res.text()).toBe(201);
  return res.json();
}

async function channel(http: APIRequestContext, gid: string, name: string, kind = 'text') {
  const res = await http.post(`${API}/guilds/${gid}/channels`, { data: { name, kind, parent_id: null } });
  expect(res.status(), await res.text()).toBe(201);
  return (await res.json()) as { id: string };
}

async function befriend(api: Api, a: User, b: User) {
  expect((await (await api(a.token)).post(`${API}/users/@me/relationships`, { data: { username: b.name } })).status()).toBe(201);
  expect((await (await api(b.token)).put(`${API}/users/@me/relationships/${a.id}`, { data: { kind: 'friend' } })).status()).toBe(200);
}

const switcher = (page: Page) => page.getByRole('dialog', { name: 'Aller vers' });
const field = (page: Page) => switcher(page).getByRole('combobox');
const composer = (page: Page, channel: string) => page.getByLabel(`Écrire dans #${channel}`);

/** Un compte avec deux serveurs : « Océan » (#général, vocal Général) et « Forge » (#météo en plus). */
async function setup(user: (p?: string) => Promise<User>, api: Api) {
  const a = await user('nina');
  const http = await api(a.token);
  const g1 = await guild(http, 'Océan');
  const g2 = await guild(http, 'Forge');
  const meteo = await channel(http, g2.guild.id, 'météo');
  const general = g1.channels.find((c) => c.kind === 'text')!;
  await a.page.goto(`/channels/${g1.guild.id}/${general.id}`);
  await expect(a.page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
  return { a, http, g1, g2, meteo, general };
}

test('qs-H1 Ctrl+K depuis la saisie : accents ignorés, Entrée dans un autre serveur, Échap rend le focus', async ({ user, api }) => {
  const { a, g2, meteo, general } = await setup(user, api);
  const page = a.page;
  const box = composer(page, 'général');
  await box.click();
  await page.keyboard.press('Control+k');
  await expect(switcher(page)).toBeVisible();
  await expect(field(page)).toBeFocused();
  // Rien de visité avant : l'état vide le dit, et la liste n'est pas déployée.
  await expect(field(page)).toHaveAttribute('aria-expanded', 'false');
  await expect(switcher(page).getByRole('status')).toHaveText('Aucun lieu récent');
  await a11y(page);

  await page.keyboard.press('Escape');
  await expect(switcher(page)).toHaveCount(0);
  await expect(box).toBeFocused();

  await page.keyboard.press('Control+k');
  // Les lettres trouvées sont dans <mark>, en gras et soulignées (pas seulement une couleur).
  await field(page).fill('MET');
  await expect(switcher(page).getByText('mét', { exact: true })).toHaveCSS('text-decoration-line', 'underline');
  await expect(switcher(page).getByText('mét', { exact: true })).toHaveCSS('font-weight', '700');
  await field(page).fill('METEO');
  const option = switcher(page).getByRole('option', { name: /météo/ });
  await expect(option).toBeVisible();
  await expect(option).toContainText('Forge');
  // La ligne tient dans la fenêtre : le détail (nom du serveur) n'est pas coupé au bord.
  const [rowBox, dialogBox] = [await option.boundingBox(), await switcher(page).boundingBox()];
  expect(rowBox!.x + rowBox!.width).toBeLessThanOrEqual(dialogBox!.x + dialogBox!.width + 1);
  await expect(switcher(page).getByRole('status')).toHaveText('1 résultat');
  await expect(field(page)).toHaveAttribute('aria-expanded', 'true');
  await expect(field(page)).toHaveAttribute('aria-controls', 'sw-list');
  await expect(field(page)).toHaveAttribute('aria-activedescendant', (await option.getAttribute('id'))!);
  await expect(option).toHaveAttribute('aria-selected', 'true');
  await expect(switcher(page).getByRole('group', { name: 'Salons' })).toBeVisible();
  await a11y(page);
  await field(page).press('Enter');
  await expect(switcher(page)).toHaveCount(0);
  await expect(page).toHaveURL(new RegExp(`/channels/${g2.guild.id}/${meteo.id}$`));
  await expect(page.getByRole('heading', { name: 'météo', level: 1 })).toBeVisible();

  // Récents : le salon quitté (pas celui où l'on est), et les identifiants seuls dans le stockage.
  await page.keyboard.press('Control+k');
  const recents = switcher(page).getByRole('group', { name: 'Récents' });
  await expect(recents.getByRole('option', { name: /général/ })).toBeVisible();
  await expect(recents.getByRole('option', { name: /météo/ })).toHaveCount(0);
  await expect(switcher(page).getByRole('status')).toHaveText('1 lieu récent');
  const stored = await page.evaluate(() => localStorage.getItem('kraken.recents'));
  expect(stored).toBe(`${meteo.id},${general.id}`);
  await page.keyboard.press('Escape');
  // Les récents survivent au rechargement.
  await page.reload();
  await expect(page.getByRole('heading', { name: 'météo', level: 1 })).toBeVisible();
  await page.keyboard.press('Control+k');
  await expect(switcher(page).getByRole('group', { name: 'Récents' }).getByRole('option', { name: /général/ })).toBeVisible();
});

test('qs-H2 flèches (avec retour), Home et End, Tab reste dans la fenêtre, aucun résultat annoncé', async ({ user, api }) => {
  const { a } = await setup(user, api);
  const page = a.page;
  await page.keyboard.press('Control+k');
  await field(page).fill('gen');
  const options = switcher(page).getByRole('option');
  // #général et le vocal Général de chacun des deux serveurs, rien d'autre.
  await expect(options).toHaveCount(4);
  await expect(switcher(page).getByRole('status')).toHaveText('4 résultats');
  const selected = switcher(page).getByRole('option', { selected: true });
  await expect(selected).toHaveCount(1);
  const first = await selected.getAttribute('id');
  await field(page).press('ArrowDown');
  await expect(selected).not.toHaveAttribute('id', first!);
  await field(page).press('ArrowUp');
  await expect(selected).toHaveAttribute('id', first!);
  // Retour à l'autre bout, dans les deux sens.
  await field(page).press('ArrowUp');
  await expect(selected).not.toHaveAttribute('id', first!);
  await field(page).press('ArrowDown');
  await expect(selected).toHaveAttribute('id', first!);
  await field(page).press('End');
  await expect(selected).not.toHaveAttribute('id', first!);
  await field(page).press('Home');
  await expect(selected).toHaveAttribute('id', first!);
  // Le focus reste dans le champ ; Tab ne sort pas de la fenêtre (champ, Fermer, retour au champ).
  await expect(field(page)).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(switcher(page).getByRole('button', { name: 'Fermer' })).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(field(page)).toBeFocused();
  await page.keyboard.press('Shift+Tab');
  await expect(switcher(page).getByRole('button', { name: 'Fermer' })).toBeFocused();
  await page.keyboard.press('Shift+Tab');
  await expect(field(page)).toBeFocused();

  await field(page).fill('zzzzz');
  await expect(switcher(page).getByText('Aucun résultat pour « zzzzz »')).toBeVisible();
  await expect(switcher(page).getByRole('status')).toHaveText('Aucun résultat');
  await expect(field(page)).toHaveAttribute('aria-expanded', 'false');
  await expect(switcher(page).getByRole('listbox')).toHaveCount(0);
  // Ctrl+K referme.
  await page.keyboard.press('Control+k');
  await expect(switcher(page)).toHaveCount(0);
});

test('qs-H3 opérateurs # * @ : salons seuls, serveurs seuls, personnes, MP et groupes', async ({ user, api }) => {
  const { a, g1, g2 } = await setup(user, api);
  const b = await user('basile');
  const c = await user('camille');
  await befriend(api, a, b);
  await befriend(api, a, c);
  const http = await api(a.token);
  const dm = await (await http.post(`${API}/users/@me/channels`, { data: { recipients: [b.id] } })).json();
  const group = await (await http.post(`${API}/users/@me/channels`, { data: { recipients: [b.id, c.id], name: 'Les trois' } })).json();
  await a.page.reload();
  const page = a.page;
  await expect(page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
  await expect(page.getByRole('navigation', { name: 'Serveurs' }).getByRole('link', { name: /Forge/ })).toBeVisible();
  await page.keyboard.press('Control+k');
  await expect(switcher(page).getByText('# salons · @ personnes et messages privés · * serveurs')).toBeVisible();

  // # : les salons seulement (texte et vocal), jamais un serveur ni une personne.
  await field(page).fill('#o');
  await expect(switcher(page).getByRole('group', { name: 'Salons' })).toBeVisible();
  await expect(switcher(page).getByRole('group', { name: 'Serveurs' })).toHaveCount(0);
  await field(page).fill('#gen');
  await expect(switcher(page).getByRole('option', { name: /Salon vocal Général Océan/ })).toBeVisible();
  await expect(switcher(page).getByRole('option', { name: /Salon textuel général Océan/ })).toBeVisible();
  // * : les serveurs seulement.
  await field(page).fill('*o');
  await expect(switcher(page).getByRole('group', { name: 'Serveurs' }).getByRole('option', { name: /Océan/ })).toBeVisible();
  await expect(switcher(page).getByRole('group', { name: 'Serveurs' }).getByRole('option', { name: /Forge/ })).toBeVisible();
  await expect(switcher(page).getByRole('group', { name: 'Salons' })).toHaveCount(0);
  await field(page).fill('*oce');
  await expect(switcher(page).getByRole('option')).toHaveCount(1);
  await field(page).press('Enter');
  await expect(page).toHaveURL(new RegExp(`/channels/${g1.guild.id}/`));
  // Un serveur s'ouvre sur le dernier salon qu'on y a vu, ou sinon le premier.
  await page.keyboard.press('Control+k');
  await field(page).fill('*forge');
  await field(page).press('Enter');
  await expect(page).toHaveURL(new RegExp(`/channels/${g2.guild.id}/`));
  await expect(page.getByRole('heading', { level: 1 })).toBeVisible();

  // @ : un MP (par le nom de la personne), un groupe (par le nom d'un de ses membres), jamais un salon.
  await page.keyboard.press('Control+k');
  await field(page).fill(`@${b.name}`);
  const dms = switcher(page).getByRole('group', { name: 'Messages privés' });
  await expect(dms.getByRole('option', { name: new RegExp(`Message privé ${b.name}`) })).toBeVisible();
  await expect(dms.getByRole('option', { name: /Groupe Les trois/ })).toBeVisible();
  await expect(switcher(page).getByRole('group', { name: 'Salons' })).toHaveCount(0);
  await a11y(page);
  await dms.getByRole('option', { name: /Message privé/ }).click();
  await expect(switcher(page)).toHaveCount(0);
  await expect(page).toHaveURL(new RegExp(`/channels/@me/${dm.id}$`));

  // Une personne sans MP ouvert : le MP s'ouvre (et se crée) à la sélection.
  await page.keyboard.press('Control+k');
  await field(page).fill(`@${c.name}`);
  const people = switcher(page).getByRole('group', { name: 'Personnes' });
  await expect(people.getByRole('option', { name: new RegExp(c.name) })).toBeVisible();
  await expect(switcher(page).getByRole('group', { name: 'Messages privés' }).getByRole('option', { name: /Groupe Les trois/ })).toBeVisible();
  await people.getByRole('option', { name: new RegExp(c.name) }).click();
  await expect(page.getByRole('heading', { name: c.name, level: 1 })).toBeVisible();
  await expect(page).toHaveURL(/\/channels\/@me\/[0-9a-f-]{36}$/);
  expect(page.url()).not.toContain(group.id);
  expect(page.url()).not.toContain(dm.id);
});

test.describe('à 390 px', () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test('qs-H4 bouton « Aller vers… », feuille pleine largeur, lignes de 44 px, rien ne déborde', async ({ user, api }) => {
    const { a, g2, meteo } = await setup(user, api);
    const page = a.page;
    await page.getByRole('button', { name: 'Ouvrir la navigation' }).click();
    await page.getByRole('button', { name: 'Aller vers…' }).click();
    const dialog = switcher(page);
    await expect(dialog).toBeVisible();
    await field(page).fill('o');
    const rows = dialog.getByRole('option');
    await expect(rows.first()).toBeVisible();
    const box = (await dialog.boundingBox())!;
    expect(Math.round(box.x)).toBe(8);
    expect(Math.round(box.width)).toBe(374);
    for (let i = 0; i < (await rows.count()); i++) expect((await rows.nth(i).boundingBox())!.height).toBeGreaterThanOrEqual(44);
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
    expect(overflow).toBeLessThanOrEqual(0);
    await a11y(page);
    await field(page).fill('meteo');
    await dialog.getByRole('option', { name: /météo/ }).click();
    await expect(page).toHaveURL(new RegExp(`/channels/${g2.guild.id}/${meteo.id}$`));
    // Le tiroir se ferme avec le choix.
    await expect(page.getByRole('heading', { name: 'météo', level: 1 })).toBeVisible();
    await expect(page.getByRole('navigation', { name: 'Salons' })).not.toBeInViewport();
  });
});

test('nav-H1 Alt+↑/↓ : salon précédent et suivant (vocal compris, sans rejoindre), Ctrl+Alt+↑/↓ : serveurs', async ({ user, api }) => {
  const { a, http, g1, g2, general } = await setup(user, api);
  await channel(http, g1.guild.id, 'zèbre');
  const page = a.page;
  await page.reload();
  await expect(page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
  const links = (nav: string, pattern: RegExp) =>
    page.getByRole('navigation', { name: nav }).getByRole('link').evaluateAll((els, src) => {
      const re = new RegExp(src);
      return els.map((e) => e.getAttribute('href') ?? '').filter((h) => re.test(h));
    }, pattern.source);
  // L'ordre de la barre latérale, catégories exclues : celui de Alt+↑/↓.
  const chans = await links('Salons', new RegExp(`^/channels/${g1.guild.id}/[0-9a-f-]{36}$`));
  expect(chans.length).toBeGreaterThanOrEqual(3);
  expect(chans.some((h) => h.endsWith(g1.channels.find((c) => c.kind === 'voice')!.id))).toBe(true);
  let at = chans.findIndex((h) => h.endsWith(general.id));
  await composer(page, 'général').click();
  for (let i = 0; i < chans.length + 1; i++) {
    await page.keyboard.press('Alt+ArrowDown');
    at = (at + 1) % chans.length;
    await expect(page).toHaveURL(new RegExp(`${chans[at]}$`));
  }
  await page.keyboard.press('Alt+ArrowUp');
  at = (at + chans.length - 1) % chans.length;
  await expect(page).toHaveURL(new RegExp(`${chans[at]}$`));
  // Un salon vocal s'ouvre sans qu'on le rejoigne.
  const voice = g1.channels.find((c) => c.kind === 'voice')!;
  while (!chans[at].endsWith(voice.id)) {
    await page.keyboard.press('Alt+ArrowDown');
    at = (at + 1) % chans.length;
    await expect(page).toHaveURL(new RegExp(`${chans[at]}$`));
  }
  await expect(page.getByRole('button', { name: 'Rejoindre', exact: true })).toBeVisible();

  // Serveurs : l'ordre du rail, avec retour au début.
  const rail = await links('Serveurs', /^\/channels\/[0-9a-f-]{36}$/);
  expect(rail).toEqual(expect.arrayContaining([`/channels/${g1.guild.id}`, `/channels/${g2.guild.id}`]));
  let s = rail.indexOf(`/channels/${g1.guild.id}`);
  for (let i = 0; i < rail.length + 1; i++) {
    await page.keyboard.press('Control+Alt+ArrowDown');
    s = (s + 1) % rail.length;
    await expect(page).toHaveURL(new RegExp(`${rail[s]}(/|$)`));
  }
  await page.keyboard.press('Control+Alt+ArrowUp');
  s = (s + rail.length - 1) % rail.length;
  await expect(page).toHaveURL(new RegExp(`${rail[s]}(/|$)`));
});

test('kb-H1 Ctrl+/ et Réglages listent les mêmes raccourcis ; ? seulement hors des champs', async ({ user }) => {
  const a = await user('aide');
  const page = a.page;
  await page.keyboard.press('Control+/');
  const help = page.getByRole('dialog', { name: 'Raccourcis clavier' });
  await expect(help).toBeVisible();
  await expect(help.getByRole('heading', { level: 3 })).toHaveText(['Navigation', 'Messages', 'Appel', 'Général']);
  // La fenêtre s'ouvre en haut : titre à l'écran, « Fermer » reste visible sous le contenu défilant.
  await expect(help.getByRole('heading', { name: 'Raccourcis clavier', level: 2 })).toBeInViewport();
  await expect(help.getByRole('button', { name: 'Fermer' })).toBeInViewport();
  expect(await help.evaluate((e) => e.scrollTop)).toBe(0);
  await expect(help.getByText('Lecture seule')).toBeVisible();
  await expect(help.getByText('hors de la fenêtre')).toBeVisible();
  const terms = await help.getByRole('term').allTextContents();
  const keys = await help.getByRole('definition').allTextContents();
  expect(terms.length).toBeGreaterThanOrEqual(12);
  expect(terms).toEqual(expect.arrayContaining(['Salon précédent', 'Serveur suivant', 'Couper ou réactiver le micro']));
  await expect(help.getByRole('definition').first()).toHaveText('CtrlK');
  await a11y(page);
  await page.keyboard.press('Escape');
  await expect(help).toHaveCount(0);

  // La page de Réglages montre la même liste ; Ctrl+/ y ouvre la fenêtre par-dessus, Échap ne ferme que celle-là.
  await page.goto('/settings/keybinds');
  const settings = page.getByRole('dialog', { name: 'Réglages' });
  await expect(settings.getByRole('heading', { name: 'Raccourcis clavier', level: 2 })).toBeVisible();
  await expect(settings.getByText('Lecture seule')).toBeVisible();
  expect(await settings.getByRole('term').allTextContents()).toEqual(terms);
  expect(await settings.getByRole('definition').allTextContents()).toEqual(keys);
  await a11y(page);
  await page.keyboard.press('Control+/');
  await expect(help).toBeVisible();
  await a11y(page);
  await page.keyboard.press('Escape');
  await expect(help).toHaveCount(0);
  await expect(settings).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(settings).toHaveCount(0);

  // « ? » ouvre l'aide hors d'un champ, et reste une lettre dans la saisie.
  await page.goto('/channels/@me');
  await page.getByRole('heading', { name: 'Amis', level: 1 }).click();
  await page.keyboard.press('Shift+?');
  await expect(help).toBeVisible();
  await help.getByRole('button', { name: 'Fermer' }).click();
  await expect(help).toHaveCount(0);
});

test('kb-H2 les touches seules ne sont pas prises dans la saisie ; Ctrl+K marche partout', async ({ user, api }) => {
  const { a, g1 } = await setup(user, api);
  const page = a.page;
  const box = composer(page, 'général');
  await box.click();
  for (const key of ['?', 'k', 'm', 'd', '/', 'ArrowUp']) await page.keyboard.press(key === '?' ? 'Shift+?' : key);
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(box).toHaveValue('?kmd/');
  await expect(page).toHaveURL(new RegExp(`/channels/${g1.guild.id}/`));
  // Hors d'un appel, Ctrl+Maj+M et Ctrl+Maj+D ne font rien (et n'annoncent rien).
  await page.keyboard.press('Control+Shift+M');
  await page.keyboard.press('Control+Shift+D');
  await expect(page.getByText(/^(Micro|Son) (coupé|réactivé)/)).toHaveCount(0);
  await expect(box).toBeFocused();
  // Dans la recherche d'emojis aussi, une lettre reste une lettre.
  await page.getByRole('button', { name: 'Insérer un emoji' }).click();
  const search = page.getByRole('searchbox', { name: 'Chercher un emoji' });
  await expect(search).toBeFocused();
  await page.keyboard.press('Shift+?');
  await expect(search).toHaveValue('?');
  await expect(page.getByRole('dialog', { name: 'Raccourcis clavier' })).toHaveCount(0);
});

test('kb-H3 Ctrl+Maj+M coupe et rétablit le micro, Ctrl+Maj+D le son, pendant un appel', async ({ user, api }) => {
  test.setTimeout(90_000);
  const a = await user('vox');
  const http = await api(a.token);
  const g = await guild(http, 'Cloche');
  const voice = g.channels.find((c) => c.kind === 'voice')!;
  const page = a.page;
  await page.goto(`/channels/${g.guild.id}/${voice.id}`);
  await expect(page.getByRole('heading', { name: 'Général', level: 1 })).toBeVisible();
  await page.getByRole('button', { name: 'Rejoindre', exact: true }).click();
  await expect(page.getByRole('region', { name: 'Connexion vocale' }).getByRole('status')).toHaveText('En appel', { timeout: 20_000 });
  const bar = page.getByRole('group', { name: "Commandes de l'appel" });
  const mic = bar.getByRole('button', { name: 'Couper le micro' });
  const deaf = bar.getByRole('button', { name: 'Couper le son' });
  await expect(mic).toHaveAttribute('aria-pressed', 'false');

  await page.keyboard.press('Control+Shift+M');
  await expect(mic).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByText('Micro coupé', { exact: true })).toHaveCount(1);
  await page.keyboard.press('Control+Shift+M');
  await expect(mic).toHaveAttribute('aria-pressed', 'false');
  await expect(page.getByText('Micro réactivé', { exact: true })).toHaveCount(1);
  await page.keyboard.press('Control+Shift+D');
  await expect(deaf).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByText('Son coupé, micro coupé', { exact: true })).toHaveCount(1);
  await page.keyboard.press('Control+Shift+D');
  await expect(deaf).toHaveAttribute('aria-pressed', 'false');
  await expect(page.getByText('Son réactivé', { exact: true })).toHaveCount(1);
  await bar.getByRole('button', { name: 'Quitter', exact: true }).click();
});

test('em-H1 sélecteur d\'emojis : un seul arrêt de Tab, flèches, Début/Fin, pages, Entrée insère, Tab sort de la grille', async ({ user, api }) => {
  const { a } = await setup(user, api);
  const page = a.page;
  const box = composer(page, 'général');
  await box.click();
  await page.getByRole('button', { name: 'Insérer un emoji' }).click();
  const picker = page.getByRole('dialog', { name: 'Insérer un emoji' });
  const search = picker.getByRole('searchbox', { name: 'Chercher un emoji' });
  await expect(search).toBeFocused();
  const cells = picker.getByRole('group', { name: 'Emojis' }).getByRole('button');
  expect(await cells.count()).toBeGreaterThan(50);
  const stops = () => cells.evaluateAll((els) => els.filter((e) => (e as HTMLElement).tabIndex === 0).length);
  expect(await stops()).toBe(1);
  // Les flèches ne font jamais défiler la page : la touche est bien retenue.
  await page.evaluate(() => addEventListener('keydown', (e) => ((window as unknown as { held: boolean }).held = e.defaultPrevented)));
  const held = () => page.evaluate(() => (window as unknown as { held: boolean }).held);
  const where = () =>
    page.evaluate(() => {
      const el = document.activeElement as HTMLElement;
      const r = el.getBoundingClientRect();
      return { x: Math.round(r.left), y: Math.round(r.top), tab: el.tabIndex, name: el.getAttribute('aria-label') };
    });

  await page.keyboard.press('Tab');
  const start = await where();
  expect(start.tab).toBe(0);
  await expect(cells.first()).toBeFocused();
  await page.keyboard.press('ArrowRight');
  const right = await where();
  expect(right.x).toBeGreaterThan(start.x);
  expect(right.y).toBe(start.y);
  expect(await held()).toBe(true);
  expect(await stops()).toBe(1);
  expect(right.tab).toBe(0);
  await page.keyboard.press('ArrowDown');
  const down = await where();
  expect(down.y).toBeGreaterThan(right.y);
  expect(Math.abs(down.x - right.x)).toBeLessThan(8);
  expect(await held()).toBe(true);
  await page.keyboard.press('ArrowUp');
  expect(await where()).toMatchObject({ x: right.x, y: right.y });
  await page.keyboard.press('ArrowLeft');
  expect(await where()).toMatchObject({ x: start.x, y: start.y });
  await page.keyboard.press('ArrowLeft');
  // Retour à la ligne : avant la première case, la dernière.
  expect(await where()).toMatchObject({ name: await cells.last().getAttribute('aria-label') });
  await page.keyboard.press('Control+Home');
  await expect(cells.first()).toBeFocused();
  await page.keyboard.press('End');
  const end = await where();
  expect(end.y).toBe(start.y);
  expect(end.x).toBeGreaterThan(start.x);
  await page.keyboard.press('Home');
  expect(await where()).toMatchObject({ x: start.x, y: start.y });
  await page.keyboard.press('PageDown');
  const page1 = await where();
  expect(page1.y).toBeGreaterThan(down.y);
  await page.keyboard.press('PageUp');
  expect((await where()).y).toBeLessThan(page1.y);
  await page.keyboard.press('Control+End');
  await expect(cells.last()).toBeFocused();
  expect(await stops()).toBe(1);
  await page.keyboard.press('Control+Home');
  await a11y(page);

  // Tab quitte la grille (la recherche revient avec Maj+Tab), Entrée insère l'emoji de la case active.
  await page.keyboard.press('Tab');
  await expect(cells.first()).not.toBeFocused();
  expect(await page.evaluate(() => !!document.activeElement?.closest('[role="group"]'))).toBe(false);
  await page.keyboard.press('Shift+Tab');
  await expect(cells.first()).toBeFocused();
  await page.keyboard.press('Shift+Tab');
  await expect(search).toBeFocused();
  await search.press('ArrowDown');
  await expect(cells.first()).toBeFocused();
  const name = (await cells.first().getAttribute('aria-label'))!;
  const glyph = (await cells.first().textContent())!;
  await page.keyboard.press('Enter');
  await expect(picker).toHaveCount(0);
  await expect(box).toHaveValue(new RegExp(glyph));
  expect(name.length).toBeGreaterThan(0);
});
