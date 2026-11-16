// Langues de l'interface, lot « salon » (PR #31) : le fil de messages, la saisie, les réponses, les réactions, les
// épingles, les pièces jointes, le sélecteur d'emojis, les notifications d'un salon et les non-lus, en anglais.
// Fichiers migrés : chat.rs, pins.rs, emoji.rs, files.rs, unread.rs, markdown.rs. Chaque test garde un œil sur
// le français : aucune des phrases françaises du catalogue « chat » ne doit rester à l'écran, texte ou nom accessible.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test as base, expect, a11y, watch, API, PASSWORD, unique } from '../fixtures';
import { png } from '../images';
import type { APIRequestContext, BrowserContext, Locator, Page } from '@playwright/test';

type Account = { page: Page; context: BrowserContext; id: string; name: string; token: string };

/**
 * `signedIn(locale?, viewport?)` : un compte neuf, connecté dans un navigateur de cette langue, sans attendre un
 * titre de page (la fixture `user` attend « Amis », écrit en français).
 */
const test = base.extend<{ signedIn: (locale?: string, viewport?: { width: number; height: number }) => Promise<Account> }>({
  signedIn: async ({ browser, baseURL, issues, locale: browserLocale }, use) => {
    const contexts: BrowserContext[] = [];
    await use(async (locale = browserLocale ?? 'fr-FR', viewport = { width: 1440, height: 900 }) => {
      const context = await browser.newContext({ baseURL, locale, viewport });
      contexts.push(context);
      const name = unique('i18nc');
      const res = await context.request.post(`${API}/auth/register`, { data: { username: name, email: `${name}@example.org`, password: PASSWORD } });
      expect(res.status(), await res.text()).toBeLessThan(300);
      const { token, user } = await res.json();
      const page = await context.newPage();
      watch(page, issues);
      await page.goto('/login');
      await page.evaluate(() => localStorage.setItem('kraken.signed_in', '1'));
      await page.goto('/channels/@me');
      await expect(page.locator('#main')).toBeVisible();
      return { page, context, id: user.id, name, token };
    });
    for (const c of contexts) await c.close();
  },
});

test.use({ locale: 'en-US' });

// ───────── outils ─────────

/** Les phrases françaises du catalogue « chat » : ce qui ne doit plus se lire en anglais (sans variables, sans mot identique). */
const FRENCH = readFileSync(join(__dirname, '../../crates/kraken-client/i18n/en/chat.tsv'), 'utf8')
  .split('\n')
  .filter((l) => l && !l.startsWith('#'))
  .map((l) => l.split('\t') as [string, string])
  .filter(([fr, en]) => fr !== en && !fr.includes('{') && fr.length >= 8)
  .map(([fr]) => fr.replace(/\\n/g, '\n'));

/** Des mots français courts, trop courts pour la liste ci-dessus : cherchés comme mots entiers. */
const SHORT = ['Répondre', 'Modifier', 'Supprimer', 'Fermer', 'Annuler', 'Envoyer', 'Réagir', 'Réessayer', 'Télécharger', 'Retirer', 'Membres', 'Fils', 'Épinglé', 'Enregistrer', 'Rechercher', 'Débloquer'];

/** Tout ce qu'on lit ou entend sur la page : le texte visible et les noms accessibles (aria-label, title, placeholder, alt). */
const everything = (p: Page) =>
  p.evaluate(() => {
    const names = [...document.querySelectorAll('[aria-label],[title],[placeholder],[alt]')].flatMap((e) =>
      ['aria-label', 'title', 'placeholder', 'alt'].map((a) => e.getAttribute(a) ?? ''),
    );
    return document.body.innerText + '\n' + names.join('\n');
  });

/** Aucune phrase française de ce lot n'est à l'écran. */
async function noFrench(p: Page) {
  expect(FRENCH.length, 'le catalogue « chat » est lu').toBeGreaterThan(100);
  const seen = await everything(p);
  const words = SHORT.filter((w) => new RegExp(`(^|[^\\p{L}])${w}($|[^\\p{L}])`, 'u').test(seen));
  expect({ phrases: FRENCH.filter((k) => seen.includes(k)), words }).toEqual({ phrases: [], words: [] });
}

