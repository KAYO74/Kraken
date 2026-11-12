// Profil (S20 : bio, pronoms, bannière, animations, carte de profil), compte (S06 : pseudo, e-mail,
// mot de passe) et présence (S04 : statut choisi, inactif, ne pas déranger, invisible).
import { test, expect, a11y, createGuild, say, messages, watch, PASSWORD, API, unique, type User } from '../fixtures';
import { png, flatPng, animatedGif } from '../images';
import type { APIRequestContext, BrowserContext, Locator, Page } from '@playwright/test';

// Un appel qui sonne a besoin du micro factice de Chromium (voir calls.spec.ts).
test.use({ launchOptions: { args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'] } });

const REJECTED = /status of 4\d\d/;

type Api = (t?: string) => Promise<APIRequestContext>;

const me = async (api: Api, u: User) => (await (await api(u.token)).get(`${API}/users/@me`)).json();

async function befriend(api: Api, a: User, b: User) {
  expect((await (await api(a.token)).post(`${API}/users/@me/relationships`, { data: { username: b.name } })).status()).toBe(201);
  expect((await (await api(b.token)).put(`${API}/users/@me/relationships/${a.id}`, { data: { kind: 'friend' } })).status()).toBe(200);
}

/** Invite `who` dans le serveur par l'API ; il le voit apparaître dans son rail. */
async function invite(api: Api, owner: User, cid: string, who: User) {
  const { code } = await (await (await api(owner.token)).post(`${API}/channels/${cid}/invites`, { data: {} })).json();
  expect((await (await api(who.token)).post(`${API}/invites/${code}`)).status()).toBe(200);
}

/** Ouvre le serveur dans la page de `who` ; la liste des membres est là aux grandes largeurs. */
async function openGuild(who: User, name: string) {
  await who.page.getByRole('link', { name: `Serveur ${name}` }).click();
  await expect(who.page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
}

const members = (p: Page) => p.getByRole('complementary', { name: 'Membres' });
const byHandle = (p: Page, u: User) => members(p).getByRole('button', { name: new RegExp(`@${u.name}`) });

/** Choisit un statut dans la carte « Mon compte », puis la referme. */
async function choose(page: Page, label: RegExp) {
  await page.getByRole('button', { name: /^Mon compte/ }).click();
  const card = page.getByRole('group', { name: 'Mon compte' });
  await card.locator('label', { hasText: label }).click();
  await expect(card.getByRole('radio', { name: label })).toBeChecked();
  await page.keyboard.press('Escape');
  await expect(card).toHaveCount(0);
}

/** La ligne d'un ami dans l'onglet « Tous ». */
async function friendRow(page: Page, who: User) {
  await page.goto('/channels/@me');
  await page.getByRole('tab', { name: 'Tous' }).click();
  return page.getByRole('listitem').filter({ hasText: `@${who.name}` });
}

/** Seul le champ mis en cause porte aria-invalid, et l'alerte lui est reliée (aria-describedby). */
async function blames(form: Locator, blamed: string, fields: string[]) {
  const alert = form.getByRole('alert');
  await expect(alert).toBeVisible();
  const message = (await alert.textContent()) ?? '';
  for (const label of fields) {
    const field = form.getByLabel(label, { exact: true });
    if (label === blamed) {
      await expect(field).toHaveAttribute('aria-invalid', 'true');
      await expect(field).toHaveAccessibleDescription(new RegExp(message.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')));
    } else {
      await expect(field).not.toHaveAttribute('aria-invalid', 'true');
      await expect(field).not.toHaveAccessibleDescription(new RegExp(message.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')));
    }
  }
}

/** La barre d'actions de la carte ne colle ni aux bords ni au contenu : 16 px de marge au moins. */
async function actionsBreathe(card: Locator) {
  const pad = await card.locator('.app-card-actions').evaluate((el) => {
    const c = getComputedStyle(el);
    return [c.paddingTop, c.paddingRight, c.paddingBottom, c.paddingLeft].map(parseFloat);
  });
  expect(pad.every((v) => v >= 16), `marges de la barre d'actions : ${pad.join(' ')}`).toBe(true);
}

// ───────── S20 : profil ─────────

test('S20-H1 bio et pronoms : compteur, aperçu en direct, enregistrés, bornés', async ({ user, api }) => {
  const a = await user('ondine');
  const { page } = a;
  await page.goto('/settings/profiles');
  await a11y(page);
  const bio = page.getByLabel('À propos de toi');
  const pronouns = page.getByLabel('Pronoms');
  const preview = page.getByRole('region', { name: 'Aperçu de ta carte' });
  await expect(preview.getByText(`@${a.name}`)).toBeVisible();
  await expect(page.getByText('0 / 190')).toBeVisible();

  await pronouns.fill('elle/elles');
  await bio.fill('Plongeuse de nuit.\nSans lampe.');
  await expect(page.getByText('30 / 190')).toBeVisible();
  // L'aperçu montre ce qui est tapé, avant même d'enregistrer.
  await expect(preview).toContainText('elle/elles');
  await expect(preview).toContainText('Plongeuse de nuit.');
  expect((await me(api, a)).bio ?? null).toBeNull();

  await page.getByRole('button', { name: 'Enregistrer' }).click();
  await expect(page.getByRole('status')).toHaveText('Profil enregistré.');
  const saved = await me(api, a);
  expect([saved.bio, saved.pronouns]).toEqual(['Plongeuse de nuit.\nSans lampe.', 'elle/elles']);
  await page.reload();
  await expect(page.getByLabel('À propos de toi')).toHaveValue('Plongeuse de nuit.\nSans lampe.');
  await expect(page.getByLabel('Pronoms')).toHaveValue('elle/elles');
  // L'aperçu dit le vrai statut dès le rechargement (la fiche se remplit après le READY), pas « Hors ligne ».
  const status = page.getByRole('region', { name: 'Aperçu de ta carte' }).locator('.app-card-status');
  await expect(status).toHaveText('En ligne');
  expect((await (await api(a.token)).put(`${API}/users/@me/presence`, { data: { presence: 'dnd' } })).status()).toBe(200);
  await page.reload();
  await expect(page.getByLabel('Pronoms')).toHaveValue('elle/elles');
  await expect(status).toHaveText('Ne pas déranger');

  // Les bornes : 190 caractères de bio, 40 de pronoms, coupés dans le champ.
  await page.getByLabel('À propos de toi').fill('x'.repeat(250));
  await expect(page.getByLabel('À propos de toi')).toHaveValue('x'.repeat(190));
  await expect(page.getByText('190 / 190')).toBeVisible();
  await page.getByLabel('Pronoms').fill('p'.repeat(60));
  await expect(page.getByLabel('Pronoms')).toHaveValue('p'.repeat(40));

  // Vider les champs les efface.
  await page.getByLabel('À propos de toi').fill('');
  await page.getByLabel('Pronoms').fill('');
  await page.getByRole('button', { name: 'Enregistrer' }).click();
  await expect(page.getByRole('status')).toHaveText('Profil enregistré.');
  const cleared = await me(api, a);
  expect([cleared.bio ?? null, cleared.pronouns ?? null]).toEqual([null, null]);
});

test('S20-H2 bannière : recadrage large, aperçu, retrait ; une animation reste animée', async ({ user, api }) => {
  const a = await user('maree');
  const { page } = a;
  const asA = await api(a.token);
  await page.goto('/settings/profiles');
  const preview = page.getByRole('region', { name: 'Aperçu de ta carte' });
  await expect(page.getByRole('button', { name: 'Retirer la bannière' })).toHaveCount(0);

  await page.getByLabel('Changer la bannière').setInputFiles({ name: 'banniere.png', mimeType: 'image/png', buffer: png(900, 240) });
  const crop = page.getByRole('dialog', { name: 'Recadrer ta bannière' });
  await expect(crop).toBeVisible();
  await expect(crop.getByRole('button', { name: 'Appliquer' })).toBeEnabled();
  await crop.getByRole('slider', { name: 'Zoom' }).fill('1.5');
  await crop.getByRole('application').press('ArrowRight');
  await a11y(page);
  await crop.getByRole('button', { name: 'Appliquer' }).click();
  await expect(crop).toHaveCount(0);
  await expect(page.getByRole('status').filter({ hasText: 'Bannière enregistrée.' })).toBeVisible();

  const key = (await me(api, a)).banner as string;
  expect(key).toMatch(/^[0-9a-f]{32}$/);
  const served = await asA.get(`${API}/banners/${key}`);
  expect([served.status(), served.headers()['content-type']]).toEqual([200, 'image/png']);
  const current = page.getByRole('img', { name: 'Ta bannière actuelle' });
  await expect(current).toBeVisible();
  await expect.poll(() => current.evaluate((i: HTMLImageElement) => [i.naturalWidth, i.naturalHeight])).toEqual([960, 320]);
  await expect(preview.locator('.app-card-banner img')).toBeVisible();
  await a11y(page);

  // Un GIF animé part tel quel, sans fenêtre de recadrage, et le serveur le garde en GIF.
  await page.getByLabel('Changer la bannière').setInputFiles({ name: 'vague.gif', mimeType: 'image/gif', buffer: animatedGif(2) });
  await expect(page.getByRole('dialog', { name: 'Recadrer ta bannière' })).toHaveCount(0);
  await expect.poll(async () => (await me(api, a)).banner).not.toBe(key);
  const gif = await asA.get(`${API}/banners/${(await me(api, a)).banner}`);
  expect([gif.status(), gif.headers()['content-type']]).toEqual([200, 'image/gif']);
  expect((await asA.get(`${API}/banners/${key}`)).status()).toBe(404);

  // La photo de profil animée aussi.
  await page.getByLabel('Changer la photo').setInputFiles({ name: 'bulles.gif', mimeType: 'image/gif', buffer: animatedGif(3) });
  await expect(page.getByRole('status').filter({ hasText: 'Photo enregistrée.' })).toBeVisible();
  const avatar = await asA.get(`${API}/avatars/${(await me(api, a)).avatar}`);
  expect([avatar.status(), avatar.headers()['content-type']]).toEqual([200, 'image/gif']);

  // Retirer la bannière : le fichier n'est plus servi.
  const second = (await me(api, a)).banner as string;
  await page.getByRole('button', { name: 'Retirer la bannière' }).click();
  await expect(page.getByRole('status').filter({ hasText: 'Bannière retirée.' })).toBeVisible();
  await expect(current).toHaveCount(0);
  expect((await asA.get(`${API}/banners/${second}`)).status()).toBe(404);
  expect((await me(api, a)).banner ?? null).toBeNull();
});

test('S20-E1 bannière : pas une image, trop lourde ; rien d’enregistré', async ({ user, api }) => {
  const a = await user('brume');
  await a.page.goto('/settings/profiles');
  const input = a.page.getByLabel('Changer la bannière');
  await input.setInputFiles({ name: 'notes.txt', mimeType: 'text/plain', buffer: Buffer.from('ceci n’est pas une image') });
  await expect(a.page.getByRole('alert').filter({ hasText: 'pas une image lisible' })).toBeVisible();
  await expect(a.page.getByRole('dialog', { name: 'Recadrer ta bannière' })).toHaveCount(0);
  await input.setInputFiles({ name: 'enorme.png', mimeType: 'image/png', buffer: Buffer.alloc(9 * 1024 * 1024) });
  await expect(a.page.getByRole('alert').filter({ hasText: '8 Mo au plus' })).toBeVisible();
  expect((await me(api, a)).banner ?? null).toBeNull();
});

test('S20-E2 texte du profil : caractères invisibles refusés, emojis composés acceptés', async ({ user, api, allow }) => {
  allow.push(REJECTED);
  const a = await user('opale');
  const { page } = a;
  await page.goto('/settings/profiles');
  const alert = page.getByRole('form', { name: 'Ce que les autres voient' }).getByRole('alert');
  // Espace de largeur nulle dans le nom, marque de sens d'écriture dans la bio : refusés, rien d'enregistré.
  await page.getByLabel('Nom affiché').fill('Opa\u200Ble');
  await page.getByRole('button', { name: 'Enregistrer' }).click();
  await expect(alert).toContainText('Nom affiché : les caractères invisibles de mise en forme');
  await page.getByLabel('Nom affiché').fill('Opale');
  await page.getByLabel('À propos de toi').fill('texte \u202Eà l’envers');
  await page.getByRole('button', { name: 'Enregistrer' }).click();
  await expect(alert).toContainText('Bio : les caractères invisibles de mise en forme');
  const refused = await me(api, a);
  expect([refused.display_name ?? null, refused.bio ?? null]).toEqual([null, null]);

  // Le liant d'emoji et le sélecteur de variante ne sont pas touchés : famille, drapeau, cœur.
  const bio = '👩\u200D👩\u200D👧 🏳\uFE0F\u200D🌈 ❤\uFE0F';
  await page.getByLabel('À propos de toi').fill(bio);
  await page.getByLabel('Pronoms').fill('❤\uFE0F');
  await page.getByRole('button', { name: 'Enregistrer' }).click();
  await expect(page.getByRole('status')).toHaveText('Profil enregistré.');
  const saved = await me(api, a);
  expect([saved.bio, saved.pronouns]).toEqual([bio, '❤\uFE0F']);
});

test('S20-E3 photo et bannière : une image trop grande est refusée d’après son en-tête', async ({ user, api, allow }) => {
  allow.push(REJECTED);
  const a = await user('cime');
  const asA = await api(a.token);
  const send = (which: string, data: Buffer) => asA.put(`${API}/users/@me/${which}`, { data, headers: { 'content-type': 'image/png' } });
  for (const which of ['avatar', 'banner']) {
    // Plus de 4096 pixels de côté : refusé, même s'il ne pèse que quelques Ko.
    const wide = await send(which, flatPng(4100, 16));
    expect([wide.status(), (await wide.json()).code]).toEqual([400, 'image_too_big']);
    expect((await wide.json()).message).toContain('4096 pixels de côté');
    // Plus de 16 millions de pixels au total, chaque côté dans la limite.
    const heavy = await send(which, flatPng(4096, 4096));
    expect([heavy.status(), (await heavy.json()).code]).toEqual([400, 'image_too_big']);
  }
  const user0 = await me(api, a);
  expect([user0.avatar ?? null, user0.banner ?? null]).toEqual([null, null]);
  // 4096 de côté passe.
  expect((await send('banner', flatPng(4096, 16))).status()).toBe(200);
});

test('S20-H3 carte de profil : depuis la liste des membres et depuis un message, avec ses actions', async ({ user, api }) => {
  const a = await user('perle');
  const b = await user('quartz');
  const [gid, cid] = await createGuild(a.page, 'Récif');
  await invite(api, a, cid, b);
  const asA = await api(a.token);
  expect((await asA.patch(`${API}/users/@me`, { data: { display_name: 'Perle des abysses', bio: 'Collectionne les épaves.', pronouns: 'elle/elles' } })).status()).toBe(200);
  expect((await asA.put(`${API}/users/@me/banner`, { data: png(900, 300), headers: { 'content-type': 'image/png' } })).status()).toBe(200);
  const role = await (await asA.post(`${API}/guilds/${gid}/roles`, { data: { name: 'Plongeurs', color: 3, hoist: false, permissions: 0 } })).json();
  expect((await asA.put(`${API}/guilds/${gid}/members/${a.id}/roles/${role.id}`)).status()).toBe(200);

  await openGuild(b, 'Récif');
  const opener = byHandle(b.page, a);
  await expect(opener).toBeVisible();
  await opener.click();
  const card = b.page.getByRole('dialog', { name: 'Profil de Perle des abysses' });
  await expect(card).toBeVisible();
  await expect(card.getByText('Perle des abysses', { exact: true })).toBeVisible();
  await expect(card).toContainText(`@${a.name}`);
  await expect(card).toContainText('elle/elles');
  await expect(card).toContainText('Collectionne les épaves.');
  await expect(card).toContainText('Membre depuis');
  await expect(card).toContainText('Sur ce serveur depuis');
  await expect(card.getByRole('region', { name: 'Rôles' }).getByRole('listitem')).toHaveText(['Plongeurs']);
  await expect(card.getByText('En ligne', { exact: true })).toBeVisible();
  await expect(card.getByRole('img', { name: 'Perle des abysses, en ligne' })).toBeVisible();
  await expect.poll(() => card.locator('.app-card-banner img').evaluate((i: HTMLImageElement) => i.naturalWidth)).toBe(960);
  await expect(card.getByRole('button', { name: 'Envoyer un message' })).toBeVisible();
  await expect(card.getByRole('button', { name: 'Ajouter en ami' })).toBeVisible();
  await expect(card.getByRole('button', { name: 'Bloquer', exact: true })).toBeVisible();
  await actionsBreathe(card);
  await a11y(b.page);
  // Échap referme la carte et rend le focus à ce qui l'a ouverte.
  await b.page.keyboard.press('Escape');
  await expect(card).toHaveCount(0);
  await expect(opener).toBeFocused();

  // Depuis un message : le nom de l'auteur ouvre la même carte.
  await say(a.page, 'général', 'Coucou le récif');
  const msg = messages(b.page).filter({ hasText: 'Coucou le récif' });
  await msg.getByRole('button', { name: 'Perle des abysses' }).click();
  await expect(card).toBeVisible();
  await b.page.getByRole('button', { name: 'Fermer le profil (Échap)' }).click();
  await expect(card).toHaveCount(0);

  // Ajouter en ami : la demande part, la carte le dit et ne la propose plus.
  await opener.click();
  await card.getByRole('button', { name: 'Ajouter en ami' }).click();
  await expect(card.getByRole('status')).toHaveText("Demande d'ami envoyée.");
  await expect(card.getByRole('button', { name: 'Demande envoyée' })).toBeDisabled();
  await expect(card.getByRole('button', { name: 'Ajouter en ami' })).toHaveCount(0);
  const asB = await api(b.token);
  const rels = await (await asB.get(`${API}/users/@me/relationships`)).json();
  expect(rels.map((r: { kind: string }) => r.kind)).toEqual(['pending_outgoing']);

  // Envoyer un message : ouvre le MP et ferme la carte.
  await card.getByRole('button', { name: 'Envoyer un message' }).click();
  await b.page.waitForURL(/\/channels\/@me\/[0-9a-f-]{36}$/);
  await expect(card).toHaveCount(0);

  // Bloquer : confirmation, puis la carte propose de débloquer et n'offre plus d'écrire.
  await openGuild(b, 'Récif');
  await opener.click();
  await card.getByRole('button', { name: 'Bloquer', exact: true }).click();
  const confirm = b.page.getByRole('alertdialog', { name: 'Bloquer Perle des abysses ?' });
  await expect(confirm).toBeVisible();
  await confirm.getByRole('button', { name: 'Bloquer' }).click();
  await expect(confirm).toHaveCount(0);
  await expect(card.getByRole('button', { name: 'Débloquer' })).toBeVisible();
  await expect(card.getByRole('button', { name: 'Envoyer un message' })).toHaveCount(0);
  await expect(card.getByRole('button', { name: 'Bloquer', exact: true })).toHaveCount(0);
  await card.getByRole('button', { name: 'Débloquer' }).click();
  await expect(card.getByRole('button', { name: 'Ajouter en ami' })).toBeVisible();
  await expect(card.getByRole('button', { name: 'Envoyer un message' })).toBeVisible();
});

test('S20-H4 sa propre carte : un lien vers Mon profil', async ({ user }) => {
  const a = await user('ilot');
  await createGuild(a.page, 'Atoll');
  await byHandle(a.page, a).click();
  const card = a.page.getByRole('dialog', { name: `Profil de ${a.name}` });
  await expect(card).toBeVisible();
  await expect(card.getByRole('button', { name: 'Envoyer un message' })).toHaveCount(0);
  await card.getByRole('link', { name: 'Modifier mon profil' }).click();
  await expect(a.page).toHaveURL(/\/settings\/profiles$/);
  await expect(card).toHaveCount(0);
  await expect(a.page.getByRole('dialog', { name: 'Réglages' })).toBeVisible();
});

// ───────── S06 : compte ─────────

test('S06-H1 changer de pseudo : mot de passe demandé, en direct chez les amis, limité à deux par heure', async ({ user, api, allow }) => {
  allow.push(REJECTED);
  const a = await user('carina');
  const b = await user('dorian');
  await befriend(api, a, b);
  const row = await friendRow(b.page, a);
  await expect(row).toBeVisible();

  await a.page.goto('/settings/account');
  await a11y(a.page);
  const form = a.page.getByRole('form', { name: 'Pseudo' });
  const change = async (name: string, password: string) => {
    await form.getByLabel('Nouveau pseudo').fill(name);
    await form.getByLabel('Mot de passe actuel').fill(password);
    await form.getByRole('button', { name: 'Changer le pseudo' }).click();
  };
  const pseudoFields = ['Nouveau pseudo', 'Mot de passe actuel'];
  await change(unique('nouv'), 'pas-le-bon-mot');
  await expect(form.getByRole('alert')).toHaveText('Mot de passe incorrect.');
  await blames(form, 'Mot de passe actuel', pseudoFields);
  await a11y(a.page); // l'erreur affichée, aria-describedby et aria-invalid compris
  await change(b.name, PASSWORD);
  await expect(form.getByRole('alert')).toHaveText('Ce pseudo est déjà pris.');
  await blames(form, 'Nouveau pseudo', pseudoFields);
  // Un nom réservé (l'équipe, les mentions) est refusé sans compter dans la limite horaire.
  await change('support', PASSWORD);
  await expect(form.getByRole('alert')).toHaveText('Ce pseudo est réservé : choisis-en un autre.');
  await blames(form, 'Nouveau pseudo', pseudoFields);
  expect((await me(api, a)).username).toBe(a.name);

  const first = unique('nouv');
  await change(first, PASSWORD);
  await expect(form.getByRole('status')).toHaveText(`Ton pseudo est maintenant @${first}.`);
  await expect(form.getByRole('alert')).toHaveCount(0);
  expect((await me(api, a)).username).toBe(first);
  // L'ami le voit sans recharger ; l'ancien pseudo ne connecte plus, le nouveau si.
  await expect(b.page.getByRole('listitem').filter({ hasText: `@${first}` })).toBeVisible();
  expect((await (await api()).post(`${API}/auth/login`, { data: { login: a.name, password: PASSWORD } })).status()).toBe(401);
  expect((await (await api()).post(`${API}/auth/login`, { data: { login: first, password: PASSWORD } })).status()).toBe(200);

  await change(unique('nouv'), PASSWORD);
  await expect(form.getByRole('status')).toContainText('Ton pseudo est maintenant');
  await change(unique('nouv'), PASSWORD);
  await expect(form.getByRole('alert')).toContainText('Tu as déjà changé ton pseudo 2 fois en une heure. Réessaie dans');
  await blames(form, 'Nouveau pseudo', pseudoFields);
});

test('S06-H2 changer d’adresse e-mail : mot de passe demandé, adresse déjà prise refusée', async ({ user, api, allow }) => {
  allow.push(REJECTED);
  const a = await user('elea');
  const b = await user('fabio');
  await a.page.goto('/settings/account');
  const form = a.page.getByRole('form', { name: 'Adresse e-mail' });
  await expect(form.getByText(`Ton adresse actuelle est ${a.email} (pas encore confirmée).`)).toBeVisible();
  const change = async (email: string, password: string) => {
    await form.getByLabel('Nouvelle adresse e-mail').fill(email);
    await form.getByLabel('Mot de passe actuel').fill(password);
    await form.getByRole('button', { name: "Changer l'adresse" }).click();
  };
  const fresh = `${unique('nouvelle')}@example.org`;
  const mailFields = ['Nouvelle adresse e-mail', 'Mot de passe actuel'];
  await change(fresh, 'pas-le-bon-mot');
  await expect(form.getByRole('alert')).toHaveText('Mot de passe incorrect.');
  await blames(form, 'Mot de passe actuel', mailFields);
  await change(b.email, PASSWORD);
  await expect(form.getByRole('alert')).toHaveText('Un compte existe déjà avec cette adresse.');
  await blames(form, 'Nouvelle adresse e-mail', mailFields);
  await change(a.email.toUpperCase(), PASSWORD);
  await expect(form.getByRole('alert')).toHaveText("C'est déjà ton adresse e-mail.");
  await blames(form, 'Nouvelle adresse e-mail', mailFields);

  // Sans e-mail sur l'instance, la nouvelle adresse remplace l'ancienne tout de suite.
  await change(fresh, PASSWORD);
  await expect(form.getByRole('status')).toHaveText('Adresse enregistrée.');
  await expect(form.getByText(`Ton adresse actuelle est ${fresh}`)).toBeVisible();
  const account = await (await (await api(a.token)).get(`${API}/users/@me/account`)).json();
  expect(account.email).toBe(fresh);
  expect((await (await api()).post(`${API}/auth/login`, { data: { login: fresh, password: PASSWORD } })).status()).toBe(200);
});

test('S06-H3 changer de mot de passe : les autres appareils sont déconnectés, celui-ci reste ouvert', async ({ user, api, browser, baseURL, issues, allow }) => {
  allow.push(REJECTED);
  const a = await user('gaelle');
  // Un deuxième appareil, connecté et ouvert sur l'accueil.
  const other: BrowserContext = await browser.newContext({ baseURL });
  const login = await other.request.post(`${API}/auth/login`, { data: { login: a.name, password: PASSWORD } });
  expect(login.ok()).toBeTruthy();
  const otherToken = (await login.json()).token as string;
  const second = await other.newPage();
  watch(second, issues);
  await second.goto('/login');
  await second.evaluate(() => localStorage.setItem('kraken.signed_in', '1'));
  await second.goto('/channels/@me');
  await expect(second.getByRole('navigation', { name: 'Serveurs' })).toBeVisible();

  await a.page.goto('/settings/account');
  const form = a.page.getByRole('form', { name: 'Mot de passe' });
  const change = async (current: string, next: string) => {
    await form.getByLabel('Mot de passe actuel').fill(current);
    await form.getByLabel('Nouveau mot de passe').fill(next);
    await form.getByRole('button', { name: 'Changer le mot de passe' }).click();
  };
  const next = 'un-autre-mot-de-passe';
  const passFields = ['Mot de passe actuel', 'Nouveau mot de passe'];
  await change('pas-le-bon-mot', next);
  await expect(form.getByRole('alert')).toHaveText('Mot de passe incorrect.');
  await blames(form, 'Mot de passe actuel', passFields);
  await change(PASSWORD, PASSWORD);
  await expect(form.getByRole('alert')).toHaveText("Choisis un mot de passe différent de l'actuel.");
  await blames(form, 'Nouveau mot de passe', passFields);
  await change(PASSWORD, next);
  await expect(form.getByRole('status')).toHaveText('Mot de passe changé. Tes autres appareils ont été déconnectés.');

  // L'autre appareil est renvoyé à la connexion ; celui-ci garde sa session.
  await second.waitForURL('**/login**', { timeout: 10_000 });
  expect((await (await api(otherToken)).get(`${API}/users/@me`)).status()).toBe(401);
  expect((await (await api(a.token)).get(`${API}/users/@me`)).status()).toBe(200);
  await a.page.reload();
  await expect(a.page.getByRole('form', { name: 'Mot de passe' })).toBeVisible();
  expect((await (await api()).post(`${API}/auth/login`, { data: { login: a.name, password: PASSWORD } })).status()).toBe(401);
  expect((await (await api()).post(`${API}/auth/login`, { data: { login: a.name, password: next } })).status()).toBe(200);
  await other.close();
});

test('S06-E1 inscription : un pseudo réservé est refusé avec son message', async ({ page, allow }) => {
  allow.push(REJECTED);
  await page.goto('/register');
  await page.getByLabel('E-mail').fill(`${unique('res')}@example.org`);
  await page.getByLabel('Pseudo').fill('admin');
  await page.getByLabel('Mot de passe').fill(PASSWORD);
  await page.getByRole('button', { name: 'Créer mon compte' }).click();
  await expect(page.getByRole('alert')).toHaveText('Ce pseudo est réservé : choisis-en un autre.');
  await expect(page).toHaveURL(/\/register/);
});

// ───────── S04 : présence ─────────

/** Ce qui distingue une pastille d'une autre sans la couleur : la forme de ses deux pièces. */
const shapes = (page: Page) =>
  page.evaluate(() => {
    const out: Record<string, string> = {};
    for (const dot of document.querySelectorAll<HTMLElement>('i.k-presence')) {
      const kind = [...dot.classList].find((c) => c.startsWith('k-presence--'))!.slice(12);
      const part = (which: '::before' | '::after') => {
        const s = getComputedStyle(dot, which);
        return [s.content, s.borderTopStyle, s.width, s.height, s.top, s.left, s.borderTopLeftRadius].join(' ');
      };
      out[kind] = `${part('::before')} | ${part('::after')}`;
    }
    return out;
  });

test('S04-H1 choisir son statut : visible des amis avec forme et mot, invisible = hors ligne pour les autres', async ({ user, api }) => {
  const a = await user('aurore');
  const b = await user('boreal');
  const c = await user('calme');
  await befriend(api, a, b);
  await befriend(api, c, b);
  // Un MP et un serveur en commun : le statut se voit aussi dans la barre des MP et dans la liste des membres.
  const dm = await (await (await api(a.token)).post(`${API}/users/@me/channels`, { data: { recipients: [b.id] } })).json();
  expect(dm.id).toMatch(/^[0-9a-f-]{36}$/);
  const [, cid] = await createGuild(a.page, 'Aube');
  await invite(api, a, cid, b);

  // Chez b : a est en ligne, c (dont la page est fermée) hors ligne.
  await c.page.close();
  const online = await friendRow(b.page, a);
  await expect(online).toContainText(`@${a.name} · en ligne`);
  await expect(online.locator(`[aria-label="${a.name}, en ligne"]`)).toBeVisible();
  const offline = b.page.getByRole('listitem').filter({ hasText: `@${c.name}` });
  await expect(offline).toContainText(`@${c.name} · hors ligne`);

  // Chez a : le menu du statut, sous « Mon compte ».
  await a.page.getByRole('button', { name: /^Mon compte/ }).click();
  const menu = a.page.getByRole('group', { name: 'Mon compte' });
  const radios = menu.getByRole('group', { name: 'Mon statut' });
  await expect(radios.getByRole('radio')).toHaveCount(4);
  await expect(radios.getByRole('radio', { name: 'En ligne' })).toBeChecked();
  await a11y(a.page);
  // Les cinq pastilles ont chacune leur forme, et pas seulement leur couleur.
  const mine = await shapes(a.page);
  const theirs = await shapes(b.page);
  const all = { ...theirs, ...mine };
  expect(Object.keys(all).sort()).toEqual(['dnd', 'idle', 'invisible', 'offline', 'online']);
  expect(new Set(Object.values(all)).size).toBe(5);
  await a.page.keyboard.press('Escape');

  // Ne pas déranger : visible du MP, de la liste des membres et des amis, avec le mot.
  await choose(a.page, /^Ne pas déranger/);
  await expect(a.page.getByRole('button', { name: 'Mon compte, ne pas déranger' })).toBeVisible();
  await expect(b.page.getByRole('listitem').filter({ hasText: `@${a.name}` })).toContainText(`@${a.name} · ne pas déranger`);
  await expect(b.page.getByRole('listitem').filter({ hasText: `@${a.name}` }).locator(`[aria-label="${a.name}, ne pas déranger"]`)).toBeVisible();
  await b.page.getByRole('link', { name: new RegExp(`^${a.name}`) }).first().click();
  await expect(b.page.getByRole('navigation', { name: 'Messages privés' }).locator(`[aria-label="${a.name}, ne pas déranger"]`)).toBeVisible();
  await openGuild(b, 'Aube');
  const avatar = members(b.page).locator(`[aria-label="${a.name}, ne pas déranger"]`);
  await expect(avatar).toBeVisible();
  // Sur l'avatar de 32 px de la liste, la pastille sort de 3 px au moins : elle ne mord pas sur les initiales.
  const [face, dot] = [(await avatar.boundingBox())!, (await avatar.locator('i.k-presence').boundingBox())!];
  expect(face.width).toBe(32);
  expect(dot.x + dot.width - (face.x + face.width)).toBeGreaterThanOrEqual(3);
  expect(dot.y + dot.height - (face.y + face.height)).toBeGreaterThanOrEqual(3);
  expect((await me(api, a)).presence ?? null).toBeNull(); // le choix n'est pas dans la fiche publique

  // Invisible : a garde son choix et son mot, b le voit hors ligne ; le choix survit au rechargement.
  await choose(a.page, /^Invisible/);
  await expect(a.page.getByRole('button', { name: 'Mon compte, invisible' })).toBeVisible();
  await expect(members(b.page).locator(`[aria-label="${a.name}, hors ligne"]`)).toBeVisible();
  await expect(members(b.page).locator(`[aria-label="${a.name}, invisible"]`)).toHaveCount(0);
  await a.page.reload();
  await expect(a.page.getByRole('button', { name: 'Mon compte, invisible' })).toBeVisible();
  await expect(members(b.page).locator(`[aria-label="${a.name}, hors ligne"]`)).toBeVisible();
  // Sa propre carte de profil dit « Invisible » à lui seul.
  await byHandle(a.page, a).click();
  await expect(a.page.getByRole('dialog', { name: `Profil de ${a.name}` }).getByText('Invisible', { exact: true })).toBeVisible();
  await a.page.keyboard.press('Escape');

  // De retour en ligne.
  await choose(a.page, /^En ligne/);
  await expect(members(b.page).locator(`[aria-label="${a.name}, en ligne"]`)).toBeVisible();
});

test('S04-H2 inactif après 5 minutes sans geste, en ligne au geste suivant, jamais par-dessus un choix manuel', async ({ user, api, issues }) => {
  const a = await user('veilleur');
  const b = await user('guetteur');
  await befriend(api, a, b);
  const row = await friendRow(b.page, a);
  await expect(row).toContainText('en ligne');

  // L'appareil de a tourne sur une horloge qu'on avance à la main ; son autre page est fermée.
  const p = await a.context.newPage();
  watch(p, issues);
  await a.page.close();
  await p.clock.install({ time: Date.now() });
  await p.goto('/channels/@me');
  await expect(p.getByRole('button', { name: 'Mon compte, en ligne' })).toBeVisible();
  await expect(row).toContainText('en ligne');
  await p.clock.fastForward(4 * 60 * 1000);
  await p.waitForTimeout(500);
  await expect(row).toContainText('en ligne');
  await p.clock.fastForward(2 * 60 * 1000);
  await expect(row).toContainText(`@${a.name} · inactif`);
  await expect(p.getByRole('button', { name: 'Mon compte, inactif' })).toBeVisible();
  // Un geste, et il est de nouveau en ligne.
  await p.mouse.move(200, 200);
  await p.mouse.move(260, 240);
  await expect(row).toContainText(`@${a.name} · en ligne`);
  await expect(p.getByRole('button', { name: 'Mon compte, en ligne' })).toBeVisible();

  // Ne pas déranger n'est jamais remplacé par « inactif ».
  await choose(p, /^Ne pas déranger/);
  await expect(row).toContainText('ne pas déranger');
  await p.clock.fastForward(6 * 60 * 1000);
  await p.waitForTimeout(500);
  await expect(row).toContainText('ne pas déranger');
  await expect(p.getByRole('button', { name: 'Mon compte, ne pas déranger' })).toBeVisible();
});

/** Remplace l'API Notification par un enregistreur (permission accordée au contexte). */
async function recordNotifications(ctx: BrowserContext) {
  await ctx.grantPermissions(['notifications']);
  await ctx.addInitScript(() => {
    const w = window as unknown as { __notes: { title: string; body?: string }[] };
    w.__notes = [];
    const Real = window.Notification;
    class Recorder {
      static get permission() {
        return 'granted';
      }
      static requestPermission() {
        return Real.requestPermission().then(() => 'granted');
      }
      onclick: (() => void) | null = null;
      constructor(title: string, opts?: NotificationOptions) {
        w.__notes.push({ title, body: opts?.body });
      }
      close() {}
    }
    (window as unknown as { Notification: unknown }).Notification = Recorder;
  });
}

test('S04-H3 ne pas déranger coupe les notifications du bureau, pas les messages reçus', async ({ user, api }) => {
  const a = await user('expediteur');
  const b = await user('dormeur');
  await befriend(api, a, b);
  const asA = await api(a.token);
  const dm = (await (await asA.post(`${API}/users/@me/channels`, { data: { recipients: [b.id] } })).json()).id as string;
  const send = (content: string) => asA.post(`${API}/channels/${dm}/messages`, { data: { content } });
  const notes = () => b.page.evaluate(() => (window as unknown as { __notes: unknown[] }).__notes);
  await recordNotifications(b.context);
  await b.page.goto('/settings/notifications');
  await b.page.getByRole('switch', { name: 'Notifications sur le bureau' }).check();
  await b.page.goto('/channels/@me');

  await choose(b.page, /^Ne pas déranger/);
  await b.page.evaluate(() => window.dispatchEvent(new Event('blur')));
  expect((await send('Première pensée')).status()).toBe(201);
  // Le message est bien reçu (badge, titre de l'onglet, fil), sans notification du bureau.
  await expect(b.page).toHaveTitle('(1) KRAKEN');
  await b.page.waitForTimeout(700);
  expect(await notes()).toEqual([]);

  await choose(b.page, /^En ligne/);
  await b.page.evaluate(() => window.dispatchEvent(new Event('blur')));
  expect((await send('Deuxième pensée')).status()).toBe(201);
  await expect.poll(notes).toEqual([{ title: a.name, body: 'Deuxième pensée' }]);
  await b.page.goto(`/channels/@me/${dm}`);
  await expect(messages(b.page).filter({ hasText: 'Première pensée' })).toHaveCount(1);
});

test('S04-H4 ne pas déranger coupe la sonnerie d’appel, pas la carte d’appel', async ({ user, api }) => {
  const a = await user('appelant');
  const b = await user('endormi');
  await befriend(api, a, b);
  // La sonnerie ouvre un contexte audio : on les compte.
  await b.context.addInitScript(() => {
    const w = window as unknown as { __audio: number; AudioContext: typeof AudioContext };
    w.__audio = 0;
    const Real = w.AudioContext;
    w.AudioContext = class extends Real {
      constructor(...args: ConstructorParameters<typeof AudioContext>) {
        super(...args);
        w.__audio++;
      }
    };
  });
  await b.page.reload();
  const audio = () => b.page.evaluate(() => (window as unknown as { __audio: number }).__audio);
  await choose(b.page, /^Ne pas déranger/);

  await a.page.getByRole('tab', { name: 'Tous' }).click();
  await a.page.getByRole('button', { name: `Appeler ${b.name}` }).click();
  const ring = b.page.getByRole('region', { name: 'Appels entrants' }).getByRole('alertdialog', { name: `Appel de ${a.name}` });
  await expect(ring).toBeVisible();
  await b.page.waitForTimeout(1200);
  expect(await audio()).toBe(0);
  // En ligne : la sonnerie démarre.
  await choose(b.page, /^En ligne/);
  await expect(ring).toBeVisible();
  await expect.poll(audio).toBeGreaterThan(0);
});

// ───────── largeur mobile ─────────

test.describe('largeur mobile', () => {
  test.use({ viewport: { width: 390, height: 844 } });

  const fits = async (page: Page) =>
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth)).toBe(true);

  test('S20-M1 Mon profil, Mon compte, la carte de profil et le menu du statut tiennent à 390 px', async ({ user, api }) => {
    const a = await user('petite');
    const b = await user('mince');
    // Le rail des serveurs est dans le tiroir à cette largeur : le serveur est donc créé par l'API.
    const asA = await api(a.token);
    const made = await (await asA.post(`${API}/guilds`, { data: { name: 'Crique' } })).json();
    const gid = made.guild.id as string;
    const cid = made.channels.find((c: { kind: string }) => c.kind === 'text').id as string;
    await invite(api, a, cid, b);
    expect((await asA.patch(`${API}/users/@me`, { data: { bio: 'Une bio assez longue pour passer à la ligne sur un petit écran, avec un_mot_très_très_long_sans_espace_du_tout.', pronouns: 'iel/iels' } })).status()).toBe(200);

    await a.page.goto('/settings/profiles');
    await expect(a.page.getByRole('region', { name: 'Aperçu de ta carte' })).toBeVisible();
    await fits(a.page);
    await a11y(a.page);
    await a.page.goto('/settings/account');
    await expect(a.page.getByRole('form', { name: 'Pseudo' })).toBeVisible();
    await fits(a.page);
    await a11y(a.page);

    // La carte, ouverte depuis un message (la liste des membres est repliée à cette largeur).
    await a.page.goto(`/channels/${gid}/${cid}`);
    await b.page.goto(`/channels/${gid}/${cid}`);
    await expect(b.page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
    await say(a.page, 'général', 'Salut de la crique');
    await messages(b.page).filter({ hasText: 'Salut de la crique' }).getByRole('button', { name: a.name }).click();
    const card = b.page.getByRole('dialog', { name: `Profil de ${a.name}` });
    await expect(card).toBeVisible();
    await actionsBreathe(card);
    const box = (await card.boundingBox())!;
    expect(box.x).toBeGreaterThanOrEqual(0);
    expect(box.x + box.width).toBeLessThanOrEqual(390);
    await fits(b.page);
    await a11y(b.page);
    await b.page.keyboard.press('Escape');

    // Le menu du statut, dans le tiroir de navigation.
    await b.page.getByRole('button', { name: 'Ouvrir la navigation' }).click();
    await b.page.getByRole('button', { name: /^Mon compte/ }).click();
    const menu = b.page.getByRole('group', { name: 'Mon compte' });
    await expect(menu.getByRole('radio', { name: 'Ne pas déranger' })).toBeAttached();
    const menuBox = (await menu.boundingBox())!;
    expect(menuBox.x + menuBox.width).toBeLessThanOrEqual(390);
    await fits(b.page);
    await a11y(b.page);
  });
});
