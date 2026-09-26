// Vue Semaine (7 jours, 3 sur téléphone) : grille horaire et glisser-déposer.
import { esc, todayN, weekStart, dayOf, mins, hm, fromAbs, ds, ymd, isNarrow, DAYS_SHORT, nowMin, fmtDur, h } from '../util.js';
import { draggable, autoScroll } from '../dnd.js';
import { span } from './month.js';

const SNAP = 15;

/** Répartit les segments d'un jour en colonnes quand ils se chevauchent. */
function layout(segs) {
  segs.sort((a, b) => a.s - b.s || b.e - a.e);
  let cluster = [];
  let clusterEnd = -1;
  const flush = () => {
    const cols = [];
    for (const g of cluster) {
      let c = cols.findIndex((end) => end <= g.s);
      if (c < 0) {
        c = cols.length;
        cols.push(0);
      }
      cols[c] = Math.max(g.e, g.s + 20);
      g.col = c;
    }
    for (const g of cluster) g.cols = cols.length;
    cluster = [];
  };
  for (const g of segs) {
    if (g.s >= clusterEnd && cluster.length) flush();
    cluster.push(g);
    clusterEnd = Math.max(clusterEnd, Math.max(g.e, g.s + 20));
  }
  if (cluster.length) flush();
  return segs;
}

