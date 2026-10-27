// F02 Ajouter un ami (demande par pseudo, accepter, refuser, annuler, retirer), F03 ouvrir un
// MP ou un groupe (choix d'amis, nom, ajout, renommer, quitter) et F11 bloquer puis débloquer.
// Comptes jetables : le compte de démo n'est que lu (Zed, qu'il a bloqué, est replié dans Abysse).
import { test, expect, a11y, messages, API, type User } from '../fixtures';
import type { APIRequestContext, Page } from '@playwright/test';

const tab = (page: Page, name: string) => page.getByRole('tab', { name });

/** Demande par l'API, acceptée par l'API : `a` et `b` deviennent amis. */
async function befriend(api: (t?: string) => Promise<APIRequestContext>, a: User, b: User) {
  const asA = await api(a.token);
  expect((await asA.post(`${API}/users/@me/relationships`, { data: { username: b.name } })).status()).toBe(201);
  const asB = await api(b.token);
  expect((await asB.put(`${API}/users/@me/relationships/${a.id}`, { data: { kind: 'friend' } })).status()).toBe(200);
}

async function addByName(page: Page, name: string) {
  await tab(page, 'Ajouter un ami').click();
  await page.getByLabel('Pseudo de ton ami').fill(name);
  await page.getByRole('button', { name: 'Envoyer la demande' }).click();
}

test('F02-H2 demande par pseudo, reçue en direct, acceptée des deux côtés', async ({ user, allow }) => {
  allow.push(/status of 409/);
  const a = await user('ami');
  const b = await user('amie');
  await addByName(a.page, `@${b.name.toUpperCase()}`);
  await expect(a.page.getByRole('status').filter({ hasText: 'Demande envoyée à' })).toBeVisible();
  await expect(a.page.getByLabel('Pseudo de ton ami')).toHaveValue('');

  // Chez b, sans recharger : l'onglet et le lien Amis annoncent la demande.
  await expect(tab(b.page, 'En attente 1 à accepter ou refuser')).toBeVisible();
  await expect(b.page.getByRole('link', { name: /Amis.*demande en attente/ })).toBeVisible();
  await tab(b.page, 'En attente').click();
  await a11y(b.page);
  await b.page.getByRole('button', { name: `Accepter la demande de ${a.name}` }).click();
  await expect(b.page.getByRole('heading', { name: 'En attente 0' })).toBeVisible();
  await tab(b.page, 'Tous').click();
  await expect(b.page.getByRole('button', { name: `Écrire à ${a.name}` })).toBeVisible();

  // Chez a aussi, en direct ; une seconde demande est refusée clairement.
  await tab(a.page, 'Tous').click();
  await expect(a.page.getByRole('heading', { name: 'Tous 1' })).toBeVisible();
  await addByName(a.page, b.name);
  await expect(a.page.getByRole('alert')).toHaveText('Vous êtes déjà amis.');
});

test('F02-E3 pseudo inconnu, le sien, demande en double : erreurs claires', async ({ user, allow }) => {
  allow.push(/status of 40[049]/);
  const a = await user('seul');
  const b = await user('autre');
  await addByName(a.page, 'personne_de_ce_nom');
  await expect(a.page.getByRole('alert')).toContainText('Aucun compte ne porte le pseudo « personne_de_ce_nom »');
  await expect(a.page.getByLabel('Pseudo de ton ami')).toHaveAttribute('aria-invalid', 'true');
  await a11y(a.page);
  await a.page.getByLabel('Pseudo de ton ami').fill(a.name);
  await a.page.getByRole('button', { name: 'Envoyer la demande' }).click();
  await expect(a.page.getByRole('alert')).toHaveText('C\'est ton propre pseudo.');
  await a.page.getByLabel('Pseudo de ton ami').fill(b.name);
  await a.page.getByRole('button', { name: 'Envoyer la demande' }).click();
  await expect(a.page.getByRole('status').filter({ hasText: 'Demande envoyée' })).toBeVisible();
  await a.page.getByLabel('Pseudo de ton ami').fill(b.name);
  await a.page.getByRole('button', { name: 'Envoyer la demande' }).click();
  await expect(a.page.getByRole('alert')).toContainText('Ta demande est déjà partie');
});

