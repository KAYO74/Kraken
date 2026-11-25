// F04 Vocal natif de l'appli de bureau (Linux, macOS, Windows) : quand le navigateur intégré ne peut pas faire l'appel
// (pas de WebRTC : WebKitGTK de Linux ; pas de micro possible : WKWebView de macOS sur une instance en http), l'appli de
// bureau le fait avec son moteur Rust et la page le pilote par les commandes `native_voice_*`. Quand il le peut (WebRTC,
// contexte sécurisé, `navigator.mediaDevices`), le chemin du navigateur reste celui d'avant. La règle est une seule
// fonction du client, `voice::browser_can_call`. Sa constante `ALWAYS_NATIVE` (« partout natif » : le moteur sert dans
// toute l'appli de bureau où il répond) est à `false` : l'autre valeur n'est pas testée ici (elle changerait V13, qui
// attend le navigateur quand tout y est).
//
// Un « faux bureau » : `window.__TAURI_INTERNALS__.invoke` simulé, et le navigateur privé de ce qui lui manque : par défaut
// `RTCPeerConnection` supprimé (comme `voice-unsupported.spec.ts`), ou (V11, V12) WebRTC présent mais ni contexte sécurisé ni
// `navigator.mediaDevices`, ou (V13) un navigateur complet.
//  (a) `invoke` répond en JavaScript, sans moteur : états, « parle », micro coupé, quitter, ce qui manque, erreurs ;
//  (b) `invoke` est routé vers le vrai pont (`target/release/examples/bridge`, le moteur synthétique en HTTP) :
//      un vrai Chromium A et le faux bureau B entrent dans le même salon, à travers le vrai SFU, et le puits du
//      pont dit si B a entendu A.
// La page ne peut pas appeler le pont par `fetch` (sa CSP est `connect-src 'self'`) : `invoke` passe par une fonction
// exposée de Playwright, comme la liaison d'IPC de Tauri passe hors de la CSP.
//
// Le pont se compile avec : cargo build --release --example bridge -p kraken-voice-native --no-default-features
// (`BRIDGE_BIN` donne un autre emplacement ; sinon `$CARGO_TARGET_DIR` ou `target/` à la racine du dépôt).
import { spawn, type ChildProcess } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { test, expect, a11y, API, type User } from '../fixtures';
import type { APIRequestContext, Page, WebSocketRoute } from '@playwright/test';

// Micro factice de Chromium, sans fenêtre d'autorisation : pour A dans la partie (b).
test.use({ launchOptions: { args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'] } });

type Api = (token?: string) => Promise<APIRequestContext>;
type Win = Record<string, any>; // eslint-disable-line @typescript-eslint/no-explicit-any

const NO_VIDEO = "Pas de caméra ni de partage d'écran dans l'appli de bureau pour l'instant : ouvre cette page dans ton navigateur.";
const tiles = (page: Page) => page.getByRole('list', { name: 'Participants' }).getByRole('listitem');
const bar = (page: Page) => page.getByRole('group', { name: "Commandes de l'appel" });
const panel = (page: Page) => page.getByRole('region', { name: 'Connexion vocale' });
const open = (page: Page) => page.getByRole('button', { name: 'Ouvrir dans mon navigateur' });

/** Un serveur neuf dont `b` est membre : l'identifiant du serveur, du salon textuel et du salon vocal. */
async function guild(api: Api, owner: User, member: User) {
  const asOwner = await api(owner.token);
  const created = await asOwner.post(`${API}/guilds`, { data: { name: 'Vocal natif' } });
  expect(created.status()).toBe(201);
  const g = await created.json();
  const text = g.channels.find((c: { kind: string }) => c.kind === 'text').id;
  const voice = g.channels.find((c: { kind: string }) => c.kind === 'voice').id;
  const invite = await (await asOwner.post(`${API}/channels/${text}/invites`, { data: {} })).json();
  expect((await (await api(member.token)).post(`${API}/invites/${invite.code}`)).status()).toBe(200);
  return { gid: g.guild.id as string, text: text as string, voice: voice as string };
}

/** Ouvre le salon vocal (avant d'entrer). */
async function lobby(u: User, g: { gid: string; text: string }) {
  await u.page.goto(`/channels/${g.gid}/${g.text}`);
  await u.page.getByRole('link', { name: 'salon vocal Général', exact: true }).click();
  await expect(u.page.getByRole('heading', { name: 'Général', level: 1 })).toBeVisible();
}

// ───── le faux bureau ─────

/** L'état du faux moteur de la partie (a), lu et réglé par le test. */
type Mock = {
  calls: { cmd: string; args: Record<string, any> }[]; // eslint-disable-line @typescript-eslint/no-explicit-any
  snapshot: Record<string, unknown>;
  available: boolean;
  startError: string | null;
  noMic: boolean;
  polling: boolean;
  devices: { inputs: unknown[]; outputs: unknown[] };
};

/** Ce que le navigateur intégré du faux bureau sait faire : rien (Linux : ni WebRTC), WebRTC sans micro possible (macOS, instance en http), ou tout. */
type Webview = 'no-webrtc' | 'insecure' | 'full';

/**
 * Le faux bureau de la partie (a) : le navigateur intégré selon `webview` (par défaut sans WebRTC), `invoke` répond comme le moteur
 * (voir `__native`). Chaque appel est noté. `local` : des valeurs de `localStorage` posées avant le chargement de la page ; `start` :
 * l'état du faux moteur dès le chargement (ce que `native_voice_available` répond au démarrage ne se change plus après).
 */
async function fakeEngine(u: User, local: Record<string, string> = {}, start: Partial<Mock> = {}, webview: Webview = 'no-webrtc') {
  await u.context.addInitScript(({ keep, start, webview }) => {
    const w = window as unknown as Win;
    if (webview === 'no-webrtc') {
      delete w.RTCPeerConnection;
      delete w.webkitRTCPeerConnection;
    } else if (webview === 'insecure') {
      // WKWebView sur une instance en http hors de cet ordinateur : WebRTC existe, mais ni contexte sécurisé ni `mediaDevices`.
      Object.defineProperty(window, 'isSecureContext', { value: false, configurable: true });
      Object.defineProperty(navigator, 'mediaDevices', { value: undefined, configurable: true });
    }
    for (const [k, v] of Object.entries(keep)) localStorage.setItem(k, v);
    const native: Mock = {
      calls: [],
      snapshot: {
        state: { kind: 'connecting' },
        participants: [],
        speaking: [],
        self_speaking: false,
        self_mute: false,
        self_deaf: false,
        mic_level: 0,
        output_level: 0,
        audio_error: null,
        packets_sent: 0,
        packets_received: 0,
      },
      available: true,
      startError: null,
      noMic: false,
      polling: true,
      devices: {
        inputs: [
          { name: 'Micro du casque', id: 'alsa:casque', is_default: true },
          { name: 'Micro de la webcam', id: 'alsa:webcam', is_default: false },
        ],
        outputs: [
          { name: 'Haut-parleurs', id: 'alsa:hp', is_default: true },
          { name: 'Casque USB', id: 'alsa:usb', is_default: false },
        ],
      },
    };
    Object.assign(native, start);
    w.__native = native;
    w.__TAURI_INTERNALS__ = {
      invoke: async (cmd: string, args: Record<string, any> = {}) => { // eslint-disable-line @typescript-eslint/no-explicit-any
        native.calls.push({ cmd, args: JSON.parse(JSON.stringify(args)) });
        switch (cmd) {
          case 'native_voice_available':
            return native.available;
          case 'native_voice_start':
            if (native.startError) throw native.startError;
            return {
              sdp: 'v=0',
              mids: { mic: '0', screen: '1', screen_audio: '2', camera: '3' },
              self_mute: native.noMic || !!args.opts?.self_mute,
              self_deaf: !!args.opts?.self_deaf,
            };
          case 'native_voice_poll':
            return native.polling ? native.snapshot : null;
          case 'native_voice_devices':
            return native.devices;
          default:
            return null;
        }
      },
    };
  }, { keep: local, start, webview });
  await u.page.reload();
}

const calls = (page: Page, cmd: string) =>
  page.evaluate((c) => (window as unknown as Win).__native.calls.filter((x: { cmd: string }) => x.cmd === c), cmd) as Promise<
    { cmd: string; args: Record<string, any> }[] // eslint-disable-line @typescript-eslint/no-explicit-any
  >;
/** Les `patch` envoyés à `native_voice_set`, le dernier en dernier. */
const patches = async (page: Page) => (await calls(page, 'native_voice_set')).map((c) => c.args.patch as Record<string, unknown>);
/** Règle ce que le faux moteur répond au prochain relevé. */
const engine = (page: Page, change: Partial<Mock> & { snapshot?: Record<string, unknown> }) =>
  page.evaluate((c) => {
    const n = (window as unknown as Win).__native as Mock;
    const { snapshot, ...rest } = c;
    Object.assign(n, rest);
    if (snapshot) Object.assign(n.snapshot, snapshot);
  }, change);

/** Ce que la page sait du salon : le serveur de test ne connaît pas le faux moteur, les événements vocaux sont injectés par le gateway. */
async function gateway(page: Page) {
  let socket: WebSocketRoute | undefined;
  let seq = 900_000;
  await page.routeWebSocket(/\/gateway/, (ws) => {
    socket = ws;
    const server = ws.connectToServer();
    server.onMessage((m) => ws.send(m));
    ws.onMessage((m) => server.send(m));
  });
  return {
    /** Un état vocal arrive, comme si le serveur l'envoyait. */
    voiceState: async (state: object) => {
      await expect.poll(() => socket !== undefined).toBe(true);
      socket!.send(JSON.stringify({ op: 'DISPATCH', d: { seq: seq++, event: { t: 'VOICE_STATE_UPDATE', d: state } } }));
    },
  };
}

/** Partie (a) : le serveur de test ne reçoit pas l'offre factice, la page reçoit une réponse. */
async function fakeServerAnswer(page: Page) {
  await page.route(`**${API}/channels/*/voice`, (route) =>
    route.request().method() === 'POST'
      ? route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ sdp: 'v=0' }) })
      : route.continue(),
  );
}

