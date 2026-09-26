// Barre latérale : mini-calendrier et liste des calendriers.
import { esc, todayN, monthStart, weekStart, addMonths, ymd, dayOf, MONTHS, DAYS_1 } from '../util.js';

let miniMonth = null;

export function renderMini(app, el) {
  const S = app.state;
  if (miniMonth === null || S.view === 'month') miniMonth = monthStart(S.cursor);
  const ms = miniMonth;
  const start = weekStart(ms);
  const { y, m } = ymd(ms);
  const today = todayN();
  const busy = new Set(S.events.map((e) => dayOf(e.start)));
  const selA = S.view === 'week' ? weekStart(S.cursor) : S.cursor;
  const selB = S.view === 'week' ? selA + 6 : S.cursor;
  let cells = DAYS_1.map((d) => `<div class="dow">${d}</div>`).join('');
  for (let i = 0; i < 42; i++) {
    const n = start + i;
    const cls = [ymd(n).m !== m ? 'out' : '', n === today ? 'today' : '', n >= selA && n <= selB && S.view !== 'tasks' ? 'sel' : '', busy.has(n) ? 'has' : ''].join(' ');
    cells += `<button class="${cls}" data-day="${n}" aria-label="${n}">${ymd(n).d}</button>`;
  }
  el.innerHTML = `<div class="mini-head"><b>${MONTHS[m - 1]} ${y}</b><span>
    <button class="btn ghost icon" data-mini="-1" aria-label="Mois précédent"><svg><use href="#i-left"/></svg></button>
    <button class="btn ghost icon" data-mini="1" aria-label="Mois suivant"><svg><use href="#i-right"/></svg></button></span></div>
    <div class="mini-grid">${cells}</div>`;
  el.onclick = (e) => {
    const nav = e.target.closest('[data-mini]');
    if (nav) {
      miniMonth = addMonths(miniMonth, +nav.dataset.mini);
      renderMini(app, el);
      return;
    }
    const d = e.target.closest('[data-day]');
    if (d) {
      if (S.view === 'tasks' || S.view === 'agenda') S.view = 'week';
      app.go(+d.dataset.day);
    }
  };
}

export function renderCalendars(app, el) {
  const S = app.state;
  const hidden = new Set(app.prefs.hiddenCals || []);
  el.innerHTML = S.calendars
    .map((c) => `<div class="cal-item ${hidden.has(c.name) || c.hidden ? 'off' : ''}" data-cal="${esc(c.name)}" style="--c:${esc(c.color)}" role="checkbox" aria-checked="${!(hidden.has(c.name) || c.hidden)}" tabindex="0">
      <i class="swatch"></i><span>${esc(c.title)}</span>${c.readonly ? '<small title="Abonnement">↻</small>' : ''}</div>`)
    .join('') || '<div class="note" style="padding:0 10px">Aucun calendrier</div>';
  el.onclick = (e) => {
    const it = e.target.closest('[data-cal]');
    if (!it) return;
    const n = it.dataset.cal;
    const set = new Set(app.prefs.hiddenCals || []);
    set.has(n) ? set.delete(n) : set.add(n);
    app.prefs.hiddenCals = [...set];
    app.savePrefs();
    app.refresh();
  };
}
