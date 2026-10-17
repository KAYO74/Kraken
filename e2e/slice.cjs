// Tranche verticale dans Chromium, deux comptes, deux navigateurs : inscription, création
// d'un serveur, invitation, adhésion, message reçu en direct sans recharger.
// Usage : NODE_PATH=$(npm root -g) node e2e/slice.cjs   (serveur sur KRAKEN_URL)
const { chromium } = require('playwright');
const assert = require('node:assert/strict');

const base = process.env.KRAKEN_URL || 'http://127.0.0.1:8080';
const tag = Date.now().toString(36);

async function register(browser, name, errors) {
  const page = await (await browser.newContext()).newPage();
  page.on('pageerror', (e) => errors.push(`${name}: ${e}`));
  page.on('console', (m) => m.type() === 'error' && errors.push(`${name}: ${m.text()}`));
  await page.goto(`${base}/register`);
  await page.getByLabel('E-mail').fill(`${name}@example.org`);
  await page.getByLabel('Pseudo').fill(name);
  await page.getByLabel('Mot de passe').fill('motdepasse-solide');
  await page.getByRole('button', { name: 'Créer mon compte' }).click();
  await page.waitForURL('**/channels/@me');
  return page;
}

(async () => {
  const browser = await chromium.launch();
  const errors = [];
  const a = await register(browser, `alba_${tag}`, errors);
  const b = await register(browser, `bruno_${tag}`, errors);

  // A crée un serveur et copie le lien d'invitation.
  await a.getByRole('link', { name: 'Créer ou rejoindre un serveur' }).click();
  await a.getByLabel('Nom du serveur').fill('Fosse des Mariannes');
  await a.getByRole('button', { name: 'Créer' }).click();
  await a.waitForURL(/\/channels\/[0-9a-f-]{36}\/[0-9a-f-]{36}$/);
  await a.getByRole('heading', { name: 'général' }).waitFor();
  await a.getByRole('button', { name: 'Inviter des amis' }).click();
  const link = await a.getByLabel('Lien à partager').inputValue();
  assert.match(link, /\/invite\/[A-Za-z0-9]{8}$/);
  await a.getByRole('button', { name: 'Fermer' }).click();

  // B ouvre le lien et rejoint.
  await b.goto(link);
  await b.getByRole('heading', { name: 'Fosse des Mariannes' }).waitFor();
  await b.getByText('1 membre').waitFor();
  await b.getByRole('button', { name: 'Rejoindre le serveur' }).click();
  await b.getByRole('heading', { name: 'général' }).waitFor();

  // A écrit ; B le voit sans recharger, et inversement.
  await a.getByLabel('Écrire dans #général').fill('Bienvenue au fond !');
  await a.keyboard.press('Enter');
  await b.getByText('Bienvenue au fond !').waitFor({ timeout: 5000 });
  await b.getByLabel('Écrire dans #général').fill('Merci, il fait sombre ici.');
  await b.getByRole('button', { name: 'Envoyer' }).click();
  await a.getByText('Merci, il fait sombre ici.').waitFor({ timeout: 5000 });

  // A voit B dans la liste des membres (MEMBER_ADD).
  await a.getByRole('complementary', { name: 'Membres' }).getByText(`bruno_${tag}`).waitFor();

  await browser.close();
  assert.deepEqual(errors, [], 'aucune erreur console');
  console.log('tranche verticale : OK');
})().catch((e) => { console.error(e); process.exit(1); });
