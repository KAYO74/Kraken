// F12 Inscription et connexion, plus la gestion du compte (déconnexion partout, suppression).
import { test, expect, a11y, unique, createGuild, PASSWORD, API } from '../fixtures';

const REJECTED = /status of 4\d\d/;

async function fillRegister(page: import('@playwright/test').Page, name: string, email = `${name}@example.org`) {
  await page.goto('/register');
  await page.getByLabel('E-mail').fill(email);
  await page.getByLabel('Pseudo').fill(name);
  await page.getByLabel('Mot de passe').fill(PASSWORD);
}

test('F12-H1 inscription puis arrivée sur l’accueil', async ({ page }) => {
  const name = unique('ana');
  await fillRegister(page, name);
  await a11y(page);
  await page.getByRole('button', { name: 'Créer mon compte' }).click();
  await page.waitForURL('**/channels/@me');
  await expect(page.getByRole('link', { name: 'Réglages utilisateur' })).toBeVisible();
  await expect(page.getByText(name).first()).toBeVisible();
  await a11y(page);
});

test('F12-H2 déconnexion puis connexion par pseudo et par e-mail', async ({ user }) => {
  const u = await user('bea');
  const { page } = u;
  for (const login of [u.name, u.email.toUpperCase()]) {
    await page.goto('/settings/account');
    await page.getByRole('button', { name: 'Se déconnecter', exact: true }).click();
    await page.waitForURL('**/login**');
    await page.getByLabel('Pseudo ou e-mail').fill(login);
    await page.getByLabel('Mot de passe').fill(PASSWORD);
    await page.getByRole('button', { name: 'Se connecter' }).click();
    await page.waitForURL('**/channels/@me');
  }
});

test('F12-N1 pseudo déjà pris, F12-N2 e-mail déjà utilisé', async ({ page, user, allow }) => {
  allow.push(REJECTED);
  const u = await user('cleo');
  await fillRegister(page, u.name, `autre_${u.name}@example.org`);
  await page.getByRole('button', { name: 'Créer mon compte' }).click();
  await expect(page.getByRole('alert')).toContainText(/pseudo/i);
  await fillRegister(page, unique('cleo'), u.email);
  await page.getByRole('button', { name: 'Créer mon compte' }).click();
  await expect(page.getByRole('alert')).toContainText(/adresse|e-mail/i);
  await expect(page).toHaveURL(/\/register/);
});

test('F12-N3 mauvais mot de passe : message clair, champ marqué invalide', async ({ page, user, allow }) => {
  allow.push(REJECTED);
  const u = await user('dan');
  await page.goto('/login');
  await page.getByLabel('Pseudo ou e-mail').fill(u.name);
  await page.getByLabel('Mot de passe').fill('pas-le-bon-mot');
  await page.getByRole('button', { name: 'Se connecter' }).click();
  await expect(page.getByRole('alert')).toBeVisible();
  await expect(page.getByLabel('Mot de passe')).toHaveAttribute('aria-invalid', 'true');
  await expect(page).toHaveURL(/\/login/);
});

test('F12-E1 pseudo avec accents, emoji ou trop long refusé', async ({ page, allow }) => {
  allow.push(REJECTED);
  await page.goto('/register');
  await page.getByLabel('Pseudo').fill('x'.repeat(40));
  await expect(page.getByLabel('Pseudo')).toHaveValue('x'.repeat(32));
  for (const bad of ['élodie', 'poulpe🐙', 'a b']) {
    await fillRegister(page, bad, `${unique('e')}@example.org`);
    await page.getByRole('button', { name: 'Créer mon compte' }).click();
    // Refusé par le navigateur (motif, longueur) ou, à défaut, par le serveur.
    const blocked = await page.getByLabel('Pseudo').evaluate((el: HTMLInputElement) => !el.checkValidity());
    if (!blocked) await expect(page.getByRole('alert')).toContainText(/pseudo/i);
    await expect(page).toHaveURL(/\/register/);
  }
});

test('F12-E2 double clic sur « Créer mon compte » : un seul compte, aucune erreur', async ({ page }) => {
  const name = unique('eve');
  await fillRegister(page, name);
  await page.getByRole('button', { name: 'Créer mon compte' }).dblclick();
  await page.waitForURL('**/channels/@me');
  await expect(page.getByRole('alert')).toHaveCount(0);
});

test('F12-E3 clavier seul : Tab puis Entrée suffisent pour se connecter', async ({ page, user }) => {
  const u = await user('fanny');
  await page.goto('/login');
  await expect(page.getByLabel('Pseudo ou e-mail')).toBeFocused();
  await page.keyboard.type(u.name);
  await page.keyboard.press('Tab');
  await page.keyboard.type(PASSWORD);
  await page.keyboard.press('Enter');
  await page.waitForURL('**/channels/@me');
});

