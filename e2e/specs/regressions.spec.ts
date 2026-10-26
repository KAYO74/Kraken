// Régressions : bugs trouvés à la relecture du code, chacun avec le test qui l'a d'abord fait échouer.
import { test, expect, createGuild, messages, API } from '../fixtures';

test('SEC-1 ?next= ne sort jamais de l’instance (barre oblique inverse, tabulation)', async ({ user }) => {
  const { page } = await user('ivo');
  const origin = new URL(page.url()).origin;
  await page.route((url) => url.hostname.endsWith('evil.example'), (r) => r.abort());
  for (const next of ['/%5Cevil.example', '/%09/evil.example', '/%5C%5Cevil.example']) {
    await page.goto(`/login?next=${next}`);
    await page.waitForURL('**/channels/@me');
    expect(new URL(page.url()).origin).toBe(origin);
  }
});

test('CHAT-2 un message reçu ne ramène pas en bas qui lit l’historique', async ({ user, api }) => {
  const u = await user('ula');
  const { page } = u;
  const [, cid] = await createGuild(page, 'Fosse des archives');
  const as = await api(u.token);
  for (let i = 0; i < 30; i++) {
    expect((await as.post(`${API}/channels/${cid}/messages`, { data: { content: `ancien ${i}` } })).status()).toBe(201);
  }
  await page.reload();
  await expect(messages(page).filter({ hasText: 'ancien 29' })).toBeVisible();
  const log = page.getByRole('log');
  await log.evaluate((el) => (el.scrollTop = 0));
  await as.post(`${API}/channels/${cid}/messages`, { data: { content: 'tout nouveau' } });
  await expect(messages(page).filter({ hasText: 'tout nouveau' })).toHaveCount(1);
  await page.waitForTimeout(300);
  expect(await log.evaluate((el) => el.scrollTop)).toBeLessThan(50);
});

test('NAV-1 replier une catégorie cache ses salons', async ({ user }) => {
  const { page } = await user('nils');
  await createGuild(page, 'Salle des cartes');
  const voice = page.getByRole('link', { name: 'salon vocal Général' });
  await expect(voice).toBeVisible();
  await page.getByRole('button', { name: 'Salons vocaux' }).click();
  await expect(voice).toBeHidden();
});

test('MOB-1 sous 1024 px la liste des membres s’ouvre et son bouton dit vrai', async ({ user }) => {
  const { page } = await user('mia');
  await page.setViewportSize({ width: 900, height: 800 });
  await createGuild(page, 'Petit écran');
  const toggle = page.getByRole('button', { name: 'Liste des membres' });
  const aside = page.getByRole('complementary', { name: 'Membres' });
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
  await expect(aside).toBeHidden();
  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-expanded', 'true');
  await expect(aside).toBeVisible();
});

test('A11Y-1 les modales prennent le focus ; Échap ferme la plus haute seulement', async ({ user }) => {
  const { page } = await user('ali');
  await page.getByRole('button', { name: 'Mon compte' }).click();
  await page.getByRole('link', { name: 'Réglages utilisateur' }).click();
  const settings = page.getByRole('dialog', { name: 'Réglages' });
  await expect(page.getByRole('button', { name: 'Fermer les réglages (Échap)' })).toBeFocused();
  await settings.getByRole('button', { name: 'Supprimer le compte' }).click();
  const confirm = page.getByRole('alertdialog', { name: 'Supprimer ton compte ?' });
  await expect(confirm.getByRole('button', { name: 'Annuler' })).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(confirm).toBeHidden();
  await expect(settings).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(settings).toBeHidden();
});

test('A11Y-2 le lien d’évitement met le focus sur le contenu', async ({ user }) => {
  const { page } = await user('eli');
  await page.keyboard.press('Tab');
  await expect(page.getByRole('link', { name: 'Aller au contenu' })).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(page.locator('#main')).toBeFocused();
});
