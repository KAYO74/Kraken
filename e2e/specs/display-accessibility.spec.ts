// Réglages d'affichage de l'appareil : Apparence (S11), Accessibilité (S12), Langue et heure (S14).
// Chaque choix s'applique tout de suite (sans bouton Enregistrer), reste sur l'appareil, et est posé
// sur <html> par boot.js avant le premier rendu.
import { test, expect, a11y, createGuild, say, messages, watch, PASSWORD, API, unique, type User } from '../fixtures';
import { animatedGif, png } from '../images';
import type { APIRequestContext, Locator, Page } from '@playwright/test';

type Api = (t?: string) => Promise<APIRequestContext>;

const reglages = (p: Page) => p.getByRole('dialog', { name: 'Réglages' });
const group = (p: Page, name: string) => p.getByRole('group', { name, exact: true });
const radio = (p: Page, g: string, label: string) => group(p, g).getByRole('radio', { name: label, exact: true });
const attr = (p: Page, name: string) => p.locator('html').getAttribute(name);
const style = (loc: Locator, prop: string) => loc.evaluate((el, p) => getComputedStyle(el).getPropertyValue(p), prop);
const members = (p: Page) => p.getByRole('complementary', { name: 'Membres' });
/** Le zoom réellement appliqué (`--zoom` sur <html>). */
const appliedZoom = async (p: Page) => parseFloat(await style(p.locator('html'), '--zoom'));
/** Le zoom attendu : le choix, plafonné pour que la page garde la largeur du point de rupture (1024 px, puis 640 px) ; 1 sous 640 px. */
const capped = (choice: number, width: number) => (width < 640 ? 1 : Math.min(choice, width / (width >= 1024 ? 1024 : 640)));

/** Choisit un bouton radio et vérifie qu'il est coché tout de suite. */
async function pick(p: Page, g: string, label: string) {
  await radio(p, g, label).check();
  await expect(radio(p, g, label)).toBeChecked();
}

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

