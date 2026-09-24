// Vue Mois : grille de 6 semaines, barres multi-jours, glisser-déposer entre jours.
import { esc, todayN, weekStart, monthStart, ymd, dayOf, ds, mins, timeOf, isNarrow, MONTHS_SHORT, DAYS_SHORT, fmtDay, cap, h } from '../util.js';
import { draggable } from '../dnd.js';
import { rowHtml, bindRows } from './agenda.js';

/** Jours couverts par une occurrence (fin incluse). */
export function span(e) {
  const a = dayOf(e.start);
  let b;
  if (e.all_day) b = dayOf(e.end) - 1;
  else {
    b = dayOf(e.end);
    if (b > a && mins(e.end) === 0) b -= 1;
  }
  return [a, Math.max(a, b)];
}

export function renderMonth(app, el) {
  const S = app.state;
  const compact = isNarrow();
  const ms = monthStart(S.cursor);
  const start = weekStart(ms);
  const month = ymd(ms).m;
  const today = todayN();
  const nowAbs = today * 1440 + new Date().getHours() * 60 + new Date().getMinutes();
  const evs = S.events.map((e, i) => ({ e, i, s: span(e) }));

  if (compact) return renderCompact(app, el, evs, start, month, today);

  const avail = Math.max(300, el.clientHeight - 16 - 24);
  const rowH = avail / 6;
  const lanes = Math.max(1, Math.floor((rowH - 32) / 24));
  let rows = '';
  for (let w = 0; w < 6; w++) {
    const ws = start + w * 7;
    const we = ws + 6;
    const segs = evs
      .filter((x) => x.s[0] <= we && x.s[1] >= ws)
      .map((x) => ({ ...x, a: Math.max(x.s[0], ws), b: Math.min(x.s[1], we), bar: x.e.all_day || x.s[1] > x.s[0] }))
      .sort((p, q) => q.bar - p.bar || p.a - q.a || q.b - q.a - (p.b - p.a) || (p.e.start < q.e.start ? -1 : 1));
    const used = Array.from({ length: lanes }, () => new Array(7).fill(false));
    const hiddenCount = new Array(7).fill(0);
    let items = '';
    for (const sg of segs) {
      let lane = -1;
      for (let l = 0; l < lanes && lane < 0; l++) {
        let free = true;
        for (let d = sg.a; d <= sg.b; d++) if (used[l][d - ws]) free = false;
        if (free) lane = l;
      }
      if (lane < 0) {
        for (let d = sg.a; d <= sg.b; d++) hiddenCount[d - ws]++;
        continue;
      }
      for (let d = sg.a; d <= sg.b; d++) used[lane][d - ws] = true;
      const e = sg.e;
      const col = sg.a - ws + 1;
      const n = sg.b - sg.a + 1;
      const past = (e.all_day ? (sg.s[1] + 1) * 1440 : dayOf(e.end) * 1440 + mins(e.end)) <= nowAbs;
      const cls = ['chip', sg.bar ? 'bar' : '', sg.s[0] < ws ? 'cont-l' : '', sg.s[1] > we ? 'cont-r' : '', past ? 'past' : '', e.status === 'cancelled' ? 'cancelled' : ''].join(' ');
      const inner = sg.bar
        ? `<span class="n">${!e.all_day && sg.a === sg.s[0] ? `<span class="t">${timeOf(e.start)}</span> ` : ''}${esc(e.title)}</span>`
        : `<i class="dot"></i><span class="t">${timeOf(e.start)}</span><span class="n">${esc(e.title)}</span>`;
      items += `<div class="${cls}" data-i="${sg.i}" style="grid-column:${col} / span ${n};grid-row:${lane + 2};--c:${esc(e.color)}" title="${esc(e.title)}">${inner}</div>`;
    }
    let cells = '';
    for (let d = 0; d < 7; d++) {
      const n = ws + d;
      const { d: dd, m } = ymd(n);
      const cls = ['day-cell', m !== month ? 'out' : '', n === today ? 'today' : '', d >= 5 ? 'weekend' : ''].join(' ');
      const first = dd === 1 ? ` first" data-m="${MONTHS_SHORT[m - 1]}` : '';
      cells += `<div class="${cls}" data-day="${n}" style="grid-column:${d + 1}"><span class="num${first}">${dd}</span></div>`;
      if (hiddenCount[d]) items += `<div class="more" data-more="${n}" style="grid-column:${d + 1};grid-row:${lanes + 2}">+${hiddenCount[d]} autre${hiddenCount[d] > 1 ? 's' : ''}</div>`;
    }
    rows += `<div class="week-row" style="grid-template-rows:30px repeat(${lanes}, 22px) 1fr">${cells}${items}</div>`;
  }
  el.innerHTML = `<div class="month swap"><div class="month-dows">${DAYS_SHORT.map((d) => `<div>${d}</div>`).join('')}</div><div class="month-grid">${rows}</div></div>`;

  const grid = el.querySelector('.month-grid');
  // clic sur un numéro : semaine de ce jour ; « +N » : vue jour
  grid.addEventListener('click', (e) => {
    const more = e.target.closest('[data-more]');
    if (more) {
      app.state.view = 'week';
      return app.go(+more.dataset.more);
    }
    const num = e.target.closest('.num');
    if (num) {
      app.state.view = 'week';
      app.go(+num.closest('[data-day]').dataset.day);
    }
  });
  grid.addEventListener('dblclick', (e) => {
    if (e.target.closest('.chip, .more, .num')) return;
    const c = e.target.closest('[data-day]');
    if (c) app.openNew({ start: ds(+c.dataset.day), allDay: true });
  });

  const cellAt = (x, y) => document.elementFromPoint(x, y)?.closest?.('.day-cell');

  // déplacer un élément vers un autre jour
  for (const chip of grid.querySelectorAll('.chip')) {
    const x = evs[+chip.dataset.i];
    const e = x.e;
    let ghost = null;
    let target = null;
    let grabDay = x.s[0];
    draggable(chip, {
      filter: () => !e.readonly,
      click: () => app.openEditor(e),
      start: (ev) => {
        const c = cellAt(ev.clientX, ev.clientY);
        grabDay = c ? +c.dataset.day : x.s[0];
        for (const o of grid.querySelectorAll(`.chip[data-i="${x.i}"]`)) o.classList.add('dragging');
        chip.style.pointerEvents = 'none';
        ghost = h(`<div class="drag-ghost" style="position:fixed;--c:${esc(e.color)};width:${Math.min(220, chip.offsetWidth)}px">${esc(e.title)}</div>`);
        document.body.append(ghost);
      },
      move: (ev) => {
        ghost.style.left = ev.clientX + 8 + 'px';
        ghost.style.top = ev.clientY + 8 + 'px';
        const c = cellAt(ev.clientX, ev.clientY);
        if (c !== target) {
          target?.classList.remove('drop');
          target = c;
          target?.classList.add('drop');
        }
      },
      end: () => {
        ghost?.remove();
        target?.classList.remove('drop');
        chip.style.pointerEvents = '';
        const delta = target ? +target.dataset.day - grabDay : 0;
        if (!delta) return app.render();
        const shift = (s) => ds(dayOf(s) + delta) + s.slice(10);
        if (e.all_day) app.moveEvent(e, ds(x.s[0] + delta), ds(x.s[1] + delta));
        else app.moveEvent(e, shift(e.start), shift(e.end));
      },
      cancel: () => {
        ghost?.remove();
        target?.classList.remove('drop');
        app.render();
      },
    });
  }

  // sélectionner plusieurs jours pour créer un événement sur la journée
  let selA = null;
  let selB = null;
  const paint = () => {
    for (const c of grid.querySelectorAll('.day-cell')) {
      const n = +c.dataset.day;
      c.classList.toggle('selecting', selA !== null && n >= Math.min(selA, selB) && n <= Math.max(selA, selB));
    }
  };
  draggable(grid, {
    filter: (ev) => !ev.target.closest('.chip, .more, .num'),
    start: (ev, p) => {
      const c = cellAt(p.x, p.y);
      if (!c) return;
      selA = selB = +c.dataset.day;
      paint();
    },
    move: (ev) => {
      const c = cellAt(ev.clientX, ev.clientY);
      if (c && selA !== null) {
        selB = +c.dataset.day;
        paint();
      }
    },
    end: () => {
      if (selA === null) return;
      const a = Math.min(selA, selB);
      const b = Math.max(selA, selB);
      selA = selB = null;
      paint();
      app.openNew({ start: ds(a), end: ds(b), allDay: true });
    },
    cancel: () => {
      selA = selB = null;
      paint();
    },
  });
}

