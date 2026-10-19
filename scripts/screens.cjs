// Captures du clone pour /replica-diff, au viewport CSS des captures de référence (1728×1117),
// plus une vue mobile (390 px). Serveur lancé avec --seed.
// Usage : NODE_PATH=$(npm root -g) node scripts/screens.cjs [dossier]
const { chromium } = require('playwright');

const base = process.env.KRAKEN_URL || 'http://127.0.0.1:8080';
const out = process.argv[2] || 'replica/clone-screens';

(async () => {
  const browser = await chromium.launch();
  const ctx = await browser.newContext({ viewport: { width: 1728, height: 1117 } });
  const page = await ctx.newPage();
  const errors = [];
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
  page.on('pageerror', (e) => errors.push(String(e)));
  const shot = (id) => page.screenshot({ path: `${out}/${id}.png` });

  await page.goto(`${base}/channels/@me`);
  await page.waitForURL('**/login**');
  await shot('S28');
  await page.fill('input[name=login]', 'demo');
  await page.fill('input[name=password]', 'kraken-demo');
  await page.click('button[type=submit]');
  await page.waitForURL('**/channels/@me');
  await page.getByRole('tab', { name: 'Tous' }).click();
  await page.getByText('Navigatrice des fonds abyssaux').first().waitFor();
  await shot('S01');

  await page.getByRole('link', { name: 'Serveur Abysse' }).click();
  await page.getByText('Bienvenue dans Abysse').waitFor();
  await page.getByRole('link', { name: 'général' }).click();
  await page.getByText('Parfait, à ce soir alors.').waitFor();
  await shot('S05');

  await page.getByRole('link', { name: 'Accueil et messages privés' }).click();
  await page.getByRole('link', { name: /Lou/ }).first().click();
  await page.getByText("Tu as vu l'invitation pour Abysse ?").waitFor();
  await shot('S19');

  await page.getByRole('link', { name: 'Créer ou rejoindre un serveur' }).click();
  await page.getByRole('dialog').waitFor();
  await shot('S21');

  await page.goto(`${base}/settings/account`);
  await page.getByRole('button', { name: 'Se déconnecter', exact: true }).waitFor();
  await shot('S06');
  await page.getByRole('link', { name: 'Apparence' }).click();
  await page.getByText('Automatique').waitFor();
  await shot('S11');

  // Mobile : salon puis tiroir ouvert.
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole('button', { name: /Fermer les réglages/ }).click();
  await page.getByRole('button', { name: 'Ouvrir la navigation' }).click();
  await page.getByRole('link', { name: 'Serveur Abysse' }).click();
  await page.getByRole('link', { name: 'général' }).click();
  await page.getByText('Parfait, à ce soir alors.').waitFor();
  await shot('S05-mobile');
  await page.getByRole('button', { name: 'Ouvrir la navigation' }).click();
  await page.waitForTimeout(300);
  await shot('S02-mobile');

  await browser.close();
  if (errors.length) {
    console.error('erreurs console :\n' + errors.join('\n'));
    process.exit(1);
  }
  console.log(`captures écrites dans ${out}`);
})().catch((e) => { console.error(e); process.exit(1); });