/** Ouvre les réglages Voix et vidéo sans recharger la page (l'appel continue). */
async function settings(page: Page) {
  await page.evaluate(() => {
    const a = document.createElement('a');
    a.href = '/settings/voice';
    document.body.append(a);
    a.click();
    a.remove();
  });
  await expect(page.getByRole('heading', { name: 'Voix et vidéo', level: 2 })).toBeVisible();
}

const join = (u: User) => u.page.getByRole('button', { name: 'Rejoindre', exact: true }).click();

// ───── (a) le moteur simulé ─────

test('F04-V1 navigateur sans WebRTC mais moteur natif : « Rejoindre » marche, la voix seule est dite en toutes lettres', async ({
  user,
  api,
}) => {
  const a = await user('maitre');
  const b = await user('natif');
  const g = await guild(api, a, b);
  await fakeEngine(b);
  await fakeServerAnswer(b.page);
  await lobby(b, g);
  const rejoindre = b.page.getByRole('button', { name: 'Rejoindre', exact: true });
  await expect(rejoindre).toBeEnabled();
  // Plus de message « navigateur sans WebRTC », ni de détails : le vocal natif s'en charge.
  await expect(b.page.getByText('livré sans WebRTC', { exact: false })).toHaveCount(0);
  await expect(b.page.getByText('WebRTC y est absent ou désactivé', { exact: false })).toHaveCount(0);
  await expect(b.page.getByText(/Détails : page en/)).toHaveCount(0);
  // Pas de message « connexion non sécurisée » : le navigateur intégré n'a pas non plus de micro à demander.
  await expect(b.page.getByText('Connexion non sécurisée', { exact: false })).toHaveCount(0);
  // Ce qui n'existe pas, avant même d'entrer : une phrase visible et le moyen d'y arriver.
  await expect(b.page.getByText(NO_VIDEO)).toBeVisible();
  await expect(open(b.page)).toBeVisible();
  await a11y(b.page);

  // Le moteur n'est interrogé qu'une fois pour dire s'il est là, et rien ne démarre avant « Rejoindre ».
  expect((await calls(b.page, 'native_voice_available')).length).toBe(1);
  expect((await calls(b.page, 'native_voice_start')).length).toBe(0);
});

