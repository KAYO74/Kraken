// Langues de l'interface (PR #31) : français par défaut, anglais automatique d'après le navigateur, choix dans
// Réglages › Langue et heure (et sous la carte de connexion), rechargement qui rend focus et phrase d'état,
// dates et heures dans la langue choisie, initiales d'avatar, sélecteur grisé pendant un appel.
// Les écrans migrés dans ce lot : connexion et inscription, coquille (rail, barres latérales, compte),
// Réglages (fenêtre, Apparence, Accessibilité, Langue et heure).
import { test as base, expect, a11y, watch, API, PASSWORD, unique } from '../fixtures';
import type { BrowserContext, Page } from '@playwright/test';

type Account = { page: Page; context: BrowserContext; name: string; token: string };

/**
 * `signedIn(locale?, viewport?)` : un compte neuf, connecté dans un navigateur de cette langue, sans attendre un
 * titre de page (la fixture `user` attend « Amis », écrit en français : ces tests changent la langue).
 */
const test = base.extend<{ signedIn: (locale?: string, viewport?: { width: number; height: number }) => Promise<Account> }>({
  signedIn: async ({ browser, baseURL, issues, locale: browserLocale }, use) => {
    const contexts: BrowserContext[] = [];
    await use(async (locale = browserLocale ?? 'fr-FR', viewport = { width: 1440, height: 900 }) => {
      const context = await browser.newContext({ baseURL, locale, viewport });
      contexts.push(context);
      const name = unique('i18n');
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

// Micro et écran factices (un seul test entre en appel), sans fenêtre d'autorisation.
test.use({ launchOptions: { args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'] } });

const html = (p: Page) => p.locator('html');
const noScroll = async (p: Page) =>
  expect(await p.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(p.viewportSize()!.width);
const stored = (p: Page, key: string) => p.evaluate((k) => localStorage.getItem(k), key);
/** Le texte de la zone d'état de la coquille (il y en a une autre sur la page de langue : on filtre). */
const said = (p: Page, text: string) => p.getByRole('status').filter({ hasText: text });

// ───────── français par défaut ─────────

test('I18N-H1 français par défaut : connexion et inscription, <html lang="fr" dir="ltr">, sélecteur en pied de carte', async ({ page }) => {
  await page.goto('/login');
  await expect(html(page)).toHaveAttribute('lang', 'fr');
  await expect(html(page)).toHaveAttribute('dir', 'ltr');
  await expect(page.getByRole('heading', { name: 'Bon retour sous la surface', level: 1 })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Se connecter' })).toBeVisible();
  const language = page.getByRole('combobox', { name: 'Langue' });
  await expect(language).toHaveValue('auto');
  // « Langue de l'appareil », puis chaque langue dans son écriture, avec son attribut `lang`.
  expect(await language.locator('option').allTextContents()).toEqual(["Langue de l'appareil", 'Français', 'English', 'فارسی', 'Deutsch']);
  expect(await language.locator('option').evaluateAll((o) => o.map((x) => x.getAttribute('lang')))).toEqual([null, 'fr', 'en', 'fa', 'de']);
  await expect(language).toHaveAccessibleDescription('Le changement recharge la page.');
  await expect(page.getByText('Le changement recharge la page.')).toBeVisible();
  await a11y(page);
  await page.goto('/register');
  await expect(page.getByRole('heading', { name: 'Créer un compte', level: 1 })).toBeVisible();
  await expect(page.getByRole('combobox', { name: 'Langue' })).toBeVisible();
  await a11y(page);
});

test('I18N-H2 la langue du navigateur sans traduction retombe sur le français ; en-GB suit l’anglais et garde ses usages', async ({ browser, baseURL }) => {
  // Le néerlandais n'est pas pris en charge (l'allemand l'est depuis la 0.25.0 : voir `i18n-de.spec.ts`).
  const nl = await browser.newContext({ baseURL, locale: 'nl-NL' });
  const dutch = await nl.newPage();
  await dutch.goto('/login');
  await expect(html(dutch)).toHaveAttribute('lang', 'fr');
  await expect(dutch.getByRole('heading', { name: 'Bon retour sous la surface' })).toBeVisible();
  await nl.close();
  // Plusieurs langues : la première prise en charge (ici en-GB).
  const multi = await browser.newContext({ baseURL, locale: 'en-GB' });
  const gb = await multi.newPage();
  await gb.goto('/login');
  await expect(html(gb)).toHaveAttribute('lang', 'en');
  await expect(gb.getByRole('heading', { name: 'Welcome back' })).toBeVisible();
  await multi.close();
});

test('I18N-H3 un choix enregistré invalide est ignoré : « Automatique »', async ({ page }) => {
  await page.goto('/login');
  await page.evaluate(() => localStorage.setItem('kraken.lang', 'xx'));
  await page.reload();
  await expect(html(page)).toHaveAttribute('lang', 'fr');
  await expect(page.getByRole('combobox', { name: 'Langue' })).toHaveValue('auto');
});

// ───────── anglais automatique ─────────

test.describe('navigateur en anglais', () => {
  test.use({ locale: 'en-US' });

  test('I18N-H4 connexion et inscription en anglais, <html lang="en">, erreur du serveur traduite', async ({ page, allow }) => {
    allow.push(/401/);
    await page.goto('/login');
    await expect(html(page)).toHaveAttribute('lang', 'en');
    await expect(html(page)).toHaveAttribute('dir', 'ltr');
    await expect(page.getByRole('heading', { name: 'Welcome back', level: 1 })).toBeVisible();
    await expect(page.getByLabel('Username or email')).toBeVisible();
    await expect(page.getByLabel('Password')).toBeVisible();
    await expect(page.getByRole('link', { name: 'Forgot your password?' })).toBeVisible();
    await expect(page.getByRole('link', { name: 'No account yet? Sign up' })).toBeVisible();
    await expect(page.getByRole('link', { name: 'Privacy' })).toBeVisible();
    await expect(page.getByRole('link', { name: 'Terms' })).toBeVisible();
    const language = page.getByRole('combobox', { name: 'Language' });
    await expect(language).toHaveValue('auto');
    expect(await language.locator('option').allTextContents()).toEqual(['Device language', 'Français', 'English', 'فارسی', 'Deutsch']);
    await expect(language).toHaveAccessibleDescription('Changing it reloads the page.');
    await a11y(page);
    // Un message du serveur (français, statique) s'affiche traduit.
    await page.getByLabel('Username or email').fill('personne_inconnue');
    await page.getByLabel('Password').fill('mauvais-mot-de-passe');
    await page.getByRole('button', { name: 'Sign in' }).click();
    await expect(page.getByRole('alert')).toHaveText('Incorrect username or password.');
    await a11y(page);

    await page.getByRole('link', { name: 'No account yet? Sign up' }).click();
    await expect(page.getByRole('heading', { name: 'Create an account', level: 1 })).toBeVisible();
    await expect(page.getByLabel('Email')).toBeVisible();
    await expect(page.getByRole('textbox', { name: 'Username' })).toHaveAccessibleDescription(/Letters, numbers, _ and \. only/);
    await expect(page.getByRole('button', { name: 'Create my account' })).toBeVisible();
    await expect(page.getByRole('link', { name: 'Already have an account? Sign in' })).toBeVisible();
    await a11y(page);
  });

  test('I18N-H5 la coquille en anglais : rail, messages privés, compte, statuts, Réglages', async ({ signedIn }) => {
    const { page } = await signedIn();
    await expect(html(page)).toHaveAttribute('lang', 'en');
    await expect(page.getByRole('navigation', { name: 'Servers' })).toBeVisible();
    await expect(page.getByRole('link', { name: 'Home and direct messages' })).toBeVisible();
    await expect(page.getByRole('link', { name: 'Create or join a server' })).toBeVisible();
    await expect(page.getByRole('link', { name: 'Explore public servers' })).toBeVisible();
    const sidebar = page.getByRole('navigation', { name: 'Direct messages' });
    await expect(sidebar.getByRole('link', { name: 'Friends' })).toBeVisible();
    await expect(sidebar.getByRole('heading', { name: 'Direct messages' })).toBeVisible();
    await expect(sidebar.getByRole('button', { name: 'New conversation or group' })).toBeVisible();
    await expect(sidebar.getByText('No conversations yet.')).toBeVisible();
    await expect(page.getByRole('link', { name: 'Skip to content' })).toBeAttached();
    await a11y(page);
    // La carte du compte.
    await page.getByRole('button', { name: /^My account, online/ }).click();
    const card = page.getByRole('group', { name: 'My account' });
    await expect(card.getByRole('group', { name: 'My status' }).getByRole('radio')).toHaveCount(4);
    for (const label of ['Online', 'Idle', 'Do not disturb', 'Invisible']) await expect(card.getByText(label, { exact: true })).toBeVisible();
    await expect(card.getByText('Others see you as offline.')).toBeVisible();
    await expect(card.getByText('Connected', { exact: true })).toBeVisible();
    await expect(card.getByRole('link', { name: 'User settings' })).toBeVisible();
    await expect(card.getByRole('button', { name: 'Sign out' })).toBeVisible();
    await a11y(page);
    // Un statut change le nom du bouton, dans la langue.
    await card.getByText('Do not disturb', { exact: true }).click();
    await page.keyboard.press('Escape');
    await expect(page.getByRole('button', { name: /^My account, do not disturb/ })).toBeVisible();
    // Réglages : la fenêtre, ses sections, Apparence.
    await page.getByRole('button', { name: /^My account/ }).click();
    await card.getByRole('link', { name: 'Appearance' }).click();
    const settings = page.getByRole('dialog', { name: 'Settings' });
    await expect(settings.getByRole('navigation', { name: 'Settings sections' }).getByRole('link', { name: 'Language and time' })).toBeVisible();
    await expect(settings.getByRole('heading', { name: 'Appearance', level: 2 })).toBeVisible();
    await expect(settings.getByRole('group', { name: 'Theme' }).getByRole('radio', { name: 'Dark', exact: true })).toBeChecked();
    await expect(settings.getByRole('group', { name: 'Text size' })).toBeVisible();
    await expect(settings.getByRole('combobox', { name: 'Interface zoom' })).toBeVisible();
    await expect(settings.getByRole('button', { name: 'Close settings (Esc)' })).toBeVisible();
    await a11y(page);
    await settings.getByRole('link', { name: 'Accessibility' }).click();
    await expect(settings.getByRole('group', { name: 'Motion' })).toBeVisible();
    await expect(settings.getByRole('switch', { name: 'Underline links' })).toBeVisible();
    await a11y(page);
  });

  test('I18N-H6 Réglages › Langue et heure : le sélecteur recharge la page, rend le focus et dit la langue', async ({ signedIn }) => {
    const { page } = await signedIn();
    await page.goto('/settings/language');
    const dialog = page.getByRole('dialog', { name: 'Settings' });
    const language = dialog.getByRole('combobox', { name: 'Language' });
    await expect(language).toHaveValue('auto');
    await expect(language).toHaveAccessibleDescription('Changing it reloads the page.');
    expect(await language.locator('option').allTextContents()).toEqual(['Device language', 'Français', 'English', 'فارسی', 'Deutsch']);
    await expect(dialog.getByRole('group', { name: 'Time format' })).toBeVisible();
    // Le navigateur est anglais, l'heure automatique est celle de l'anglais américain.
    await expect(said(page, 'Example').first()).toHaveText('Example: 2:30 PM');
    await a11y(page);

    // Français malgré le navigateur : la page recharge, même page, focus sur le sélecteur, phrase dite en français.
    await language.selectOption('fr');
    await expect(html(page)).toHaveAttribute('lang', 'fr');
    const dialogFr = page.getByRole('dialog', { name: 'Réglages' });
    const languageFr = dialogFr.getByRole('combobox', { name: 'Langue' });
    await expect(languageFr).toHaveValue('fr');
    await expect(languageFr).toBeFocused();
    await expect(said(page, 'Langue : Français.')).toBeVisible();
    expect(page.url()).toContain('/settings/language');
    expect(await stored(page, 'kraken.lang')).toBe('fr');
    // L'heure automatique suit la langue de l'interface, pas celle du navigateur : 24 h.
    await expect(said(page, 'Exemple').first()).toHaveText('Exemple : 14:30');
    expect(await page.evaluate(() => sessionStorage.getItem('kraken.lang-flag'))).toBeNull();
    await a11y(page);
    // Le choix reste après un rechargement, sans nouvelle phrase ni focus volé.
    await page.reload();
    await expect(html(page)).toHaveAttribute('lang', 'fr');
    await expect(languageFr).toHaveValue('fr');
    await expect(languageFr).not.toBeFocused();
    await expect(said(page, 'Langue : Français.')).toHaveCount(0);

    // English : même chemin dans l'autre sens, chaque option garde son écriture.
    await languageFr.selectOption('en');
    await expect(html(page)).toHaveAttribute('lang', 'en');
    await expect(dialog.getByRole('combobox', { name: 'Language' })).toHaveValue('en');
    await expect(dialog.getByRole('combobox', { name: 'Language' })).toBeFocused();
    await expect(said(page, 'Language: English.')).toBeVisible();
    expect(await stored(page, 'kraken.lang')).toBe('en');
    // « Langue de l'appareil » : retour au navigateur.
    await dialog.getByRole('combobox', { name: 'Language' }).selectOption('auto');
    await expect(dialog.getByRole('combobox', { name: 'Language' })).toHaveValue('auto');
    expect(await stored(page, 'kraken.lang')).toBe('auto');
  });

  test('I18N-H7 en-GB : l’anglais garde les usages du navigateur (24 h)', async ({ signedIn }) => {
    const { page } = await signedIn('en-GB');
    await page.goto('/settings/language');
    await expect(html(page)).toHaveAttribute('lang', 'en');
    await expect(said(page, 'Example').first()).toHaveText('Example: 14:30');
  });

  test('I18N-H13 à 390 px, après un changement de langue, l’onglet « Langue et heure » est dans la vue', async ({ signedIn }) => {
    const { page } = await signedIn('en-US', { width: 390, height: 844 });
    await page.goto('/settings/language');
    const tab = page.locator('.k-settings nav a[aria-current="page"]');
    await expect(tab).toHaveText('Language and time');
    await expect(tab).toBeInViewport({ ratio: 1 });
    // La page se recharge dans l'autre langue, sur le même onglet : il reste en vue (et dans l'autre sens aussi).
    await page.getByRole('dialog', { name: 'Settings' }).getByRole('combobox', { name: 'Language' }).selectOption('fr');
    await expect(html(page)).toHaveAttribute('lang', 'fr');
    await expect(tab).toHaveText('Langue et heure');
    await expect(tab).toBeInViewport({ ratio: 1 });
    await page.getByRole('dialog', { name: 'Réglages' }).getByRole('combobox', { name: 'Langue' }).selectOption('en');
    await expect(html(page)).toHaveAttribute('lang', 'en');
    await expect(tab).toHaveText('Language and time');
    await expect(tab).toBeInViewport({ ratio: 1 });
  });

  test('I18N-H14 messages du serveur à valeur en anglais : le gabarit est traduit, la valeur et le libellé du champ suivent', async ({ signedIn, allow }) => {
    allow.push(/status of (40[0-9]|429)/); // le pseudo inconnu, le caractère invisible, le délai (429) : refusés exprès
    const { page: p } = await signedIn('en-US');
    const french = /Tu as|Réessaie|Aucun compte|caractères|invisibles|Nom affiché|Pronoms/;

    // Un pseudo bien formé que personne ne porte : la valeur, mise en minuscules par le serveur, reste telle quelle.
    await p.goto('/channels/@me');
    await p.getByRole('tablist', { name: 'Filter friends' }).getByRole('tab', { name: 'Add a friend' }).click();
    const add = p.getByRole('tabpanel', { name: 'Add a friend' });
    await add.getByLabel("Your friend's username").fill('Nobody.Here_2');
    await add.getByRole('button', { name: 'Send request' }).click();
    await expect(add.getByRole('alert')).toHaveText('No account has the username "nobody.here_2". Check the spelling.');
    expect(await p.locator('body').innerText()).not.toMatch(french);

    // Un caractère invisible dans le profil : le libellé du champ (français côté serveur) est traduit lui aussi.
    await p.goto('/settings/profiles');
    const form = p.getByRole('form', { name: 'What others see' });
    const alert = form.getByRole('alert');
    await form.getByLabel('Display name').fill('a\u200Bb');
    await form.getByRole('button', { name: 'Save' }).click();
    await expect(alert).toHaveText("Display name: invisible characters (zero-width space, text direction change, filler) aren't allowed.");
    await form.getByLabel('Display name').fill('');
    await form.getByLabel('Pronouns').fill('a\u200Bb');
    await form.getByRole('button', { name: 'Save' }).click();
    await expect(alert).toHaveText("Pronouns: invisible characters (zero-width space, text direction change, filler) aren't allowed.");
    expect(await p.locator('body').innerText()).not.toMatch(french);

    // Le délai entre deux changements de pseudo : deux changements permis par heure, le troisième est refusé avec l'attente.
    await p.goto('/settings/account');
    const username = p.getByRole('form', { name: 'Username' });
    for (const _ of [1, 2]) {
      await username.getByLabel('New username').fill(unique('i18nc'));
      await username.getByLabel('Current password').fill(PASSWORD);
      await username.getByRole('button', { name: 'Change username' }).click();
      await expect(username.getByRole('status').filter({ hasText: 'Your username is now @' })).toBeVisible();
    }
    await username.getByLabel('New username').fill(unique('i18nc'));
    await username.getByLabel('Current password').fill(PASSWORD);
    await username.getByRole('button', { name: 'Change username' }).click();
    await expect(username.getByRole('alert')).toHaveText(/^You've already changed your username 2 times in an hour\. Try again in \d+ (s|min)\.$/);
    expect(await p.locator('body').innerText()).not.toMatch(french);
  });
});

// ───────── le choix d'une langue depuis un navigateur français ─────────

test('I18N-H8 anglais choisi depuis un navigateur français : la coquille, les dates longues et l’heure suivent la langue', async ({ user, api }) => {
  const a = await user('dates');
  const { page } = a;
  const created = await (await api(a.token)).post(`${API}/guilds`, { data: { name: 'Calendrier' } });
  expect(created.status()).toBe(201);
  const g = await created.json();
  const text = g.channels.find((c: { kind: string }) => c.kind === 'text').id;
  await page.evaluate(() => localStorage.setItem('kraken.lang', 'en'));
  await page.goto(`/channels/${g.guild.id}/${text}`);
  await expect(html(page)).toHaveAttribute('lang', 'en');
  await expect(page.getByRole('navigation', { name: 'Channels' })).toBeVisible();
  // La carte de profil : « Membre depuis » est une date longue de la langue de l'interface.
  const handle = page.getByRole('button', { name: new RegExp(`@${a.name}`) });
  await handle.click();
  const when = page.locator('.app-card-dates dd').first();
  await expect(when).toHaveText(/^[A-Z][a-z]+ \d{1,2}, \d{4}$/);
  await page.keyboard.press('Escape');
  await page.evaluate(() => localStorage.setItem('kraken.lang', 'fr'));
  await page.reload();
  await expect(html(page)).toHaveAttribute('lang', 'fr');
  await page.getByRole('button', { name: new RegExp(`@${a.name}`) }).click();
  await expect(page.locator('.app-card-dates dd').first()).toHaveText(/^\d{1,2}(er)? [a-zéûè]+ \d{4}$/);
});

// ───────── connexion : ce qui est saisi reste, jamais le mot de passe ─────────

test('I18N-H9 changer de langue sur /login garde l’identifiant, vide le mot de passe, rend le focus', async ({ page }) => {
  await page.goto('/login');
  await page.getByLabel('Pseudo ou e-mail').fill('perle@example.org');
  await page.getByLabel('Mot de passe').fill('un-secret-bien-gardé');
  await page.getByRole('combobox', { name: 'Langue' }).selectOption('en');
  await expect(html(page)).toHaveAttribute('lang', 'en');
  await expect(page.getByLabel('Username or email')).toHaveValue('perle@example.org');
  await expect(page.getByLabel('Password')).toHaveValue('');
  await expect(page.getByRole('combobox', { name: 'Language' })).toBeFocused();
  await expect(page.getByRole('combobox', { name: 'Language' })).toHaveValue('en');
  // Rien ne traîne dans le stockage de l'onglet, et surtout pas le mot de passe.
  const kept = await page.evaluate(() => Object.entries(sessionStorage));
  expect(JSON.stringify(kept)).not.toContain('un-secret');
  expect(kept).toEqual([]);
  // Une seconde fois : l'identifiant vidé ne revient pas de lui-même.
  await page.getByLabel('Username or email').fill('');
  await page.getByRole('combobox', { name: 'Language' }).selectOption('fr');
  await expect(html(page)).toHaveAttribute('lang', 'fr');
  await expect(page.getByLabel('Pseudo ou e-mail')).toHaveValue('');

  // Inscription : e-mail et pseudo gardés, mot de passe jamais.
  await page.goto('/register');
  await page.getByLabel('E-mail').fill('lou@example.org');
  await page.getByRole('textbox', { name: 'Pseudo' }).fill('lou_marin');
  await page.getByLabel('Mot de passe').fill('un-autre-secret');
  await page.getByRole('combobox', { name: 'Langue' }).selectOption('en');
  await expect(html(page)).toHaveAttribute('lang', 'en');
  await expect(page.getByLabel('Email')).toHaveValue('lou@example.org');
  await expect(page.getByRole('textbox', { name: 'Username' })).toHaveValue('lou_marin');
  await expect(page.getByLabel('Password')).toHaveValue('');
  expect(await page.evaluate(() => Object.entries(sessionStorage))).toEqual([]);
});

// ───────── pendant un appel ─────────

test('I18N-H10 en appel, le sélecteur de langue est grisé avec sa raison visible ; hors appel, il ne l’est plus', async ({ user, api }) => {
  test.setTimeout(90_000);
  const a = await user('appel');
  const { page } = a;
  const created = await (await api(a.token)).post(`${API}/guilds`, { data: { name: 'Cloche' } });
  expect(created.status()).toBe(201);
  const g = await created.json();
  const text = g.channels.find((c: { kind: string }) => c.kind === 'text').id;
  await page.goto(`/channels/${g.guild.id}/${text}`);
  await page.getByRole('link', { name: 'salon vocal Général', exact: true }).click();
  await page.getByRole('button', { name: 'Rejoindre', exact: true }).click();
  await expect(page.getByRole('region', { name: 'Connexion vocale' }).getByRole('status')).toHaveText('En appel', { timeout: 20_000 });
  // Dans l'appli (un rechargement couperait l'appel) : Compte › Réglages › Langue et heure.
  await page.getByRole('button', { name: /^Mon compte/ }).click();
  await page.getByRole('group', { name: 'Mon compte' }).getByRole('link', { name: 'Réglages utilisateur' }).click();
  const dialog = page.getByRole('dialog', { name: 'Réglages' });
  await dialog.getByRole('link', { name: 'Langue et heure' }).click();
  const language = dialog.getByRole('combobox', { name: 'Langue' });
  await expect(language).toBeDisabled();
  await expect(dialog.getByText("Quitte l'appel pour changer de langue.")).toBeVisible();
  await expect(language).toHaveAccessibleDescription(/Quitte l'appel pour changer de langue\./);
  await a11y(page);
  // Quitter l'appel : le sélecteur redevient actif, sans recharger.
  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: 'Quitter le salon vocal' }).click();
  await expect(page.getByRole('button', { name: 'Quitter le salon vocal' })).toHaveCount(0);
  await page.getByRole('button', { name: /^Mon compte/ }).click();
  await page.getByRole('group', { name: 'Mon compte' }).getByRole('link', { name: 'Réglages utilisateur' }).click();
  await dialog.getByRole('link', { name: 'Langue et heure' }).click();
  await expect(dialog.getByRole('combobox', { name: 'Langue' })).toBeEnabled();
  await expect(dialog.getByText("Quitte l'appel pour changer de langue.")).toHaveCount(0);
});

// ───────── initiales d'avatar : premier graphème ─────────

test('I18N-H11 les initiales d’un avatar sont des graphèmes entiers : persan, japonais, emoji lié, accents', async ({ user, api }) => {
  const a = await user('initiales');
  const { page } = a;
  const asA = await api(a.token);
  const ZWNJ = '‌';
  const family = '👨‍👩‍👧';
  const cases: [string, string][] = [
    ['ada lovelace', 'AL'],
    ['علی رضایی', 'عر'],
    [`می${ZWNJ}خواهم کتاب${ZWNJ}خانه`, 'مک'],
    ['山田 太郎', '山太'],
    [`${family} Lou`, `${family}L`],
    ['élodie dupont', 'ÉD'],
  ];
  for (const [display, initials] of cases) {
    const res = await asA.patch(`${API}/users/@me`, { data: { display_name: display } });
    expect(res.status(), await res.text()).toBe(200);
    await page.reload();
    const avatar = page.getByRole('button', { name: /^Mon compte/ }).locator('.k-avatar');
    await expect(avatar).toHaveText(initials);
  }
});

// ───────── petits écrans, grands textes ─────────

for (const [name, locale] of [['français', 'fr-FR'], ['anglais', 'en-US']] as const) {
  test.describe(`390 px, texte 20, ${name}`, () => {
    test.use({ locale, viewport: { width: 390, height: 844 } });

    test(`I18N-H12 rien ne déborde : connexion, inscription, coquille et Réglages (${name})`, async ({ page, signedIn }) => {
      await page.goto('/login');
      await page.evaluate(() => localStorage.setItem('kraken.text', '20'));
      for (const path of ['/login', '/register', '/forgot-password']) {
        await page.goto(path);
        await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
        await noScroll(page);
      }
      await page.goto('/login');
      await a11y(page);
      const { page: app } = await signedIn(locale, { width: 390, height: 844 });
      await app.evaluate(() => localStorage.setItem('kraken.text', '20'));
      await app.goto('/settings/language');
      await expect(app.getByRole('combobox', { name: /^(Langue|Language)$/ })).toBeVisible();
      await noScroll(app);
      await a11y(app);
      // La barre d'onglets défile de côté : l'onglet ouvert (« Langue et heure », le dernier) est dans la vue.
      const tab = app.locator('.k-settings nav a[aria-current="page"]');
      await expect(tab).toHaveText(/^(Langue et heure|Language and time)$/);
      await expect(tab).toBeInViewport({ ratio: 1 });
      await app.goto('/settings/appearance');
      await expect(app.getByRole('group', { name: /^(Thème|Theme)$/ })).toBeVisible();
      await noScroll(app);
    });
  });
}
