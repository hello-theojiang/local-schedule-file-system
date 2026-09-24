// Point d'entrée de l'interface.
import { call, host, ApiError } from './api.js';
import { $, $$, esc, icon, todayN, weekStart, monthStart, addMonths, ymd, ds, MONTHS, MONTHS_SHORT, isNarrow, cap, h } from './util.js';
import { renderMonth } from './views/month.js';
import { renderWeek } from './views/week.js';
import { renderAgenda } from './views/agenda.js';
import { renderTasks } from './views/tasks.js';
import { renderMini, renderCalendars } from './views/sidebar.js';
import { openPalette } from './palette.js';
import { openEditor, openNew } from './editor.js';
import { openSettings, openConflicts, renderOnboarding } from './settings.js';
import { scheduleReminders } from './notify.js';

const PREFS_KEY = 'agenda.prefs';
function loadPrefs() {
  try {
    return JSON.parse(localStorage.getItem(PREFS_KEY) || '{}');
  } catch (e) {
    return {};
  }
}

export const app = {
  prefs: loadPrefs(),
  state: {
    view: null,
    cursor: todayN(),
    events: [],
    tasks: [],
    calendars: [],
    version: 0,
    info: null,
    hostInfo: {},
    agendaDays: 21,
    selDay: todayN(),
    ready: false,
  },
  call,
  host,
};
const S = app.state;
S.view = app.prefs.view || (isNarrow() ? 'agenda' : 'week');

app.savePrefs = () => {
  try {
    localStorage.setItem(PREFS_KEY, JSON.stringify(app.prefs));
  } catch (e) {}
};

// ------------------------------------------------------------ thème
const media = matchMedia('(prefers-color-scheme: dark)');
app.applyTheme = () => {
  const t = app.prefs.theme === 'dark' || (app.prefs.theme !== 'light' && media.matches) ? 'dark' : 'light';
  document.documentElement.dataset.theme = t;
  document.documentElement.dataset.accent = app.prefs.accent || 'violet';
  const meta = document.querySelector('meta[name="theme-color"]');
  if (meta) meta.content = t === 'dark' ? '#121116' : '#f7f6f3';
};
media.addEventListener('change', app.applyTheme);
app.toggleTheme = () => {
  const cur = document.documentElement.dataset.theme;
  app.prefs.theme = cur === 'dark' ? 'light' : 'dark';
  app.savePrefs();
  app.applyTheme();
};

// ------------------------------------------------------------ toasts
app.toast = (msg, opts = {}) => {
  const box = $('#toasts');
  const el = h(`<div class="toast ${opts.error ? 'error' : ''}"><span>${esc(msg)}</span></div>`);
  if (opts.undo) {
    const b = h('<button>Annuler</button>');
    b.onclick = () => {
      dismiss();
      app.undo();
    };
    el.append(b);
  }
  if (opts.action) {
    const b = h(`<button>${esc(opts.action.label)}</button>`);
    b.onclick = () => {
      dismiss();
      opts.action.run();
    };
    el.append(b);
  }
  box.append(el);
  const dismiss = () => {
    el.classList.add('leave');
    setTimeout(() => el.remove(), 220);
  };
  setTimeout(dismiss, opts.error ? 6000 : opts.undo ? 5500 : 3200);
  while (box.children.length > 3) box.firstChild.remove();
};
app.error = (e) => app.toast(e instanceof ApiError || e instanceof Error ? e.message : String(e), { error: true });

// ------------------------------------------------------------ actions
/** Appel qui modifie l'agenda : rafraîchit et propose l'annulation. */
/** Marque une écriture locale : ses échos dans la boucle de surveillance ne sont pas « externes ». */
const local = { busy: 0, until: 0 };
const mark = (d) => {
  local.busy += d;
  local.until = Date.now() + 2500;
};
async function mutate(method, params) {
  mark(1);
  try {
    return await call(method, params);
  } finally {
    mark(-1);
  }
}
app.mutate = mutate;

