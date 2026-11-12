// Emojis perso de serveur : Réglages › Emojis (ajouter, nommer, renommer, supprimer, glisser-déposer, droits,
// 50 emplacements), puis leur usage : `:nom:` avec suggestions au clavier, sélecteur, message, grand format,
// animation fixe, réaction, modification, emoji retiré. Les images sont fabriquées sur place (images.ts).
import { test, expect, a11y, createGuild, messages, API, type User } from '../fixtures';
import { png, animatedGif } from '../images';
import type { APIRequestContext, Locator, Page } from '@playwright/test';

/** Envoie un emoji par l'API (le corps est l'image) et rend sa fiche. */
async function addEmoji(as: APIRequestContext, gid: string, name: string, image: Buffer) {
  const res = await as.post(`${API}/guilds/${gid}/emojis?name=${name}`, { data: image, headers: { 'content-type': 'application/octet-stream' } });
  expect(res.status(), await res.text()).toBe(201);
  return (await res.json()) as { id: string; name: string; animated: boolean };
}

/** `owner` invite `guest` dans le serveur du salon `cid`, qui arrive sur #général. */
async function invite(as: APIRequestContext, cid: string, guest: User) {
  const { code } = await (await as.post(`${API}/channels/${cid}/invites`, { data: {} })).json();
  await guest.page.goto(`/invite/${code}`);
  await guest.page.getByRole('button', { name: 'Rejoindre le serveur' }).click();
  await expect(guest.page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
}

/** Le message qui contient `text`, une fois confirmé par le serveur. */
async function sent(page: Page, text: string) {
  const m = messages(page).filter({ hasText: text });
  await expect(m).toHaveCount(1);
  await expect(m).not.toHaveAttribute('data-pending', '');
  return m;
}

const sizeOf = async (el: Locator) => {
  const box = await el.boundingBox();
  expect(box).not.toBeNull();
  return { w: Math.round(box!.width), h: Math.round(box!.height) };
};

/** Une image qui s'est vraiment chargée. */
async function loaded(img: Locator) {
  await expect.poll(() => img.evaluate((i) => (i as HTMLImageElement).complete && (i as HTMLImageElement).naturalWidth)).toBeGreaterThan(0);
}

const settings = (p: Page) => p.getByRole('dialog', { name: 'Paramètres du serveur' });
const noOverflow = (p: Page) => p.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth);

