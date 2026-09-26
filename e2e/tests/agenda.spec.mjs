// Parcours principaux de l'interface, contre « agenda serve » sur un dossier d'exemple.
import { test, expect } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { DIR } from '../playwright.config.mjs';

const CAP = path.resolve('captures');
fs.mkdirSync(CAP, { recursive: true });

async function api(request, method, params = {}) {
  const r = await request.post(`/api/${method}`, { data: params, headers: { 'Content-Type': 'application/json' } });
  const j = await r.json();
  if (!j.ok) throw new Error(j.error);
  return j.result;
}

async function open(page, view) {
  await page.addInitScript((v) => {
    localStorage.setItem('agenda.prefs', JSON.stringify({ view: v, theme: 'light' }));
  }, view);
  await page.goto('/');
  await page.waitForSelector('body[data-ready="1"]');
  await page.waitForTimeout(250);
}

async function shot(page, name) {
  await page.waitForTimeout(350);
  await page.screenshot({ path: path.join(CAP, `capture-${name}.png`) });
}

test.describe('bureau', () => {
  test.use({ viewport: { width: 1440, height: 900 } });

  test('les quatre vues s’affichent', async ({ page }) => {
    const errors = [];
    page.on('pageerror', (e) => errors.push(e.message));
    await open(page, 'week');
    await expect(page.locator('.ev', { hasText: 'Dentiste' })).toBeVisible();
    await shot(page, 'semaine');
    await page.click('.segmented [data-view="month"]');
    await expect(page.locator('.month-grid')).toBeVisible();
    await expect(page.locator('.chip', { hasText: 'Concert' })).toBeVisible();
    await shot(page, 'mois');
    await page.click('.segmented [data-view="agenda"]');
    await expect(page.locator('.hero')).toBeVisible();
    await expect(page.getByText('Payer le loyer')).toBeVisible();
    await shot(page, 'jour');
    await page.click('.segmented [data-view="tasks"]');
    await expect(page.locator('.lane[data-status="doing"] .tcard', { hasText: 'Rendre le rapport' })).toBeVisible();
    await shot(page, 'taches');
    expect(errors).toEqual([]);
  });

  test('créer en langage naturel avec aperçu', async ({ page, request }) => {
    await open(page, 'week');
    await page.keyboard.press('n');
    const input = page.locator('.palette input');
    await input.fill('Réunion test demain 15h-16h @Salle_B #e2e');
    await expect(page.locator('.opt.create')).toContainText('15:00 → 16:00');
    await expect(page.locator('.opt.create')).toContainText('@Salle B');
    await shot(page, 'palette');
    await page.keyboard.press('Enter');
    await expect(page.locator('.toast', { hasText: 'Créé' })).toBeVisible();
    // notre propre écriture ne doit pas être annoncée comme une modification externe
    await page.waitForTimeout(1500);
    await expect(page.locator('.toast', { hasText: 'modification externe' })).toHaveCount(0);
    await expect(page.locator('.ev', { hasText: 'Réunion test' })).toBeVisible();
    const found = await api(request, 'search', { q: 'reunion test' });
    expect(found[0].title).toBe('Réunion test');
    const item = await api(request, 'get', { id: found[0].id });
    expect(item.location).toBe('Salle B');
    expect(item.tags).toEqual(['e2e']);
    expect(item.raw).toContain('start: ');
  });

  test('déplacer à la souris puis annuler', async ({ page, request }) => {
    await open(page, 'week');
    const ev = page.locator('.ev', { hasText: 'Dentiste' });
    const id = (await api(request, 'search', { q: 'dentiste' }))[0].id;
    const before = (await api(request, 'get', { id })).start;
    const box = await ev.boundingBox();
    const hour = await page.evaluate(() => parseFloat(getComputedStyle(document.querySelector('.week-box')).getPropertyValue('--hour')));
    await page.mouse.move(box.x + box.width / 2, box.y + 8);
    await page.mouse.down();
    for (let i = 1; i <= 10; i++) await page.mouse.move(box.x + box.width / 2, box.y + 8 + (i * 2 * hour) / 10);
    await expect(page.locator('.drag-ghost')).toContainText('16:00');
    await page.mouse.up();
    await expect(page.locator('.toast').last()).toContainText('déplacé');
    await expect.poll(async () => (await api(request, 'get', { id })).start).toBe(before.replace('14:00', '16:00'));
    await page.locator('.toast button', { hasText: 'Annuler' }).last().click();
    await expect.poll(async () => (await api(request, 'get', { id })).start).toBe(before);
    // raccourci clavier : rétablir puis annuler
    await page.keyboard.press('Control+Shift+Z');
    await expect.poll(async () => (await api(request, 'get', { id })).start).toBe(before.replace('14:00', '16:00'));
    await page.keyboard.press('Control+Z');
    await expect.poll(async () => (await api(request, 'get', { id })).start).toBe(before);
  });

  test('créer en sélectionnant un créneau', async ({ page, request }) => {
    await open(page, 'week');
    const hour = await page.evaluate(() => parseFloat(getComputedStyle(document.querySelector('.week-box')).getPropertyValue('--hour')));
    await page.locator('.week-body').evaluate((b, h) => {
      b.style.scrollBehavior = 'auto';
      b.scrollTop = 13 * h;
    }, hour);
    const col = page.locator('.day-col').nth(2);
    const box = await col.boundingBox();
    const y0 = box.y + 16 * hour;
    await page.mouse.move(box.x + 30, y0 + 2);
    await page.mouse.down();
    for (let i = 1; i <= 6; i++) await page.mouse.move(box.x + 30, y0 + 2 + (i * 1.5 * hour) / 6);
    await page.mouse.up();
    await expect(page.locator('.sheet')).toBeVisible();
    await expect(page.locator('#ed-st')).toHaveValue('16:00');
    await expect(page.locator('#ed-et')).toHaveValue('17:30');
    await page.fill('#ed-title', 'Créneau sélectionné');
    await page.click('.sheet-foot .btn.primary');
    await expect(page.locator('.ev', { hasText: 'Créneau sélectionné' })).toBeVisible();
    const hit = (await api(request, 'search', { q: 'creneau selectionne' }))[0];
    expect(hit.when.endsWith('16:00')).toBeTruthy();
  });

  test('recherche insensible aux accents', async ({ page }) => {
    await open(page, 'week');
    await page.keyboard.press('Control+k');
    await page.locator('.palette input').fill('reunion stage');
    await expect(page.locator('.opt', { hasText: 'Réunion de stage' })).toBeVisible();
    await page.keyboard.press('Enter');
    await expect(page.locator('.sheet #ed-title')).toHaveValue('Réunion de stage');
  });

  test('thème sombre', async ({ page }) => {
    await open(page, 'week');
    await page.click('[data-act="theme"]');
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await shot(page, 'sombre');
    await page.click('.segmented [data-view="month"]');
    await shot(page, 'sombre-mois');
    await page.click('[data-act="theme"]');
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  });

  test('tâches : ajout rapide et case à cocher', async ({ page, request }) => {
    await open(page, 'tasks');
    await page.fill('.task-add input', 'Acheter des timbres demain !haute');
    await page.click('.task-add button');
    const card = page.locator('.tcard', { hasText: 'Acheter des timbres' });
    await expect(card).toBeVisible();
    await expect(card).toContainText('haute');
    await card.locator('[data-check]').click();
    await expect(page.locator('.lane[data-status="done"] .tcard', { hasText: 'Acheter des timbres' })).toBeVisible();
    const t = (await api(request, 'search', { q: 'timbres' }))[0];
    expect(t.status).toBe('done');
  });

  test('modification externe détectée en direct', async ({ page }) => {
    await open(page, 'tasks');
    fs.writeFileSync(path.join(DIR, 'tasks', 'hermes.md'), '---\ntitle: Ajoutée par Hermes\nstatus: todo\n---\n');
    await expect(page.locator('.tcard', { hasText: 'Ajoutée par Hermes' })).toBeVisible({ timeout: 10000 });
    await expect(page.locator('.toast', { hasText: 'modification externe' })).toBeVisible();
  });

  test('conflit Syncthing signalé puis résolu', async ({ page }) => {
    await open(page, 'agenda');
    const conflict = path.join(DIR, 'tasks', 'dm.sync-conflict-20260924-101010-ABCDEFG.md');
    fs.writeFileSync(conflict, fs.readFileSync(path.join(DIR, 'tasks', 'dm.md'), 'utf8').replace('DM de probabilités', 'DM de probas (téléphone)'));
    await expect(page.locator('#banner')).toContainText('conflit', { timeout: 10000 });
    await shot(page, 'conflit');
    await page.click('#banner button');
    await expect(page.locator('.conflict')).toContainText('DM de probas (téléphone)');
    await page.locator('.conflict [data-keep="conflict"]').click();
    await expect(page.locator('#banner')).toBeHidden();
    expect(fs.existsSync(conflict)).toBeFalsy();
    expect(fs.readFileSync(path.join(DIR, 'tasks', 'dm.md'), 'utf8')).toContain('DM de probas (téléphone)');
  });
});