const noScroll = async (p: Page) =>
  expect(await p.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(p.viewportSize()!.width);

/** Couleur de texte et couleur de fond effective (le premier fond non transparent en remontant) d'un élément. */
const colors = (loc: Locator) =>
  loc.evaluate((el) => {
    let bg = 'rgba(0, 0, 0, 0)';
    for (let n: Element | null = el; n; n = n.parentElement) {
      const c = getComputedStyle(n).backgroundColor;
      if (!c.startsWith('rgba(0, 0, 0, 0')) {
        bg = c;
        break;
      }
    }
    return { fg: getComputedStyle(el).color, bg };
  });

/** Des images (une photo de profil animée, par exemple) mises sur le compte par l'API. */
async function put(api: Api, u: User, what: 'avatar' | 'banner', body: Buffer, mime: string) {
  const res = await (await api(u.token)).put(`${API}/users/@me/${what}`, { headers: { 'content-type': mime }, data: body });
  expect(res.status(), await res.text()).toBe(200);
}

// ───────── S11 : Apparence ─────────

test('S11-H1 thème : Automatique (système) d’abord, appliqué tout de suite, gardé après rechargement', async ({ user }) => {
  const { page } = await user('thema');
  await page.goto('/settings/appearance');
  await expect(page.getByRole('heading', { name: 'Apparence', level: 2 })).toBeVisible();
  await a11y(page);
  // L'aperçu est tout en haut, avant les choix ; pas de bouton Enregistrer.
  const preview = page.getByRole('region', { name: 'Aperçu' });
  await expect(preview).toBeVisible();
  await expect(preview).toContainText('Nacre');
  await expect(preview).toContainText('Récif');
  await expect(page.getByRole('button', { name: 'Enregistrer' })).toHaveCount(0);
  expect((await preview.boundingBox())!.y).toBeLessThan((await group(page, 'Thème').boundingBox())!.y);

  expect(await group(page, 'Thème').getByRole('radio').evaluateAll((r) => r.map((x) => x.getAttribute('aria-labelledby') && document.getElementById(x.getAttribute('aria-labelledby')!)!.textContent)))
    .toEqual(['Automatique (système)', 'Sombre', 'Clair', 'Noir']);
  await expect(radio(page, 'Thème', 'Sombre')).toBeChecked();

  const bg = () => page.evaluate(() => getComputedStyle(document.body).backgroundColor);
  const dark = await bg();
  await pick(page, 'Thème', 'Clair');
  expect(await attr(page, 'data-theme')).toBe('light');
  const light = await bg();
  expect(light).not.toBe(dark);
  // L'aperçu suit : son fond change avec le thème.
  expect(await style(preview, 'background-color')).toBe('rgb(255, 255, 255)');
  await pick(page, 'Thème', 'Noir');
  expect(await attr(page, 'data-theme')).toBe('black');
  expect(await bg()).toBe('rgb(0, 0, 0)');

  // Automatique : suit le système, et le suit encore quand il change.
  await pick(page, 'Thème', 'Automatique (système)');
  expect(await attr(page, 'data-theme')).toBe('auto');
  await page.emulateMedia({ colorScheme: 'light' });
  expect(await bg()).toBe(light);
  await page.emulateMedia({ colorScheme: 'dark' });
  expect(await bg()).toBe(dark);

  await pick(page, 'Thème', 'Clair');
  await page.reload();
  await expect(radio(page, 'Thème', 'Clair')).toBeChecked();
  expect(await attr(page, 'data-theme')).toBe('light');
  await a11y(page);
});

test('S11-H2 taille du texte : 14, 16, 18 ou 20 px, appliquée tout de suite à toute l’interface et à l’aperçu', async ({ user }) => {
  const { page } = await user('texta');
  await page.goto('/settings/appearance');
  const message = page.getByRole('region', { name: 'Aperçu' }).locator('p').first();
  const title = page.getByRole('heading', { name: 'Apparence', level: 2 });
  await expect(radio(page, 'Taille du texte', 'Normal')).toBeChecked();
  expect(await style(message, 'font-size')).toBe('16px');
  const h2 = parseFloat(await style(title, 'font-size'));
  for (const [label, size, value] of [['Petit', '14px', '14'], ['Grand', '18px', '18'], ['Très grand', '20px', '20']]) {
    await pick(page, 'Taille du texte', label);
    expect(await attr(page, 'data-text')).toBe(value);
    expect(await style(message, 'font-size')).toBe(size);
    // Les titres suivent le même multiplicateur.
    expect(parseFloat(await style(title, 'font-size'))).toBeCloseTo((h2 * Number(value)) / 16, 1);
  }
  await a11y(page);
  await page.reload();
  await expect(radio(page, 'Taille du texte', 'Très grand')).toBeChecked();
  expect(await style(message, 'font-size')).toBe('20px');
  await pick(page, 'Taille du texte', 'Normal');
  expect(await attr(page, 'data-text')).toBeNull();
  expect(await style(message, 'font-size')).toBe('16px');
});

test('S11-H3 les flèches du clavier changent de choix et l’appliquent tout de suite', async ({ user }) => {
  const { page } = await user('clavia');
  await page.goto('/settings/appearance');
  await radio(page, 'Taille du texte', 'Normal').focus();
  await page.keyboard.press('ArrowDown');
  await expect(radio(page, 'Taille du texte', 'Grand')).toBeChecked();
  await expect(radio(page, 'Taille du texte', 'Grand')).toBeFocused();
  expect(await attr(page, 'data-text')).toBe('18');
  await page.keyboard.press('ArrowRight');
  await expect(radio(page, 'Taille du texte', 'Très grand')).toBeChecked();
  await page.keyboard.press('ArrowUp');
  await page.keyboard.press('ArrowUp');
  await expect(radio(page, 'Taille du texte', 'Normal')).toBeChecked();
  expect(await attr(page, 'data-text')).toBeNull();
  // Tab quitte le groupe (un seul arrêt par groupe) pour le zoom.
  await page.keyboard.press('Tab');
  await expect(page.getByLabel("Zoom de l'interface")).toBeFocused();
  // Le focus se voit : un anneau d'au moins 2 px.
  await radio(page, 'Densité', 'Confortable').focus();
  await page.keyboard.press('ArrowDown');
  await expect(radio(page, 'Densité', 'Compacte')).toBeChecked();
  expect(parseFloat(await style(radio(page, 'Densité', 'Compacte'), 'outline-width'))).toBeGreaterThanOrEqual(2);
  expect(await attr(page, 'data-density')).toBe('compact');
});

test('S11-H4 zoom de l’interface : 90 à 150 % (plafonné à la largeur de la fenêtre), tout tient encore dans la fenêtre', async ({ user }) => {
  const { page } = await user('zooma');
  await page.goto('/settings/appearance');
  const zoom = page.getByLabel("Zoom de l'interface");
  await expect(zoom).toHaveValue('100');
  expect(await zoom.locator('option').allTextContents()).toEqual(['90 %', '100 %', '110 %', '125 %', '150 %']);
  const { width, height } = page.viewportSize()!;
  for (const z of ['90', '110', '125', '150']) {
    await zoom.selectOption(z);
    expect(await attr(page, 'data-zoom')).toBe(z);
    // À 1440 px, 150 % est ramené à 1440 / 1024 : le choix reste 150, le zoom appliqué est plafonné.
    const expected = capped(Number(z) / 100, width);
    expect(parseFloat(await style(page.locator('body'), 'zoom'))).toBeCloseTo(expected, 2);
    expect(await appliedZoom(page)).toBeCloseTo(expected, 2);
    // La fenêtre des réglages reste entière à l'écran, la page ne déborde pas.
    const box = (await reglages(page).boundingBox())!;
    expect(box.x).toBeGreaterThanOrEqual(0);
    expect(box.y).toBeGreaterThanOrEqual(0);
    expect(box.x + box.width).toBeLessThanOrEqual(width + 1);
    expect(box.y + box.height).toBeLessThanOrEqual(height + 1);
    await noScroll(page);
    const app = (await page.locator('.app').boundingBox())!;
    expect(app.height).toBeLessThanOrEqual(height + 1);
  }
  await a11y(page);
  await page.reload();
  await expect(zoom).toHaveValue('150');
  await zoom.selectOption('100');
  expect(await attr(page, 'data-zoom')).toBeNull();
});

test('S11-H5 densité : Compacte resserre les messages (4 px) et réduit les avatars (24 px)', async ({ user }) => {
  const a = await user('densa');
  const { page } = a;
  const [gid, cid] = await createGuild(page, 'Banc');
  await say(page, 'général', 'premier message');
  await expect(messages(page)).toHaveCount(1);
  await page.goto('/settings/appearance');
  const preview = page.getByRole('region', { name: 'Aperçu' });
  const second = preview.locator('.k-msg').nth(1);
  const avatar = preview.locator('.k-avatar').first();
  await expect(radio(page, 'Densité', 'Confortable')).toBeChecked();
  expect([await style(second, 'margin-top'), await style(avatar, 'width')]).toEqual(['16px', '40px']);
  await pick(page, 'Densité', 'Compacte');
  expect(await attr(page, 'data-density')).toBe('compact');
  expect([await style(second, 'margin-top'), await style(avatar, 'width'), await style(avatar, 'height')]).toEqual(['4px', '24px', '24px']);
  await a11y(page);
  // Dans un salon aussi, après rechargement.
  await page.goto(`/channels/${gid}/${cid}`);
  await expect(messages(page)).toHaveCount(1);
  expect(await style(messages(page).first().locator('.k-avatar'), 'width')).toBe('24px');
  await page.goto('/settings/appearance');
  await pick(page, 'Densité', 'Confortable');
  expect(await attr(page, 'data-density')).toBeNull();
});

test('S11-E1 sans stockage local, les réglages s’appliquent quand même (le temps de la session)', async ({ browser, baseURL, api, issues }) => {
  const name = unique('privee');
  await (await api()).post(`${API}/auth/register`, { data: { username: name, email: `${name}@example.org`, password: PASSWORD } });
  const context = await browser.newContext({ baseURL, locale: 'fr-FR' });
  // Navigation privée stricte : toucher au stockage lève une exception.
  await context.addInitScript(() => {
    Object.defineProperty(window, 'localStorage', {
      get() {
        throw new DOMException('Accès refusé', 'SecurityError');
      },
    });
  });
  const page = await context.newPage();
  watch(page, issues);
  await page.goto('/login');
  await page.getByLabel('Pseudo ou e-mail').fill(name);
  await page.getByLabel('Mot de passe').fill(PASSWORD);
  await page.getByRole('button', { name: 'Se connecter' }).click();
  await page.waitForURL('**/channels/@me');
  await page.getByRole('button', { name: /^Mon compte/ }).click();
  await page.getByRole('link', { name: 'Apparence', exact: true }).click();
  await pick(page, 'Taille du texte', 'Très grand');
  await pick(page, 'Thème', 'Clair');
  await pick(page, 'Densité', 'Compacte');
  await expect(page.locator('html')).toHaveAttribute('data-text', '20');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  // Le choix tient d'une page de réglages à l'autre, sans recharger.
  await page.getByRole('link', { name: 'Accessibilité', exact: true }).click();
  await pick(page, 'Mouvement', 'Réduit');
  await page.getByRole('link', { name: 'Apparence', exact: true }).click();
  await expect(radio(page, 'Taille du texte', 'Très grand')).toBeChecked();
  await expect(radio(page, 'Thème', 'Clair')).toBeChecked();
  await expect(page.locator('html')).toHaveAttribute('data-motion', 'reduce');
  await context.close();
});

test('S11-E2 boot.js pose les réglages avant le premier rendu ; une valeur inconnue est ignorée', async ({ browser, baseURL, allow, issues }) => {
  allow.push(/wasm|kraken-client|Failed to fetch|ERR_FAILED|dynamically imported|Failed to load/i);
  const context = await browser.newContext({ baseURL, locale: 'fr-FR', reducedMotion: 'reduce', contrast: 'more' });
  await context.addInitScript(() => {
    const set = (k: string, v: string) => localStorage.setItem(k, v);
    set('kraken.theme', 'light');
    set('kraken.text', '20');
    set('kraken.zoom', '125');
    set('kraken.density', 'compact');
    set('kraken.underline', 'on');
    set('kraken.motion', 'auto');
    set('kraken.contrast', 'auto');
  });
  const page = await context.newPage();
  watch(page, issues);
  // Le client WASM ne démarre pas : seuls les attributs de boot.js peuvent expliquer ce qu'on lit.
  await page.route('**/*.wasm', (r) => r.abort());
  await page.goto('/login');
  const html = page.locator('html');
  await expect(html).toHaveAttribute('data-theme', 'light');
  await expect(html).toHaveAttribute('data-text', '20');
  await expect(html).toHaveAttribute('data-zoom', '125');
  await expect(html).toHaveAttribute('data-density', 'compact');
  await expect(html).toHaveAttribute('data-underline-links', '');
  // « Automatique » suit le système (ici : mouvement réduit et contraste plus élevé).
  await expect(html).toHaveAttribute('data-motion', 'reduce');
  await expect(html).toHaveAttribute('data-contrast', 'high');
  expect(await style(page.locator('body'), 'zoom')).toBe('1.25');
  await context.close();

  const other = await browser.newContext({ baseURL, locale: 'fr-FR' });
  await other.addInitScript(() => {
    localStorage.setItem('kraken.theme', 'rose');
    localStorage.setItem('kraken.text', '99');
    localStorage.setItem('kraken.zoom', '<script>');
    localStorage.setItem('kraken.contrast', 'high');
    localStorage.setItem('kraken.motion', 'normal');
  });
  const bad = await other.newPage();
  watch(bad, issues);
  await bad.route('**/*.wasm', (r) => r.abort());
  await bad.goto('/login');
  const root = bad.locator('html');
  await expect(root).toHaveAttribute('data-theme', 'dark');
  await expect(root).not.toHaveAttribute('data-text', /.*/);
  await expect(root).not.toHaveAttribute('data-zoom', /.*/);
  await expect(root).toHaveAttribute('data-contrast', 'high');
  await expect(root).toHaveAttribute('data-motion', 'normal');
  await other.close();
});

test('S11-E3 le client reprend ce que boot.js a posé : rien ne clignote, les réglages du système suivent en direct', async ({ user }) => {
  const { page, context } = await user('bootia');
  await context.addInitScript(() => {
    if (!sessionStorage.getItem('seeded')) {
      sessionStorage.setItem('seeded', '1');
      localStorage.setItem('kraken.text', '18');
      localStorage.setItem('kraken.contrast', 'auto');
      localStorage.setItem('kraken.motion', 'auto');
    }
  });
  await page.reload();
  await expect(page.getByRole('heading', { name: 'Amis', level: 1 })).toBeVisible();
  await expect(page.locator('html')).toHaveAttribute('data-text', '18');
  await expect(page.locator('html')).not.toHaveAttribute('data-contrast', 'high');
  await page.emulateMedia({ contrast: 'more', reducedMotion: 'reduce' });
  await expect(page.locator('html')).toHaveAttribute('data-contrast', 'high');
  await expect(page.locator('html')).toHaveAttribute('data-motion', 'reduce');
  await page.emulateMedia({ contrast: 'no-preference', reducedMotion: 'no-preference' });
  await expect(page.locator('html')).not.toHaveAttribute('data-contrast', 'high');
  await expect(page.locator('html')).toHaveAttribute('data-motion', 'normal');
});

test('S11-H6 zoom plafonné : à 1440 px et 150 %, la page garde la mise en page large, sans écraser la colonne du salon', async ({ user }) => {
  const { page } = await user('zoomlarge');
  const [gid, cid] = await createGuild(page, 'Banc');
  await page.evaluate(() => localStorage.setItem('kraken.zoom', '150'));
  await page.goto(`/channels/${gid}/${cid}`);
  await expect(page.getByLabel(/^Écrire dans/)).toBeVisible();
  const zoom = await appliedZoom(page);
  expect(zoom).toBeLessThanOrEqual(1440 / 1024 + 0.001);
  expect(zoom).toBeGreaterThan(1.4);
  expect(parseFloat(await style(page.locator('body'), 'zoom'))).toBeCloseTo(zoom, 3);
  // La page zoomée est au moins aussi large que le point de rupture du bureau : la liste des membres reste affichée.
  expect(1440 / zoom).toBeGreaterThanOrEqual(1023.9);
  await expect(members(page)).toBeVisible();
  await noScroll(page);
  // La colonne du salon n'est pas plus étroite que dans une fenêtre de 1024 px sans zoom.
  const main = async (p: Page) => (await p.locator('#main').evaluate((m) => m.getBoundingClientRect().width)) / (await appliedZoom(p));
  const wide = await main(page);
  const ref = await page.context().newPage();
  await ref.setViewportSize({ width: 1024, height: 768 });
  await ref.goto(`/channels/${gid}/${cid}`);
  await expect(ref.getByLabel(/^Écrire dans/)).toBeVisible();
  expect(await appliedZoom(ref)).toBe(1);
  expect(wide).toBeGreaterThanOrEqual((await main(ref)) - 2);
  await ref.close();
  // Les réglages gardent le choix de l'utilisateur : 150 %.
  await page.goto('/settings/appearance');
  await expect(page.getByLabel("Zoom de l'interface")).toHaveValue('150');
  await expect(page.getByLabel("Zoom de l'interface")).toBeEnabled();
  await expect(page.getByText('Le zoom est limité par la largeur de la fenêtre.')).toBeVisible();
  await expect(page.getByLabel("Zoom de l'interface")).toHaveAccessibleDescription(/limité par la largeur de la fenêtre/);
  await a11y(page);
});

test('S11-H7 zoom plafonné : recalculé quand la fenêtre change de taille, gelé sous 640 px', async ({ user }) => {
  const { page } = await user('zoomresize');
  await page.goto('/settings/appearance');
  const select = page.getByLabel("Zoom de l'interface");
  await select.selectOption('150');
  for (const [width, height] of [[1100, 800], [1023, 800], [900, 800], [700, 800], [639, 800], [390, 800], [1440, 900]]) {
    await page.setViewportSize({ width, height });
    await expect.poll(() => appliedZoom(page)).toBeCloseTo(capped(1.5, width), 2);
    expect(await attr(page, 'data-zoom')).toBe('150');
    // Sous 640 px le zoom est gelé : la liste est désactivée et la page est à sa taille d'origine.
    if (width < 640) await expect(select).toBeDisabled();
    else await expect(select).toBeEnabled();
    await noScroll(page);
  }
  // Le choix est gardé : de retour à 1440 px, le plafond de cette largeur s'applique à nouveau.
  expect(await appliedZoom(page)).toBeCloseTo(1440 / 1024, 2);
  await expect(select).toHaveValue('150');
});

/**
 * Texte à 20 px et zoom à 150 % (plafonné par la largeur de la fenêtre) : les fenêtres de dialogue sont plus hautes que
 * l'écran. Chacune doit tenir dans l'écran (elle défile), et son dernier bouton doit se cliquer pour de vrai : on le fait
 * défiler dans la vue puis on clique, sans `force`.
 */
async function dialogsReachable(f: { user: (p?: string) => Promise<User>; api: Api }) {
  const owner = await f.user('fenetres');
  const { page } = owner;
  const res = await (await f.api(owner.token)).post(`${API}/guilds`, { data: { name: 'Poche' } });
  expect(res.status(), await res.text()).toBe(201);
  const g = await res.json();
  const cid = g.channels.find((c: { kind: string }) => c.kind === 'text').id;
  await page.evaluate(() => {
    localStorage.setItem('kraken.zoom', '150');
    localStorage.setItem('kraken.text', '20');
  });
  await page.goto(`/channels/${g.guild.id}/${cid}`);
  await expect(page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
  const { width, height } = page.viewportSize()!;
  expect(await appliedZoom(page)).toBeCloseTo(capped(1.5, width), 2);

  const reach = async (dialog: Locator, last: string) => {
    await expect(dialog).toBeVisible();
    const box = (await dialog.boundingBox())!;
    expect(box.y, 'le haut de la fenêtre est visible').toBeGreaterThanOrEqual(-1);
    expect(box.y + box.height, 'la fenêtre tient dans l’écran').toBeLessThanOrEqual(height + 1);
    const button = dialog.getByRole('button', { name: last, exact: true });
    await expect(button).toBeEnabled();
    await button.scrollIntoViewIfNeeded();
    await expect(button).toBeInViewport({ ratio: 1 });
    await button.click({ timeout: 3000 });
    await expect(dialog).toHaveCount(0);
  };

  // Créer un salon. Sur téléphone la liste des salons est dans le tiroir.
  const menu = page.getByRole('button', { name: 'Ouvrir la navigation' });
  if (await menu.isVisible()) await menu.click();
  await page.getByRole('navigation', { name: 'Salons' }).getByRole('button', { name: 'Créer un salon' }).first().click();
  const create = page.getByRole('dialog', { name: 'Créer un salon' });
  await create.getByLabel('Nom du salon').fill('fenetre');
  await reach(create, 'Créer le salon');
  await expect(page.getByRole('heading', { name: 'fenetre', level: 1 })).toBeVisible();

  // Notifications du salon.
  await page.getByRole('button', { name: 'Notifications du salon' }).click();
  await reach(page.getByRole('dialog', { name: /Notifications/ }), 'Fermer');

  // Recadrer la photo.
  await page.goto('/settings/profiles');
  await page.getByLabel('Changer la photo').setInputFiles({ name: 'photo.png', mimeType: 'image/png', buffer: png(600, 600) });
  await reach(page.getByRole('dialog', { name: 'Recadrer ta photo' }), 'Appliquer');
  // Recadrer la bannière.
  await page.getByLabel('Changer la bannière').setInputFiles({ name: 'banniere.png', mimeType: 'image/png', buffer: png(900, 240) });
  await reach(page.getByRole('dialog', { name: 'Recadrer ta bannière' }), 'Appliquer');
}


test.describe('largeur mobile', () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test('S11-N1 à 390 px les choix s’empilent, l’aperçu reste en haut et rien ne déborde', async ({ user }) => {
    const { page } = await user('mobila');
    await page.goto('/settings/appearance');
    await a11y(page);
    await noScroll(page);
    const preview = page.getByRole('region', { name: 'Aperçu' });
    // Les quatre tailles du texte sont l'une sous l'autre, à la même abscisse et sur toute la largeur.
    const boxes = await group(page, 'Taille du texte').getByRole('radio').evaluateAll((r) => r.map((x) => x.closest('label')!.getBoundingClientRect()));
    expect(new Set(boxes.map((b) => Math.round(b.x))).size).toBe(1);
    boxes.slice(1).forEach((b, i) => expect(b.y).toBeGreaterThan(boxes[i].y));
    expect(boxes[0].width).toBeGreaterThan(300);
    // L'aperçu est le premier bloc, et le reste en haut quand on descend.
    expect((await preview.boundingBox())!.y).toBeLessThan((await group(page, 'Thème').boundingBox())!.y);
    await page.locator('.k-settings main').evaluate((m) => m.scrollTo(0, m.scrollHeight));
    await expect(radio(page, 'Densité', 'Compacte')).toBeInViewport();
    const top = (await preview.boundingBox())!;
    expect(top.y).toBeLessThan(120);
    expect(top.y + top.height).toBeLessThan(400);
    await pick(page, 'Densité', 'Compacte');
    await noScroll(page);
    await pick(page, 'Taille du texte', 'Très grand');
    await noScroll(page);
    await a11y(page);
  });

  test('S11-N2 à 390 px le zoom est gelé : liste désactivée, page à sa taille, même avec 150 % gardé', async ({ user }) => {
    const { page } = await user('mobilz');
    await page.evaluate(() => localStorage.setItem('kraken.zoom', '150'));
    await page.goto('/settings/appearance');
    const select = page.getByLabel("Zoom de l'interface");
    await expect(select).toBeDisabled();
    await expect(select).toHaveValue('150');
    await expect(select).toHaveAccessibleDescription(/limité par la largeur de la fenêtre/);
    expect(await attr(page, 'data-zoom')).toBe('150');
    expect(await appliedZoom(page)).toBe(1);
    expect(await style(page.locator('body'), 'zoom')).toBe('1');
    await noScroll(page);
    await a11y(page);
  });

  test('S11-N3 à 390 px, zoom 150 % et texte 20 px : chaque fenêtre défile et son dernier bouton se clique', async ({ user, api }) => {
    await dialogsReachable({ user, api });
  });
});

test.describe('fenêtre de 1280 × 720', () => {
  test.use({ viewport: { width: 1280, height: 720 } });

  test('S11-N4 à 1280 × 720, zoom 150 % (plafonné) et texte 20 px : chaque fenêtre défile et son dernier bouton se clique', async ({ user, api }) => {
    await dialogsReachable({ user, api });
  });
});

// ───────── S12 : Accessibilité ─────────

test('S12-H1 mouvement : Automatique, Réduit ou Normal ; Réduit coupe les transitions, y compris celles du tiroir', async ({ user }) => {
  const { page } = await user('mouva');
  await page.goto('/settings/accessibility');
  await expect(page.getByRole('heading', { name: 'Accessibilité', level: 2 })).toBeVisible();
  await a11y(page);
  await expect(page.getByRole('region', { name: 'Aperçu' })).toBeVisible();
  expect(await group(page, 'Mouvement').getByRole('radio').evaluateAll((r) => r.map((x) => document.getElementById(x.getAttribute('aria-labelledby')!)!.textContent)))
    .toEqual(['Automatique', 'Réduit', 'Normal']);
  await expect(radio(page, 'Mouvement', 'Automatique')).toBeChecked();
  await expect(page.locator('html')).toHaveAttribute('data-motion', 'normal');
  const button = page.getByRole('button', { name: 'Fermer les réglages (Échap)' });
  expect(parseFloat(await style(button, 'transition-duration'))).toBeGreaterThan(0);

  await pick(page, 'Mouvement', 'Réduit');
  await expect(page.locator('html')).toHaveAttribute('data-motion', 'reduce');
  expect(parseFloat(await style(button, 'transition-duration'))).toBe(0);
  // Automatique suit le système, en direct.
  await pick(page, 'Mouvement', 'Automatique');
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await expect(page.locator('html')).toHaveAttribute('data-motion', 'reduce');
  // Normal garde les animations même quand le système en demande moins.
  await pick(page, 'Mouvement', 'Normal');
  await expect(page.locator('html')).toHaveAttribute('data-motion', 'normal');
  expect(parseFloat(await style(button, 'transition-duration'))).toBeGreaterThan(0);
  await page.reload();
  await expect(radio(page, 'Mouvement', 'Normal')).toBeChecked();
  await a11y(page);
});

test.describe('largeur mobile', () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test('S12-N1 à 390 px : le tiroir ne glisse plus en mouvement réduit, les choix s’empilent', async ({ user }) => {
    const { page } = await user('tiroira');
    await page.goto('/settings/accessibility');
    await a11y(page);
    await noScroll(page);
    const boxes = await group(page, 'Contraste').getByRole('radio').evaluateAll((r) => r.map((x) => x.closest('label')!.getBoundingClientRect()));
    expect(new Set(boxes.map((b) => Math.round(b.x))).size).toBe(1);
    expect(boxes[0].width).toBeGreaterThan(300);
    await pick(page, 'Contraste', 'Élevé');
    await pick(page, 'Mouvement', 'Réduit');
    await noScroll(page);
    await a11y(page);
    await page.keyboard.press('Escape');
    await page.getByRole('button', { name: 'Ouvrir la navigation' }).click();
    expect(await style(page.locator('.app-nav'), 'transition-property')).toBe('none');
    await expect(page.getByRole('navigation', { name: 'Serveurs' })).toBeVisible();
  });
});