test('emojis-H1 Réglages › Emojis : ajouter (recadrage, nom), refuser un nom, renommer, glisser-déposer, supprimer', async ({ user, api }) => {
  const owner = await user('chef');
  const [gid] = await createGuild(owner.page, 'Abysse des emojis');
  const { page } = owner;
  await page.goto(`/guilds/${gid}/settings/emojis`);
  const admin = settings(page);
  await expect(admin.getByRole('heading', { name: 'Emojis', level: 2 })).toBeVisible();
  // L'onglet vient après « Salons ».
  await expect(admin.getByRole('navigation').getByRole('link')).toHaveText(['Rôles', 'Salons', 'Emojis', 'Invitations', 'Bannis', 'Propriété']);
  await expect(admin.getByText("Aucun emoji pour l'instant")).toBeVisible();
  await expect(admin.getByText('0 / 50 emplacements')).toBeVisible();
  await a11y(page);

  // Une image fixe : choisie au bouton, recadrée en carré, puis nommée d'après son fichier.
  const chooser = page.waitForEvent('filechooser');
  await admin.getByRole('button', { name: 'Ajouter un emoji' }).click();
  await (await chooser).setFiles({ name: 'poulpe-bleu.png', mimeType: 'image/png', buffer: png(300, 180) });
  const crop = page.getByRole('dialog', { name: 'Recadrer ton emoji' });
  await expect(crop.getByRole('button', { name: 'Appliquer' })).toBeEnabled();
  await crop.getByRole('button', { name: 'Appliquer' }).click();
  const add = page.getByRole('dialog', { name: 'Ajouter un emoji' });
  const name = add.getByLabel('Nom', { exact: true });
  await expect(name).toHaveValue('poulpe_bleu');
  await expect(name).toBeFocused();
  await expect(add.getByText(':poulpe_bleu:')).toBeVisible();
  await expect(add.getByRole('img', { name: "Aperçu de l'emoji" })).toBeVisible();
  // Le fond sombre couvre toute la fenêtre : la navigation des réglages n'est pas cliquable derrière.
  const scrim = await add.locator('xpath=..').boundingBox();
  expect(scrim!.width).toBeGreaterThanOrEqual(page.viewportSize()!.width - 1);
  await a11y(page);

  // Un nom refusé s'explique sur le champ.
  await name.fill('é!');
  await add.getByRole('button', { name: "Ajouter l'emoji" }).click();
  await expect(name).toHaveAttribute('aria-invalid', 'true');
  await expect(name).toHaveAccessibleDescription(/2 à 32 caractères/);
  await expect(add.getByRole('alert')).toContainText('lettres sans accent');
  await name.fill('poulpe');
  await expect(name).not.toHaveAttribute('aria-invalid', 'true');
  await add.getByRole('button', { name: "Ajouter l'emoji" }).click();
  await expect(add).toHaveCount(0);
  const list = admin.getByRole('list', { name: 'Emojis du serveur' });
  const row = list.getByRole('listitem').first();
  await expect(row).toContainText(':poulpe:');
  await expect(row).toContainText(`ajouté par ${owner.name}`);
  await loaded(row.locator('img'));
  expect((await sizeOf(row.locator('.app-emoji-thumb'))).w).toBe(32);
  await expect(admin.getByText('1 / 50 emplacements')).toBeVisible();
  await expect(admin.getByRole('status')).toContainText('Emoji :poulpe: ajouté.');
  await expect(admin.getByRole('button', { name: 'Ajouter un emoji' })).toBeFocused();
  await a11y(page);

  // Renommer : le nom devient un champ ; Échap l'abandonne sans fermer les réglages, Entrée valide.
  await row.getByRole('button', { name: 'Actions pour :poulpe:' }).click();
  await row.getByRole('button', { name: 'Renommer' }).click();
  const field = row.getByLabel('Nouveau nom de :poulpe:');
  await expect(field).toBeFocused();
  await field.fill('Poulpe_2');
  await field.press('Escape');
  await expect(field).toHaveCount(0);
  await expect(admin).toBeVisible();
  await expect(row).toContainText(':poulpe:');
  await expect(row.getByRole('button', { name: 'Actions pour :poulpe:' })).toBeFocused();
  await row.getByRole('button', { name: 'Actions pour :poulpe:' }).click();
  await row.getByRole('button', { name: 'Renommer' }).click();
  await field.fill('x');
  await field.press('Enter');
  await expect(field).toHaveAttribute('aria-invalid', 'true');
  await expect(field).toHaveAccessibleDescription(/2 à 32 caractères/);
  await field.fill('Poulpe_2');
  await field.press('Enter');
  await expect(field).toHaveCount(0);
  await expect(row).toContainText(':Poulpe_2:');
  await expect(row.getByRole('button', { name: 'Actions pour :Poulpe_2:' })).toBeFocused();

  // Glisser-déposer d'une animation : un cadre le dit, le dépôt ouvre la fenêtre d'ajout (sans recadrage).
  const dt = await page.evaluateHandle((b64) => {
    const data = new DataTransfer();
    data.items.add(new File([Uint8Array.from(atob(b64), (c) => c.charCodeAt(0))], 'vague.gif', { type: 'image/gif' }));
    return data;
  }, animatedGif(3, 16).toString('base64'));
  await page.dispatchEvent('.app-emojis', 'dragenter', { dataTransfer: dt });
  await expect(admin.getByText("Dépose l'image pour l'ajouter")).toBeVisible();
  await page.dispatchEvent('.app-emojis', 'drop', { dataTransfer: dt });
  await expect(admin.getByText("Dépose l'image pour l'ajouter")).toBeHidden();
  await expect(add.getByLabel('Nom', { exact: true })).toHaveValue('vague');
  // Un nom déjà pris, sans tenir compte de la casse.
  await name.fill('poulpe_2');
  await add.getByRole('button', { name: "Ajouter l'emoji" }).click();
  await expect(name).toHaveAttribute('aria-invalid', 'true');
  await expect(name).toHaveAccessibleDescription(/déjà pris/);
  await name.fill('danse');
  await add.getByRole('button', { name: "Ajouter l'emoji" }).click();
  await expect(add).toHaveCount(0);
  const dance = list.getByRole('listitem').filter({ hasText: ':danse:' });
  await expect(dance).toBeVisible();
  const asOwner = await api(owner.token);
  const emojis: { id: string; name: string; animated: boolean }[] = await (await asOwner.get(`${API}/guilds/${gid}/emojis`)).json();
  expect(emojis.map((e) => [e.name, e.animated])).toEqual([['Poulpe_2', false], ['danse', true]]);

  // Supprimer : confirmation (Échap et Annuler gardent l'emoji), puis il disparaît et son image n'est plus servie.
  await dance.getByRole('button', { name: 'Actions pour :danse:' }).click();
  await dance.getByRole('button', { name: 'Supprimer' }).click();
  const confirm = page.getByRole('alertdialog', { name: 'Supprimer :danse: ?' });
  await expect(confirm.getByRole('button', { name: 'Annuler' })).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(confirm).toHaveCount(0);
  await expect(dance).toBeVisible();
  await expect(dance.getByRole('button', { name: 'Actions pour :danse:' })).toBeFocused();
  await dance.getByRole('button', { name: 'Actions pour :danse:' }).click();
  await dance.getByRole('button', { name: 'Supprimer' }).click();
  await a11y(page);
  await confirm.getByRole('button', { name: "Supprimer l'emoji" }).click();
  await expect(confirm).toHaveCount(0);
  await expect(dance).toHaveCount(0);
  await expect(admin.getByText('1 / 50 emplacements')).toBeVisible();
  await expect(admin.getByRole('status')).toContainText('Emoji :danse: supprimé.');
  expect((await asOwner.get(`${API}/emojis/${emojis[1].id}/image`)).status()).toBe(404);
  expect((await asOwner.get(`${API}/emojis/${emojis[0].id}/image`)).status()).toBe(200);
});