export function renderWeek(app, el, keep) {
  const S = app.state;
  const narrow = isNarrow();
  const first = narrow ? S.cursor : weekStart(S.cursor);
  const ncol = narrow ? 3 : 7;
  const days = Array.from({ length: ncol }, (_, i) => first + i);
  const today = todayN();
  const nowAbs = today * 1440 + nowMin();
  const scrollTop = keep ? keep.scrollTop : null;

  // bandeau « journée entière » : événements sur la journée ou de 24 h et plus
  const evs = S.events.map((e, i) => {
    const a = dayOf(e.start) * 1440 + mins(e.start);
    const b = dayOf(e.end) * 1440 + mins(e.end);
    return { e, i, a, b, top: e.all_day || b - a >= 1440, sp: span(e) };
  });
  const tops = evs.filter((x) => x.top && x.sp[0] <= days[ncol - 1] && x.sp[1] >= days[0]).sort((p, q) => p.sp[0] - q.sp[0] || q.sp[1] - p.sp[1]);
  const lanes = [];
  let allday = '';
  for (const x of tops) {
    const a = Math.max(x.sp[0], days[0]) - days[0];
    const b = Math.min(x.sp[1], days[ncol - 1]) - days[0];
    let l = lanes.findIndex((row) => row.every((v, k) => !v || k < a || k > b));
    if (l < 0) {
      l = lanes.length;
      lanes.push(new Array(ncol).fill(false));
    }
    for (let k = a; k <= b; k++) lanes[l][k] = true;
    const cls = ['chip', 'bar', x.sp[0] < days[0] ? 'cont-l' : '', x.sp[1] > days[ncol - 1] ? 'cont-r' : '', x.e.status === 'cancelled' ? 'cancelled' : ''].join(' ');
    allday += `<div class="${cls}" data-i="${x.i}" style="grid-column:${a + 2} / span ${b - a + 1};grid-row:${l + 1};--c:${esc(x.e.color)}" title="${esc(x.e.title)}"><span class="n">${esc(x.e.title)}</span></div>`;
  }
  const nl = Math.max(1, lanes.length);

  let head = '<div class="h"></div>';
  let cols = '';
  for (const [k, n] of days.entries()) {
    const { d } = ymd(n);
    head += `<div class="h ${n === today ? 'today' : ''}" data-day="${n}"><small>${DAYS_SHORT[(((n + 3) % 7) + 7) % 7]}</small><b>${d}</b></div>`;
    const segs = [];
    for (const x of evs) {
      if (x.top) continue;
      const lo = n * 1440;
      const hi = lo + 1440;
      if (x.b <= lo || x.a >= hi) {
        if (!(x.a === x.b && x.a >= lo && x.a < hi)) continue;
      }
      segs.push({ x, s: Math.max(x.a, lo) - lo, e: Math.min(Math.max(x.b, x.a + SNAP), hi) - lo, cut: x.a < lo || x.b > hi });
    }
    let inner = '';
    for (const g of layout(segs)) {
      const e = g.x.e;
      const dur = g.e - g.s;
      const short = dur < 50;
      const tiny = dur < 25;
      const w = 100 / g.cols;
      const cls = ['ev', short ? 'short' : '', tiny ? 'tiny' : '', g.x.b <= nowAbs ? 'past' : '', e.readonly ? 'readonly' : ''].join(' ');
      const meta = `${hm(mins(e.start))} – ${hm(mins(e.end))}${e.location && !short ? ' · ' + esc(e.location) : ''}${e.recurring ? ' ↻' : ''}`;
      inner += `<div class="${cls}" data-i="${g.x.i}" data-day="${n}" style="top:calc(var(--hour) * ${g.s / 60});height:max(20px, calc(var(--hour) * ${dur / 60} - 2px));left:calc(${g.col * w}% + 2px);width:calc(${w}% - 4px);--c:${esc(e.color)}">
        <b>${esc(e.title)}</b><div class="meta">${meta}</div>${!e.readonly && !g.cut ? '<div class="resize"></div>' : ''}</div>`;
    }
    if (n === today) inner += `<div class="now-line" style="top:calc(var(--hour) * ${nowMin() / 60})"></div>`;
    cols += `<div class="day-col ${k >= 5 && !narrow ? 'weekend' : ''}" data-day="${n}">${inner}</div>`;
  }
  let hours = '<div class="hours">';
  for (let i = 1; i < 24; i++) hours += `<span style="top:calc(var(--hour) * ${i})">${String(i).padStart(2, '0')}:00</span>`;
  hours += '</div>';

  el.innerHTML = `<div class="week ${keep ? '' : 'swap'}"><div class="week-box" style="--cols:${ncol}">
    <div class="week-head">${head}</div>
    <div class="week-allday" style="grid-template-rows:repeat(${nl}, 24px)"><div class="lbl">jour</div>${days.map((_, k) => `<div class="col" style="grid-column:${k + 2}"></div>`).join('')}${allday}</div>
    <div class="week-body"><div class="week-body-inner">${hours}${cols}</div></div></div></div>`;

  const body = el.querySelector('.week-body');
  const hourPx = () => parseFloat(getComputedStyle(el.querySelector('.week-box')).getPropertyValue('--hour')) || 52;
  if (scrollTop !== null) body.scrollTop = scrollTop;
  else {
    const firstEv = Math.min(...evs.filter((x) => !x.top && days.includes(Math.floor(x.a / 1440))).map((x) => x.a % 1440), 8 * 60);
    const target = days.includes(today) ? Math.min(firstEv, Math.max(0, nowMin() - 120)) : firstEv;
    body.style.scrollBehavior = 'auto';
    body.scrollTop = Math.max(0, (target / 60 - 0.5) * hourPx());
    body.style.scrollBehavior = '';
  }

  el.querySelector('.week-head').onclick = (e) => {
    const hd = e.target.closest('[data-day]');
    if (hd) {
      S.view = 'agenda';
      app.state.cursor = +hd.dataset.day;
      app.setView('agenda');
    }
  };

  const colEls = [...el.querySelectorAll('.day-col')];
  const colAt = (x) => colEls.find((c) => {
    const r = c.getBoundingClientRect();
    return x >= r.left && x < r.right;
  }) || (x < colEls[0].getBoundingClientRect().left ? colEls[0] : colEls[colEls.length - 1]);
  const minAt = (col, y) => ((y - col.getBoundingClientRect().top) / hourPx()) * 60;
  const snap = (m) => Math.round(m / SNAP) * SNAP;
  const ghostEl = (color, top, dur, text) => {
    const g = h(`<div class="drag-ghost" style="--c:${esc(color)};left:2px;right:2px"></div>`);
    place(g, top, dur, text);
    return g;
  };
  const place = (g, top, dur, text) => {
    g.style.top = `calc(var(--hour) * ${top / 60})`;
    g.style.height = `calc(var(--hour) * ${Math.max(dur, SNAP) / 60} - 2px)`;
    g.innerHTML = text;
  };

  // déplacer / redimensionner
  for (const node of el.querySelectorAll('.ev')) {
    const x = evs[+node.dataset.i];
    const e = x.e;
    const dur = x.b - x.a;
    let ghost = null;
    let grab = 0;
    let target = null;
    let newStart = null;
    draggable(node, {
      filter: (ev) => !ev.target.closest('.resize') && !e.readonly,
      click: () => app.openEditor(e),
      start: (ev, p) => {
        const col = colAt(p.x);
        grab = minAt(col, p.y) - (x.a - +col.dataset.day * 1440);
        node.classList.add('dragging');
        ghost = ghostEl(e.color, x.a % 1440, dur, esc(e.title));
        col.append(ghost);
      },
      move: (ev) => {
        autoScroll(body, ev);
        const col = colAt(ev.clientX);
        if (col !== target) {
          target = col;
          col.append(ghost);
        }
        const m = Math.max(0, Math.min(1440 - Math.min(dur, 1440), snap(minAt(col, ev.clientY) - grab)));
        newStart = +col.dataset.day * 1440 + m;
        place(ghost, m, dur, `${esc(e.title)}<small>${hm(m)} – ${hm((m + dur) % 1440)}</small>`);
      },
      end: () => {
        ghost?.remove();
        if (newStart === null || newStart === x.a) return app.render();
        app.moveEvent(e, fromAbs(newStart), fromAbs(newStart + dur));
      },
      cancel: () => {
        ghost?.remove();
        app.render();
      },
    });
    const handle = node.querySelector('.resize');
    if (handle) {
      let newEnd = null;
      draggable(handle, {
        start: () => {
          node.classList.add('dragging');
          ghost = ghostEl(e.color, x.a % 1440, dur, esc(e.title));
          node.parentElement.append(ghost);
        },
        move: (ev) => {
          autoScroll(body, ev);
          const col = node.parentElement;
          const top = x.a % 1440;
          const end = Math.max(top + SNAP, Math.min(1440, snap(minAt(col, ev.clientY))));
          newEnd = Math.floor(x.a / 1440) * 1440 + end;
          place(ghost, top, end - top, `${esc(e.title)}<small>${hm(top)} – ${hm(end % 1440)} · ${fmtDur(end - top)}</small>`);
        },
        end: () => {
          ghost?.remove();
          if (newEnd === null || newEnd === x.b) return app.render();
          app.moveEvent(e, e.start, fromAbs(newEnd));
        },
        cancel: () => {
          ghost?.remove();
          app.render();
        },
      });
    }
  }

  // créer en sélectionnant un créneau (ou en cliquant)
  for (const col of colEls) {
    let ghost = null;
    let a = 0;
    let b = 0;
    const day = +col.dataset.day;
    draggable(col, {
      filter: (ev) => ev.target === col || ev.target.classList.contains('now-line'),
      click: (ev) => {
        const m = Math.min(1440 - 60, Math.floor(minAt(col, ev.clientY) / 30) * 30);
        app.openNew({ start: fromAbs(day * 1440 + m), end: fromAbs(day * 1440 + m + 60) });
      },
      start: (ev, p) => {
        a = b = Math.floor(minAt(col, p.y) / SNAP) * SNAP;
        ghost = ghostEl('var(--accent)', a, SNAP, '');
        col.append(ghost);
      },
      move: (ev) => {
        autoScroll(body, ev);
        b = Math.max(0, Math.min(1440, snap(minAt(col, ev.clientY))));
        const lo = Math.min(a, b);
        const hi = Math.max(a + SNAP, b);
        place(ghost, lo, hi - lo, `Nouvel événement<small>${hm(lo)} – ${hm(hi % 1440)}</small>`);
      },
      end: () => {
        ghost?.remove();
        const lo = Math.min(a, b);
        const hi = Math.max(a + SNAP, b);
        app.openNew({ start: fromAbs(day * 1440 + lo), end: fromAbs(day * 1440 + hi) });
      },
      cancel: () => ghost?.remove(),
    });
  }

  // bandeau journée entière : clic et déplacement par jours
  const heads = [...el.querySelectorAll('.week-head .h[data-day]')];
  const headAt = (px) => heads.find((hd) => {
    const r = hd.getBoundingClientRect();
    return px >= r.left && px < r.right;
  });
  for (const chip of el.querySelectorAll('.week-allday .chip')) {
    const x = evs[+chip.dataset.i];
    const e = x.e;
    let from = null;
    let to = null;
    draggable(chip, {
      filter: () => !e.readonly,
      click: () => app.openEditor(e),
      start: (ev, p) => {
        from = +(headAt(p.x)?.dataset.day ?? x.sp[0]);
        chip.classList.add('lifted');
      },
      move: (ev) => {
        const hd = headAt(ev.clientX);
        if (hd) to = +hd.dataset.day;
        chip.style.transform = `translateX(${(to ?? from) === from ? 0 : ((to - from) * chip.parentElement.getBoundingClientRect().width) / (ncol + 0.6)}px)`;
      },
      end: () => {
        const delta = to === null ? 0 : to - from;
        if (!delta) return app.render();
        if (e.all_day) app.moveEvent(e, ds(x.sp[0] + delta), ds(x.sp[1] + delta));
        else app.moveEvent(e, fromAbs(x.a + delta * 1440), fromAbs(x.b + delta * 1440));
      },
      cancel: () => app.render(),
    });
  }
}
