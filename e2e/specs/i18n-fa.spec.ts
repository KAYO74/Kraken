// Persan (PR #33), phase 1 : l'infrastructure de la troisième langue de l'interface et la première qui s'écrit de droite à
// gauche. Le catalogue persan (`i18n/fa/*.tsv`) est vide à ce stade : tout texte à l'écran se lit en anglais (le repli de
// `Lang::Fa`), puis en français pour ce que l'anglais ne connaît pas. Ce qui est déjà persan : le nom de la langue dans le
// sélecteur, `<html lang="fa" dir="rtl">`, les durées et « il y a 5 min » (`kraken_core::span`), les dates (`Intl` en
// `fa-u-nu-latn` : calendrier persan, chiffres latins), l'heure sur 24 h.
//
// PHASE 2 (traductions) : les lignes marquées `// PHASE 2` lisent un texte de repli en anglais ; quand le catalogue persan
// existe, elles pointent sur la phrase persane (le titre de la connexion, les noms accessibles, la phrase d'état).
import { test as base, expect, a11y, watch, API, PASSWORD, unique } from '../fixtures';
import type { Browser, BrowserContext, Locator, Page } from '@playwright/test';

type Account = { page: Page; context: BrowserContext; name: string; token: string };
type Viewport = { width: number; height: number };

/**
 * `signedIn(locale?, viewport?)` : un compte neuf, connecté dans un navigateur de cette langue, sans attendre un titre de page
 * (la fixture `user` attend « Amis », écrit en français : ces tests changent la langue). Même fixture que `i18n.spec.ts`.
 */
const test = base.extend<{ signedIn: (locale?: string, viewport?: Viewport) => Promise<Account> }>({
  signedIn: async ({ browser, baseURL, issues, locale: browserLocale }, use) => {
    const contexts: BrowserContext[] = [];
    await use(async (locale = browserLocale ?? 'fr-FR', viewport = { width: 1440, height: 900 }) => {
      const context = await browser.newContext({ baseURL, locale, viewport });
      contexts.push(context);
      const name = unique('i18nf');
      const res = await context.request.post(`${API}/auth/register`, { data: { username: name, email: `${name}@example.org`, password: PASSWORD } });
      expect(res.status(), await res.text()).toBeLessThan(300);
      const { token } = await res.json();
      const page = await context.newPage();
      watch(page, issues);
      await page.goto('/login');
      await page.evaluate(() => localStorage.setItem('kraken.signed_in', '1'));
      await page.goto('/channels/@me');
      await expect(page.locator('#main')).toBeVisible();
      return { page, context, name, token };
    });
    for (const c of contexts) await c.close();
  },
});

