// Mise en page de droite à gauche (PR #32) : arabe, persan, hébreu.
// Aucune langue de droite à gauche n'est livrée encore (les traductions viennent dans la PR suivante) : ces tests basculent
// <html dir="rtl" lang="fa|ar|he"> EN DIRECT, sur une interface qui reste écrite en français, et mesurent la géométrie
// (boîtes réelles) : tout doit suivre l'attribut `dir` sans rechargement (feuilles de style logiques, `--flip`, `.k-flip`,
// sens des flèches et placement des menus lus à l'événement). Les écrans français et anglais (gauche à droite) ne bougent pas :
// RTL-L1 compare les boîtes après un aller-retour dir=rtl puis dir=ltr.
import { test, expect, a11y, createGuild, say, messages, API, type User } from '../fixtures';
import { png } from '../images';
import type { APIRequestContext, Locator, Page } from '@playwright/test';

// Micro et caméra factices : un test entre dans un salon vocal pour voir la barre d'appel.
test.use({ launchOptions: { args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'] } });

const LANGS = ['fa', 'ar', 'he'] as const;
type Api = (token?: string) => Promise<APIRequestContext>;
type R = { x: number; y: number; w: number; h: number; r: number; b: number };

/** Bascule la page en droite-à-gauche (ou la rend), sans la recharger. */
const rtl = (p: Page, lang: string = 'fa') =>
  p.evaluate((l) => {
    document.documentElement.dir = 'rtl';
    document.documentElement.lang = l;
  }, lang);
const ltr = (p: Page, lang = 'fr') =>
  p.evaluate((l) => {
    document.documentElement.dir = 'ltr';
    document.documentElement.lang = l;
  }, lang);
/** La boîte d'un élément (même caché), en pixels de la fenêtre. */
const rect = (l: Locator): Promise<R> =>
  l.evaluate((e) => {
    const r = e.getBoundingClientRect();
    return { x: r.x, y: r.y, w: r.width, h: r.height, r: r.right, b: r.bottom };
  });
const W = (p: Page) => p.viewportSize()!.width;
/** `a` est entièrement à droite de `b` (au demi-pixel près), ou à gauche. */
const rightOf = (a: R, b: R) => expect(a.x).toBeGreaterThanOrEqual(b.r - 0.5);
const leftOf = (a: R, b: R) => expect(a.r).toBeLessThanOrEqual(b.x + 0.5);
const near = (a: number, b: number, tol = 1.5) => expect(Math.abs(a - b), `${a} ≈ ${b}`).toBeLessThanOrEqual(tol);
/** La boîte reste dans la fenêtre. */
async function inView(p: Page, r: R, what = 'élément') {
  const v = p.viewportSize()!;
  expect(r.x, `${what} : bord gauche`).toBeGreaterThanOrEqual(-0.5);
  expect(r.r, `${what} : bord droit`).toBeLessThanOrEqual(v.width + 0.5);
  expect(r.y, `${what} : haut`).toBeGreaterThanOrEqual(-0.5);
  expect(r.b, `${what} : bas`).toBeLessThanOrEqual(v.height + 0.5);
}
const noOverflow = async (p: Page) =>
  expect(await p.evaluate(() => document.documentElement.scrollWidth), 'débordement horizontal').toBeLessThanOrEqual(W(p));
const attr = (l: Locator, name: string) => l.getAttribute(name);
const css = (l: Locator, prop: string) => l.evaluate((e, k) => getComputedStyle(e).getPropertyValue(k), prop);

/** Un serveur créé par l'API (un salon texte « général », un salon vocal « Général »), ouvert sur son salon texte. */
async function openGuild(u: User, api: Api, name = 'Récif corallien') {
  const res = await (await api(u.token)).post(`${API}/guilds`, { data: { name } });
  expect(res.status(), await res.text()).toBe(201);
  const g = await res.json();
  const text = g.channels.find((c: { kind: string }) => c.kind === 'text').id as string;
  const voice = g.channels.find((c: { kind: string }) => c.kind === 'voice').id as string;
  await u.page.goto(`/channels/${g.guild.id}/${text}`);
  await expect(u.page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
  return { gid: g.guild.id as string, text, voice };
}
/** Envoie un message et attend qu'il soit confirmé (plus d'envoi en cours : requis avant axe). */
async function post(p: Page, text: string, shown = text.split('\n')[0]) {
  await say(p, 'général', text);
  await expect(p.locator('article[data-pending]')).toHaveCount(0);
  await expect(messages(p).filter({ hasText: shown })).toHaveCount(1);
}

// ───────── bureau : la coquille et les messages ─────────

for (const lang of LANGS) {
  test(`RTL-1 [${lang}] coquille : rail à droite des salons, membres à gauche du fil, avatar à droite du texte`, async ({ user, api }) => {
    const a = await user('rtl1');
    await openGuild(a, api);
    await post(a.page, 'Premier message du récif');
    const p = a.page;
    const msg = messages(p).filter({ hasText: 'Premier message du récif' });
    const shell = async () => ({
      rail: await rect(p.locator('.app-rail')),
      nav: await rect(p.locator('.app-nav')),
      main: await rect(p.locator('.app-main')),
      chat: await rect(p.locator('.app-chat')),
      aside: await rect(p.locator('.app-aside--members')),
      avatar: await rect(msg.locator('.k-avatar').first()),
      text: await rect(msg.locator('.app-md')),
      composer: await rect(p.locator('.k-composer')),
      send: await rect(p.getByRole('button', { name: 'Envoyer' })),
      attach: await rect(p.getByRole('button', { name: 'Joindre un fichier' })),
    });
    const l = await shell();
    // Gauche à droite : rail, salons, fil, membres.
    leftOf(l.rail, l.nav);
    leftOf(l.nav, l.main);
    leftOf(l.chat, l.aside);
    leftOf(l.avatar, l.text);
    leftOf(l.attach, l.send);
    await rtl(p, lang);
    await expect(p.locator('html')).toHaveAttribute('dir', 'rtl');
    const r = await shell();
    rightOf(r.rail, r.nav);
    rightOf(r.nav, r.main);
    rightOf(r.chat, r.aside);
    rightOf(r.avatar, r.text);
    rightOf(r.attach, r.send); // la pièce jointe au début (à droite), l'envoi à la fin (à gauche)
    near(r.rail.r, W(p), 0.5);
    near(r.aside.x, 0, 0.5);
    // Un vrai miroir : chaque boîte de la coquille est le symétrique de sa position en gauche-à-droite.
    for (const k of ['rail', 'nav', 'main', 'chat', 'aside', 'composer'] as const) {
      near(r[k].x, W(p) - l[k].r, 1);
      near(r[k].w, l[k].w, 1);
    }
    await noOverflow(p);
  });

  test(`RTL-2 [${lang}] message : boutons d'action vers le bord de fin (gauche), heure et réponse`, async ({ user, api }) => {
    const a = await user('rtl2');
    await openGuild(a, api);
    await post(a.page, 'Message à survoler');
    const p = a.page;
    const msg = messages(p).filter({ hasText: 'Message à survoler' });
    const bar = msg.locator('.k-msg-actions');
    await msg.hover();
    await expect(bar).toBeVisible();
    const l = { msg: await rect(msg), bar: await rect(bar) };
    // Gauche à droite : la barre colle au bord droit du message (à `inset-inline-end`).
    const gap = l.msg.r - l.bar.r;
    expect(gap).toBeGreaterThan(0);
    await p.mouse.move(0, 0);
    await expect(bar).toBeHidden();
    await rtl(p, lang);
    await msg.hover();
    await expect(bar).toBeVisible();
    const r = { msg: await rect(msg), bar: await rect(bar) };
    near(r.bar.x - r.msg.x, gap, 1); // la même distance, au bord de fin (gauche)
    expect(r.bar.x + r.bar.w / 2).toBeLessThan(r.msg.x + r.msg.w / 2);
    await inView(p, r.bar, 'barre des actions');
    // Répondre : la barre « Tu réponds à… » garde son bouton fermer au bout (à gauche).
    await bar.getByRole('button', { name: 'Répondre' }).click();
    const replying = p.locator('.app-replying');
    await expect(replying).toBeVisible();
    const rr = await rect(replying);
    const close = await rect(replying.getByRole('button'));
    expect(close.x).toBeLessThan(rr.x + rr.w / 2); // fermer au bord de fin (à gauche)
    await ltr(p);
    const lr = await rect(replying);
    const lclose = await rect(replying.getByRole('button'));
    near(close.x - rr.x, lr.r - lclose.r, 1); // la même marge, du côté opposé
    expect(lclose.x).toBeGreaterThan(lr.x + lr.w / 2);
  });
}

test('RTL-3 menus : compte, menu contextuel (placé près du curseur, jamais hors de la fenêtre) et sélecteur d’emojis', async ({ user, api }) => {
  const a = await user('rtl3');
  await openGuild(a, api);
  const p = a.page;
  // Mon compte, en gauche à droite : la carte s'ouvre du côté du fil (à droite du bouton).
  const card = p.locator('#me-card');
  await p.getByRole('button', { name: /^Mon compte/ }).click();
  await expect(card).toBeVisible();
  const lc = await rect(card);
  await p.keyboard.press('Escape');
  await expect(card).toBeHidden();
  await rtl(p, 'ar');
  // En droite à gauche : le miroir exact (elle s'ouvre à gauche du bouton), dans la fenêtre.
  await p.getByRole('button', { name: /^Mon compte/ }).click();
  await expect(card).toBeVisible();
  const c = await rect(card);
  near(c.x, W(p) - lc.r, 1);
  near(c.w, lc.w, 1);
  near(c.b, lc.b, 2); // toujours posée sur le bas (l'interligne plus haut en arabe l'allonge vers le haut)
  await inView(p, c, 'carte Mon compte');
  await p.keyboard.press('Escape');
  await expect(card).toBeHidden();

  // Menu contextuel d'un salon : son bord de fin (droit) touche le curseur ; près d'un bord, il reste dans la fenêtre.
  const channel = p.locator('.app-sidebar').getByRole('link', { name: 'général', exact: true });
  const menu = p.getByRole('menu');
  const box = (await channel.boundingBox())!;
  const cx = box.x + box.width / 2;
  await channel.click({ button: 'right' });
  await expect(menu).toBeVisible();
  const m1 = await rect(menu);
  near(m1.r, cx, 2);
  await inView(p, m1, 'menu contextuel au centre');
  await p.keyboard.press('Escape');
  for (const x of [4, 120, W(p) - 4]) {
    await channel.evaluate((e, cx) => e.dispatchEvent(new MouseEvent('contextmenu', { clientX: cx, clientY: 300, bubbles: true, cancelable: true })), x);
    await expect(menu).toBeVisible();
    const m = await rect(menu);
    await inView(p, m, `menu contextuel à x=${x}`);
    if (x > 230) near(m.r, x, 2);
    await p.keyboard.press('Escape');
    await expect(menu).toBeHidden();
  }
  // Au clavier (Maj+F10), il s'ouvre sous le bord de début (droit) du salon.
  await channel.focus();
  await p.keyboard.press('Shift+F10');
  await expect(menu).toBeVisible();
  const mk = await rect(menu);
  near(mk.r, box.x + box.width - 16, 2);
  await inView(p, mk, 'menu au clavier');
  await p.keyboard.press('Escape');

  // Sélecteur d'emojis : ancré au bord de fin du fil (à gauche), dans la fenêtre.
  const picker = p.locator('.app-picker');
  await ltr(p); // le même sélecteur en gauche à droite : ancré au bord droit du fil
  await p.getByRole('button', { name: 'Insérer un emoji' }).click();
  await expect(picker).toBeVisible();
  const lpk = await rect(picker);
  const lchat = await rect(p.locator('.app-chat'));
  await p.keyboard.press('Escape');
  await expect(picker).toHaveCount(0);
  await rtl(p, 'ar');
  const chat = await rect(p.locator('.app-chat'));
  await p.getByRole('button', { name: 'Insérer un emoji' }).click();
  await expect(picker).toBeVisible();
  const pk = await rect(picker);
  near(pk.x - chat.x, lchat.r - lpk.r, 1.5);
  near(pk.w, lpk.w, 1);
  await inView(p, pk, 'sélecteur d’emojis');
  await a11y(p);
});

test('RTL-4 sélecteur d’emojis : la flèche gauche va à la case de gauche (la suivante dans le document)', async ({ user, api }) => {
  const a = await user('rtl4');
  await openGuild(a, api);
  const p = a.page;
  const focused = () =>
    p.evaluate(() => {
      const r = document.activeElement!.getBoundingClientRect();
      return { x: r.x, y: r.y };
    });
  const open = async () => {
    await p.getByRole('button', { name: 'Insérer un emoji' }).click();
    await expect(p.locator('.app-picker')).toBeVisible();
    await p.keyboard.press('ArrowDown'); // de la recherche à la grille
    await expect(p.locator('.app-picker-emoji:focus')).toHaveCount(1);
  };
  // Gauche à droite : → va à la case de droite.
  await open();
  const l0 = await focused();
  await p.keyboard.press('ArrowRight');
  const l1 = await focused();
  expect(l1.y).toBe(l0.y);
  expect(l1.x).toBeGreaterThan(l0.x);
  await p.keyboard.press('Escape');
  await expect(p.locator('.app-picker')).toHaveCount(0);
  // Droite à gauche : la grille se remplit depuis la droite ; ← va à la case visuellement à gauche.
  await rtl(p, 'fa');
  await open();
  const r0 = await focused();
  await p.keyboard.press('ArrowLeft');
  const r1 = await focused();
  expect(r1.y).toBe(r0.y);
  expect(r1.x).toBeLessThan(r0.x);
  await p.keyboard.press('ArrowRight');
  const r2 = await focused();
  expect(r2.x).toBeCloseTo(r0.x, 0);
  // Fin de ligne : la case la plus à gauche ; Début : la plus à droite.
  await p.keyboard.press('End');
  const end = await focused();
  expect(end.x).toBeLessThan(r0.x);
  await p.keyboard.press('Home');
  expect((await focused()).x).toBeCloseTo(r0.x, 0);
});

// ───────── tiroirs et petit écran ─────────

test.describe('390 px', () => {
  test.use({ viewport: { width: 390, height: 844 } });

  for (const lang of LANGS) {
    test(`RTL-5 [${lang}] tiroir mobile : la barre latérale s’ouvre depuis le bord de début (droit), les membres depuis le bord de fin`, async ({ user, api }) => {
      const a = await user('rtl5');
      await openGuild(a, api);
      const p = a.page;
      const app = p.locator('.app');
      const rail = p.locator('.app-rail');
      const nav = p.locator('.app-nav');
      const open = p.getByRole('button', { name: 'Ouvrir la navigation' });
      // Gauche à droite : fermé hors de l'écran à gauche, ouvert collé à gauche.
      expect((await rect(rail)).r).toBeLessThanOrEqual(0);
      const closedNav = await rect(nav); // fermée, masquée (visibility) : hors de la vue
      await expect(nav).toBeHidden();
      await open.click();
      await expect(app).toHaveAttribute('data-drawer', 'open');
      await expect(rail).toBeVisible();
      await expect.poll(async () => (await rect(rail)).x).toBeCloseTo(0, 0);
      leftOf(await rect(rail), await rect(nav));
      await p.locator('.app-scrim').click({ position: { x: 380, y: 400 } });
      await expect(app).not.toHaveAttribute('data-drawer', 'open');
      await expect(rail).toBeHidden();
      // Droite à gauche : fermé hors de l'écran à droite, ouvert collé à droite, les salons à gauche du rail.
      await rtl(p, lang);
      await p.waitForTimeout(300);
      expect((await rect(rail)).x).toBeGreaterThanOrEqual(W(p));
      near((await rect(nav)).x, W(p) - closedNav.r, 1); // la même position, en miroir
      await expect(nav).toBeHidden();
      await open.click();
      await expect(app).toHaveAttribute('data-drawer', 'open');
      await expect(rail).toBeVisible();
      await expect(nav).toBeVisible();
      await expect.poll(async () => (await rect(rail)).r).toBeCloseTo(W(p), 0);
      const r = await rect(rail);
      const n = await rect(nav);
      rightOf(r, n);
      near(n.r, W(p) - r.w, 1);
      await inView(p, n, 'barre latérale ouverte');
      await p.locator('.app-scrim').click({ position: { x: 10, y: 400 } });
      await expect(app).not.toHaveAttribute('data-drawer', 'open');
      // La liste des membres passe par-dessus le fil, au bord de fin : à gauche.
      await p.getByRole('button', { name: 'Liste des membres' }).click();
      const members = p.locator('.app-aside--members');
      await expect(members).toBeVisible();
      const mb = await rect(members);
      await expect.poll(async () => (await rect(members)).x).toBeCloseTo(0, 0);
      await inView(p, await rect(members), 'liste des membres');
      await noOverflow(p);
    });
  }

  test('RTL-6 Réglages à 390 px : la barre d’onglets défile, l’onglet ouvert reste visible, scrollLeft devient négatif', async ({ user }) => {
    const a = await user('rtl6');
    const p = a.page;
    await p.goto('/settings/account');
    const dialog = p.getByRole('dialog', { name: 'Réglages' });
    await expect(dialog).toBeVisible();
    const nav = dialog.locator('nav');
    // Gauche à droite : la barre défile vers la droite (scrollLeft positif) jusqu'au dernier onglet.
    const scrolled = async () => nav.evaluate((e) => ({ left: e.scrollLeft, max: e.scrollWidth - e.clientWidth }));
    expect((await scrolled()).max).toBeGreaterThan(50);
    const links = nav.getByRole('link');
    const lFirst = (await rect(links.first())).x - (await rect(nav)).x; // marge avant le premier onglet, au bord de début
    await rtl(p, 'ar');
    const last = links.last();
    await last.click();
    await expect(last).toHaveAttribute('aria-current', 'page');
    await expect.poll(async () => (await scrolled()).left).toBeLessThan(-20);
    const n = await rect(nav);
    const t = await rect(last);
    expect(t.x).toBeGreaterThanOrEqual(n.x - 0.5);
    expect(t.r).toBeLessThanOrEqual(n.r + 0.5);
    // Le premier onglet est à droite : on y revient, scrollLeft redevient (presque) nul.
    const first = links.first();
    await first.click();
    await expect(first).toHaveAttribute('aria-current', 'page');
    await expect.poll(async () => Math.abs((await scrolled()).left)).toBeLessThan(2);
    const f = await rect(first);
    near((await rect(nav)).r - f.r, lFirst, 1); // le premier onglet au bord de début (droit), même marge
    // Une page de réglages longue ne déborde pas de côté.
    await noOverflow(p);
    expect(await dialog.locator('main').evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true);
  });
});

// ───────── fenêtres, barres d’onglets, formulaires ─────────

test('RTL-7 Réglages (bureau) : la liste des rubriques à droite, le contenu à gauche ; axe sans violation', async ({ user }) => {
  const a = await user('rtl7');
  const p = a.page;
  await p.getByRole('button', { name: /^Mon compte/ }).click();
  await p.getByRole('link', { name: 'Réglages utilisateur' }).click();
  const dialog = p.getByRole('dialog', { name: 'Réglages' });
  await expect(dialog).toBeVisible();
  const l = { nav: await rect(dialog.locator('nav')), main: await rect(dialog.locator('main')), box: await rect(dialog) };
  leftOf(l.nav, l.main);
  await rtl(p, 'fa');
  const r = { nav: await rect(dialog.locator('nav')), main: await rect(dialog.locator('main')), box: await rect(dialog) };
  rightOf(r.nav, r.main);
  near(r.box.x, l.box.x, 1); // la fenêtre reste centrée
  near(r.nav.x - r.box.x, r.box.w - l.nav.w - 1, 2); // la liste colle au bord droit de la fenêtre
  // Un onglet actif : sa pastille de focus et la liste restent lisibles.
  await dialog.getByRole('link', { name: 'Apparence' }).click();
  await expect(dialog.getByRole('heading', { name: 'Apparence', level: 2 })).toBeVisible();
  await a11y(p);
});

test('RTL-8 groupe radio natif : les flèches suivent le sens visuel en RTL', async ({ user }) => {
  const a = await user('rtl8');
  const p = a.page;
  await p.goto('/settings/appearance');
  const theme = p.getByRole('radiogroup', { name: /Thème/ }).or(p.getByRole('group', { name: /Thème/ }));
  const radios = theme.getByRole('radio');
  await expect(radios.first()).toBeVisible();
  const n = await radios.count();
  expect(n).toBeGreaterThanOrEqual(3);
  const x = (i: number) => radios.nth(i).evaluate((e) => e.closest('label')!.getBoundingClientRect().x);
  const checked = () => radios.evaluateAll((all) => all.findIndex((r) => (r as HTMLInputElement).checked));
  // Gauche à droite : les choix vont de gauche à droite ; → passe au suivant, qui est plus à droite.
  expect(await x(1)).toBeGreaterThan(await x(0));
  await radios.nth(checked() ? await checked() : 0).focus();
  const start = await checked();
  await p.keyboard.press('ArrowRight');
  const after = await checked();
  expect(after).not.toBe(start);
  // Droite à gauche : le premier choix est à droite du deuxième ; la flèche gauche va vers un choix plus à gauche.
  await rtl(p, 'ar');
  expect(await x(1)).toBeLessThan(await x(0));
  const here = await checked();
  await radios.nth(here).focus();
  const xHere = await x(here);
  await p.keyboard.press('ArrowLeft');
  const there = await checked();
  expect(there).not.toBe(here);
  expect(await x(there)).toBeLessThan(xHere);
  await p.keyboard.press('ArrowRight');
  expect(await x(await checked())).toBeGreaterThan(await x(there));
});

for (const lang of LANGS) {
  test(`RTL-9 [${lang}] Amis : les onglets se suivent de droite à gauche, la flèche gauche passe à l’onglet suivant`, async ({ user }) => {
    const a = await user('rtl9');
    const p = a.page;
    const tabs = p.getByRole('tablist').getByRole('tab');
    const names = await tabs.allInnerTexts();
    expect(names.slice(0, 3).map((s) => s.trim())).toEqual(['En ligne', 'Tous', 'En attente']);
    const selected = () => tabs.evaluateAll((all) => all.findIndex((t) => t.getAttribute('aria-selected') === 'true'));
    expect(await selected()).toBe(0);
    // Gauche à droite : → passe au suivant, ← revient.
    await tabs.first().focus();
    await p.keyboard.press('ArrowRight');
    expect(await selected()).toBe(1);
    await p.keyboard.press('ArrowLeft');
    expect(await selected()).toBe(0);
    // Droite à gauche : le premier onglet est à droite ; ← passe au suivant, → revient, Fin va au dernier (à gauche).
    await rtl(p, lang);
    const x = (i: number) => tabs.nth(i).evaluate((e) => e.getBoundingClientRect().x);
    expect(await x(0)).toBeGreaterThan(await x(1));
    await tabs.first().focus();
    await p.keyboard.press('ArrowLeft');
    expect(await selected()).toBe(1);
    await expect(tabs.nth(1)).toBeFocused();
    await p.keyboard.press('ArrowRight');
    expect(await selected()).toBe(0);
    await p.keyboard.press('ArrowRight'); // avant le premier : on boucle sur le dernier
    expect(await selected()).toBe((await tabs.count()) - 1);
    // « Ajouter un ami » : le champ du pseudo est un fragment technique, de gauche à droite.
    await expect(p.getByRole('textbox', { name: 'Pseudo de ton ami' })).toBeVisible();
    expect(await attr(p.getByRole('textbox', { name: 'Pseudo de ton ami' }), 'dir')).toBe('ltr');
    await noOverflow(p);
    await a11y(p);
  });
}

test('RTL-10 onglets des rôles (Affichage, Permissions, Membres) : même règle des flèches', async ({ user, api }) => {
  const a = await user('rtl10');
  const { gid } = await openGuild(a, api);
  const p = a.page;
  await p.goto(`/guilds/${gid}/settings/roles`);
  const dialog = p.getByRole('dialog', { name: 'Paramètres du serveur' });
  await expect(dialog).toBeVisible();
  await dialog.getByRole('button', { name: /tout le monde/ }).click();
  const tabs = dialog.getByRole('tablist').getByRole('tab');
  await expect(tabs).toHaveCount(3);
  const selected = () => tabs.evaluateAll((all) => all.findIndex((t) => t.getAttribute('aria-selected') === 'true'));
  await tabs.first().click();
  await p.keyboard.press('ArrowRight');
  expect(await selected()).toBe(1);
  await rtl(p, 'he');
  await tabs.first().click();
  expect(await selected()).toBe(0);
  await p.keyboard.press('ArrowLeft');
  expect(await selected()).toBe(1);
  await expect(tabs.nth(1)).toBeFocused();
  await p.keyboard.press('ArrowRight');
  expect(await selected()).toBe(0);
});

test('RTL-11 Ctrl+K : fenêtre centrée, icône au début (à droite), raccourcis de gauche à droite', async ({ user, api }) => {
  const a = await user('rtl11');
  await openGuild(a, api);
  const p = a.page;
  await rtl(p, 'ar');
  await p.keyboard.press('Control+k');
  const box = p.getByRole('combobox');
  await expect(box).toBeVisible();
  expect(await attr(box, 'dir')).toBe('auto');
  const dialog = p.locator('.app-sw');
  const d = await rect(dialog);
  near(d.x + d.w / 2, W(p) / 2, 1);
  await inView(p, d, 'sélecteur rapide');
  await box.fill('gén');
  const row = p.getByRole('option').first();
  await expect(row).toBeVisible();
  const lead = await rect(row.locator('.app-sw-lead'));
  const name = await rect(row.locator('.app-sw-name'));
  rightOf(lead, name);
  // Le bouton de fermeture suit le champ : au bord de fin (à gauche).
  const x = await rect(dialog.locator('.app-sw-field .k-btn'));
  leftOf(x, await rect(box));
  // Les raccourcis (↑ ↓ ↵ Échap) restent de gauche à droite.
  const kbd = dialog.locator('.k-kbd').first();
  expect(await css(kbd, 'direction')).toBe('ltr');
  await a11y(p);
  await p.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
});

test('RTL-12 fenêtre des invitations et fenêtres à pied : boutons au bord de fin, lien de gauche à droite', async ({ user, api }) => {
  const a = await user('rtl12');
  await openGuild(a, api);
  const p = a.page;
  await rtl(p, 'fa');
  await p.getByRole('button', { name: 'Inviter des amis' }).click();
  const dialog = p.getByRole('dialog', { name: /Inviter dans/ });
  await expect(dialog).toBeVisible();
  const link = dialog.getByLabel('Lien à partager');
  await expect(link).toBeVisible();
  expect(await attr(link, 'dir')).toBe('ltr');
  expect(await link.inputValue()).toMatch(/\/invite\/[A-Za-z0-9]{8}$/);
  const d = await rect(dialog);
  near(d.x + d.w / 2, W(p) / 2, 1);
  await inView(p, d, 'fenêtre des invitations');
  const close = await rect(dialog.getByRole('button', { name: 'Fermer' }));
  const copy = await rect(dialog.getByRole('button', { name: 'Copier le lien' }));
  // Le pied range ses boutons au bord de fin (gauche) : « Fermer » (le premier) à droite de « Copier le lien ».
  rightOf(close, copy);
  // Le nom du serveur dans le titre est isolé : une phrase ne mêle pas son sens d'écriture à celui du nom.
  expect(await dialog.getByRole('heading').innerText()).toContain('⁨Récif corallien⁩');
  await a11y(p);
  // Le miroir exact de la même fenêtre en gauche à droite : le bouton du bord de fin est au bord droit.
  await ltr(p);
  const lc = await rect(dialog.getByRole('button', { name: 'Copier le lien' }));
  const ld = await rect(dialog);
  near(ld.r - lc.r, copy.x - d.x, 1);
  leftOf(await rect(dialog.getByRole('button', { name: 'Fermer' })), lc);
});

// ───────── appel ─────────

test('RTL-13 barre d’appel d’un salon vocal : commandes dans l’ordre inverse, dans la fenêtre', async ({ user, api }) => {
  test.setTimeout(90_000);
  const a = await user('rtl13');
  const { gid, voice } = await openGuild(a, api);
  const p = a.page;
  await p.goto(`/channels/${gid}/${voice}`);
  await p.getByRole('button', { name: 'Rejoindre', exact: true }).click();
  await expect(p.getByRole('region', { name: 'Connexion vocale' }).getByRole('status')).toHaveText('En appel', { timeout: 20_000 });
  const bar = p.getByRole('group', { name: "Commandes de l'appel" });
  await expect(bar).toBeVisible();
  const buttons = bar.getByRole('button');
  const count = await buttons.count();
  expect(count).toBeGreaterThanOrEqual(3);
  const xs = async () => buttons.evaluateAll((all) => all.map((b) => b.getBoundingClientRect().x));
  const l = await xs();
  expect(l[0]).toBeLessThan(l[count - 1]);
  await rtl(p, 'ar');
  const r = await xs();
  expect(r[0]).toBeGreaterThan(r[count - 1]);
  await inView(p, await rect(bar), 'barre d’appel');
  // La barre reste centrée, les tuiles aussi ; la carte « Connexion vocale » du bas de la barre latérale est entière.
  const b = await rect(bar);
  near(b.x + b.w / 2, (await rect(p.locator('.app-call'))).x + (await rect(p.locator('.app-call'))).w / 2, 2);
  await inView(p, await rect(p.getByRole('region', { name: 'Connexion vocale' })), 'carte vocale');
  await noOverflow(p);
  await a11y(p);
  await bar.getByRole('button', { name: /Quitter/ }).click();
});

// ───────── bidi : saisie, pseudos, code ─────────

test('RTL-14 champs de saisie : texte libre en dir="auto", données techniques en dir="ltr"', async ({ page, user, api }) => {
  // Page de connexion et d'inscription : identifiant, e-mail, pseudo (ASCII) de gauche à droite.
  await page.goto('/register');
  await rtl(page, 'fa');
  expect(await attr(page.locator('input[type=email]'), 'dir')).toBe('ltr');
  expect(await attr(page.getByLabel('Pseudo', { exact: false }).first(), 'dir')).toBe('ltr');
  await page.goto('/login');
  await rtl(page, 'fa');
  expect(await attr(page.locator('input[name=login]'), 'dir')).toBe('ltr');

  const a = await user('rtl14');
  await openGuild(a, api);
  const p = a.page;
  await rtl(p, 'ar');
  const composer = p.getByLabel('Écrire dans #général');
  expect(await attr(composer, 'dir')).toBe('auto');
  // Chercher (Ctrl+K), nom du serveur, bio, nom affiché : « auto » ; la direction suit la saisie.
  await composer.fill('مرحبا بالجميع');
  expect(await composer.evaluate((e) => getComputedStyle(e).direction)).toBe('rtl');
  await composer.fill('Hello everyone');
  expect(await composer.evaluate((e) => getComputedStyle(e).direction)).toBe('ltr');
  await composer.fill('');
  await p.goto('/settings/profiles');
  await expect(p.locator('#display-name')).toBeVisible();
  for (const id of ['#display-name', '#pronouns', '#bio']) expect(await attr(p.locator(id), 'dir'), id).toBe('auto');
  await p.goto('/guilds/new');
  await expect(p.getByLabel('Nom du serveur')).toBeVisible();
  expect(await attr(p.getByLabel('Nom du serveur'), 'dir')).toBe('auto');
  await rtl(p, 'ar');
  await noOverflow(p);
});

test('RTL-15 texte bidirectionnel : messages « auto », code de gauche à droite, pseudos isolés', async ({ user, api }) => {
  const a = await user('rtl15');
  await openGuild(a, api);
  const p = a.page;
  await post(p, 'سلام دنیا، این یک پیام است');
  await post(p, 'Hello world, a plain message');
  await post(p, '```\nconst x = 1; // code\n```', 'const x = 1');
  await post(p, 'voir `inline_code` ici', 'voir inline_code ici');
  const lpre = await rect(p.locator('pre.app-code'));
  const lcode = await rect(p.locator('pre.app-code code'));
  await rtl(p, 'fa');
  const para = (text: string) => messages(p).filter({ hasText: text }).locator('p[dir=auto]').first();
  expect(await css(para('سلام دنیا'), 'direction')).toBe('rtl');
  expect(await css(para('Hello world'), 'direction')).toBe('ltr');
  const pre = p.locator('pre.app-code');
  await expect(pre).toBeVisible();
  expect(await css(pre, 'direction')).toBe('ltr');
  expect(await css(pre, 'text-align')).toMatch(/^(start|left)$/);
  const code = pre.locator('code');
  expect(await css(code, 'direction')).toBe('ltr');
  // Le code est calé au bord gauche du bloc (début d'un texte de gauche à droite), pas à droite.
  const pr = await rect(pre);
  const cr = await rect(code);
  near(cr.x - pr.x, lcode.x - lpre.x, 1); // la même marge qu'en gauche à droite
  const inline = p.locator('code.app-code-inline');
  expect(await css(inline, 'direction')).toBe('ltr');
  expect(await css(inline, 'unicode-bidi')).toBe('isolate');
  // Les pseudos @nom de la liste des membres sont isolés (bdi de gauche à droite).
  const handle = p.getByRole('complementary', { name: 'Membres' }).locator('bdi[dir=ltr]').first();
  await expect(handle).toContainText(`@${a.name}`);
  expect(await css(handle, 'direction')).toBe('ltr');
  expect(await css(handle, 'unicode-bidi')).toMatch(/isolate/);
  // `bdi` posés autour des pseudos (PR #31) : toujours là.
  await expect(p.locator('.k-msg header b bdi, .k-msg header bdi').first()).toBeVisible();
  await a11y(p);
});

test('RTL-16 icônes : celles qui indiquent un sens se retournent, les autres non', async ({ user, api }) => {
  const a = await user('rtl16');
  await openGuild(a, api);
  await post(a.page, 'Un message pour répondre');
  const p = a.page;
  const matrix = (l: Locator) => l.evaluate((e) => getComputedStyle(e).transform);
  const send = p.getByRole('button', { name: 'Envoyer' }).locator('svg');
  const attach = p.getByRole('button', { name: 'Joindre un fichier' }).locator('svg');
  const pin = p.getByRole('button', { name: 'Messages épinglés' }).locator('svg');
  expect(await matrix(send)).toBe('none');
  await rtl(p, 'fa');
  expect(await matrix(send)).toBe('matrix(-1, 0, 0, 1, 0, 0)');
  expect(await matrix(attach)).toBe('none');
  expect(await matrix(pin)).toBe('none');
  // Répondre (barre des actions) et le coin fléché de la réponse.
  const msg = messages(p).filter({ hasText: 'Un message pour répondre' });
  await msg.hover();
  expect(await matrix(msg.getByRole('button', { name: 'Répondre' }).locator('svg'))).toBe('matrix(-1, 0, 0, 1, 0, 0)');
  expect(await matrix(msg.getByRole('button', { name: 'Ajouter une réaction' }).locator('svg'))).toBe('none');
  // Règle générale : une icône est retournée si et seulement si elle porte `k-flip` (la pile des catégories tourne, elle, d'un quart de tour).
  const wrong = await p.evaluate(() =>
    [...document.querySelectorAll('svg.k-icon')]
      .filter((s) => !s.closest('.k-category') && s.getClientRects().length > 0)
      .filter((s) => (['matrix(-1, 0, 0, 1, 0, 0)', 'scaleX(-1)'].includes(getComputedStyle(s).transform)) !== s.classList.contains('k-flip'))
      .map((s) => s.parentElement?.outerHTML.slice(0, 80)),
  );
  expect(wrong).toEqual([]);
  // Le chevron d'une catégorie repliée pointe vers le côté de fin : un quart de tour dans l'autre sens.
  const cat = p.locator('.k-category').first();
  await cat.click();
  await expect(cat).toHaveAttribute('aria-expanded', 'false');
  expect(await matrix(cat.locator('svg'))).toBe('matrix(0, 1, -1, 0, 0, 0)');
  await ltr(p);
  expect(await matrix(cat.locator('svg'))).toBe('matrix(0, -1, 1, 0, 0, 0)');
});

test('RTL-17 visionneuse d’images : précédente à droite, → revient en arrière, ← passe à la suivante', async ({ user, api }) => {
  const a = await user('rtl17');
  await openGuild(a, api);
  const p = a.page;
  await p.locator('input[type=file]').setInputFiles([
    { name: 'un.png', mimeType: 'image/png', buffer: png(200, 100, 10) },
    { name: 'deux.png', mimeType: 'image/png', buffer: png(100, 200, 90) },
  ]);
  const tray = p.getByRole('list', { name: 'Fichiers à joindre' });
  await expect(tray.getByRole('listitem')).toHaveCount(2);
  await expect(tray.getByRole('progressbar')).toHaveCount(0);
  await p.getByLabel('Écrire dans #général').press('Enter');
  await expect(p.locator('article[data-pending]')).toHaveCount(0);
  await rtl(p, 'ar');
  await messages(p).getByRole('button', { name: "Ouvrir l'image un.png" }).click();
  const viewer = p.getByRole('dialog', { name: "Visionneuse d'images" });
  await expect(viewer).toContainText('1 sur 2');
  const prev = viewer.getByRole('button', { name: 'Image précédente' });
  const next = viewer.getByRole('button', { name: 'Image suivante' });
  rightOf(await rect(prev), await rect(next));
  expect(await attr(prev, 'title')).toBe('Précédente (→)');
  expect(await attr(next, 'title')).toBe('Suivante (←)');
  await p.keyboard.press('ArrowLeft');
  await expect(viewer).toContainText('2 sur 2');
  await p.keyboard.press('ArrowRight');
  await expect(viewer).toContainText('1 sur 2');
  await a11y(p);
});

test('RTL-18 polices du système : arabe et persan, hébreu, interligne ; Inter et JetBrains Mono gardés pour le latin ; aucune police chargée', async ({ user, api }) => {
  const a = await user('rtl18');
  await openGuild(a, api);
  const p = a.page;
  const requests: string[] = [];
  p.on('request', (r) => ['font', 'stylesheet'].includes(r.resourceType()) && requests.push(`${r.resourceType()} ${r.url()}`));
  await post(p, '```\nlet a = 1;\n```', 'let a = 1');
  const body = p.locator('body');
  const font = () => body.evaluate((e) => getComputedStyle(e).fontFamily);
  const line = () => body.evaluate((e) => parseFloat(getComputedStyle(e).lineHeight));
  const latin = await font();
  expect(latin.startsWith('Inter')).toBe(true);
  expect(await line()).toBeCloseTo(22, 1);
  const before = requests.length;
  await rtl(p, 'ar');
  expect(await font()).toMatch(/^Inter, "Segoe UI", Tahoma, "Noto Sans Arabic"/);
  expect(await line()).toBeCloseTo(22 * 1.25, 1);
  await rtl(p, 'fa');
  expect(await font()).toContain('"Noto Sans Arabic"');
  expect(await line()).toBeCloseTo(22 * 1.25, 1);
  await rtl(p, 'he');
  expect(await font()).toMatch(/^Inter, "Segoe UI", "Arial Hebrew", "Noto Sans Hebrew"/);
  expect(await line()).toBeCloseTo(22 * 1.1, 1);
  // Le code garde sa police à chasse fixe, quelle que soit la langue.
  expect(await css(p.locator('pre.app-code code'), 'font-family')).toContain('JetBrains Mono');
  // Le français et l'anglais ne changent pas.
  await ltr(p, 'en');
  expect(await font()).toBe(latin);
  expect(await line()).toBeCloseTo(22, 1);
  expect(requests.length, requests.slice(before).join(', ')).toBe(before);
  expect(await body.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--flip').trim())).toBe('1');
  await rtl(p, 'fa');
  expect(await body.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--flip').trim())).toBe('-1');
});

// ───────── 390 px : aucun débordement, axe ─────────

test.describe('390 px sur chaque écran', () => {
  test.use({ viewport: { width: 390, height: 844 } });

  for (const lang of LANGS) {
    test(`RTL-19 [${lang}] aucun débordement horizontal, nulle part (accueil, onglets, salon, tiroir, réglages, nouveau serveur)`, async ({ user, api }) => {
      const a = await user('rtl19');
      const { gid } = await openGuild(a, api);
      const p = a.page;
      await post(p, 'Un message assez long pour vérifier que la ligne passe à la ligne sans pousser la page de côté, même en droite à gauche');
      await rtl(p, lang);
      const where: string[] = [];
      const check = async (what: string) => {
        const over = await p.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
        if (over > 0) where.push(`${what} : +${over}px`);
      };
      await check('salon');
      await p.getByRole('button', { name: 'Ouvrir la navigation' }).click();
      await check('tiroir ouvert');
      await p.locator('.app-scrim').click({ position: { x: 10, y: 400 } });
      await p.getByRole('button', { name: 'Liste des membres' }).click();
      await check('membres');
      await p.keyboard.press('Escape');
      for (const path of [
        '/channels/@me',
        '/guilds/new',
        '/settings/account',
        '/settings/profiles',
        '/settings/appearance',
        '/settings/language',
        '/settings/keybinds',
        `/guilds/${gid}/settings/roles`,
        `/guilds/${gid}/settings/invites`,
      ]) {
        await p.goto(path);
        await expect(p.locator('#main, .k-settings').first()).toBeVisible();
        await rtl(p, lang);
        await p.waitForTimeout(150);
        await check(path);
      }
      await p.goto('/channels/@me');
      await rtl(p, lang);
      for (const tab of ['Tous', 'En attente', 'Bloqués', 'Ajouter un ami']) {
        await p.getByRole('tab', { name: tab }).click();
        await check(`Amis › ${tab}`);
      }
      expect(where).toEqual([]);
    });
  }

  test('RTL-20 axe à 390 px : salon, tiroir ouvert, réglages', async ({ user, api }) => {
    const a = await user('rtl20');
    await openGuild(a, api);
    const p = a.page;
    await post(p, 'Un message à contrôler');
    await rtl(p, 'fa');
    await a11y(p);
    await p.getByRole('button', { name: 'Ouvrir la navigation' }).click();
    await expect(p.locator('.app-rail')).toBeVisible();
    await a11y(p);
    await p.goto('/settings/language');
    await expect(p.getByRole('dialog', { name: 'Réglages' })).toBeVisible();
    await rtl(p, 'fa');
    await a11y(p);
  });
});

// ───────── gauche à droite : rien ne change ─────────

test('RTL-L1 dir="ltr" rend les mêmes boîtes qu’avant : un aller-retour dir=rtl puis dir=ltr ne déplace rien', async ({ user, api }) => {
  const a = await user('rtll1');
  await openGuild(a, api);
  await post(a.page, 'Message de référence');
  const p = a.page;
  const msg = messages(p).filter({ hasText: 'Message de référence' });
  await msg.hover();
  const snap = async () => ({
    rail: await rect(p.locator('.app-rail')),
    nav: await rect(p.locator('.app-nav')),
    main: await rect(p.locator('.app-main')),
    chat: await rect(p.locator('.app-chat')),
    aside: await rect(p.locator('.app-aside--members')),
    title: await rect(p.getByRole('heading', { name: 'général', level: 1 })),
    tools: await rect(p.locator('.app-header-tools')),
    avatar: await rect(msg.locator('.k-avatar').first()),
    text: await rect(msg.locator('.app-md')),
    actions: await rect(msg.locator('.k-msg-actions')),
    composer: await rect(p.locator('.k-composer')),
    send: await rect(p.getByRole('button', { name: 'Envoyer' })),
    find: await rect(p.locator('.app-find')),
    owner: await rect(p.getByRole('button', { name: /^Mon compte/ })),
  });
  const base = await snap();
  // Boîtes du rendu par défaut : gauche à droite (rail < salons < fil < membres).
  leftOf(base.rail, base.nav);
  leftOf(base.chat, base.aside);
  await rtl(p, 'fa');
  await p.mouse.move(0, 0);
  const flipped = await snap();
  expect(flipped.rail.x, 'le miroir bouge bien quelque chose').not.toBeCloseTo(base.rail.x, 0);
  await ltr(p, 'fr');
  await msg.hover();
  expect(await snap()).toEqual(base);
  // L'anglais explicite (dir=ltr lang=en) n'a pas d'autre géométrie que le français par défaut.
  await ltr(p, 'en');
  expect(await snap()).toEqual(base);
  // `--flip` vaut 1, les icônes ne se retournent pas, aucun `k-flip` actif.
  expect(await p.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--flip').trim())).toBe('1');
  expect(
    await p.evaluate(() => [...document.querySelectorAll('.k-flip')].filter((e) => getComputedStyle(e).transform !== 'none').length),
  ).toBe(0);
});

test('RTL-L2 le serveur de démo et la création d’un serveur en gauche à droite : toujours le même ordre (rail, salons, fil)', async ({ user }) => {
  const a = await user('rtll2');
  const p = a.page;
  await createGuild(p, 'Pont supérieur');
  const rail = await rect(p.locator('.app-rail'));
  const nav = await rect(p.locator('.app-nav'));
  const main = await rect(p.locator('.app-main'));
  near(rail.x, 0, 0.5);
  near(nav.x, rail.w, 0.5);
  near(main.x, rail.w + nav.w, 0.5);
  expect(await p.locator('html').getAttribute('dir')).toBe('ltr');
  expect(await p.locator('html').getAttribute('lang')).toBe('fr');
});

// ───────── suite : bloc de code long, noms saisis, sélecteur rapide, appels sans WebRTC, menu contextuel ─────────

/** Position horizontale (centre) du premier et du dernier caractère de `needle`, dans le premier nœud de texte de `l` qui le contient. */
const ends = (l: Locator, needle: string) =>
  l.evaluate((root, n) => {
    const w = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    for (let t = w.nextNode() as Text | null; t; t = w.nextNode() as Text | null) {
      const i = t.data.indexOf(n);
      if (i < 0) continue;
      const at = (k: number) => {
        const r = document.createRange();
        r.setStart(t, k);
        r.setEnd(t, k + 1);
        const b = r.getBoundingClientRect();
        return b.x + b.width / 2;
      };
      return { first: at(i), last: at(i + n.length - 1) };
    }
    throw new Error(`texte introuvable : ${n}`);
  }, needle);
/** Le texte saisi se lit de gauche à droite (nom latin) : son dernier caractère est à droite du premier, ponctuation comprise. */
const latin = async (l: Locator, needle: string) => {
  const e = await ends(l.first(), needle);
  expect(e.last, `« ${needle} » : le dernier caractère à droite du premier`).toBeGreaterThan(e.first);
};
/** Le texte saisi se lit de droite à gauche (nom arabe ou persan) : son dernier caractère est à gauche du premier. */
const arabic = async (l: Locator, needle: string) => {
  const e = await ends(l.first(), needle);
  expect(e.last, `« ${needle} » : le dernier caractère à gauche du premier`).toBeLessThan(e.first);
};
/** Le premier et le dernier caractère du code d'un bloc, en pixels de la fenêtre. */
const codeEnds = (pre: Locator) =>
  pre.locator('code').evaluate((code) => {
    const t = code.firstChild as Text;
    const at = (k: number) => {
      const r = document.createRange();
      r.setStart(t, k);
      r.setEnd(t, k + 1);
      const b = r.getBoundingClientRect();
      return { x: b.x, r: b.right };
    };
    const n = t.data.replace(/\n$/, '').length;
    return { first: at(0), last: at(n - 1) };
  });

const LONG_CODE =
  'const resultatDeLaRequete = await chargerLesDonneesDuServeur(identifiantDuSalon, identifiantDuMessage, optionsDeLaRequete);';

// RTL-21 : à texte 20 (et zoom 150), un bloc de code long ne pousse ni le message ni la page : il défile dans sa boîte.
for (const [where, width, zoom] of [
  ['390 px', 390, 1],
  ['1536 px, zoom 150', 1536, 1.5],
] as const) {
  test.describe(`RTL-21 texte 20, ${where}`, () => {
    test.use({ viewport: { width, height: 844 } });
    for (const lang of ['fa', 'fr'] as const) {
      test(`RTL-21 [${lang}] bloc de code long à ${where} : il défile dans sa boîte, la page et le message restent dans la fenêtre, axe propre`, async ({ user, api }) => {
        const a = await user('rtl21');
        await openGuild(a, api);
        const p = a.page;
        await p.evaluate(() => {
          localStorage.setItem('kraken.text', '20');
          localStorage.setItem('kraken.zoom', '150');
        });
        await p.reload();
        await expect(p.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
        await expect(p.locator('html')).toHaveAttribute('data-text', '20');
        await expect(p.locator('html')).toHaveAttribute('data-zoom', '150');
        near(parseFloat(await css(p.locator('html'), '--zoom')), zoom, 0.01); // sous 640 px le zoom est gelé à 1
        await post(p, `\`\`\`\n${LONG_CODE}\n\`\`\``, 'const resultatDeLaRequete');
        await post(p, 'Un message court après le code');
        if (lang === 'fa') await rtl(p, 'fa');
        const msg = messages(p).filter({ hasText: 'const resultatDeLaRequete' });
        const pre = msg.locator('pre.app-code');
        await expect(pre).toBeVisible();
        await noOverflow(p);
        // Le message tient dans la fenêtre : l'article, son corps (la colonne du texte) et le bloc.
        const v = p.viewportSize()!;
        const within = async (l: Locator, what: string) => {
          const r = await rect(l);
          expect(r.x, `${what} : bord gauche`).toBeGreaterThanOrEqual(-0.5);
          expect(r.r, `${what} : bord droit`).toBeLessThanOrEqual(v.width + 0.5);
        };
        await within(msg, 'article');
        await within(msg.locator('.app-md'), 'corps du message');
        await within(pre, 'bloc de code');
        const body = await rect(msg.locator('.app-md'));
        const box = await rect(pre);
        expect(box.x, 'le bloc commence dans le corps').toBeGreaterThanOrEqual(body.x - 0.5);
        expect(box.r, 'le bloc finit dans le corps').toBeLessThanOrEqual(body.r + 0.5);
        // Le bloc défile dans sa boîte, de gauche à droite même en droite à gauche.
        expect(await css(pre, 'direction')).toBe('ltr');
        const m = await pre.evaluate((e) => ({ sw: e.scrollWidth, cw: e.clientWidth, sl: e.scrollLeft }));
        expect(m.sw, 'la ligne est plus large que la boîte').toBeGreaterThan(m.cw);
        expect(m.sl).toBe(0);
        const start = await codeEnds(pre);
        expect(start.first.x, 'le début du code est visible').toBeGreaterThanOrEqual(box.x - 0.5);
        expect(start.first.x).toBeLessThanOrEqual(box.r);
        // Au clavier : le bloc prend le focus (région nommée) et les flèches le font défiler.
        expect(await attr(pre, 'tabindex')).toBe('0');
        expect(await attr(pre, 'role')).toBe('region');
        expect(await attr(pre, 'aria-label')).toBe('Bloc de code');
        await pre.focus();
        await expect(pre).toBeFocused();
        await p.keyboard.press('ArrowRight');
        await expect.poll(() => pre.evaluate((e) => e.scrollLeft)).toBeGreaterThan(0);
        // La fin du ligne est atteignable.
        await pre.evaluate((e) => (e.scrollLeft = e.scrollWidth));
        const end = await codeEnds(pre);
        expect(end.last.r, 'la fin du code, une fois défilé, est dans la boîte').toBeLessThanOrEqual(box.r + 0.5);
        expect(end.last.x).toBeGreaterThanOrEqual(box.x - 0.5);
        await noOverflow(p);
        await a11y(p);
      });
    }
  });
}

// RTL-22 : un texte saisi garde son propre sens d'écriture : la ponctuation aux extrémités reste à sa place.
for (const lang of LANGS) {
  test(`RTL-22 [${lang}] noms saisis (rôle, serveur, catégorie, salon, nom affiché, sujet, @tout le monde) : l'ordre visuel est celui du texte saisi`, async ({ user, api }) => {
    test.setTimeout(90_000);
    const a = await user('rtl22');
    const b = await user('rtl22b');
    const asA = await api(a.token);
    const asB = await api(b.token);
    const ALICE = '(Capitaine) Alice!';
    const BABAK = 'بابک (دوست)!';
    const [GUILD, CAT, CHAN, TOPIC] = ['(Récif) corallien!', '(Zone) nord!', '(salon)-test!', '(sujet) du salon!'];
    const [ROLE, ROLE_FA] = ['(Équipe) Alpha!', 'نقش (آلفا)!'];
    expect((await asA.patch(`${API}/users/@me`, { data: { display_name: ALICE } })).status()).toBe(200);
    expect((await asB.patch(`${API}/users/@me`, { data: { display_name: BABAK } })).status()).toBe(200);
    expect((await asA.post(`${API}/users/@me/relationships`, { data: { username: b.name } })).status()).toBe(201);
    expect((await asB.put(`${API}/users/@me/relationships/${a.id}`, { data: { kind: 'friend' } })).status()).toBe(200);
    expect((await asA.post(`${API}/users/@me/channels`, { data: { recipients: [b.id] } })).status()).toBeLessThan(300);
    const { gid, text } = await openGuild(a, api, GUILD);
    const ok = async (r: import('@playwright/test').APIResponse) => {
      expect(r.status(), await r.text()).toBeLessThan(300);
      return r.json();
    };
    const cat = await ok(await asA.post(`${API}/guilds/${gid}/channels`, { data: { name: CAT, kind: 'category' } }));
    const chan = await ok(await asA.post(`${API}/guilds/${gid}/channels`, { data: { name: CHAN, kind: 'text', parent_id: cat.id } }));
    await ok(await asA.patch(`${API}/channels/${chan.id}`, { data: { topic: TOPIC } }));
    const role = await ok(await asA.post(`${API}/guilds/${gid}/roles`, { data: { name: ROLE, permissions: 0, hoist: true, color: 3 } }));
    const roleFa = await ok(await asA.post(`${API}/guilds/${gid}/roles`, { data: { name: ROLE_FA, permissions: 0, hoist: true, color: 5 } }));
    const { code } = await ok(await asA.post(`${API}/channels/${text}/invites`, { data: {} }));
    await ok(await asB.post(`${API}/invites/${code}`));
    await ok(await asA.put(`${API}/guilds/${gid}/members/${a.id}/roles/${role.id}`));
    await ok(await asA.put(`${API}/guilds/${gid}/members/${b.id}/roles/${roleFa.id}`));
    await ok(await asA.post(`${API}/channels/${chan.id}/messages`, { data: { content: 'Bonjour le récif' } }));
    const p = a.page;
    const visit = async (path: string) => {
      await p.goto(path);
      await expect(p.locator('#main, .k-settings').first()).toBeVisible();
      await rtl(p, lang);
    };

    // Barre latérale, en-tête du salon, sujet, liste des membres par rôle, auteur du message.
    await visit(`/channels/${gid}/${chan.id}`);
    const nav = p.locator('.app-nav');
    await expect(nav.locator('.app-guild-head h2')).toContainText(GUILD);
    await latin(nav.locator('.app-guild-head h2'), GUILD);
    await latin(nav.locator('.k-category', { hasText: CAT }), CAT);
    await latin(nav.locator('.k-channel', { hasText: CHAN }).locator('.app-ellipsis'), CHAN);
    await latin(p.getByRole('heading', { level: 1 }), CHAN);
    await latin(p.locator('.app-topic'), TOPIC);
    const aside = p.getByRole('complementary', { name: 'Membres' });
    await latin(aside.locator('.app-group', { hasText: ROLE }), ROLE);
    await arabic(aside.locator('.app-group', { hasText: ROLE_FA }), ROLE_FA);
    await latin(aside.locator('.app-who', { hasText: ALICE }).locator('bdi').first(), ALICE);
    await arabic(aside.locator('.app-who', { hasText: BABAK }).locator('bdi').first(), BABAK);
    const msg = messages(p).filter({ hasText: 'Bonjour le récif' });
    await latin(msg.locator('header bdi'), ALICE);
    // La carte de profil : le nom, et les pastilles de rôle.
    await msg.locator('.app-who-name').click();
    const card = p.getByRole('dialog', { name: /^Profil de / }); // le nom y est isolé (FSI/PDI) dans une page de droite à gauche
    await expect(card).toBeVisible();
    await latin(card.locator('.app-card-name'), ALICE);
    await latin(card.locator('.app-chip', { hasText: ROLE }), ROLE);
    await p.keyboard.press('Escape');
    await expect(card).toBeHidden();
    // Amis et messages privés : le nom affiché persan reste persan.
    await visit('/channels/@me');
    await p.getByRole('tab', { name: 'Tous' }).click();
    await arabic(p.locator('.app-row b bdi', { hasText: BABAK }), BABAK);
    await arabic(p.locator('.app-sidebar .k-dm > div bdi', { hasText: BABAK }), BABAK);

    // Réglages › Rôles : la liste et le titre, « @tout le monde », et « @everyone » d'une description.
    await visit(`/guilds/${gid}/settings/roles`);
    const list = p.getByRole('list', { name: 'Rôles, du plus haut au plus bas' });
    const title = p.locator('.app-roles-title');
    await list.locator('.app-role-pick', { hasText: ROLE }).click();
    await latin(list.locator('.app-role-pick', { hasText: ROLE }).locator('.app-ellipsis'), ROLE);
    await latin(title, ROLE);
    await list.locator('.app-role-pick', { hasText: ROLE_FA }).click();
    await arabic(list.locator('.app-role-pick', { hasText: ROLE_FA }).locator('.app-ellipsis'), ROLE_FA);
    await arabic(title, ROLE_FA);
    await list.locator('.app-role--base .app-role-pick').click();
    await latin(list.locator('.app-role--base .app-ellipsis'), '@tout le monde');
    await latin(title, '@tout le monde');
    await p.getByRole('tab', { name: 'Permissions' }).click();
    await latin(p.locator('.k-setting p', { hasText: '@everyone' }), '@everyone');
    await a11y(p);
  });
}

// RTL-23 : sélecteur rapide : l'alignement du nom suit le sens de l'interface, pas celui du nom saisi.
test('RTL-23 sélecteur rapide (Ctrl+K) : un nom persan reste calé au début de la ligne en gauche à droite, un nom latin au début en droite à gauche', async ({ user, api }) => {
  const a = await user('rtl23');
  const b = await user('rtl23b');
  const asA = await api(a.token);
  const asB = await api(b.token);
  expect((await asB.patch(`${API}/users/@me`, { data: { display_name: 'بابک' } })).status()).toBe(200);
  expect((await asA.post(`${API}/users/@me/relationships`, { data: { username: b.name } })).status()).toBe(201);
  expect((await asB.put(`${API}/users/@me/relationships/${a.id}`, { data: { kind: 'friend' } })).status()).toBe(200);
  await openGuild(a, api);
  const p = a.page;
  await p.keyboard.press('Control+k');
  const field = p.locator('.app-sw-field .k-input');
  await expect(field).toBeVisible();
  // Un nom persan (un ami) et un nom latin (le salon « général »), cherchés l'un après l'autre : où est le nom dans sa ligne ?
  const place = async (query: string, needle: string) => {
    await field.fill(query);
    const row = p.locator('.app-sw-opt').filter({ hasText: needle }).first();
    await expect(row).toBeVisible();
    const span = await rect(row.locator('.app-sw-name'));
    const name = await rect(row.locator('.app-sw-name bdi'));
    return { start: name.x - span.x, end: span.r - name.r, x: name.x, dir: await attr(row.locator('.app-sw-name'), 'dir') };
  };
  // Gauche à droite (le français, comme main) : les deux noms au début de la ligne (à gauche), sans `dir="auto"` sur la ligne.
  const fa = await place('بابک', 'بابک');
  const la = await place('général', 'général');
  expect(fa.dir).toBeNull();
  near(fa.start, 0, 1);
  near(la.start, 0, 1);
  near(fa.x, la.x, 1);
  // Droite à gauche : les deux noms au début de la ligne (à droite).
  await rtl(p, 'fa');
  const fa2 = await place('بابک', 'بابک');
  const la2 = await place('général', 'général');
  near(fa2.end, 0, 1);
  near(la2.end, 0, 1);
  await a11y(p);
  await ltr(p, 'fr');
  near((await place('بابک', 'بابک')).start, 0, 1);
});

// RTL-24 : sans WebRTC, les boutons d'appel grisés disent pourquoi (infobulle et texte relié), au lieu d'un grisé muet.
test('RTL-24 WebRTC absent : « Appeler », « Démarrer un appel », « Rejoindre l’appel » et « Rejoindre » grisés expliquent pourquoi', async ({ user, api }) => {
  test.setTimeout(90_000);
  const a = await user('rtl24');
  const b = await user('rtl24b');
  const asA = await api(a.token);
  const asB = await api(b.token);
  expect((await asA.post(`${API}/users/@me/relationships`, { data: { username: b.name } })).status()).toBe(201);
  expect((await asB.put(`${API}/users/@me/relationships/${a.id}`, { data: { kind: 'friend' } })).status()).toBe(200);
  const dm = (await (await asA.post(`${API}/users/@me/channels`, { data: { recipients: [b.id] } })).json()).id as string;
  // B n'a pas WebRTC.
  await b.context.addInitScript(() => {
    delete (window as unknown as Record<string, unknown>).RTCPeerConnection;
    delete (window as unknown as Record<string, unknown>).webkitRTCPeerConnection;
  });
  const p = b.page;
  await p.goto('/channels/@me');
  const WHY = /WebRTC y est absent ou désactivé/;
  /** Le bouton est grisé, son infobulle dit pourquoi, et un texte relié par aria-describedby le dit aussi. */
  const explained = async (btn: Locator) => {
    await expect(btn).toBeDisabled();
    await expect(btn).toHaveAttribute('title', WHY);
    const id = await btn.getAttribute('aria-describedby');
    expect(id, 'aria-describedby').toBeTruthy();
    await expect(p.locator(`[id="${id}"]`)).toContainText(WHY);
  };
  // Amis : « Appeler ».
  await p.getByRole('tab', { name: 'Tous' }).click();
  await explained(p.getByRole('button', { name: `Appeler ${a.name}` }));
  await a11y(p);
  // Le MP, sans appel : « Démarrer un appel » dans l'en-tête.
  await p.goto(`/channels/@me/${dm}`);
  await expect(p.getByRole('heading', { level: 1 })).toBeVisible();
  await explained(p.getByRole('button', { name: 'Démarrer un appel' }));
  // A appelle : B voit la carte d'appel entrant (« Rejoindre » grisé), puis, dans le MP, « Rejoindre l'appel » grisé avec la phrase en ligne.
  await a.page.goto(`/channels/@me/${dm}`);
  await a.page.getByRole('button', { name: 'Démarrer un appel' }).click();
  await expect(a.page.getByRole('region', { name: 'Connexion vocale' }).getByRole('status')).toHaveText('En appel', { timeout: 20_000 });
  await p.goto('/channels/@me');
  const ring = p.getByRole('region', { name: 'Appels entrants' }).getByRole('alertdialog', { name: `Appel de ${a.name}` });
  await expect(ring).toBeVisible();
  await explained(ring.getByRole('button', { name: 'Rejoindre' }));
  await a11y(p);
  await p.goto(`/channels/@me/${dm}`);
  const lobby = p.getByRole('region', { name: 'Appel', exact: true });
  await explained(lobby.getByRole('button', { name: "Rejoindre l'appel" }));
  await expect(lobby.getByRole('paragraph').filter({ hasText: WHY })).toBeVisible();
  await a11y(p);
  // Avec WebRTC (le compte A), rien n'est grisé ni expliqué.
  await expect(a.page.getByRole('button', { name: 'Démarrer un appel' })).toHaveCount(0);
});

// RTL-25 : le menu contextuel se pose d'après sa taille mesurée : jamais hors d'une fenêtre basse, quelle que soit la langue.
for (const lang of LANGS) {
  test.describe(`RTL-25 fenêtre basse [${lang}]`, () => {
    test.use({ viewport: { width: 1000, height: 300 } });
    test(`RTL-25 [${lang}] menu contextuel dans une fenêtre de 300 px de haut : entier dans la fenêtre, près du curseur`, async ({ user, api }) => {
      const a = await user('rtl25');
      await openGuild(a, api);
      const p = a.page;
      await rtl(p, lang);
      const channel = p.locator('.app-sidebar').getByRole('link', { name: 'général', exact: true });
      const menu = p.getByRole('menu');
      const v = p.viewportSize()!;
      for (const [x, y] of [
        [500, 295],
        [500, 299],
        [6, 290],
        [v.width - 6, 296],
      ]) {
        await channel.evaluate(
          (e, at) => e.dispatchEvent(new MouseEvent('contextmenu', { clientX: at[0], clientY: at[1], bubbles: true, cancelable: true })),
          [x, y],
        );
        await expect(menu).toBeVisible();
        const m = await rect(menu);
        expect(m.b, `bas du menu (${x}, ${y})`).toBeLessThanOrEqual(v.height + 0.5);
        expect(m.y, `haut du menu (${x}, ${y})`).toBeGreaterThanOrEqual(-0.5);
        await inView(p, m, `menu contextuel (${x}, ${y})`);
        // Sa hauteur est mesurée, pas supposée : le menu entier tient, à n'importe quelle hauteur.
        expect(m.h).toBeGreaterThan(100);
        await p.keyboard.press('Escape');
        await expect(menu).toBeHidden();
      }
    });
  });
}
