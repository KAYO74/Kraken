// F02 Ajouter un ami (liste et filtres ; l'envoi de demande n'est pas encore construit)
// et F03 Ouvrir un MP (l'appel n'est pas encore construit).
import { test, expect, a11y, messages } from '../fixtures';

test('F02-H1 onglets d’amis : en ligne, tous, en attente', async ({ demo }) => {
  const { page } = await demo('demo');
  const tab = (name: string) => page.getByRole('tab', { name });
  await expect(tab('En ligne')).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByRole('heading', { name: /^En ligne — \d+$/ })).toBeVisible();
  await a11y(page);
  await tab('Tous').click();
  await expect(page.getByRole('heading', { name: 'Tous — 4' })).toBeVisible();
  await tab('En attente').click();
  await expect(page.getByRole('heading', { name: 'En attente — 1' })).toBeVisible();
  await expect(page.getByText('Demande reçue')).toBeVisible();
});

test('F02-E1 filtre par nom, insensible à la casse, état vide clair', async ({ demo }) => {
  const { page } = await demo('demo');
  await page.getByRole('tab', { name: 'Tous' }).click();
  const search = page.getByLabel('Chercher parmi tes amis');
  await search.fill('LOU');
  await expect(page.getByRole('heading', { name: 'Tous — 1' })).toBeVisible();
  await search.fill('personne-de-ce-nom');
  await expect(page.getByRole('heading', { name: 'Tous — 0' })).toBeVisible();
});

test('F02-E2 nouveau compte : listes vides expliquées', async ({ user }) => {
  const { page } = await user('nils');
  await expect(page.getByText('Personne en ligne pour l\'instant')).toBeVisible();
  await page.getByRole('tab', { name: 'Tous' }).click();
  await expect(page.getByText('Pas encore d\'amis')).toBeVisible();
});

test('F02-N1 l’ajout d’ami est annoncé comme indisponible, pas cassé', async ({ demo }) => {
  const { page } = await demo('demo');
  await page.getByRole('tab', { name: 'Ajouter un ami' }).click();
  await expect(page.getByLabel('Pseudo de ton ami')).toBeDisabled();
  await expect(page.getByRole('button', { name: 'Envoyer la demande' })).toBeDisabled();
});

test('F03-H1 ouvrir un MP depuis la liste d’amis et écrire', async ({ demo }) => {
  const { page } = await demo('demo');
  await page.getByRole('tab', { name: 'Tous' }).click();
  await page.getByRole('link', { name: 'Écrire à Lou' }).click();
  await expect(page.getByRole('heading', { name: 'Lou', level: 1 })).toBeVisible();
  await a11y(page);
  const text = `Coucou Lou ${Date.now()}`;
  await page.getByRole('textbox').last().fill(text);
  await page.keyboard.press('Enter');
  await expect(messages(page).filter({ hasText: text })).toHaveCount(1);
});

test('F03-E1 le MP de groupe s’ouvre depuis la liste des conversations', async ({ demo }) => {
  const { page } = await demo('demo');
  await page.getByRole('navigation', { name: 'Messages privés' }).getByRole('link', { name: /Équipée du soir/ }).click();
  await expect(page.getByRole('heading', { name: 'Équipée du soir', level: 1 })).toBeVisible();
  await expect(messages(page).first()).toBeVisible();
});

test('F03-N1 un MP entre deux autres comptes reste invisible', async ({ demo, user, allow }) => {
  allow.push(/status of 404/);
  const lou = await demo('lou');
  await lou.page.getByRole('navigation', { name: 'Messages privés' }).getByRole('link', { name: /Démo/ }).click();
  const dm = new URL(lou.page.url()).pathname;
  const intruder = await user('oscar');
  await intruder.page.goto(dm);
  await expect(intruder.page.getByRole('heading', { name: 'Démo', level: 1 })).toHaveCount(0);
  const cid = dm.split('/').pop();
  expect((await intruder.context.request.get(`/api/v1/channels/${cid}/messages`)).status()).toBe(404);
});