test('F02-E4 refuser une demande, en annuler une autre, retirer un ami', async ({ user, api }) => {
  const a = await user('dem');
  const b = await user('dest');
  const asA = await api(a.token);
  await asA.post(`${API}/users/@me/relationships`, { data: { username: b.name } });
  await tab(b.page, 'En attente').click();
  await b.page.getByRole('button', { name: `Refuser la demande de ${a.name}` }).click();
  await expect(b.page.getByText('Aucune demande en attente')).toBeVisible();
  await tab(a.page, 'En attente').click();
  await expect(a.page.getByText('Aucune demande en attente')).toBeVisible();

  // a redemande puis annule : b voit la demande arriver puis partir.
  await asA.post(`${API}/users/@me/relationships`, { data: { username: b.name } });
  await expect(b.page.getByText('Demande reçue')).toBeVisible();
  await a.page.getByRole('button', { name: `Annuler la demande à ${b.name}` }).click();
  await expect(b.page.getByText('Aucune demande en attente')).toBeVisible();

  // Amis, puis b retire a après confirmation (Échap annule d'abord).
  await befriend(api, a, b);
  await tab(b.page, 'Tous').click();
  await b.page.getByRole('button', { name: `Plus d'actions pour ${a.name}` }).click();
  await b.page.getByRole('button', { name: 'Retirer de tes amis' }).click();
  await expect(b.page.getByRole('alertdialog')).toBeVisible();
  await b.page.keyboard.press('Escape');
  await expect(b.page.getByRole('alertdialog')).toHaveCount(0);
  await b.page.getByRole('button', { name: `Plus d'actions pour ${a.name}` }).click();
  await b.page.getByRole('button', { name: 'Retirer de tes amis' }).click();
  await b.page.getByRole('alertdialog').getByRole('button', { name: 'Retirer' }).click();
  await expect(b.page.getByText('Pas encore d\'amis')).toBeVisible();
  await tab(a.page, 'Tous').click();
  await expect(a.page.getByText('Pas encore d\'amis')).toBeVisible();
});

test('F11-H1 bloquer un ami : il disparaît chez lui, ne peut ni écrire ni redemander ; débloquer', async ({ user, api, allow }) => {
  allow.push(/status of 403/);
  const a = await user('bloq');
  const b = await user('gene');
  await befriend(api, a, b);
  // Un MP existait : b y écrit avant le blocage.
  await tab(b.page, 'Tous').click();
  await b.page.getByRole('button', { name: `Écrire à ${a.name}` }).click();
  await expect(b.page.getByRole('heading', { name: a.name, level: 1 })).toBeVisible();
  const box = b.page.getByLabel(`Écrire à ${a.name}`);
  await box.fill('Avant le blocage');
  await box.press('Enter');
  await expect(messages(b.page).filter({ hasText: 'Avant le blocage' })).toHaveCount(1);
  const dm = b.page.url();

  await tab(a.page, 'Tous').click();
  await a.page.getByRole('button', { name: `Plus d'actions pour ${b.name}` }).click();
  await a.page.getByRole('button', { name: `Bloquer ${b.name}` }).click();
  const dialog = a.page.getByRole('alertdialog', { name: `Bloquer ${b.name} ?` });
  await a11y(a.page);
  await dialog.getByRole('button', { name: 'Bloquer' }).click();
  await expect(a.page.getByRole('heading', { name: 'Tous 0' })).toBeVisible();
  await tab(a.page, 'Bloqués').click();
  await expect(a.page.getByRole('button', { name: `Débloquer ${b.name}` })).toBeVisible();

  // Chez b : le message ne part plus, l'amitié a disparu, la demande est refusée.
  await box.fill('Après le blocage');
  await box.press('Enter');
  await expect(b.page.getByRole('alert').filter({ hasText: 'n\'a pas pu être remis' })).toBeVisible();
  await b.page.goto('/channels/@me');
  await tab(b.page, 'Tous').click();
  await expect(b.page.getByText('Pas encore d\'amis')).toBeVisible();
  await addByName(b.page, a.name);
  await expect(b.page.getByRole('alert')).toContainText('n\'accepte pas de demande');

  // Chez a, le MP dit pourquoi on ne peut plus écrire ; débloquer depuis le MP rouvre la saisie.
  await a.page.goto(new URL(dm).pathname);
  await expect(a.page.getByRole('status').filter({ hasText: `Tu as bloqué ${b.name}` })).toBeVisible();
  await expect(a.page.getByLabel(`Écrire à ${b.name}`)).toHaveCount(0);
  // Ses messages y sont repliés, comme partout.
  await expect(a.page.getByText('Message d\'un compte que tu as bloqué.')).toBeVisible();
  await a.page.getByRole('button', { name: 'Débloquer' }).click();
  await expect(messages(a.page).filter({ hasText: 'Avant le blocage' })).toHaveCount(1);
  await expect(a.page.getByLabel(`Écrire à ${b.name}`)).toBeVisible();
  // b peut de nouveau demander a en ami.
  await b.page.getByLabel('Pseudo de ton ami').fill(a.name);
  await b.page.getByRole('button', { name: 'Envoyer la demande' }).click();
  await expect(b.page.getByRole('status').filter({ hasText: 'Demande envoyée' })).toBeVisible();
});

