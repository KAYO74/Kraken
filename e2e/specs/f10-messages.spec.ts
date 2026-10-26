// F10 Messages complets : modifier, supprimer, répondre, réagir, markdown et mentions.
import { test, expect, a11y, createGuild, say, messages, API, type User } from '../fixtures';
import type { Page } from '@playwright/test';

/** `a` crée un serveur, `b` le rejoint par invitation ; les deux sont sur #général. */
async function together(a: User, b: User, name: string) {
  const [gid, cid] = await createGuild(a.page, name);
  const res = await a.context.request.post(`${API}/channels/${cid}/invites`, {
    data: {},
    headers: { Authorization: `Session ${a.token}` },
  });
  const { code } = await res.json();
  await b.page.goto(`/invite/${code}`);
  await b.page.getByRole('button', { name: 'Rejoindre le serveur' }).click();
  await expect(b.page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
  return [gid, cid];
}

/** Le message qui contient `text`, une fois confirmé par le serveur. */
async function sent(page: Page, text: string) {
  const m = messages(page).filter({ hasText: text });
  await expect(m).toHaveCount(1);
  await expect(m).not.toHaveAttribute('data-pending', '');
  return m;
}

/** Ouvre la barre d'actions d'un message (survol) et clique l'action. */
async function act(page: Page, text: string, action: string) {
  const m = await sent(page, text);
  await m.hover();
  await m.getByRole('toolbar', { name: 'Actions sur le message' }).getByRole('button', { name: action }).click();
}

test('F10-H1 modifier son message : Entrée enregistre, Échap annule, l’autre voit la modification', async ({ user }) => {
  const a = await user('mod');
  const b = await user('lec');
  await together(a, b, 'Atelier des corrections');
  await say(a.page, 'général', 'Premier jet avec une fote');
  await expect(messages(b.page).filter({ hasText: 'Premier jet' })).toHaveCount(1);

  await act(a.page, 'Premier jet', 'Modifier le message');
  const box = a.page.getByRole('textbox', { name: 'Modifier le message' });
  await expect(box).toBeFocused();
  await box.fill('Brouillon abandonné');
  await box.press('Escape');
  await expect(box).toHaveCount(0);
  await expect(messages(a.page).filter({ hasText: 'Premier jet avec une fote' })).toHaveCount(1);

  await act(a.page, 'Premier jet', 'Modifier le message');
  await box.fill('Premier jet sans faute');
  await box.press('Enter');
  const fixed = messages(a.page).filter({ hasText: 'Premier jet sans faute' });
  await expect(fixed).toContainText('(édité)');
  await expect(messages(b.page).filter({ hasText: 'Premier jet sans faute' })).toContainText('(édité)');
  await a.page.reload();
  await expect(messages(a.page).filter({ hasText: 'Premier jet sans faute' })).toContainText('(édité)');

  // Flèche haut dans une saisie vide : modifier son dernier message.
  await a.page.getByLabel('Écrire dans #général').press('ArrowUp');
  await expect(a.page.getByRole('textbox', { name: 'Modifier le message' })).toHaveValue('Premier jet sans faute');
  await a11y(a.page);
});

test('F10-H2 supprimer son message après confirmation, disparu chez l’autre', async ({ user }) => {
  const a = await user('sup');
  const b = await user('vue');
  await together(a, b, 'Grand ménage');
  await say(a.page, 'général', 'À effacer bientôt');
  await expect(messages(b.page).filter({ hasText: 'À effacer bientôt' })).toHaveCount(1);

  await act(a.page, 'À effacer bientôt', 'Supprimer le message');
  const dialog = a.page.getByRole('alertdialog', { name: 'Supprimer ce message ?' });
  await expect(dialog).toContainText('À effacer bientôt');
  await a11y(a.page);
  await dialog.getByRole('button', { name: 'Annuler' }).click();
  await expect(dialog).toHaveCount(0);
  await expect(messages(a.page).filter({ hasText: 'À effacer bientôt' })).toHaveCount(1);

  await act(a.page, 'À effacer bientôt', 'Supprimer le message');
  await dialog.getByRole('button', { name: 'Supprimer' }).click();
  await expect(messages(a.page).filter({ hasText: 'À effacer bientôt' })).toHaveCount(0);
  await expect(messages(b.page).filter({ hasText: 'À effacer bientôt' })).toHaveCount(0);
  await a.page.reload();
  await expect(a.page.getByText("Ici commence #général.")).toBeVisible();
  await expect(messages(a.page)).toHaveCount(0);
});

test('F10-N1 un membre ne modifie ni ne supprime le message d’un autre', async ({ user, api, allow }) => {
  allow.push(/status of 403/);
  const a = await user('own');
  const b = await user('oth');
  const [, cid] = await together(a, b, 'Chacun ses mots');
  await say(a.page, 'général', 'Écrit par le propriétaire');
  const theirs = await sent(b.page, 'Écrit par le propriétaire');
  await theirs.hover();
  const tools = theirs.getByRole('toolbar', { name: 'Actions sur le message' });
  await expect(tools.getByRole('button', { name: 'Répondre' })).toBeVisible();
  await expect(tools.getByRole('button', { name: 'Modifier le message' })).toHaveCount(0);
  await expect(tools.getByRole('button', { name: 'Supprimer le message' })).toHaveCount(0);

  const asA = await api(a.token);
  const list = (await (await asA.get(`${API}/channels/${cid}/messages`)).json()) as { id: string; content: string }[];
  const id = list.find((m) => m.content === 'Écrit par le propriétaire')!.id;
  const asB = await api(b.token);
  expect((await asB.patch(`${API}/channels/${cid}/messages/${id}`, { data: { content: 'pirate' } })).status()).toBe(403);
  expect((await asB.delete(`${API}/channels/${cid}/messages/${id}`)).status()).toBe(403);
  await expect(messages(a.page).filter({ hasText: 'Écrit par le propriétaire' })).toHaveCount(1);

  // Le propriétaire, lui, peut supprimer le message de b.
  await say(b.page, 'général', 'Message de b à modérer');
  await act(a.page, 'Message de b à modérer', 'Supprimer le message');
  await expect(a.page.getByRole('alertdialog')).toContainText("Son auteur n'en sera pas prévenu");
  await a.page.getByRole('alertdialog').getByRole('button', { name: 'Supprimer' }).click();
  await expect(messages(b.page).filter({ hasText: 'Message de b à modérer' })).toHaveCount(0);
});

test('F10-N2 la modération (MANAGE_MESSAGES) supprime le message d’un membre', async ({ demo }) => {
  const writer = await demo('demo');
  const modo = await demo('sam');
  const text = `À modérer ${Date.now()}`;
  for (const u of [writer, modo]) {
    await u.page.getByRole('link', { name: 'Serveur Abysse' }).click();
    await u.page.getByRole('navigation', { name: 'Salons' }).getByRole('link', { name: 'général' }).click();
    await expect(u.page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
  }
  await say(writer.page, 'général', text);
  await act(modo.page, text, 'Supprimer le message');
  await modo.page.getByRole('alertdialog').getByRole('button', { name: 'Supprimer' }).click();
  await expect(messages(writer.page).filter({ hasText: text })).toHaveCount(0);
});

test('F10-H3 répondre : la citation s’affiche au-dessus et mène au message d’origine', async ({ user }) => {
  const a = await user('ori');
  const b = await user('rep');
  await together(a, b, 'Fil de réponses');
  await say(a.page, 'général', 'Qui vient plonger samedi ?');
  for (let i = 0; i < 25; i++) await say(a.page, 'général', `bavardage ${i}`);
  await sent(b.page, 'bavardage 24');

  await act(b.page, 'Qui vient plonger', 'Répondre');
  await expect(b.page.getByText('Tu réponds à')).toBeVisible();
  const box = b.page.getByLabel('Écrire dans #général');
  await expect(box).toBeFocused();
  await box.fill('Moi, avec plaisir !');
  await box.press('Enter');
  await expect(b.page.getByText('Tu réponds à')).toHaveCount(0);
  const reply = await sent(a.page, 'Moi, avec plaisir !');
  const link = reply.getByRole('button', { name: /Aller au message d'origine/ });
  await expect(link).toContainText('Qui vient plonger samedi ?');
  await link.click();
  const original = messages(a.page).filter({ hasText: 'Qui vient plonger samedi ?' }).filter({ hasNotText: 'Moi, avec plaisir' });
  await expect(original).toBeFocused();
  await expect(original).toBeInViewport();

  // Échap annule une réponse commencée.
  await act(a.page, 'bavardage 3', 'Répondre');
  await a.page.getByLabel('Écrire dans #général').press('Escape');
  await expect(a.page.getByText('Tu réponds à')).toHaveCount(0);

  // L'original supprimé : la réponse le dit.
  await original.hover();
  await original.getByRole('button', { name: 'Supprimer le message' }).click();
  await a.page.getByRole('alertdialog').getByRole('button', { name: 'Supprimer' }).click();
  await expect(messages(b.page).filter({ hasText: 'Moi, avec plaisir' })).toContainText("Le message d'origine a été supprimé.");
  await a11y(b.page);
});

test('F10-H3b le message d’origine hors de l’historique chargé est retrouvé', async ({ user, api }) => {
  const a = await user('loin');
  const [, cid] = await createGuild(a.page, 'Mémoire longue');
  const http = await api(a.token);
  const post = async (data: object) => (await http.post(`${API}/channels/${cid}/messages`, { data })).json();
  const first = await post({ content: 'Tout au fond de l’historique' });
  for (let i = 0; i < 60; i++) await post({ content: `remplissage ${i}` });
  await post({ content: 'Je reviens sur ce point', reply_to: first.id });
  await a.page.reload();
  const reply = await sent(a.page, 'Je reviens sur ce point');
  await expect(messages(a.page).filter({ hasText: 'Tout au fond' }).filter({ hasNotText: 'Je reviens' })).toHaveCount(0);
  await reply.getByRole('button', { name: /Aller au message d'origine/ }).click();
  const original = messages(a.page).filter({ hasText: 'Tout au fond' }).filter({ hasNotText: 'Je reviens' });
  await expect(original).toBeFocused();
  await expect(original).toBeInViewport();
});

test('F10-H4 réactions : sélecteur, compteur, qui a réagi, retrait', async ({ user }) => {
  const a = await user('rea');
  const b = await user('cti');
  await together(a, b, 'Récif des réactions');
  await say(a.page, 'général', 'On fait une sortie ?');
  await act(a.page, 'On fait une sortie', 'Ajouter une réaction');
  const picker = a.page.getByRole('dialog', { name: 'Réagir avec un emoji' });
  await expect(picker.getByRole('searchbox', { name: 'Chercher un emoji' })).toBeFocused();
  await a11y(a.page);
  await picker.getByRole('searchbox').fill('pieuvre');
  await picker.getByRole('button', { name: 'pieuvre, kraken' }).click();
  await expect(picker).toHaveCount(0);

  const mineA = messages(a.page).filter({ hasText: 'On fait une sortie' });
  await expect(mineA.getByRole('button', { name: '🐙 : 1 réaction, dont la tienne' })).toHaveAttribute('aria-pressed', 'true');
  const onB = messages(b.page).filter({ hasText: 'On fait une sortie' });
  const octoB = onB.getByRole('button', { name: /^🐙 : 1 réaction$/ });
  await expect(octoB).toHaveAttribute('aria-pressed', 'false');
  await octoB.click();
  const both = onB.getByRole('button', { name: '🐙 : 2 réactions, dont la tienne' });
  await expect(both).toHaveAttribute('title', `${a.name} et ${b.name} ont réagi avec 🐙`);
  await expect(mineA.getByRole('button', { name: '🐙 : 2 réactions, dont la tienne' })).toBeVisible();

  // Retirer : clic sur sa propre réaction ; le clavier suffit.
  await mineA.getByRole('button', { name: '🐙 : 2 réactions, dont la tienne' }).focus();
  await a.page.keyboard.press('Enter');
  await expect(onB.getByRole('button', { name: '🐙 : 1 réaction, dont la tienne' })).toBeVisible();
  await a.page.reload();
  await expect(messages(a.page).filter({ hasText: 'On fait une sortie' }).getByRole('button', { name: /^🐙 : 1 réaction$/ })).toBeVisible();

  // Échap ferme le sélecteur ; seuls les emojis passent côté serveur.
  await act(a.page, 'On fait une sortie', 'Ajouter une réaction');
  await a.page.keyboard.press('Escape');
  await expect(a.page.getByRole('dialog', { name: 'Réagir avec un emoji' })).toHaveCount(0);
});

test('F10-H5 markdown : mise en forme, code, citation, spoiler, lien ; jamais de HTML', async ({ user }) => {
  const { page } = await user('md');
  await createGuild(page, 'Typographie abyssale');
  const box = page.getByLabel('Écrire dans #général');
  await box.fill('**gras** *penché* __souligné__ ~~barré~~ `code` ||fin du film|| https://exemple.org/page <b>brut</b>');
  await box.press('Enter');
  const m = await sent(page, 'gras');
  await expect(m.locator('strong')).toHaveText('gras');
  await expect(m.locator('em')).toHaveText('penché');
  await expect(m.locator('u')).toHaveText('souligné');
  await expect(m.locator('s')).toHaveText('barré');
  await expect(m.locator('code')).toHaveText('code');
  await expect(m.locator('b', { hasText: 'brut' })).toHaveCount(0);
  await expect(m).toContainText('<b>brut</b>');
  const link = m.getByRole('link', { name: 'https://exemple.org/page' });
  await expect(link).toHaveAttribute('href', 'https://exemple.org/page');
  await expect(link).toHaveAttribute('rel', /noopener/);
  const spoiler = m.getByRole('button', { name: 'Afficher le contenu masqué' });
  await expect(m.getByText('fin du film')).toBeHidden();
  await spoiler.click();
  await expect(m.getByText('fin du film')).toBeVisible();

  await box.fill('> une citation');
  await box.press('Shift+Enter');
  await box.pressSequentially('et la suite');
  await box.press('Enter');
  const q = await sent(page, 'une citation');
  await expect(q.locator('blockquote')).toHaveText('une citation');
  await box.fill('```\nfn main() { println!("**pas gras**"); }\n```');
  await box.press('Enter');
  const code = await sent(page, 'fn main()');
  await expect(code.locator('pre code')).toHaveText('fn main() { println!("**pas gras**"); }');
  await expect(code.locator('strong')).toHaveCount(0);
  // Le spoiler se dévoile aussi au clavier.
  await say(page, 'général', 'clavier ||caché||');
  const k = await sent(page, 'clavier');
  await k.getByRole('button', { name: 'Afficher le contenu masqué' }).focus();
  await page.keyboard.press('Enter');
  await expect(k.getByText('caché')).toBeVisible();
  await a11y(page);
});

test('F10-H6 mentions : surlignées pour qui est visé, @everyone selon le droit, #salon en lien', async ({ user }) => {
  const a = await user('men');
  const b = await user('tio');
  await together(a, b, 'Appels dans le noir');
  // Autocomplétion : quelques lettres, Entrée.
  const box = b.page.getByLabel('Écrire dans #général');
  await box.pressSequentially(`Coucou @${a.name.slice(0, 5)}`);
  const option = b.page.getByRole('listbox', { name: 'Suggestions' }).getByRole('option', { name: new RegExp(a.name) });
  await expect(option).toBeVisible();
  await box.press('Enter');
  await expect(box).toHaveValue(`Coucou @${a.name} `);
  await box.pressSequentially('va voir #gén');
  await b.page.getByRole('listbox', { name: 'Suggestions' }).getByRole('option', { name: '#général' }).click();
  await box.press('Enter');

  const forA = await sent(a.page, 'Coucou');
  await expect(forA).toHaveClass(/k-msg--mention/);
  await expect(forA.locator('.app-mention[data-me]')).toHaveText(`@${a.name}`);
  await expect(forA.getByRole('link', { name: '#général' })).toHaveAttribute('href', /\/channels\//);
  await expect(await sent(b.page, 'Coucou')).not.toHaveClass(/k-msg--mention/);

  // b n'a pas le droit de mentionner tout le monde, le propriétaire si.
  await expect(b.page.getByRole('option', { name: '@everyone' })).toHaveCount(0);
  await say(b.page, 'général', '@everyone réveillez-vous');
  await expect(await sent(a.page, 'réveillez-vous')).not.toHaveClass(/k-msg--mention/);
  await say(a.page, 'général', '@everyone réunion au récif');
  await expect(await sent(b.page, 'réunion au récif')).toHaveClass(/k-msg--mention/);
  await a11y(b.page);
});

test('F10-H7 emoji dans la saisie : inséré au curseur', async ({ user }) => {
  const { page } = await user('emo');
  await createGuild(page, 'Émoticônes marines');
  const box = page.getByLabel('Écrire dans #général');
  await box.fill('Bonne plongée ');
  await page.getByRole('button', { name: 'Insérer un emoji' }).click();
  await page.getByRole('dialog', { name: 'Insérer un emoji' }).getByRole('button', { name: 'dauphin' }).click();
  await expect(box).toHaveValue('Bonne plongée 🐬');
  await expect(box).toBeFocused();
  await box.press('Enter');
  await sent(page, 'Bonne plongée 🐬');
});

test('F01-E10 envoi raté : Supprimer l’abandonne sans rien envoyer', async ({ user, allow }) => {
  allow.push(/ERR_INTERNET_DISCONNECTED|WebSocket/);
  const u = await user('aba');
  const [, cid] = await createGuild(u.page, 'Message perdu');
  const { page } = u;
  await u.context.setOffline(true);
  await say(page, 'général', 'Jamais parti');
  const failed = messages(page).filter({ hasText: 'Jamais parti' });
  await expect(failed).toHaveAttribute('data-failed', '');
  await failed.getByRole('button', { name: 'Supprimer' }).click();
  await expect(messages(page).filter({ hasText: 'Jamais parti' })).toHaveCount(0);
  await u.context.setOffline(false);
  const res = await u.context.request.get(`${API}/channels/${cid}/messages`);
  expect(((await res.json()) as unknown[]).length).toBe(0);
});
