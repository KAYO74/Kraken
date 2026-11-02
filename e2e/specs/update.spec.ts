// Mise à jour de l'appli de bureau : dans l'appli (simulée par un faux `__TAURI_INTERNALS__.invoke`
// qui annonce la version 9.9.9), une tuile « Mise à jour » apparaît au bas du rail ; elle ouvre une
// carte, au clavier aussi, qui installe (progression, redémarrage) ou, sans paquet signé, ouvre le
// téléchargement. Dans un navigateur, rien ne s'affiche et rien n'est appelé.
import { test, expect, a11y, type User } from '../fixtures';

type Mode = 'install' | 'download';

/** L'appli de bureau, vue de la page : `update_status` annonce 9.9.9, `update_install` avance par étapes. */
async function fakeDesktop(u: User, mode: Mode = 'install') {
  await u.context.addInitScript((mode) => {
    const w = window as unknown as Record<string, unknown>;
    const calls: string[] = [];
    const state = { phase: 'idle', downloaded: 0, total: null as number | null };
    w.__calls = calls;
    w.__fail = null as string | null;
    w.__TAURI_INTERNALS__ = {
      invoke: (cmd: string) => {
        calls.push(cmd);
        if (cmd === 'update_status') {
          return Promise.resolve({
            current: '0.8.0',
            available: '9.9.9',
            notes: null,
            notes_url: 'https://github.com/KAYO74/Kraken/releases/tag/v9.9.9',
            mode,
            ...state,
          });
        }
        if (cmd === 'update_install') {
          if (w.__fail) return Promise.reject(w.__fail);
          if (mode === 'download') return Promise.resolve(null);
          Object.assign(state, { phase: 'downloading', downloaded: 42, total: 100 });
          return new Promise((done) => setTimeout(() => done(null), 1500));
        }
        return Promise.resolve(null);
      },
    };
  }, mode);
  await u.page.reload();
}

const calls = (u: User): Promise<string[]> => u.page.evaluate(() => (window as unknown as { __calls: string[] }).__calls);
const tile = (u: User) => u.page.getByRole('button', { name: 'Mise à jour v9.9.9 disponible' });
const card = (u: User) => u.page.getByRole('group', { name: 'Mise à jour', exact: true });

test('dans un navigateur : ni tuile ni appel', async ({ user }) => {
  const a = await user('maj');
  await expect(a.page.getByRole('link', { name: 'Explorer les serveurs publics' })).toBeVisible();
  await expect(a.page.getByRole('button', { name: /Mise à jour/ })).toHaveCount(0);
  await a.page.getByRole('button', { name: 'Mon compte' }).click();
  await expect(a.page.getByRole('button', { name: /Mettre à jour/ })).toHaveCount(0);
});

test('dans l’appli : la tuile s’atteint au clavier et installe avec progression', async ({ user }) => {
  const a = await user('maj');
  await fakeDesktop(a);
  await expect(tile(a)).toBeVisible();
  await expect(tile(a)).toHaveAttribute('title', 'Mise à jour v9.9.9 disponible');
  await a11y(a.page);

  // Tab depuis la dernière tuile de serveurs.
  await a.page.getByRole('link', { name: 'Explorer les serveurs publics' }).focus();
  await a.page.keyboard.press('Tab');
  await expect(tile(a)).toBeFocused();
  await a.page.keyboard.press('Enter');
  await expect(card(a)).toBeVisible();
  await expect(card(a).getByText('KRAKEN v9.9.9 est disponible')).toBeVisible();
  await expect(card(a).getByRole('link', { name: 'Nouveautés' })).toHaveAttribute('href', /releases\/tag\/v9\.9\.9$/);
  await a11y(a.page);

  // Échap referme la carte et rend le focus à la tuile ; la tuile reste.
  await a.page.keyboard.press('Escape');
  await expect(card(a)).toHaveCount(0);
  await expect(tile(a)).toBeFocused();
  await a.page.keyboard.press('Enter');

  const install = card(a).getByRole('button', { name: 'Installer et redémarrer' });
  await install.focus();
  await a.page.keyboard.press('Enter');
  const bar = card(a).getByRole('progressbar');
  await expect(bar).toHaveAttribute('aria-valuenow', '42');
  await expect(card(a).getByText('Téléchargement… 42 %')).toBeVisible();
  await expect(install).toBeDisabled();
  await expect(card(a).getByRole('button', { name: 'Plus tard' })).toBeDisabled();
  await expect(card(a).getByText('Redémarrage…')).toBeVisible();
  expect(await calls(a)).toContain('update_install');
});

test('une erreur s’affiche, la tuile reste, « Réessayer » relance', async ({ user }) => {
  const a = await user('maj');
  await fakeDesktop(a);
  await a.page.evaluate(() => ((window as unknown as { __fail: string }).__fail = 'La mise à jour n’a pas pu s’installer.'));
  await tile(a).click();
  await card(a).getByRole('button', { name: 'Installer et redémarrer' }).click();
  await expect(card(a).getByRole('alert')).toHaveText('La mise à jour n’a pas pu s’installer.');
  await expect(tile(a)).toBeVisible();
  await a.page.evaluate(() => ((window as unknown as { __fail: null }).__fail = null));
  await card(a).getByRole('button', { name: 'Réessayer' }).click();
  await expect(card(a).getByRole('progressbar')).toBeVisible();
  await expect(card(a).getByText('Redémarrage…')).toBeVisible();
});

test('sans paquet signé : « Télécharger » ouvre le téléchargement, sans barre', async ({ user }) => {
  const a = await user('maj');
  await fakeDesktop(a, 'download');
  await tile(a).click();
  await expect(card(a).getByRole('button', { name: 'Installer et redémarrer' })).toHaveCount(0);
  await card(a).getByRole('button', { name: 'Télécharger' }).click();
  await expect.poll(() => calls(a)).toContain('update_install');
  await expect(card(a)).toHaveCount(0);
});

test('« Mon compte » propose aussi la mise à jour', async ({ user }) => {
  const a = await user('maj');
  await fakeDesktop(a);
  await a.page.getByRole('button', { name: 'Mon compte' }).click();
  await a.page.getByRole('button', { name: 'Mettre à jour vers v9.9.9' }).click();
  await expect(card(a)).toBeVisible();
  await expect(card(a).getByRole('link', { name: 'Nouveautés' })).toBeFocused();
});
