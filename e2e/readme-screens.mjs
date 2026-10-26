// Captures du README, sur une base neuve chargée avec les données de démo (comptes fictifs).
// Usage : serveur `--seed` sur une base vide, puis `KRAKEN_URL=http://127.0.0.1:8091 node readme-screens.mjs`.
import { chromium } from '@playwright/test';

const base = process.env.KRAKEN_URL || 'http://127.0.0.1:8080';
const out = new URL('../docs/screens/', import.meta.url).pathname;
// Micro et écran factices de Chromium pour l'appel.
const browser = await chromium.launch({ args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'] });

async function login(viewport, who = 'demo', init) {
  const context = await browser.newContext({ baseURL: base, viewport, locale: 'fr-FR', deviceScaleFactor: 1 });
  if (init) await context.addInitScript(init);
  const page = await context.newPage();
  await page.goto('/login');
  await page.getByLabel('Pseudo ou e-mail').fill(who);
  await page.getByLabel('Mot de passe').fill('kraken-demo');
  await page.getByRole('button', { name: 'Se connecter' }).click();
  await page.waitForURL('**/channels/@me');
  return { context, page };
}

async function shoot(viewport, steps) {
  const { context, page } = await login(viewport);
  await steps(page);
  await context.close();
}

let channel = '/channels/@me';
const snap = async (page, name) => {
  await page.mouse.move(page.viewportSize().width - 1, page.viewportSize().height - 1);
  await page.waitForTimeout(400);
  await page.screenshot({ path: `${out}${name}.png` });
};

await shoot({ width: 1440, height: 900 }, async (page) => {
  await snap(page, 'accueil');
  await page.getByRole('link', { name: 'Abysse' }).first().click();
  await page.getByRole('link', { name: 'général' }).click();
  await page.getByRole('heading', { name: 'général', level: 1 }).waitFor();
  await snap(page, 'salon');
  channel = page.url();
  await page.goto('/settings/appearance');
  await page.getByRole('dialog', { name: 'Réglages' }).waitFor();
  await snap(page, 'reglages');
});
await shoot({ width: 390, height: 844 }, async (page) => {
  await page.goto(channel);
  await page.getByRole('heading', { name: 'général', level: 1 }).waitFor();
  await snap(page, 'mobile-salon');
});

// Appel : Démo partage son écran depuis l'appli de bureau (simulée : `remote_begin` accepte),
// Lou le regarde puis en prend le contrôle.
const desktop = () => {
  window.__TAURI_INTERNALS__ = { invoke: (cmd) => Promise.resolve(cmd === 'remote_begin' ? 'jeton' : null) };
};
const demo = await login({ width: 1440, height: 900 }, 'demo', desktop);
const lou = await login({ width: 1440, height: 900 }, 'lou');
const bar = (page) => page.getByRole('group', { name: "Commandes de l'appel" });
for (const { page } of [demo, lou]) {
  await page.goto(channel);
  await page.getByRole('link', { name: 'salon vocal Salle vocale', exact: true }).click();
  await page.getByRole('button', { name: 'Rejoindre', exact: true }).click();
  await page.getByRole('region', { name: 'Connexion vocale' }).getByText('En appel').waitFor({ timeout: 20_000 });
}
await bar(demo.page).getByRole('button', { name: 'Partager ton écran' }).click();
await bar(demo.page).getByRole('button', { name: 'Arrêter le partage' }).waitFor({ timeout: 15_000 });
await lou.page.getByRole('button', { name: "Regarder l'écran de Démo" }).click();
await lou.page.waitForFunction(() => [...document.querySelectorAll('video')].some((v) => v.videoWidth > 0), null, { timeout: 20_000 });
await snap(lou.page, 'appel');
await lou.page.getByRole('button', { name: "Demander le contrôle de l'écran de Démo" }).click();
const request = demo.page.getByRole('region', { name: 'Contrôle à distance de ton écran' }).getByRole('alert');
await request.waitFor();
await snap(demo.page, 'controle-demande');
await request.getByRole('button', { name: 'Accepter' }).click();
await lou.page.getByText("Tu contrôles l'écran de Démo").waitFor();
await lou.page.waitForTimeout(400);
await lou.page.screenshot({ path: `${out}controle.png` });
await demo.context.close();
await lou.context.close();
await browser.close();