test('F04-V2 états de la connexion, qui parle, micro coupé, son coupé, quitter', async ({ user, api }) => {
  test.setTimeout(90_000);
  const a = await user('hote');
  const b = await user('bureau');
  const g = await guild(api, a, b);
  await fakeEngine(b);
  await fakeServerAnswer(b.page);
  const gw = await gateway(b.page);
  await lobby(b, g);
  await gw.voiceState({ guild_id: g.gid, channel_id: g.voice, user_id: b.id, self_mute: false, self_deaf: false, streaming: false, camera: false });
  await gw.voiceState({ guild_id: g.gid, channel_id: g.voice, user_id: a.id, self_mute: false, self_deaf: false, streaming: false, camera: false });
  const states = [`Connexion…`, 'En appel'];

  // Connexion : le moteur démarre avec les réglages de la page, l'offre part au serveur, la réponse revient au moteur.
  await join(b);
  await expect(b.page.getByRole('status').filter({ hasText: 'Connexion au salon vocal…' })).toBeVisible();
  await expect(panel(b.page).getByRole('status')).toHaveText(states[0]);
  await expect.poll(async () => (await calls(b.page, 'native_voice_answer')).length).toBe(1);
  const [start] = await calls(b.page, 'native_voice_start');
  expect(start.args.opts).toEqual({
    mic_device: null,
    speaker_device: null,
    mic_gain: 1,
    output_volume: 1,
    self_mute: false,
    self_deaf: false,
  });
  expect((await calls(b.page, 'native_voice_answer'))[0].args).toEqual({ sdp: 'v=0' });

  // Connecté.
  await engine(b.page, { snapshot: { state: { kind: 'connected' } } });
  await expect(panel(b.page).getByRole('status')).toHaveText(states[1]);
  await expect(b.page.getByText('Connexion au salon vocal…')).toHaveCount(0);
  await expect(tiles(b.page)).toHaveCount(2);
  await a11y(b.page);

  // Qui parle, d'après le relevé : l'autre, puis moi.
  const other = tiles(b.page).filter({ hasText: a.name });
  const mine = tiles(b.page).filter({ hasText: `${b.name} (toi)` });
  await expect(other).not.toHaveAttribute('data-speaking', '');
  await engine(b.page, { snapshot: { speaking: [a.id] } });
  await expect(other).toHaveAttribute('data-speaking', '');
  await expect(mine).not.toHaveAttribute('data-speaking', '');
  await engine(b.page, { snapshot: { speaking: [], self_speaking: true } });
  await expect(mine).toHaveAttribute('data-speaking', '');
  await expect(other).not.toHaveAttribute('data-speaking', '');
  await engine(b.page, { snapshot: { self_speaking: false } });
  await expect(mine).not.toHaveAttribute('data-speaking', '');

  // Micro coupé, puis son coupé : chaque changement part par `native_voice_set`, avec les deux états.
  const mic = bar(b.page).getByRole('button', { name: 'Couper le micro' });
  await mic.click();
  await expect(mic).toHaveAttribute('aria-pressed', 'true');
  await expect.poll(async () => (await patches(b.page)).at(-1)?.self_mute).toBe(true);
  expect((await patches(b.page)).at(-1)).toMatchObject({ self_mute: true, self_deaf: false, mic_gain: 1, output_volume: 1 });
  const deaf = bar(b.page).getByRole('button', { name: 'Couper le son' });
  await deaf.click();
  await expect(deaf).toHaveAttribute('aria-pressed', 'true');
  await expect.poll(async () => (await patches(b.page)).at(-1)?.self_deaf).toBe(true);
  expect((await patches(b.page)).at(-1)).toMatchObject({ self_mute: true, self_deaf: true });
  await deaf.click();
  await expect.poll(async () => (await patches(b.page)).at(-1)?.self_deaf).toBe(false);
  // Le son revenu, le micro, lui, reste coupé : c'est mon choix.
  expect((await patches(b.page)).at(-1)).toMatchObject({ self_mute: true, self_deaf: false });
  await mic.click();
  await expect(mic).toHaveAttribute('aria-pressed', 'false');
  await expect.poll(async () => (await patches(b.page)).at(-1)?.self_mute).toBe(false);

  // Perdu : le motif du moteur n'est pas montré tel quel, « Reconnecter » relance le même salon.
  const before = (await calls(b.page, 'native_voice_start')).length;
  await engine(b.page, { snapshot: { state: { kind: 'lost', reason: 'connexion perdue' } } });
  const lost = b.page.getByRole('alert').filter({ hasText: 'Connexion vocale perdue.' });
  await expect(lost).toBeVisible();
  await expect(panel(b.page).getByRole('status')).toHaveText('Connexion perdue');
  await expect.poll(async () => (await calls(b.page, 'native_voice_stop')).length).toBe(1);
  await engine(b.page, { snapshot: { state: { kind: 'connecting' } } });
  await lost.getByRole('button', { name: 'Reconnecter' }).click();
  await expect(b.page.getByRole('status').filter({ hasText: 'Connexion au salon vocal…' })).toBeVisible();
  await expect.poll(async () => (await calls(b.page, 'native_voice_start')).length).toBe(before + 1);
  // Le nouvel appel part avec l'état de la page : le micro est ouvert, le son aussi.
  expect((await calls(b.page, 'native_voice_start')).at(-1)!.args.opts).toMatchObject({ self_mute: false, self_deaf: false });
  await engine(b.page, { snapshot: { state: { kind: 'connected' } } });
  await expect(panel(b.page).getByRole('status')).toHaveText('En appel');

  // Quitter : le moteur s'arrête, et le serveur est prévenu.
  const left = b.page.waitForRequest((r) => r.method() === 'DELETE' && r.url().endsWith(`${API}/voice`));
  await bar(b.page).getByRole('button', { name: 'Quitter', exact: true }).click();
  await left;
  await expect.poll(async () => (await calls(b.page, 'native_voice_stop')).length).toBe(2);
  await expect(panel(b.page)).toHaveCount(0);
  await expect(b.page.getByRole('button', { name: 'Rejoindre', exact: true })).toBeVisible();
  // Plus aucun relevé une fois parti.
  const polls = (await calls(b.page, 'native_voice_poll')).length;
  await b.page.waitForTimeout(600);
  expect((await calls(b.page, 'native_voice_poll')).length).toBeLessThanOrEqual(polls + 1);
});