const noScroll = async (p: Page) =>
  expect(await p.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(p.viewportSize()!.width);
/** La zone `status` de la coquille avec ce texte (il y en a d'autres : on filtre toujours). */
const said = (p: Page, text: string) => p.getByRole('status').filter({ hasText: text });
const messages = (p: Page) => p.getByRole('log').getByRole('article');
/** Le message qui contient `text`, confirmé par le serveur. */
async function sent(p: Page, text: string) {
  const m = messages(p).filter({ hasText: text });
  await expect(m).toHaveCount(1);
  await expect(m).not.toHaveAttribute('data-pending', '');
  return m;
}
const toolbar = (m: Locator) => m.getByRole('toolbar', { name: 'Message actions' });
const actions = (m: Locator) => toolbar(m).getByRole('button').evaluateAll((els) => els.map((e) => e.getAttribute('aria-label')));
const asApi = (api: (t?: string) => Promise<APIRequestContext>, a: Account) => api(a.token);
const withHeader = (a: Account) => ({ Authorization: `Session ${a.token}` });

/** Un serveur (par l'API) : rend son id, celui de #général, et le code d'une invitation. */
async function guildOf(as: APIRequestContext, name: string) {
  const res = await as.post(`${API}/guilds`, { data: { name } });
  expect(res.status(), await res.text()).toBeLessThan(300);
  const g = await res.json();
  const cid: string = g.channels.find((c: { kind: string }) => c.kind === 'text').id;
  const inv = await as.post(`${API}/channels/${cid}/invites`, { data: {} });
  expect(inv.status(), await inv.text()).toBeLessThan(300);
  return { gid: g.guild.id as string, cid, code: (await inv.json()).code as string };
}

async function post(as: APIRequestContext, cid: string, content: string, extra: object = {}): Promise<string> {
  const res = await as.post(`${API}/channels/${cid}/messages`, { data: { content, ...extra } });
  expect(res.status(), await res.text()).toBe(201);
  return (await res.json()).id;
}

const react = async (as: APIRequestContext, cid: string, mid: string, emoji: string) =>
  expect((await as.put(`${API}/channels/${cid}/messages/${mid}/reactions/${encodeURIComponent(emoji)}/@me`)).status()).toBeLessThan(300);

async function befriend(api: (t?: string) => Promise<APIRequestContext>, a: Account, b: Account) {
  expect((await (await api(a.token)).post(`${API}/users/@me/relationships`, { data: { username: b.name } })).status()).toBe(201);
  expect((await (await api(b.token)).put(`${API}/users/@me/relationships/${a.id}`, { data: { kind: 'friend' } })).status()).toBe(200);
}

// ───────── le salon, vide ─────────

test('I18N-C1 salon vide en anglais : barre d’outils, saisie, membres, panneau des épingles vide ; rien de français', async ({ signedIn, api }) => {
  const a = await signedIn('en-US');
  const { gid, cid } = await guildOf(await asApi(api, a), 'Workshop');
  const p = a.page;
  await p.goto(`/channels/${gid}/${cid}`);
  await expect(p.locator('html')).toHaveAttribute('lang', 'en');
  await expect(p.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();

  // La barre d'outils : noms accessibles et infobulles en anglais.
  await expect(p.getByRole('link', { name: 'Threads', exact: true })).toHaveAttribute('title', 'Threads');
  await expect(p.getByRole('button', { name: 'Pinned messages', exact: true })).toHaveAttribute('title', 'Pinned messages');
  await expect(p.getByRole('button', { name: 'Member list' })).toHaveAttribute('title', 'Members');
  await expect(p.getByRole('button', { name: 'Channel notifications' })).toHaveAttribute('title', 'Notifications');
  await expect(p.getByRole('link', { name: 'Search', exact: true })).toHaveAttribute('title', 'Search');
  await expect(p.getByRole('link', { name: 'Inbox', exact: true })).toHaveAttribute('title', 'Inbox');

  // Le fil et la saisie.
  await expect(p.getByRole('region', { name: 'Messages in général' })).toBeVisible();
  await expect(p.getByRole('log', { name: 'Messages' })).toBeVisible();
  await expect(p.getByRole('heading', { name: 'général', level: 2 })).toBeVisible();
  await expect(p.getByText("This is the start of #général. The channel's full history starts here.")).toBeVisible();
  await expect(p.getByRole('textbox', { name: 'Message #général' })).toHaveAttribute('placeholder', 'Message #général');
  await expect(p.getByRole('button', { name: 'Attach a file' })).toHaveAttribute('title', 'Attach a file');
  await expect(p.getByRole('button', { name: 'Insert an emoji' })).toHaveAttribute('title', 'Emoji');
  await expect(p.getByRole('button', { name: 'Send', exact: true })).toBeVisible();

  // Les membres, ouverts d'office sur grand écran.
  const members = p.getByRole('complementary', { name: 'Members' });
  await expect(members.getByRole('heading', { name: 'Members 1', level: 2 })).toBeVisible();
  await expect(members.locator('button.app-who').first()).toHaveAttribute('title', 'Open profile');
  await expect(members.getByText('(owner)')).toHaveCount(1);
  await noFrench(p);
  await a11y(p);

  // Le panneau des épingles, vide : un titre, de quoi faire, et le compteur ; un seul panneau à la fois.
  const opener = p.getByRole('button', { name: 'Pinned messages', exact: true });
  await opener.click();
  const pins = p.getByRole('complementary', { name: 'Pinned messages' });
  await expect(pins.getByRole('heading', { name: 'Pinned messages', level: 2 })).toBeFocused();
  await expect(pins.getByText('No pinned messages')).toBeVisible();
  await expect(pins.getByText('Pin a message from its actions to find it here.')).toBeVisible();
  await expect(pins.getByText('0 / 50')).toBeVisible();
  await expect(pins.getByRole('button', { name: 'Close pinned messages' })).toHaveAttribute('title', 'Close');
  await expect(members).toHaveCount(0);
  await noFrench(p);
  await a11y(p);
  await p.keyboard.press('Escape');
  await expect(pins).toHaveCount(0);
  await expect(opener).toBeFocused();
});

// ───────── messages, réponses, réactions, épingles ─────────

test('I18N-C2 messages, réponses, réactions, épingles, modification et suppression en anglais ; <bdi> et dir="auto"', async ({ signedIn, api }) => {
  const a = await signedIn('en-US');
  const b = await signedIn('en-US');
  const asA = await asApi(api, a);
  const asB = await asApi(api, b);
  const { gid, cid, code } = await guildOf(asA, 'Workshop');
  expect((await asB.post(`${API}/invites/${code}`)).status()).toBeLessThan(300);
  const first = await post(asA, cid, 'First message from the owner');
  const hello = await post(asB, cid, 'Hello from the other member');
  await post(asA, cid, 'Replying to the other member', { reply_to: hello });
  await post(asB, cid, 'مرحبا بالعالم');
  await react(asB, cid, first, '👍');
  await react(asA, cid, first, '👍');
  await react(asB, cid, hello, '🔥');
  expect((await asA.put(`${API}/channels/${cid}/pins/${first}`)).status()).toBe(200);

  const p = a.page;
  await p.goto(`/channels/${gid}/${cid}`);
  const mine = await sent(p, 'First message from the owner');
  // La réponse cite le message d'origine : le premier article qui le contient est l'original.
  const theirs = messages(p).filter({ hasText: 'Hello from the other member' }).first();
  await expect(theirs).not.toHaveAttribute('data-pending', '');
  const reply = await sent(p, 'Replying to the other member');

  // L'en-tête : le pseudo dans un <bdi> ; le repère « Pinned » dit aux lecteurs d'écran.
  await expect(mine.locator('header b bdi')).toHaveText(a.name);
  await expect(mine.locator('header .app-pinned').getByText('Pinned', { exact: true })).toHaveCount(1);
  // Le texte des messages prend le sens d'écriture de son contenu ; le rendu latin ne change pas.
  await expect(mine.locator('.app-md p')).toHaveAttribute('dir', 'auto');
  await expect(mine.locator('.app-md p')).toHaveCSS('direction', 'ltr');
  const arabic = await sent(p, 'مرحبا بالعالم');
  await expect(arabic.locator('.app-md p')).toHaveCSS('direction', 'rtl');

  // La barre d'outils d'un message épinglé, de moi, que je peux gérer : dans l'ordre, le danger en dernier.
  await mine.hover();
  expect(await actions(mine)).toEqual(['Add a reaction', 'Reply', 'Unpin message', 'Edit message', 'Delete message']);
  await expect(toolbar(mine).getByRole('button', { name: 'Add a reaction' })).toHaveAttribute('title', 'React');
  await expect(toolbar(mine).getByRole('button', { name: 'Edit message' })).toHaveAttribute('title', 'Edit');
  await expect(toolbar(mine).getByRole('button', { name: 'Delete message' })).toHaveAttribute('title', 'Delete');
  // Celui d'un autre : je gère les messages, je peux le supprimer, pas le modifier.
  await theirs.hover();
  expect(await actions(theirs)).toEqual(['Add a reaction', 'Reply', 'Pin message', 'Delete message']);

  // Les réactions : le groupe, le nombre dit en toutes lettres, « dont la tienne », qui a réagi, une de plus.
  const bar = mine.getByRole('group', { name: 'Reactions' });
  const thumbs = bar.getByRole('button', { name: '👍: 2 reactions, including yours' });
  await expect(thumbs).toHaveCount(1);
  await expect(thumbs).toHaveAttribute('title', new RegExp(`^(${a.name} and ${b.name}|${b.name} and ${a.name}) reacted with 👍$`));
  await expect(bar.getByRole('button', { name: 'Add another reaction' })).toHaveAttribute('title', 'Add another reaction');
  const fire = theirs.getByRole('group', { name: 'Reactions' }).getByRole('button', { name: '🔥: 1 reaction' });
  await expect(fire).toHaveAttribute('title', `${b.name} reacted with 🔥`);

  // La réponse : la ligne qui mène au message d'origine.
  const origin = reply.getByRole('button', { name: `Go to the original message from ${b.name}` });
  await expect(origin).toContainText('Hello from the other member');
  await expect(origin).toContainText(`@${b.name}`);

  // Répondre : la barre au-dessus de la saisie, et son annulation.
  await theirs.hover();
  await toolbar(theirs).getByRole('button', { name: 'Reply' }).click();
  const replying = p.locator('.app-replying');
  await expect(replying).toContainText("You're replying to");
  await expect(replying.locator('b bdi')).toHaveText(b.name);
  await expect(replying.getByRole('button', { name: 'Cancel reply' })).toHaveAttribute('title', 'Cancel reply (Esc)');
  await noFrench(p);
  await p.getByRole('textbox', { name: 'Message #général' }).press('Escape');
  await expect(replying).toHaveCount(0);

  // Épingler depuis la barre : la zone d'état le dit, le panneau suit.
  await theirs.hover();
  await toolbar(theirs).getByRole('button', { name: 'Pin message', exact: true }).click();
  await expect(said(p, 'Message pinned.')).toBeVisible();
  await p.getByRole('button', { name: 'Pinned messages', exact: true }).click();
  const pins = p.getByRole('complementary', { name: 'Pinned messages' });
  const rows = pins.getByRole('listitem');
  await expect(rows).toHaveCount(2);
  await expect(pins.getByText('Most recently pinned first.')).toBeVisible();
  await expect(pins.getByText('2 / 50')).toBeVisible();
  await expect(rows.nth(0)).toContainText('Hello from the other member');
  await expect(rows.nth(0).locator('.app-pin-head b bdi')).toHaveText(b.name);
  await expect(rows.nth(0).getByRole('button', { name: `Go to message from ${b.name}` })).toHaveText('Go to message');
  await expect(rows.nth(0).getByRole('button', { name: `Unpin message from ${b.name}` })).toHaveText('Unpin');
  await expect(rows.nth(1).locator('.app-pin-text')).toHaveAttribute('dir', 'auto');
  await noFrench(p);
  await a11y(p);
  // Désépingler depuis le panneau.
  await rows.nth(0).getByRole('button', { name: 'Unpin message from', exact: false }).click();
  await expect(said(p, 'Message unpinned.')).toBeVisible();
  await expect(rows).toHaveCount(1);
  await rows.nth(0).getByRole('button', { name: 'Unpin message from', exact: false }).click();
  await expect(pins.getByText('No pinned messages')).toBeVisible();
  await p.keyboard.press('Escape');

  // Modifier : la zone de saisie, son aide, puis « (edited) ».
  await reply.hover();
  await toolbar(reply).getByRole('button', { name: 'Edit message' }).click();
  const box = p.getByRole('textbox', { name: 'Edit message' });
  await expect(box).toBeFocused();
  await expect(p.getByText('Enter saves, Esc cancels.')).toBeVisible();
  await expect(p.getByRole('button', { name: 'Cancel', exact: true })).toBeVisible();
  await expect(p.getByRole('button', { name: 'Save', exact: true })).toBeVisible();
  await noFrench(p);
  await box.fill('Replying, corrected');
  await box.press('Enter');
  const edited = await sent(p, 'Replying, corrected');
  const mark = edited.locator('.app-edited');
  await expect(mark).toHaveText('(edited)');
  await expect(mark).toHaveAttribute('title', /^Edited .+/);

  // Supprimer : la confirmation, avec ou sans « son auteur n'en sera pas prévenu ».
  await theirs.hover();
  await toolbar(theirs).getByRole('button', { name: 'Delete message' }).click();
  const dialog = p.getByRole('alertdialog', { name: 'Delete this message?' });
  await expect(dialog.getByText("It will disappear for everyone and can't be undone. Its author won't be notified.")).toBeVisible();
  await expect(dialog.getByRole('button', { name: 'Cancel' })).toBeFocused();
  await expect(dialog.getByRole('button', { name: 'Delete' })).toBeVisible();
  await noFrench(p);
  await a11y(p);
  await dialog.getByRole('button', { name: 'Cancel' }).click();
  await edited.hover();
  await toolbar(edited).getByRole('button', { name: 'Delete message' }).click();
  await expect(p.getByRole('alertdialog').getByText("It will disappear for everyone and can't be undone.", { exact: true })).toBeVisible();
  await p.getByRole('alertdialog').getByRole('button', { name: 'Delete' }).click();
  await expect(messages(p).filter({ hasText: 'Replying, corrected' })).toHaveCount(0);

  // Un membre simple : seulement répondre et réagir.
  await b.page.goto(`/channels/${gid}/${cid}`);
  const other = await sent(b.page, 'First message from the owner');
  await other.hover();
  expect(await actions(other)).toEqual(['Add a reaction', 'Reply']);
  await expect(other.getByRole('group', { name: 'Reactions' }).getByRole('button', { name: '👍: 2 reactions, including yours' })).toHaveCount(1);
  await noFrench(b.page);
  await settled(p);
  await a11y(p);
});

/** Plus aucun envoi en cours : les lignes confirmées sont redessinées, on attend qu'elles aient fini. */
const settled = (p: Page) => expect(p.locator('article[data-pending]')).toHaveCount(0);

test('I18N-C3 trois réactions ou plus : la liste des noms, « and 2 others » et « and 1 other »', async ({ signedIn, api }) => {
  const a = await signedIn('en-US');
  const others = [await signedIn('en-US'), await signedIn('en-US'), await signedIn('en-US')];
  const asA = await asApi(api, a);
  const { gid, cid, code } = await guildOf(asA, 'Crowd');
  const id = await post(asA, cid, 'Everybody react');
  for (const o of others) {
    expect((await (await asApi(api, o)).post(`${API}/invites/${code}`)).status()).toBeLessThan(300);
  }
  await react(asA, cid, id, '🎉');
  await react(await asApi(api, others[0]), cid, id, '🎉');
  await a.page.goto(`/channels/${gid}/${cid}`);
  const m = await sent(a.page, 'Everybody react');
  const party = m.getByRole('group', { name: 'Reactions' }).getByRole('button', { name: /^🎉: / });
  await expect(party).toHaveAttribute('aria-label', '🎉: 2 reactions, including yours');
  await react(await asApi(api, others[1]), cid, id, '🎉');
  await expect(party).toHaveAttribute('aria-label', '🎉: 3 reactions, including yours');
  await expect(party).toHaveAttribute('title', /^[^,]+, [^,]+ and [^,]+ reacted with 🎉$/);
  await react(await asApi(api, others[2]), cid, id, '🎉');
  await expect(party).toHaveAttribute('title', /^[^,]+, [^,]+, [^,]+ and 1 other reacted with 🎉$/);
  await noFrench(a.page);
});

// ───────── pièces jointes ─────────

test('I18N-C4 pièces jointes : plateau, taille en anglais, refus, calque de dépôt, fichiers et visionneuse', async ({ signedIn, api }) => {
  const a = await signedIn('en-US');
  const { gid, cid } = await guildOf(await asApi(api, a), 'Files');
  const p = a.page;
  await p.goto(`/channels/${gid}/${cid}`);
  await expect(p.getByRole('textbox', { name: 'Message #général' })).toBeVisible();

  // Le calque de dépôt : « Drop to attach to #général ».
  await p.evaluate(() => {
    const dt = new DataTransfer();
    dt.items.add(new File(['x'], 'drag.txt'));
    document.querySelector('.app-chat')!.dispatchEvent(new DragEvent('dragenter', { dataTransfer: dt, bubbles: true, cancelable: true }));
  });
  await expect(p.locator('.app-drop')).toContainText('Drop to attach to #général');
  await p.evaluate(() => {
    const dt = new DataTransfer();
    dt.items.add(new File(['x'], 'drag.txt'));
    document.querySelector('.app-chat')!.dispatchEvent(new DragEvent('dragleave', { dataTransfer: dt, bubbles: true, cancelable: true }));
  });
  await expect(p.locator('.app-drop')).toHaveCount(0);

  // Un fichier vide est refusé en anglais ; les autres rejoignent le plateau.
  await p.locator('input[type=file]').setInputFiles([
    { name: 'empty.txt', mimeType: 'text/plain', buffer: Buffer.alloc(0) },
    { name: 'notes.txt', mimeType: 'text/plain', buffer: Buffer.from('hello world') },
    { name: 'report.txt', mimeType: 'text/plain', buffer: Buffer.alloc(12_800, 97) },
    { name: 'photo.png', mimeType: 'image/png', buffer: png(64, 48) },
    { name: 'photo2.png', mimeType: 'image/png', buffer: png(48, 64) },
  ]);
  await expect(p.getByRole('alert').filter({ hasText: '"empty.txt" is empty.' })).toBeVisible();
  const tray = p.getByRole('list', { name: 'Files to attach' });
  await expect(tray.getByRole('listitem')).toHaveCount(4);
  await expect(tray.getByText('11 B')).toBeVisible();
  await expect(tray.getByText('12.5 KB')).toBeVisible();
  await expect(tray.getByRole('button', { name: 'Remove notes.txt' })).toHaveAttribute('title', 'Remove');
  await expect(tray.getByRole('textbox', { name: 'Alt text for photo.png' })).toHaveAttribute('placeholder', 'Description');
  await noFrench(p);
  await a11y(p);
  await p.getByRole('alert').getByRole('button', { name: 'Close' }).click();

  // Retirer un fichier, puis envoyer.
  await tray.getByRole('button', { name: 'Remove notes.txt' }).click();
  await expect(tray.getByRole('listitem')).toHaveCount(3);
  await expect(tray.locator('[role=progressbar]')).toHaveCount(0, { timeout: 15_000 });
  await p.getByRole('textbox', { name: 'Message #général' }).fill('Here are the files');
  await p.getByRole('button', { name: 'Send' }).click();
  const m = await sent(p, 'Here are the files');
  await expect(m.getByRole('link', { name: 'Download report.txt' })).toHaveText('Download');
  await expect(m.getByRole('link', { name: 'report.txt', exact: true })).toBeVisible();
  await expect(m.getByText('12.5 KB')).toBeVisible();
  await expect(m.getByRole('button', { name: 'Open image photo.png' })).toBeVisible();

  // La visionneuse : deux images, « 1 of 2 ».
  await m.getByRole('button', { name: 'Open image photo.png' }).click();
  const viewer = p.getByRole('dialog', { name: 'Image viewer' });
  await expect(viewer.getByText(/ (B|KB|MB) · 1 of 2$/)).toBeVisible();
  await expect(viewer.getByRole('link', { name: 'Open original' })).toHaveText('Open original');
  await expect(viewer.getByRole('link', { name: 'Download' })).toHaveText('Download');
  await expect(viewer.getByRole('button', { name: 'Close' })).toBeFocused();
  await expect(viewer.getByRole('button', { name: 'Next image' })).toHaveAttribute('title', 'Next (→)');
  await expect(viewer.getByRole('button', { name: 'Previous image' })).toHaveAttribute('title', 'Previous (←)');
  await noFrench(p);
  await a11y(p);
  await p.keyboard.press('Escape');
  await expect(viewer).toHaveCount(0);
  await settled(p);
});

// ───────── sélecteur d'emojis, saisie ─────────

test('I18N-C5 sélecteur d’emojis : noms en anglais, recherche, emojis du serveur, suggestions et limite de taille', async ({ signedIn, api }) => {
  const a = await signedIn('en-US');
  const asA = await asApi(api, a);
  const { gid, cid } = await guildOf(asA, 'Emoji Lab');
  const up = await asA.post(`${API}/guilds/${gid}/emojis?name=blobfish`, { data: png(32, 32), headers: { 'content-type': 'image/png' } });
  expect(up.status(), await up.text()).toBeLessThan(300);
  const p = a.page;
  await p.goto(`/channels/${gid}/${cid}`);
  const box = p.getByRole('textbox', { name: 'Message #général' });
  await expect(box).toBeVisible();

  // Le sélecteur, depuis la saisie : nom, recherche, les deux sections.
  await p.getByRole('button', { name: 'Insert an emoji' }).click();
  const picker = p.getByRole('dialog', { name: 'Insert an emoji' });
  await expect(picker.getByRole('searchbox', { name: 'Search emojis' })).toHaveAttribute('placeholder', 'Search emojis');
  await expect(picker.locator('.app-picker-title').first()).toContainText('Emoji Lab');
  await expect(picker.getByRole('group', { name: 'Emojis from Emoji Lab' }).getByRole('button', { name: ':blobfish:' })).toBeVisible();
  await expect(picker.getByRole('heading', { name: 'Standard emojis' })).toBeVisible();
  await expect(picker.getByRole('group', { name: 'Emojis', exact: true })).toBeVisible();
  const red = picker.getByRole('button', { name: 'red heart' });
  await expect(red).toHaveAttribute('title', 'red heart');
  await expect(picker.getByRole('button', { name: 'thumbs up, agree' })).toBeVisible();
  await noFrench(p);
  await a11y(p);
  // La recherche porte sur les noms anglais, pas sur les noms français.
  await picker.getByRole('searchbox').fill('heart');
  await expect(picker.getByRole('button', { name: 'broken heart' })).toBeVisible();
  await expect(picker.getByRole('button', { name: 'thumbs up, agree' })).toHaveCount(0);
  await picker.getByRole('searchbox').fill('cœur');
  await expect(picker.getByText('No emoji in the selection matches.')).toBeVisible();
  await picker.getByRole('searchbox').fill('hello');
  await picker.getByRole('searchbox').press('Escape');
  await expect(picker).toHaveCount(0);

  // Choisir un emoji l'insère ; réagir à un message ouvre le même sélecteur, nommé autrement.
  await p.getByRole('button', { name: 'Insert an emoji' }).click();
  await picker.getByRole('button', { name: 'red heart' }).click();
  await expect(box).toHaveValue('❤️');
  await box.fill('Reacting here');
  await box.press('Enter');
  const m = await sent(p, 'Reacting here');
  await m.hover();
  await toolbar(m).getByRole('button', { name: 'Add a reaction' }).click();
  await expect(p.getByRole('dialog', { name: 'React with an emoji' })).toBeVisible();
  await p.getByRole('dialog', { name: 'React with an emoji' }).getByRole('button', { name: 'red heart' }).click();
  await expect(m.getByRole('button', { name: '❤️: 1 reaction, including yours' })).toBeVisible();
  // Un emoji du serveur en réaction : son nom en toutes lettres.
  await m.hover();
  await toolbar(m).getByRole('button', { name: 'Add a reaction' }).click();
  await p.getByRole('dialog', { name: 'React with an emoji' }).getByRole('button', { name: ':blobfish:' }).click();
  await expect(m.getByRole('button', { name: ':blobfish:: 1 reaction, including yours' })).toBeVisible();

  // Les suggestions : les mentions parlent anglais, les emojis du serveur aussi.
  await box.fill('@ev');
  const list = p.getByRole('listbox', { name: 'Suggestions' });
  await expect(list.getByRole('option', { name: /@everyone/ })).toContainText('Everyone who can see this channel');
  await box.fill('@he');
  await expect(list.getByRole('option', { name: /@here/ })).toContainText('Everyone who is online right now');
  await box.fill('see :blob');
  await expect(list.getByRole('option', { name: /:blobfish:/ })).toContainText('Emoji Lab');
  await noFrench(p);
  await a11y(p);
  await box.fill('');

  // Trop long : le compte, en anglais, et l'envoi refusé.
  await box.fill('x'.repeat(4001));
  await expect(p.getByRole('alert').filter({ hasText: '4001 out of 4000 characters: shorten it a bit.' })).toBeVisible();
  await noFrench(p);
  await box.fill('');
  await settled(p);
});

// ───────── notifications, non-lus, sourdine ─────────

test('I18N-C6 notifications d’un salon et d’un serveur, menu contextuel, non-lus et mentions dits en anglais', async ({ signedIn, api }) => {
  const a = await signedIn('en-US');
  const b = await signedIn('en-US');
  const asA = await asApi(api, a);
  const { gid, cid, code } = await guildOf(asA, 'Quiet Place');
  expect((await (await asApi(api, b)).post(`${API}/invites/${code}`)).status()).toBeLessThan(300);
  const p = a.page;
  await p.goto(`/channels/${gid}/${cid}`);
  await expect(p.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();

  // La boîte du salon : sourdine, niveau, aide.
  await p.getByRole('button', { name: 'Channel notifications' }).click();
  const dialog = p.getByRole('dialog', { name: 'Notifications for #général' });
  await expect(dialog.getByRole('combobox', { name: 'Mute' })).toBeVisible();
  expect(await dialog.getByRole('combobox', { name: 'Mute' }).locator('option').allTextContents()).toEqual([
    'Not muted',
    'For 15 minutes',
    'For 1 hour',
    'For 8 hours',
    'For 24 hours',
    'Until I unmute',
  ]);
  await expect(dialog.getByText('When muted, nothing turns bold and you get no notifications. Mentions are still counted.')).toBeVisible();
  const levels = dialog.getByRole('group', { name: 'Notify me for' });
  await expect(levels.getByRole('radio', { name: /Same as the server/ })).toBeChecked();
  await expect(levels.getByText('Currently: mentions only')).toBeVisible();
  await expect(levels.getByRole('radio', { name: /^All messages/ })).toBeVisible();
  await expect(levels.getByText('Every new message can show up on your desktop.')).toBeVisible();
  await expect(levels.getByRole('radio', { name: /^Mentions only/ })).toBeVisible();
  await expect(levels.getByText('Only when someone mentions you, replies to your message or pings everyone.')).toBeVisible();
  await expect(levels.getByRole('radio', { name: /Nothing/ })).toBeVisible();
  await expect(levels.getByText('No notifications and no mention counter.')).toBeVisible();
  await noFrench(p);
  await a11y(p);
  await levels.getByRole('radio', { name: /^All messages/ }).check();
  await expect(levels.getByRole('radio', { name: /^All messages/ })).toBeChecked();
  await dialog.getByRole('combobox', { name: 'Mute' }).selectOption({ label: 'For 1 hour' });
  await expect(dialog.getByRole('combobox', { name: 'Mute' }).locator('option').first()).toHaveText('Not muted');
  await expect(dialog.getByRole('combobox', { name: 'Mute' }).locator('option[value=current]')).toHaveText(/^Muted until /);
  await noFrench(p);
  await dialog.getByRole('button', { name: 'Close' }).click();
  await expect(dialog).toHaveCount(0);
  // Une sourdine se dit aussi aux lecteurs d'écran, dans la barre latérale.
  const link = p.getByRole('navigation', { name: 'Channels' }).getByRole('link', { name: /général/ });
  await expect(link).toContainText(', muted');

  // Le menu contextuel du salon, au clic droit : « Unmute » puisque c'est en sourdine.
  await link.click({ button: 'right' });
  const menu = p.getByRole('menu', { name: 'Actions for #général' });
  await expect(menu.getByRole('menuitem', { name: 'Mark as read' })).toBeVisible();
  await expect(menu.getByRole('menuitem', { name: 'Unmute' })).toBeVisible();
  await expect(menu.getByRole('menuitem', { name: 'Notifications…' })).toBeVisible();
  await noFrench(p);
  await menu.getByRole('menuitem', { name: 'Unmute' }).click();
  await expect(link).not.toContainText('muted');
  await link.click({ button: 'right' });
  await expect(menu.getByRole('menuitem', { name: 'Mute…' })).toBeVisible();
  await p.keyboard.press('Escape');

  // La boîte du serveur : le niveau par défaut, et @everyone.
  await p.getByRole('button', { name: 'Server notifications' }).click();
  const guildDialog = p.getByRole('dialog', { name: 'Notifications for Quiet Place' });
  await expect(guildDialog.getByRole('radio', { name: /Mentions only \(default\)/ })).toBeChecked();
  await expect(guildDialog.getByText('Ignore @everyone and @here')).toBeVisible();
  await expect(guildDialog.getByText('These server-wide pings no longer count as mentions.')).toBeVisible();
  await noFrench(p);
  await a11y(p);
  await guildDialog.getByRole('button', { name: 'Close' }).click();

  // Non-lu et mention, vus depuis l'accueil : le nom du serveur dans le rail le dit en anglais.
  await p.goto('/channels/@me');
  await expect(p.locator('#main')).toBeVisible();
  const rail = p.getByRole('navigation', { name: 'Servers' });
  await expect(rail.getByRole('link', { name: /^Server Quiet Place/ })).toBeVisible();
  await post(await asApi(api, b), cid, `@${a.name} are you there?`);
  await expect(rail.getByRole('link', { name: 'Server Quiet Place, unread, 1 mention' })).toBeVisible();
});

// ───────── erreurs ─────────

test('I18N-C7 échecs fréquents en anglais : envoi manqué et nouvel essai, fil et épingles impossibles à charger', async ({ signedIn, api, allow }) => {
  allow.push(/Failed to load resource|ERR_/);
  const a = await signedIn('en-US');
  const { gid, cid } = await guildOf(await asApi(api, a), 'Flaky');
  const old = await post(await asApi(api, a), cid, 'An earlier message');
  expect((await (await asApi(api, a)).put(`${API}/channels/${cid}/pins/${old}`)).status()).toBe(200);
  const p = a.page;

  // Le fil ne se charge pas : le message, l'erreur du réseau traduite, et « Retry ».
  let down = true;
  await p.route(/\/api\/v1\/channels\/[^/]+\/messages\?/, (r) => (down && r.request().method() === 'GET' ? r.abort() : r.continue()));
  await p.goto(`/channels/${gid}/${cid}`);
  const empty = p.getByRole('log').locator('.k-empty');
  await expect(empty.getByText("Couldn't load messages")).toBeVisible();
  await expect(empty.getByText("The server isn't responding. Check your connection.")).toBeVisible();
  await noFrench(p);
  down = false;
  await empty.getByRole('button', { name: 'Retry' }).click();
  await sent(p, 'An earlier message');

  // Un envoi qui n'aboutit pas : « sending… », puis « not sent », réessayer ou supprimer.
  let blocked = true;
  await p.route(/\/api\/v1\/channels\/[^/]+\/messages$/, (r) => (blocked && r.request().method() === 'POST' ? r.abort() : r.continue()));
  const box = p.getByRole('textbox', { name: 'Message #général' });
  await box.fill('This one will fail');
  await box.press('Enter');
  const failed = messages(p).filter({ hasText: 'This one will fail' });
  await expect(failed.locator('time')).toHaveText('not sent');
  await expect(failed.getByRole('alert')).toContainText("The server isn't responding. Check your connection.");
  await expect(failed.getByRole('button', { name: 'Retry' })).toBeVisible();
  await expect(failed.getByRole('button', { name: 'Delete' })).toBeVisible();
  await noFrench(p);
  await a11y(p);
  blocked = false;
  await failed.getByRole('button', { name: 'Retry' }).click();
  await sent(p, 'This one will fail');

  // Les épingles ne se chargent pas : le panneau le dit, et « Retry » les relit.
  let pinsDown = true;
  await p.route(/\/api\/v1\/channels\/[^/]+\/pins$/, (r) => (pinsDown ? r.abort() : r.continue()));
  await p.getByRole('button', { name: 'Pinned messages', exact: true }).click();
  const pins = p.getByRole('complementary', { name: 'Pinned messages' });
  await expect(pins.getByText("Couldn't load pinned messages")).toBeVisible();
  await expect(pins.getByText("The server isn't responding. Check your connection.")).toBeVisible();
  await noFrench(p);
  pinsDown = false;
  await pins.getByRole('button', { name: 'Retry' }).click();
  await expect(pins.getByRole('listitem')).toHaveCount(1);
  await expect(pins.getByText('An earlier message')).toBeVisible();
  await settled(p);
});

test('I18N-C8 un message d’un compte bloqué et un MP bloqué : repli, bannière et déblocage en anglais', async ({ signedIn, api }) => {
  const a = await signedIn('en-US');
  const b = await signedIn('en-US');
  const asA = await asApi(api, a);
  const asB = await asApi(api, b);
  const { gid, cid, code } = await guildOf(asA, 'Blocks');
  expect((await asB.post(`${API}/invites/${code}`)).status()).toBeLessThan(300);
  const said1 = await post(asB, cid, 'Something from someone blocked');
  expect((await asA.put(`${API}/channels/${cid}/pins/${said1}`)).status()).toBe(200);
  await befriend(api, a, b);
  const dm = await (await asA.post(`${API}/users/@me/channels`, { data: { recipients: [b.id] } })).json();

  // Le MP d'abord : l'introduction, la saisie, et « Create a group with … » pour un ami.
  const p = a.page;
  await p.goto(`/channels/@me/${dm.id}`);
  await expect(p.getByText(`This is the start of your conversation with ${b.name}. It stays between the two of you.`)).toBeVisible();
  await expect(p.getByRole('textbox', { name: `Message ${b.name}` })).toBeVisible();
  await expect(p.getByRole('button', { name: `Create a group with ${b.name}` })).toHaveAttribute('title', 'Create a group');
  await expect(p.getByRole('button', { name: 'Channel notifications' })).toHaveCount(0);
  await expect(p.getByRole('button', { name: 'Conversation notifications' })).toHaveAttribute('title', 'Notifications');
  await expect(p.getByRole('region', { name: `Messages in ${b.name}` })).toBeVisible();
  await noFrench(p);
  await a11y(p);

  // Bloquer l'autre : la bannière à la place de la saisie.
  expect((await asA.put(`${API}/users/@me/relationships/${b.id}`, { data: { kind: 'blocked' } })).status()).toBe(200);
  await p.reload();
  await expect(p.getByText(`You blocked ${b.name}, so neither of you can write here. The history is still readable.`)).toBeVisible();
  await expect(p.getByRole('button', { name: 'Unblock' })).toBeVisible();
  await expect(p.getByRole('textbox', { name: `Message ${b.name}` })).toHaveCount(0);
  await noFrench(p);

  // Dans le serveur : son message est replié derrière une ligne, dépliable.
  await p.goto(`/channels/${gid}/${cid}`);
  const folded = p.getByRole('log').locator('.app-blocked');
  await expect(folded.getByText('Message from an account you blocked.')).toBeVisible();
  const show = folded.getByRole('button', { name: 'Show' });
  await expect(show).toHaveAttribute('aria-expanded', 'false');
  await show.click();
  await expect(folded.getByRole('button', { name: 'Hide' })).toHaveAttribute('aria-expanded', 'true');
  await expect(messages(p).filter({ hasText: 'Something from someone blocked' })).toBeVisible();
  // Et dans le panneau des épingles : même ligne, même bouton.
  await p.getByRole('button', { name: 'Pinned messages', exact: true }).click();
  const pins = p.getByRole('complementary', { name: 'Pinned messages' });
  await expect(pins.getByText('Message from an account you blocked.')).toBeVisible();
  await expect(pins.getByRole('button', { name: 'Show' })).toBeVisible();
  await noFrench(p);
  await a11y(p);
});

test('I18N-C9 groupe privé : barre d’outils, membres, propriétaire et retrait en anglais', async ({ signedIn, api }) => {
  const a = await signedIn('en-US');
  const b = await signedIn('en-US');
  const c = await signedIn('en-US');
  await befriend(api, a, b);
  await befriend(api, a, c);
  const asA = await asApi(api, a);
  const g = await (await asA.post(`${API}/users/@me/channels`, { data: { recipients: [b.id, c.id], name: 'The three' } })).json();
  const p = a.page;
  await p.goto(`/channels/@me/${g.id}`);
  await expect(p.getByRole('heading', { name: 'The three', level: 1 })).toBeVisible();
  for (const [name, title] of [
    ['Member list', 'Members'],
    ['Add friends to the group', 'Add friends'],
    ['Rename group', 'Rename'],
    ['Leave group', 'Leave group'],
  ]) {
    await expect(p.getByRole('button', { name, exact: true })).toHaveAttribute('title', title);
  }
  await expect(p.getByRole('textbox', { name: 'Message The three' })).toBeVisible();
  const members = p.getByRole('complementary', { name: 'Group members' });
  await expect(members.getByRole('heading', { name: 'Members 3', level: 2 })).toBeVisible();
  await expect(members.getByText('(owner)')).toHaveCount(1);
  await members.getByRole('button', { name: `More actions for ${b.name}` }).click();
  await expect(p.getByRole('button', { name: 'Remove from group' })).toBeVisible();
  await noFrench(p);
  await a11y(p);
  await p.keyboard.press('Escape');
});

// ───────── petit écran ─────────

test('I18N-C10 sur 390 px : le fil, la feuille des épingles et les membres tiennent sans débordement, axe propre', async ({ signedIn, api }) => {
  const a = await signedIn('en-US', { width: 390, height: 844 });
  const asA = await asApi(api, a);
  const { gid, cid } = await guildOf(asA, 'Pocket');
  const one = await post(asA, cid, 'A pinned note with a rather long line that has to wrap on a narrow screen without ever pushing the page wider');
  await post(asA, cid, 'Second message, with a very-long-unbroken-token-that-should-wrap-' + 'x'.repeat(60));
  await react(asA, cid, one, '👍');
  expect((await asA.put(`${API}/channels/${cid}/pins/${one}`)).status()).toBe(200);
  const up = await asA.post(`${API}/channels/${cid}/attachments?filename=a-long-file-name-for-a-narrow-screen.txt`, { data: Buffer.from('hello'), headers: { 'content-type': 'text/plain' } });
  expect(up.status(), await up.text()).toBeLessThan(300);
  await post(asA, cid, 'With a file', { attachments: [{ id: (await up.json()).id }] });
  const p = a.page;
  await p.goto(`/channels/${gid}/${cid}`);
  await sent(p, 'With a file');
  await expect(p.getByRole('textbox', { name: 'Message #général' })).toBeVisible();
  await noScroll(p);
  await noFrench(p);
  await a11y(p);

  // La feuille des épingles : une boîte de dialogue pleine page, avec sa fermeture visible.
  await p.getByRole('button', { name: 'Pinned messages', exact: true }).click();
  const sheet = p.getByRole('dialog', { name: 'Pinned messages' });
  await expect(sheet.getByRole('heading', { name: 'Pinned messages', level: 2 })).toBeFocused();
  await expect(sheet.getByRole('listitem')).toHaveCount(1);
  await expect(sheet.getByRole('button', { name: 'Go to message from' + ` ${a.name}` })).toBeVisible();
  await noScroll(p);
  await noFrench(p);
  await a11y(p);
  await sheet.getByRole('button', { name: 'Close pinned messages' }).click();
  await expect(sheet).toHaveCount(0);

  // La liste des membres, tiroir par-dessus le fil.
  await p.getByRole('button', { name: 'Member list' }).click();
  await expect(p.getByRole('complementary', { name: 'Members' })).toBeVisible();
  await noScroll(p);
  await noFrench(p);
  await a11y(p);
});

// ───────── le français ne bouge pas ─────────

test.describe('navigateur en français', () => {
  test.use({ locale: 'fr-FR' });

  test('I18N-C11 les mêmes écrans en français : les textes d’origine, ni anglais ni espace perdu', async ({ signedIn, api }) => {
    const a = await signedIn('fr-FR');
    const b = await signedIn('fr-FR');
    const asA = await asApi(api, a);
    const { gid, cid, code } = await guildOf(asA, 'Atelier');
    expect((await (await asApi(api, b)).post(`${API}/invites/${code}`)).status()).toBeLessThan(300);
    const first = await post(asA, cid, 'Premier message');
    await react(await asApi(api, b), cid, first, '👍');
    await react(asA, cid, first, '👍');
    const p = a.page;
    await p.goto(`/channels/${gid}/${cid}`);
    await expect(p.locator('html')).toHaveAttribute('lang', 'fr');
    const m = await sent(p, 'Premier message');
    await m.hover();
    expect(await m.getByRole('toolbar', { name: 'Actions sur le message' }).getByRole('button').evaluateAll((els) => els.map((e) => e.getAttribute('aria-label')))).toEqual([
      'Ajouter une réaction',
      'Répondre',
      'Épingler le message',
      'Modifier le message',
      'Supprimer le message',
    ]);
    await expect(m.getByRole('button', { name: '👍 : 2 réactions, dont la tienne' })).toBeVisible();
    await expect(p.getByRole('textbox', { name: 'Écrire dans #général' })).toBeVisible();
    await expect(p.getByRole('region', { name: 'Messages de général' })).toBeVisible();
    await expect(p.getByText("Ici commence #général. Tout l'historique du salon se lit à partir d'ici.")).toBeVisible();
    await expect(p.getByRole('heading', { name: 'Membres 2', level: 2 })).toBeVisible();
    await p.getByRole('button', { name: 'Messages épinglés', exact: true }).click();
    await expect(p.getByText('Aucun message épinglé')).toBeVisible();
    await expect(p.getByText('0 / 50')).toBeVisible();
    await a11y(p);
  });
});