test('emojis-H2 écrire :nom: (suggestions au clavier), sélecteur, grand format, animation fixe, réaction, modification', async ({ user, api, allow }) => {
  allow.push(/status of 404/, /status of 400/);
  const a = await user('chef');
  const c = await user('mousse');
  const asA = await api(a.token);
  const [gx, cx] = await createGuild(a.page, 'Abysse');
  const poulpe = await addEmoji(asA, gx, 'poulpe', png(40, 40));
  await addEmoji(asA, gx, 'poussin', png(40, 40));
  await addEmoji(asA, gx, 'danse', animatedGif(3, 16));
  // Les emojis d'un autre de ses serveurs s'écrivent partout : on écrit dans Récif, où mousse n'a pas Abysse.
  const [gy, cy] = await createGuild(a.page, 'Récif');
  await addEmoji(asA, gy, 'corail', png(40, 40));
  await invite(asA, cy, c);
  const box = a.page.getByLabel('Écrire dans #général');
  const suggestions = a.page.getByRole('listbox', { name: 'Suggestions' });

  // Deux caractères au moins, et jamais au milieu d'un mot : « :p », « 12:30 » et « http://… » ne proposent rien.
  for (const text of ['salut :p', '12:30', 'http://exemple.org', ':', 'a:po']) {
    await box.fill(text);
    await box.press('End');
    await expect(suggestions).toHaveCount(0);
  }
  // Haut, bas, Entrée, Tab, Échap.
  await box.fill('');
  await box.pressSequentially('bonjour :pou');
  await expect(suggestions.getByRole('option')).toHaveText([/:poulpe:\s*Abysse/, /:poussin:\s*Abysse/]);
  await expect(box).toHaveAttribute('aria-controls', /ac-/);
  const options = suggestions.getByRole('option');
  await expect(options.nth(0)).toHaveAttribute('aria-selected', 'true');
  await expect(box).toHaveAttribute('aria-activedescendant', (await options.nth(0).getAttribute('id'))!);
  await loaded(options.nth(0).locator('img'));
  await a11y(a.page);
  await box.press('ArrowDown');
  await expect(options.nth(1)).toHaveAttribute('aria-selected', 'true');
  await box.press('ArrowUp');
  await expect(options.nth(0)).toHaveAttribute('aria-selected', 'true');
  await box.press('Escape');
  await expect(suggestions).toHaveCount(0);
  await expect(box).toHaveValue('bonjour :pou');
  await box.pressSequentially('l');
  await expect(options).toHaveCount(1);
  await box.press('Tab');
  await expect(box).toHaveValue('bonjour :poulpe: ');
  await expect(suggestions).toHaveCount(0);
  await box.press('Enter');

  // L'image s'affiche à la taille du texte, chez l'auteur comme chez mousse, qui n'est pas dans Abysse.
  const mine = messages(a.page).filter({ hasText: 'bonjour' });
  await expect(mine).toHaveCount(1);
  await loaded(mine.locator('img[alt=":poulpe:"]'));
  expect(await sizeOf(mine.locator('.app-emoji'))).toEqual({ w: 22, h: 22 });
  const theirs = messages(c.page).filter({ hasText: 'bonjour' });
  await expect(theirs).toHaveCount(1);
  await loaded(theirs.locator('img[alt=":poulpe:"]'));
  // Le serveur garde le jeton, pas le texte écrit.
  const stored = await (await asA.get(`${API}/channels/${cy}/messages`)).json();
  expect(stored.at(-1).content).toBe(`bonjour <:poulpe:${poulpe.id}>`);
  // Un nom inconnu reste du texte.
  await box.fill('personne :inconnu: ici');
  await box.press('Enter');
  const unknown = await sent(a.page, 'personne');
  await expect(unknown).toContainText('personne :inconnu: ici');
  await expect(unknown.locator('img')).toHaveCount(0);

  // Un message fait d'emojis seuls les montre en grand ; avec du texte, à la taille du texte.
  await box.fill(':poulpe: :poulpe:');
  await box.press('Enter');
  const jumbo = messages(a.page).filter({ has: a.page.locator('.app-md[data-jumbo]') });
  await expect(jumbo).toHaveCount(1);
  await expect(jumbo).not.toHaveAttribute('data-pending', '');
  await loaded(jumbo.locator('img').first());
  expect(await sizeOf(jumbo.locator('.app-emoji').first())).toEqual({ w: 48, h: 48 });
  await expect(mine.locator('.app-md[data-jumbo]')).toHaveCount(0);

  // Une animation reste fixe dans la liste, s'anime au survol, et reste fixe en mouvement réduit.
  await box.fill('regarde :danse:');
  await box.press('Enter');
  const dancing = await sent(a.page, 'regarde');
  const still = dancing.locator('img.k-still');
  const live = dancing.locator('img.k-live');
  await expect(still).toBeVisible();
  await expect(live).toBeHidden();
  await dancing.hover();
  await expect(live).toBeVisible();
  await expect(still).toBeHidden();
  await a.page.mouse.move(2, 2);
  await expect(live).toBeHidden();
  await a.page.emulateMedia({ reducedMotion: 'reduce' });
  await expect(a.page.locator('html')).toHaveAttribute('data-motion', 'reduce');
  await dancing.hover();
  await expect(live).toBeHidden();
  await expect(still).toBeVisible();
  await a.page.emulateMedia({ reducedMotion: 'no-preference' });
  await a.page.mouse.move(2, 2);

  // Sélecteur : un titre par serveur (celui du salon d'abord), cases nommées, recherche par nom, clavier.
  await a.page.getByRole('button', { name: 'Insérer un emoji' }).click();
  const picker = a.page.getByRole('dialog', { name: 'Insérer un emoji' });
  await expect(picker.getByRole('searchbox', { name: 'Chercher un emoji' })).toBeFocused();
  await expect(picker.getByRole('heading', { level: 3 })).toContainText(['Récif', 'Abysse', 'Emojis standards']);
  await expect(picker.getByRole('region', { name: 'Abysse' }).getByRole('button')).toHaveText(['', '', '']);
  await expect(picker.getByRole('button', { name: ':corail:' })).toHaveAttribute('title', ':corail:');
  expect((await sizeOf(picker.getByRole('button', { name: ':corail:' }))).w).toBeGreaterThanOrEqual(36);
  await a11y(a.page);
  await picker.getByRole('searchbox').fill('pou');
  await expect(picker.getByRole('button', { name: ':poulpe:' })).toBeVisible();
  await expect(picker.getByRole('button', { name: ':corail:' })).toHaveCount(0);
  await expect(picker.getByRole('heading', { level: 3 }).first()).toContainText('Abysse');
  await expect(picker.getByRole('heading', { level: 3, name: /Récif/ })).toHaveCount(0);
  await picker.getByRole('searchbox').fill(':corail:');
  await picker.getByRole('searchbox').press('Enter');
  await expect(picker).toHaveCount(0);
  await expect(box).toHaveValue(':corail: ');
  await box.fill('');
  await a.page.getByRole('button', { name: 'Insérer un emoji' }).click();
  await picker.getByRole('searchbox').fill('dauphin');
  await picker.getByRole('button', { name: 'dauphin' }).click();
  await expect(box).toHaveValue('🐬');
  await box.fill('');

  // Réaction avec un emoji perso, depuis le sélecteur ; l'autre membre la voit, la rejoint.
  const target = await sent(a.page, 'bonjour');
  await target.hover();
  await target.getByRole('toolbar', { name: 'Actions sur le message' }).getByRole('button', { name: 'Ajouter une réaction' }).click();
  const react = a.page.getByRole('dialog', { name: 'Réagir avec un emoji' });
  await react.getByRole('button', { name: ':poulpe:' }).click();
  await expect(react).toHaveCount(0);
  const pill = target.getByRole('button', { name: ':poulpe: : 1 réaction, dont la tienne' });
  await expect(pill).toHaveAttribute('aria-pressed', 'true');
  await expect(pill).toHaveAttribute('title', `${a.name} a réagi avec :poulpe:`);
  await loaded(pill.locator('img'));
  expect(await sizeOf(pill.locator('.app-emoji'))).toEqual({ w: 18, h: 18 });
  const onC = await sent(c.page, 'bonjour');
  const pillC = onC.locator('.k-reaction[aria-pressed]').first();
  await expect(pillC).toHaveAccessibleName('emoji personnalisé : 1 réaction');
  // Réagir demande d'être membre du serveur de l'emoji : sinon le refus s'explique.
  await pillC.click();
  await expect(c.page.getByRole('alert')).toContainText('Seuls les emojis de la sélection ou ceux de tes serveurs');
  await invite(asA, cx, c);
  await c.page.goto(`/channels/${gy}/${cy}`);
  const joined = (await sent(c.page, 'bonjour')).getByRole('button', { name: ':poulpe: : 1 réaction' });
  await joined.click();
  await expect(target.getByRole('button', { name: ':poulpe: : 2 réactions, dont la tienne' })).toBeVisible();
  await a.page.reload();
  await expect(messages(a.page).filter({ hasText: 'bonjour' }).getByRole('button', { name: ':poulpe: : 2 réactions, dont la tienne' })).toBeVisible();

  // Modifier : les jetons redeviennent `:nom:`, et une modification sans changement n'ajoute pas « édité ».
  const edited = await sent(a.page, 'bonjour');
  await edited.hover();
  await edited.getByRole('toolbar', { name: 'Actions sur le message' }).getByRole('button', { name: 'Modifier le message' }).click();
  const area = a.page.getByRole('textbox', { name: 'Modifier le message' });
  await expect(area).toHaveValue('bonjour :poulpe:');
  await area.press('Enter');
  await expect(area).toHaveCount(0);
  await expect(edited).not.toContainText('(édité)');
  await edited.hover();
  await edited.getByRole('toolbar', { name: 'Actions sur le message' }).getByRole('button', { name: 'Modifier le message' }).click();
  await area.fill('salut :poussin: et :poulpe:');
  await area.press('Enter');
  await expect(messages(a.page).filter({ hasText: 'salut' })).toContainText('(édité)');
  const after = await (await asA.get(`${API}/channels/${cy}/messages`)).json();
  expect(after.find((m: { content: string }) => m.content.startsWith('salut')).content).toMatch(/^salut <:poussin:[0-9a-f-]{36}> et <:poulpe:[0-9a-f-]{36}>$/);

  // L'emoji est retiré du serveur : il quitte le sélecteur, les messages et réactions qui le citent restent,
  // et ceux qui les découvrent ensuite n'y lisent que son nom.
  expect((await asA.delete(`${API}/guilds/${gx}/emojis/${poulpe.id}`)).status()).toBe(204);
  await a.page.getByRole('button', { name: 'Insérer un emoji' }).click();
  await expect(picker.getByRole('button', { name: ':poussin:' })).toBeVisible();
  await expect(picker.getByRole('button', { name: ':poulpe:' })).toHaveCount(0);
  await a.page.keyboard.press('Escape');
  const late = await user('curieux');
  await invite(asA, cy, late);
  const old = await sent(late.page, 'salut');
  await expect(old).toContainText(':poulpe:');
  await expect(old.locator('img[alt=":poulpe:"]')).toHaveCount(0);
  await expect(old.getByRole('button', { name: 'emoji supprimé : 2 réactions' })).toBeVisible();
  await expect(old.locator('.app-emoji-gone')).toHaveText('emoji supprimé');
  await a11y(late.page);
});