app.act = async (method, params, label) => {
  try {
    const r = await mutate(method, params);
    if (r && r.version) S.version = r.version;
    await app.refresh();
    if (r && r.conflicts && r.conflicts.length) {
      app.toast(`Modifié ailleurs entre-temps : « ${r.conflicts.join(', ')} » conservé(s), votre version est dans les conflits`, { action: { label: 'Voir', run: () => openConflicts(app) } });
    } else if (label !== false) {
      app.toast(label || cap(r?.undo || 'Enregistré'), { undo: true });
    }
    scheduleReminders(app);
    return r;
  } catch (e) {
    app.error(e);
    await app.refresh();
    throw e;
  }
};
app.undo = async () => {
  try {
    const r = await mutate('undo');
    S.version = r.version;
    await app.refresh();
    app.toast(`Annulé : ${r.undone}`, { action: { label: 'Rétablir', run: app.redo } });
    scheduleReminders(app);
  } catch (e) {
    app.error(e);
  }
};
app.redo = async () => {
  try {
    const r = await mutate('redo');
    S.version = r.version;
    await app.refresh();
    app.toast(`Rétabli : ${r.redone}`);
  } catch (e) {
    app.error(e);
  }
};

/** Demande à l'utilisateur de choisir parmi des options (petite boîte de dialogue). */
app.choose = (title, text, options) =>
  new Promise((resolve) => {
    const layer = $('#layer');
    const el = h(`<div class="overlay" style="align-items:center"><div class="dialog" style="width:min(420px,100%)" role="dialog" aria-modal="true">
      <div class="dialog-head"><h2 style="font-size:24px">${esc(title)}</h2></div>
      <div class="dialog-body"><p style="margin:0;color:var(--text-2)">${esc(text)}</p></div>
      <div class="dialog-foot"></div></div></div>`);
    const foot = $('.dialog-foot', el);
    const done = (v) => {
      el.remove();
      document.removeEventListener('keydown', key, true);
      resolve(v);
    };
    const key = (e) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        done(null);
      }
    };
    document.addEventListener('keydown', key, true);
    for (const o of [{ label: 'Annuler', value: null }, ...options]) {
      const b = h(`<button class="btn ${o.primary ? 'primary' : ''}">${esc(o.label)}</button>`);
      b.onclick = () => done(o.value);
      foot.append(b);
    }
    el.addEventListener('pointerdown', (e) => {
      if (e.target === el) done(null);
    });
    layer.append(el);
    foot.lastChild.focus();
  });

/** Déplacement d'un événement (glisser-déposer), avec choix pour les séries. */
app.moveEvent = async (ev, start, end) => {
  if (ev.readonly) return app.toast('Abonnement en lecture seule', { error: true });
  let scope = 'one';
  if (ev.recurring) {
    scope = await app.choose('Événement répété', `Déplacer « ${ev.title} » :`, [
      { label: 'Toute la série', value: 'all' },
      { label: 'Cette occurrence', value: 'one', primary: true },
    ]);
    if (!scope) return app.render();
  }
  await app.act('move', { id: ev.id, start, end, occurrence: ev.occurrence, scope }, `« ${ev.title} » déplacé`).catch(() => {});
};

app.calendarColor = (name) => S.calendars.find((c) => c.name === name)?.color;
app.isHidden = (name) => (app.prefs.hiddenCals || []).includes(name || '') || S.calendars.find((c) => c.name === name)?.hidden;

// ------------------------------------------------------------ données
function range() {
  const c = S.cursor;
  switch (S.view) {
    case 'month': {
      const s = weekStart(monthStart(c));
      return [s, s + 42];
    }
    case 'week': {
      if (isNarrow()) return [c - 1, c + 4];
      const s = weekStart(c);
      return [s, s + 7];
    }
    case 'agenda':
      return [todayN(), todayN() + S.agendaDays];
    default:
      return [todayN() - 7, todayN() + 35];
  }
}
app.range = range;

let refreshing = null;
app.refresh = async () => {
  if (!S.info) return;
  const run = async () => {
    const [a, b] = range();
    const [list, tasks, cals] = await Promise.all([
      call('list', { from: ds(a), to: ds(b - 1), include_hidden: true, tasks: false }),
      call('tasks', { include_done: S.view === 'tasks' }),
      call('calendars'),
    ]);
    S.events = list.events.filter((e) => !app.isHidden(e.calendar));
    S.tasks = tasks;
    S.calendars = cals;
    app.render();
  };
  // évite les rafraîchissements concurrents
  if (refreshing) {
    await refreshing;
  }
  refreshing = run().finally(() => (refreshing = null));
  return refreshing;
};