test('F04-V3 caméra et partage d’écran grisés avec leur phrase, l’image des autres ne s’affiche pas, « Ouvrir dans mon navigateur »', async ({
  user,
  api,
}) => {
  const a = await user('partage');
  const b = await user('lecteur');
  const g = await guild(api, a, b);
  await fakeEngine(b);
  await fakeServerAnswer(b.page);
  const gw = await gateway(b.page);
  await lobby(b, g);
  const here = (u: User, extra: object = {}) =>
    gw.voiceState({ guild_id: g.gid, channel_id: g.voice, user_id: u.id, self_mute: false, self_deaf: false, streaming: false, camera: false, ...extra });
  await here(b);
  await here(a);
  await join(b);
  await engine(b.page, { snapshot: { state: { kind: 'connected' } } });
  await expect(panel(b.page).getByRole('status')).toHaveText('En appel');

  // Les deux boutons sont grisés, et la phrase qui dit pourquoi est visible (pas seulement une infobulle).
  const camera = bar(b.page).getByRole('button', { name: 'Caméra' });
  const share = bar(b.page).getByRole('button', { name: 'Partager ton écran' });
  await expect(camera).toBeDisabled();
  await expect(share).toBeDisabled();
  const why = b.page.getByText(NO_VIDEO);
  await expect(why).toBeVisible();
  await expect(camera).toHaveAttribute('aria-describedby', 'call-native-why');
  await expect(share).toHaveAttribute('aria-describedby', 'call-native-why');
  await expect(b.page.locator('#call-native-why')).toHaveText(NO_VIDEO);
  await expect(camera).toHaveAttribute('title', NO_VIDEO);
  // Le message habituel du navigateur (« Ce navigateur ne permet pas de partager l'écran ») n'a pas sa place ici.
  await expect(b.page.getByText('Ce navigateur ne permet pas de partager', { exact: false })).toHaveCount(0);
  // Rien ne bouge de la barre quand la phrase est là : elle est sous les boutons.
  const box = await bar(b.page).boundingBox();
  const note = await why.boundingBox();
  expect(note!.y).toBeGreaterThanOrEqual(box!.y + box!.height - 1);
  await a11y(b.page);

  // « Ouvrir dans mon navigateur » appelle la commande de l'appli.
  await open(b.page).click();
  await expect.poll(async () => (await calls(b.page, 'open_in_browser')).length).toBe(1);

  // Quelqu'un partage son écran, puis allume sa caméra : une phrase dit que l'image ne s'affiche pas ici, et pas de « Regarder ».
  const unseen = b.page.getByRole('status').filter({ hasText: "l'image ne s'affiche pas dans l'appli de bureau" });
  await expect(unseen).toHaveCount(0);
  await here(a, { streaming: true });
  await expect(unseen).toContainText(`Écran partagé ou caméra allumée chez ${a.name}`);
  await expect(unseen).toContainText('Ouvre cette page dans ton navigateur pour la voir.');
  await expect(tiles(b.page).filter({ hasText: a.name }).getByText('En direct')).toBeVisible();
  await expect(tiles(b.page).getByRole('button', { name: /Regarder/ })).toHaveCount(0);
  await here(a, { streaming: false, camera: true });
  await expect(unseen).toContainText(a.name);
  await here(a);
  await expect(unseen).toHaveCount(0);
  // Ma propre caméra ou mon propre écran ne déclenchent pas cette phrase : seuls les autres comptent.
  await here(b, { camera: true });
  await expect(unseen).toHaveCount(0);

  // Rien de caméra ni d'écran n'est demandé au navigateur : aucun appel de partage n'est parti.
  expect((await calls(b.page, 'native_voice_start')).length).toBe(1);
});

test('F04-V4 micro introuvable : entré micro coupé, avis visible, le rouvrir demande un nouvel appel ; panne audio en cours d’appel', async ({
  user,
  api,
}) => {
  const a = await user('hote');
  const b = await user('sansmicro');
  const g = await guild(api, a, b);
  await fakeEngine(b);
  await fakeServerAnswer(b.page);
  await lobby(b, g);
  await engine(b.page, {
    noMic: true,
    snapshot: { state: { kind: 'connected' }, audio_error: 'Micro indisponible : tu entres micro coupé.' },
  });
  await join(b);
  const mic = bar(b.page).getByRole('button', { name: 'Couper le micro' });
  await expect(mic).toBeVisible();
  // Le moteur est entré micro coupé : la page le montre, et le dit.
  await expect(mic).toHaveAttribute('aria-pressed', 'true');
  const notice = b.page.getByRole('status').filter({ hasText: 'Micro indisponible : tu entres micro coupé.' });
  await expect(notice).toBeVisible();
  expect((await calls(b.page, 'native_voice_start'))[0].args.opts.self_mute).toBe(false);
  // L'offre part avec ce que le moteur a rendu : micro coupé.
  await expect.poll(async () => (await patches(b.page)).at(-1)?.self_mute).toBe(true);
  // Le rouvrir : le moteur ne change pas de micro à chaud, la page le dit et reste coupée.
  await mic.click();
  await expect(mic).toHaveAttribute('aria-pressed', 'true');
  await expect(b.page.getByRole('status').filter({ hasText: "Aucun micro n'est utilisable" })).toContainText(
    'quitte le salon et rejoins-le',
  );
  expect((await patches(b.page)).every((p) => p.self_mute === true)).toBe(true);

  // Une panne qui arrive en cours d'appel : une phrase générale, pas le texte du pilote.
  await engine(b.page, { snapshot: { audio_error: 'ALSA lib pcm.c:8740:(snd_pcm_recover) underrun occurred' } });
  const failed = b.page.getByRole('status').filter({ hasText: "L'audio ne répond plus" });
  await expect(failed).toBeVisible();
  await expect(b.page.getByText('ALSA lib', { exact: false })).toHaveCount(0);
  await a11y(b.page);
});

test('F04-V5 le moteur refuse de démarrer : sa phrase française s’affiche, le bouton pour ouvrir le navigateur est là, rien ne reste en route', async ({
  user,
  api,
}) => {
  const a = await user('hote');
  const b = await user('refuse');
  const g = await guild(api, a, b);
  await fakeEngine(b);
  await fakeServerAnswer(b.page);
  await lobby(b, g);
  await engine(b.page, { startError: 'Le micro ou les haut-parleurs sont inutilisables.' });
  await join(b);
  await expect(b.page.getByRole('alert').filter({ hasText: 'Le micro ou les haut-parleurs sont inutilisables.' })).toBeVisible();
  await expect(b.page.getByRole('button', { name: 'Rejoindre', exact: true })).toBeEnabled();
  await expect(open(b.page)).toBeVisible();
  await expect(panel(b.page)).toHaveCount(0);
  // Aucune offre n'est partie au serveur, et le moteur est arrêté (rien ne reste ouvert).
  expect((await calls(b.page, 'native_voice_answer')).length).toBe(0);
  await expect.poll(async () => (await calls(b.page, 'native_voice_stop')).length).toBe(1);
  await a11y(b.page);
  // Une autre tentative réussit : l'erreur disparaît.
  await engine(b.page, { startError: null, snapshot: { state: { kind: 'connected' } } });
  await join(b);
  await expect(panel(b.page).getByRole('status')).toHaveText('En appel');
  await expect(b.page.getByRole('alert').filter({ hasText: 'inutilisables' })).toHaveCount(0);
});

