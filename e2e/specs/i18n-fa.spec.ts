// Persan (PR #33) : la troisième langue de l'interface et la première qui s'écrit de droite à gauche. Le catalogue persan
// (`i18n/fa/*.tsv`) est complet : chaque texte de l'interface, chaque nom accessible et chaque message du serveur est persan,
// et les noms de salons et de catégories des modèles aussi (`.fa("…")` dans `templates.rs`). Ce que ces tests tiennent :
// `<html lang="fa" dir="rtl">`, le nom de la langue dans le sélecteur, les durées et « 5 دقیقه پیش » (`kraken_core::span`),
// les dates (`Intl` en `fa-u-nu-latn` : calendrier persan, chiffres latins), l'heure sur 24 h, la géométrie de droite à
// gauche, et (I18N-F13) qu'aucun texte français ne reste visible. Les noms accessibles se lisent en persan (« زبان »).
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
  await expect(language).toHaveValue('auto'); // le client est monté : les options sont là
  expect(await language.locator('option').allTextContents()).toEqual(["Langue de l'appareil", 'Français', 'English', FA, 'Deutsch']);
  expect(await language.locator('option').evaluateAll((o) => o.map((x) => x.getAttribute('lang')))).toEqual([null, 'fr', 'en', 'fa', 'de']);
  await expect(html(page)).toHaveAttribute('dir', 'ltr');
  await expectLabelAtStart(page, 'ltr');

  await page.getByLabel('Pseudo ou e-mail').fill('perle@example.org');
  await page.getByLabel('Mot de passe').fill('un-secret-bien-gardé');
  await language.selectOption({ label: FA });
  await expect(html(page)).toHaveAttribute('lang', 'fa');
  await expect(html(page)).toHaveAttribute('dir', 'rtl');
  expect(await stored(page, 'kraken.lang')).toBe('fa');
  await expect(page.getByRole('heading', { name: 'خوش برگشتید', level: 1 })).toBeVisible();
  await expect(page.getByLabel('نام کاربری یا ایمیل')).toHaveValue('perle@example.org');
  await expect(page.getByLabel('رمز عبور', { exact: true })).toHaveValue('');
  const after = page.getByRole('combobox', { name: 'زبان' });
  await expect(after).toHaveValue('fa');
  await expect(after).toBeFocused();
  expect(await after.locator('option').allTextContents()).toEqual(['زبان دستگاه', 'Français', 'English', FA, 'Deutsch']);
  expect(await after.locator('option').evaluateAll((o) => o.map((x) => x.getAttribute('lang')))).toEqual([null, 'fr', 'en', 'fa', 'de']);
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
  await expect(page.getByLabel('ایمیل', { exact: true })).toHaveAttribute('dir', 'ltr');
});

