// Vue Tâches : colonnes par statut, ajout rapide, glisser-déposer entre colonnes.
import { esc, todayN, dayOf, h } from '../util.js';
import { draggable } from '../dnd.js';
import { taskPills } from './agenda.js';

const LANES = [
  { k: 'todo', t: 'À faire', c: 'var(--text-3)' },
  { k: 'doing', t: 'En cours', c: 'var(--accent)' },
  { k: 'waiting', t: 'En attente', c: 'var(--warn)' },
  { k: 'done', t: 'Terminé', c: 'var(--ok)' },
];

export function renderTasks(app, el, keep) {
  const S = app.state;
  const today = todayN();
  const scrolls = keep ? [...el.querySelectorAll('.cards')].map((c) => c.scrollTop) : [];
  const draft = el.querySelector('.task-add input')?.value || '';
  const hadFocus = document.activeElement?.matches?.('.task-add input');
  const byLane = Object.fromEntries(LANES.map((l) => [l.k, []]));
  for (const t of S.tasks) {
    if (t.status === 'cancelled') continue;
    (byLane[t.status] || byLane.todo).push(t);
  }
  byLane.done.sort((a, b) => (b.done_at || '').localeCompare(a.done_at || ''));
  byLane.done = byLane.done.slice(0, 40);
  const card = (t) => {
    const done = t.status === 'done';
    const late = t.due && dayOf(t.due) < today && !done;
    return `<div class="tcard" data-id="${esc(t.id)}" ${late ? 'style="border-color:color-mix(in srgb,var(--danger) 35%,var(--line))"' : ''}>
      <button class="check ${done ? 'on' : ''} ${t.priority === 'high' ? 'prio-high' : ''}" data-check aria-label="${done ? 'Rouvrir' : 'Terminer'}"></button>
      <div class="body ${done ? 'done-txt' : ''}"><b>${esc(t.title)}</b><div class="meta">${taskPills(t)}</div></div></div>`;
  };
  el.innerHTML = `<div class="tasks ${keep ? '' : 'swap'}">
    <form class="task-add"><input placeholder="Nouvelle tâche… ex. « Rendre le rapport vendredi !haute #cours »" aria-label="Nouvelle tâche" autocomplete="off"><button class="btn primary">Ajouter</button></form>
    <div class="board">${LANES.map((l) => `<section class="lane" data-status="${l.k}" style="--c:${l.c}"><h4><i></i>${l.t}<span class="count">${byLane[l.k].length}</span></h4><div class="cards">${byLane[l.k].map(card).join('') || '<div class="note" style="padding:8px">—</div>'}</div></section>`).join('')}</div></div>`;
  el.querySelectorAll('.cards').forEach((c, i) => (c.scrollTop = scrolls[i] || 0));
  const input = el.querySelector('.task-add input');
  input.value = draft;
  if (hadFocus) input.focus();
  el.querySelector('.task-add').onsubmit = async (e) => {
    e.preventDefault();
    const text = input.value.trim();
    if (!text) return;
    input.value = '';
    try {
      const r = await app.act('add', { text, kind: 'task' }, false);
      app.toast(`Ajoutée : ${r.summary}`, { undo: true });
    } catch (err) {
      input.value = text;
    }
  };
  const byId = new Map(S.tasks.map((t) => [t.id, t]));
  const laneAt = (x, y) => document.elementFromPoint(x, y)?.closest?.('.lane');
  for (const c of el.querySelectorAll('.tcard')) {
    const t = byId.get(c.dataset.id);
    let ghost = null;
    let lane = null;
    draggable(c, {
      filter: (e) => !e.target.closest('[data-check]'),
      click: () => app.openEditor(t),
      start: () => {
        c.classList.add('dragging');
        c.style.pointerEvents = 'none';
        ghost = h(`<div class="tcard lifted" style="position:fixed;z-index:70;pointer-events:none;width:${c.offsetWidth}px">${c.innerHTML}</div>`);
        document.body.append(ghost);
      },
      move: (e) => {
        ghost.style.left = e.clientX - 20 + 'px';
        ghost.style.top = e.clientY - 20 + 'px';
        const l = laneAt(e.clientX, e.clientY);
        if (l !== lane) {
          lane?.classList.remove('drop');
          lane = l;
          lane?.classList.add('drop');
        }
      },
      end: () => {
        ghost?.remove();
        lane?.classList.remove('drop');
        c.style.pointerEvents = '';
        const st = lane?.dataset.status;
        if (!st || st === t.status) return app.render();
        if (st === 'done') app.act('task_done', { id: t.id, done: true }, `✓ ${t.title}`).catch(() => {});
        else app.act('update', { id: t.id, fields: { status: st, done_at: null } }, `« ${t.title} » → ${LANES.find((l) => l.k === st).t}`).catch(() => {});
      },
      cancel: () => {
        ghost?.remove();
        lane?.classList.remove('drop');
        app.render();
      },
    });
    c.querySelector('[data-check]').addEventListener('click', (e) => {
      e.stopPropagation();
      const done = t.status !== 'done';
      e.currentTarget.classList.toggle('on', done);
      setTimeout(() => app.act('task_done', { id: t.id, done }, done ? `✓ ${t.title}` : `Rouverte : ${t.title}`).catch(() => {}), 180);
    });
  }
}
