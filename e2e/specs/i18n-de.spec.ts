// Allemand (PR #35) : la quatrième langue de l'interface, de gauche à droite. Le code (`<html lang dir>`, sélecteur, `boot.js`,
// repli, durées « vor 5 Min. », dates en `de-u-nu-latn`, e-mails du serveur, noms des salons d'un modèle, mise en page) et le
// catalogue (`i18n/de/*.tsv`, six fichiers, complet) sont allemands : ces tests lisent les vrais textes allemands, rassemblés dans
// `UI` ci-dessous (noms accessibles et phrases du catalogue), et `I18N-D9` vérifie qu'aucun texte français ne reste visible
// (modèle : `I18N-F13` de `i18n-fa.spec.ts`).
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
/** Ce que l'interface allemande écrit (noms accessibles et phrases de `i18n/de/*.tsv`) ; la clé française est en commentaire. */
const UI = {
  language: 'Sprache', // « Langue »
  languageIs: /^\s*Sprache: Deutsch\.\s*$/, // « Langue : {nom}. »
  deviceLanguage: 'Sprache des Geräts', // « Langue de l'appareil » : le premier choix du sélecteur
  welcome: 'Willkommen zurück in der Tiefe', // « Bon retour sous la surface » : le titre de la connexion
  registerTitle: 'Konto erstellen', // « Créer un compte »
  forgotTitle: 'Passwort vergessen', // « Mot de passe oublié »
  login: 'Benutzername oder E-Mail', // « Pseudo ou e-mail » : le champ identifiant de la connexion
  password: 'Passwort', // « Mot de passe »
  settings: 'Einstellungen', // « Réglages »
  languageTime: 'Sprache und Zeit', // « Langue et heure » : le titre de l'onglet
  example: /^Beispiel: 14:30$/, // « Exemple : » + l'heure d'exemple
  createLink: 'Server erstellen oder beitreten', // « Créer ou rejoindre un serveur » : le lien du rail
  serverName: 'Servername', // « Nom du serveur » : le champ de la page de création
  createServer: 'Server erstellen', // « Créer le serveur » : le bouton de la page de création
  validFor: /Gültig für 1 Std\. 30 Min\./, // « Valable {duree}, … » (la durée est écrite par `span::duration`)
  fullDate: / um \d{1,2}:\d{2}$/, // « {date} à {heure} »
  email: 'E-Mail', // « E-mail »
  sendLink: 'Link senden', // « Envoyer le lien » : le bouton de « Mot de passe oublié »
  username: 'Benutzername', // « Pseudo »
  createAccount: 'Mein Konto erstellen', // « Créer mon compte » : le bouton d'inscription
};