test('emojis-H3 sans « Gérer les émojis » la liste se lit sans se changer ; 50 emplacements au plus', async ({ user, api }) => {
  const owner = await user('chef');
  const mod = await user('modo');
  const asOwner = await api(owner.token);
  const [gid, cid] = await createGuild(owner.page, 'Atelier');
  await addEmoji(asOwner, gid, 'poulpe', png(32, 32));
  await invite(asOwner, cid, mod);

  // Un rôle qui bannit voit les emojis, sans pouvoir les changer.
  const role = await (await asOwner.post(`${API}/guilds/${gid}/roles`, { data: { name: 'Modération', permissions: 1 << 23 } })).json();
  expect((await asOwner.put(`${API}/guilds/${gid}/members/${mod.id}/roles/${role.id}`)).status()).toBe(200);
  await mod.page.goto(`/guilds/${gid}/settings/emojis`);
  const admin = settings(mod.page);
  await expect(admin.getByRole('navigation').getByRole('link')).toHaveText(['Emojis', 'Invitations', 'Bannis']);
  await expect(admin.getByRole('list', { name: 'Emojis du serveur' })).toContainText(':poulpe:');
  await expect(admin.getByRole('button', { name: 'Ajouter un emoji' })).toHaveCount(0);
  await expect(admin.getByRole('button', { name: /^Actions pour/ })).toHaveCount(0);
  await expect(admin.getByText('Il faut le droit « Gérer les émojis »')).toBeVisible();
  await a11y(mod.page);
  const asMod = await api(mod.token);
  const refused = await asMod.post(`${API}/guilds/${gid}/emojis?name=autre`, { data: png(16, 16), headers: { 'content-type': 'image/png' } });
  expect(refused.status()).toBe(403);
  // Un simple membre n'a pas l'onglet.
  const plain = await user('simple');
  await invite(asOwner, cid, plain);
  await plain.page.goto(`/guilds/${gid}/settings/emojis`);
  await expect(settings(plain.page).getByRole('link')).toHaveText(['Invitations']);

  // Le droit arrive en direct : le bouton d'ajout apparaît.
  expect((await asOwner.patch(`${API}/guilds/${gid}/roles/${role.id}`, { data: { permissions: (1 << 23) | (1 << 20) } })).status()).toBe(200);
  await expect(admin.getByRole('button', { name: 'Ajouter un emoji' })).toBeVisible();
  await expect(admin.getByRole('button', { name: 'Actions pour :poulpe:' })).toBeVisible();

  // 50 emplacements : le décompte, un mot sur la page, et le refus nomme la limite.
  const asModWrites = await api(mod.token);
  for (let i = 1; i < 50; i++) await addEmoji(asModWrites, gid, `e_${String(i).padStart(2, '0')}`, png(16, 16));
  await expect(admin.getByText('50 / 50 emplacements')).toBeVisible();
  await expect(admin.getByText('Tous les emplacements sont pris')).toBeVisible();
  await admin.getByRole('button', { name: 'Ajouter un emoji' }).click();
  await expect(admin.getByRole('alert')).toContainText('50 emojis');
  const over = await asModWrites.post(`${API}/guilds/${gid}/emojis?name=de_trop`, { data: png(16, 16), headers: { 'content-type': 'image/png' } });
  expect(over.status()).toBe(400);
  expect((await over.json()).code).toBe('emoji_limit');
  await a11y(mod.page);
});

