// F04 Contrôle à distance (S18) : celui qui regarde un écran partagé demande à le contrôler. Dans
// un navigateur, la demande est refusée d'office (il faut l'appli de bureau) ; dans l'appli de
// bureau, simulée ici par un faux `__TAURI_INTERNALS__.invoke`, la demande est acceptée, les
// actions arrivent à l'appli, et le contrôle s'arrête par Échap, par le bouton, par l'appli ou
// avec le partage. La fenêtre native et la saisie simulée, elles, sont testées dans l'appli.
import { test, expect, a11y, API, type User } from '../fixtures';
import type { Page } from '@playwright/test';

test.use({ launchOptions: { args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'] } });

const bar = (page: Page) => page.getByRole('group', { name: "Commandes de l'appel" });
const panel = (page: Page) => page.getByRole('region', { name: 'Contrôle à distance de ton écran' });

type Call = { cmd: string; args: Record<string, unknown> };

/** L'appli de bureau, vue de la page : `invoke` note chaque appel ; `remote_begin` accepte. */
async function fakeDesktop(u: User) {
  await u.context.addInitScript(() => {
    const w = window as unknown as Record<string, unknown>;
    const calls: Call[] = [];
    w.__calls = calls;
    w.__TAURI_INTERNALS__ = {
      invoke: (cmd: string, args: Record<string, unknown>) => {
        calls.push({ cmd, args: JSON.parse(JSON.stringify(args ?? {})) });
        if (cmd === 'remote_begin') {
          const refuse = w.__refuse as object | undefined;
          return refuse ? Promise.reject(refuse) : Promise.resolve(`jeton-${calls.length}`);
        }
        return Promise.resolve(null);
      },
    };
  });
}

const calls = (u: User): Promise<Call[]> => u.page.evaluate(() => (window as unknown as { __calls: Call[] }).__calls);

/** Deux comptes dans le salon vocal d'un serveur neuf ; `a` partage son écran, `b` le regarde. */
async function setup(user: (p?: string) => Promise<User>, api: (t?: string) => Promise<import('@playwright/test').APIRequestContext>, desktop: boolean) {
  const a = await user('pilote');
  const b = await user('copilote');
  if (desktop) await fakeDesktop(a);
  const owner = await api(a.token);
  const g = await (await owner.post(`${API}/guilds`, { data: { name: 'Poste de pilotage' } })).json();
  const text = g.channels.find((c: { kind: string }) => c.kind === 'text').id;
  const inv = await (await owner.post(`${API}/channels/${text}/invites`, { data: {} })).json();
  expect((await (await api(b.token)).post(`${API}/invites/${inv.code}`)).status()).toBe(200);
  for (const u of [a, b]) {
    await u.page.goto(`/channels/${g.guild.id}/${text}`);
    await u.page.getByRole('link', { name: 'salon vocal Général', exact: true }).click();
    await u.page.getByRole('button', { name: 'Rejoindre', exact: true }).click();
    await expect(u.page.getByRole('region', { name: 'Connexion vocale' }).getByRole('status')).toHaveText('En appel', {
      timeout: 20_000,
    });
  }
  await bar(a.page).getByRole('button', { name: 'Partager ton écran' }).click();
  await expect(bar(a.page).getByRole('button', { name: 'Arrêter le partage' })).toBeVisible({ timeout: 15_000 });
  await b.page.getByRole('button', { name: `Regarder l'écran de ${a.name}` }).click();
  const screen = b.page.getByLabel(`Écran partagé par ${a.name}`);
  await expect.poll(() => screen.evaluate((v: HTMLVideoElement) => v.videoWidth), { timeout: 20_000 }).toBeGreaterThan(0);
  const ask = b.page.getByRole('button', { name: `Demander le contrôle de l'écran de ${a.name}` });
  return { a, b, screen, ask };
}

test('F04-RC1 depuis un navigateur, l’écran ne peut pas être contrôlé', async ({ user, api }) => {
  test.setTimeout(90_000);
  const { a, b, ask } = await setup(user, api, false);
  await ask.click();
  await expect(b.page.getByText(`${a.name} partage depuis un navigateur`)).toBeVisible();
  await expect(ask).toBeVisible();
  await expect(panel(a.page)).toContainText(`${b.name} a demandé à contrôler ton écran`);
  await expect(panel(a.page)).toContainText("n'est possible que depuis l'appli de bureau KRAKEN");
  await expect(panel(a.page).getByRole('button', { name: 'Accepter' })).toHaveCount(0);
  await a11y(a.page);
  await a11y(b.page);
  await panel(a.page).getByRole('button', { name: 'Masquer ce message' }).click();
  await expect(panel(a.page)).toHaveCount(0);
});

test('F04-RC2 appli de bureau : accepter, contrôler, relâcher, arrêter, refuser', async ({ user, api }) => {
  test.setTimeout(120_000);
  const { a, b, screen, ask } = await setup(user, api, true);
  const stage = b.page.locator('figure.app-stage');
  const hint = b.page.getByText(`Tu contrôles l'écran de ${a.name}. Échap pour relâcher`);
  const request = panel(a.page).getByRole('alert').filter({ hasText: `${b.name} demande à contrôler ton écran` });
  const accept = async () => {
    await ask.click();
    await expect(stage.getByRole('status').filter({ hasText: 'Demande envoyée…' })).toBeVisible();
    await expect(request).toBeVisible();
    await request.getByRole('button', { name: 'Accepter' }).click();
    await expect(hint).toBeVisible();
    await expect(panel(a.page)).toContainText(`${b.name} contrôle ton écran.`);
  };

  // B demande, A accepte : l'appli a été consultée avec le nom et la taille de l'image.
  await accept();
  await a11y(a.page);
  await a11y(b.page);
  const begin = (await calls(a)).find((c) => c.cmd === 'remote_begin')!;
  expect(begin.args.name).toBe(b.name);
  expect(begin.args.width).toBeGreaterThan(0);
  expect(begin.args.height).toBeGreaterThan(0);

  // Souris et clavier sur l'image : les actions arrivent chez A, dans l'ordre, avec le jeton.
  const box = (await screen.boundingBox())!;
  await b.page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await b.page.mouse.down();
  await b.page.mouse.up();
  await b.page.keyboard.press('a');
  await b.page.mouse.wheel(0, 300);
  await expect
    .poll(async () => (await calls(a)).filter((c) => c.cmd === 'remote_input').flatMap((c) => c.args.events as object[]))
    .toEqual(
      expect.arrayContaining([
        { k: 'button', button: 'left', down: true },
        { k: 'button', button: 'left', down: false },
        { k: 'key', code: 'KeyA', down: true },
        { k: 'key', code: 'KeyA', down: false },
        { k: 'wheel', dx: 0, dy: 3 },
      ]),
    );
  const inputs = (await calls(a)).filter((c) => c.cmd === 'remote_input');
  expect(inputs.every((c) => c.args.token === inputs[0].args.token)).toBe(true);
  const moves = inputs.flatMap((c) => c.args.events as { k: string; x: number; y: number }[]).filter((e) => e.k === 'move');
  expect(moves.length).toBeGreaterThan(0);
  for (const m of moves) {
    expect(m.x).toBeGreaterThanOrEqual(0);
    expect(m.x).toBeLessThanOrEqual(1);
  }
  expect(Math.abs(moves[0].x - 0.5)).toBeLessThan(0.05);

  // Échap rend la main : B regarde toujours, A n'est plus contrôlé et l'appli rend l'accord.
  await b.page.keyboard.press('Escape');
  await expect(hint).toHaveCount(0);
  await expect(screen).toBeVisible();
  await expect(ask).toBeVisible();
  await expect(panel(a.page)).toHaveCount(0);
  await expect.poll(async () => (await calls(a)).filter((c) => c.cmd === 'remote_end').length).toBe(1);
  // Après Échap, le clavier ne part plus.
  const sent = (await calls(a)).filter((c) => c.cmd === 'remote_input').length;
  await b.page.keyboard.press('b');
  await b.page.waitForTimeout(300);
  expect((await calls(a)).filter((c) => c.cmd === 'remote_input').length).toBe(sent);

  // L'appli retire l'accord (menu « Arrêter le contrôle à distance ») : fin pour les deux.
  await accept();
  await a.page.evaluate(() => window.dispatchEvent(new Event('kraken-remote-ended')));
  await expect(b.page.getByText(`Le contrôle de l'écran de ${a.name} a pris fin.`)).toBeVisible();
  await expect(hint).toHaveCount(0);
  await expect(panel(a.page)).toHaveCount(0);

  // A arrête depuis son panneau.
  await accept();
  await panel(a.page).getByRole('button', { name: 'Arrêter le contrôle' }).click();
  await expect(hint).toHaveCount(0);
  await expect(b.page.getByText(`Le contrôle de l'écran de ${a.name} a pris fin.`)).toBeVisible();

  // B arrête depuis la légende de l'image.
  await accept();
  await stage.getByRole('button', { name: 'Arrêter le contrôle' }).click();
  await expect(hint).toHaveCount(0);
  await expect(panel(a.page)).toHaveCount(0);

  // L'appli ne peut pas (Wayland…) : B l'apprend, A voit pourquoi.
  await a.page.evaluate(() => {
    (window as unknown as Record<string, unknown>).__refuse = {
      reason: 'unavailable',
      message: "Le contrôle à distance n'est pas disponible sous Wayland.",
    };
  });
  await ask.click();
  await request.getByRole('button', { name: 'Accepter' }).click();
  await expect(b.page.getByText(`n'est pas possible sur l'ordinateur de ${a.name}`)).toBeVisible();
  await expect(panel(a.page)).toContainText("n'est pas disponible sous Wayland");
  await a.page.evaluate(() => delete (window as unknown as Record<string, unknown>).__refuse);
  await panel(a.page).getByRole('button', { name: 'Masquer ce message' }).click();

  // A refuse : B l'apprend.
  await ask.click();
  await request.getByRole('button', { name: 'Refuser' }).click();
  await expect(b.page.getByText(`${a.name} a refusé ta demande de contrôle.`)).toBeVisible();
  await expect(panel(a.page)).toHaveCount(0);

  // Le partage s'arrête pendant le contrôle : fin pour les deux.
  await b.page.getByRole('button', { name: 'Masquer ce message' }).first().click();
  await b.page.waitForTimeout(5_200); // délai après un refus
  await accept();
  await bar(a.page).getByRole('button', { name: 'Arrêter le partage' }).click();
  await expect(screen).toHaveCount(0);
  await expect(b.page.getByText(`Le contrôle de l'écran de ${a.name} a pris fin.`)).toBeVisible();
  await expect(panel(a.page)).toHaveCount(0);
  await expect.poll(async () => (await calls(a)).filter((c) => c.cmd === 'remote_end').length).toBeGreaterThanOrEqual(4);
});
