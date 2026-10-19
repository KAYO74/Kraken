// Captures du README, sur une base neuve chargée avec les données de démo (comptes fictifs).
// Usage : serveur `--seed` sur une base vide, puis `KRAKEN_URL=http://127.0.0.1:8091 node readme-screens.mjs`.
import { chromium } from '@playwright/test';

const base = process.env.KRAKEN_URL || 'http://127.0.0.1:8080';
const out = new URL('../docs/screens/', import.meta.url).pathname;
const browser = await chromium.launch();

async function shoot(viewport, steps) {
  const context = await browser.newContext({ baseURL: base, viewport, locale: 'fr-FR', deviceScaleFactor: 1 });
  const page = await context.newPage();
  await page.goto('/login');
  await page.getByLabel('Pseudo ou e-mail').fill('demo');
  await page.getByLabel('Mot de passe').fill('kraken-demo');
  await page.getByRole('button', { name: 'Se connecter' }).click();
  await page.waitForURL('**/channels/@me');
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
await browser.close();
