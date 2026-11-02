// MP et groupes : fermer un MP (il revient avec le prochain message), propriétaire d'un groupe
// (couronne, retrait d'un membre après confirmation).
import { test, expect, API, type User } from '../fixtures';
import type { APIRequestContext } from '@playwright/test';

type Api = (t?: string) => Promise<APIRequestContext>;

async function befriend(api: Api, a: User, b: User) {
  expect((await (await api(a.token)).post(`${API}/users/@me/relationships`, { data: { username: b.name } })).status()).toBe(201);
  expect((await (await api(b.token)).put(`${API}/users/@me/relationships/${a.id}`, { data: { kind: 'friend' } })).status()).toBe(200);
}

test('dm-H1 fermer un MP : il quitte la liste et revient avec un nouveau message', async ({ user, api }) => {
  const a = await user('alix');
  const b = await user('bruno');
  await befriend(api, a, b);
  const asA = await api(a.token);
  const dm = await (await asA.post(`${API}/users/@me/channels`, { data: { recipients: [b.id] } })).json();
  const nav = a.page.getByRole('navigation', { name: 'Messages privés' });
  const row = nav.getByRole('link', { name: new RegExp(b.name) });
  await expect(row).toBeVisible();

  await row.hover();
  await nav.getByRole('button', { name: `Fermer la conversation avec ${b.name}` }).click();
  await expect(row).toHaveCount(0);
  // Chez l'autre, rien ne change.
  await expect(b.page.getByRole('navigation', { name: 'Messages privés' }).getByRole('link', { name: new RegExp(a.name) })).toBeVisible();

  const asB = await api(b.token);
  expect((await asB.post(`${API}/channels/${dm.id}/messages`, { data: { content: 'de retour ?' } })).status()).toBe(201);
  await expect(row).toBeVisible();
  // Au clavier : la croix prend le focus avec la ligne.
  await row.focus();
  await a.page.keyboard.press('Tab');
  await expect(nav.getByRole('button', { name: `Fermer la conversation avec ${b.name}` })).toBeFocused();
});

test('groupe-H1 le propriétaire a une couronne et peut retirer un membre', async ({ user, api }) => {
  const a = await user('cleo');
  const b = await user('dario');
  const c = await user('enzo');
  await befriend(api, a, b);
  await befriend(api, a, c);
  const g = await (await (await api(a.token)).post(`${API}/users/@me/channels`, { data: { recipients: [b.id, c.id], name: 'Les trois' } })).json();
  expect(g.owner_id).toBe(a.id);

  await a.page.goto(`/channels/@me/${g.id}`);
  const members = a.page.getByRole('complementary', { name: 'Membres du groupe' });
  await expect(members.getByRole('listitem')).toHaveCount(3);
  await expect(members.getByRole('listitem').filter({ hasText: '(propriétaire)' })).toHaveCount(1);
  await expect(members.getByRole('listitem').filter({ hasText: '(propriétaire)' })).toContainText(a.name);

  // Un simple membre ne voit pas le menu de retrait.
  await b.page.goto(`/channels/@me/${g.id}`);
  const bMembers = b.page.getByRole('complementary', { name: 'Membres du groupe' });
  await expect(bMembers.getByRole('listitem')).toHaveCount(3);
  await expect(bMembers.getByRole('button', { name: /Plus d'actions/ })).toHaveCount(0);

  await members.getByRole('listitem').filter({ hasText: b.name }).hover();
  await members.getByRole('button', { name: `Plus d'actions pour ${b.name}` }).click();
  await members.getByRole('button', { name: 'Retirer du groupe' }).click();
  const confirm = a.page.getByRole('alertdialog', { name: new RegExp(`Retirer ${b.name} du groupe`) });
  await expect(confirm).toBeVisible();
  await confirm.getByRole('button', { name: 'Retirer du groupe' }).click();
  await expect(members.getByRole('listitem')).toHaveCount(2);
  // Chez enzo, la liste se met à jour ; dario perd le groupe.
  await c.page.goto(`/channels/@me/${g.id}`);
  await expect(c.page.getByRole('complementary', { name: 'Membres du groupe' }).getByRole('listitem')).toHaveCount(2);
  await expect(b.page.getByRole('navigation', { name: 'Messages privés' }).getByRole('link', { name: /Les trois/ })).toHaveCount(0);
});

test('serveur-E1 un nom d’un seul caractère annonce la vraie règle', async ({ user, api }) => {
  const a = await user('fanny');
  const res = await (await api(a.token)).post(`${API}/guilds`, { data: { name: 'x' } });
  expect(res.status()).toBe(400);
  expect((await res.json()).message).toContain('2 à 100 caractères');
});
