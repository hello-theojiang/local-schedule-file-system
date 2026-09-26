// Vue Aujourd'hui : liste des prochains jours, tâches en retard et à échéance.
import { esc, icon, todayN, dayOf, ds, mins, timeOf, fmtDay, relDay, ymd, MONTHS, DAYS, fmtDur, nowMin, cap } from '../util.js';
import { span } from './month.js';

function taskPills(t) {
  const today = todayN();
  let out = '';
  if (t.due) {
    const d = dayOf(t.due);
    const late = d < today && t.status !== 'done';
    const label = relDay(d) || fmtDay(d).split(' ').slice(0, 3).join(' ');
    out += `<span class="pill ${late ? 'late' : d <= today + 1 ? 'soon' : ''}">${esc(label)}${t.due.length > 10 ? ' ' + timeOf(t.due) : ''}</span>`;
  }
  if (t.priority === 'high') out += `<span class="pill high">${icon('flag')}haute</span>`;
  if (t.priority === 'low') out += `<span class="pill low">basse</span>`;
  if (t.repeat) out += `<span class="pill">${icon('repeat')}</span>`;
  for (const tg of t.tags || []) out += `<span class="pill">#${esc(tg)}</span>`;
  return out;
}
export { taskPills };

/** Ligne d'un événement ou d'une tâche (vue liste). */
export function rowHtml(x, day, isTask = false, idx = null) {
  const key = idx === null ? '' : ` data-idx="${idx}"`;
  if (isTask || x.kind === 'task') {
    const done = x.status === 'done';
    return `<div class="row ${done ? 'done-txt' : ''}" data-task="${esc(x.id)}"${key}>
      <button class="check ${done ? 'on' : ''} ${x.priority === 'high' ? 'prio-high' : ''}" data-check aria-label="${done ? 'Rouvrir' : 'Terminer'}"></button>
      <div class="main-txt"><b>${esc(x.title)}</b><div class="preview-fields">${taskPills(x)}</div></div></div>`;
  }
  const [a, b] = span(x);
  let time;
  if (x.all_day) time = b > a ? `Journée<small>jusqu'au ${ymd(b).d} ${MONTHS[ymd(b).m - 1]}</small>` : 'Journée';
  else if (a < day && b > day) time = 'Toute la journée<small>(suite)</small>';
  else if (a < day) time = `→ ${timeOf(x.end)}<small>depuis la veille</small>`;
  else {
    const d = dayOf(x.end) * 1440 + mins(x.end) - (dayOf(x.start) * 1440 + mins(x.start));
    time = `${timeOf(x.start)}<small>${b > a ? '→ ' + fmtDay(b).split(' ').slice(0, 2).join(' ') : fmtDur(d)}</small>`;
  }
  const endAbs = x.all_day ? (b + 1) * 1440 : dayOf(x.end) * 1440 + mins(x.end);
  const nowAbs = todayN() * 1440 + nowMin();
  const startAbs = dayOf(x.start) * 1440 + mins(x.start);
  const cls = endAbs <= nowAbs ? 'past' : startAbs <= nowAbs && !x.all_day ? 'now' : '';
  const sub = [x.location ? `${esc(x.location)}` : '', x.recurring ? '↻' : '', (x.tags || []).map((t) => '#' + esc(t)).join(' ')].filter(Boolean).join(' · ');
  return `<div class="row ${cls}" data-ev${key} style="--c:${esc(x.color)}">
    <div class="time">${time}</div><i class="bar"></i>
    <div class="main-txt"><b>${esc(x.title)}</b>${sub ? `<small>${sub}</small>` : ''}</div></div>`;
}

/** Clics sur les lignes : ouvrir l'éditeur, cocher une tâche. */
export function bindRows(app, root, items) {
  root.querySelectorAll('.row').forEach((r, i) => {
    if (r.dataset.idx === undefined) r.dataset.idx = i;
  });
  root.onclick = (e) => {
    const row = e.target.closest('.row');
    if (!row) return;
    const it = items[+row.dataset.idx];
    if (!it) return;
    if (e.target.closest('[data-check]')) {
      const btn = e.target.closest('[data-check]');
      const done = it.status !== 'done';
      btn.classList.toggle('on', done);
      setTimeout(() => app.act('task_done', { id: it.id, done }, done ? `✓ ${it.title}` : `Rouverte : ${it.title}`).catch(() => {}), 180);
      return;
    }
    app.openEditor(it);
  };
}