function renderCompact(app, el, evs, start, month, today) {
  const S = app.state;
  let rows = '';
  for (let w = 0; w < 6; w++) {
    let cells = '';
    for (let d = 0; d < 7; d++) {
      const n = start + w * 7 + d;
      const { d: dd, m } = ymd(n);
      const colors = [...new Set(evs.filter((x) => x.s[0] <= n && x.s[1] >= n).map((x) => x.e.color))].slice(0, 3);
      const cls = ['day-cell', m !== month ? 'out' : '', n === today ? 'today' : '', n === S.selDay ? 'sel' : '', d >= 5 ? 'weekend' : ''].join(' ');
      cells += `<div class="${cls}" data-day="${n}" style="grid-column:${d + 1}"><span class="num">${dd}</span><span class="dots">${colors.map((c) => `<i style="--c:${esc(c)}"></i>`).join('')}</span></div>`;
    }
    rows += `<div class="week-row">${cells}</div>`;
  }
  const sel = S.selDay;
  const dayEvs = evs.filter((x) => x.s[0] <= sel && x.s[1] >= sel).map((x) => x.e);
  const dayTasks = S.tasks.filter((t) => t.due && dayOf(t.due) === sel && t.status !== 'done' && t.status !== 'cancelled');
  const list = dayEvs.length || dayTasks.length
    ? `<div class="card">${dayEvs.map((e) => rowHtml(e, sel)).join('')}${dayTasks.map((t) => rowHtml(t, sel, true)).join('')}</div>`
    : `<div class="empty">Rien de prévu.<br><button class="btn" data-new-day style="margin-top:10px">Ajouter</button></div>`;
  el.innerHTML = `<div class="month compact swap"><div class="month-dows">${DAYS_SHORT.map((d) => `<div>${d[0].toUpperCase()}</div>`).join('')}</div>
    <div class="month-grid">${rows}</div>
    <div class="month-day-list"><div class="day-group"><h4>${cap(fmtDay(sel))}</h4>${list}</div></div></div>`;
  el.querySelector('.month-grid').onclick = (e) => {
    const c = e.target.closest('[data-day]');
    if (!c) return;
    S.selDay = +c.dataset.day;
    renderCompact(app, el, evs, start, month, today);
  };
  el.querySelector('[data-new-day]')?.addEventListener('click', () => app.openNew({ start: ds(sel), allDay: true }));
  bindRows(app, el, [...dayEvs, ...dayTasks]);
}