const html = (p: Page) => p.locator('html');
const stored = (p: Page, key: string) => p.evaluate((k) => localStorage.getItem(k), key);
const noScroll = async (p: Page) =>
  expect(await p.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(p.viewportSize()!.width);
/**
 * Les éléments dont le texte dépasse leur boîte : un mot composé trop long pour la ligne, un bouton trop étroit (« balise.classe : texte »).
 * Ne comptent pas : une zone qui défile exprès (`overflow` auto ou scroll, comme la barre d'onglets), le texte réservé aux lecteurs
 * d'écran (`.k-sr`) et la coupure voulue par « … » (`text-overflow: ellipsis`).
 */
const overflowing = (p: Page) =>
  p.evaluate(() => {
    const found = new Set<string>();
    for (const el of document.body.querySelectorAll('*')) {
      const style = getComputedStyle(el);
      if (style.display === 'none' || style.display === 'inline' || style.visibility === 'hidden' || el.clientWidth <= 1) continue; // 1 px : texte pour lecteurs d'écran
      if (style.overflowX === 'auto' || style.overflowX === 'scroll' || style.textOverflow === 'ellipsis' || el.closest('.k-sr')) continue;
      const text = (el.textContent ?? '').replace(/\s+/g, ' ').trim();
      if (text && el.scrollWidth > el.clientWidth + 1) {
        const cls = typeof el.className === 'string' && el.className.trim() ? `.${el.className.trim().split(/\s+/).join('.')}` : '';
        found.add(`${el.tagName.toLowerCase()}${cls} (${el.scrollWidth} > ${el.clientWidth} px) : ${text.slice(0, 70)}`);
      }
    }
    return [...found];
  });
/**
 * Les mots plus larges que la boîte qui les porte, que `overflow-wrap: anywhere` (boutons, onglets, pastilles) coupe alors au
 * milieu du mot au lieu de les laisser dépasser : « Berechtigu / ngen ». Ils ne débordent pas, mais ils se lisent mal.
 */
const splitWords = (p: Page) =>
  p.evaluate(() => {
    const found = new Set<string>();
    const probe = document.createElement('span');
    probe.style.cssText = 'position:absolute;visibility:hidden;white-space:nowrap;left:-9999px';
    document.body.appendChild(probe);
    for (const el of document.body.querySelectorAll('*')) {
      const style = getComputedStyle(el);
      if (style.display === 'none' || style.display === 'inline' || style.visibility === 'hidden' || el.clientWidth <= 1 || el.closest('.k-sr')) continue;
      const own = [...el.childNodes].filter((n) => n.nodeType === Node.TEXT_NODE).map((n) => n.textContent).join(' ');
      probe.style.font = style.font;
      probe.style.letterSpacing = style.letterSpacing;
      probe.style.textTransform = style.textTransform;
      const room = el.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
      for (const word of own.split(/\s+/)) {
        if (word.length < 9) continue;
        probe.textContent = word;
        if (probe.getBoundingClientRect().width > room + 1) found.add(word);
      }
    }
    probe.remove();
    return [...found];
  });
/**
 * Les mots allemands qui se coupent encore au milieu du mot à 390 px et texte 20 : ils ne se coupent pas à la main ici, ils se
 * raccourcissent dans `i18n/de/*.tsv` (puis on les retire de cette liste). Ce sont les onglets d'un rôle (Serveur › Rollen).
 */
const SPLIT_WORDS = ['Berechtigungen', 'Mitglieder'];
/**
 * Rien ne dépasse de sa boîte sur cet écran, et aucun mot ne s'y coupe au milieu (hors `SPLIT_WORDS`). Un texte allemand trop long
 * pour 390 px à texte 20 ne se coupe pas à la main : la ligne d'erreur dit l'élément et son texte.
 */
const fits = async (p: Page, where: string) => {
  expect(await overflowing(p), `${where} : du texte dépasse de sa boîte`).toEqual([]);
  expect((await splitWords(p)).filter((w) => !SPLIT_WORDS.includes(w)), `${where} : un mot se coupe au milieu`).toEqual([]);
};
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
  // Le titre, les champs et le sélecteur sont en allemand.
  await expect(page.getByRole('heading', { name: UI.welcome, level: 1 })).toBeVisible();
  await expect(page.getByLabel(UI.login)).toHaveValue('perle@example.org');
  await expect(page.getByLabel(UI.password, { exact: true })).toHaveValue('');
  const after = page.getByRole('combobox', { name: UI.language });
  await expect(after).toHaveValue('de');
  await expect(after).toBeFocused();
  // Le nom de chaque langue s'écrit dans son écriture, quelle que soit la langue de la page.
  expect(await after.locator('option').allTextContents()).toEqual([UI.deviceLanguage, 'Français', 'English', 'فارسی', DE]);
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
  for (const [path, title] of [
    ['/register', UI.registerTitle],
    ['/forgot-password', UI.forgotTitle],
    ['/login', UI.welcome],
  ] as const) {
    await page.goto(path);
    await expect(html(page)).toHaveAttribute('lang', 'de');
    await expect(html(page)).toHaveAttribute('dir', 'ltr');
    await expect(page.getByRole('heading', { level: 1 })).toHaveText(title);
    // Le sélecteur de langue est en pied de carte sur /login et /register (pas sur « Mot de passe oublié »).
    if (path !== '/forgot-password') await expect(page.getByRole('combobox', { name: UI.language })).toHaveValue('de');
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
  // La phrase d'état dit la langue, nom de la langue dans son écriture : « Sprache: Deutsch. »
  await expect(said(page, DE)).toHaveText(UI.languageIs);
  // L'heure automatique suit la langue de l'interface, pas celle du navigateur (français ici) : 24 h, « Beispiel: 14:30 ».
  await expect(said(page, '14:30').first()).toHaveText(UI.example);
  // La fenêtre des Réglages et son titre de section sont allemands.
  await expect(page.getByRole('dialog', { name: UI.settings, exact: true })).toBeVisible();
  await expect(page.getByRole('dialog').getByRole('heading', { level: 2 })).toHaveText(UI.languageTime);
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
  // La phrase du catalogue (« Gültig für … ») et la durée, écrite par `span::duration` (code, pas catalogue).
  await expect(rows.first()).toContainText(UI.validFor);
  // La date complète (infobulle et lecteur d'écran) : `Intl` en de-u-nu-latn, « Montag, 23. November 2026 », chiffres latins, 24 h.
  const full = (await when.locator('.k-sr').textContent()) ?? '';
  expect(full).toMatch(/^(Montag|Dienstag|Mittwoch|Donnerstag|Freitag|Samstag|Sonntag), \d{1,2}\. (Januar|Februar|März|April|Mai|Juni|Juli|August|September|Oktober|November|Dezember) \d{4}/);
  expect(full).toMatch(UI.fullDate); // « … um 14:30 »
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
  await expect(said(page, '14:30').first()).toHaveText(UI.example); // « Beispiel: 14:30 »
  // Plus d'AM/PM, sauf l'exemple du choix « 12-Stunden-Format » (« 2:30 PM »), qui montre justement ce format.
  expect((await page.locator('body').innerText()).replace('2:30 PM', '')).not.toMatch(/\b(AM|PM)\b/);
});

