// Langues de l'interface, lot « appels et voix » (PR #31) : Réglages › Voix et vidéo, le sélecteur rapide Ctrl+K, l'aide
// des raccourcis clavier, le salon vocal et les appels des MP et des groupes (barre de commandes, tuiles, panneau « En
// appel », sonnerie, contrôle à distance de l'écran), en anglais. Fichiers migrés : voice_settings.rs, voice.rs, call.rs,
// switcher.rs, shortcuts.rs. Micro, caméra et écran factices de Chromium, vrai son à travers le SFU. Chaque test garde un
// œil sur le français : aucune des phrases françaises du catalogue « voice » ne doit rester à l'écran, texte ou nom accessible.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test as base, expect, a11y, watch, API, PASSWORD, unique } from '../fixtures';
import type { APIRequestContext, BrowserContext, Page } from '@playwright/test';

/* eslint-disable @typescript-eslint/no-explicit-any */

type Account = { page: Page; context: BrowserContext; id: string; name: string; token: string };
type Api = (t?: string) => Promise<APIRequestContext>;
type Who = { id: string; name: string; token: string };

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
      const name = unique('i18nv');
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

// Micro (bip), caméra (mire) et écran (mire animée) factices, sans fenêtre d'autorisation.
test.use({ locale: 'en-US', launchOptions: { args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'] } });

// ───────── outils ─────────

const catalog = readFileSync(join(__dirname, '../../crates/kraken-client/i18n/en/voice.tsv'), 'utf8').split('\n');
/** Les phrases françaises du catalogue « voice » : ce qui ne doit plus se lire en anglais. */
const FRENCH = catalog
  .filter((l) => l && !l.startsWith('#'))
  .map((l) => l.split('\t') as [string, string])
  .filter(([fr, en]) => fr !== en && !fr.includes('{') && fr.length >= 8)
  .map(([fr]) => fr.replace(/^\\#/, '#'));
/** Des mots français courts, trop courts pour la liste ci-dessus : cherchés comme mots entiers. */
const SHORT = ['Quitter', 'Raccrocher', 'Rejoindre', 'Appel', 'Caméra', 'Accepter', 'Refuser', 'Regarder', 'Fermer', 'Récents', 'Personnes', 'Résultats', 'Appareils', 'Espace', 'Entrée', 'Échap', 'Maj'];
/** Les phrases à trou, par un morceau qui ne change pas. */
const FRAGMENTS = [/contrôle ton écran/, /Ça sonne/, /t'appelle/, /\(toi\)/, /Regarder l'écran/, /Demande envoyée/, /dans le salon\b/, /lieux? récents?/, /Aucun résultat/, /Salon vocal /, /Écran partagé/];

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
  expect(FRENCH.length, 'le catalogue « voice » est lu').toBeGreaterThan(100);
  const seen = await everything(p);
  const words = SHORT.filter((w) => new RegExp(`(^|[^\\p{L}])${w}($|[^\\p{L}])`, 'u').test(seen));
  expect({ phrases: FRENCH.filter((k) => seen.includes(k)), words, fragments: FRAGMENTS.filter((r) => r.test(seen)).map(String) }).toEqual({
    phrases: [],
    words: [],
    fragments: [],
  });
}

const noScroll = async (p: Page) =>
  expect(await p.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(p.viewportSize()!.width);

/** Les noms des appareils cachés tant que rien n'a été autorisé, comme dans un vrai navigateur (voir devices.spec.ts). */
const HIDE_LABELS = () => {
  const w = window as any;
  const md = navigator.mediaDevices;
  if (!md) return;
  const gum = md.getUserMedia.bind(md);
  md.getUserMedia = async (c?: MediaStreamConstraints) => {
    const s = await gum(c);
    w.__granted = true;
    return s;
  };
  const enumerate = md.enumerateDevices.bind(md);
  md.enumerateDevices = async () => {
    const list = await enumerate();
    if (w.__granted) return list;
    return list.map((d) =>
      Object.create(MediaDeviceInfo.prototype, {
        deviceId: { value: d.deviceId },
        groupId: { value: d.groupId },
        kind: { value: d.kind },
        label: { value: '' },
      }),
    );
  };
};

async function register(api: Api, prefix: string): Promise<Who> {
  const name = unique(prefix);
  const res = await (await api()).post(`${API}/auth/register`, { data: { username: name, email: `${name}@example.org`, password: PASSWORD } });
  expect(res.status(), await res.text()).toBeLessThan(300);
  const { token, user } = await res.json();
  return { id: user.id as string, name, token: token as string };
}

async function befriend(api: Api, a: Who, b: Who) {
  expect((await (await api(a.token)).post(`${API}/users/@me/relationships`, { data: { username: b.name } })).status()).toBe(201);
  expect((await (await api(b.token)).put(`${API}/users/@me/relationships/${a.id}`, { data: { kind: 'friend' } })).status()).toBe(200);
}

type Guild = { gid: string; text: string; voice: string };
/** Un serveur (par l'API) : son id, son salon textuel et son salon vocal. */
async function guildOf(as: APIRequestContext, name: string): Promise<Guild> {
  const res = await as.post(`${API}/guilds`, { data: { name } });
  expect(res.status(), await res.text()).toBe(201);
  const g = await res.json();
  return {
    gid: g.guild.id as string,
    text: g.channels.find((c: { kind: string }) => c.kind === 'text').id as string,
    voice: g.channels.find((c: { kind: string }) => c.kind === 'voice').id as string,
  };
}

async function channel(as: APIRequestContext, gid: string, name: string, kind: string) {
  const res = await as.post(`${API}/guilds/${gid}/channels`, { data: { name, kind, parent_id: null } });
  expect(res.status(), await res.text()).toBe(201);
  return (await res.json()) as { id: string };
}

const invite = async (api: Api, owner: Who, text: string, guest: Who) => {
  const inv = await (await api(owner.token)).post(`${API}/channels/${text}/invites`, { data: {} });
  expect(inv.status(), await inv.text()).toBeLessThan(300);
  const { code } = await inv.json();
  expect((await (await api(guest.token)).post(`${API}/invites/${code}`)).status()).toBe(200);
};

// Les éléments de l'appel, en anglais.
const tiles = (p: Page) => p.getByRole('list', { name: 'Participants' }).getByRole('listitem');
const bar = (p: Page) => p.getByRole('group', { name: 'Call controls' });
const panel = (p: Page) => p.getByRole('region', { name: 'Voice connection' });
const callArea = (p: Page) => p.getByRole('region', { name: 'Call', exact: true });
const incoming = (p: Page, from: string) => p.getByRole('region', { name: 'Incoming calls' }).getByRole('alertdialog', { name: `Call from ${from}` });
const remote = (p: Page) => p.getByRole('region', { name: 'Remote control of your screen' });
const inCall = (p: Page) => expect(panel(p).getByRole('status')).toHaveText('In call', { timeout: 20_000 });
/** Une zone `status` avec ce texte (il y en a beaucoup : on filtre toujours). */
const said = (p: Page, text: string | RegExp) => p.getByRole('status').filter({ hasText: text });

const settings = (p: Page) => p.getByRole('dialog', { name: 'Settings' });
const pttKey = (p: Page) => p.getByRole('button', { name: /Push-to-talk key/ });
const switcher = (p: Page) => p.getByRole('dialog', { name: 'Go to' });
const field = (p: Page) => switcher(p).getByRole('combobox');
const helpDialog = (p: Page) => p.getByRole('dialog', { name: 'Keyboard shortcuts' });

// ───────── Réglages › Voix et vidéo ─────────

test('I18N-V1 Réglages › Voix et vidéo en anglais : appareils, test du micro, aperçu, traitements du son, appuyer pour parler ; rien de français', async ({ signedIn }) => {
  test.setTimeout(90_000);
  const { page: p } = await signedIn('en-US');
  await p.addInitScript(HIDE_LABELS);
  await p.goto('/settings/voice');
  const dialog = settings(p);
  await expect(dialog.getByRole('heading', { name: 'Voice and video', level: 2 })).toBeVisible();
  await expect(dialog.getByRole('navigation', { name: 'Settings sections' }).getByRole('link', { name: 'Voice and video' })).toHaveAttribute('aria-current', 'page');
  await expect(dialog.getByText('These settings stay on this device. A change applies right away, even during a call.')).toBeVisible();

  // Appareils : trois listes avec « System default », puis des noms génériques tant que rien n'est autorisé.
  await expect(dialog.getByRole('heading', { name: 'Devices', level: 3 })).toBeVisible();
  const mic = dialog.getByLabel('Audio input', { exact: true });
  const speaker = dialog.getByLabel('Audio output', { exact: true });
  const camera = dialog.getByLabel('Camera', { exact: true });
  for (const select of [mic, speaker, camera]) {
    await expect(select).toBeVisible();
    await expect(select).toHaveValue('');
    await expect(select.getByRole('option', { name: 'System default' })).toHaveCount(1);
  }
  const allow = dialog.getByRole('button', { name: 'Allow access' });
  await expect(allow).toBeVisible();
  await expect(allow).toHaveAccessibleDescription("The browser only shows your devices' names once access to the microphone and camera is allowed.");
  expect(await mic.getByRole('option').allTextContents()).toEqual(['System default', 'Microphone 1', 'Microphone 2']);
  expect((await speaker.getByRole('option').allTextContents()).slice(1).every((t) => /^Speaker \d+$/.test(t))).toBe(true);
  expect((await camera.getByRole('option').allTextContents()).slice(1).every((t) => /^Camera \d+$/.test(t))).toBe(true);
  await noFrench(p);
  await a11y(p);
  await allow.click();
  await expect(allow).toHaveCount(0);
  await expect(mic.getByRole('option', { name: 'Microphone 1' })).toHaveCount(0);
  await noFrench(p);

  // Test du micro : le bouton bascule, la jauge porte son nom.
  await expect(dialog.getByRole('heading', { name: 'Microphone test', level: 3 })).toBeVisible();
  await expect(dialog.getByText('Speak: the bar rises with your voice. Nothing is recorded or played back through your speakers.')).toBeVisible();
  await expect(dialog.getByRole('meter', { name: 'Microphone level' })).toBeVisible();
  await dialog.getByRole('button', { name: 'Test microphone' }).click();
  await expect(dialog.getByRole('button', { name: 'Stop test' })).toBeVisible();
  await a11y(p);
  await dialog.getByRole('button', { name: 'Stop test' }).click();
  await expect(dialog.getByRole('button', { name: 'Test microphone' })).toBeVisible();

  // Aperçu de la caméra : seulement sur demande.
  await expect(dialog.getByRole('heading', { name: 'Camera preview', level: 3 })).toBeVisible();
  await expect(dialog.getByText('The camera only turns on during the preview, and turns off when you leave this page.')).toBeVisible();
  await expect(p.getByLabel('Your camera image')).toHaveCount(0);
  await dialog.getByRole('button', { name: 'Camera preview' }).click();
  await expect(dialog.getByRole('button', { name: 'Stop preview' })).toBeVisible();
  await expect(p.getByLabel('Your camera image')).toBeVisible();
  await noFrench(p);
  await a11y(p);
  await dialog.getByRole('button', { name: 'Stop preview' }).click();
  await expect(p.getByLabel('Your camera image')).toHaveCount(0);

  // Les trois traitements du son, leurs explications.
  await expect(dialog.getByRole('heading', { name: 'Audio processing', level: 3 })).toBeVisible();
  const echo = dialog.getByRole('switch', { name: 'Echo cancellation' });
  await expect(echo).toBeChecked();
  await expect(echo).toHaveAccessibleDescription('Keeps the sound coming out of your speakers from being picked up by your microphone.');
  await expect(dialog.getByRole('switch', { name: 'Noise suppression' })).toHaveAccessibleDescription('Reduces background noise: keyboard, fan, street.');
  await expect(dialog.getByRole('switch', { name: 'Automatic volume control' })).toHaveAccessibleDescription(
    'Keeps your voice at an even level, whether you speak loudly or softly.',
  );

  // Appuyer pour parler : l'explication, la ligne de la touche, les refus, la touche choisie.
  await expect(dialog.getByRole('heading', { name: 'Push to talk', level: 3 })).toBeVisible();
  const toggle = dialog.getByRole('switch', { name: 'Push to talk' });
  await expect(toggle).not.toBeChecked();
  await expect(toggle).toHaveAccessibleDescription(/closes a moment after you let go \(0\.2 s\)\. This only works while the window is in the foreground/);
  await expect(pttKey(p)).toHaveCount(0);
  await toggle.check();
  await expect(pttKey(p)).toBeVisible();
  await expect(pttKey(p)).toContainText('Choose a key');
  await expect(said(p, 'No key chosen: your microphone stays open as usual.')).toBeVisible();
  await noFrench(p);
  await a11y(p);
  await pttKey(p).click();
  await expect(said(p, 'Press a key… Esc cancels')).toBeVisible();
  await expect(pttKey(p)).toContainText('Waiting…');
  await p.keyboard.press('Tab');
  await expect(p.getByRole('alert').filter({ hasText: 'The Tab key is used to navigate the page: choose another one.' })).toBeVisible();
  await p.keyboard.press('Enter');
  await expect(p.getByRole('alert').filter({ hasText: "The Enter key on its own can't be used: choose another one." })).toBeVisible();
  await expect(pttKey(p)).toHaveAttribute('aria-invalid', 'true');
  await noFrench(p);
  await a11y(p);
  await p.keyboard.press('Escape');
  await expect(dialog).toBeVisible();
  await expect(pttKey(p)).toContainText('Choose a key');
  await pttKey(p).click();
  await p.keyboard.press('Space');
  await expect(pttKey(p).locator('kbd')).toHaveText('Space');
  await expect(said(p, 'Click to change the key.')).toBeVisible();
  await noFrench(p);
  await a11y(p);
});

test('I18N-V2 la touche choisie pour parler a un nom dans chaque langue ; le choix suit un changement de langue', async ({ signedIn }) => {
  test.setTimeout(90_000);
  const { page: p } = await signedIn('en-US');
  await p.goto('/settings/voice');
  await settings(p).getByRole('switch', { name: 'Push to talk' }).check();
  // Une touche sans caractère porte un nom dans la langue ; une lettre garde ce que la touche écrit.
  const keys: [string, string, string][] = [
    ['Space', 'Space', 'Espace'],
    ['ArrowUp', 'Up arrow', 'Flèche haut'],
    ['ArrowDown', 'Down arrow', 'Flèche bas'],
    ['ArrowLeft', 'Left arrow', 'Flèche gauche'],
    ['ArrowRight', 'Right arrow', 'Flèche droite'],
    ['Backspace', 'Backspace', 'Retour arrière'],
    ['CapsLock', 'Caps Lock', 'Verr. maj.'],
    ['ControlLeft', 'Left Ctrl', 'Ctrl gauche'],
    ['ControlRight', 'Right Ctrl', 'Ctrl droite'],
    ['ShiftLeft', 'Left Shift', 'Maj gauche'],
    ['ShiftRight', 'Right Shift', 'Maj droite'],
    ['AltLeft', 'Alt', 'Alt'],
    ['AltRight', 'Right Alt', 'Alt Gr'],
    ['Numpad5', 'Numpad 5', 'Pavé 5'],
    ['KeyM', 'M', 'M'],
  ];
  for (const [code, en] of keys) {
    await pttKey(p).click();
    await p.keyboard.press(code);
    await expect(pttKey(p).locator('kbd')).toHaveText(en);
    await p.waitForTimeout(450);
  }
  // Aucun nom de touche en anglais n'est gardé dans le stockage : seule la lettre l'est, quand elle diffère du code.
  expect(await p.evaluate(() => [localStorage.getItem('kraken.ptt_key'), localStorage.getItem('kraken.ptt_label')])).toEqual(['KeyM', null]);

  // Le choix d'une touche sans caractère suit la langue, après rechargement.
  await pttKey(p).click();
  await p.keyboard.press('ArrowUp');
  await expect(pttKey(p).locator('kbd')).toHaveText('Up arrow');
  expect(await p.evaluate(() => [localStorage.getItem('kraken.ptt_key'), localStorage.getItem('kraken.ptt_label')])).toEqual(['ArrowUp', null]);
  await p.evaluate(() => localStorage.setItem('kraken.lang', 'fr'));
  await p.reload();
  await expect(p.getByRole('dialog', { name: 'Réglages' })).toBeVisible();
  await expect(p.getByRole('button', { name: /Touche pour parler/ }).locator('kbd')).toHaveText('Flèche haut');
  await p.evaluate(() => localStorage.setItem('kraken.lang', 'en'));
  await p.reload();
  await expect(pttKey(p).locator('kbd')).toHaveText('Up arrow');
});

// ───────── sélecteur rapide ─────────

test('I18N-V3 le sélecteur rapide Ctrl+K en anglais : opérateurs, groupes, types de résultat, comptes, récents, état vide', async ({ signedIn, api }) => {
  test.setTimeout(90_000);
  const a = await signedIn('en-US');
  const p = a.page;
  const http = await api(a.token);
  const cove = await guildOf(http, 'Cove');
  const forge = await guildOf(http, 'Forge');
  await channel(http, cove.gid, 'news', 'announcement');
  await channel(http, cove.gid, 'ideas', 'forum');
  const secret = await channel(http, cove.gid, 'secret', 'text');
  expect((await http.patch(`${API}/channels/${secret.id}`, { data: { private: true } })).status()).toBe(200);
  const b = await register(api, 'basil');
  const c = await register(api, 'cleo');
  await befriend(api, a, b);
  await befriend(api, a, c);
  expect((await http.post(`${API}/users/@me/channels`, { data: { recipients: [b.id] } })).status()).toBe(201);
  expect((await http.post(`${API}/users/@me/channels`, { data: { recipients: [b.id, c.id], name: 'The trio' } })).status()).toBe(201);

  await p.goto(`/channels/${cove.gid}/${cove.text}`);
  await expect(p.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
  // Le bouton de la barre latérale, puis Ctrl+K : même fenêtre.
  const open = p.getByRole('button', { name: 'Go to…' });
  await expect(open).toHaveAttribute('aria-keyshortcuts', 'Control+K');
  await open.click();
  await expect(switcher(p)).toBeVisible();
  await expect(field(p)).toBeFocused();
  await expect(field(p)).toHaveAccessibleName('Channel, person or server');
  await expect(field(p)).toHaveAttribute('placeholder', 'Go to a channel, a person, a server');
  await expect(field(p)).toHaveAccessibleDescription('# channels · @ people and direct messages · * servers');
  await expect(switcher(p).getByRole('button', { name: 'Close' })).toHaveAttribute('title', 'Close (Esc)');
  for (const word of ['select', 'open', 'close']) await expect(switcher(p).locator('footer')).toContainText(word);
  await expect(switcher(p).locator('footer')).toContainText('Esc');
  // Rien de visité avant : l'état vide le dit.
  await expect(said(switcher(p), 'No recent places')).toBeAttached();
  await expect(switcher(p).getByText('Nothing recent', { exact: true })).toBeVisible();
  await expect(switcher(p).getByText('Type the name of a channel, a person or a server.')).toBeVisible();
  await noFrench(p);
  await a11y(p);

  // # : les salons, chacun avec son type lu avant le nom.
  await field(p).fill('#');
  const channels = switcher(p).getByRole('group', { name: 'Channels' });
  await expect(channels).toBeVisible();
  await expect(channels.getByRole('option', { name: /^Voice channel Général Cove/ })).toBeVisible();
  await expect(channels.getByRole('option', { name: /^Text channel général Cove/ })).toBeVisible();
  await expect(channels.getByRole('option', { name: /^Announcements news Cove/ })).toBeVisible();
  await expect(channels.getByRole('option', { name: /^Forum ideas Cove/ })).toBeVisible();
  await expect(channels.getByRole('option', { name: /^Private channel secret Cove/ })).toBeVisible();
  await expect(said(switcher(p), /^\d+ results$/)).toBeAttached();
  await noFrench(p);
  await a11y(p);
  await field(p).fill('#ideas');
  await expect(said(switcher(p), '1 result')).toBeAttached();
  await expect(switcher(p).getByRole('option')).toHaveCount(1);

  // * : les serveurs seulement.
  await field(p).fill('*');
  const servers = switcher(p).getByRole('group', { name: 'Servers' });
  await expect(servers.getByRole('option', { name: /^Server Cove/ })).toBeVisible();
  await expect(servers.getByRole('option', { name: /^Server Forge/ })).toBeVisible();
  await expect(switcher(p).getByRole('group', { name: 'Channels' })).toHaveCount(0);
  await a11y(p);

  // @ : un MP, un groupe (avec le nombre de membres), une personne sans MP ouvert.
  await field(p).fill('@');
  const dms = switcher(p).getByRole('group', { name: 'Direct messages' });
  await expect(dms.getByRole('option', { name: new RegExp(`^Direct message ${b.name}`) })).toBeVisible();
  const group = dms.getByRole('option', { name: /^Group The trio/ });
  await expect(group).toContainText('3 members');
  const people = switcher(p).getByRole('group', { name: 'People' });
  await expect(people.getByRole('option', { name: new RegExp(`^Person ${c.name}`) })).toBeVisible();
  await expect(switcher(p).getByRole('group', { name: 'Channels' })).toHaveCount(0);
  await noFrench(p);
  await a11y(p);

  // Aucun résultat : la requête est rappelée, entre guillemets anglais droits.
  await field(p).fill('zzzzqq');
  await expect(said(switcher(p), 'No results')).toBeAttached();
  await expect(switcher(p).getByText('No results for "zzzzqq"')).toBeVisible();
  await expect(field(p)).toHaveAttribute('aria-expanded', 'false');
  await noFrench(p);
  await a11y(p);

  // Aller dans un autre serveur, puis un troisième endroit : les récents comptent leurs lieux au singulier, puis au pluriel.
  await field(p).fill('*forge');
  await field(p).press('Enter');
  await expect(switcher(p)).toHaveCount(0);
  await expect(p).toHaveURL(new RegExp(`/channels/${forge.gid}/`));
  await p.keyboard.press('Control+k');
  await expect(switcher(p).getByRole('group', { name: 'Recent' }).getByRole('option', { name: /^Text channel général Cove/ })).toBeVisible();
  await expect(said(switcher(p), '1 recent place')).toBeAttached();
  await a11y(p);
  await p.keyboard.press('Escape');
  await expect(switcher(p)).toHaveCount(0);
  await p.goto(`/channels/${cove.gid}/${cove.voice}`);
  await expect(p.getByRole('heading', { name: 'Général', level: 1 })).toBeVisible();
  await p.keyboard.press('Control+k');
  await expect(said(switcher(p), '2 recent places')).toBeAttached();
  await noFrench(p);
  await p.keyboard.press('Escape');
});

// ───────── aide des raccourcis ─────────

const TERMS = [
  'Go to a channel, person or server',
  'Previous channel',
  'Next channel',
  'Previous server',
  'Next server',
  'Previous unread channel',
  'Next unread channel',
  'Send message',
  'New line',
  'Edit my last message (empty message box)',
  'Cancel the reply or edit',
  'Mute or unmute',
  'Deafen or undeafen',
  'Show keyboard shortcuts',
  'Close the open window',
];

test('I18N-V4 l’aide des raccourcis en anglais (Ctrl+/ et Réglages) : groupes, libellés, touches, « or », lecture seule ; sur Mac, ⌘ ⌥ ⇧', async ({ signedIn }) => {
  const a = await signedIn('en-US');
  const p = a.page;
  await p.keyboard.press('Control+/');
  const help = helpDialog(p);
  await expect(help).toBeVisible();
  await expect(help.getByRole('heading', { name: 'Keyboard shortcuts', level: 2 })).toBeVisible();
  await expect(help.getByRole('heading', { level: 3 })).toHaveText(['Navigation', 'Messages', 'Call', 'General']);
  expect(await help.getByRole('term').allTextContents()).toEqual(TERMS);
  const defs = await help.getByRole('definition').allTextContents();
  expect(defs).toEqual([
    'CtrlK',
    'Alt↑up arrow',
    'Alt↓down arrow',
    'CtrlAlt↑up arrow',
    'CtrlAlt↓down arrow',
    'AltShiftShift↑up arrow',
    'AltShiftShift↓down arrow',
    'Enter',
    'ShiftShiftEnter',
    '↑up arrow',
    'Esc',
    'CtrlShiftShiftM',
    'CtrlShiftShiftD',
    'Ctrl/or?',
    'Esc',
  ]);
  await expect(help.getByText("Read only: these shortcuts can't be changed yet, and none of them work outside the KRAKEN window (no global shortcut for now).")).toBeVisible();
  await expect(help.getByRole('button', { name: 'Close' })).toBeInViewport();
  await noFrench(p);
  await a11y(p);
  await p.keyboard.press('Escape');
  await expect(help).toHaveCount(0);

  // Réglages › Raccourcis clavier : la même liste.
  await p.goto('/settings/keybinds');
  const dialog = settings(p);
  await expect(dialog.getByRole('heading', { name: 'Keyboard shortcuts', level: 2 })).toBeVisible();
  expect(await dialog.getByRole('term').allTextContents()).toEqual(TERMS);
  expect(await dialog.getByRole('definition').allTextContents()).toEqual(defs);
  await expect(dialog.getByText('Read only:')).toBeVisible();
  await noFrench(p);
  await a11y(p);

  // « ? » ouvre l'aide hors des champs.
  await p.goto('/channels/@me');
  await expect(p.getByRole('link', { name: 'Friends' })).toBeVisible();
  await p.locator('main h1').click();
  await p.keyboard.press('Shift+?');
  await expect(help).toBeVisible();
  await help.getByRole('button', { name: 'Close' }).click();
  await expect(help).toHaveCount(0);

  // Sur un Mac : ⌘ ⌥ ⇧, avec leur nom dit à voix haute.
  const mac = await signedIn('en-US');
  await mac.context.addInitScript(() => Object.defineProperty(navigator, 'platform', { get: () => 'MacIntel' }));
  await mac.page.goto('/channels/@me');
  await expect(mac.page.getByRole('link', { name: 'Friends' })).toBeVisible();
  await mac.page.keyboard.press('Meta+/');
  await expect(helpDialog(mac.page)).toBeVisible();
  const macDefs = await helpDialog(mac.page).getByRole('definition').allTextContents();
  expect(macDefs.slice(0, 6)).toEqual([
    '⌘CommandK',
    '⌥Option↑up arrow',
    '⌥Option↓down arrow',
    '⌘Command⌥Option↑up arrow',
    '⌘Command⌥Option↓down arrow',
    '⌥Option⇧Shift↑up arrow',
  ]);
  await noFrench(mac.page);
  await a11y(mac.page);
});

// ───────── salon vocal ─────────

test('I18N-V5 le salon vocal en anglais : avant d’entrer, barre de commandes, tuiles, micro, son, caméra, partage, panneau « In call » ; l’autre participant reste en français', async ({ signedIn, api }) => {
  test.setTimeout(180_000);
  const a = await signedIn('en-US');
  const b = await signedIn('fr-FR');
  const cove = await guildOf(await api(a.token), 'Cove');
  await invite(api, a, cove.text, b);
  const [pa, pb] = [a.page, b.page];

  // Avant d'entrer : le salon, personne, « Join ».
  await pa.goto(`/channels/${cove.gid}/${cove.voice}`);
  await expect(pa.getByRole('heading', { name: 'Général', level: 1 })).toBeVisible();
  const room = pa.getByRole('region', { name: 'Voice channel Général' });
  await expect(room.getByText('Nobody here yet.')).toBeVisible();
  await expect(room.getByRole('button', { name: 'Join', exact: true })).toBeEnabled();
  await expect(pa.getByRole('link', { name: 'Search', exact: true })).toBeVisible();
  await expect(pa.getByRole('link', { name: 'Inbox', exact: true })).toBeVisible();
  await noFrench(pa);
  await a11y(pa);

  // B (en français) entre le premier : A le voit dans l'aperçu et dans la barre latérale.
  await pb.goto(`/channels/${cove.gid}/${cove.voice}`);
  await pb.getByRole('button', { name: 'Rejoindre', exact: true }).click();
  await expect(pb.getByRole('region', { name: 'Connexion vocale' }).getByRole('status')).toHaveText('En appel', { timeout: 20_000 });
  await expect(room.getByText('1 person in the channel')).toBeVisible();
  const side = pa.getByRole('navigation', { name: 'Channels' }).getByRole('list', { name: 'In Général' });
  await expect(side.getByText(b.name)).toBeVisible();
  await noFrench(pa);

  // A entre : le panneau de la barre latérale, la barre de commandes, les tuiles.
  await room.getByRole('button', { name: 'Join', exact: true }).click();
  await inCall(pa);
  await expect(panel(pa)).toContainText('Général / Cove');
  await expect(panel(pa).getByRole('button', { name: 'Leave voice channel' })).toBeVisible();
  await expect(tiles(pa)).toHaveCount(2);
  const mine = tiles(pa).filter({ hasText: `${a.name} (you)` });
  await expect(mine).toBeVisible();
  await expect(tiles(pa).filter({ hasText: b.name })).toBeVisible();
  await expect(tiles(pb)).toHaveCount(2);
  for (const name of ['Mute', 'Deafen', 'Camera', 'Share your screen', 'Leave']) {
    await expect(bar(pa).getByRole('button', { name, exact: true })).toBeVisible();
  }
  await expect(bar(pa).getByRole('button', { name: 'Leave', exact: true })).toHaveAttribute('title', 'Leave voice channel');
  await expect(bar(pa).getByRole('button', { name: 'Mute', exact: true })).toHaveAttribute('title', 'Mute');
  await noFrench(pa);
  await a11y(pa);

  // Micro coupé, puis son : A voit ses icônes (en anglais), B les voit en français ; le bouton du micro change d'infobulle.
  const mute = bar(pa).getByRole('button', { name: 'Mute', exact: true });
  const tileA = tiles(pb).filter({ hasText: a.name });
  await mute.click();
  await expect(mute).toHaveAttribute('aria-pressed', 'true');
  await expect(mute).toHaveAttribute('title', 'Unmute');
  await expect(mine.getByRole('img', { name: 'muted' })).toHaveAttribute('title', 'Muted');
  await expect(side.getByRole('listitem').filter({ hasText: a.name }).getByRole('img', { name: 'muted' })).toBeVisible();
  await expect(tileA.getByRole('img', { name: 'micro coupé' })).toBeVisible();
  const deafen = bar(pa).getByRole('button', { name: 'Deafen', exact: true });
  await deafen.click();
  await expect(deafen).toHaveAttribute('aria-pressed', 'true');
  await expect(deafen).toHaveAttribute('title', 'Undeafen');
  await expect(mine.getByRole('img', { name: 'deafened' })).toHaveAttribute('title', 'Deafened');
  await expect(tileA.getByRole('img', { name: 'son coupé' })).toBeVisible();
  await noFrench(pa);
  await a11y(pa);
  await deafen.click();
  await expect(mine.getByRole('img', { name: 'deafened' })).toHaveCount(0);
  await mute.click();
  await expect(mine.getByRole('img', { name: 'muted' })).toHaveCount(0);

  // Les raccourcis annoncent l'état du micro et du son en anglais.
  await pa.keyboard.press('Control+Shift+M');
  await expect(mute).toHaveAttribute('aria-pressed', 'true');
  await expect(pa.getByText('Muted', { exact: true })).toHaveCount(1);
  await pa.keyboard.press('Control+Shift+M');
  await expect(pa.getByText('Unmuted', { exact: true })).toHaveCount(1);
  await pa.keyboard.press('Control+Shift+D');
  await expect(deafen).toHaveAttribute('aria-pressed', 'true');
  await expect(pa.getByText('Deafened, muted', { exact: true })).toHaveCount(1);
  await pa.keyboard.press('Control+Shift+D');
  await expect(pa.getByText('Undeafened', { exact: true })).toHaveCount(1);
  await expect(deafen).toHaveAttribute('aria-pressed', 'false');

  // Caméra : l'aperçu chez A, l'image chez B (en français), l'icône dans la barre latérale.
  const camera = bar(pa).getByRole('button', { name: 'Camera', exact: true });
  await expect(camera).toHaveAttribute('title', 'Turn on camera');
  await camera.click();
  await expect(camera).toHaveAttribute('aria-pressed', 'true');
  await expect(camera).toHaveAttribute('title', 'Turn off camera');
  await expect(pa.getByLabel('Preview of your camera')).toBeVisible();
  await expect(pb.getByLabel(`Caméra de ${a.name}`)).toBeVisible();
  await expect(side.getByRole('listitem').filter({ hasText: a.name }).getByRole('img', { name: 'camera on' })).toHaveAttribute('title', 'Camera on');
  await noFrench(pa);
  await camera.click();
  await expect(camera).toHaveAttribute('title', 'Turn on camera');
  await expect(pa.getByLabel('Preview of your camera')).toHaveCount(0);

  // Partage d'écran : « Live » dans la tuile et la barre latérale, l'aperçu, le bouton qui devient « Stop sharing ».
  await bar(pa).getByRole('button', { name: 'Share your screen', exact: true }).click();
  const stop = bar(pa).getByRole('button', { name: 'Stop sharing', exact: true });
  await expect(stop).toBeVisible({ timeout: 15_000 });
  await expect(pa.getByLabel('Preview of your shared screen')).toBeVisible();
  await expect(mine.getByText('Live', { exact: true })).toBeVisible();
  await expect(side.getByRole('listitem').filter({ hasText: a.name }).getByText('Live', { exact: true })).toBeVisible();
  await expect(tileA.getByText('En direct')).toBeVisible();
  await noFrench(pa);
  await a11y(pa);
  await stop.click();
  await expect(bar(pa).getByRole('button', { name: 'Share your screen', exact: true })).toBeVisible();

  // Quitter : retour à l'écran d'avant, B reste seul.
  await bar(pa).getByRole('button', { name: 'Leave', exact: true }).click();
  await expect(panel(pa)).toHaveCount(0);
  await expect(room.getByRole('button', { name: 'Join', exact: true })).toBeVisible();
  await expect(room.getByText('1 person in the channel')).toBeVisible();
  await pb.getByRole('group', { name: "Commandes de l'appel" }).getByRole('button', { name: 'Quitter', exact: true }).click();
  await expect(pa.getByRole('region', { name: 'Voice channel Général' }).getByText('Nobody here yet.')).toBeVisible();
});

test('I18N-V6 appuyer pour parler en appel, en anglais : l’indicateur, « Mic open », les annonces ; le nom de la touche suit la langue', async ({ signedIn, api }) => {
  test.setTimeout(120_000);
  const a = await signedIn('en-US');
  const p = a.page;
  const cove = await guildOf(await api(a.token), 'Cove');
  await p.evaluate(() => {
    localStorage.setItem('kraken.ptt', 'on');
    localStorage.setItem('kraken.ptt_key', 'Space');
  });
  await p.goto(`/channels/${cove.gid}/${cove.voice}`);
  await p.getByRole('button', { name: 'Join', exact: true }).click();
  await inCall(p);
  const indicator = bar(p).locator('.app-call-ptt');
  await expect(indicator.getByText('Push to talk:')).toBeVisible();
  await expect(indicator).toContainText('Space');
  await expect(indicator.getByText('Mic open')).toBeHidden();
  await noFrench(p);
  await a11y(p);
  await p.keyboard.down('Space');
  await expect(indicator).toHaveAttribute('data-on', '');
  await expect(indicator.getByText('Mic open')).toBeVisible();
  await expect(said(p, /^Mic open$/)).toBeVisible();
  await p.keyboard.up('Space');
  await expect(said(p, /^Mic closed$/)).toBeVisible();
  // Micro coupé : l'annonce le dit, au lieu de « Mic open ».
  await bar(p).getByRole('button', { name: 'Mute', exact: true }).click();
  await p.keyboard.down('Space');
  await expect(said(p, /^Muted$/)).toBeVisible();
  await p.keyboard.up('Space');
  await noFrench(p);
  await bar(p).getByRole('button', { name: 'Leave', exact: true }).click();
  await expect(panel(p)).toHaveCount(0);
});

// ───────── appels des MP et des groupes ─────────

test('I18N-V7 appel d’un groupe en anglais : sonnerie (« Ringing », « is calling you in »), refuser, rejoindre, l’appel en cours', async ({ signedIn, api }) => {
  test.setTimeout(180_000);
  const a = await signedIn('en-US');
  const b = await signedIn('en-US');
  const c = await signedIn('fr-FR');
  await befriend(api, a, b);
  await befriend(api, a, c);
  const res = await (await api(a.token)).post(`${API}/users/@me/channels`, { data: { recipients: [b.id, c.id], name: 'The trio' } });
  expect(res.status()).toBe(201);
  const group = (await res.json()).id as string;

  await a.page.goto(`/channels/@me/${group}`);
  await a.page.getByRole('button', { name: 'Start a call' }).click();
  await inCall(a.page);
  await expect(panel(a.page).getByRole('link', { name: /^The trio$|Call with The trio/ })).toBeVisible();
  const ringing = callArea(a.page).getByRole('status').filter({ hasText: 'Ringing' });
  await expect(ringing).toContainText(b.name);
  await expect(ringing).toContainText(c.name);
  await expect(ringing).toHaveText(/Ringing .+ and .+…$/);
  await noFrench(a.page);
  await a11y(a.page);

  // Chez B (anglais) et chez C (français) : la carte d'appel dit où.
  const ring = incoming(b.page, a.name);
  await expect(ring).toBeVisible();
  await expect(ring).toContainText('is calling you in The trio');
  await expect(ring.getByRole('button', { name: 'Join' })).toBeVisible();
  await expect(ring.getByRole('button', { name: 'Decline' })).toBeVisible();
  await expect(b.page.getByRole('navigation', { name: 'Direct messages' }).getByRole('img', { name: 'Call in progress' })).toBeVisible();
  await noFrench(b.page);
  await a11y(b.page);
  await expect(c.page.getByRole('region', { name: 'Appels entrants' }).getByRole('alertdialog', { name: `Appel de ${a.name}` })).toContainText("t'appelle dans The trio");

  // B refuse : plus de sonnerie chez lui, A n'attend plus que C.
  await ring.getByRole('button', { name: 'Decline' }).click();
  await expect(ring).toHaveCount(0);
  await expect(ringing).toHaveText(`Ringing ${c.name}…`);
  // C (en français) décroche.
  await c.page.getByRole('region', { name: 'Appels entrants' }).getByRole('button', { name: 'Rejoindre' }).click();
  await expect(c.page.getByRole('region', { name: 'Connexion vocale' }).getByRole('status')).toHaveText('En appel', { timeout: 20_000 });
  await expect(ringing).toHaveCount(0);

  // B ouvre le groupe : l'appel en cours, qui y est, « Join call ».
  await b.page.goto(`/channels/@me/${group}`);
  const lobby = callArea(b.page);
  await expect(lobby.getByText('Call in progress')).toBeVisible();
  await expect(lobby.getByRole('list', { name: 'In the call' }).getByRole('listitem')).toHaveCount(2);
  await expect(b.page.getByRole('button', { name: 'Start a call' })).toHaveCount(0);
  await noFrench(b.page);
  await a11y(b.page);
  await lobby.getByRole('button', { name: 'Join call' }).click();
  await inCall(b.page);
  for (const u of [a, b]) await expect(tiles(u.page)).toHaveCount(3);
  await expect(tiles(a.page).filter({ hasText: `${a.name} (you)` })).toBeVisible();
  await expect(tiles(b.page).filter({ hasText: `${b.name} (you)` })).toBeVisible();
  await expect(tiles(c.page).filter({ hasText: `${c.name} (toi)` })).toBeVisible();
  await expect(panel(a.page)).toContainText('The trio');
  await noFrench(b.page);
  await a11y(b.page);

  // Chacun raccroche : la barre dit « Hang up » dans un appel de MP ou de groupe, « Leave » dans un salon.
  await expect(bar(b.page).getByRole('button', { name: 'Hang up', exact: true })).toHaveAttribute('title', 'Hang up');
  await expect(panel(b.page).getByRole('button', { name: 'Hang up' })).toBeVisible();
  await c.page.getByRole('group', { name: "Commandes de l'appel" }).getByRole('button', { name: 'Raccrocher', exact: true }).click();
  await bar(b.page).getByRole('button', { name: 'Hang up', exact: true }).click();
  await bar(a.page).getByRole('button', { name: 'Hang up', exact: true }).click();
  for (const u of [a, b]) await expect(u.page.getByRole('button', { name: 'Start a call' })).toBeVisible();
});

test('I18N-V8 contrôle de l’écran en anglais : navigateur sans appli de bureau, demande annulée, refusée, acceptée, arrêtée', async ({ signedIn, api }) => {
  test.setTimeout(180_000);
  const a = await signedIn('en-US');
  const b = await signedIn('en-US');
  await befriend(api, a, b);
  const res = await (await api(a.token)).post(`${API}/users/@me/channels`, { data: { recipients: [b.id] } });
  expect(res.status()).toBe(201);
  const dm = (await res.json()).id as string;

  // A appelle B dans leur MP ; B décroche.
  await a.page.goto(`/channels/@me/${dm}`);
  await a.page.getByRole('button', { name: 'Start a call' }).click();
  await inCall(a.page);
  await expect(panel(a.page)).toContainText(`Call with ${b.name}`);
  await expect(callArea(a.page).getByRole('status').filter({ hasText: `Ringing ${b.name}…` })).toBeVisible();
  const ring = incoming(b.page, a.name);
  await expect(ring).toContainText('is calling you');
  await expect(ring).not.toContainText('is calling you in');
  await ring.getByRole('button', { name: 'Join' }).click();
  await b.page.waitForURL(`**/channels/@me/${dm}`);
  await inCall(b.page);
  await expect(callArea(a.page).getByText('Ringing')).toHaveCount(0);

  // A partage son écran ; B le regarde : l'écran en grand et ses commandes.
  await bar(a.page).getByRole('button', { name: 'Share your screen', exact: true }).click();
  await expect(bar(a.page).getByRole('button', { name: 'Stop sharing', exact: true })).toBeVisible({ timeout: 15_000 });
  const tileA = tiles(b.page).filter({ hasText: a.name });
  await expect(tileA.getByText('Live', { exact: true })).toBeVisible();
  await tileA.getByRole('button', { name: `Watch ${a.name}'s screen` }).click();
  await expect(tileA.getByRole('button', { name: 'Stop watching' })).toHaveAttribute('aria-pressed', 'true');
  const stage = b.page.locator('figure.app-stage');
  await expect(stage.getByLabel(`Screen shared by ${a.name}`)).toBeVisible();
  await expect(stage.getByText('Live', { exact: true })).toBeVisible();
  await expect(stage.getByRole('button', { name: 'Full screen' })).toBeVisible();
  await expect(stage.getByRole('button', { name: 'Stop watching' })).toBeVisible();
  const ask = stage.getByRole('button', { name: `Request control of ${a.name}'s screen` });
  await expect(ask).toContainText('Request control');
  await noFrench(b.page);
  await a11y(b.page);

  // Depuis un navigateur, l'écran ne peut pas être contrôlé : B et A le lisent en anglais.
  await ask.click();
  const refused = said(b.page, `${a.name} is sharing from a browser: to be controlled, they need to share from the KRAKEN desktop app.`);
  await expect(refused).toBeVisible();
  await expect(remote(a.page).getByText(`${b.name} asked to control your screen. Remote control only works from the KRAKEN desktop app: share your screen from the app to accept.`)).toBeVisible();
  await expect(b.page.getByRole('button', { name: 'Hide this message' })).toHaveAttribute('title', 'Hide');
  await noFrench(a.page);
  await noFrench(b.page);
  await a11y(a.page);
  await b.page.getByRole('button', { name: 'Hide this message' }).click();
  await expect(refused).toHaveCount(0);
  await remote(a.page).getByRole('button', { name: 'Hide this message' }).click();
  await expect(remote(a.page)).toHaveCount(0);

  // L'appli de bureau (simulée) répond : la demande est posée, annulée, posée, refusée, posée, acceptée.
  await a.page.evaluate(() => {
    const w = window as any;
    w.__calls = [];
    w.__TAURI_INTERNALS__ = {
      invoke: (cmd: string, args: unknown) => {
        w.__calls.push({ cmd, args });
        return Promise.resolve(cmd === 'remote_begin' ? `jeton-${w.__calls.length}` : null);
      },
    };
  });
  await ask.click();
  await expect(stage.getByText('Request sent…')).toBeVisible();
  const asking = remote(a.page).getByRole('alert').filter({ hasText: `${b.name} is asking to control your screen (mouse and keyboard).` });
  await expect(asking).toBeVisible();
  await expect(asking.getByRole('button', { name: 'Accept' })).toBeVisible();
  await expect(asking.getByRole('button', { name: 'Decline' })).toBeVisible();
  await noFrench(a.page);
  await noFrench(b.page);
  await a11y(a.page);
  await stage.getByRole('button', { name: 'Cancel request' }).click();
  await expect(asking).toHaveCount(0);
  await expect(ask).toBeVisible();
  await ask.click();
  await asking.getByRole('button', { name: 'Decline' }).click();
  await expect(said(b.page, `${a.name} declined your control request.`)).toBeVisible();
  await b.page.getByRole('button', { name: 'Hide this message' }).click();
  await b.page.waitForTimeout(5_200); // délai avant une nouvelle demande, après un refus
  await ask.click();
  await asking.getByRole('button', { name: 'Accept' }).click();
  await expect(said(b.page, `You're controlling ${a.name}'s screen. Press Esc to release (Shift+Esc sends Esc).`)).toBeVisible();
  await expect(stage.getByRole('button', { name: 'Stop control' })).toBeVisible();
  await expect(remote(a.page).getByText(`${b.name} is controlling your screen.`)).toBeVisible();
  await noFrench(a.page);
  await noFrench(b.page);
  await a11y(b.page);

  // A reprend la main : B l'apprend.
  await remote(a.page).getByRole('button', { name: 'Stop control' }).click();
  await expect(said(b.page, `Control of ${a.name}'s screen has ended.`)).toBeVisible();
  await expect(remote(a.page)).toHaveCount(0);
  await noFrench(b.page);

  // B arrête de regarder, puis tout le monde raccroche.
  await stage.getByRole('button', { name: 'Stop watching' }).click();
  await expect(stage).toHaveCount(0);
  await bar(b.page).getByRole('button', { name: 'Hang up', exact: true }).click();
  await voicePanelHangUp(a.page);
  for (const u of [a, b]) await expect(u.page.getByRole('button', { name: 'Start a call' })).toBeVisible();
});

/** Raccroche depuis le panneau de la barre latérale. */
const voicePanelHangUp = (p: Page) => panel(p).getByRole('button', { name: 'Hang up' }).click();

// ───────── 390 px ─────────

test.describe('à 390 px', () => {
  test('I18N-V9 Voix et vidéo, sélecteur rapide, aide des raccourcis et barre d’appel en anglais : rien ne déborde, axe', async ({ signedIn, api }) => {
    test.setTimeout(150_000);
    const a = await signedIn('en-US', { width: 390, height: 844 });
    const p = a.page;
    const cove = await guildOf(await api(a.token), 'Cove');
    await p.addInitScript(HIDE_LABELS);

    // Réglages › Voix et vidéo : les listes, le test, l'aperçu et la touche tiennent dans l'écran.
    await p.goto('/settings/voice');
    await expect(settings(p).getByRole('heading', { name: 'Voice and video', level: 2 })).toBeVisible();
    await settings(p).getByRole('switch', { name: 'Push to talk' }).check();
    await p.getByRole('button', { name: 'Camera preview' }).click();
    await expect(p.getByLabel('Your camera image')).toBeVisible();
    await p.getByRole('button', { name: 'Test microphone' }).click();
    await pttKey(p).click();
    await p.keyboard.press('Tab');
    await expect(p.getByRole('alert').filter({ hasText: 'The Tab key is used to navigate the page' })).toBeVisible();
    await noScroll(p);
    for (const name of ['Audio input', 'Audio output', 'Camera']) {
      const box = await p.getByLabel(name, { exact: true }).boundingBox();
      expect(box!.x).toBeGreaterThanOrEqual(0);
      expect(box!.x + box!.width).toBeLessThanOrEqual(390);
    }
    await noFrench(p);
    await a11y(p);

    // Le sélecteur rapide : le bouton de la barre latérale, une feuille pleine largeur.
    await p.goto(`/channels/${cove.gid}/${cove.text}`);
    await p.getByRole('button', { name: 'Open navigation' }).click();
    await p.getByRole('button', { name: 'Go to…' }).click();
    await expect(switcher(p)).toBeVisible();
    await field(p).fill('#');
    await expect(switcher(p).getByRole('option').first()).toBeVisible();
    await expect(switcher(p).getByRole('group', { name: 'Channels' })).toBeVisible();
    await noScroll(p);
    await noFrench(p);
    await a11y(p);
    await field(p).fill('zzzzqq');
    await expect(switcher(p).getByText('No results for "zzzzqq"')).toBeVisible();
    await noScroll(p);
    await p.keyboard.press('Escape');

    // L'aide des raccourcis.
    await p.keyboard.press('Control+/');
    await expect(helpDialog(p)).toBeVisible();
    await expect(helpDialog(p).getByRole('button', { name: 'Close' })).toBeInViewport();
    await noScroll(p);
    await noFrench(p);
    await a11y(p);
    await p.keyboard.press('Escape');

    // En appel : la barre et son indicateur tiennent sur la largeur de l'écran.
    await p.evaluate(() => {
      localStorage.setItem('kraken.ptt', 'on');
      localStorage.setItem('kraken.ptt_key', 'Space');
    });
    await p.goto(`/channels/${cove.gid}/${cove.voice}`);
    await expect(p.getByRole('heading', { name: 'Général', level: 1 })).toBeVisible();
    await noScroll(p);
    await noFrench(p);
    await a11y(p);
    await p.getByRole('button', { name: 'Join', exact: true }).click();
    await expect(bar(p).getByRole('button', { name: 'Leave', exact: true })).toBeVisible();
    await expect(bar(p).locator('.app-call-ptt')).toBeVisible();
    for (const button of await bar(p).getByRole('button').all()) {
      const box = (await button.boundingBox())!;
      expect(box.x).toBeGreaterThanOrEqual(0);
      expect(box.x + box.width).toBeLessThanOrEqual(390);
    }
    await noScroll(p);
    await noFrench(p);
    await a11y(p);
    // Le panneau « In call » vit dans la barre latérale, qui s'ouvre en tiroir à cette largeur.
    await p.getByRole('button', { name: 'Open navigation' }).click();
    await inCall(p);
    await expect(panel(p).getByRole('button', { name: 'Leave voice channel' })).toBeVisible();
    await noScroll(p);
    await noFrench(p);
    await a11y(p);
    await panel(p).getByRole('button', { name: 'Leave voice channel' }).click();
    await expect(panel(p)).toHaveCount(0);
  });
});