test.describe('à 390 px', () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test('emojis-H4 sur mobile : réglages, fenêtre d’ajout, sélecteur et messages tiennent sans défilement horizontal', async ({ user, api }) => {
    const a = await user('chef');
    const asA = await api(a.token);
    const made = await (await asA.post(`${API}/guilds`, { data: { name: 'Abysse' } })).json();
    const gid: string = made.guild.id;
    const cid: string = made.channels.find((c: { kind: string }) => c.kind === 'text').id;
    await addEmoji(asA, gid, 'poulpe', png(40, 40));
    await addEmoji(asA, gid, 'un_nom_tres_long_pour_voir_ca_ok', png(40, 40));
    await a.page.goto(`/guilds/${gid}/settings/emojis`);
    const admin = settings(a.page);
    await expect(admin.getByRole('list', { name: 'Emojis du serveur' }).getByRole('listitem')).toHaveCount(2);
    expect((await sizeOf(admin.locator('.app-emoji-thumb').first())).w).toBe(40);
    expect(await noOverflow(a.page)).toBe(true);
    await a11y(a.page);

    const chooser = a.page.waitForEvent('filechooser');
    await admin.getByRole('button', { name: 'Ajouter un emoji' }).click();
    await (await chooser).setFiles({ name: 'danse.gif', mimeType: 'image/gif', buffer: animatedGif(3, 16) });
    const add = a.page.getByRole('dialog', { name: 'Ajouter un emoji' });
    await expect(add.getByLabel('Nom', { exact: true })).toBeFocused();
    expect((await sizeOf(add.locator('.app-emoji-add-preview'))).w).toBe(128);
    expect(await noOverflow(a.page)).toBe(true);
    await a11y(a.page);
    await add.getByRole('button', { name: 'Annuler' }).click();
    await expect(add).toHaveCount(0);

    await a.page.goto(`/channels/${gid}/${cid}`);
    await expect(a.page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
    const box = a.page.getByLabel('Écrire dans #général');
    await box.fill(':poulpe: salut :poulpe:');
    await box.press('Enter');
    const m = await sent(a.page, 'salut');
    expect(await sizeOf(m.locator('.app-emoji').first())).toEqual({ w: 22, h: 22 });
    await a.page.getByRole('button', { name: 'Insérer un emoji' }).click();
    const picker = a.page.getByRole('dialog', { name: 'Insérer un emoji' });
    await expect(picker.getByRole('button', { name: ':poulpe:' })).toBeVisible();
    const frame = await picker.boundingBox();
    expect(frame!.x).toBeGreaterThanOrEqual(0);
    expect(frame!.x + frame!.width).toBeLessThanOrEqual(390);
    expect(await noOverflow(a.page)).toBe(true);
    await a11y(a.page);
  });
});