export function renderAgenda(app, el, keep) {
  const S = app.state;
  const today = todayN();
  const scroll = keep ? keep.scrollTop : 0;
  const evs = S.events.map((e) => ({ e, s: span(e) }));
  const open = S.tasks.filter((t) => t.status !== 'done' && t.status !== 'cancelled');
  const overdue = open.filter((t) => t.due && dayOf(t.due) < today);
  const undated = open.filter((t) => !t.due);
  const items = [];
  const add = (x, day, isTask) => {
    items.push(x);
    return rowHtml(x, day, isTask, items.length - 1);
  };
  const todayEvents = evs.filter((x) => x.s[0] <= today && x.s[1] >= today).map((x) => x.e);
  const todayTasks = open.filter((t) => t.due && dayOf(t.due) === today);
  const { y, m, d } = ymd(today);
  let html = `<div class="hero"><div class="big">${d}</div><div class="sub"><b>${DAYS[(((today + 3) % 7) + 7) % 7]}</b>${MONTHS[m - 1]} ${y} · ${todayEvents.length} événement${todayEvents.length > 1 ? 's' : ''}${todayTasks.length ? ` · ${todayTasks.length} tâche${todayTasks.length > 1 ? 's' : ''}` : ''}</div></div>`;
  if (overdue.length) {
    html += `<div class="day-group"><h4><span style="color:var(--danger)">En retard</span><small>${overdue.length}</small></h4><div class="card">${overdue.map((t) => add(t, today, true)).join('')}</div></div>`;
  }
  let shown = 0;
  for (let n = today; n < today + S.agendaDays; n++) {
    const dayEv = evs.filter((x) => x.s[0] <= n && x.s[1] >= n).map((x) => x.e);
    const dayTasks = open.filter((t) => t.due && dayOf(t.due) === n);
    if (!dayEv.length && !dayTasks.length && n !== today) continue;
    shown++;
    const rel = relDay(n);
    let body = '';
    if (n === today) {
      const now = nowMin();
      let marked = false;
      for (const e of dayEv) {
        const st = e.all_day || dayOf(e.start) < n ? -1 : mins(e.start);
        if (!marked && st > now) {
          body += '<div class="now-marker"><span>' + String(Math.floor(now / 60)).padStart(2, '0') + ':' + String(now % 60).padStart(2, '0') + '</span></div>';
          marked = true;
        }
        body += add(e, n);
      }
      body += dayTasks.map((t) => add(t, n, true)).join('');
      if (!dayEv.length && !dayTasks.length) body = `<div class="empty"><span class="big-ico">☀️</span>Rien de prévu aujourd'hui.</div>`;
    } else {
      body = dayEv.map((e) => add(e, n)).join('') + dayTasks.map((t) => add(t, n, true)).join('');
    }
    html += `<div class="day-group" style="animation-delay:${Math.min(shown, 8) * 30}ms"><h4>${rel ? `<span class="rel">${rel}</span>` : ''}${cap(fmtDay(n))}</h4><div class="card">${body}</div></div>`;
  }
  if (undated.length) {
    html += `<div class="day-group"><h4>Sans échéance<small>${undated.length}</small></h4><div class="card">${undated.slice(0, 8).map((t) => add(t, today, true)).join('')}</div></div>`;
  }
  html += `<button class="btn load-more" data-more>Afficher les ${S.agendaDays < 60 ? 30 : 60} jours suivants</button>`;
  el.innerHTML = `<div class="agenda swap"><div class="agenda-inner">${html}</div></div>`;
  const box = el.firstElementChild;
  box.scrollTop = scroll;
  if (keep) box.classList.remove('swap');
  bindRows(app, box, items);
  box.querySelector('[data-more]').addEventListener('click', (e) => {
    e.stopPropagation();
    S.agendaDays += S.agendaDays < 60 ? 30 : 60;
    app.refresh();
  });
}
export { ds };