test('F04-V6 le moteur répond « pas disponible » : le message d’avant s’affiche, comme sans moteur', async ({ user, api }) => {
  const a = await user('hote');
  const b = await user('rien');
  const g = await guild(api, a, b);
  await fakeEngine(b, {}, { available: false });
  await lobby(b, g);
  await expect(b.page.getByRole('button', { name: 'Rejoindre', exact: true })).toBeDisabled();
  await expect(b.page.getByText('livré sans WebRTC', { exact: false })).toBeVisible();
  await expect(open(b.page)).toBeVisible();
  await expect(b.page.getByText(NO_VIDEO)).toHaveCount(0);
});

test('F04-V7 appuyer pour parler, gain et volume : tout passe par native_voice_set', async ({ user, api }) => {
  test.setTimeout(90_000);
  const a = await user('hote');
  const b = await user('ptt');
  const g = await guild(api, a, b);
  // Appuyer pour parler armé (touche P), gain à 2 et volume à 0,5 : choisis avant la page.
  await fakeEngine(b, {
    'kraken.ptt': 'on',
    'kraken.ptt_key': 'KeyP',
    'kraken.ptt_label': 'P',
    'kraken.native.gain': '2',
    'kraken.native.volume': '0.5',
  });
  await fakeServerAnswer(b.page);
  await lobby(b, g);
  await join(b);
  await engine(b.page, { snapshot: { state: { kind: 'connected' } } });
  await expect(panel(b.page).getByRole('status')).toHaveText('En appel');
  // Au départ, la touche n'est pas tenue : le micro est fermé par un gain nul (sans se dire coupé au serveur).
  const opts = (await calls(b.page, 'native_voice_start'))[0].args.opts;
  expect(opts).toMatchObject({ mic_gain: 0, output_volume: 0.5, self_mute: false, self_deaf: false });
  await expect.poll(async () => (await patches(b.page)).at(-1)).toEqual({ self_mute: false, self_deaf: false, mic_gain: 0, output_volume: 0.5 });

  // La touche tenue ouvre le micro au gain choisi ; relâchée, il se referme.
  await b.page.keyboard.down('p');
  await expect.poll(async () => (await patches(b.page)).at(-1)?.mic_gain).toBe(2);
  expect((await patches(b.page)).at(-1)).toMatchObject({ self_mute: false });
  await b.page.keyboard.up('p');
  await expect.poll(async () => (await patches(b.page)).at(-1)?.mic_gain).toBe(0);
  expect((await patches(b.page)).at(-1)).toMatchObject({ self_mute: false });
});

test('F04-V8 Réglages › Voix et vidéo : appareils du moteur, niveaux, phrases honnêtes à la place du test, de la caméra et des traitements', async ({
  user,
  api,
}) => {
  test.setTimeout(90_000);
  const a = await user('hote');
  const b = await user('reglages');
  const g = await guild(api, a, b);
  await fakeEngine(b);
  await fakeServerAnswer(b.page);
  await lobby(b, g);
  await join(b);
  await engine(b.page, { snapshot: { state: { kind: 'connected' } } });
  await expect(panel(b.page).getByRole('status')).toHaveText('En appel');

  await settings(b.page);
  await expect(b.page.getByRole('heading', { name: 'Appareils' })).toBeVisible();

  // Les listes viennent du moteur : « Par défaut du système », puis ses appareils (celui du système marqué).
  const mic = b.page.getByLabel('Entrée audio');
  const speaker = b.page.getByLabel('Sortie audio');
  await expect(mic.locator('option')).toHaveText(['Par défaut du système', 'Micro du casque (par défaut)', 'Micro de la webcam']);
  await expect(speaker.locator('option')).toHaveText(['Par défaut du système', 'Haut-parleurs (par défaut)', 'Casque USB']);
  await mic.selectOption({ label: 'Micro de la webcam' });
  await speaker.selectOption({ label: 'Casque USB' });
  expect(await b.page.evaluate(() => localStorage.getItem('kraken.native.mic'))).toBe('alsa:webcam');
  expect(await b.page.evaluate(() => localStorage.getItem('kraken.native.speaker'))).toBe('alsa:usb');
  // Pas de relance en cours d'appel : le choix vaut pour le prochain, et la page le dit.
  await expect(b.page.getByText('servent au prochain appel', { exact: false })).toBeVisible();
  expect((await calls(b.page, 'native_voice_start')).length).toBe(1);

  // Gain du micro et volume de sortie : tout de suite, en appel.
  await b.page.getByLabel('Volume du micro').fill('2.5');
  await expect(b.page.locator('output').filter({ hasText: '250 %' })).toBeVisible();
  await expect.poll(async () => (await patches(b.page)).at(-1)?.mic_gain).toBe(2.5);
  await b.page.getByLabel('Volume des autres').fill('1.5');
  await expect.poll(async () => (await patches(b.page)).at(-1)?.output_volume).toBe(1.5);
  expect(await b.page.evaluate(() => localStorage.getItem('kraken.native.gain'))).toBe('2.5');
  expect(await b.page.evaluate(() => localStorage.getItem('kraken.native.volume'))).toBe('1.5');
  // Revenir à 1 efface le réglage gardé : inchangé.
  await b.page.getByLabel('Volume du micro').fill('1');
  await expect.poll(async () => (await patches(b.page)).at(-1)?.mic_gain).toBe(1);
  expect(await b.page.evaluate(() => localStorage.getItem('kraken.native.gain'))).toBeNull();

  // Le niveau du micro suit le relevé pendant l'appel.
  const meter = b.page.getByRole('meter', { name: 'Niveau du micro' });
  await expect(meter).toHaveAttribute('aria-valuenow', '0');
  await engine(b.page, { snapshot: { mic_level: 0.5 } });
  await expect(meter).toHaveAttribute('aria-valuenow', '50');

  // Ce qui n'existe pas, en toutes lettres (et pas de boutons qui ne feraient rien).
  await expect(b.page.getByText(NO_VIDEO)).toBeVisible();
  await expect(b.page.getByText('un casque évite l’écho'.replace('’', "'"), { exact: false })).toBeVisible();
  await expect(b.page.getByText("Le test du micro n'existe pas encore dans l'appli de bureau", { exact: false })).toBeVisible();
  await expect(b.page.getByRole('button', { name: 'Tester le micro' })).toHaveCount(0);
  await expect(b.page.getByRole('button', { name: 'Aperçu de la caméra' })).toHaveCount(0);
  await expect(b.page.getByRole('switch', { name: "Annulation d'écho" })).toHaveCount(0);
  await expect(b.page.getByRole('switch', { name: 'Suppression du bruit' })).toHaveCount(0);
  await expect(open(b.page)).toBeVisible();
  // Appuyer pour parler reste là, comme avec le navigateur.
  await expect(b.page.getByRole('switch', { name: 'Appuyer pour parler' })).toBeVisible();
  await a11y(b.page);

  // Le choix gardé sert au prochain appel : le moteur reçoit les identifiants choisis.
  await b.page.goBack();
  await bar(b.page).getByRole('button', { name: 'Quitter', exact: true }).click();
  await expect.poll(async () => (await calls(b.page, 'native_voice_stop')).length).toBe(1);
  await expect(b.page.getByRole('button', { name: 'Rejoindre', exact: true })).toBeVisible();
  await join(b);
  await expect.poll(async () => (await calls(b.page, 'native_voice_start')).length).toBe(2);
  const second = (await calls(b.page, 'native_voice_start'))[1].args.opts;
  expect(second).toMatchObject({ mic_device: 'alsa:webcam', speaker_device: 'alsa:usb', mic_gain: 1, output_volume: 1.5 });
});