test('I18N-F3 Réglages › Langue et heure : choisir « فارسی » recharge la page, rend le focus et dit la langue ; l’heure suit la langue (24 h) ; retour au français', async ({ signedIn }) => {
  const { page } = await signedIn('fr-FR');
  await page.goto('/settings/language');
  const dialog = page.getByRole('dialog');
  const language = dialog.getByRole('combobox', { name: 'Langue' });
  await expect(language).toHaveValue('auto');
  expect(await language.locator('option').allTextContents()).toEqual(["Langue de l'appareil", 'Français', 'English', FA, 'Deutsch']);
  await expectLabelAtStart(page, 'ltr');

  await language.selectOption({ label: FA });
  await expect(html(page)).toHaveAttribute('lang', 'fa');
  await expect(html(page)).toHaveAttribute('dir', 'rtl');
  expect(await stored(page, 'kraken.lang')).toBe('fa');
  expect(page.url()).toContain('/settings/language');
  const after = page.getByRole('dialog').getByRole('combobox', { name: 'زبان' });
  await expect(after).toHaveValue('fa');
  await expect(after).toBeFocused();
  // La phrase d'état dit la langue, nom de la langue dans son écriture (isolé entre U+2068 et U+2069 en droite à gauche).
  await expect(said(page, FA)).toHaveText(new RegExp(`زبان: \\u2068?${FA}\\u2069?\\.`));
  // Le fond de la page de réglages suit le sens : la fenêtre, ses onglets et le sélecteur.
  await expectLabelAtStart(page, 'rtl');
  // L'heure automatique suit la langue de l'interface, pas celle du navigateur (français ici) : 24 h, chiffres latins.
  await expect(said(page, 'نمونه').first()).toHaveText('نمونه: 14:30');
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
  const twelve = page.getByRole('dialog').getByRole('group', { name: 'قالب ساعت' }).getByRole('radio').nth(1);
  await expect(page.getByRole('dialog').getByRole('radio', { name: '12 ساعته' })).toBeVisible();
  await twelve.check();
  await expect(said(page, 'نمونه').first()).toHaveText(/^نمونه: 2:30 ب\.ظ\.$/);

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
  await page.getByRole('dialog').getByRole('combobox', { name: 'زبان' }).selectOption({ label: 'English' });
  await expect(html(page)).toHaveAttribute('lang', 'en');
  await expect(html(page)).toHaveAttribute('dir', 'ltr');
  await expectLabelAtStart(page, 'ltr');
  // « Langue de l'appareil » : retour au navigateur (français).
  await page.getByRole('dialog').getByRole('combobox', { name: 'Language' }).selectOption('auto');
  await expect(html(page)).toHaveAttribute('lang', 'fr');
  expect(await stored(page, 'kraken.lang')).toBe('auto');
});

// ───────── le navigateur choisit la langue ─────────