test('S12-H7 trois choix : Mouvement, Contraste et Format de l’heure tiennent sur une seule ligne dans une fenêtre large', async ({ user }) => {
  const { page } = await user('troisla');
  const row = async (name: string) =>
    (await group(page, name).getByRole('radio').evaluateAll((r) => r.map((x) => x.closest('label')!.getBoundingClientRect()))).map((b) => [Math.round(b.y), Math.round(b.x)]);
  for (const [path, name] of [['accessibility', 'Mouvement'], ['accessibility', 'Contraste'], ['language', "Format de l'heure"]]) {
    await page.goto(`/settings/${path}`);
    await expect(group(page, name)).toBeVisible();
    const boxes = await row(name);
    expect(boxes).toHaveLength(3);
    expect(new Set(boxes.map(([y]) => y)).size, `${name} : une seule ligne`).toBe(1);
    expect(new Set(boxes.map(([, x]) => x)).size, `${name} : trois colonnes`).toBe(3);
  }
});

test('S12-H2 images animées : fixes dans les listes, animées au survol, au focus et sur la carte ; fixes partout en mouvement réduit', async ({ user, api }) => {
  const a = await user('animee');
  const b = await user('fixee');
  const { page } = a;
  await put(api, a, 'avatar', animatedGif(3), 'image/gif');
  await put(api, a, 'banner', animatedGif(3), 'image/gif');
  await put(api, b, 'avatar', png(64, 64), 'image/png');
  const [gid, cid] = await createGuild(page, 'Aquarium');
  const { code } = await (await (await api(a.token)).post(`${API}/channels/${cid}/invites`, { data: {} })).json();
  expect((await (await api(b.token)).post(`${API}/invites/${code}`)).status()).toBe(200);
  await page.goto(`/channels/${gid}/${cid}`);
  await expect(members(page).getByText(b.name)).toBeVisible();

  const row = members(page).getByRole('listitem').filter({ hasText: a.name });
  const profile = row.getByRole('button', { name: new RegExp(`^${a.name}`) });
  const still = row.locator('img.k-still');
  const live = row.locator('img.k-live');
  // Dans la liste : l'image fixe, qui est bien la première image (rouge) de l'animation.
  await expect(still).toHaveCount(1);
  expect(await still.getAttribute('src')).toMatch(/^data:image\/png/);
  const pixel = await still.evaluate(async (img: HTMLImageElement) => {
    await img.decode();
    const c = document.createElement('canvas');
    c.width = img.naturalWidth;
    c.height = img.naturalHeight;
    const g = c.getContext('2d')!;
    g.drawImage(img, 0, 0);
    return Array.from(g.getImageData(1, 1, 1, 1).data);
  });
  expect(pixel).toEqual([255, 0, 0, 255]);
  expect(await style(live, 'display')).toBe('none');
  expect(await style(still, 'visibility')).toBe('visible');
  // L'image d'un compte qui n'a qu'une photo fixe reste seule, et visible.
  const plain = members(page).getByRole('listitem').filter({ hasText: b.name }).locator('img');
  await expect(plain).toHaveCount(1);
  await expect(plain).not.toHaveAttribute('data-probing', '');
  await expect(plain).toHaveCSS('opacity', '1');
  await expect(plain).not.toHaveClass(/k-still|k-live/);

  // Au survol de la ligne : l'animation.
  await row.hover();
  expect(await style(live, 'display')).toBe('block');
  expect(await style(still, 'visibility')).toBe('hidden');
  await page.mouse.move(5, 5);
  expect(await style(live, 'display')).toBe('none');
  // Au focus du clavier aussi.
  await profile.focus();
  expect(await style(live, 'display')).toBe('block');
  await page.getByRole('textbox', { name: /^Écrire dans/ }).focus();
  expect(await style(live, 'display')).toBe('none');

  // La carte de profil : photo et bannière animées, pas de bouton « Lire l'animation ».
  await profile.click();
  const card = page.getByRole('dialog', { name: `Profil de ${a.name}` });
  await expect(card).toBeVisible();
  await expect(card.locator('.k-avatar--xl img.k-live')).toHaveCount(1);
  expect(await style(card.locator('.k-avatar--xl img.k-live'), 'display')).toBe('block');
  expect(await style(card.locator('.app-card-banner img.k-live'), 'display')).toBe('block');
  expect(await style(card.locator('.k-avatar--xl img.k-still'), 'visibility')).toBe('hidden');
  await expect(card.getByRole('button', { name: "Lire l'animation" })).toBeHidden();
  await a11y(page);
  await page.keyboard.press('Escape');

  // Mouvement réduit : fixes partout, et « Lire l'animation » sur la carte.
  await page.goto('/settings/accessibility');
  await pick(page, 'Mouvement', 'Réduit');
  await page.goto(`/channels/${gid}/${cid}`);
  await expect(still).toHaveCount(1);
  await row.hover();
  expect(await style(live, 'display')).toBe('none');
  expect(await style(still, 'visibility')).toBe('visible');
  await profile.click();
  await expect(card).toBeVisible();
  const play = card.getByRole('button', { name: "Lire l'animation" });
  await expect(play).toBeVisible();
  await expect(play).toHaveAttribute('aria-pressed', 'false');
  expect(await style(card.locator('.k-avatar--xl img.k-live'), 'display')).toBe('none');
  expect(await style(card.locator('.app-card-banner img.k-live'), 'display')).toBe('none');
  await a11y(page);
  // Au clavier : le bouton se focalise, Entrée anime, une seconde fois fige.
  await play.focus();
  await page.keyboard.press('Enter');
  await expect(play).toHaveAttribute('aria-pressed', 'true');
  expect(await style(card.locator('.k-avatar--xl img.k-live'), 'display')).toBe('block');
  expect(await style(card.locator('.app-card-banner img.k-live'), 'display')).toBe('block');
  await page.keyboard.press('Enter');
  await expect(play).toHaveAttribute('aria-pressed', 'false');
  expect(await style(card.locator('.k-avatar--xl img.k-live'), 'display')).toBe('none');
  await page.keyboard.press('Escape');

  // Un compte à photo fixe n'a pas ce bouton sur sa carte.
  await members(page).getByRole('listitem').filter({ hasText: b.name }).getByRole('button', { name: new RegExp(`^${b.name}`) }).click();
  const other = page.getByRole('dialog', { name: `Profil de ${b.name}` });
  await expect(other).toBeVisible();
  await expect(other.getByRole('button', { name: "Lire l'animation" })).toBeHidden();
});