test('F04-V9 un appareil choisi puis débranché : l’appel retombe sur celui du système, sans rien dire', async ({ user, api }) => {
  const a = await user('hote');
  const b = await user('debranche');
  const g = await guild(api, a, b);
  await fakeEngine(b, { 'kraken.native.mic': 'alsa:disparu', 'kraken.native.speaker': 'alsa:usb' });
  await fakeServerAnswer(b.page);
  await lobby(b, g);
  await join(b);
  await expect.poll(async () => (await calls(b.page, 'native_voice_start')).length).toBe(1);
  const opts = (await calls(b.page, 'native_voice_start'))[0].args.opts;
  expect(opts.mic_device).toBeNull();
  expect(opts.speaker_device).toBe('alsa:usb');
});

test('F04-V10 à 390 px et texte à 20 : rien ne déborde dans le salon, l’appel et les réglages', async ({ user, api }) => {
  const a = await user('hote');
  const b = await user('mobile');
  const g = await guild(api, a, b);
  await fakeEngine(b, { 'kraken.text': '20' });
  await fakeServerAnswer(b.page);
  const gw = await gateway(b.page);
  await lobby(b, g);
  // À 390 px la liste des salons est dans le tiroir : on entre dans le salon en grand, puis on réduit la fenêtre.
  await b.page.setViewportSize({ width: 390, height: 844 });
  const overflow = () => b.page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(await overflow()).toBeLessThanOrEqual(0);
  await gw.voiceState({ guild_id: g.gid, channel_id: g.voice, user_id: b.id, self_mute: false, self_deaf: false, streaming: false, camera: false });
  await gw.voiceState({ guild_id: g.gid, channel_id: g.voice, user_id: a.id, self_mute: false, self_deaf: false, streaming: true, camera: false });
  await join(b);
  await engine(b.page, { snapshot: { state: { kind: 'connected' } } });
  // À 390 px le panneau de connexion est dans le tiroir : la connexion se lit à la fin de « Connexion au salon vocal… ».
  await expect(b.page.getByText('Connexion au salon vocal…')).toHaveCount(0);
  await expect(tiles(b.page)).toHaveCount(2);
  await expect(b.page.getByText(NO_VIDEO)).toBeVisible();
  await expect(b.page.getByText("l'image ne s'affiche pas", { exact: false })).toBeVisible();
  expect(await overflow()).toBeLessThanOrEqual(0);
  await settings(b.page);
  await expect(b.page.getByLabel('Volume du micro')).toBeVisible();
  expect(await overflow()).toBeLessThanOrEqual(0);
});

test('F04-V11 WebRTC présent mais contexte non sécurisé (WKWebView, instance en http) : le moteur natif prend le relais, « Rejoindre » marche', async ({
  user,
  api,
}) => {
  test.setTimeout(90_000);
  const a = await user('hote');
  const b = await user('wkwebview');
  const g = await guild(api, a, b);
  await fakeEngine(b, {}, {}, 'insecure');
  await fakeServerAnswer(b.page);
  await lobby(b, g);
  // La page est bien dans le cas simulé : WebRTC est là, mais le contexte n'est pas sécurisé et il n'y a pas de `mediaDevices`.
  expect(await b.page.evaluate(() => [typeof RTCPeerConnection, window.isSecureContext, typeof navigator.mediaDevices])).toEqual([
    'function',
    false,
    'undefined',
  ]);
  // Le moteur est interrogé une fois ; « Rejoindre » est permis, sans le message « Connexion non sécurisée » (le natif parle et écoute).
  expect((await calls(b.page, 'native_voice_available')).length).toBe(1);
  await expect(b.page.getByRole('button', { name: 'Rejoindre', exact: true })).toBeEnabled();
  await expect(b.page.getByText('Connexion non sécurisée', { exact: false })).toHaveCount(0);
  await expect(b.page.getByText(NO_VIDEO)).toBeVisible();
  await expect(open(b.page)).toBeVisible();
  await a11y(b.page);

  await join(b);
  await engine(b.page, { snapshot: { state: { kind: 'connected' } } });
  await expect(panel(b.page).getByRole('status')).toHaveText('En appel');
  expect((await calls(b.page, 'native_voice_start')).length).toBe(1);
  // Le chemin du navigateur n'a pas servi : pas de message de micro refusé, caméra et partage grisés comme avec le moteur.
  await expect(b.page.getByText('Connexion non sécurisée', { exact: false })).toHaveCount(0);
  await expect(bar(b.page).getByRole('button', { name: 'Caméra' })).toBeDisabled();
  await expect(bar(b.page).getByRole('button', { name: 'Partager ton écran' })).toBeDisabled();
  await a11y(b.page);
  // Seul macOS reçoit l'avis « Micro silencieux » : ici le niveau reste à zéro et rien ne vient.
  await b.page.waitForTimeout(8_000);
  await expect(b.page.getByText('Micro silencieux', { exact: false })).toHaveCount(0);
});

test('F04-V12 contexte non sécurisé et moteur natif absent : comme avant, on écoute seulement, avec le message du navigateur', async ({ user, api }) => {
  const a = await user('hote');
  const b = await user('http');
  const g = await guild(api, a, b);
  await fakeEngine(b, {}, { available: false }, 'insecure');
  await lobby(b, g);
  await expect(b.page.getByRole('button', { name: 'Rejoindre', exact: true })).toBeEnabled();
  await expect(b.page.getByText('Connexion non sécurisée', { exact: false })).toBeVisible();
  await expect(b.page.getByText(NO_VIDEO)).toHaveCount(0);
  expect((await calls(b.page, 'native_voice_available')).length).toBe(1);
  expect((await calls(b.page, 'native_voice_start')).length).toBe(0);
});