// ------------------------------------------------------------ rendu
function title() {
  const c = S.cursor;
  const { y, m } = ymd(c);
  switch (S.view) {
    case 'month':
      return `${cap(MONTHS[m - 1])} <em>${y}</em>`;
    case 'week': {
      const a = isNarrow() ? c : weekStart(c);
      const b = a + (isNarrow() ? 3 : 7);
      const A = ymd(a);
      const B = ymd(b - 1);
      if (isNarrow()) return A.m === B.m ? `${A.d}–${B.d} ${MONTHS_SHORT[A.m - 1]}` : `${A.d} ${MONTHS_SHORT[A.m - 1]} – ${B.d} ${MONTHS_SHORT[B.m - 1]}`;
      if (A.m === B.m) return `${A.d} – ${B.d} ${MONTHS[A.m - 1]} <em>${A.y}</em>`;
      return `${A.d} ${MONTHS_SHORT[A.m - 1]} – ${B.d} ${MONTHS_SHORT[B.m - 1]} <em>${B.y}</em>`;
    }
    case 'agenda':
      return "Aujourd'hui";
    case 'tasks':
      return 'Tâches';
  }
  return 'Agenda';
}

app.render = () => {
  if (!S.info) return;
  const view = $('#view');
  $('#title').innerHTML = title();
  for (const b of $$('[data-view]')) {
    const on = b.dataset.view === S.view;
    b.setAttribute(b.closest('.segmented') ? 'aria-pressed' : 'aria-current', on);
  }
  $('#arrows').style.visibility = S.view === 'agenda' || S.view === 'tasks' ? 'hidden' : '';
  $('#today-btn').hidden = S.view === 'tasks' || S.view === 'agenda';
  const open = S.tasks.filter((t) => t.status !== 'done' && t.status !== 'cancelled').length;
  $('#task-count').textContent = open || '';
  const keepScroll = view.dataset.view === S.view ? view.firstElementChild?.querySelector?.('.week-body, .agenda, .cards') : null;
  view.dataset.view = S.view;
  ({ month: renderMonth, week: renderWeek, agenda: renderAgenda, tasks: renderTasks })[S.view](app, view, keepScroll);
  renderMini(app, $('#mini'));
  renderCalendars(app, $('#cal-list'));
};

app.setView = (v) => {
  if (!v || v === S.view) return;
  S.view = v;
  app.prefs.view = v;
  app.savePrefs();
  $('#view').dataset.view = '';
  closeSidebar();
  return app.refresh();
};
app.go = (n) => {
  S.cursor = n;
  S.selDay = n;
  return app.refresh();
};
app.step = (k) => {
  if (S.view === 'month') app.go(addMonths(S.cursor, k));
  else if (S.view === 'week') app.go(S.cursor + k * (isNarrow() ? 3 : 7));
};
app.openEditor = (item) => openEditor(app, item);
app.openNew = (defaults) => openNew(app, defaults);
app.openPalette = (mode, text) => openPalette(app, mode, text);

// ------------------------------------------------------------ changements externes
async function watchLoop() {
  for (;;) {
    try {
      const r = await call('changes', { since: S.version, wait: 25000 });
      showConflicts(r.conflicts);
      if (r.version !== S.version) {
        const external = S.version !== 0 && local.busy === 0 && Date.now() > local.until;
        S.version = r.version;
        await app.refresh();
        scheduleReminders(app);
        if (external && document.visibilityState === 'visible') app.toast('Agenda mis à jour (modification externe)');
      }
    } catch (e) {
      await new Promise((res) => setTimeout(res, 4000));
    }
  }
}

function showConflicts(n) {
  const b = $('#banner');
  if (!n) {
    b.hidden = true;
    return;
  }
  b.hidden = false;
  b.innerHTML = `${icon('alert')}<span style="flex:1"><b>${n} conflit${n > 1 ? 's' : ''} de synchronisation</b> — deux versions d'un même élément existent. Rien n'a été perdu.</span><button class="btn">Résoudre</button>`;
  $('button', b).onclick = () => openConflicts(app);
}
app.showConflicts = showConflicts;

// ------------------------------------------------------------ barre latérale mobile
function closeSidebar() {
  $('#sidebar').classList.remove('open');
  $('.side-scrim')?.remove();
}
function openSidebar() {
  const sb = $('#sidebar');
  sb.classList.add('open');
  const scrim = h('<div class="overlay side-scrim" style="z-index:44;padding:0"></div>');
  scrim.onclick = closeSidebar;
  document.body.append(scrim);
}

