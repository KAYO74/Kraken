// Fixtures communes : garde-fous (erreurs console, réponses 5xx), comptes jetables, accès API.
import { test as base, expect, request, type APIRequestContext, type BrowserContext, type Page } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

export { expect };
export const PASSWORD = 'motdepasse-solide';
export const DEMO_PASSWORD = 'kraken-demo';
export const API = '/api/v1';

let n = 0;
/** Pseudo unique (32 caractères au plus) pour que les tests puissent tourner en parallèle. */
export const unique = (prefix: string) =>
  `${prefix}_${Date.now().toString(36).slice(-5)}${(n++).toString(36)}${Math.random().toString(36).slice(2, 5)}`;

export type User = { id: string; name: string; email: string; token: string; page: Page; context: BrowserContext };

type Fixtures = {
  /** Problèmes relevés sur toutes les pages du test ; le test échoue s'il en reste. */
  issues: string[];
  /** Erreurs attendues par un test (hors ligne, mauvais mot de passe…). */
  allow: RegExp[];
  /** Nouveau compte, connecté, sur l'accueil. */
  user: (prefix?: string) => Promise<User>;
  /** Compte de démo (mot de passe connu), connecté. */
  demo: (login: string) => Promise<User>;
  /** Client HTTP authentifié par l'en-tête `Authorization`, comme un bot ou une appli native. */
  api: (token?: string) => Promise<APIRequestContext>;
};

export function watch(page: Page, issues: string[]) {
  page.on('pageerror', (e) => issues.push(`pageerror: ${e.message}`));
  page.on('console', (m) => m.type() === 'error' && issues.push(`console: ${m.text()}`));
  page.on('response', (r) => r.status() >= 500 && issues.push(`${r.status()} ${r.url()}`));
}

/** Analyse axe (WCAG 2.1 A et AA) de la page telle qu'elle est affichée. */
export async function a11y(page: Page) {
  const { violations } = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa']).analyze();
  expect(violations.map((v) => `${v.id}: ${v.nodes.map((x) => x.target.join(' ')).join(', ')}`)).toEqual([]);
}

export const test = base.extend<Fixtures>({
  allow: async ({}, use) => use([]),
  issues: async ({ allow }, use) => {
    const issues: string[] = [];
    await use(issues);
    expect(issues.filter((i) => !allow.some((a) => a.test(i)))).toEqual([]);
  },
  page: async ({ page, issues }, use) => {
    watch(page, issues);
    await use(page);
  },
  api: async ({ baseURL }, use) => {
    const made: APIRequestContext[] = [];
    await use(async (token) => {
      const ctx = await request.newContext({
        baseURL,
        extraHTTPHeaders: token ? { Authorization: `Session ${token}` } : {},
      });
      made.push(ctx);
      return ctx;
    });
    for (const c of made) await c.dispose();
  },
  user: async ({ browser, baseURL, viewport, locale, issues }, use) => {
    const contexts: BrowserContext[] = [];
    const open = async (body: object, path: string): Promise<User> => {
      const context = await browser.newContext({ baseURL, viewport, locale });
      contexts.push(context);
      const res = await context.request.post(`${API}${path}`, { data: body });
      expect(res.status(), await res.text()).toBeLessThan(300);
      const { token, user } = await res.json();
      const page = await context.newPage();
      watch(page, issues);
      await page.goto('/login');
      await page.evaluate(() => localStorage.setItem('kraken.signed_in', '1'));
      await page.goto('/channels/@me');
      await expect(page.getByRole('heading', { name: 'Amis', level: 1 })).toBeVisible();
      return { id: user.id, name: user.username, email: (body as { email?: string }).email ?? '', token, page, context };
    };
    await use(async (prefix = 'u') => {
      const name = unique(prefix);
      return open({ username: name, email: `${name}@example.org`, password: PASSWORD }, '/auth/register');
    });
    for (const c of contexts) await c.close();
  },
  demo: async ({ browser, baseURL, viewport, locale, issues }, use) => {
    const contexts: BrowserContext[] = [];
    await use(async (login) => {
      const context = await browser.newContext({ baseURL, viewport, locale });
      contexts.push(context);
      const res = await context.request.post(`${API}/auth/login`, { data: { login, password: DEMO_PASSWORD } });
      expect(res.status(), await res.text()).toBe(200);
      const { token, user } = await res.json();
      const page = await context.newPage();
      watch(page, issues);
      await page.goto('/login');
      await page.evaluate(() => localStorage.setItem('kraken.signed_in', '1'));
      await page.goto('/channels/@me');
      await expect(page.getByRole('heading', { name: 'Amis', level: 1 })).toBeVisible();
      return { id: user.id, name: user.username, email: '', token, page, context };
    });
    for (const c of contexts) await c.close();
  },
});

/** Crée un serveur depuis l'interface et attend son premier salon. Renvoie [guildId, channelId]. */
export async function createGuild(page: Page, name: string): Promise<[string, string]> {
  await page.getByRole('link', { name: 'Créer ou rejoindre un serveur' }).click();
  await page.getByLabel('Nom du serveur').fill(name);
  await page.getByRole('button', { name: 'Créer', exact: true }).click();
  await page.waitForURL(/\/channels\/[0-9a-f-]{36}\/[0-9a-f-]{36}$/);
  await expect(page.getByRole('heading', { name: 'général', level: 1 })).toBeVisible();
  const [, , gid, cid] = new URL(page.url()).pathname.split('/');
  return [gid, cid];
}

/** Envoie un message avec Entrée dans le salon affiché. */
export async function say(page: Page, channel: string, text: string) {
  const box = page.getByLabel(`Écrire dans #${channel}`);
  await box.fill(text);
  await box.press('Enter');
}

/** Messages du fil affiché, y compris les envois en cours. */
export const messages = (page: Page) => page.getByRole('log').getByRole('article');