test('F11-H2 liste des comptes bloqués dans les réglages, débloquer', async ({ user, api }) => {
  const a = await user('regl');
  const b = await user('cible');
  const asA = await api(a.token);
  expect((await asA.put(`${API}/users/@me/relationships/${b.id}`, { data: { kind: 'blocked' } })).status()).toBe(200);
  await a.page.goto('/settings/messaging');
  const dialog = a.page.getByRole('dialog', { name: 'Réglages' });
  await expect(dialog.getByRole('heading', { name: 'Comptes bloqués' })).toBeVisible();
  await a11y(a.page);
  await dialog.getByRole('button', { name: `Débloquer ${b.name}` }).click();
  await expect(dialog.getByText('Personne n\'est bloqué')).toBeVisible();
});

test('F11-E1 messages d’un compte bloqué repliés dans un salon partagé, Afficher les montre', async ({ demo }) => {
  const { page } = await demo('demo');
  await page.getByRole('link', { name: 'Serveur Abysse' }).click();
  await page.getByRole('link', { name: 'général' }).click();
  const zed = messages(page).filter({ hasText: 'codes promo' });
  await expect(page.getByText('Message d\'un compte que tu as bloqué.')).toBeVisible();
  await expect(zed).toBeHidden();
  const show = page.getByRole('button', { name: 'Afficher', exact: true });
  await expect(show).toHaveAttribute('aria-expanded', 'false');
  await show.click();
  await expect(zed).toBeVisible();
  await expect(page.getByRole('button', { name: 'Masquer' })).toHaveAttribute('aria-expanded', 'true');
  await a11y(page);
  await page.getByRole('button', { name: 'Masquer' }).click();
  await expect(zed).toBeHidden();
});

test('F03-H2 créer un groupe avec deux amis, le renommer, ajouter, quitter', async ({ user, api }) => {
  const a = await user('chef');
  const b = await user('mate');
  const c = await user('trio');
  const d = await user('quat');
  for (const f of [b, c, d]) await befriend(api, a, f);

  const nav = a.page.getByRole('navigation', { name: 'Messages privés' });
  await nav.getByRole('button', { name: 'Nouvelle conversation ou groupe' }).click();
  const picker = a.page.getByRole('dialog', { name: 'Nouvelle conversation' });
  await expect(picker.getByRole('button', { name: 'Ouvrir la conversation' })).toBeDisabled();
  await picker.getByLabel('Chercher parmi tes amis').fill(b.name);
  await picker.getByRole('checkbox', { name: new RegExp(b.name) }).check();
  await picker.getByLabel('Chercher parmi tes amis').fill('');
  await picker.getByRole('checkbox', { name: new RegExp(c.name) }).check();
  await expect(picker.getByRole('status')).toHaveText('2 sur 9 choisi(s)');
  await picker.getByLabel('Nom du groupe (facultatif)').fill('Équipage');
  await a11y(a.page);
  await picker.getByRole('button', { name: 'Créer le groupe' }).click();
  await expect(a.page.getByRole('heading', { name: 'Équipage', level: 1 })).toBeVisible();

  // b le voit arriver dans sa liste, et y écrit.
  const bNav = b.page.getByRole('navigation', { name: 'Messages privés' });
  await bNav.getByRole('link', { name: /Équipage/ }).click();
  await expect(b.page.getByRole('heading', { name: 'Équipage', level: 1 })).toBeVisible();
  await b.page.getByLabel('Écrire à Équipage').fill('Présent !');
  await b.page.keyboard.press('Enter');
  await expect(messages(a.page).filter({ hasText: 'Présent !' })).toHaveCount(1);

  // Renommer : tout le groupe voit le nouveau nom.
  await a.page.getByRole('button', { name: 'Renommer le groupe' }).click();
  const rename = a.page.getByRole('dialog', { name: 'Renommer le groupe' });
  await rename.getByLabel('Nom du groupe').fill('Équipage du soir');
  await rename.getByRole('button', { name: 'Enregistrer' }).click();
  await expect(a.page.getByRole('heading', { name: 'Équipage du soir', level: 1 })).toBeVisible();
  await expect(b.page.getByRole('heading', { name: 'Équipage du soir', level: 1 })).toBeVisible();
  await expect(b.page.getByLabel('Écrire à Équipage du soir')).toBeVisible();

  // Ajouter d : seuls les amis pas encore dans le groupe sont proposés.
  await a.page.getByRole('button', { name: 'Ajouter des amis au groupe' }).click();
  const add = a.page.getByRole('dialog', { name: 'Ajouter des amis au groupe' });
  await expect(add.getByRole('checkbox')).toHaveCount(1);
  await add.getByRole('checkbox', { name: new RegExp(d.name) }).check();
  await add.getByRole('button', { name: 'Ajouter au groupe' }).click();
  await expect(add).toHaveCount(0);
  await expect(d.page.getByRole('navigation', { name: 'Messages privés' }).getByRole('link', { name: /Équipage du soir/ })).toBeVisible();
  await expect(bNav.getByRole('link', { name: /Équipage du soir/ })).toContainText('4 membres');

  // b quitte : le groupe disparaît de sa liste, il revient à l'accueil.
  await b.page.getByRole('button', { name: 'Quitter le groupe' }).click();
  await b.page.getByRole('alertdialog').getByRole('button', { name: 'Quitter le groupe' }).click();
  await expect(b.page.getByRole('heading', { name: 'Amis', level: 1 })).toBeVisible();
  await expect(bNav.getByRole('link', { name: /Équipage/ })).toHaveCount(0);
  await expect(nav.getByRole('link', { name: /Équipage du soir/ })).toContainText('3 membres');
});

