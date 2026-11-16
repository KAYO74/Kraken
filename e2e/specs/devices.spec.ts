// S10 Réglages › Voix et vidéo : choix du micro, de la sortie audio et de la caméra, test du micro,
// aperçu de la caméra, traitements du son et « Appuyer pour parler ». Micro (bip) et caméra (mire)
// factices de Chromium, sans fenêtre d'autorisation. Une sonde posée avant le chargement de la page
// compte les pistes ouvertes, relève les contraintes demandées et donne accès aux connexions WebRTC,
// sans rien ajouter à l'appli.
import { test, expect, a11y, API, type User } from '../fixtures';
import type { APIRequestContext, Page } from '@playwright/test';

test.use({ launchOptions: { args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'] } });

type Api = (token?: string) => Promise<APIRequestContext>;
/* eslint-disable @typescript-eslint/no-explicit-any */

/** Les flux demandés au navigateur, les contraintes de chaque demande et les connexions WebRTC. */
const PROBE = ({ hide, stable }: { hide: boolean; stable: boolean }) => {
  const w = window as any;
  w.__streams = [];
  w.__constraints = [];
  w.__pcs = [];
  w.__nego = 0;
  const md = navigator.mediaDevices;
  if (md) {
    const gum = md.getUserMedia.bind(md);
    md.getUserMedia = async (c?: MediaStreamConstraints) => {
      w.__constraints.push(JSON.parse(JSON.stringify(c ?? {})));
      const s = await gum(c);
      w.__streams.push(s);
      w.__granted = true;
      return s;
    };
    // Deux comportements de navigateur que Chromium factice n'a pas : sans autorisation, les appareils n'ont pas de
    // nom (`hide`, jusqu'à la première prise du micro) ; avec une autorisation durable, leurs identifiants restent les
    // mêmes d'un chargement à l'autre (`stable` : ici, le genre et le nom).
    if (hide || stable) {
      const enumerate = md.enumerateDevices.bind(md);
      const view = (d: MediaDeviceInfo, deviceId: string, label: string) =>
        Object.create(MediaDeviceInfo.prototype, {
          deviceId: { value: deviceId },
          groupId: { value: d.groupId },
          kind: { value: d.kind },
          label: { value: label },
        });
      md.enumerateDevices = async () => {
        const list = await enumerate();
        if (hide && !w.__granted) return list.map((d) => view(d, d.deviceId, ''));
        return stable ? list.map((d) => view(d, `${d.kind}|${d.label}`, d.label)) : list;
      };
    }
  }
  const Orig = w.RTCPeerConnection;
  w.RTCPeerConnection = function (...args: unknown[]) {
    const pc = new Orig(...args);
    pc.addEventListener('negotiationneeded', () => w.__nego++);
    w.__pcs.push(pc);
    return pc;
  };
  w.RTCPeerConnection.prototype = Orig.prototype;
};

/** Ouvre `path` avec la sonde en place (`hide` et `stable` : voir plus haut). */
async function open(page: Page, path: string, how: { hide?: boolean; stable?: boolean } = {}) {
  await page.addInitScript(PROBE, { hide: false, stable: false, ...how });
  await page.goto(path);
}

/** Combien de pistes (micro, caméra) sont encore ouvertes. */
const live = (page: Page) =>
  page.evaluate(
    () =>
      (window as any).__streams
        .flatMap((s: MediaStream) => s.getTracks())
        .filter((t: MediaStreamTrack) => t.readyState === 'live').length,
  );
/** La piste du micro envoyée dans l'appel : son numéro, et si elle laisse passer le son. */
const sent = (page: Page) =>
  page.evaluate(() => {
    const t = (window as any).__pcs[0].getSenders()[0].track as MediaStreamTrack | null;
    return t && { id: t.id, enabled: t.enabled, live: t.readyState === 'live' };
  });
const enabled = async (page: Page) => (await sent(page))?.enabled;

const reglages = (p: Page) => p.getByRole('dialog', { name: 'Réglages' });
const bar = (p: Page) => p.getByRole('group', { name: "Commandes de l'appel" });
const noScroll = (p: Page) =>
  p.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth);
const pttKey = (p: Page) => p.getByRole('button', { name: /Touche pour parler/ });
/** Contraste WCAG entre deux couleurs CSS `rgb(r, g, b)`. */
function ratio(a: string, b: string) {
  const lum = (c: string) => {
    const [r, g, bl] = (c.match(/[\d.]+/g) ?? ['0', '0', '0']).slice(0, 3).map((v) => {
      const x = Number(v) / 255;
      return x <= 0.04045 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * r + 0.7152 * g + 0.0722 * bl;
  };
  const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}
const level = (p: Page) => p.getByRole('meter', { name: 'Niveau du micro' });

/** Un compte seul dans un serveur tout neuf : son identifiant et celui du salon textuel. */
async function solo(user: (prefix?: string) => Promise<User>, api: Api) {
  const u = await user('voice');
  const created = await (await api(u.token)).post(`${API}/guilds`, { data: { name: 'Studio' } });
  expect(created.status()).toBe(201);
  const g = await created.json();
  return { u, gid: g.guild.id as string, text: g.channels.find((c: { kind: string }) => c.kind === 'text').id as string };
}

/** Entre dans le salon vocal du serveur et attend la connexion. */
async function join(page: Page, gid: string, text: string) {
  await page.goto(`/channels/${gid}/${text}`);
  await page.getByRole('link', { name: 'salon vocal Général', exact: true }).click();
  await page.getByRole('button', { name: 'Rejoindre', exact: true }).click();
  await expect(page.getByRole('region', { name: 'Connexion vocale' }).getByRole('status')).toHaveText('En appel', {
    timeout: 20_000,
  });
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

test('S10-H1 Voix et vidéo : trois listes avec « Par défaut du système », noms des appareils après autorisation', async ({
  user,
}) => {
  const { page } = await user('vv');
  await open(page, '/settings/voice', { hide: true });
  await expect(page.getByRole('heading', { name: 'Voix et vidéo', level: 2 })).toBeVisible();
  await expect(page.getByRole('link', { name: 'Voix et vidéo' })).toHaveAttribute('aria-current', 'page');
  for (const name of ['Entrée audio', 'Sortie audio', 'Caméra']) {
    const select = page.getByLabel(name, { exact: true });
    await expect(select).toBeVisible();
    await expect(select).toHaveValue('');
    await expect(select.getByRole('option', { name: 'Par défaut du système' })).toHaveCount(1);
  }
  // Rien ne démarre tout seul : ni micro, ni caméra.
  expect(await live(page)).toBe(0);
  await expect(page.getByRole('img', { name: 'Image de ta caméra' })).toHaveCount(0);

  // Sans autorisation les appareils n'ont pas de nom : « Micro 1 »… et un bouton demande l'accès une fois.
  const mic = page.getByLabel('Entrée audio', { exact: true });
  const allow = page.getByRole('button', { name: "Autoriser l'accès" });
  await expect(allow).toBeVisible();
  await expect(allow).toHaveAccessibleDescription(/nom de tes appareils/);
  expect(await mic.getByRole('option').allTextContents()).toEqual(['Par défaut du système', 'Micro 1', 'Micro 2']);
  await a11y(page);
  await allow.click();
  await expect(allow).toHaveCount(0);
  await expect(mic.getByRole('option', { name: 'Micro 1' })).toHaveCount(0);
  expect((await mic.getByRole('option').allTextContents()).length).toBeGreaterThan(1);
  await expect(page.getByLabel('Caméra', { exact: true }).getByRole('option')).not.toHaveCount(1);
  expect(await live(page)).toBe(0);
  await a11y(page);
});

test('S10-H2 test du micro : une jauge qui bouge, sans écho, arrêtée en changeant de page', async ({ user }) => {
  const { page } = await user('vtest');
  await open(page, '/settings/voice');
  const meter = level(page);
  await expect(meter).toHaveAttribute('aria-valuemin', '0');
  await expect(meter).toHaveAttribute('aria-valuemax', '100');
  await expect(meter).toHaveAttribute('aria-valuenow', '0');
  expect(await live(page)).toBe(0);

  const test = page.getByRole('button', { name: 'Tester le micro' });
  await test.click();
  await expect(page.getByRole('button', { name: 'Arrêter le test' })).toBeVisible();
  // Le micro factice bipe : la jauge monte, et reste une valeur entre 0 et 100.
  await expect
    .poll(async () => Number(await meter.getAttribute('aria-valuenow')), { timeout: 15_000 })
    .toBeGreaterThan(0);
  const now = Number(await meter.getAttribute('aria-valuenow'));
  expect(now).toBeLessThanOrEqual(100);
  expect(await live(page)).toBeGreaterThan(0);
  // Pas de lecture du micro dans les haut-parleurs : aucun élément audio ne porte le flux.
  expect(await page.evaluate(() => [...document.querySelectorAll('audio,video')].some((e: any) => e.srcObject))).toBe(false);
  await a11y(page);

  // La barre se distingue de son fond d'au moins 3 pour 1, dans chaque thème.
  for (const theme of ['dark', 'light', 'black']) {
    await page.evaluate((t) => document.documentElement.setAttribute('data-theme', t), theme);
    const colors = await meter.evaluate((el) => ({
      track: getComputedStyle(el).backgroundColor,
      fill: getComputedStyle(el.firstElementChild!).backgroundColor,
      height: el.getBoundingClientRect().height,
    }));
    expect(colors.height).toBe(8);
    expect(ratio(colors.fill, colors.track), theme).toBeGreaterThanOrEqual(3);
  }
  await page.evaluate(() => document.documentElement.setAttribute('data-theme', 'dark'));

  // « Arrêter le test » libère le micro et remet la jauge à zéro.
  await page.getByRole('button', { name: 'Arrêter le test' }).click();
  await expect(test).toBeVisible();
  await expect.poll(() => live(page)).toBe(0);
  await expect(meter).toHaveAttribute('aria-valuenow', '0');

  // Un test relancé s'arrête aussi en quittant la page.
  await test.click();
  await expect.poll(() => live(page)).toBeGreaterThan(0);
  await reglages(page).getByRole('link', { name: 'Apparence' }).click();
  await expect(page.getByRole('heading', { name: 'Apparence', level: 2 })).toBeVisible();
  await expect.poll(() => live(page)).toBe(0);

  // Et en fermant les réglages.
  await reglages(page).getByRole('link', { name: 'Voix et vidéo' }).click();
  await page.getByRole('button', { name: 'Tester le micro' }).click();
  await expect.poll(() => live(page)).toBeGreaterThan(0);
  await page.keyboard.press('Escape');
  await expect(reglages(page)).toHaveCount(0);
  await expect.poll(() => live(page)).toBe(0);
});

test('S10-H3 un choix est gardé après rechargement ; un appareil disparu retombe sur celui du système', async ({
  user,
}) => {
  const { page } = await user('vkeep');
  await open(page, '/settings/voice', { stable: true });
  const mic = page.getByLabel('Entrée audio', { exact: true });
  await expect.poll(() => mic.getByRole('option').count()).toBeGreaterThan(1);
  const [, first] = await mic.getByRole('option').evaluateAll((o) => o.map((x: any) => x.value));
  await mic.selectOption(first);
  expect(await page.evaluate(() => localStorage.getItem('kraken.mic'))).toBe(first);
  await page.reload();
  await expect.poll(() => mic.getByRole('option').count()).toBeGreaterThan(1);
  await expect(mic).toHaveValue(first);

  // Retour au système : la clé est oubliée.
  await mic.selectOption('');
  expect(await page.evaluate(() => localStorage.getItem('kraken.mic'))).toBeNull();

  // Un identifiant qui n'existe plus : la liste montre le défaut, et le test du micro marche quand même.
  await page.evaluate(() => localStorage.setItem('kraken.mic', 'disparu'));
  await page.reload();
  await expect(mic).toHaveValue('');
  await page.getByRole('button', { name: 'Tester le micro' }).click();
  await expect.poll(async () => Number(await level(page).getAttribute('aria-valuenow')), { timeout: 15_000 }).toBeGreaterThan(0);
  await expect(page.getByRole('alert')).toHaveCount(0);
});

test('S10-H4 l’aperçu de la caméra ne démarre que sur demande, en miroir, et s’arrête en quittant la page', async ({
  user,
}) => {
  const { page } = await user('vcam');
  await open(page, '/settings/voice');
  const button = page.getByRole('button', { name: 'Aperçu de la caméra' });
  await expect(button).toBeVisible();
  await page.waitForTimeout(500);
  expect(await live(page)).toBe(0);
  await expect(page.getByRole('img', { name: 'Image de ta caméra' })).toHaveCount(0);

  await button.click();
  const image = page.getByLabel('Image de ta caméra');
  await expect(image).toBeVisible();
  await expect(page.getByRole('button', { name: "Arrêter l'aperçu" })).toBeVisible();
  await expect(image).toHaveCSS('transform', 'matrix(-1, 0, 0, 1, 0, 0)');
  expect(await image.evaluate((v: HTMLVideoElement) => v.muted)).toBe(true);
  await expect.poll(() => image.evaluate((v: HTMLVideoElement) => v.videoWidth), { timeout: 15_000 }).toBeGreaterThan(0);
  const box = await image.boundingBox();
  expect(Math.abs(box!.width / box!.height - 16 / 9)).toBeLessThan(0.05);
  await a11y(page);

  await page.getByRole('button', { name: "Arrêter l'aperçu" }).click();
  await expect(image).toHaveCount(0);
  await expect.poll(() => live(page)).toBe(0);

  await button.click();
  await expect(image).toBeVisible();
  await reglages(page).getByRole('link', { name: 'Accessibilité' }).click();
  await expect(page.getByRole('heading', { name: 'Accessibilité', level: 2 })).toBeVisible();
  await expect.poll(() => live(page)).toBe(0);
});

test('S10-H5 les trois traitements du son : actifs au départ, gardés, envoyés au navigateur', async ({ user }) => {
  const { page } = await user('vproc');
  await open(page, '/settings/voice');
  const switches = ['Annulation d’écho', 'Suppression du bruit', 'Réglage automatique du volume'].map((n) =>
    page.getByRole('switch', { name: n.replace('’', "'") }),
  );
  for (const s of switches) await expect(s).toBeChecked();

  const constraints = () => page.evaluate(() => (window as any).__constraints.at(-1)?.audio);
  await page.getByRole('button', { name: 'Tester le micro' }).click();
  await expect.poll(constraints).toMatchObject({ echoCancellation: true, noiseSuppression: true, autoGainControl: true });

  // Écho coupé pendant le test : le test repart avec la nouvelle contrainte.
  await switches[0].uncheck();
  await expect.poll(constraints).toMatchObject({ echoCancellation: false, noiseSuppression: true, autoGainControl: true });
  await page.getByRole('button', { name: 'Arrêter le test' }).click();
  await switches[2].uncheck();
  await a11y(page);

  expect(await page.evaluate(() => [localStorage.getItem('kraken.echo'), localStorage.getItem('kraken.noise'), localStorage.getItem('kraken.agc')])).toEqual(['off', null, 'off']);
  await page.reload();
  await expect(switches[0]).not.toBeChecked();
  await expect(switches[1]).toBeChecked();
  await expect(switches[2]).not.toBeChecked();
});

test('S10-H6 changer de micro en appel remplace la piste sans renégocier et sans toucher à la sourdine', async ({
  user,
  api,
}) => {
  test.setTimeout(120_000);
  const { u, gid, text } = await solo(user, api);
  const { page } = u;
  await open(page, '/channels/@me');
  // Un appareil mémorisé qui n'existe plus : l'appel démarre quand même, avec le micro du système.
  await page.evaluate(() => localStorage.setItem('kraken.mic', 'disparu'));
  await join(page, gid, text);
  const before = await sent(page);
  expect(before).toMatchObject({ enabled: true, live: true });

  // Sourdine, puis un autre micro dans les réglages : nouvelle piste, toujours en sourdine.
  await bar(page).getByRole('button', { name: 'Couper le micro' }).click();
  await expect.poll(() => enabled(page)).toBe(false);
  await settings(page);
  const mic = page.getByLabel('Entrée audio', { exact: true });
  await expect.poll(() => mic.getByRole('option').count()).toBeGreaterThan(1);
  const options = await mic.getByRole('option').evaluateAll((o) => o.map((x: any) => x.value));
  const nego = await page.evaluate(() => (window as any).__nego);
  await mic.selectOption(options[1]);
  await expect.poll(async () => (await sent(page))?.id).not.toBe(before!.id);
  const after = await sent(page);
  expect(after).toMatchObject({ enabled: false, live: true });
  expect(await page.evaluate(() => (window as any).__nego)).toBe(nego);
  expect(await page.evaluate(() => (window as any).__pcs[0].signalingState)).toBe('stable');

  // Retour à l'appel : le micro est toujours en sourdine, et l'ancienne piste est arrêtée.
  await page.goBack();
  await expect(bar(page).getByRole('button', { name: 'Couper le micro' })).toHaveAttribute('aria-pressed', 'true');
  await bar(page).getByRole('button', { name: 'Couper le micro' }).click();
  await expect.poll(() => enabled(page)).toBe(true);
  const stopped = await page.evaluate((id) => {
    const all = (window as any).__streams.flatMap((s: MediaStream) => s.getAudioTracks());
    return all.filter((t: MediaStreamTrack) => t.id === id).every((t: MediaStreamTrack) => t.readyState === 'ended');
  }, before!.id);
  expect(stopped).toBe(true);
});

test('S10-H7 appuyer pour parler : choix de la touche, indicateur, micro ouvert tant que la touche est tenue', async ({
  user,
  api,
}) => {
  test.setTimeout(120_000);
  const { u, gid, text } = await solo(user, api);
  const { page } = u;
  await open(page, '/settings/voice');

  // Éteint au départ : pas de ligne pour la touche.
  const toggle = page.getByRole('switch', { name: 'Appuyer pour parler' });
  await expect(toggle).not.toBeChecked();
  await expect(pttKey(page)).toHaveCount(0);
  await toggle.check();
  await expect(pttKey(page)).toBeVisible();
  await expect(pttKey(page)).toContainText('Choisir une touche');
  await expect(page.getByText('Ça ne marche que lorsque la fenêtre est au premier plan')).toBeVisible();
  await a11y(page);

  // Tab, Entrée : refusés avec un message sur le champ. Échap annule sans fermer les réglages.
  await pttKey(page).click();
  const status = page.getByRole('status').filter({ hasText: 'Appuie sur une touche… Échap annule' });
  await expect(status).toBeVisible();
  await page.keyboard.press('Tab');
  await expect(pttKey(page)).toHaveAttribute('aria-invalid', 'true');
  await expect(page.getByRole('alert')).toContainText('Tab');
  await expect(pttKey(page)).toBeFocused();
  await expect(status).toBeVisible();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('alert')).toContainText('Entrée');
  await expect(pttKey(page)).toHaveAttribute('aria-invalid', 'true');
  await a11y(page);
  await page.keyboard.press('Escape');
  await expect(reglages(page)).toBeVisible();
  await expect(status).toHaveCount(0);
  await expect(pttKey(page)).toContainText('Choisir une touche');
  await expect(pttKey(page)).not.toHaveAttribute('aria-invalid', 'true');
  expect(await page.evaluate(() => localStorage.getItem('kraken.ptt_key'))).toBeNull();

  // Espace est accepté : la touche s'affiche en <kbd>, gardée sur l'appareil.
  await pttKey(page).click();
  await page.keyboard.press('Space');
  await expect(pttKey(page).locator('kbd')).toHaveText('Espace');
  await expect(pttKey(page)).not.toHaveAttribute('aria-invalid', 'true');
  await expect(page.getByRole('status').filter({ hasText: 'Appuie sur une touche' })).toHaveCount(0);
  expect(await page.evaluate(() => [localStorage.getItem('kraken.ptt'), localStorage.getItem('kraken.ptt_key')])).toEqual(['on', 'Space']);
  await a11y(page);

  // En appel : l'indicateur dit la touche, le micro est fermé tant qu'elle n'est pas tenue.
  await join(page, gid, text);
  const indicator = bar(page).getByText('Appuyer pour parler :');
  await expect(indicator).toContainText('Espace');
  await expect.poll(() => enabled(page)).toBe(false);
  await expect(page.getByRole('list', { name: 'Participants' }).getByRole('img', { name: 'micro coupé' })).toHaveCount(0);
  await a11y(page);

  const said = (t: string) => page.getByRole('status').filter({ hasText: new RegExp(`^${t}$`) });
  await page.keyboard.down('Space');
  await expect.poll(() => enabled(page)).toBe(true);
  await expect(indicator).toHaveAttribute('data-on', '');
  await expect(said('Micro ouvert')).toBeVisible();
  // Une touche qui répète ne fait rien de plus.
  await page.keyboard.down('Space');
  expect(await enabled(page)).toBe(true);
  await page.keyboard.up('Space');
  await expect.poll(() => enabled(page)).toBe(false);
  await expect(indicator).not.toHaveAttribute('data-on', '');
  await expect(said('Micro fermé')).toBeVisible();
  // Le serveur n'apprend rien : ma sourdine ne bouge pas.
  await expect(page.getByRole('list', { name: 'Participants' }).getByRole('img', { name: 'micro coupé' })).toHaveCount(0);

  // Le bouton « Couper le micro » reste le maître : tenu ou non, la touche n'ouvre rien.
  const mute = bar(page).getByRole('button', { name: 'Couper le micro' });
  await mute.click();
  await expect(mute).toHaveAttribute('aria-pressed', 'true');
  await page.keyboard.down('Space');
  await expect(said('Micro coupé')).toBeVisible();
  expect(await enabled(page)).toBe(false);
  await page.keyboard.up('Space');
  // La touche n'a pas « cliqué » le bouton qui avait le focus.
  await expect(mute).toHaveAttribute('aria-pressed', 'true');
  await mute.click();
  await expect(mute).toHaveAttribute('aria-pressed', 'false');
  await expect.poll(() => enabled(page)).toBe(false);

  // Dans un champ de saisie, la touche écrit : elle n'ouvre pas le micro. La barre latérale suit l'appel.
  await page.getByRole('link', { name: 'général', exact: true }).first().click();
  const box = page.getByLabel('Écrire dans #général');
  await box.click();
  await page.keyboard.down('Space');
  await page.waitForTimeout(300);
  expect(await enabled(page)).toBe(false);
  await page.keyboard.up('Space');
  await expect(box).toHaveValue(' ');
  await box.fill('');
  await page.getByRole('heading', { name: 'général', level: 1 }).click();

  // La fenêtre perd le focus touche tenue : le micro se ferme sans attendre le relâchement.
  await page.keyboard.down('Space');
  await expect.poll(() => enabled(page)).toBe(true);
  const closed = await page.evaluate(async () => {
    window.dispatchEvent(new Event('blur'));
    await new Promise((r) => setTimeout(r, 50));
    return (window as any).__pcs[0].getSenders()[0].track.enabled;
  });
  expect(closed).toBe(false);
  await page.keyboard.up('Space');

  // Le relâchement a un délai : une reprise rapide garde le micro ouvert.
  await page.keyboard.down('Space');
  await expect.poll(() => enabled(page)).toBe(true);
  await page.keyboard.up('Space');
  expect(await enabled(page)).toBe(true);
  await expect.poll(() => enabled(page)).toBe(false);

  // Éteint dans les réglages : plus d'indicateur, le micro reprend comme avant.
  await settings(page);
  await page.getByRole('switch', { name: 'Appuyer pour parler' }).uncheck();
  await expect(pttKey(page)).toHaveCount(0);
  await page.goBack();
  await expect(bar(page).getByText('Appuyer pour parler :')).toHaveCount(0);
  await expect.poll(() => enabled(page)).toBe(true);
});

test('S10-H8 à 390 px : rien ne déborde, listes, test et aperçu sur toute la largeur', async ({ user }) => {
  const { page } = await user('vsmall');
  await page.setViewportSize({ width: 390, height: 844 });
  await open(page, '/settings/voice');
  await expect(page.getByRole('heading', { name: 'Voix et vidéo', level: 2 })).toBeVisible();
  await page.getByRole('switch', { name: 'Appuyer pour parler' }).check();
  await page.getByRole('button', { name: 'Aperçu de la caméra' }).click();
  await expect(page.getByLabel('Image de ta caméra')).toBeVisible();
  await page.getByRole('button', { name: 'Tester le micro' }).click();
  await pttKey(page).click();
  await page.keyboard.press('Tab');
  expect(await noScroll(page)).toBe(true);
  const main = await reglages(page).locator('main').boundingBox();
  for (const name of ['Entrée audio', 'Sortie audio', 'Caméra']) {
    const box = await page.getByLabel(name, { exact: true }).boundingBox();
    expect(box!.x).toBeGreaterThanOrEqual(0);
    expect(box!.x + box!.width).toBeLessThanOrEqual(390);
    expect(box!.width).toBeGreaterThan(main!.width * 0.7);
  }
  const preview = await page.getByLabel('Image de ta caméra').boundingBox();
  expect(preview!.x + preview!.width).toBeLessThanOrEqual(390);
  expect(preview!.width).toBeGreaterThan(main!.width * 0.7);
  await a11y(page);
  // Texte à 20 px et zoom à 150 % : toujours rien qui déborde.
  await page.evaluate(() => {
    localStorage.setItem('kraken.text', '20');
    localStorage.setItem('kraken.zoom', '150');
  });
  await page.reload();
  await expect(page.getByRole('heading', { name: 'Voix et vidéo', level: 2 })).toBeVisible();
  expect(await noScroll(page)).toBe(true);
});