test.describe('navigateur en persan', () => {
  test.use({ locale: 'fa-IR' });

  test('I18N-F4 « Langue de l’appareil » : navigator.languages = fa-IR donne le persan, noms accessibles compris, sans un mot de français ni d’anglais', async ({ signedIn }) => {
    const { page } = await signedIn();
    await expect(html(page)).toHaveAttribute('lang', 'fa');
    await expect(html(page)).toHaveAttribute('dir', 'rtl');
    expect(await stored(page, 'kraken.lang')).toBeNull();
    // Les noms accessibles de la coquille sont persans (`\u200c` est le demi-espace persan, dans « پیام\u200cهای »).
    await expect(page.getByRole('navigation', { name: 'سرورها' })).toBeVisible();
    await expect(page.getByRole('link', { name: 'خانه و پیام\u200cهای خصوصی' })).toBeVisible();
    await expect(page.getByRole('link', { name: 'ایجاد یا پیوستن به سرور' })).toBeVisible();
    const sidebar = page.getByRole('navigation', { name: 'پیام\u200cهای خصوصی' });
    await expect(sidebar.getByRole('link', { name: 'دوستان' })).toBeVisible();
    await expect(sidebar.getByText('هنوز گفتگویی وجود ندارد.')).toBeVisible();
    // Aucune phrase de la coquille ne reste en français, ni en anglais (l'ancien repli du catalogue vide).
    const body = await page.locator('body').innerText();
    expect(body).not.toMatch(/Amis|Messages privés|Serveurs|Créer ou rejoindre|Aucune conversation/);
    expect(body).not.toMatch(/Friends|Direct messages|Servers|Create or join|No conversations/);
    await a11y(page);
    // « Langue de l'appareil » est le choix affiché, et le persan est dans la liste.
    await page.goto('/settings/language');
    const language = page.getByRole('dialog').getByRole('combobox', { name: 'زبان' });
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
  const cases: [string[], 'fa' | 'fr' | 'en' | 'de'][] = [
    [['fa'], 'fa'],
    [['fa-IR'], 'fa'],
    [['fa-AF'], 'fa'],
    [['FA-ir'], 'fa'],
    [['fa_IR'], 'fa'],
    [['ja-JP', 'fa-IR', 'en'], 'fa'],
    [['de-DE', 'fa-IR', 'en'], 'de'],
    [['en-US', 'fa-IR'], 'en'],
    [['ar-SA', 'he-IL'], 'fr'],
    [['nl-NL'], 'fr'],
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
    const rows = page.getByRole('list', { name: 'رویدادهای گزارش ممیزی' }).getByRole('listitem');
    await expect(rows.first()).toBeVisible();
    return rows;
  };

  // Moins d'une minute après : « الان ».
  await page.clock.setFixedTime(Date.now() + 5_000);
  let rows = await open();
  const when = rows.first().locator('time');
  await expect(when.locator('[aria-hidden="true"]')).toHaveText('الان');
  // Durée dans la phrase (« معتبر برای … »), la valeur isolée entre U+2068 et U+2069 ; chiffres latins.
  await expect(rows.first()).toContainText(/معتبر برای \u2068?1 ساعت و 30 دقیقه\u2069?/);
  // La date complète (infobulle et lecteur d'écran) : calendrier persan (l'année est 14xx, pas 20xx), mois en persan, chiffres
  // latins. L'ordre « année mois jour, jour de la semaine » est celui que le calendrier persan donne à `Intl` (CLDR).
  const full = (await when.locator('.k-sr').textContent()) ?? '';
  expect(full).toMatch(/\b14\d{2} [\u0600-\u06FF]+ \d{1,2}\b/);
  expect(full).toMatch(/[\u0600-\u06FF]+\u2069?، ساعت \d{1,2}:\d{2}$/); // « {date}، ساعت {heure} », et plus « at »
  expect(full).not.toMatch(/\b(at|à)\b/);
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
    await expect(app.getByRole('combobox', { name: 'زبان' })).toBeVisible();
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
  const rail = await page.getByRole('navigation', { name: 'سرورها' }).boundingBox();
  expect(rail).not.toBeNull();
  expect(rail!.x + rail!.width).toBeGreaterThan(1440 - 80);
  // `:lang(fa)` : la pile de polices persane s'applique (Inter d'abord pour le latin, puis une police du système).
  const family = await page.evaluate(() => getComputedStyle(document.body).fontFamily);
  expect(family).toMatch(/Inter/);
  expect(family.toLowerCase()).toMatch(/tahoma|segoe|noto|geeza|arabic/);
});

// ───────── créer un serveur : la langue part avec la demande ─────────

test('I18N-F11 créer un serveur en persan : la demande porte lang="fa", les salons et catégories du modèle portent leurs noms persans, une langue inconnue donne le français', async ({ signedIn, api }) => {
  const a = await signedIn('fa-IR');
  const { page } = a;
  // Par l'interface : la langue de l'interface part avec la demande.
  await page.getByRole('link', { name: 'ایجاد یا پیوستن به سرور' }).click();
  await page.getByLabel('نام سرور', { exact: true }).fill('Abyss');
  const request = page.waitForRequest((r) => r.method() === 'POST' && r.url().endsWith(`${API}/guilds`));
  await page.getByRole('button', { name: 'ایجاد سرور', exact: true }).click();
  expect((await request).postDataJSON().lang).toBe('fa');
  await page.waitForURL(/\/channels\/[0-9a-f-]{36}\/[0-9a-f-]{36}$/);
  // Le modèle « Vide » : le salon textuel porte son nom persan (`.fa("عمومی")` dans `templates.rs`).
  await expect(page.getByRole('heading', { name: 'عمومی', level: 1 })).toBeVisible();
  // Par l'API : une étiquette régionale (fa-IR) est comprise, une langue inconnue donne le français.
  const as = await api(a.token);
  const channels = async (lang: string) => {
    const res = await as.post(`${API}/guilds`, { data: { name: 'Abyss', template: 'communaute', lang } });
    expect(res.status(), await res.text()).toBe(201);
    const g = await res.json();
    const of = (kind: string) => g.channels.filter((c: { kind: string }) => c.kind === kind).map((c: { name: string }) => c.name).sort();
    return { text: of('text'), voice: of('voice'), category: of('category') };
  };
  // Le modèle « Communauté » en persan : salons textuels, vocal et catégories (`\u200c` est le demi-espace persan).
  expect(await channels('fa-IR')).toEqual({
    text: ['اطلاعیه\u200cها', 'عمومی', 'قوانین', 'کمک'],
    voice: ['گفتگوی آزاد'],
    category: ['خوش\u200cآمدگویی', 'کانال\u200cهای صوتی', 'گفتگوها'],
  });
  expect((await channels('fr')).text).toEqual(['aide', 'annonces', 'général', 'règles']);
  expect((await channels('nl')).text).toEqual(['aide', 'annonces', 'général', 'règles']);
});

test('I18N-F14 le nom de serveur proposé en persan est une donnée : sans marque d’isolation, et le serveur se crée', async ({ signedIn }) => {
  const { page } = await signedIn('fa-IR');
  await page.getByRole('link', { name: 'ایجاد یا پیوستن به سرور' }).click();
  const field = page.getByLabel('نام سرور', { exact: true });
  await expect(field).toHaveValue(/^سرور \S+/);
  // Le nom proposé part tel quel au serveur, qui refuse les marques de sens d'écriture (U+2066 à U+2069).
  expect(await field.inputValue()).not.toMatch(/[\u200e\u200f\u202a-\u202e\u2066-\u2069]/);
  const request = page.waitForRequest((r) => r.method() === 'POST' && r.url().endsWith(`${API}/guilds`));
  await page.getByRole('button', { name: 'ایجاد سرور', exact: true }).click();
  expect((await request).postDataJSON().name).not.toMatch(/[\u200e\u200f\u202a-\u202e\u2066-\u2069]/);
  await page.waitForURL(/\/channels\/[0-9a-f-]{36}\/[0-9a-f-]{36}$/);
});

test('I18N-F15 Ctrl+K en persan : demi-espace tapé comme une espace, chiffres persans, « ك » et « ي » arabes trouvent les salons du modèle', async ({ signedIn, api }) => {
  const a = await signedIn('fa-IR');
  const { page } = a;
  const as = await api(a.token);
  const res = await as.post(`${API}/guilds`, { data: { name: 'Abyss', template: 'jeux', lang: 'fa' } });
  expect(res.status(), await res.text()).toBe(201);
  const g = await res.json();
  const general = g.channels.find((c: { kind: string; name: string }) => c.kind === 'text' && c.name === 'عمومی');
  await page.goto(`/channels/${g.guild.id}/${general.id}`);
  await expect(page.getByRole('heading', { name: 'عمومی', level: 1 })).toBeVisible();
  await page.keyboard.press('Control+k');
  const dialog = page.getByRole('dialog', { name: 'رفتن به' });
  await expect(dialog).toBeVisible();
  const field = dialog.getByRole('combobox');
  // Chaque saisie, avec les habitudes d'un clavier persan ou arabe, trouve le salon dont le nom est écrit autrement.
  for (const [typed, found] of [
    ['هم تیمی', 'یافتن-هم\u200cتیمی'],
    ['#نکته ها', 'نکته\u200cها'],
    ['بازی ۱', 'بازی 1'],
    ['بازی ٢', 'بازی 2'],
    ['نكته', 'نکته\u200cها'],
    ['يافتن', 'یافتن-هم\u200cتیمی'],
  ]) {
    await field.fill(typed);
    await expect(dialog.getByRole('option', { name: new RegExp(found.replace(/[-\u200c]/g, '.')) }).first(), typed).toBeVisible();
  }
});

test('I18N-F16 complétion de @ et de # en persan : « علي » et « رضايي » arabes, ۱ et demi-espace trouvent la personne et le salon', async ({ signedIn, api }) => {
  const a = await signedIn('fa-IR');
  const b = await signedIn('fa-IR');
  const c = await signedIn('fa-IR');
  const d = await signedIn('fa-IR');
  const asA = await api(a.token);
  const asB = await api(b.token);
  expect((await asB.patch(`${API}/users/@me`, { data: { display_name: 'علی رضایی' } })).status()).toBe(200);
  // Des noms composés avec demi-espace (U+200C), la frappe normale d'un nom persan.
  expect((await (await api(c.token)).patch(`${API}/users/@me`, { data: { display_name: 'علی\u200cرضا' } })).status()).toBe(200);
  expect((await (await api(d.token)).patch(`${API}/users/@me`, { data: { display_name: 'نیم\u200cفاصله خان' } })).status()).toBe(200);
  const res = await asA.post(`${API}/guilds`, { data: { name: 'Abyss', template: 'jeux', lang: 'fa' } });
  expect(res.status(), await res.text()).toBe(201);
  const g = await res.json();
  const general = g.channels.find((c: { kind: string; name: string }) => c.kind === 'text' && c.name === 'عمومی');
  const { code } = await (await asA.post(`${API}/channels/${general.id}/invites`, { data: {} })).json();
  for (const who of [b, c, d]) expect((await (await api(who.token)).post(`${API}/invites/${code}`)).status()).toBeLessThan(300);
  const { page } = a;
  await page.goto(`/channels/${g.guild.id}/${general.id}`);
  await expect(page.getByRole('heading', { name: 'عمومی', level: 1 })).toBeVisible();
  const box = page.locator('textarea').first();
  const list = page.getByRole('listbox');
  // « @ » : le nom affiché est écrit avec « ی » persans ; les touches « ي » arabes le trouvent aussi.
  for (const typed of ['@علی', '@علي', '@رضایی', '@رضايي']) {
    await box.fill('');
    await box.pressSequentially(typed);
    await expect(list.getByRole('option', { name: /علی رضایی/ }), typed).toBeVisible();
  }
  // Le demi-espace fait partie du mot : tapé ou non, il ne cache pas « علی‌رضا » ni « نیم‌فاصله خان ».
  for (const [typed, found] of [
    ['@علی\u200c', 'علی\u200cرضا'],
    ['@علی\u200cرض', 'علی\u200cرضا'],
    ['@علیرض', 'علی\u200cرضا'],
    ['@نیم\u200cفا', 'نیم\u200cفاصله خان'],
    ['@نیمفا', 'نیم\u200cفاصله خان'],
  ]) {
    await box.fill('');
    await box.pressSequentially(typed);
    await expect(list.getByRole('option', { name: new RegExp(found) }), typed).toBeVisible();
  }
  // « # » : le salon « نکته‌ها » (demi-espace) se trouve par ses premières lettres, avec « ك » arabe aussi.
  for (const typed of ['#نکته', '#نكته']) {
    await box.fill('');
    await box.pressSequentially(typed);
    await expect(list.getByRole('option', { name: /#نکته\u200cها/ }), typed).toBeVisible();
  }
});

// ───────── e-mails du serveur : la langue de l'interface part avec la demande (PR #34) ─────────

test('I18N-F17 e-mails du serveur : l’inscription et le mot de passe oublié envoient la langue de l’interface (fa, en), `lang` est accepté par l’API (étiquette régionale et langue inconnue compris), un mauvais type est refusé comme à la création d’un serveur', async ({ page, api, allow }) => {
  // Le serveur de test n'a pas de SMTP : « mot de passe oublié » répond 501 (le message de l'interface le dit), c'est attendu.
  allow.push(/auth\/reset/, /status of 5\d\d/);
  const setLang = async (code: string) => {
    await page.goto('/login');
    await page.evaluate((c) => localStorage.setItem('kraken.lang', c), code);
  };

  // En persan : la demande d'inscription et celle du mot de passe oublié portent lang="fa" (la page ne lit pas l'e-mail).
  await setLang('fa');
  await page.goto('/forgot-password');
  await expect(html(page)).toHaveAttribute('lang', 'fa');
  await page.getByLabel('ایمیل', { exact: true }).fill('personne@example.org');
  const reset = page.waitForRequest((r) => r.method() === 'POST' && r.url().endsWith(`${API}/auth/reset`));
  await page.getByRole('button', { name: 'ارسال پیوند', exact: true }).click();
  expect((await reset).postDataJSON()).toEqual({ email: 'personne@example.org', lang: 'fa' });

  const name = unique('mailfa');
  await page.goto('/register');
  await page.getByLabel('ایمیل', { exact: true }).fill(`${name}@example.org`);
  await page.getByLabel('نام کاربری').fill(name); // le nom accessible porte aussi l’aide du champ
  await page.getByLabel('رمز عبور', { exact: true }).fill(PASSWORD);
  const register = page.waitForRequest((r) => r.method() === 'POST' && r.url().endsWith(`${API}/auth/register`));
  await page.getByRole('button', { name: 'ایجاد حساب کاربری من', exact: true }).click();
  expect((await register).postDataJSON()).toEqual({ username: name, email: `${name}@example.org`, password: PASSWORD, lang: 'fa' });
  await page.waitForURL('**/channels/@me');

  // En anglais, puis en français : la langue de l'interface, rien d'autre.
  for (const [code, label, button] of [['en', 'Email', 'Send link'], ['fr', 'E-mail', 'Envoyer le lien']] as const) {
    await setLang(code);
    await page.goto('/forgot-password');
    await page.getByLabel(label, { exact: true }).fill('personne@example.org');
    const sent = page.waitForRequest((r) => r.method() === 'POST' && r.url().endsWith(`${API}/auth/reset`));
    await page.getByRole('button', { name: button, exact: true }).click();
    expect((await sent).postDataJSON().lang, code).toBe(code);
  }

  // Par l'API : `lang` est facultatif, une étiquette régionale ou une langue inconnue est acceptée (l'e-mail part alors dans la
  // langue connue la plus proche, à défaut en français), comme `lang` de la création d'un serveur.
  const anon = await api();
  for (const lang of [undefined, 'fa', 'en', 'fr', 'fa-IR', 'xx']) {
    const user = unique('mail');
    const res = await anon.post(`${API}/auth/register`, { data: { username: user, email: `${user}@example.org`, password: PASSWORD, lang } });
    expect(res.status(), `${lang} : ${await res.text()}`).toBe(201);
  }
  // Un mot de passe oublié avec `lang` : la demande est comprise (le serveur de test répond « pas d'e-mail » : 501, pas 4xx).
  const noMail = await anon.post(`${API}/auth/reset`, { data: { email: 'personne@example.org', lang: 'fa' } });
  expect(noMail.status()).toBe(501);
  expect((await noMail.json()).code).toBe('no_email');
  // Changer d'adresse (sans SMTP : tout de suite) avec `lang` : accepté.
  const user = unique('mail');
  const made = await anon.post(`${API}/auth/register`, { data: { username: user, email: `${user}@example.org`, password: PASSWORD, lang: 'fa' } });
  const { token } = await made.json();
  const as = await api(token);
  const changed = await as.put(`${API}/users/@me/email`, { data: { email: `${user}.2@example.org`, password: PASSWORD, lang: 'fa-IR' } });
  expect(changed.status(), await changed.text()).toBe(200);

  // Un `lang` qui n'est pas du texte : le même refus que `POST /guilds` (le champ est typé), et aucun compte n'est créé.
  const guild = await as.post(`${API}/guilds`, { data: { name: 'Abyss', lang: 7 } });
  expect(guild.status()).toBeGreaterThanOrEqual(400);
  expect(guild.status()).toBeLessThan(500);
  const bad = unique('mail');
  const refused = await anon.post(`${API}/auth/register`, { data: { username: bad, email: `${bad}@example.org`, password: PASSWORD, lang: 7 } });
  expect(refused.status()).toBe(guild.status());
  expect((await anon.post(`${API}/auth/login`, { data: { login: bad, password: PASSWORD } })).status()).toBe(401);
  expect((await anon.post(`${API}/auth/reset`, { data: { email: 'personne@example.org', lang: ['fa'] } })).status()).toBe(guild.status());
  expect((await as.put(`${API}/users/@me/email`, { data: { email: `${user}.3@example.org`, password: PASSWORD, lang: {} } })).status()).toBe(guild.status());
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

// ───────── couverture : aucun français visible ─────────

/** Une lettre qu'aucun mot persan ni anglais ne contient, mais que presque chaque phrase française porte (accents, ç, œ). */
const FRENCH_LETTER = /[éèêëàâçùûîïôœ]/i;
/** Les noms d'exemple de l'aperçu des Réglages › Apparence (`display.rs`, marqués `i18n-ok : nom d'exemple`) : des prénoms, pas des phrases. */
const SAMPLE_NAMES = ['Récif', 'RÉ'];
/**
 * Ce qu'une page laisse voir en français : le texte affiché (ligne par ligne) et les noms accessibles (`aria-label`, `title`,
 * `placeholder`, `alt`). « Français », le nom de la langue dans le sélecteur, s'écrit dans son écriture : il est permis.
 */
const frenchLeft = (p: Page) =>
  p.evaluate(
    ([source, samples]) => {
      const letter = new RegExp(source, 'i');
      const found = new Set<string>();
      const look = (text: string | null) => {
        for (const line of (text ?? '').split('\n'))
          if (letter.test(line.replace('Français', '')) && !samples.includes(line.trim())) found.add(line.trim());
      };
      look(document.body.innerText);
      for (const el of document.querySelectorAll('[aria-label],[title],[placeholder],[alt]'))
        for (const a of ['aria-label', 'title', 'placeholder', 'alt']) look(el.getAttribute(a));
      return [...found];
    },
    [FRENCH_LETTER.source, SAMPLE_NAMES] as [string, string[]],
  );

test('I18N-F13 couverture : aucun texte français ne reste visible en persan (connexion, coquille, Réglages, serveur et ses paramètres), ni dans les noms accessibles', async ({ page, signedIn, api }) => {
  const left: Record<string, string[]> = {};
  const check = async (p: Page, where: string) => {
    await p.waitForLoadState('networkidle');
    const found = await frenchLeft(p);
    if (found.length) left[where] = found;
  };

  // Hors connexion : les trois écrans de la carte, la langue choisie dans le navigateur de l'appareil (enregistrée).
  await page.goto('/login');
  await page.evaluate(() => localStorage.setItem('kraken.lang', 'fa'));
  for (const path of ['/login', '/register', '/forgot-password']) {
    await page.goto(path);
    await expect(html(page)).toHaveAttribute('dir', 'rtl');
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
    await check(page, path);
  }

  // Connecté : l'accueil, chaque onglet des Réglages, un serveur (créé en persan, comme le fait l'interface) et ses paramètres.
  const a = await signedIn('fa-IR');
  const { page: app } = a;
  await expect(html(app)).toHaveAttribute('dir', 'rtl');
  await check(app, '/channels/@me');
  const settings = ['account', 'profiles', 'privacy', 'messaging', 'apps', 'notifications', 'voice', 'appearance', 'accessibility', 'keybinds', 'language'];
  for (const slug of settings) {
    await app.goto(`/settings/${slug}`);
    await expect(app.getByRole('dialog').getByRole('heading', { level: 2 }).first()).toBeVisible();
    await check(app, `/settings/${slug}`);
  }
  const as = await api(a.token);
  const created = await as.post(`${API}/guilds`, { data: { name: 'Corail', template: 'communaute', lang: 'fa' } });
  expect(created.status(), await created.text()).toBe(201);
  const g = await created.json();
  const text = g.channels.find((c: { kind: string }) => c.kind === 'text').id;
  await app.goto(`/channels/${g.guild.id}/${text}`);
  await expect(app.getByRole('heading', { level: 1 })).toBeVisible();
  await check(app, 'serveur : salon');
  for (const slug of ['overview', 'roles', 'channels', 'emojis', 'invites', 'bans', 'audit', 'owner']) {
    await app.goto(`/guilds/${g.guild.id}/settings/${slug}`);
    await expect(app.getByRole('dialog').getByRole('heading', { level: 2 }).first()).toBeVisible();
    await check(app, `/guilds/…/settings/${slug}`);
  }
  expect(left, 'texte encore en français : écran → lignes').toEqual({});
});