test('S12-H3 mouvement réduit : le fichier stocké reste animé, seul l’affichage est figé', async ({ user, api }) => {
  const a = await user('stocka');
  await put(api, a, 'avatar', animatedGif(3), 'image/gif');
  const key = (await (await (await api(a.token)).get(`${API}/users/@me`)).json()).avatar as string;
  const res = await (await api(a.token)).get(`${API}/avatars/${key}`);
  expect([res.status(), res.headers()['content-type']]).toEqual([200, 'image/gif']);
  expect(Buffer.compare(await res.body(), animatedGif(3))).toBe(0);
  const { page } = a;
  await page.goto('/settings/accessibility');
  await pick(page, 'Mouvement', 'Réduit');
  await page.goto('/settings/profiles');
  await expect(page.locator('.k-avatar--xl img.k-still')).toHaveCount(1);
  await expect(page.locator('.k-avatar--xl img.k-live')).toHaveCount(1);
  // L'original est toujours servi tel quel.
  expect(await page.locator('.k-avatar--xl img.k-live').getAttribute('src')).toBe(`${API}/avatars/${key}`);
  expect(Buffer.compare(await (await (await api(a.token)).get(`${API}/avatars/${key}`)).body(), animatedGif(3))).toBe(0);
});

test('S12-H4 contraste élevé : texte 7:1, bordures 3:1, focus de 3 px, dans les trois thèmes', async ({ user }) => {
  const { page } = await user('contra');
  await page.goto('/settings/accessibility');
  await expect(radio(page, 'Contraste', 'Normal')).toBeChecked();
  await expect(page.locator('html')).not.toHaveAttribute('data-contrast', 'high');
  const hint = page.locator('.app-prefs > .app-hint').first();
  const normal = ratio(...(Object.values(await colors(hint)) as [string, string]));
  expect(normal).toBeLessThan(7);

  await pick(page, 'Contraste', 'Élevé');
  await expect(page.locator('html')).toHaveAttribute('data-contrast', 'high');
  for (const theme of ['Sombre', 'Clair', 'Noir']) {
    await page.getByRole('link', { name: 'Apparence', exact: true }).click();
    await pick(page, 'Thème', theme);
    // Le texte secondaire et le texte atténué passent à 7:1 au moins.
    for (const sel of ['.app-prefs > .app-hint', '.k-radio span', '.k-label']) {
      const c = await colors(page.locator(sel).first());
      expect(ratio(c.fg, c.bg), `${theme} ${sel} : ${c.fg} sur ${c.bg}`).toBeGreaterThanOrEqual(7);
    }
    const title = await colors(page.getByRole('heading', { name: 'Apparence', level: 2 }));
    expect(ratio(title.fg, title.bg)).toBeGreaterThanOrEqual(7);
    // Le texte atténué est remonté au niveau du texte secondaire.
    const rootStyle = await page.evaluate(() => {
      const s = getComputedStyle(document.documentElement);
      return [s.getPropertyValue('--text-muted').trim(), s.getPropertyValue('--text-secondary').trim(), s.getPropertyValue('--size-focus').trim()];
    });
    expect(rootStyle[0]).toBe(rootStyle[1]);
    expect(rootStyle[2]).toBe('3px');
    // Les bordures se voient : 3:1 au moins contre le fond de l'aperçu.
    const border = await page.getByRole('region', { name: 'Aperçu' }).evaluate((el) => {
      const s = getComputedStyle(el);
      return { fg: s.borderTopColor, bg: getComputedStyle(document.body).backgroundColor === 'rgba(0, 0, 0, 0)' ? 'rgb(0, 0, 0)' : getComputedStyle(el).backgroundColor };
    });
    expect(ratio(border.fg, border.bg), `${theme} bordure`).toBeGreaterThanOrEqual(3);
    // L'anneau de focus fait 3 px.
    await radio(page, 'Thème', theme).focus();
    await page.keyboard.press('Shift+Tab');
    await page.keyboard.press('Tab');
    expect(await style(radio(page, 'Thème', theme), 'outline-width')).toBe('3px');
    await a11y(page);
    await page.getByRole('link', { name: 'Accessibilité', exact: true }).click();
  }
  // Retour à Normal : les couleurs d'origine et un focus de 2 px.
  await pick(page, 'Contraste', 'Normal');
  await expect(page.locator('html')).not.toHaveAttribute('data-contrast', 'high');
  expect(await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--size-focus').trim())).toBe('2px');
});

test('S12-H5 contraste automatique : suit la préférence « contraste plus élevé » du système', async ({ user }) => {
  const { page } = await user('autoc');
  await page.goto('/settings/accessibility');
  await pick(page, 'Contraste', 'Automatique');
  await expect(page.locator('html')).not.toHaveAttribute('data-contrast', 'high');
  await page.emulateMedia({ contrast: 'more' });
  await expect(page.locator('html')).toHaveAttribute('data-contrast', 'high');
  await a11y(page);
  await page.emulateMedia({ contrast: 'no-preference' });
  await expect(page.locator('html')).not.toHaveAttribute('data-contrast', 'high');
  await page.reload();
  await expect(radio(page, 'Contraste', 'Automatique')).toBeChecked();
});

test('S12-H6 souligner les liens : un interrupteur, appliqué tout de suite et gardé', async ({ user }) => {
  const { page } = await user('lienna');
  await page.goto('/settings/accessibility');
  const toggle = page.getByRole('switch', { name: 'Souligner les liens' });
  const link = page.getByRole('region', { name: 'Aperçu' }).locator('a');
  await expect(toggle).not.toBeChecked();
  expect(await style(link, 'text-decoration-line')).toBe('none');
  await toggle.check();
  await expect(page.locator('html')).toHaveAttribute('data-underline-links', '');
  expect(await style(link, 'text-decoration-line')).toBe('underline');
  // Les boutons et la navigation ne sont pas soulignés.
  expect(await style(page.getByRole('link', { name: 'Accessibilité', exact: true }), 'text-decoration-line')).toBe('none');
  await a11y(page);
  await page.reload();
  await expect(toggle).toBeChecked();
  await toggle.uncheck();
  expect(await style(link, 'text-decoration-line')).toBe('none');
  await expect(page.locator('html')).not.toHaveAttribute('data-underline-links', /.*/);
});

// ───────── S14 : Langue et heure ─────────

test('S14-H1 langue et heure : Français seul, format 12 h ou 24 h avec un exemple en direct', async ({ user }) => {
  const { page } = await user('heura');
  await page.goto('/settings/language');
  await expect(page.getByRole('heading', { name: 'Langue et heure', level: 2 })).toBeVisible();
  await a11y(page);
  const language = page.getByRole('combobox', { name: 'Langue' });
  expect(await language.locator('option').allTextContents()).toEqual(['Français']);
  await expect(language).toHaveAccessibleDescription("D'autres langues arrivent.");
  expect(await group(page, "Format de l'heure").getByRole('radio').evaluateAll((r) => r.map((x) => document.getElementById(x.getAttribute('aria-labelledby')!)!.textContent)))
    .toEqual(['Automatique', '12 h', '24 h']);
  await expect(radio(page, "Format de l'heure", 'Automatique')).toBeChecked();
  const example = page.getByRole('status').filter({ hasText: 'Exemple' });
  await expect(example).toHaveText('Exemple : 14:30');
  await pick(page, "Format de l'heure", '12 h');
  await expect(example).toHaveText('Exemple : 2:30 PM');
  await pick(page, "Format de l'heure", '24 h');
  await expect(example).toHaveText('Exemple : 14:30');
  await a11y(page);
  await pick(page, "Format de l'heure", '12 h');
  await page.reload();
  await expect(radio(page, "Format de l'heure", '12 h')).toBeChecked();
  await expect(example).toHaveText('Exemple : 2:30 PM');
  // Les flèches du clavier changent de format.
  await radio(page, "Format de l'heure", '12 h').focus();
  await page.keyboard.press('ArrowDown');
  await expect(radio(page, "Format de l'heure", '24 h')).toBeChecked();
  await expect(example).toHaveText('Exemple : 14:30');
});

test('S14-H2 un seul format d’heure partout : messages, suite d’un message, exclusion temporaire', async ({ user, api, browser, baseURL, issues }) => {
  const owner = await user('chefa');
  const crew = await user('matela');
  const [gid, cid] = await createGuild(owner.page, 'Horloge');
  const { code } = await (await (await api(owner.token)).post(`${API}/channels/${cid}/invites`, { data: {} })).json();
  expect((await (await api(crew.token)).post(`${API}/invites/${code}`)).status()).toBe(200);
  const { page } = owner;
  await say(page, 'général', 'un');
  await say(page, 'général', 'deux');
  await expect(messages(page)).toHaveCount(2);
  const stamp = (n: number) => messages(page).nth(n).locator('time');
  const H24 = /^\d{2}:\d{2}$/;
  const H12 = /^\d{1,2}:\d{2} [AP]M$/;
  // Automatique en français : 24 h, comme avant.
  await expect(stamp(0)).toHaveText(H24);
  await expect(stamp(1)).toHaveText(H24);
  await expect(messages(page).nth(1)).toHaveAccessibleName(new RegExp(`, ${H24.source.slice(1, -1)}$`));

  await page.goto('/settings/language');
  await pick(page, "Format de l'heure", '12 h');
  await page.goto(`/channels/${gid}/${cid}`);
  await expect(messages(page)).toHaveCount(2);
  await expect(stamp(0)).toHaveText(H12);
  await expect(stamp(1)).toHaveText(H12);
  // Le message de la suite montre la même heure au survol, au même format.
  await messages(page).nth(1).hover();
  await expect(messages(page).nth(1).locator('.app-gutter-time')).toHaveText(H12);
  await expect(messages(page).nth(1)).toHaveAccessibleName(/, \d{1,2}:\d{2} [AP]M$/);
  await a11y(page);

  // « Exclu jusqu'à » chez la personne exclue, avec son propre réglage.
  const own = await browser.newContext({ baseURL, locale: 'fr-FR' });
  await own.addInitScript(() => localStorage.setItem('kraken.clock', '12'));
  expect((await (await api(owner.token)).put(`${API}/guilds/${gid}/members/${crew.id}/timeout`, { data: { seconds: 600 } })).status()).toBe(200);
  const res = await own.request.post(`${API}/auth/login`, { data: { login: crew.name, password: PASSWORD } });
  expect(res.status()).toBe(200);
  const p2 = await own.newPage();
  watch(p2, issues);
  await p2.goto('/login');
  await p2.evaluate(() => localStorage.setItem('kraken.signed_in', '1'));
  await p2.goto(`/channels/${gid}/${cid}`);
  const composer = p2.getByRole('textbox', { name: /^Exclu jusqu/ });
  await expect(composer).toBeDisabled();
  await expect(composer).toHaveAccessibleName(/^Exclu jusqu.à (\d{2}\/\d{2}\/\d{4} )?\d{1,2}:\d{2} [AP]M$/);
  await own.close();
});

test.describe('largeur mobile', () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test('S14-N1 à 390 px les formats s’empilent et rien ne déborde', async ({ user }) => {
    const { page } = await user('petita');
    await page.goto('/settings/language');
    await a11y(page);
    await noScroll(page);
    const boxes = await group(page, "Format de l'heure").getByRole('radio').evaluateAll((r) => r.map((x) => x.closest('label')!.getBoundingClientRect()));
    expect(new Set(boxes.map((b) => Math.round(b.x))).size).toBe(1);
    boxes.slice(1).forEach((b, i) => expect(b.y).toBeGreaterThan(boxes[i].y));
    await pick(page, "Format de l'heure", '12 h');
    await expect(page.getByRole('status').filter({ hasText: 'Exemple' })).toHaveText('Exemple : 2:30 PM');
    await noScroll(page);
  });
});
