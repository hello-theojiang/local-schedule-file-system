// Palette : créer en langage naturel (avec aperçu), rechercher, lancer une commande.
import { $, esc, icon, h, debounce, todayN, dayOf, fmtDay, timeOf, cap } from './util.js';
import { openSettings, openConflicts, importIcs, exportIcs, updateSubscriptions, showShortcuts } from './settings.js';

const fold = (s) => s.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase();

function commands(app) {
  const c = [
    ['today', "Aller à aujourd'hui", 'T', () => app.go(todayN())],
    ['today', 'Vue Jour (aujourd’hui)', 'D', () => app.setView('agenda')],
    ['week', 'Vue Semaine', 'S', () => app.setView('week')],
    ['month', 'Vue Mois', 'M', () => app.setView('month')],
    ['tasks', 'Vue Tâches', 'X', () => app.setView('tasks')],
    ['moon', 'Basculer thème clair / sombre', '', () => app.toggleTheme()],
    ['undo', 'Annuler la dernière action', 'Ctrl Z', () => app.undo()],
    ['undo', 'Rétablir', 'Ctrl ⇧ Z', () => app.redo()],
    ['alert', 'Résoudre les conflits', '', () => openConflicts(app)],
    ['refresh', 'Actualiser les abonnements', '', () => updateSubscriptions(app)],
    ['upload', 'Importer un fichier .ics', '', () => importIcs(app)],
    ['download', 'Exporter en .ics', '', () => exportIcs(app)],
    ['gear', 'Réglages', '', () => openSettings(app)],
    ['sparkle', 'Raccourcis clavier et syntaxe', '?', () => showShortcuts(app)],
  ];
  return c.map(([ic, label, key, run]) => ({ type: 'cmd', ic, label, key, run }));
}

