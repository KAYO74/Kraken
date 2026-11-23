// F04 Salon vocal sans WebRTC : le message dit pourquoi et sur quelle page, au lieu d'un texte vague.
import { test, expect, API, type User } from '../fixtures';
import type { APIRequestContext } from '@playwright/test';

test.use({ launchOptions: { args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'] } });

/** Un serveur neuf, et le salon vocal ouvert (avant d'entrer). */
async function lobby(u: User, api: (token?: string) => Promise<APIRequestContext>) {
  const owner = await api(u.token);
  const created = await owner.post(`${API}/guilds`, { data: { name: 'Sans WebRTC' } });
  expect(created.status()).toBe(201);
  const g = await created.json();
  const text = g.channels.find((c: { kind: string }) => c.kind === 'text').id;
  await u.page.goto(`/channels/${g.guild.id}/${text}`);
  await u.page.getByRole('link', { name: 'salon vocal Général', exact: true }).click();
  await expect(u.page.getByRole('heading', { name: 'Général', level: 1 })).toBeVisible();
}

test('F04-U1 WebRTC absent : le salon dit pourquoi, avec la page et le navigateur, et « Rejoindre » est grisé', async ({ user, api }) => {
  const u = await user('sanswebrtc');
  await u.page.addInitScript(() => {
    delete (window as unknown as Record<string, unknown>).RTCPeerConnection;
    delete (window as unknown as Record<string, unknown>).webkitRTCPeerConnection;
  });
  await lobby(u, api);
  await expect(u.page.getByRole('button', { name: 'Rejoindre', exact: true })).toBeDisabled();
  // localhost est une page sécurisée : le navigateur lui-même n'a pas WebRTC.
  await expect(u.page.getByText('WebRTC y est absent ou désactivé', { exact: false })).toBeVisible();
  await expect(u.page.getByText(/Détails : page en HTTPS ou localhost · RTCPeerConnection absent · .*Chrome/)).toBeVisible();
});

test('F04-U2 WebRTC sous son nom préfixé : l’alias le remet, on peut rejoindre', async ({ user, api }) => {
  const u = await user('webkitrtc');
  await u.page.addInitScript(() => {
    const w = window as unknown as Record<string, unknown>;
    w.webkitRTCPeerConnection = w.RTCPeerConnection;
    delete w.RTCPeerConnection;
  });
  await lobby(u, api);
  await expect(u.page.getByRole('button', { name: 'Rejoindre', exact: true })).toBeEnabled();
  await expect(u.page.getByText('WebRTC y est absent ou désactivé', { exact: false })).toHaveCount(0);
});

test('F04-U3 le navigateur refuse de créer la connexion : l’erreur dit laquelle', async ({ user, api }) => {
  const u = await user('refuse');
  await u.page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).RTCPeerConnection = function () {
      throw new DOMException('pas ici', 'NotSupportedError');
    };
  });
  await lobby(u, api);
  await u.page.getByRole('button', { name: 'Rejoindre', exact: true }).click();
  await expect(u.page.getByRole('alert').filter({ hasText: 'NotSupportedError : pas ici' })).toContainText(
    'Le navigateur a refusé de démarrer l\'appel vocal',
  );
});

test('F04-U4 appli de bureau sans WebRTC (WebKitGTK de Linux) : le message le dit, « Ouvrir dans mon navigateur » appelle la commande', async ({ user, api }) => {
  const u = await user('bureau');
  await u.page.addInitScript(() => {
    const w = window as unknown as Record<string, unknown>;
    delete w.RTCPeerConnection;
    delete w.webkitRTCPeerConnection;
    const calls: string[] = [];
    w.__calls = calls;
    w.__fail = false;
    // L'appli de bureau, vue de la page : `invoke` note chaque appel, et `open_in_browser` peut échouer.
    w.__TAURI_INTERNALS__ = {
      invoke: (cmd: string) => {
        calls.push(cmd);
        return cmd === 'open_in_browser' && w.__fail ? Promise.reject('non') : Promise.resolve(null);
      },
    };
  });
  await lobby(u, api);
  await expect(u.page.getByRole('button', { name: 'Rejoindre', exact: true })).toBeDisabled();
  await expect(u.page.getByText('livré sans WebRTC', { exact: false })).toBeVisible();
  const open = u.page.getByRole('button', { name: 'Ouvrir dans mon navigateur' });
  await open.click();
  await expect.poll(() => u.page.evaluate(() => (window as unknown as { __calls: string[] }).__calls)).toContain('open_in_browser');
  await expect(u.page.getByRole('alert').filter({ hasText: /Impossible d.ouvrir le navigateur/ })).toHaveCount(0);
  await u.page.evaluate(() => ((window as unknown as Record<string, unknown>).__fail = true));
  await open.click();
  await expect(u.page.getByRole('alert').filter({ hasText: /Impossible d.ouvrir le navigateur/ })).toBeVisible();
});

test('F04-U5 navigateur sans WebRTC : pas de bouton « Ouvrir dans mon navigateur », et l’appli de bureau n’est pas promise partout', async ({ user, api }) => {
  const u = await user('seulnav');
  await u.page.addInitScript(() => {
    delete (window as unknown as Record<string, unknown>).RTCPeerConnection;
    delete (window as unknown as Record<string, unknown>).webkitRTCPeerConnection;
  });
  await lobby(u, api);
  await expect(u.page.getByText("sous Windows et macOS, avec l'appli de bureau", { exact: false })).toBeVisible();
  await expect(u.page.getByRole('button', { name: 'Ouvrir dans mon navigateur' })).toHaveCount(0);
});
