// F08 Personnaliser son profil : le nom affiché (le pseudo unique reste écrit en dessous).
import { test, expect, a11y, createGuild, say, messages, API } from '../fixtures';

test('F08-H1 changer son nom affiché : partout, en direct, @pseudo dessous', async ({ user, api }) => {
  const a = await user('pablo');
  const b = await user('quinn');
  const [, cid] = await createGuild(a.page, 'Atelier');
  const inv = await (await (await api(a.token)).post(`${API}/channels/${cid}/invites`, { data: {} })).json();
  await b.page.goto(`/invite/${inv.code}`);
  await b.page.getByRole('button', { name: 'Rejoindre le serveur' }).click();
  await say(a.page, 'général', 'Avant le changement');
  await expect(messages(b.page).filter({ hasText: 'Avant le changement' })).toHaveCount(1);

  await a.page.goto('/settings/account');
  await a11y(a.page);
  const field = a.page.getByLabel('Nom affiché');
  await expect(field).toHaveAttribute('placeholder', a.name);
  await field.fill('Pablo des grands fonds 🐙');
  await a.page.getByRole('button', { name: 'Enregistrer' }).click();
  await expect(a.page.getByRole('status')).toHaveText('Nom affiché enregistré.');
  await expect(a.page.getByText(`@${a.name}`).first()).toBeVisible();

  // Le panneau utilisateur : nom affiché, pseudo unique en dessous.
  await a.page.getByRole('button', { name: 'Fermer les réglages (Échap)' }).click();
  await expect(a.page.getByText('Pablo des grands fonds 🐙').first()).toBeVisible();
  await expect(a.page.getByText(`@${a.name}`).first()).toBeVisible();

  // Chez l'autre membre, sans recharger : liste des membres et auteur du message.
  const members = b.page.getByRole('complementary', { name: 'Membres' });
  await expect(members.getByText('Pablo des grands fonds 🐙')).toBeVisible();
  await expect(members.getByText(`@${a.name}`)).toBeVisible();
  await expect(messages(b.page).filter({ hasText: 'Avant le changement' })).toContainText('Pablo des grands fonds 🐙');
});

test('F08-E1 nom vide : retour au pseudo unique', async ({ user }) => {
  const { page, name } = await user('rita');
  await page.goto('/settings/account');
  const field = page.getByLabel('Nom affiché');
  await field.fill('Rita');
  await page.getByRole('button', { name: 'Enregistrer' }).click();
  await expect(page.getByRole('status')).toBeVisible();
  await field.fill('   ');
  await page.getByRole('button', { name: 'Enregistrer' }).click();
  await expect(field).toHaveValue('');
  await page.reload();
  await expect(page.getByLabel('Nom affiché')).toHaveValue('');
  await expect(page.getByRole('heading', { level: 1 }).or(page.getByText(name).first())).toBeVisible();
});

test('F08-N1 nom trop long : coupé à 32 dans le champ, refusé par l’API', async ({ user, api }) => {
  const u = await user('sven');
  await u.page.goto('/settings/account');
  await u.page.getByLabel('Nom affiché').fill('n'.repeat(40));
  await expect(u.page.getByLabel('Nom affiché')).toHaveValue('n'.repeat(32));
  const res = await (await api(u.token)).patch(`${API}/users/@me`, { data: { display_name: 'n'.repeat(33) } });
  expect(res.status()).toBe(400);
});