const FA = 'فارسی';
const html = (p: Page) => p.locator('html');
const stored = (p: Page, key: string) => p.evaluate((k) => localStorage.getItem(k), key);
const noScroll = async (p: Page) =>
  expect(await p.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(p.viewportSize()!.width);
/** La zone d'état qui dit ce texte (la coquille et la page de langue en ont chacune une : on filtre toujours). */
const said = (p: Page, text: string | RegExp) => p.getByRole('status').filter({ hasText: text });
/** Chiffres persans (۰-۹) et arabo-indiens (٠-٩) : l'interface n'en écrit jamais, ses chiffres sont latins. */
const INDIC = /[۰-۹٠-٩]/;
/** Une lettre de l'écriture arabe (le persan s'y écrit). */
const ARABIC_SCRIPT = /[\u0600-\u06FF]/;

/** Le sélecteur de langue de la page (carte de connexion ou Réglages) et son étiquette. */
const field = (p: Page) => p.locator('label.app-pref-select:has(select option[value="fa"])').first();
const select = (p: Page) => field(p).locator('select');

/** Le rectangle du texte d'un élément (pas de sa boîte) : là où les lettres sont vraiment posées. */
const textBox = (loc: Locator) =>
  loc.evaluate((el) => {
    const r = document.createRange();
    r.selectNodeContents(el);
    const b = r.getBoundingClientRect();
    return { left: b.left, right: b.right };
  });

/**
 * L'étiquette du sélecteur est calée sur le bord de début du champ : à droite de la page en droite à gauche, à gauche en
 * gauche à droite. (Le sens se lit sur le texte posé, pas sur un attribut.)
 */
async function expectLabelAtStart(p: Page, dir: 'ltr' | 'rtl') {
  const box = await select(p).boundingBox();
  const label = await textBox(field(p).locator('.k-label'));
  expect(box).not.toBeNull();
  if (dir === 'rtl') expect(Math.abs(label.right - (box!.x + box!.width))).toBeLessThanOrEqual(2);
  else expect(Math.abs(label.left - box!.x)).toBeLessThanOrEqual(2);
  expect(await select(p).evaluate((s) => getComputedStyle(s).direction)).toBe(dir);
}

/** Un navigateur qui annonce ces langues (`navigator.languages`), avant tout script de la page. */
async function browserSpeaking(browser: Browser, baseURL: string | undefined, languages: string[]) {
  const context = await browser.newContext({ baseURL, locale: 'fr-FR' });
  await context.addInitScript((list) => {
    Object.defineProperty(navigator, 'languages', { get: () => list, configurable: true });
    Object.defineProperty(navigator, 'language', { get: () => list[0], configurable: true });
  }, languages);
  const page = await context.newPage();
  return { context, page };
}

// ───────── le sélecteur de langue ─────────

test('I18N-F1 connexion : « فارسی » est proposé avec lang="fa", le choix recharge en lang=fa dir=rtl, l’identifiant reste, le mot de passe non', async ({ page }) => {
  await page.goto('/login');
  const language = page.getByRole('combobox', { name: 'Langue' });
  expect(await language.locator('option').allTextContents()).toEqual(["Langue de l'appareil", 'Français', 'English', FA]);
  expect(await language.locator('option').evaluateAll((o) => o.map((x) => x.getAttribute('lang')))).toEqual([null, 'fr', 'en', 'fa']);
  await expect(html(page)).toHaveAttribute('dir', 'ltr');
  await expectLabelAtStart(page, 'ltr');

  await page.getByLabel('Pseudo ou e-mail').fill('perle@example.org');
  await page.getByLabel('Mot de passe').fill('un-secret-bien-gardé');
  await language.selectOption({ label: FA });
  await expect(html(page)).toHaveAttribute('lang', 'fa');
  await expect(html(page)).toHaveAttribute('dir', 'rtl');
  expect(await stored(page, 'kraken.lang')).toBe('fa');
  // PHASE 2 : « Bon retour sous la surface » en persan ; le repli anglais s'affiche tant que le catalogue est vide.
  await expect(page.getByRole('heading', { name: 'Welcome back', level: 1 })).toBeVisible();
  await expect(page.getByLabel('Username or email')).toHaveValue('perle@example.org');
  await expect(page.getByLabel('Password')).toHaveValue('');
  const after = page.getByRole('combobox', { name: 'Language' }); // PHASE 2 : le nom accessible persan de « Langue »
  await expect(after).toHaveValue('fa');
  await expect(after).toBeFocused();
  expect(await after.locator('option').allTextContents()).toEqual(['Device language', 'Français', 'English', FA]); // PHASE 2 : 1re option
  expect(await after.locator('option').evaluateAll((o) => o.map((x) => x.getAttribute('lang')))).toEqual([null, 'fr', 'en', 'fa']);
  expect(await page.evaluate(() => Object.entries(sessionStorage))).toEqual([]);
  // Le sélecteur s'affiche dans le sens de la page : étiquette à droite, champ plein, rien ne déborde.
  await expectLabelAtStart(page, 'rtl');
  await noScroll(page);
  await a11y(page);

  // Dans l'autre sens : retour au français, la page repasse de gauche à droite.
  await after.selectOption({ label: 'Français' });
  await expect(html(page)).toHaveAttribute('lang', 'fr');
  await expect(html(page)).toHaveAttribute('dir', 'ltr');
  await expect(page.getByRole('heading', { name: 'Bon retour sous la surface', level: 1 })).toBeVisible();
  await expect(page.getByRole('combobox', { name: 'Langue' })).toHaveValue('fr');
  await expectLabelAtStart(page, 'ltr');
  expect(await stored(page, 'kraken.lang')).toBe('fr');
});

test('I18N-F2 inscription et mot de passe oublié en persan : lang=fa dir=rtl, sélecteur en place, axe sans violation', async ({ page }) => {
  await page.goto('/login');
  await page.evaluate(() => localStorage.setItem('kraken.lang', 'fa'));
  for (const path of ['/register', '/forgot-password', '/login']) {
    await page.goto(path);
    await expect(html(page)).toHaveAttribute('lang', 'fa');
    await expect(html(page)).toHaveAttribute('dir', 'rtl');
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
    if (path !== '/forgot-password') await expectLabelAtStart(page, 'rtl');
    await noScroll(page);
    await a11y(page);
  }
  // Le champ technique reste de gauche à droite dans une page de droite à gauche (pseudo, e-mail).
  await page.goto('/register');
  await expect(page.getByLabel('Email')).toHaveAttribute('dir', 'ltr'); // PHASE 2 : le nom accessible persan
});

test('I18N-F3 Réglages › Langue et heure : choisir « فارسی » recharge la page, rend le focus et dit la langue ; l’heure suit la langue (24 h) ; retour au français', async ({ signedIn }) => {
  const { page } = await signedIn('fr-FR');
  await page.goto('/settings/language');
  const dialog = page.getByRole('dialog');
  const language = dialog.getByRole('combobox', { name: 'Langue' });
  await expect(language).toHaveValue('auto');
  expect(await language.locator('option').allTextContents()).toEqual(["Langue de l'appareil", 'Français', 'English', FA]);
  await expectLabelAtStart(page, 'ltr');

  await language.selectOption({ label: FA });
  await expect(html(page)).toHaveAttribute('lang', 'fa');
  await expect(html(page)).toHaveAttribute('dir', 'rtl');
  expect(await stored(page, 'kraken.lang')).toBe('fa');
  expect(page.url()).toContain('/settings/language');
  const after = page.getByRole('dialog').getByRole('combobox', { name: 'Language' }); // PHASE 2 : le nom persan de « Langue »
  await expect(after).toHaveValue('fa');
  await expect(after).toBeFocused();
  // La phrase d'état dit la langue, nom de la langue dans son écriture (isolé entre U+2068 et U+2069 en droite à gauche).
  // PHASE 2 : la phrase persane « زبان: فارسی » à la place de « Language: فارسی. ».
  await expect(said(page, FA)).toHaveText(new RegExp(`Language: \\u2068?${FA}\\u2069?\\.`));
  // Le fond de la page de réglages suit le sens : la fenêtre, ses onglets et le sélecteur.
  await expectLabelAtStart(page, 'rtl');
  // L'heure automatique suit la langue de l'interface, pas celle du navigateur (français ici) : 24 h, chiffres latins.
  // PHASE 2 : « نمونه: 14:30 ».
  await expect(said(page, 'Example').first()).toHaveText('Example: 14:30');
  expect(await page.locator('body').innerText()).not.toMatch(INDIC);
  await noScroll(page);
  await a11y(page);
  // Le choix reste après un rechargement, sans nouvelle phrase ni focus volé.
  await page.reload();
  await expect(html(page)).toHaveAttribute('dir', 'rtl');
  await expect(after).toHaveValue('fa');
  await expect(after).not.toBeFocused();
  await expect(said(page, FA)).toHaveCount(0);

  // 12 h choisi à la main : la langue ne change pas ce choix.
  // PHASE 2 : « 12 ساعته » ; le repli anglais dit « 12-hour ».
  const twelve = page.getByRole('dialog').getByRole('group', { name: 'Time format' }).getByRole('radio').nth(1);
  await twelve.check();
  await expect(said(page, 'Example').first()).toHaveText(/^Example: 2:30 PM$/);

  // Dans l'autre sens : français, puis anglais (jamais de droite à gauche pour eux).
  await after.selectOption({ label: 'Français' });
  await expect(html(page)).toHaveAttribute('lang', 'fr');
  await expect(html(page)).toHaveAttribute('dir', 'ltr');
  const fr = page.getByRole('dialog', { name: 'Réglages' }).getByRole('combobox', { name: 'Langue' });
  await expect(fr).toHaveValue('fr');
  await expect(fr).toBeFocused();
  await expect(said(page, 'Langue : Français.')).toBeVisible();
  await expectLabelAtStart(page, 'ltr');
  await fr.selectOption({ label: FA });
  await expect(html(page)).toHaveAttribute('dir', 'rtl');
  await page.getByRole('dialog').getByRole('combobox', { name: 'Language' }).selectOption({ label: 'English' });
  await expect(html(page)).toHaveAttribute('lang', 'en');
  await expect(html(page)).toHaveAttribute('dir', 'ltr');
  await expectLabelAtStart(page, 'ltr');
  // « Langue de l'appareil » : retour au navigateur (français).
  await page.getByRole('dialog').getByRole('combobox', { name: 'Language' }).selectOption('auto');
  await expect(html(page)).toHaveAttribute('lang', 'fr');
  expect(await stored(page, 'kraken.lang')).toBe('auto');
});

// ───────── le repli : persan, puis anglais, puis français ─────────

test.describe('navigateur en persan', () => {
  test.use({ locale: 'fa-IR' });

  test('I18N-F4 « Langue de l’appareil » : navigator.languages = fa-IR donne le persan ; le repli est l’anglais, jamais le français', async ({ signedIn }) => {
    const { page } = await signedIn();
    await expect(html(page)).toHaveAttribute('lang', 'fa');
    await expect(html(page)).toHaveAttribute('dir', 'rtl');
    expect(await stored(page, 'kraken.lang')).toBeNull();
    // PHASE 2 : ces noms accessibles sont en persan ; aujourd'hui l'anglais, le repli de `Lang::Fa`.
    await expect(page.getByRole('navigation', { name: 'Servers' })).toBeVisible();
    await expect(page.getByRole('link', { name: 'Home and direct messages' })).toBeVisible();
    await expect(page.getByRole('link', { name: 'Create or join a server' })).toBeVisible();
    const sidebar = page.getByRole('navigation', { name: 'Direct messages' });
    await expect(sidebar.getByRole('link', { name: 'Friends' })).toBeVisible();
    await expect(sidebar.getByText('No conversations yet.')).toBeVisible();
    // Aucune phrase de la coquille ne reste en français (le français est la dernière marche, pas la première).
    expect(await page.locator('body').innerText()).not.toMatch(/Amis|Messages privés|Serveurs|Créer ou rejoindre|Aucune conversation/);
    await a11y(page);
    // « Langue de l'appareil » est le choix affiché, et le persan est dans la liste.
    await page.goto('/settings/language');
    const language = page.getByRole('dialog').getByRole('combobox', { name: 'Language' });
    await expect(language).toHaveValue('auto');
    await expect(language.locator('option[value="fa"]')).toHaveText(FA);
    await expect(language.locator('option[value="fa"]')).toHaveAttribute('lang', 'fa');
  });

  test('I18N-F5 un choix enregistré passe avant le navigateur : français choisi, la page reste de gauche à droite', async ({ signedIn }) => {
    const { page } = await signedIn();
    await page.evaluate(() => localStorage.setItem('kraken.lang', 'fr'));
    await page.reload();
    await expect(html(page)).toHaveAttribute('lang', 'fr');
    await expect(html(page)).toHaveAttribute('dir', 'ltr');
    await expect(page.getByRole('navigation', { name: 'Serveurs' })).toBeVisible();
  });
});

test('I18N-F6 navigator.languages : fa, fa-IR, fa-AF et fa_IR donnent le persan, la première langue prise en charge gagne, l’arabe et l’hébreu retombent sur le français', async ({ browser, baseURL }) => {
  const cases: [string[], 'fa' | 'fr' | 'en'][] = [
    [['fa'], 'fa'],
    [['fa-IR'], 'fa'],
    [['fa-AF'], 'fa'],
    [['FA-ir'], 'fa'],
    [['fa_IR'], 'fa'],
    [['de-DE', 'fa-IR', 'en'], 'fa'],
    [['en-US', 'fa-IR'], 'en'],
    [['ar-SA', 'he-IL'], 'fr'],
    [['de-DE'], 'fr'],
  ];
  for (const [languages, want] of cases) {
    const { context, page } = await browserSpeaking(browser, baseURL, languages);
    await page.goto('/login');
    await expect(html(page), languages.join()).toHaveAttribute('lang', want);
    await expect(html(page), languages.join()).toHaveAttribute('dir', want === 'fa' ? 'rtl' : 'ltr');
    await expect(page.getByRole('combobox', { name: /^(Langue|Language)$/ })).toHaveValue('auto');
    await context.close();
  }
  // Un choix enregistré et valide passe avant le navigateur ; un choix inconnu est ignoré (« Automatique »).
  const { context, page } = await browserSpeaking(browser, baseURL, ['fa-IR']);
  await page.goto('/login');
  await page.evaluate(() => localStorage.setItem('kraken.lang', 'en'));
  await page.reload();
  await expect(html(page)).toHaveAttribute('lang', 'en');
  await page.evaluate(() => localStorage.setItem('kraken.lang', 'xx'));
  await page.reload();
  await expect(html(page)).toHaveAttribute('lang', 'fa');
  await expect(html(page)).toHaveAttribute('dir', 'rtl');
  await context.close();
});

test('I18N-F7 boot.js pose lang et dir avant le client WASM, avec la même règle que le client', async ({ browser, baseURL }) => {
  const context = await browser.newContext({ baseURL, locale: 'fa-IR' });
  const page = await context.newPage();
  // Le client WASM est bloqué : seul `boot.js` a pu écrire les attributs.
  await page.route('**/kraken-client*', (route) => route.abort());
  await page.goto('/login', { waitUntil: 'domcontentloaded' });
  await expect(html(page)).toHaveAttribute('lang', 'fa');
  await expect(html(page)).toHaveAttribute('dir', 'rtl');
  await page.evaluate(() => localStorage.setItem('kraken.lang', 'en'));
  await page.reload({ waitUntil: 'domcontentloaded' });
  await expect(html(page)).toHaveAttribute('lang', 'en');
  await expect(html(page)).toHaveAttribute('dir', 'ltr');
  await page.evaluate(() => localStorage.setItem('kraken.lang', 'fa'));
  await page.reload({ waitUntil: 'domcontentloaded' });
  await expect(html(page)).toHaveAttribute('lang', 'fa');
  await expect(html(page)).toHaveAttribute('dir', 'rtl');
  await context.close();
});

// ───────── dates, heures, durées ─────────

test('I18N-F8 dates et durées en persan : « 5 دقیقه پیش », « الان », « 1 ساعت و 30 دقیقه », calendrier persan, chiffres latins', async ({ signedIn, api }) => {
  const a = await signedIn('fa-IR');
  const { page } = a;
  const as = await api(a.token);
  const created = await as.post(`${API}/guilds`, { data: { name: 'Calendrier' } });
  expect(created.status(), await created.text()).toBe(201);
  const g = await created.json();
  const text = g.channels.find((c: { kind: string }) => c.kind === 'text').id;
  // Une invitation valable 1 h 30 : l'entrée du journal d'audit porte la durée dans la langue de l'interface.
  const invite = await as.post(`${API}/channels/${text}/invites`, { data: { max_age: 5400 } });
  expect(invite.status(), await invite.text()).toBe(201);

  const open = async () => {
    await page.goto(`/guilds/${g.guild.id}/settings/audit`);
    const rows = page.getByRole('list', { name: 'Audit log events' }).getByRole('listitem'); // PHASE 2 : « Événements du journal » en persan
    await expect(rows.first()).toBeVisible();
    return rows;
  };

  // Moins d'une minute après : « الان ».
  await page.clock.setFixedTime(Date.now() + 5_000);
  let rows = await open();
  const when = rows.first().locator('time');
  await expect(when.locator('[aria-hidden="true"]')).toHaveText('الان');
  // Durée dans la phrase (l'anglais autour, le persan dedans, la valeur isolée) ; chiffres latins.
  await expect(rows.first()).toContainText(/1 ساعت و 30 دقیقه/); // PHASE 2 : la phrase persane entière
  // La date complète (infobulle et lecteur d'écran) : calendrier persan (l'année est 14xx, pas 20xx), mois en persan, chiffres
  // latins. L'ordre « année mois jour, jour de la semaine » est celui que le calendrier persan donne à `Intl` (CLDR).
  const full = (await when.locator('.k-sr').textContent()) ?? '';
  expect(full).toMatch(/\b14\d{2} [\u0600-\u06FF]+ \d{1,2}\b/);
  expect(full).toMatch(/[\u0600-\u06FF]+\u2069? at \d/); // PHASE 2 : « at » (repli anglais de « à ») devient le mot persan
  expect(full).not.toMatch(INDIC);
  expect(full).toMatch(/\d{1,2}:\d{2}/);
  expect(await when.getAttribute('title')).toBe(full);
  expect(full).not.toMatch(/\b(AM|PM)\b/); // 24 h

  // Cinq minutes et dix secondes plus tard : « 5 دقیقه پیش ».
  await page.clock.setFixedTime(Date.now() + 5 * 60_000 + 10_000);
  rows = await open();
  await expect(rows.first().locator('time [aria-hidden="true"]')).toHaveText('5 دقیقه پیش');
  // Quarante minutes : « 40 دقیقه پیش » ; au-delà d'une heure, l'heure du jour.
  await page.clock.setFixedTime(Date.now() + 40 * 60_000);
  rows = await open();
  await expect(rows.first().locator('time [aria-hidden="true"]')).toHaveText('40 دقیقه پیش');
  await page.clock.setFixedTime(Date.now() + 90 * 60_000);
  rows = await open();
  await expect(rows.first().locator('time [aria-hidden="true"]')).toHaveText(/^\d{2}:\d{2}$/);
  expect(await page.locator('body').innerText()).not.toMatch(INDIC);

  // La carte de profil : « Membre depuis » est une date longue, du calendrier persan, en chiffres latins.
  await page.clock.setFixedTime(Date.now() + 5_000);
  await page.goto(`/channels/${g.guild.id}/${text}`);
  await page.getByRole('button', { name: new RegExp(`@${a.name}`) }).click();
  const since = page.locator('.app-card-dates dd').first();
  await expect(since).toHaveText(/^\d{1,2} [\u0600-\u06FF]+ 14\d{2}$/);
  expect(await since.textContent()).not.toMatch(INDIC);
});

// ───────── petits écrans, grands textes ─────────

test.describe('390 px, texte 20, persan', () => {
  test.use({ locale: 'fa-IR', viewport: { width: 390, height: 844 } });

  test('I18N-F9 rien ne déborde en persan : connexion, inscription, coquille et Réglages ; axe sans violation', async ({ page, signedIn }) => {
    await page.goto('/login');
    await page.evaluate(() => localStorage.setItem('kraken.text', '20'));
    for (const path of ['/login', '/register', '/forgot-password']) {
      await page.goto(path);
      await expect(html(page)).toHaveAttribute('dir', 'rtl');
      await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
      await noScroll(page);
    }
    await page.goto('/login');
    await a11y(page);
    const { page: app } = await signedIn('fa-IR', { width: 390, height: 844 });
    await app.evaluate(() => localStorage.setItem('kraken.text', '20'));
    await app.goto('/settings/language');
    await expect(html(app)).toHaveAttribute('lang', 'fa');
    await expect(app.getByRole('combobox', { name: 'Language' })).toBeVisible(); // PHASE 2 : « زبان »
    await noScroll(app);
    await a11y(app);
    // La barre d'onglets défile dans le sens de la page : l'onglet ouvert (le dernier) est dans la vue.
    const tab = app.locator('.k-settings nav a[aria-current="page"]');
    await expect(tab).toBeInViewport({ ratio: 1 });
    await expectLabelAtStart(app, 'rtl');
    await app.goto('/settings/appearance');
    await noScroll(app);
    await app.goto('/channels/@me');
    await expect(app.locator('#main')).toBeVisible();
    await noScroll(app);
    await a11y(app);
  });
});

test('I18N-F10 coquille en persan à 1440 px : rail à droite, axe sans violation, aucun débordement, polices persanes', async ({ signedIn }) => {
  const { page } = await signedIn('fa-IR');
  await expect(html(page)).toHaveAttribute('dir', 'rtl');
  await noScroll(page);
  await a11y(page);
  // Le rail des serveurs est au bord de début de la page, donc à droite.
  const rail = await page.getByRole('navigation', { name: 'Servers' }).boundingBox(); // PHASE 2 : « سرورها »
  expect(rail).not.toBeNull();
  expect(rail!.x + rail!.width).toBeGreaterThan(1440 - 80);
  // `:lang(fa)` : la pile de polices persane s'applique (Inter d'abord pour le latin, puis une police du système).
  const family = await page.evaluate(() => getComputedStyle(document.body).fontFamily);
  expect(family).toMatch(/Inter/);
  expect(family.toLowerCase()).toMatch(/tahoma|segoe|noto|geeza|arabic/);
});

// ───────── créer un serveur : la langue part avec la demande ─────────

test('I18N-F11 créer un serveur en persan : la demande porte lang="fa", les noms du modèle sont les anglais tant que les noms persans manquent', async ({ signedIn, api }) => {
  const a = await signedIn('fa-IR');
  const { page } = a;
  // Par l'interface : la langue de l'interface part avec la demande.
  await page.getByRole('link', { name: 'Create or join a server' }).click();
  await page.getByLabel('Server name').fill('Abyss');
  const request = page.waitForRequest((r) => r.method() === 'POST' && r.url().endsWith(`${API}/guilds`));
  await page.getByRole('button', { name: 'Create server' }).click();
  expect((await request).postDataJSON().lang).toBe('fa');
  await page.waitForURL(/\/channels\/[0-9a-f-]{36}\/[0-9a-f-]{36}$/);
  // PHASE 2 (noms persans du modèle « Vide ») : l'anglais « general » devient le nom persan prévu dans `templates.rs`.
  await expect(page.getByRole('heading', { name: 'general', level: 1 })).toBeVisible();
  // Par l'API : une étiquette régionale (fa-IR) est comprise, une langue inconnue donne le français.
  const as = await api(a.token);
  const names = async (lang: string) => {
    const res = await as.post(`${API}/guilds`, { data: { name: 'Abyss', template: 'communaute', lang } });
    expect(res.status(), await res.text()).toBe(201);
    const g = await res.json();
    return g.channels.filter((c: { kind: string }) => c.kind === 'text').map((c: { name: string }) => c.name).sort();
  };
  expect(await names('fa-IR')).toEqual(['announcements', 'general', 'help', 'rules']); // PHASE 2 : les noms persans
  expect(await names('fr')).toEqual(['aide', 'annonces', 'général', 'règles']);
  expect(await names('de')).toEqual(['aide', 'annonces', 'général', 'règles']);
});

// ───────── le français et l'anglais ne changent pas ─────────

test('I18N-F12 le français et l’anglais restent de gauche à droite, sans une lettre persane hors du sélecteur', async ({ browser, baseURL }) => {
  for (const [locale, heading, dir] of [
    ['fr-FR', 'Bon retour sous la surface', 'ltr'],
    ['en-US', 'Welcome back', 'ltr'],
    ['en-GB', 'Welcome back', 'ltr'],
  ] as const) {
    const context = await browser.newContext({ baseURL, locale });
    const page = await context.newPage();
    await page.goto('/login');
    await expect(page.getByRole('heading', { name: heading, level: 1 })).toBeVisible();
    await expect(html(page)).toHaveAttribute('dir', dir);
    await expect(html(page)).not.toHaveAttribute('lang', 'fa');
    const body = await page.locator('main, #main, body').first().innerText();
    expect(body.replace(FA, '')).not.toMatch(ARABIC_SCRIPT);
    await expectLabelAtStart(page, 'ltr');
    await context.close();
  }
});
