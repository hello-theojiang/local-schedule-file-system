// Fiche d'édition d'un événement ou d'une tâche.
import { $, esc, icon, h, ds, dn, dayOf, mins, fromAbs, cap } from './util.js';

const REPEATS = [
  ['', 'Ne se répète pas'],
  ['FREQ=DAILY', 'Chaque jour'],
  ['FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR', 'Chaque jour de semaine'],
  ['WEEKLY', 'Chaque semaine (même jour)'],
  ['FREQ=WEEKLY;INTERVAL=2', 'Toutes les deux semaines'],
  ['FREQ=MONTHLY', 'Chaque mois (même date)'],
  ['FREQ=YEARLY', 'Chaque année'],
  ['custom', 'Personnalisée (RRULE)…'],
];
const ALARMS = ['5m', '10m', '15m', '30m', '1h', '2h', '1d', '2d', '1w'];
const ALARM_LABEL = { '5m': '5 min', '10m': '10 min', '15m': '15 min', '30m': '30 min', '1h': '1 h', '2h': '2 h', '1d': '1 jour', '2d': '2 jours', '1w': '1 semaine' };
const CODES = ['MO', 'TU', 'WE', 'TH', 'FR', 'SA', 'SU'];
const STATUS = [['todo', 'À faire'], ['doing', 'En cours'], ['waiting', 'En attente'], ['done', 'Terminé']];
const PRIO = [['', 'Aucune'], ['low', 'Basse'], ['medium', 'Moyenne'], ['high', 'Haute']];

/** « 1h30 », « 45m », « 2h » → minutes (défaut 60). */
function durMin(d) {
  if (!d) return 60;
  const m = /^(?:(\d+)w)?(?:(\d+)d)?(?:(\d+)h)?(?:(\d+)m?)?$/.exec(d);
  if (!m) return 60;
  return (+(m[1] || 0)) * 10080 + (+(m[2] || 0)) * 1440 + (+(m[3] || 0)) * 60 + (+(m[4] || 0)) || 60;
}

function close() {
  $('#layer .sheet-wrap')?.remove();
}

export async function openEditor(app, item) {
  let full;
  try {
    full = await app.call('get', { id: item.id });
  } catch (e) {
    return app.error(e);
  }
  render(app, full, item);
}

export function openNew(app, d = {}) {
  const start = d.start || ds(app.state.cursor) + ' 09:00';
  const allDay = d.allDay ?? start.length === 10;
  const kind = d.kind || 'event';
  const base = { id: null, kind, title: '', all_day: allDay, start, end: d.end || (allDay ? null : fromAbs(dayOf(start) * 1440 + mins(start) + 60)), status: 'todo', due: d.due || null, tags: [], body: '', alarm: null };
  render(app, base, null, true);
}