test('F04-V13 appli de bureau dont le navigateur peut tout (WebRTC, contexte sécurisé, micro) : le chemin d’avant, caméra et partage compris, le moteur natif n’est jamais sollicité', async ({
  user,
  api,
}) => {
  test.setTimeout(90_000);
  const a = await user('hote');
  const b = await user('complet');
  const g = await guild(api, a, b);
  await fakeEngine(b, {}, {}, 'full');
  await lobby(b, g);
  await expect(b.page.getByRole('button', { name: 'Rejoindre', exact: true })).toBeEnabled();
  await expect(b.page.getByText(NO_VIDEO)).toHaveCount(0);
  await expect(open(b.page)).toHaveCount(0);
  // Le vrai WebRTC de Chromium (micro factice), contre le vrai serveur.
  await join(b);
  await expect(panel(b.page).getByRole('status')).toHaveText('En appel', { timeout: 20_000 });
  await expect(bar(b.page).getByRole('button', { name: 'Caméra' })).toBeEnabled();
  await expect(bar(b.page).getByRole('button', { name: 'Partager ton écran' })).toBeEnabled();
  await expect(b.page.getByText(NO_VIDEO)).toHaveCount(0);
  await expect(open(b.page)).toHaveCount(0);
  // Pas une seule commande du moteur natif, pas même « est-il là ? ».
  const natives = await b.page.evaluate(() =>
    ((window as unknown as Win).__native.calls as { cmd: string }[]).map((c) => c.cmd).filter((c) => c.startsWith('native_voice_')),
  );
  expect(natives).toEqual([]);
  await a11y(b.page);
});

test('F04-V14 macOS : un micro ouvert qui reste à zéro plusieurs secondes — « Micro silencieux » avec le chemin des réglages, jamais micro coupé', async ({
  user,
  api,
}) => {
  test.setTimeout(90_000);
  const a = await user('hote');
  const b = await user('mac');
  const g = await guild(api, a, b);
  // WKWebView dit « Macintosh » dans son identifiant : c'est ce que la page lit.
  await b.context.addInitScript(() => {
    const mac = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15';
    Object.defineProperty(navigator, 'userAgent', { get: () => mac, configurable: true });
  });
  await fakeEngine(b, {}, {}, 'insecure');
  await fakeServerAnswer(b.page);
  await lobby(b, g);
  await join(b);
  await engine(b.page, { snapshot: { state: { kind: 'connected' }, mic_level: 0 } });
  await expect(panel(b.page).getByRole('status')).toHaveText('En appel');
  const notice = b.page.getByRole('status').filter({ hasText: 'Micro silencieux' });
  // Micro coupé par la personne : le silence est normal, rien n'est dit.
  const mic = bar(b.page).getByRole('button', { name: 'Couper le micro' });
  await mic.click();
  await expect(mic).toHaveAttribute('aria-pressed', 'true');
  await b.page.waitForTimeout(8_000);
  await expect(notice).toHaveCount(0);
  // Micro rouvert, toujours à zéro : l'avis arrive au bout de quelques secondes, avec où regarder.
  await mic.click();
  await expect(mic).toHaveAttribute('aria-pressed', 'false');
  await expect(notice).toBeVisible({ timeout: 20_000 });
  await expect(notice).toContainText('Réglages Système › Confidentialité et sécurité › Micro');
  await a11y(b.page);
  // Dit une fois par appel : une fois masqué, il ne revient pas.
  await b.page.getByRole('button', { name: 'Masquer ce message' }).click();
  await expect(notice).toHaveCount(0);
  await b.page.waitForTimeout(8_000);
  await expect(notice).toHaveCount(0);
});

// ───── (b) le vrai pont, le vrai SFU ─────

const BRIDGE =
  process.env.BRIDGE_BIN ??
  path.join(process.env.CARGO_TARGET_DIR ?? path.resolve(__dirname, '../../target'), 'release/examples/bridge');

type Bridge = {
  url: string;
  /** Une commande du pont, comme `invoke` : la réponse JSON, ou une erreur (`ok` faux). */
  post: (cmd: string, args?: object) => Promise<{ ok: boolean; body: any }>; // eslint-disable-line @typescript-eslint/no-explicit-any
  /** Ce que le puits du moteur a entendu (`rms`, `samples`, `tones`), `null` sans appel. */
  stats: (freqs?: number[]) => Promise<{ samples: number; window: number; rms: number; tones: { hz: number; share: number }[] } | null>;
  stop: () => Promise<void>;
};

/** Lance le pont sur un port libre : une sinusoïde de `freq` Hz en guise de micro, les adresses locales pour le SFU de la même machine. */
async function startBridge(freq: number): Promise<Bridge> {
  if (!existsSync(BRIDGE)) {
    throw new Error(
      `pont de test absent (${BRIDGE}) : cargo build --release --example bridge -p kraken-voice-native --no-default-features`,
    );
  }
  const child: ChildProcess = spawn(BRIDGE, ['0', String(freq)], {
    env: { ...process.env, BRIDGE_IPS: '127.0.0.1' },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let log = '';
  const url = await new Promise<string>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`le pont ne répond pas : ${log}`)), 10_000);
    child.stdout!.on('data', (d) => {
      log += d;
      const m = /bridge: (http:\/\/127\.0\.0\.1:\d+)/.exec(log);
      if (m) {
        clearTimeout(timer);
        resolve(m[1]);
      }
    });
    child.stderr!.on('data', (d) => (log += d));
    child.once('exit', (code) => reject(new Error(`le pont s'est arrêté (${code}) : ${log}`)));
  });
  return {
    url,
    post: async (cmd, args = {}) => {
      const res = await fetch(`${url}/${cmd}`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(args) });
      return { ok: res.ok, body: await res.json() };
    },
    stats: async (freqs = [440, 880]) => (await fetch(`${url}/stats?freqs=${freqs.join(',')}`)).json(),
    stop: async () => {
      if (child.exitCode === null) {
        child.removeAllListeners('exit');
        const gone = new Promise((r) => child.once('exit', r));
        child.kill('SIGTERM');
        await Promise.race([gone, new Promise((r) => setTimeout(r, 3_000))]);
        if (child.exitCode === null) child.kill('SIGKILL');
      }
    },
  };
}