// ------------------------------------------------------------ événements globaux
document.addEventListener('click', (e) => {
  const v = e.target.closest('[data-view]');
  if (v && !v.closest('#view')) return app.setView(v.dataset.view);
  const a = e.target.closest('[data-act]');
  if (!a) return;
  switch (a.dataset.act) {
    case 'prev':
      return app.step(-1);
    case 'next':
      return app.step(1);
    case 'today':
      return app.go(todayN());
    case 'palette':
      return app.openPalette('search');
    case 'new':
      return app.openPalette('create');
    case 'settings':
      closeSidebar();
      return openSettings(app);
    case 'theme':
      return app.toggleTheme();
    case 'menu':
      return openSidebar();
  }
});

document.addEventListener('keydown', (e) => {
  if (!S.info) return;
  const inField = e.target.closest('input, textarea, select, [contenteditable]');
  const mod = e.ctrlKey || e.metaKey;
  if (mod && e.key.toLowerCase() === 'k') {
    e.preventDefault();
    return app.openPalette('search');
  }
  if ($('#layer').children.length) return;
  if (inField) return;
  if (mod && e.key.toLowerCase() === 'z') {
    e.preventDefault();
    return e.shiftKey ? app.redo() : app.undo();
  }
  if (mod && e.key.toLowerCase() === 'y') {
    e.preventDefault();
    return app.redo();
  }
  if (mod || e.altKey) return;
  const k = e.key;
  if (k === 'n' || k === 'N') {
    e.preventDefault();
    app.openPalette('create');
  } else if (k === '/') {
    e.preventDefault();
    app.openPalette('search');
  } else if (k === 't') app.go(todayN());
  else if (k === 'j' || k === 'ArrowRight') app.step(1);
  else if (k === 'k' || k === 'ArrowLeft') app.step(-1);
  else if (k === 'm') app.setView('month');
  else if (k === 's' || k === 'w') app.setView('week');
  else if (k === 'a' || k === 'd') app.setView('agenda');
  else if (k === 'x') app.setView('tasks');
  else if (k === '?') app.openPalette('search', '>');
});

// balayage horizontal pour changer de période (mobile)
(() => {
  let sx = 0;
  let sy = 0;
  let st = 0;
  const v = $('#view');
  v.addEventListener('touchstart', (e) => {
    const t = e.touches[0];
    sx = t.clientX;
    sy = t.clientY;
    st = Date.now();
  }, { passive: true });
  v.addEventListener('touchend', (e) => {
    const t = e.changedTouches[0];
    const dx = t.clientX - sx;
    const dy = t.clientY - sy;
    if (Date.now() - st < 500 && Math.abs(dx) > 70 && Math.abs(dy) < 45 && (S.view === 'month' || S.view === 'week')) {
      if (e.target.closest('.ev, .chip, .tcard')) return;
      app.step(dx < 0 ? 1 : -1);
    }
  }, { passive: true });
})();

window.addEventListener('resize', () => {
  clearTimeout(window.__rz);
  window.__rz = setTimeout(() => app.refresh(), 150);
});
// minuit passé, ligne « maintenant » : rafraîchissement doux chaque minute
setInterval(() => {
  if (document.visibilityState === 'visible' && S.info && (S.view === 'week' || S.view === 'agenda')) app.render();
}, 60000);
document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'visible' && S.info) {
    app.refresh();
    scheduleReminders(app);
  }
});

// ------------------------------------------------------------ démarrage
app.start = async () => {
  app.applyTheme();
  try {
    await call('set_timezone', { tz: Intl.DateTimeFormat().resolvedOptions().timeZone });
  } catch (e) {}
  try {
    S.hostInfo = await call('host_info');
  } catch (e) {
    S.hostInfo = {};
  }
  try {
    S.info = await call('info');
  } catch (e) {
    S.info = null;
  }
  if (!S.info) {
    renderOnboarding(app, $('#view'));
    return;
  }
  S.version = S.info.version;
  $('#view').dataset.view = '';
  await app.refresh().catch(app.error);
  showConflicts(S.info.conflicts);
  if (S.info.invalid && S.info.invalid.length) {
    app.toast(`${S.info.invalid.length} fichier(s) illisible(s) ignoré(s) : ${S.info.invalid[0].id}`, { error: true });
  }
  S.ready = true;
  document.body.dataset.ready = '1';
  scheduleReminders(app);
  watchLoop();
};

app.opened = async () => {
  S.info = await call('info');
  S.version = S.info.version;
  S.cursor = todayN();
  $('#view').dataset.view = '';
  await app.refresh();
  if (!S.ready) {
    S.ready = true;
    document.body.dataset.ready = '1';
    watchLoop();
  }
  scheduleReminders(app);
};

window.agendaApp = app;
app.start();