test('emojis-H5 contraste élevé, texte à 20 et zoom à 150 % : anneau de focus de 3 px, images à l’échelle du texte', async ({ browser, baseURL, locale, issues, api }) => {
  const context = await browser.newContext({ baseURL, locale, viewport: { width: 1440, height: 900 } });
  await context.addInitScript(() => {
    localStorage.setItem('kraken.contrast', 'high');
    localStorage.setItem('kraken.text', '20');
    localStorage.setItem('kraken.zoom', '150');
  });
  const name = `contr_${Date.now().toString(36).slice(-6)}`;
  const reg = await context.request.post(`${API}/auth/register`, { data: { username: name, email: `${name}@example.org`, password: 'motdepasse-solide' } });
  expect(reg.status()).toBeLessThan(300);
  const { token } = await reg.json();
  const page = await context.newPage();
  page.on('pageerror', (e) => issues.push(`pageerror: ${e.message}`));
  await page.goto('/login');
  await page.evaluate(() => localStorage.setItem('kraken.signed_in', '1'));
  await page.goto('/channels/@me');
  await expect(page.getByRole('heading', { name: 'Amis', level: 1 })).toBeVisible();
  const [gid] = await createGuild(page, 'Lisible');
  const asA = await api(token);
  await addEmoji(asA, gid, 'poulpe', png(40, 40));
  await expect(page.locator('html')).toHaveAttribute('data-contrast', 'high');
  const box = page.getByLabel('Écrire dans #général');
  await box.fill('texte :poulpe:');
  await box.press('Enter');
  const m = await sent(page, 'texte');
  // 22 px × 1,25 (texte à 20) : l'image suit la taille du texte (le zoom, lui, agrandit tout, texte compris).
  expect(await m.locator('.app-emoji').evaluate((el) => parseFloat(getComputedStyle(el).width))).toBeCloseTo(27.5, 1);

  await page.getByRole('button', { name: 'Insérer un emoji' }).click();
  const picker = page.getByRole('dialog', { name: 'Insérer un emoji' });
  const cell = picker.getByRole('button', { name: ':poulpe:' });
  await expect(cell).toBeVisible();
  // Au clavier : de la recherche à la première case, l'anneau fait 3 px.
  await picker.getByRole('searchbox').press('Tab');
  await expect(cell).toBeFocused();
  // 3 px d'interface, arrondis à des pixels d'écran par le navigateur : au moins 3 pixels d'écran, zoom compris.
  const ring = await cell.evaluate((el) => parseFloat(getComputedStyle(el).outlineWidth) * parseFloat(getComputedStyle(document.body).zoom || '1'));
  expect(ring).toBeGreaterThanOrEqual(3);
  expect(await cell.evaluate((el) => getComputedStyle(el).outlineStyle)).toBe('solid');
  await a11y(page);
  await page.keyboard.press('Enter');
  await expect(picker).toHaveCount(0);
  await expect(box).toHaveValue(':poulpe: ');
  await context.close();
});