export function openPalette(app, mode = 'search', initial = '') {
  const layer = $('#layer');
  if (layer.querySelector('.palette')) return;
  const placeholder = mode === 'create' ? 'Nouveau… « Dentiste vendredi 14h-15h @Cabinet #santé »' : 'Rechercher, créer (« Rapport demain !haute ») ou > commande';
  const el = h(`<div class="overlay" role="dialog" aria-modal="true" aria-label="Palette">
    <div class="palette"><div class="palette-input">${icon(mode === 'create' ? 'plus' : 'search')}<input placeholder="${esc(placeholder)}" autocomplete="off" spellcheck="false" aria-label="Texte"><span class="kbd">Échap</span></div>
    <div class="palette-list" role="listbox"></div>
    <div class="palette-foot"><span><b>Entrée</b> valider</span><span><b>Tab</b> événement ⇄ tâche</span><span><b>⇧ Entrée</b> créer et détailler</span><span><b>&gt;</b> commandes</span></div></div></div>`);
  const input = $('input', el);
  const list = $('.palette-list', el);
  let items = [];
  let sel = 0;
  let kindOverride = null;
  let seq = 0;
  let preview = null;

  const close = () => {
    el.remove();
    document.removeEventListener('keydown', onKey, true);
  };

  const draw = () => {
    let html = '';
    let lastType = '';
    items.forEach((it, i) => {
      const sect = { create: 'Créer', item: 'Résultats', cmd: 'Commandes' }[it.type];
      if (sect !== lastType) {
        html += `<div class="sect">${sect}</div>`;
        lastType = sect;
      }
      const selAttr = `aria-selected="${i === sel}" data-i="${i}" role="option"`;
      if (it.type === 'create') {
        const f = it.p.fields;
        const chips = [f.location && `@${esc(f.location)}`, ...(f.tags || []).map((t) => `#${esc(t)}`), f.calendar && `+${esc(f.calendar)}`].filter(Boolean).map((c) => `<span class="pill">${c}</span>`).join('');
        html += `<div class="opt create" ${selAttr}><span class="ico">${icon(it.kind === 'task' ? 'tasks' : 'month')}</span>
          <span class="txt"><b>${esc(f.title || '(sans titre)')}</b><small>${esc(it.summary)}</small>${chips ? `<span class="preview-fields">${chips}</span>` : ''}</span>
          <span class="kind">${it.kind === 'task' ? 'Tâche' : 'Événement'} ⇥</span></div>`;
      } else if (it.type === 'item') {
        const r = it.r;
        const when = r.when ? (r.when.length > 10 ? `${cap(fmtDay(dayOf(r.when)))} · ${timeOf(r.when)}` : cap(fmtDay(dayOf(r.when)))) : 'sans date';
        html += `<div class="opt" ${selAttr}><span class="ico">${icon(r.kind === 'task' ? 'tasks' : r.recurring ? 'repeat' : 'month')}</span>
          <span class="txt"><b>${esc(r.title)}</b><small>${esc(when)}${r.status ? ' · ' + esc({ todo: 'à faire', doing: 'en cours', waiting: 'en attente', done: 'terminée', cancelled: 'annulée' }[r.status] || r.status) : ''}</small></span></div>`;
      } else {
        html += `<div class="opt" ${selAttr}><span class="ico">${icon(it.ic)}</span><span class="txt"><b>${esc(it.label)}</b></span>${it.key ? `<span class="kbd">${esc(it.key)}</span>` : ''}</div>`;
      }
    });
    if (!items.length) html = `<div class="empty">Tapez une phrase pour créer, un mot pour chercher.</div>`;
    list.innerHTML = html;
    list.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest' });
  };

  const update = debounce(async () => {
    const q = input.value.trim();
    const my = ++seq;
    const cmds = commands(app);
    if (q.startsWith('>')) {
      const f = fold(q.slice(1).trim());
      items = cmds.filter((c) => fold(c.label).includes(f));
      sel = 0;
      return draw();
    }
    const out = [];
    const tasks = [];
    if (q) {
      try {
        const [p, found] = await Promise.all([app.call('parse', { text: q }), mode === 'create' || q.length < 2 ? Promise.resolve([]) : app.call('search', { q, limit: 8 })]);
        if (my !== seq) return;
        preview = p;
        const kind = kindOverride || p.kind;
        out.push({ type: 'create', p, kind, summary: kindSummary(p, kind) });
        for (const r of found) tasks.push({ type: 'item', r });
      } catch (e) {
        return;
      }
    }
    const f = fold(q);
    const cm = q ? cmds.filter((c) => f.length >= 2 && fold(c.label).includes(f)).slice(0, 4) : mode === 'create' ? [] : cmds.slice(0, 6);
    items = mode === 'search' && tasks.length ? [...tasks, ...out, ...cm] : [...out, ...tasks, ...cm];
    sel = 0;
    draw();
  }, 70);

  const kindSummary = (p, kind) => {
    if (kind === p.kind) return p.summary;
    const f = p.fields;
    return kind === 'task'
      ? `Tâche${f.start || f.due ? ' · échéance ' + fmtDay(dayOf(f.start || f.due)) : ''}`
      : `Événement · ${fmtDay(dayOf(f.due || new Date().toISOString().slice(0, 10)))} · toute la journée`;
  };

  const run = async (i, detail = false) => {
    const it = items[i];
    if (!it) return;
    if (it.type === 'create') {
      close();
      try {
        const r = await app.act('add', { text: input.value.trim(), kind: it.kind }, false);
        app.toast(`Créé : ${r.summary}`, { undo: true });
        const d = it.p.fields.start || it.p.fields.due;
        if (d && app.state.view !== 'tasks' && it.kind === 'event') app.go(dayOf(d));
        if (detail) {
          const full = await app.call('get', { id: r.id });
          app.openEditor(full);
        }
      } catch (e) {}
    } else if (it.type === 'item') {
      close();
      const full = await app.call('get', { id: it.r.id }).catch(app.error);
      if (full) app.openEditor(full);
    } else {
      close();
      it.run();
    }
  };

  const onKey = (e) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      close();
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      sel = Math.min(items.length - 1, sel + 1);
      draw();
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      sel = Math.max(0, sel - 1);
      draw();
    } else if (e.key === 'Enter') {
      e.preventDefault();
      run(sel, e.shiftKey);
    } else if (e.key === 'Tab') {
      const c = items.find((x) => x.type === 'create');
      if (c) {
        e.preventDefault();
        kindOverride = c.kind === 'task' ? 'event' : 'task';
        c.kind = kindOverride;
        c.summary = kindSummary(c.p, c.kind);
        draw();
      }
    }
  };
  document.addEventListener('keydown', onKey, true);
  list.addEventListener('click', (e) => {
    const o = e.target.closest('[data-i]');
    if (!o) return;
    const i = +o.dataset.i;
    if (e.target.closest('.kind') && items[i].type === 'create') {
      kindOverride = items[i].kind === 'task' ? 'event' : 'task';
      items[i].kind = kindOverride;
      items[i].summary = kindSummary(items[i].p, kindOverride);
      return draw();
    }
    run(i);
  });
  el.addEventListener('pointerdown', (e) => {
    if (e.target === el) close();
  });
  input.addEventListener('input', () => {
    kindOverride = null;
    update();
  });
  layer.append(el);
  input.value = initial;
  input.focus();
  update();
  void preview;
}