// ───────── petits écrans, grands textes ─────────

/** Les onglets des Réglages (`settings.rs`) et les rubriques des paramètres d'un serveur (`guild.rs`) : l'adresse de chacun. */
const SETTINGS = ['account', 'profiles', 'privacy', 'messaging', 'apps', 'notifications', 'voice', 'appearance', 'accessibility', 'keybinds', 'language'];
const GUILD_SETTINGS = ['overview', 'roles', 'channels', 'emojis', 'invites', 'bans', 'audit', 'owner'];

test.describe('390 px, texte 20, allemand', () => {
  test.use({ locale: 'de-DE', viewport: { width: 390, height: 844 } });

  test('I18N-D5 rien ne déborde en allemand (mots composés compris) : connexion, inscription, coquille, Réglages, serveur et ses paramètres ; axe sans violation', async ({ page, signedIn, api }) => {
    test.setTimeout(120_000); // une trentaine d'écrans, chacun avec l'analyse axe
    await page.goto('/login');
    await page.evaluate(() => localStorage.setItem('kraken.text', '20'));
    for (const path of ['/login', '/register', '/forgot-password']) {
      await page.goto(path);
      await expect(html(page)).toHaveAttribute('lang', 'de');
      await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
      await noScroll(page);
      await fits(page, path);
      await a11y(page);
    }
    const a = await signedIn('de-DE', { width: 390, height: 844 });
    const { page: app } = a;
    await app.evaluate(() => localStorage.setItem('kraken.text', '20'));
    await app.goto('/settings/language');
    await expect(html(app)).toHaveAttribute('lang', 'de');
    await expect(app.getByRole('combobox', { name: UI.language })).toBeVisible();
    // La barre d'onglets défile : l'onglet ouvert (le dernier) est dans la vue.
    await expect(app.locator('.k-settings nav a[aria-current="page"]')).toBeInViewport({ ratio: 1 });

    // Un serveur créé en allemand (modèle « Communauté » : noms de salons allemands) : ses salons et ses paramètres.
    const as = await api(a.token);
    const created = await as.post(`${API}/guilds`, { data: { name: 'Korallenriff', template: 'communaute', lang: 'de' } });
    expect(created.status(), await created.text()).toBe(201);
    const g = await created.json();
    const text = g.channels.find((c: { kind: string }) => c.kind === 'text').id;
    const screens = [
      '/channels/@me',
      '/guilds/new',
      ...SETTINGS.map((slug) => `/settings/${slug}`),
      `/channels/${g.guild.id}/${text}`,
      `/channels/${g.guild.id}/${text}/settings`,
      `/channels/${g.guild.id}/${text}/settings/permissions`,
      ...GUILD_SETTINGS.map((slug) => `/guilds/${g.guild.id}/settings/${slug}`),
    ];
    for (const path of screens) {
      await app.goto(path);
      await expect(app.locator('#main, [role="dialog"]').first()).toBeVisible();
      await app.waitForLoadState('networkidle');
      await noScroll(app);
      await fits(app, path);
      await a11y(app);
    }
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

  // Les vrais textes allemands, à 390 px et texte 20 : la phrase d'accroche (césure automatique) et les boutons (« Anmelden »,
  // « Mein Konto erstellen ») tiennent dans leur boîte, la page ne défile pas de côté.
  const context = await browser.newContext({ baseURL, locale: 'de-DE', viewport: { width: 390, height: 844 } });
  const page = await context.newPage();
  await page.goto('/login');
  await page.evaluate(() => localStorage.setItem('kraken.text', '20'));
  for (const path of ['/login', '/register']) {
    await page.goto(path);
    await expect(html(page)).toHaveAttribute('lang', 'de');
    const hint = page.locator('p.app-hint').first();
    await expect(hint).toHaveText(path === '/login' ? 'Melde dich an, um deine Server wiederzufinden.' : 'Kostenlos, ohne Abo, alles inklusive.');
    expect(await hint.evaluate((el) => getComputedStyle(el).hyphens)).toBe('auto');
    await noScroll(page);
    await fits(page, path);
  }
  await context.close();
});

// ───────── créer un serveur : la langue part avec la demande ─────────

test('I18N-D6 créer un serveur en allemand : la demande porte lang="de", les salons et catégories du modèle portent leurs noms allemands, une étiquette régionale est comprise, une langue inconnue donne le français', async ({ signedIn, api }) => {
  const a = await signedIn('de-DE');
  const { page } = a;
  await expect(html(page)).toHaveAttribute('lang', 'de');
  // Par l'interface : la langue de l'interface part avec la demande.
  await page.getByRole('link', { name: UI.createLink }).click();
  await page.getByLabel(UI.serverName, { exact: true }).fill('Abyss');
  const request = page.waitForRequest((r) => r.method() === 'POST' && r.url().endsWith(`${API}/guilds`));
  await page.getByRole('button', { name: UI.createServer, exact: true }).click();
  expect((await request).postDataJSON().lang).toBe('de');
  await page.waitForURL(/\/channels\/[0-9a-f-]{36}\/[0-9a-f-]{36}$/);
  // Le modèle « Vide » : le salon textuel porte son nom allemand (`.de("allgemein")` dans `templates.rs`).
  await expect(page.getByRole('heading', { name: 'allgemein', level: 1 })).toBeVisible();

  // Par l'API : `de`, `de-AT`, `DE_ch` sont l'allemand, une langue inconnue donne le français.
  const as = await api(a.token);
  const channels = async (lang: string) => {
    const res = await as.post(`${API}/guilds`, { data: { name: 'Abyss', template: 'communaute', lang } });
    expect(res.status(), await res.text()).toBe(201);
    const g = await res.json();
    const of = (kind: string) => g.channels.filter((c: { kind: string }) => c.kind === kind).map((c: { name: string }) => c.name).sort();
    return { text: of('text'), voice: of('voice'), category: of('category') };
  };
  // Le modèle « Communauté » en allemand : salons textuels, vocal et catégories (`.de("…")` dans `templates.rs`).
  const german = { text: ['allgemein', 'ankündigungen', 'hilfe', 'regeln'], voice: ['Offene Runde'], category: ['Gespräche', 'Sprachkanäle', 'Willkommen'] };
  for (const lang of ['de', 'de-AT', 'DE_ch']) expect(await channels(lang), lang).toEqual(german);
  // L'anglais garde ses noms (le repli n'est plus en jeu : l'allemand ne les emprunte pas).
  const english = await channels('en');
  expect(english.text).toEqual(['announcements', 'general', 'help', 'rules']);
  expect(english).not.toEqual(german);
  expect(await channels('nl')).toEqual(await channels('fr'));
  expect((await channels('fr')).text).toEqual(['aide', 'annonces', 'général', 'règles']);
});

// ───────── e-mails du serveur : la langue de l'interface part avec la demande ─────────

test('I18N-D7 e-mails du serveur : l’inscription et le mot de passe oublié envoient lang="de", l’API accepte de, de-AT et de_CH', async ({ page, api, allow }) => {
  // Le serveur de test n'a pas de SMTP : « mot de passe oublié » répond 501 (le message de l'interface le dit), c'est attendu.
  allow.push(/auth\/reset/, /status of 5\d\d/);
  await page.goto('/login');
  await page.evaluate(() => localStorage.setItem('kraken.lang', 'de'));

  // « Mot de passe oublié » : la demande porte lang="de".
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

// ───────── couverture : aucun français visible ─────────

/** Une lettre qu'aucun mot allemand ni anglais ne porte, mais que presque chaque phrase française contient (accents, ç, œ). L'allemand a ä ö ü ß : aucune n'est dans la liste. */
const FRENCH_LETTER = /[éèêëàâçùûîïôœ]/i;
/** Des mots français que ni l'allemand ni l'anglais n'écrivent : ils trahissent un texte français sans accent (« Aucun salon », « Fermer »). `\b` ne voit pas « ä » comme une lettre (« Kanäle » contiendrait « le ») : lettres Unicode. */
const FRENCH_WORD = /(?<!\p{L})(le|la|les|une|pour|avec|dans|vous|votre|aux|aucun|aucune|salon|salons|serveur|serveurs|fermer|annuler|supprimer|enregistrer|ajouter|rejoindre)(?!\p{L})/iu;
/** Les noms d'exemple de l'aperçu des Réglages › Apparence (`display.rs`, marqués `i18n-ok : nom d'exemple`) : des prénoms, pas des phrases. */
const SAMPLE_NAMES = ['Récif', 'RÉ'];
/**
 * Ce qu'une page laisse voir en français : le texte affiché (ligne par ligne) et les noms accessibles (`aria-label`, `title`,
 * `placeholder`, `alt`). « Français », le nom de la langue dans le sélecteur, s'écrit dans son écriture : il est permis.
 */
const frenchLeft = (p: Page) =>
  p.evaluate(
    ([letterSource, wordSource, samples]) => {
      const letter = new RegExp(letterSource, 'i');
      const word = new RegExp(wordSource, 'iu');
      const found = new Set<string>();
      const look = (text: string | null) => {
        for (const line of (text ?? '').split('\n')) {
          const shown = line.replace('Français', '');
          if ((letter.test(shown) || word.test(shown)) && !samples.includes(line.trim())) found.add(line.trim());
        }
      };
      look(document.body.innerText);
      for (const el of document.querySelectorAll('[aria-label],[title],[placeholder],[alt]'))
        for (const a of ['aria-label', 'title', 'placeholder', 'alt']) look(el.getAttribute(a));
      return [...found];
    },
    [FRENCH_LETTER.source, FRENCH_WORD.source, SAMPLE_NAMES] as [string, string, string[]],
  );

test('I18N-D9 couverture : aucun texte français ne reste visible en allemand (connexion, coquille, Réglages, serveur et ses paramètres), ni dans les noms accessibles', async ({ page, signedIn, api, allow }) => {
  // Une invitation inconnue (« Invitation invalide ») : le serveur répond 404, c'est attendu.
  allow.push(/status of 404/, /invites\//);
  test.setTimeout(90_000); // une trentaine d'écrans
  const left: Record<string, string[]> = {};
  const check = async (p: Page, where: string) => {
    await p.waitForLoadState('networkidle');
    const found = await frenchLeft(p);
    if (found.length) left[where] = found;
  };

  // Hors connexion : les écrans de la carte, la langue choisie dans le sélecteur (enregistrée).
  await page.goto('/login');
  await page.evaluate(() => localStorage.setItem('kraken.lang', 'de'));
  for (const path of ['/login', '/register', '/forgot-password', '/reset-password', '/invite/inconnue', '/privacy', '/terms']) {
    await page.goto(path);
    await expect(html(page)).toHaveAttribute('lang', 'de');
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
    await check(page, path);
  }

  // Connecté : l'accueil, chaque onglet des Réglages, un serveur (créé en allemand, comme le fait l'interface) et ses paramètres.
  const a = await signedIn('de-DE');
  const { page: app } = a;
  await expect(html(app)).toHaveAttribute('lang', 'de');
  await check(app, '/channels/@me');
  for (const path of ['/guilds/new', '/inbox', '/search', '/discover']) {
    await app.goto(path);
    await expect(app.locator('#main')).toBeVisible();
    await check(app, path);
  }
  for (const slug of SETTINGS) {
    await app.goto(`/settings/${slug}`);
    await expect(app.getByRole('dialog').getByRole('heading', { level: 2 }).first()).toBeVisible();
    await check(app, `/settings/${slug}`);
  }
  const as = await api(a.token);
  const created = await as.post(`${API}/guilds`, { data: { name: 'Korallenriff', template: 'communaute', lang: 'de' } });
  expect(created.status(), await created.text()).toBe(201);
  const g = await created.json();
  const text = g.channels.find((c: { kind: string }) => c.kind === 'text').id;
  await app.goto(`/channels/${g.guild.id}/${text}`);
  await expect(app.getByRole('heading', { level: 1 })).toBeVisible();
  await check(app, 'serveur : salon');
  for (const path of [`/channels/${g.guild.id}/${text}/settings`, `/channels/${g.guild.id}/${text}/settings/permissions`]) {
    await app.goto(path);
    await expect(app.getByRole('dialog').getByRole('heading', { level: 2 }).first()).toBeVisible();
    await check(app, path.replace(g.guild.id, '…').replace(text, '…'));
  }
  for (const slug of GUILD_SETTINGS) {
    await app.goto(`/guilds/${g.guild.id}/settings/${slug}`);
    await expect(app.getByRole('dialog').getByRole('heading', { level: 2 }).first()).toBeVisible();
    await check(app, `/guilds/…/settings/${slug}`);
  }
  expect(left, 'texte encore en français : écran → lignes').toEqual({});
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