/** Le faux bureau de la partie (b) : `invoke` va au pont (par Node : la page n'a pas le droit d'y faire de `fetch`). */
async function fakeDesktopOnBridge(u: User, bridge: Bridge) {
  await u.page.exposeFunction('__bridge', (cmd: string, args: object) =>
    cmd.startsWith('native_voice_') ? bridge.post(cmd, args) : { ok: true, body: null },
  );
  await u.context.addInitScript(() => {
    const w = window as unknown as Win;
    delete w.RTCPeerConnection;
    delete w.webkitRTCPeerConnection;
    w.__TAURI_INTERNALS__ = {
      invoke: async (cmd: string, args: object = {}) => {
        const r = await w.__bridge(cmd, args);
        if (!r.ok) throw r.body; // comme Tauri : le rejet est la phrase d'erreur
        return r.body;
      },
    };
  });
  await u.page.reload();
}

/** Le puits du pont entend A : au moins `atLeast` échantillons reçus au total, et de l'énergie (le micro factice de Chromium bipe, d'où le maximum). */
async function hearsA(bridge: Bridge, atLeast: number) {
  let heard = 0;
  let seen = 0;
  await expect
    .poll(
      async () => {
        const s = await bridge.stats();
        seen = s?.samples ?? 0;
        heard = Math.max(heard, s?.rms ?? 0);
        return { samples: seen > atLeast, energy: heard > 0.002 };
      },
      { timeout: 30_000, intervals: [250], message: `échantillons ${seen}, énergie ${heard}` },
    )
    .toEqual({ samples: true, energy: true });
}

let bridge: Bridge | undefined;
test.afterEach(async () => {
  await bridge?.stop();
  bridge = undefined;
});

test('F04-B1 vrai moteur sur le vrai SFU : B (faux bureau, pont) et A (Chromium) s’entendent, muet, quitter, deux fois de suite', async ({
  user,
  api,
}) => {
  test.setTimeout(180_000);
  const a = await user('vox');
  const b = await user('pont');
  const g = await guild(api, a, b);
  bridge = await startBridge(440);
  await fakeDesktopOnBridge(b, bridge);
  await lobby(a, g);
  await lobby(b, g);
  await expect(b.page.getByRole('button', { name: 'Rejoindre', exact: true })).toBeEnabled();
  await expect(b.page.getByText(NO_VIDEO)).toBeVisible();

  // A entre avec le vrai WebRTC de Chromium (micro factice : un bip régulier).
  await join(a);
  await expect(panel(a.page).getByRole('status')).toHaveText('En appel', { timeout: 20_000 });

  for (const round of [1, 2]) {
    // B entre par le moteur du pont : connecté, et chacun voit l'autre dans le salon.
    await join(b);
    await expect(panel(b.page).getByRole('status')).toHaveText('En appel', { timeout: 20_000 });
    await expect(tiles(b.page)).toHaveCount(2);
    await expect(tiles(a.page)).toHaveCount(2);
    await expect(tiles(a.page).filter({ hasText: b.name })).toBeVisible();
    await expect(tiles(b.page).filter({ hasText: a.name })).toBeVisible();
    const poll = await bridge.post('native_voice_poll');
    expect(poll.body.state).toEqual({ kind: 'connected' });

    // Le puits du pont a reçu l'audio de A : des échantillons qui s'accumulent, de l'énergie.
    await hearsA(bridge, 48_000);

    // A voit B parler (la sinusoïde du pont), et B se voit parler lui-même.
    const bOnA = tiles(a.page).filter({ hasText: b.name });
    await expect(bOnA).toHaveAttribute('data-speaking', '', { timeout: 15_000 });
    await expect(tiles(b.page).filter({ hasText: `${b.name} (toi)` })).toHaveAttribute('data-speaking', '', { timeout: 15_000 });

    // B se coupe : A le voit coupé et ne l'entend plus parler ; il reparle quand B rouvre.
    const mic = bar(b.page).getByRole('button', { name: 'Couper le micro' });
    await mic.click();
    await expect(mic).toHaveAttribute('aria-pressed', 'true');
    await expect(bOnA.getByRole('img', { name: 'micro coupé' })).toBeVisible({ timeout: 10_000 });
    await expect(bOnA).not.toHaveAttribute('data-speaking', '', { timeout: 10_000 });
    await expect(tiles(b.page).filter({ hasText: `${b.name} (toi)` })).not.toHaveAttribute('data-speaking', '');
    await mic.click();
    await expect(bOnA.getByRole('img', { name: 'micro coupé' })).toHaveCount(0, { timeout: 10_000 });
    await expect(bOnA).toHaveAttribute('data-speaking', '', { timeout: 15_000 });
    await a11y(b.page);

    // A partage son écran (renégociation avec le moteur natif) : B reste connecté, dit qu'il ne peut pas l'afficher, et l'entend toujours.
    if (round === 1) {
      await bar(a.page).getByRole('button', { name: 'Partager ton écran' }).click();
      await expect(bar(a.page).getByRole('button', { name: 'Arrêter le partage' })).toBeVisible({ timeout: 15_000 });
      await expect(b.page.getByText("l'image ne s'affiche pas", { exact: false })).toBeVisible({ timeout: 15_000 });
      await expect(b.page.getByRole('button', { name: `Regarder l'écran de ${a.name}` })).toHaveCount(0);
      await expect(panel(b.page).getByRole('status')).toHaveText('En appel');
      await hearsA(bridge, ((await bridge.stats())?.samples ?? 0) + 48_000);
      expect((await bridge.post('native_voice_poll')).body.state).toEqual({ kind: 'connected' });
      await bar(a.page).getByRole('button', { name: 'Arrêter le partage' }).click();
      await expect(b.page.getByText("l'image ne s'affiche pas", { exact: false })).toHaveCount(0, { timeout: 15_000 });
    }

    // B quitte : plus d'appel dans le moteur, et A reste seul dans le salon.
    await bar(b.page).getByRole('button', { name: 'Quitter', exact: true }).click();
    await expect(b.page.getByRole('button', { name: 'Rejoindre', exact: true })).toBeVisible();
    await expect.poll(async () => (await bridge!.post('native_voice_poll')).body, { timeout: 10_000, message: `tour ${round}` }).toBeNull();
    await expect(tiles(a.page)).toHaveCount(1, { timeout: 15_000 });
    await expect(tiles(a.page).filter({ hasText: b.name })).toHaveCount(0);
  }

  // A quitte à son tour : le salon est vide.
  await a.page.getByRole('button', { name: 'Quitter le salon vocal' }).click();
  await expect(a.page.getByText("Personne pour l'instant.")).toBeVisible();
});