test('F12-E4 session révoquée ailleurs : l’autre onglet revient à la connexion', async ({ browser, baseURL, user }) => {
  const u = await user('gus');
  const other = await browser.newContext({ baseURL });
  const res = await other.request.post(`${API}/auth/login`, { data: { login: u.name, password: PASSWORD } });
  expect(res.ok()).toBeTruthy();
  const second = await other.newPage();
  await second.goto('/login');
  await second.evaluate(() => localStorage.setItem('kraken.signed_in', '1'));
  await second.goto('/channels/@me');
  await expect(second.getByRole('navigation', { name: 'Serveurs' })).toBeVisible();

  await u.page.goto('/settings/account');
  await u.page.getByRole('button', { name: 'Se déconnecter partout' }).click();
  await u.page.waitForURL('**/login**');
  await second.waitForURL('**/login**', { timeout: 10_000 });
  await other.close();
});

test('F12-E7 changer de compte dans le même onglet : rien de l’ancien compte ne reste', async ({ user }) => {
  const a = await user('kim');
  const b = await user('lia');
  await createGuild(a.page, 'Repaire de Kim');
  await createGuild(b.page, 'Repaire de Lia');
  const { page } = a;
  await page.goto('/settings/account');
  await page.getByRole('button', { name: 'Se déconnecter', exact: true }).click();
  await page.waitForURL('**/login**');
  await page.getByLabel('Pseudo ou e-mail').fill(b.name);
  await page.getByLabel('Mot de passe').fill(PASSWORD);
  await page.getByRole('button', { name: 'Se connecter' }).click();
  await page.waitForURL('**/channels/@me');
  const rail = page.getByRole('navigation', { name: 'Serveurs' });
  await expect(rail.getByRole('link', { name: 'Serveur Repaire de Lia' })).toBeVisible();
  await expect(rail.getByRole('link', { name: 'Serveur Repaire de Kim' })).toHaveCount(0);
});

test('F12-E5 aller à une page protégée sans session renvoie à la connexion puis y revient', async ({ page, user }) => {
  const u = await user('hugo');
  await page.goto('/settings/appearance');
  await page.waitForURL(/\/login/);
  await page.getByLabel('Pseudo ou e-mail').fill(u.name);
  await page.getByLabel('Mot de passe').fill(PASSWORD);
  await page.getByRole('button', { name: 'Se connecter' }).click();
  await page.waitForURL('**/settings/appearance');
});

test('F12-N4 mot de passe oublié sans e-mail configuré : explication, pas de plantage', async ({ page, allow }) => {
  allow.push(/auth\/reset/, /status of 5\d\d/);
  await page.goto('/forgot-password');
  await a11y(page);
  await page.getByLabel('E-mail').fill('personne@example.org');
  await page.getByRole('button', { name: 'Envoyer le lien' }).click();
  await expect(page.getByRole('alert')).toContainText(/e-mail/i);
});

test('F12-N5 compte supprimé : plus de connexion possible', async ({ user, allow }) => {
  allow.push(REJECTED);
  const { page, name } = await user('ines');
  await page.goto('/settings/account');
  await a11y(page);
  await page.getByRole('button', { name: 'Supprimer le compte' }).click();
  const dialog = page.getByRole('alertdialog');
  await dialog.getByLabel('Mot de passe').fill(PASSWORD);
  await dialog.getByRole('button', { name: 'Supprimer définitivement' }).click();
  await page.waitForURL('**/login**');
  await page.getByLabel('Pseudo ou e-mail').fill(name);
  await page.getByLabel('Mot de passe').fill(PASSWORD);
  await page.getByRole('button', { name: 'Se connecter' }).click();
  await expect(page.getByRole('alert')).toBeVisible();
});

test.describe('largeur mobile', () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test('F12-E6 inscription à 390 px sans défilement horizontal', async ({ page }) => {
    await fillRegister(page, unique('jo'));
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
    await page.getByRole('button', { name: 'Créer mon compte' }).click();
    await page.waitForURL('**/channels/@me');
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
  });
});

test('F12-H3 Confidentialité et Conditions se lisent sans compte', async ({ page }) => {
  await page.goto('/login');
  await page.getByRole('link', { name: 'Confidentialité' }).click();
  await expect(page.getByRole('heading', { name: 'Confidentialité', level: 1 })).toBeVisible();
  await expect(page.getByText('argon2id')).toBeVisible();
  await a11y(page);
  await page.getByRole('link', { name: 'Conditions' }).click();
  await expect(page.getByRole('heading', { name: "Conditions d'utilisation", level: 1 })).toBeVisible();
  await expect(page.getByRole('link', { name: 'Code source de cette instance' })).toHaveAttribute('href', /^https:\/\//);
  await a11y(page);
});
