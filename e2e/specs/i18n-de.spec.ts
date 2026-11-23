// Allemand (PR #35) : la quatrième langue de l'interface, de gauche à droite. Cette première phase pose l'infrastructure : la
// langue, son sélecteur, `boot.js`, le repli, les durées et dates, les e-mails du serveur, la mise en page ; le catalogue
// (`i18n/de/*.tsv`, six fichiers) est écrit en phase 2. Tant qu'il est vide, l'interface allemande s'affiche en ANGLAIS
// (la chaîne de repli `de → en → fr`) : ce qui est du code (`<html lang dir>`, durées « vor 5 Min. », dates en `de-u-nu-latn`)
// est déjà allemand, ce qui est du catalogue ne l'est pas encore.
//
// PHASE 2 : les noms accessibles et les phrases lus en allemand sont rassemblés dans `UI` ci-dessous (anglais pour le moment) ;
// chaque ligne marquée `// PHASE 2` change quand le catalogue est livré (et `I18N-D9`, la couverture, s'ajoute alors, sur le
// modèle de `I18N-F13` de `i18n-fa.spec.ts` : aucun texte français visible).
import { test as base, expect, a11y, watch, API, PASSWORD, unique } from '../fixtures';
import type { Browser, BrowserContext, Locator, Page } from '@playwright/test';

type Account = { page: Page; context: BrowserContext; name: string; token: string };
type Viewport = { width: number; height: number };

/**
 * `signedIn(locale?, viewport?)` : un compte neuf, connecté dans un navigateur de cette langue, sans attendre un titre de page
 * (la fixture `user` attend « Amis », écrit en français : ces tests changent la langue). Même fixture que `i18n-fa.spec.ts`.
 */
