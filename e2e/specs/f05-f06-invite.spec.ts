// F05 Créer un serveur et inviter ; F06 Rejoindre via invitation.
import { test, expect, a11y, createGuild, say, messages, API, unique } from '../fixtures';

test('F05-H1 + F06-H1 créer, inviter, rejoindre, discuter en direct', async ({ user }) => {
  const a = await user('alba');
  const b = await user('bruno');
  await a.page.goto('/guilds/new');
  await a11y(a.page);
  await a.page.goto('/channels/@me');
  await createGuild(a.page, 'Fosse des Mariannes');
  await a.page.getByRole('button', { name: 'Inviter des amis' }).click();
  await a11y(a.page);
  const link = await a.page.getByLabel('Lien à partager').inputValue();
  expect(link).toMatch(/\/invite\/[A-Za-z0-9]{8}$/);
  await a.page.getByRole('button', { name: 'Fermer' }).click();

  await b.page.goto(link);
  await expect(b.page.getByRole('heading', { name: 'Fosse des Mariannes' })).toBeVisible();
  await expect(b.page.getByText('1 membre')).toBeVisible();
  await a11y(b.page);
  await b.page.getByRole('button', { name: 'Rejoindre le serveur' }).click();
  await expect(b.page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();

  await say(a.page, 'général', 'Bienvenue au fond !');
  await expect(b.page.getByText('Bienvenue au fond !')).toBeVisible();
  await b.page.getByLabel('Écrire dans #général').fill('Merci, il fait sombre ici.');
  await b.page.getByRole('button', { name: 'Envoyer' }).click();
  await expect(a.page.getByText('Merci, il fait sombre ici.')).toBeVisible();
  await expect(a.page.getByRole('complementary', { name: 'Membres' }).getByText(b.name)).toBeVisible();
});

test('F05-N1 nom de serveur vide ou trop court refusé', async ({ user, api }) => {
  const { page, token } = await user('cyan');
  await page.goto('/guilds/new');
  const name = page.getByLabel('Nom du serveur');
  await name.fill('');
  await page.getByRole('button', { name: 'Créer', exact: true }).click();
  await expect(page).toHaveURL(/\/guilds\/new$/);
  expect(await name.evaluate((el: HTMLInputElement) => el.validity.valueMissing)).toBe(true);
  // Le serveur refuse aussi, même sans navigateur.
  const http = await api(token);
  for (const bad of ['', 'x', '     ']) {
    expect((await http.post(`${API}/guilds`, { data: { name: bad } })).status(), JSON.stringify(bad)).toBe(400);
  }
});

test('F05-E1 nom de 60 caractères : coupé proprement, page sans débordement', async ({ user }) => {
  const { page } = await user('dora');
  const long = 'Le très long nom du serveur des explorateurs des abysses ok';
  await createGuild(page, long);
  await expect(page.getByRole('navigation', { name: 'Salons' }).getByText(long)).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)).toBeLessThanOrEqual(0);
});

test('F06-N1 lien inconnu ou mal formé : invitation invalide', async ({ page, allow }) => {
  allow.push(/status of 404/);
  for (const code of ['zzzzzzzz', '..%2F..%2Fadmin']) {
    await page.goto(`/invite/${code}`);
    await expect(page.getByRole('heading', { name: 'Invitation invalide' })).toBeVisible();
  }
  await a11y(page);
});

test('F06-N2 invitation à usage unique : le second invité est refusé', async ({ user, api, allow }) => {
  allow.push(/status of 404/);
  const owner = await user('emil');
  const [, cid] = await createGuild(owner.page, 'Une place');
  const inv = await (await (await api(owner.token)).post(`${API}/channels/${cid}/invites`, { data: { max_uses: 1 } })).json();
  const first = await user('fern');
  await first.page.goto(`/invite/${inv.code}`);
  await first.page.getByRole('button', { name: 'Rejoindre le serveur' }).click();
  await expect(first.page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
  const second = await user('gael');
  await second.page.goto(`/invite/${inv.code}`);
  await expect(second.page.getByRole('heading', { name: 'Invitation invalide' })).toBeVisible();
});

test('F06-E1 déjà membre : le lien ouvre le serveur sans doublon', async ({ user, api }) => {
  const owner = await user('hana');
  const [gid, cid] = await createGuild(owner.page, 'Déjà là');
  const inv = await (await (await api(owner.token)).post(`${API}/channels/${cid}/invites`, { data: {} })).json();
  const b = await user('ivo');
  for (let i = 0; i < 2; i++) {
    await b.page.goto(`/invite/${inv.code}`);
    await b.page.getByRole('button', { name: 'Rejoindre le serveur' }).click();
    await expect(b.page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
  }
  const state = await (await (await api(owner.token)).get(`${API}/guilds/${gid}`)).json();
  expect(state.members).toHaveLength(2);
  await expect(b.page.getByRole('link', { name: 'Serveur Déjà là' })).toHaveCount(1);
});

test('F06-E2 visiteur sans compte : aperçu, inscription, retour à l’invitation', async ({ page, user, api }) => {
  const owner = await user('jade');
  const [, cid] = await createGuild(owner.page, 'Portes ouvertes');
  const inv = await (await (await api(owner.token)).post(`${API}/channels/${cid}/invites`, { data: {} })).json();
  await page.goto(`/invite/${inv.code}`);
  await expect(page.getByRole('heading', { name: 'Portes ouvertes' })).toBeVisible();
  await page.getByRole('link', { name: 'Créer un compte' }).click();
  const name = unique('kai');
  await page.getByLabel('E-mail').fill(`${name}@example.org`);
  await page.getByLabel('Pseudo').fill(name);
  await page.getByLabel('Mot de passe').fill('motdepasse-solide');
  await page.getByRole('button', { name: 'Créer mon compte' }).click();
  await page.waitForURL(`**/invite/${inv.code}`);
  await page.getByRole('button', { name: 'Rejoindre le serveur' }).click();
  await expect(page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
});

test('F06-E3 l’invitation ouvre le salon qu’elle vise, pas le premier de la liste', async ({ user, api }) => {
  const owner = await user('lena');
  const [gid, general] = await createGuild(owner.page, 'Deux salons');
  const http = await api(owner.token);
  // Un salon hors catégorie passe en tête de liste.
  const created = await http.post(`${API}/guilds/${gid}/channels`, { data: { name: 'arrivées', kind: 'text' } });
  expect(created.status()).toBe(201);
  const inv = await (await http.post(`${API}/channels/${general}/invites`, { data: {} })).json();
  const b = await user('milo');
  await b.page.goto(`/invite/${inv.code}`);
  await b.page.getByRole('button', { name: 'Rejoindre le serveur' }).click();
  await expect(b.page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
  await expect(messages(b.page)).toHaveCount(0);
});