test.describe('téléphone', () => {
  test.use({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true, deviceScaleFactor: 2 });

  test('vues mobiles', async ({ page }) => {
    await open(page, 'agenda');
    await expect(page.locator('.tabbar')).toBeVisible();
    await expect(page.locator('.fab')).toBeVisible();
    await shot(page, 'mobile-jour');
    await page.locator('.tabbar [data-view="week"]').tap();
    await expect(page.locator('.week-head .h[data-day]')).toHaveCount(3);
    await shot(page, 'mobile-semaine');
    await page.locator('.tabbar [data-view="month"]').tap();
    await expect(page.locator('.month.compact')).toBeVisible();
    await shot(page, 'mobile-mois');
    await page.locator('.tabbar [data-view="tasks"]').tap();
    await shot(page, 'mobile-taches');
    await page.locator('.fab').tap();
    await expect(page.locator('.palette')).toBeVisible();
    await page.locator('.palette input').fill('Yoga tous les lundis et jeudis à 7h');
    await expect(page.locator('.opt.create')).toContainText('chaque semaine le lundi, jeudi');
    await shot(page, 'mobile-palette');
  });

  test('glisser au doigt après un appui long', async ({ page, request }) => {
    await open(page, 'week');
    // amène la vue sur vendredi pour y trouver le dentiste
    const id = (await api(request, 'search', { q: 'dentiste' }))[0].id;
    const ev0 = await api(request, 'get', { id });
    await page.evaluate((d) => window.agendaApp.go(Math.floor(Date.UTC(+d.slice(0, 4), +d.slice(5, 7) - 1, +d.slice(8, 10)) / 864e5)), ev0.start);
    await page.waitForTimeout(300);
    const ev = page.locator('.ev', { hasText: 'Dentiste' });
    await ev.scrollIntoViewIfNeeded();
    const box = await ev.boundingBox();
    const hour = await page.evaluate(() => parseFloat(getComputedStyle(document.querySelector('.week-box')).getPropertyValue('--hour')));
    const cdp = await page.context().newCDPSession(page);
    const x = box.x + box.width / 2;
    const y = box.y + 6;
    const touch = (type, yy) => cdp.send('Input.dispatchTouchEvent', { type, touchPoints: type === 'touchEnd' ? [] : [{ x, y: yy, id: 1 }] });
    await touch('touchStart', y);
    await page.waitForTimeout(600); // appui long
    for (let i = 1; i <= 8; i++) {
      await touch('touchMove', y + (i * hour) / 8);
      await page.waitForTimeout(16);
    }
    await touch('touchEnd', y + hour);
    await expect.poll(async () => (await api(request, 'get', { id })).start).toBe(ev0.start.replace('14:00', '15:00'));
    await api(request, 'undo');
  });
});