const test = base.extend<{ signedIn: (locale?: string, viewport?: Viewport) => Promise<Account> }>({
  signedIn: async ({ browser, baseURL, issues, locale: browserLocale }, use) => {
    const contexts: BrowserContext[] = [];
    await use(async (locale = browserLocale ?? 'fr-FR', viewport = { width: 1440, height: 900 }) => {
      const context = await browser.newContext({ baseURL, locale, viewport });
      contexts.push(context);
      const name = unique('i18nd');
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

const DE = 'Deutsch';
/**
 * PHASE 2 : ce que l'interface allemande écrit (noms accessibles et phrases du catalogue). Anglais tant que `de/*.tsv` est vide
 * (repli) ; à remplacer par l'allemand, une valeur à la fois.
 */
const UI = {
  language: 'Language', // PHASE 2 : « Sprache »
  languageIs: /^Language: Deutsch\.$/, // PHASE 2 : « Sprache: Deutsch. »
  deviceLanguage: 'Device language', // PHASE 2 : le premier choix du sélecteur
  welcome: 'Welcome back', // PHASE 2 : le titre de la connexion
  login: 'Username or email', // PHASE 2 : le champ identifiant de la connexion
  password: 'Password', // PHASE 2 : « Passwort »
  settings: 'Settings', // PHASE 2 : « Einstellungen »
  example: /^Example: 14:30$/, // PHASE 2 : « Beispiel: 14:30 »
  createLink: 'Create or join a server', // PHASE 2 : le lien du rail
  serverName: 'Server name', // PHASE 2 : le champ de la page de création
  createServer: 'Create server', // PHASE 2 : le bouton de la page de création
  validFor: /Valid for 1 Std\. 30 Min\./, // PHASE 2 : « Gültig für 1 Std. 30 Min. » (la durée est déjà allemande : `span::duration`)
  email: 'Email', // PHASE 2 : « E-Mail »
  sendLink: 'Send link', // PHASE 2 : le bouton de « Mot de passe oublié »
  username: 'Username', // PHASE 2 : « Benutzername »
  createAccount: 'Create my account', // PHASE 2 : le bouton d'inscription
};

const html = (p: Page) => p.locator('html');
const stored = (p: Page, key: string) => p.evaluate((k) => localStorage.getItem(k), key);
const noScroll = async (p: Page) =>
  expect(await p.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(p.viewportSize()!.width);
/** La zone d'état qui dit ce texte (la coquille et la page de langue en ont chacune une : on filtre toujours). */
const said = (p: Page, text: string | RegExp) => p.getByRole('status').filter({ hasText: text });
/** Les options du sélecteur, sans « Langue de l'appareil » (son texte suit la langue de l'interface). */
const choices = async (select: Locator) => (await select.locator('option').allTextContents()).slice(1);
const codes = (select: Locator) =>
  select.locator('option').evaluateAll((o) => o.map((x) => x.getAttribute('lang')));

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

test('I18N-D1 connexion : « Deutsch » est proposé avec lang="de", le choix recharge en lang=de dir=ltr, l’identifiant reste, le mot de passe non ; retour au français', async ({ page }) => {
  await page.goto('/login');
  const language = page.getByRole('combobox', { name: 'Langue' });
  await expect(language).toHaveValue('auto'); // le client est monté : les options sont là
  expect(await language.locator('option').allTextContents()).toEqual(["Langue de l'appareil", 'Français', 'English', 'فارسی', DE]);
  expect(await codes(language)).toEqual([null, 'fr', 'en', 'fa', 'de']);
  await expect(html(page)).toHaveAttribute('dir', 'ltr');

  await page.getByLabel('Pseudo ou e-mail').fill('perle@example.org');
  await page.getByLabel('Mot de passe').fill('un-secret-bien-gardé');
  await language.selectOption({ label: DE });
  await expect(html(page)).toHaveAttribute('lang', 'de');
  await expect(html(page)).toHaveAttribute('dir', 'ltr');
  expect(await stored(page, 'kraken.lang')).toBe('de');
  // PHASE 2 : le titre, les champs et le sélecteur sont en allemand ; jusque-là, l'anglais du repli.
  await expect(page.getByRole('heading', { name: UI.welcome, level: 1 })).toBeVisible();
  await expect(page.getByLabel(UI.login)).toHaveValue('perle@example.org');
  await expect(page.getByLabel(UI.password, { exact: true })).toHaveValue('');
  const after = page.getByRole('combobox', { name: UI.language });
  await expect(after).toHaveValue('de');
  await expect(after).toBeFocused();
  // Le nom de chaque langue s'écrit dans son écriture, quelle que soit la langue de la page.
  expect(await choices(after)).toEqual(['Français', 'English', 'فارسی', DE]);
  expect(await codes(after)).toEqual([null, 'fr', 'en', 'fa', 'de']);
  expect(await page.evaluate(() => Object.entries(sessionStorage))).toEqual([]);
  await noScroll(page);
  await a11y(page);

  // Dans l'autre sens : retour au français, la page reste de gauche à droite.
  await after.selectOption({ label: 'Français' });
  await expect(html(page)).toHaveAttribute('lang', 'fr');
  await expect(html(page)).toHaveAttribute('dir', 'ltr');
  await expect(page.getByRole('heading', { name: 'Bon retour sous la surface', level: 1 })).toBeVisible();
  await expect(page.getByRole('combobox', { name: 'Langue' })).toHaveValue('fr');
  expect(await stored(page, 'kraken.lang')).toBe('fr');
});

test('I18N-D1b inscription et mot de passe oublié en allemand : lang=de dir=ltr, sélecteur en place, axe sans violation', async ({ page }) => {
  await page.goto('/login');
  await page.evaluate(() => localStorage.setItem('kraken.lang', 'de'));
  for (const path of ['/register', '/forgot-password', '/login']) {
    await page.goto(path);
    await expect(html(page)).toHaveAttribute('lang', 'de');
    await expect(html(page)).toHaveAttribute('dir', 'ltr');
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
    await noScroll(page);
    await a11y(page);
  }
});

test('I18N-D1c Réglages › Langue et heure : choisir « Deutsch » recharge la page, rend le focus et dit la langue ; l’heure suit la langue (24 h) ; retour au français', async ({ signedIn }) => {
  const { page } = await signedIn('fr-FR');
  await page.goto('/settings/language');
  const dialog = page.getByRole('dialog');
  const language = dialog.getByRole('combobox', { name: 'Langue' });
  await expect(language).toHaveValue('auto');
  expect(await language.locator('option').allTextContents()).toEqual(["Langue de l'appareil", 'Français', 'English', 'فارسی', DE]);

  await language.selectOption({ label: DE });
  await expect(html(page)).toHaveAttribute('lang', 'de');
  await expect(html(page)).toHaveAttribute('dir', 'ltr');
  expect(await stored(page, 'kraken.lang')).toBe('de');
  expect(page.url()).toContain('/settings/language');
  const after = page.getByRole('dialog').getByRole('combobox', { name: UI.language });
  await expect(after).toHaveValue('de');
  await expect(after).toBeFocused();
  // La phrase d'état dit la langue, nom de la langue dans son écriture. PHASE 2 : « Sprache: Deutsch. »
  await expect(said(page, DE)).toHaveText(UI.languageIs);
  // L'heure automatique suit la langue de l'interface, pas celle du navigateur (français ici) : 24 h.
  // PHASE 2 : « Beispiel: 14:30 »
  await expect(said(page, '14:30').first()).toHaveText(UI.example);
  await noScroll(page);
  await a11y(page);
  // Le choix reste après un rechargement, sans nouvelle phrase ni focus volé.
  await page.reload();
  await expect(html(page)).toHaveAttribute('lang', 'de');
  await expect(after).toHaveValue('de');
  await expect(after).not.toBeFocused();
  await expect(said(page, DE)).toHaveCount(0);

  // Dans l'autre sens : le français, puis l'anglais, puis « Langue de l'appareil » (navigateur français).
  await after.selectOption({ label: 'Français' });
  await expect(html(page)).toHaveAttribute('lang', 'fr');
  await expect(html(page)).toHaveAttribute('dir', 'ltr');
  const fr = page.getByRole('dialog', { name: 'Réglages' }).getByRole('combobox', { name: 'Langue' });
  await expect(fr).toHaveValue('fr');
  await expect(fr).toBeFocused();
  await expect(said(page, 'Langue : Français.')).toBeVisible();
  await fr.selectOption({ label: DE });
  await expect(html(page)).toHaveAttribute('lang', 'de');
  await page.getByRole('dialog').getByRole('combobox', { name: UI.language }).selectOption({ label: 'English' });
  await expect(html(page)).toHaveAttribute('lang', 'en');
  await page.getByRole('dialog').getByRole('combobox', { name: 'Language' }).selectOption('auto');
  await expect(html(page)).toHaveAttribute('lang', 'fr');
  expect(await stored(page, 'kraken.lang')).toBe('auto');
});

// ───────── le navigateur choisit la langue ─────────

test('I18N-D2 navigator.languages : de, de-DE, de-AT, de-CH et de_DE donnent l’allemand, la première langue prise en charge gagne, une autre langue retombe sur le français', async ({ browser, baseURL }) => {
  const cases: [string[], 'de' | 'fr' | 'en' | 'fa'][] = [
    [['de'], 'de'],
    [['de-DE'], 'de'],
    [['de-AT'], 'de'],
    [['de-CH'], 'de'],
    [['de_DE'], 'de'],
    [['DE-at'], 'de'],
    [['ja-JP', 'de-AT', 'en'], 'de'],
    [['de-DE', 'fa-IR'], 'de'],
    [['fa-IR', 'de-DE'], 'fa'],
    [['en-US', 'de-DE'], 'en'],
    [['fr-FR', 'de-DE'], 'fr'],
    [['nl-NL', 'da-DK'], 'fr'],
    [['deu'], 'fr'],
  ];
  for (const [languages, want] of cases) {
    const { context, page } = await browserSpeaking(browser, baseURL, languages);
    await page.goto('/login');
    await expect(html(page), languages.join()).toHaveAttribute('lang', want);
    await expect(html(page), languages.join()).toHaveAttribute('dir', want === 'fa' ? 'rtl' : 'ltr');
    await expect(page.getByRole('combobox', { name: /^(Langue|Language|زبان|Sprache)$/ })).toHaveValue('auto');
    await context.close();
  }
  // Un choix enregistré et valide passe avant le navigateur ; un choix inconnu est ignoré (« Automatique »).
  const { context, page } = await browserSpeaking(browser, baseURL, ['de-AT']);
  await page.goto('/login');
  await page.evaluate(() => localStorage.setItem('kraken.lang', 'en'));
  await page.reload();
  await expect(html(page)).toHaveAttribute('lang', 'en');
  await page.evaluate(() => localStorage.setItem('kraken.lang', 'xx'));
  await page.reload();
  await expect(html(page)).toHaveAttribute('lang', 'de');
  await expect(html(page)).toHaveAttribute('dir', 'ltr');
  await context.close();
});

test.describe('navigateur en allemand', () => {
  test.use({ locale: 'de-AT' });

  test('I18N-D2b « Langue de l’appareil » : un navigateur de-AT donne l’allemand, et « Deutsch » est le choix d’une liste qui garde « Auto » en tête', async ({ signedIn }) => {
    const { page } = await signedIn();
    await expect(html(page)).toHaveAttribute('lang', 'de');
    await expect(html(page)).toHaveAttribute('dir', 'ltr');
    expect(await stored(page, 'kraken.lang')).toBeNull();
    await page.goto('/settings/language');
    const language = page.getByRole('dialog').getByRole('combobox', { name: UI.language });
    await expect(language).toHaveValue('auto');
    await expect(language.locator('option[value="de"]')).toHaveText(DE);
    await expect(language.locator('option[value="de"]')).toHaveAttribute('lang', 'de');
    // Un choix enregistré passe avant le navigateur : français choisi, la page reste en français.
    await page.evaluate(() => localStorage.setItem('kraken.lang', 'fr'));
    await page.reload();
    await expect(html(page)).toHaveAttribute('lang', 'fr');
    await expect(page.getByRole('dialog').getByRole('combobox', { name: 'Langue' })).toHaveValue('fr');
  });
});

test('I18N-D3 boot.js pose lang et dir avant le client WASM, avec la même règle que le client', async ({ browser, baseURL }) => {
  for (const locale of ['de-DE', 'de-AT', 'de-CH']) {
    const context = await browser.newContext({ baseURL, locale });
    const page = await context.newPage();
    // Le client WASM est bloqué : seul `boot.js` a pu écrire les attributs.
    await page.route('**/kraken-client*', (route) => route.abort());
    await page.goto('/login', { waitUntil: 'domcontentloaded' });
    await expect(html(page), locale).toHaveAttribute('lang', 'de');
    await expect(html(page), locale).toHaveAttribute('dir', 'ltr');
    await context.close();
  }
  const context = await browser.newContext({ baseURL, locale: 'de-DE' });
  const page = await context.newPage();
  await page.route('**/kraken-client*', (route) => route.abort());
  await page.goto('/login', { waitUntil: 'domcontentloaded' });
  await page.evaluate(() => localStorage.setItem('kraken.lang', 'fa'));
  await page.reload({ waitUntil: 'domcontentloaded' });
  await expect(html(page)).toHaveAttribute('lang', 'fa');
  await expect(html(page)).toHaveAttribute('dir', 'rtl');
  await page.evaluate(() => localStorage.setItem('kraken.lang', 'fr'));
  await page.reload({ waitUntil: 'domcontentloaded' });
  await expect(html(page)).toHaveAttribute('lang', 'fr');
  await expect(html(page)).toHaveAttribute('dir', 'ltr');
  await page.evaluate(() => localStorage.setItem('kraken.lang', 'de'));
  await page.reload({ waitUntil: 'domcontentloaded' });
  await expect(html(page)).toHaveAttribute('lang', 'de');
  await expect(html(page)).toHaveAttribute('dir', 'ltr');
  await context.close();
});

// ───────── dates, heures, durées ─────────

test('I18N-D4 dates et durées en allemand : « gerade eben », « vor 5 Min. », « 1 Std. 30 Min. », dates en de-u-nu-latn, chiffres latins, 24 h', async ({ signedIn, api }) => {
  const a = await signedIn('de-DE');
  const { page } = a;
  await expect(html(page)).toHaveAttribute('lang', 'de');
  const as = await api(a.token);
  const created = await as.post(`${API}/guilds`, { data: { name: 'Kalender' } });
  expect(created.status(), await created.text()).toBe(201);
  const g = await created.json();
  const text = g.channels.find((c: { kind: string }) => c.kind === 'text').id;
  // Une invitation valable 1 h 30 : l'entrée du journal d'audit porte la durée dans la langue de l'interface.
  const invite = await as.post(`${API}/channels/${text}/invites`, { data: { max_age: 5400 } });
  expect(invite.status(), await invite.text()).toBe(201);

  const open = async () => {
    await page.goto(`/guilds/${g.guild.id}/settings/audit`);
    const rows = page.locator('.app-audit-list .app-audit-row');
    await expect(rows.first()).toBeVisible();
    return rows;
  };

  // Moins d'une minute après : « gerade eben ».
  await page.clock.setFixedTime(Date.now() + 5_000);
  let rows = await open();
  const when = rows.first().locator('time');
  await expect(when.locator('[aria-hidden="true"]')).toHaveText('gerade eben');
  // Durée dans la phrase, écrite par `span::duration` (code, pas catalogue) : déjà allemande. PHASE 2 : « Gültig für … ».
  await expect(rows.first()).toContainText(UI.validFor);
  // La date complète (infobulle et lecteur d'écran) : `Intl` en de-u-nu-latn, « Montag, 23. November 2026 », chiffres latins, 24 h.
  const full = (await when.locator('.k-sr').textContent()) ?? '';
  expect(full).toMatch(/^(Montag|Dienstag|Mittwoch|Donnerstag|Freitag|Samstag|Sonntag), \d{1,2}\. (Januar|Februar|März|April|Mai|Juni|Juli|August|September|Oktober|November|Dezember) \d{4}/);
  expect(full).toMatch(/\b\d{1,2}:\d{2}$/); // PHASE 2 : « … um 14:30 » (jusque-là « … at 14:30 »)
  expect(full).not.toMatch(/\b(AM|PM)\b/); // 24 h
  expect(full).not.toMatch(/[٠-٩۰-۹]/);
  expect(await when.getAttribute('title')).toBe(full);

  // Cinq minutes et dix secondes plus tard : « vor 5 Min. ».
  await page.clock.setFixedTime(Date.now() + 5 * 60_000 + 10_000);
  rows = await open();
  await expect(rows.first().locator('time [aria-hidden="true"]')).toHaveText('vor 5 Min.');
  // Quarante minutes : « vor 40 Min. » ; au-delà d'une heure, l'heure du jour sur 24 h.
  await page.clock.setFixedTime(Date.now() + 40 * 60_000);
  rows = await open();
  await expect(rows.first().locator('time [aria-hidden="true"]')).toHaveText('vor 40 Min.');
  await page.clock.setFixedTime(Date.now() + 90 * 60_000);
  rows = await open();
  await expect(rows.first().locator('time [aria-hidden="true"]')).toHaveText(/^\d{2}:\d{2}$/);
  expect(await page.locator('body').innerText()).not.toMatch(/[٠-٩۰-۹]/);

  // La carte de profil : « Membre depuis » est une date longue du calendrier grégorien, en chiffres latins.
  await page.clock.setFixedTime(Date.now() + 5_000);
  await page.goto(`/channels/${g.guild.id}/${text}`);
  await page.getByRole('button', { name: new RegExp(`@${a.name}`) }).click();
  const since = page.locator('.app-card-dates dd').first();
  await expect(since).toHaveText(/^\d{1,2}\. (Januar|Februar|März|April|Mai|Juni|Juli|August|September|Oktober|November|Dezember) \d{4}$/);
});

test('I18N-D4b l’heure en « Automatique » suit l’allemand, pas le navigateur : 24 h même avec un navigateur américain', async ({ signedIn }) => {
  const { page } = await signedIn('en-US');
  // Navigateur américain : 12 h tant que l'interface est anglaise.
  await page.goto('/settings/language');
  await expect(said(page, 'Example').first()).toHaveText('Example: 2:30 PM');
  await page.getByRole('dialog').getByRole('combobox', { name: 'Language' }).selectOption({ label: DE });
  await expect(html(page)).toHaveAttribute('lang', 'de');
  // PHASE 2 : « Beispiel: 14:30 »
  await expect(said(page, '14:30').first()).toHaveText(UI.example);
  expect(await page.locator('body').innerText()).not.toMatch(/\b(AM|PM)\b/);
});

// ───────── petits écrans, grands textes ─────────

test.describe('390 px, texte 20, allemand', () => {
  test.use({ locale: 'de-DE', viewport: { width: 390, height: 844 } });

  test('I18N-D5 rien ne déborde en allemand : connexion, inscription, coquille et Réglages ; axe sans violation', async ({ page, signedIn }) => {
    await page.goto('/login');
    await page.evaluate(() => localStorage.setItem('kraken.text', '20'));
    for (const path of ['/login', '/register', '/forgot-password']) {
      await page.goto(path);
      await expect(html(page)).toHaveAttribute('lang', 'de');
      await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
      await noScroll(page);
    }
    await page.goto('/login');
    await a11y(page);
    const { page: app } = await signedIn('de-DE', { width: 390, height: 844 });
    await app.evaluate(() => localStorage.setItem('kraken.text', '20'));
    await app.goto('/settings/language');
    await expect(html(app)).toHaveAttribute('lang', 'de');
    await expect(app.getByRole('combobox', { name: UI.language })).toBeVisible();
    await noScroll(app);
    await a11y(app);
    // La barre d'onglets défile : l'onglet ouvert (le dernier) est dans la vue.
    await expect(app.locator('.k-settings nav a[aria-current="page"]')).toBeInViewport({ ratio: 1 });
    for (const path of ['/settings/appearance', '/settings/accessibility', '/settings/account']) {
      await app.goto(path);
      await expect(app.getByRole('dialog')).toBeVisible();
      await noScroll(app);
    }
    await app.goto('/channels/@me');
    await expect(app.locator('#main')).toBeVisible();
    await noScroll(app);
    await a11y(app);
    // PHASE 2 : refaire ce parcours avec les vrais textes allemands (mots composés longs) et regarder chaque bouton, onglet et
    // pastille : rien ne doit dépasser de sa carte, à 390 px comme à texte 20.
  });
});

// ───────── la mise en page : césure et libellés étroits ─────────

test('I18N-D8 mise en page allemande : césure automatique du texte courant, jamais des champs ni du code, boutons qui passent à la ligne ; le français ne change pas', async ({ browser, baseURL }) => {
  const read = (page: Page) =>
    page.evaluate(() => {
      const css = (sel: string) => {
        const el = document.querySelector(sel);
        return el ? getComputedStyle(el) : null;
      };
      const hint = css('p.app-hint');
      const field = css('input');
      const select = css('select');
      const button = css('button.k-btn:not(.k-btn--icon)');
      return {
        hint: hint && { hyphens: hint.hyphens, wrap: hint.whiteSpace },
        field: field && field.hyphens,
        select: select && select.hyphens,
        button: button && { whiteSpace: button.whiteSpace, wrap: button.overflowWrap, height: button.height },
      };
    });
  const probe = async (lang: string) => {
    const context = await browser.newContext({ baseURL, locale: 'fr-FR' });
    const page = await context.newPage();
    await page.goto('/login');
    await page.evaluate((l) => localStorage.setItem('kraken.lang', l), lang);
    await page.reload();
    await expect(html(page)).toHaveAttribute('lang', lang);
    await expect(page.locator('p.app-hint').first()).toBeVisible();
    const css = await read(page);
    await context.close();
    return css;
  };
  const de = await probe('de');
  expect(de.hint?.hyphens).toBe('auto');
  expect(de.field).toBe('manual');
  expect(de.select).toBe('manual');
  expect(de.button?.whiteSpace).toBe('normal');
  expect(de.button?.wrap).toBe('anywhere');
  // Un bouton à une ligne garde sa hauteur d'avant (40 px au moins ; le texte a la place).
  expect(parseFloat(de.button!.height)).toBeGreaterThanOrEqual(40);
  // Français et anglais : rien ne change (pas de césure automatique, bouton sur une ligne).
  for (const lang of ['fr', 'en']) {
    const other = await probe(lang);
    expect(other.hint?.hyphens, lang).toBe('manual');
    expect(other.button?.whiteSpace, lang).toBe('nowrap');
    expect(other.button?.height, lang).toBe('40px');
  }
});

// ───────── créer un serveur : la langue part avec la demande ─────────

test('I18N-D6 créer un serveur en allemand : la demande porte lang="de", une étiquette régionale est comprise, une langue inconnue donne le français', async ({ signedIn, api }) => {
  const a = await signedIn('de-DE');
  const { page } = a;
  await expect(html(page)).toHaveAttribute('lang', 'de');
  // Par l'interface : la langue de l'interface part avec la demande. PHASE 2 : les libellés allemands de la page de création.
  await page.getByRole('link', { name: UI.createLink }).click();
  await page.getByLabel(UI.serverName, { exact: true }).fill('Abyss');
  const request = page.waitForRequest((r) => r.method() === 'POST' && r.url().endsWith(`${API}/guilds`));
  await page.getByRole('button', { name: UI.createServer, exact: true }).click();
  expect((await request).postDataJSON().lang).toBe('de');
  await page.waitForURL(/\/channels\/[0-9a-f-]{36}\/[0-9a-f-]{36}$/);

  // Par l'API : `de`, `de-AT`, `DE_ch` sont l'allemand, une langue inconnue donne le français.
  const as = await api(a.token);
  const channels = async (lang: string) => {
    const res = await as.post(`${API}/guilds`, { data: { name: 'Abyss', template: 'communaute', lang } });
    expect(res.status(), await res.text()).toBe(201);
    const g = await res.json();
    const of = (kind: string) => g.channels.filter((c: { kind: string }) => c.kind === kind).map((c: { name: string }) => c.name).sort();
    return { text: of('text'), voice: of('voice'), category: of('category') };
  };
  // Les noms de salons et de catégories du modèle « Communauté » n'ont pas encore de nom allemand : l'anglais (le repli).
  // PHASE 2 : les noms allemands de `templates.rs` (`.de("…")`), à écrire ici en toutes lettres.
  const english = await channels('en');
  expect(english.text).toEqual(['announcements', 'general', 'help', 'rules']);
  for (const lang of ['de', 'de-AT', 'DE_ch']) expect(await channels(lang), lang).toEqual(english);
  expect(await channels('nl')).toEqual(await channels('fr'));
  expect((await channels('fr')).text).toEqual(['aide', 'annonces', 'général', 'règles']);
});

// ───────── e-mails du serveur : la langue de l'interface part avec la demande ─────────

test('I18N-D7 e-mails du serveur : l’inscription et le mot de passe oublié envoient lang="de", l’API accepte de, de-AT et de_CH', async ({ page, api, allow }) => {
  // Le serveur de test n'a pas de SMTP : « mot de passe oublié » répond 501 (le message de l'interface le dit), c'est attendu.
  allow.push(/auth\/reset/, /status of 5\d\d/);
  await page.goto('/login');
  await page.evaluate(() => localStorage.setItem('kraken.lang', 'de'));

  // « Mot de passe oublié » : la demande porte lang="de". PHASE 2 : libellés allemands.
  await page.goto('/forgot-password');
  await expect(html(page)).toHaveAttribute('lang', 'de');
  await page.getByLabel(UI.email, { exact: true }).fill('personne@example.org');
  const reset = page.waitForRequest((r) => r.method() === 'POST' && r.url().endsWith(`${API}/auth/reset`));
  await page.getByRole('button', { name: UI.sendLink, exact: true }).click();
  expect((await reset).postDataJSON()).toEqual({ email: 'personne@example.org', lang: 'de' });

  // L'inscription : la demande porte lang="de" (le compte est créé, le serveur de test n'envoie rien).
  const name = unique('mailde');
  await page.goto('/register');
  await page.getByLabel(UI.email, { exact: true }).fill(`${name}@example.org`);
  await page.getByRole('textbox', { name: UI.username }).fill(name);
  await page.getByLabel(UI.password, { exact: true }).fill(PASSWORD);
  const register = page.waitForRequest((r) => r.method() === 'POST' && r.url().endsWith(`${API}/auth/register`));
  await page.getByRole('button', { name: UI.createAccount, exact: true }).click();
  expect((await register).postDataJSON()).toEqual({ username: name, email: `${name}@example.org`, password: PASSWORD, lang: 'de' });
  await page.waitForURL('**/channels/@me');

  // Par l'API : `lang` allemand, avec ou sans région, est accepté (l'e-mail part en allemand ; ici, la file seulement).
  const anon = await api();
  for (const lang of ['de', 'de-AT', 'de_CH', 'DE-de']) {
    const user = unique('mailde');
    const res = await anon.post(`${API}/auth/register`, { data: { username: user, email: `${user}@example.org`, password: PASSWORD, lang } });
    expect(res.status(), `${lang} : ${await res.text()}`).toBe(201);
  }
  const noMail = await anon.post(`${API}/auth/reset`, { data: { email: 'personne@example.org', lang: 'de-AT' } });
  expect(noMail.status()).toBe(501);
  expect((await noMail.json()).code).toBe('no_email');
  const user = unique('mailde');
  const made = await anon.post(`${API}/auth/register`, { data: { username: user, email: `${user}@example.org`, password: PASSWORD, lang: 'de' } });
  const { token } = await made.json();
  const as = await api(token);
  const changed = await as.put(`${API}/users/@me/email`, { data: { email: `${user}.2@example.org`, password: PASSWORD, lang: 'de-CH' } });
  expect(changed.status(), await changed.text()).toBe(200);
});

// ───────── le français, l'anglais et le persan ne changent pas ─────────

test('I18N-D10 l’allemand ne change rien au français, à l’anglais ni au persan : le sens d’écriture et les titres restent ceux d’avant', async ({ browser, baseURL }) => {
  for (const [locale, heading, lang, dir] of [
    ['fr-FR', 'Bon retour sous la surface', 'fr', 'ltr'],
    ['en-US', 'Welcome back', 'en', 'ltr'],
    ['en-GB', 'Welcome back', 'en', 'ltr'],
    ['fa-IR', 'خوش برگشتید', 'fa', 'rtl'],
  ] as const) {
    const context = await browser.newContext({ baseURL, locale });
    const page = await context.newPage();
    await page.goto('/login');
    await expect(page.getByRole('heading', { name: heading, level: 1 })).toBeVisible();
    await expect(html(page)).toHaveAttribute('lang', lang);
    await expect(html(page)).toHaveAttribute('dir', dir);
    await context.close();
  }
});