function render(app, it, occ, isNew = false) {
  close();
  const S = app.state;
  let kind = it.kind === 'task' ? 'task' : 'event';
  const readonly = !!it.readonly;
  const cals = S.calendars.filter((c) => !c.readonly);
  // pour une occurrence, on affiche ses propres dates
  const src = occ && occ.occurrence ? occ : it;
  const allDay0 = kind === 'event' ? !!it.all_day : false;
  const startS = kind === 'event' ? src.start : it.due || '';
  let endS = kind === 'event' ? src.end || '' : '';
  if (kind === 'event' && allDay0 && endS) endS = src === occ ? ds(dayOf(endS) - 1) : endS;
  if (kind === 'event' && allDay0 && !endS) endS = startS;
  if (kind === 'event' && !allDay0 && !endS) endS = fromAbs(dayOf(startS) * 1440 + mins(startS) + durMin(it.duration));
  const repeat0 = it.repeat || '';
  const repeatSel = REPEATS.some(([v]) => v && v === repeat0) ? repeat0 : repeat0 ? 'custom' : '';
  let alarms = it.alarm === 'none' ? [] : Array.isArray(it.alarm) ? [...it.alarm] : null;

  const wrap = h(`<div class="sheet-wrap"><div class="sheet" role="dialog" aria-modal="true" aria-label="Détails">
    <div class="sheet-head">${isNew ? `<div class="segmented" id="ed-kind"><button type="button" data-k="event" aria-pressed="${kind === 'event'}">Événement</button><button type="button" data-k="task" aria-pressed="${kind === 'task'}">Tâche</button></div>` : `<span class="kind">${kind === 'task' ? 'Tâche' : readonly ? 'Abonnement · lecture seule' : 'Événement'}</span>`}<span class="spacer" style="flex:1"></span>
      <button class="btn ghost icon" data-close aria-label="Fermer">${icon('x')}</button></div>
    <form class="sheet-body" id="ed-form"></form>
    <div class="sheet-foot">${!isNew && !readonly ? `<button type="button" class="btn danger" data-del>${icon('trash')}Supprimer</button>` : ''}<span style="flex:1"></span>
      <button type="button" class="btn" data-close>${readonly ? 'Fermer' : 'Annuler'}</button>${readonly ? '' : '<button type="submit" form="ed-form" class="btn primary">Enregistrer</button>'}</div></div></div>`);
  const form = $('#ed-form', wrap);
  const dis = readonly ? 'disabled' : '';

  const fieldsHtml = () => {
    const calOpts = `<option value="">(aucun)</option>` + cals.map((c) => `<option value="${esc(c.name)}" ${c.name === it.calendar ? 'selected' : ''}>${esc(c.title)}</option>`).join('');
    const common = `
      <div class="field"><label for="ed-cal">Calendrier</label><select id="ed-cal" class="input" ${dis}>${calOpts}</select></div>
      <div class="field"><label for="ed-tags">Mots-clés</label><input id="ed-tags" class="input" value="${esc((it.tags || []).join(', '))}" placeholder="travail, santé…" ${dis}></div>`;
    const repeatHtml = `<div class="field"><label for="ed-rep">${icon('repeat', 'hide-m')}Répétition</label>
        <select id="ed-rep" class="input" ${dis}>${REPEATS.map(([v, l]) => `<option value="${v}" ${v === repeatSel ? 'selected' : ''}>${l}</option>`).join('')}</select>
        <input id="ed-rrule" class="input" value="${esc(repeat0)}" placeholder="FREQ=WEEKLY;BYDAY=TU" ${repeatSel === 'custom' ? '' : 'hidden'} ${dis}>
        ${it.repeat_text ? `<span class="note">${esc(cap(it.repeat_text))}</span>` : ''}</div>`;
    const notes = `<div class="field"><label for="ed-body">Notes</label><textarea id="ed-body" class="input" placeholder="Notes (Markdown)" ${dis}>${esc(it.body || '')}</textarea></div>`;
    if (kind === 'task') {
      const due = it.due || '';
      return `<input class="title-input" id="ed-title" value="${esc(it.title)}" placeholder="Titre de la tâche" required ${dis}>
        <div class="field"><span class="lbl">Statut</span><div class="segmented" id="ed-status">${STATUS.map(([k, l]) => `<button type="button" data-v="${k}" aria-pressed="${(it.status || 'todo') === k}">${l}</button>`).join('')}</div></div>
        <div class="field"><label for="ed-due">Échéance</label><div class="field-row"><input type="date" id="ed-due" class="input grow" value="${due.slice(0, 10)}" ${dis}><input type="time" id="ed-due-t" class="input" value="${due.slice(11, 16)}" ${dis}></div></div>
        <div class="field"><span class="lbl">Priorité</span><div class="segmented" id="ed-prio">${PRIO.map(([k, l]) => `<button type="button" data-v="${k}" aria-pressed="${(it.priority || '') === k}">${l}</button>`).join('')}</div></div>
        ${repeatHtml}${common}${notes}${!isNew ? `<span class="note">${esc(it.id)}</span>` : ''}`;
    }
    const s = startS;
    const e = endS;
    return `<input class="title-input" id="ed-title" value="${esc(it.title)}" placeholder="Titre de l'événement" required ${dis}>
      ${occ && occ.recurring && !readonly ? `<div class="warn-box">${icon('repeat')} Occurrence d'un événement répété (${esc(it.repeat_text || '')}). À l'enregistrement, vous choisirez : cette occurrence ou toute la série.</div>` : ''}
      <label class="toggle"><input type="checkbox" id="ed-allday" ${allDay0 ? 'checked' : ''} ${dis}>Toute la journée</label>
      <div class="field"><label for="ed-sd">Début</label><div class="field-row"><input type="date" id="ed-sd" class="input grow" value="${s.slice(0, 10)}" ${dis}><input type="time" id="ed-st" class="input" value="${s.slice(11, 16) || '09:00'}" ${allDay0 ? 'hidden' : ''} ${dis}></div></div>
      <div class="field"><label for="ed-ed">Fin</label><div class="field-row"><input type="date" id="ed-ed" class="input grow" value="${(e || s).slice(0, 10)}" ${dis}><input type="time" id="ed-et" class="input" value="${e.slice(11, 16) || '10:00'}" ${allDay0 ? 'hidden' : ''} ${dis}></div></div>
      <div class="field"><label for="ed-loc">${icon('pin', 'hide-m')}Lieu</label><input id="ed-loc" class="input" value="${esc(it.location || '')}" placeholder="Adresse, salle…" ${dis}></div>
      ${repeatHtml}
      <div class="field"><span class="lbl">Rappels</span><div class="chips-edit" id="ed-alarms"></div></div>
      ${common}${notes}${!isNew && !readonly ? `<span class="note">${esc(it.id)}</span>` : ''}`;
  };

  const drawAlarms = () => {
    const box = $('#ed-alarms', wrap);
    if (!box) return;
    const cal = S.calendars.find((c) => c.name === $('#ed-cal', wrap)?.value);
    let html;
    if (alarms === null) html = `<span class="pill">Par défaut du calendrier${cal && cal.alarm && cal.alarm.length ? ' (' + cal.alarm.map((a) => ALARM_LABEL[a] || a).join(', ') + ')' : ' (aucun)'}</span>`;
    else if (!alarms.length) html = '<span class="pill">Aucun rappel</span>';
    else html = alarms.map((a) => `<span class="pill">${icon('bell')}${esc(ALARM_LABEL[a] || a)} avant${readonly ? '' : `<button type="button" data-rm="${esc(a)}" aria-label="Retirer">×</button>`}</span>`).join('');
    if (!readonly) {
      html += `<select class="input" id="ed-alarm-add" style="height:28px;width:auto"><option value="">+ ajouter…</option>${ALARMS.map((a) => `<option value="${a}">${ALARM_LABEL[a]} avant</option>`).join('')}<option value="default">Par défaut du calendrier</option><option value="none">Aucun rappel</option></select>`;
    }
    box.innerHTML = html;
    box.querySelectorAll('[data-rm]').forEach((b) => (b.onclick = () => {
      alarms = alarms.filter((x) => x !== b.dataset.rm);
      drawAlarms();
    }));
    const add = $('#ed-alarm-add', box);
    if (add) add.onchange = () => {
      const v = add.value;
      if (v === 'default') alarms = null;
      else if (v === 'none') alarms = [];
      else if (v) alarms = [...new Set([...(alarms || []), v])];
      drawAlarms();
    };
  };

  const paint = () => {
    form.innerHTML = fieldsHtml();
    drawAlarms();
    const allday = $('#ed-allday', wrap);
    if (allday) allday.onchange = () => {
      $('#ed-st', wrap).hidden = allday.checked;
      $('#ed-et', wrap).hidden = allday.checked;
    };
    const rep = $('#ed-rep', wrap);
    rep.onchange = () => ($('#ed-rrule', wrap).hidden = rep.value !== 'custom');
    for (const seg of wrap.querySelectorAll('#ed-status, #ed-prio')) {
      seg.onclick = (e) => {
        const b = e.target.closest('button');
        if (!b || readonly) return;
        seg.querySelectorAll('button').forEach((x) => x.setAttribute('aria-pressed', x === b));
      };
    }
    $('#ed-cal', wrap).onchange = drawAlarms;
    // la fin suit le début
    const sd = $('#ed-sd', wrap);
    if (sd) {
      let prev = sd.value;
      sd.onchange = () => {
        const ed = $('#ed-ed', wrap);
        if (prev && sd.value && ed.value) ed.value = ds(dn(ed.value) + dn(sd.value) - dn(prev));
        prev = sd.value;
      };
    }
    setTimeout(() => !readonly && isNew && $('#ed-title', wrap)?.focus(), 60);
  };
  paint();

  $('#ed-kind', wrap)?.addEventListener('click', (e) => {
    const b = e.target.closest('[data-k]');
    if (!b) return;
    it.title = $('#ed-title', wrap).value;
    kind = b.dataset.k;
    if (kind === 'task' && !it.due) it.due = startS.slice(0, 10);
    $('#ed-kind', wrap).querySelectorAll('button').forEach((x) => x.setAttribute('aria-pressed', x === b));
    paint();
  });

  const collect = () => {
    const v = (id) => $(id, wrap)?.value ?? '';
    const f = { title: v('#ed-title').trim() };
    const rep = v('#ed-rep');
    let repeat = rep === 'custom' ? v('#ed-rrule').trim() : rep;
    if (repeat === 'WEEKLY') {
      const d = kind === 'task' ? v('#ed-due') : v('#ed-sd');
      repeat = d ? `FREQ=WEEKLY;BYDAY=${CODES[(((dn(d) + 3) % 7) + 7) % 7]}` : 'FREQ=WEEKLY';
    }
    f.repeat = repeat || null;
    f.calendar = v('#ed-cal') || null;
    f.tags = v('#ed-tags').split(',').map((t) => t.trim().replace(/^#/, '')).filter(Boolean);
    f.body = v('#ed-body');
    if (kind === 'task') {
      const d = v('#ed-due');
      const t = v('#ed-due-t');
      f.due = d ? (t ? `${d} ${t}` : d) : null;
      f.status = $('#ed-status [aria-pressed="true"]', wrap)?.dataset.v || 'todo';
      f.priority = $('#ed-prio [aria-pressed="true"]', wrap)?.dataset.v || null;
      return f;
    }
    const all = $('#ed-allday', wrap).checked;
    const sd = v('#ed-sd');
    const ed = v('#ed-ed') || sd;
    if (!sd) throw new Error('date de début manquante');
    if (all) {
      if (dn(ed) < dn(sd)) throw new Error('la fin est avant le début');
      f.start = sd;
      f.end = ed !== sd ? ed : null;
    } else {
      f.start = `${sd} ${v('#ed-st') || '09:00'}`;
      f.end = `${ed} ${v('#ed-et') || '10:00'}`;
      if (dn(ed) * 1440 + mins(f.end) < dn(sd) * 1440 + mins(f.start)) throw new Error('la fin est avant le début');
    }
    f.location = v('#ed-loc').trim() || null;
    f.alarm = alarms === null ? null : alarms.length ? alarms : 'none';
    return f;
  };

  /** Champs modifiés par rapport à l'élément d'origine. */
  const diff = (f) => {
    const orig = {
      title: it.title,
      repeat: it.repeat || null,
      calendar: it.calendar || null,
      tags: it.tags || [],
      body: it.body || '',
      due: it.due || null,
      status: it.status || 'todo',
      priority: it.priority || null,
      start: startS,
      end: kind === 'event' ? (allDay0 ? (endS && endS !== startS ? endS : null) : endS) : null,
      location: it.location || null,
      alarm: it.alarm === undefined ? null : it.alarm,
    };
    const out = {};
    for (const [k, v] of Object.entries(f)) if (JSON.stringify(v) !== JSON.stringify(orig[k] ?? null)) out[k] = v;
    return out;
  };

  form.onsubmit = async (e) => {
    e.preventDefault();
    if (readonly) return close();
    let f;
    try {
      f = collect();
    } catch (err) {
      return app.error(err);
    }
    if (!f.title) return app.error('Le titre est vide');
    try {
      if (isNew) {
        const fields = Object.fromEntries(Object.entries(f).filter(([, v]) => v !== null && v !== '' && !(Array.isArray(v) && !v.length)));
        close();
        await app.act('create', { kind, fields }, `Créé : ${f.title}`);
        const d = f.start || f.due;
        if (d && kind === 'event') app.go(dayOf(d));
        return;
      }
      const changes = diff(f);
      if (!Object.keys(changes).length) return close();
      if (occ && occ.recurring && occ.occurrence) {
        const scope = await app.choose('Événement répété', `Appliquer les modifications de « ${f.title} » à :`, [
          { label: 'Toute la série', value: 'all' },
          { label: 'Cette occurrence', value: 'one', primary: true },
        ]);
        if (!scope) return;
        close();
        const timeChanged = 'start' in changes || 'end' in changes || scope === 'one';
        let id = it.id;
        const rest = { ...changes };
        delete rest.start;
        delete rest.end;
        if (scope === 'all' && 'repeat' in rest) {
          // nouvelle règle : le début de série reste celui de la série
        }
        if (timeChanged) {
          const endForMove = f.end || (f.start.length === 10 ? f.start : null);
          const r = await app.mutate('move', { id, occurrence: occ.occurrence, start: f.start, end: endForMove, scope });
          if (r.id) id = r.id;
        }
        if (Object.keys(rest).length) await app.mutate('update', { id, fields: rest });
        await app.act('info', {}, scope === 'one' ? `Occurrence modifiée : ${f.title}` : `Série modifiée : ${f.title}`);
        return;
      }
      close();
      await app.act('update', { id: it.id, fields: changes, rev: it.rev }, `Enregistré : ${f.title}`);
    } catch (err) {
      /* signalé par app.act */
    }
  };

  wrap.addEventListener('click', async (e) => {
    if (e.target === wrap || e.target.closest('[data-close]')) return close();
    if (e.target.closest('[data-del]')) {
      if (occ && occ.recurring && occ.occurrence) {
        const scope = await app.choose('Supprimer', `« ${it.title} » est répété.`, [
          { label: 'Toute la série', value: 'all' },
          { label: 'Cette occurrence', value: 'one', primary: true },
        ]);
        if (!scope) return;
        close();
        await app.act('delete', scope === 'one' ? { id: it.id, occurrence: occ.occurrence } : { id: it.id }, `Supprimé : ${it.title}`).catch(() => {});
      } else {
        close();
        await app.act('delete', { id: it.id }, `« ${it.title} » mis à la corbeille`).catch(() => {});
      }
    }
  });
  wrap.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      e.stopPropagation();
      close();
    } else if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      form.requestSubmit();
    }
  });
  $('#layer').append(wrap);
}