test('F03-H3 nouveau MP avec un ami, puis un groupe depuis ce MP ; 9 amis au plus', async ({ user, api }) => {
  const a = await user('solo');
  const b = await user('duo');
  const c = await user('tri');
  await befriend(api, a, b);
  await befriend(api, a, c);
  await a.page.getByRole('navigation', { name: 'Messages privés' }).getByRole('button', { name: 'Nouvelle conversation ou groupe' }).click();
  const picker = a.page.getByRole('dialog', { name: 'Nouvelle conversation' });
  await picker.getByRole('checkbox', { name: new RegExp(b.name) }).check();
  await expect(picker.getByLabel('Nom du groupe (facultatif)')).toHaveCount(0);
  await picker.getByRole('button', { name: 'Ouvrir la conversation' }).click();
  await expect(a.page.getByRole('heading', { name: b.name, level: 1 })).toBeVisible();
  await a.page.getByLabel(`Écrire à ${b.name}`).fill('Un MP tout neuf');
  await a.page.keyboard.press('Enter');
  await b.page.getByRole('navigation', { name: 'Messages privés' }).getByRole('link', { name: a.name }).click();
  await expect(messages(b.page).filter({ hasText: 'Un MP tout neuf' })).toHaveCount(1);

  // Depuis le MP : b est déjà coché, on ajoute c.
  await a.page.getByRole('button', { name: `Créer un groupe avec ${b.name}` }).click();
  const again = a.page.getByRole('dialog', { name: 'Nouvelle conversation' });
  await expect(again.getByRole('checkbox', { name: new RegExp(b.name) })).toBeChecked();
  await again.getByRole('checkbox', { name: new RegExp(c.name) }).check();
  await again.getByRole('button', { name: 'Créer le groupe' }).click();
  await expect(a.page.getByRole('heading', { name: `${b.name}, ${c.name}`, level: 1 })).toBeVisible();
});

test('F03-N2 l’API refuse un groupe de plus de 10 personnes ou avec un inconnu', async ({ user, api }) => {
  const a = await user('trop');
  const stranger = await user('loin');
  const asA = await api(a.token);
  const ids = Array.from({ length: 10 }, () => crypto.randomUUID());
  const big = await asA.post(`${API}/users/@me/channels`, { data: { recipients: ids } });
  expect(big.status()).toBe(400);
  const dm = await asA.post(`${API}/users/@me/channels`, { data: { recipients: [stranger.id] } });
  expect([dm.status(), (await dm.json()).code]).toEqual([403, 'dm_stranger']);
});
