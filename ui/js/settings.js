// Réglages, résolution des conflits, accueil (choix du dossier), import/export.
import { $, esc, icon, h, todayN, ds } from './util.js';
import { testNotification } from './notify.js';

const ACCENTS = [['violet', '#6d5dfc'], ['bleu', '#2f7cf6'], ['vert', '#14a37f'], ['orange', '#ee7a2f'], ['rose', '#e0467c'], ['graphite', '#5b6477']];
const android = () => window.AgendaAndroid;

function dialog(title, bodyHtml, footHtml = '') {
  const el = h(`<div class="overlay" role="dialog" aria-modal="true"><div class="dialog">
    <div class="dialog-head"><h2>${esc(title)}</h2><span style="flex:1"></span><button class="btn ghost icon" data-x aria-label="Fermer">${icon('x')}</button></div>
    <div class="dialog-body">${bodyHtml}</div>${footHtml ? `<div class="dialog-foot">${footHtml}</div>` : ''}</div></div>`);
  const close = () => {
    el.remove();
    document.removeEventListener('keydown', key, true);
  };
  const key = (e) => {
    if (e.key === 'Escape') {
      e.stopPropagation();
      close();
    }
  };
  document.addEventListener('keydown', key, true);
  el.addEventListener('pointerdown', (e) => {
    if (e.target === el) close();
  });
  el.querySelector('[data-x]').onclick = close;
  $('#layer').append(el);
  return { el, close };
}

// ------------------------------------------------------------ réglages
export function openSettings(app) {
  const S = app.state;
  const hi = S.hostInfo || {};
  const p = app.prefs;
  const theme = p.theme || 'auto';
  const subs = S.calendars.filter((c) => c.readonly);
  const own = S.calendars.filter((c) => !c.readonly);
  const folderHtml = hi.host === 'serve'
    ? `<p class="note">Servi par <code>agenda serve</code> depuis <b>${esc(S.info?.path || '')}</b>.</p>`
    : `<div class="field-row"><code style="flex:1;overflow-wrap:anywhere">${esc(S.info?.path || '—')}</code><button class="btn" data-folder>${icon('folder')}Changer…</button></div>
       ${android() ? `<p class="note">Accès à tous les fichiers : <b>${android().hasStorageAccess() ? 'autorisé' : 'non autorisé'}</b> <button class="btn" data-perm style="margin-left:6px">Réglages Android</button></p>` : ''}`;
  const { el, close } = dialog('Réglages', `
    <section class="set-group"><h3>Apparence</h3>
      <div class="field-row" style="gap:14px"><div class="segmented" id="set-theme">${[['auto', 'Auto'], ['light', 'Clair'], ['dark', 'Sombre']].map(([k, l]) => `<button data-v="${k}" aria-pressed="${theme === k}">${l}</button>`).join('')}</div>
      <div class="swatches" id="set-accent">${ACCENTS.map(([k, c]) => `<button data-v="${k}" style="--c:${c}" aria-label="${k}" aria-pressed="${(p.accent || 'violet') === k}"></button>`).join('')}</div></div></section>
    <section class="set-group"><h3>Dossier de l'agenda</h3>${folderHtml}</section>
    <section class="set-group"><h3>Calendriers</h3><div class="list-edit" id="set-cals">
      ${own.map((c) => `<div class="item"><input type="color" value="${esc(c.color)}" data-cal="${esc(c.name)}" aria-label="Couleur de ${esc(c.title)}"><span style="flex:1">${esc(c.title)} <small class="note">${c.count} élément(s)</small></span></div>`).join('')}
      </div><form class="field-row" id="set-newcal" style="margin-top:8px"><input class="input grow" name="n" placeholder="Nouveau calendrier (ex. Travail)"><input type="color" name="c" value="#2f7cf6" aria-label="Couleur"><button class="btn">Ajouter</button></form></section>
    <section class="set-group"><h3>Abonnements (lecture seule)</h3><div class="list-edit">
      ${subs.map((c) => `<div class="item"><i class="swatch" style="width:12px;height:12px;border-radius:4px;background:${esc(c.color)}"></i><span style="flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis">${esc(c.title)} <small class="note">${esc(c.url || '')}</small></span></div>`).join('') || '<p class="note" style="margin:0">Aucun. Ajoutez l’URL d’un calendrier .ics (webcal://… ou https://…).</p>'}
      </div><form class="field-row" id="set-newsub" style="margin-top:8px"><input class="input" name="n" placeholder="Nom" style="width:120px"><input class="input grow" name="u" placeholder="https://… ou webcal://…"><button class="btn">S’abonner</button></form>
      ${subs.length ? `<button class="btn" data-subs style="margin-top:8px">${icon('refresh')}Actualiser maintenant</button>` : ''}</section>
    <section class="set-group"><h3>Rappels</h3><p class="note" style="margin:0 0 8px">${esc(reminderText(app))}</p><button class="btn" data-test>${icon('bell')}Tester une notification</button></section>
    <section class="set-group"><h3>Import / export iCalendar</h3><div class="field-row"><button class="btn" data-import>${icon('upload')}Importer un .ics</button><button class="btn" data-export>${icon('download')}Exporter en .ics</button></div>
      ${hi.host === 'serve' ? `<p class="note">Flux à ajouter dans un autre agenda : <code>${esc(location.origin)}/agenda.ics</code></p>` : ''}</section>
    <section class="set-group"><h3>À propos</h3><p class="note" style="margin:0">Agenda ${esc(hi.version || '')} · fuseau ${esc(S.info?.timezone || '')} · ${S.info?.events ?? 0} événements, ${S.info?.tasks ?? 0} tâches<br>
      Les données sont des fichiers Markdown : rien n’est envoyé nulle part. <a href="#" data-keys>Raccourcis clavier</a></p></section>`);

  el.querySelector('#set-theme').onclick = (e) => {
    const b = e.target.closest('[data-v]');
    if (!b) return;
    app.prefs.theme = b.dataset.v;
    app.savePrefs();
    app.applyTheme();
    el.querySelectorAll('#set-theme button').forEach((x) => x.setAttribute('aria-pressed', x === b));
  };
  el.querySelector('#set-accent').onclick = (e) => {
    const b = e.target.closest('[data-v]');
    if (!b) return;
    app.prefs.accent = b.dataset.v;
    app.savePrefs();
    app.applyTheme();
    el.querySelectorAll('#set-accent button').forEach((x) => x.setAttribute('aria-pressed', x === b));
  };
  el.querySelectorAll('#set-cals input[type=color]').forEach((inp) => {
    inp.onchange = () => app.act('calendar_save', { name: inp.dataset.cal, fields: { color: inp.value } }, 'Couleur modifiée');
  });
  el.querySelector('#set-newcal').onsubmit = async (e) => {
    e.preventDefault();
    const f = e.target;
    const n = f.n.value.trim();
    if (!n) return;
    await app.act('calendar_save', { name: n, fields: { title: n, color: f.c.value } }, `Calendrier « ${n} » créé`).catch(() => {});
    close();
    openSettings(app);
  };
  el.querySelector('#set-newsub').onsubmit = async (e) => {
    e.preventDefault();
    const f = e.target;
    const n = f.n.value.trim();
    const u = f.u.value.trim();
    if (!n || !/^(https?|webcal):\/\//.test(u)) return app.error('Nom et URL (https:// ou webcal://) requis');
    try {
      await app.act('calendar_save', { name: n, fields: { title: n, url: u } }, false);
      await updateSubscriptions(app);
      close();
      openSettings(app);
    } catch (err) {}
  };
  el.querySelector('[data-subs]')?.addEventListener('click', () => updateSubscriptions(app));
  el.querySelector('[data-test]').onclick = () => testNotification(app);
  el.querySelector('[data-import]').onclick = () => importIcs(app);
  el.querySelector('[data-export]').onclick = () => exportIcs(app);
  el.querySelector('[data-keys]').onclick = (e) => {
    e.preventDefault();
    close();
    showShortcuts(app);
  };
  el.querySelector('[data-folder]')?.addEventListener('click', () => {
    close();
    chooseFolder(app);
  });
  el.querySelector('[data-perm]')?.addEventListener('click', () => android().requestStorageAccess());
}

function reminderText(app) {
  const n = window.__TAURI__?.notification;
  if (app.host.android) return 'Les rappels sont programmés dans Android pour les 7 prochains jours : ils arrivent même application fermée. Ouvrez l’application de temps en temps pour prendre en compte les nouveaux éléments synchronisés.';
  if (n) return 'Notifications du système tant que l’application est ouverte. Pour les recevoir application fermée : service « agenda remind ».';
  return 'Notifications du navigateur tant que cette page est ouverte ; sinon « agenda remind » (notify-send, ntfy…).';
}

// ------------------------------------------------------------ conflits
export async function openConflicts(app) {
  let list;
  try {
    list = await app.call('conflicts');
  } catch (e) {
    return app.error(e);
  }
  const body = list.length
    ? list.map((c, i) => `<div class="conflict" data-i="${i}"><div><b>${esc(c.title || c.original)}</b> <span class="pill">${c.source === 'syncthing' ? 'Syncthing' : 'modification simultanée'}</span><br><small class="note">${esc(c.path)}</small></div>
      ${c.fields.length ? `<table><tr><th>Champ</th><th>Version actuelle</th><th>Autre version</th></tr>${c.fields.map((f) => `<tr><td>${esc(f.field)}</td>
        <td><label><input type="radio" name="c${i}-${esc(f.field)}" value="original" checked> ${esc(show(f.original))}</label></td>
        <td><label><input type="radio" name="c${i}-${esc(f.field)}" value="conflict"> ${esc(show(f.conflict))}</label></td></tr>`).join('')}</table>` : '<p class="note" style="margin:0">Contenu identique : la copie peut être retirée sans perte.</p>'}
      <div class="field-row"><button class="btn primary" data-apply>Appliquer ce choix</button><button class="btn" data-keep="conflict">Garder l'autre version</button></div></div>`).join('')
    : '<p class="note">Aucun conflit : tout est synchronisé.</p>';
  const { el, close } = dialog('Conflits de synchronisation', `<p class="note" style="margin:0">Deux appareils ont modifié le même élément. Choisissez champ par champ ; la version écartée est placée dans <code>.trash/</code>.</p>${body}`);
  el.addEventListener('click', async (e) => {
    const box = e.target.closest('.conflict');
    if (!box) return;
    const c = list[+box.dataset.i];
    let keep = null;
    if (e.target.closest('[data-keep]')) keep = 'conflict';
    else if (e.target.closest('[data-apply]')) {
      keep = {};
      for (const f of c.fields) keep[f.field] = box.querySelector(`input[name="c${box.dataset.i}-${CSS.escape(f.field)}"]:checked`)?.value || 'original';
      if (Object.values(keep).every((v) => v === 'original')) keep = 'original';
    }
    if (!keep) return;
    try {
      await app.act('conflict_resolve', { path: c.path, keep }, 'Conflit résolu');
      const n = (await app.call('info')).conflicts;
      app.showConflicts(n);
      close();
      if (n) openConflicts(app);
    } catch (err) {}
  });
}

const show = (v) => (v === null || v === undefined ? '∅' : String(v).length > 140 ? String(v).slice(0, 140) + '…' : String(v));

// ------------------------------------------------------------ import / export / abonnements
export function importIcs(app) {
  const inp = h('<input type="file" accept=".ics,text/calendar" hidden>');
  document.body.append(inp);
  inp.onchange = async () => {
    const f = inp.files[0];
    inp.remove();
    if (!f) return;
    const text = await f.text();
    try {
      const r = await app.act('ics_import', { text }, false);
      app.toast(`${r.created} élément(s) importé(s)${r.skipped ? `, ${r.skipped} déjà présent(s)` : ''}`, { undo: r.created > 0 });
    } catch (e) {}
  };
  inp.click();
}

export async function exportIcs(app) {
  try {
    const text = await app.call('ics_export', {});
    const name = `agenda-${ds(todayN())}.ics`;
    if (app.host.tauri) {
      const path = await app.call('save_file', { name, text });
      if (path) app.toast(`Exporté : ${path}`);
      return;
    }
    const url = URL.createObjectURL(new Blob([text], { type: 'text/calendar' }));
    const a = h(`<a href="${url}" download="${name}"></a>`);
    document.body.append(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 5000);
  } catch (e) {
    app.error(e);
  }
}

export async function updateSubscriptions(app) {
  app.toast('Actualisation des abonnements…');
  try {
    const r = await app.call('sub_update');
    const ok = r.filter((x) => !x.error);
    const ko = r.filter((x) => x.error);
    await app.refresh();
    if (ko.length) app.toast(`${ko[0].name} : ${ko[0].error}`, { error: true });
    else app.toast(`${ok.length} abonnement(s) à jour`);
  } catch (e) {
    app.error(e);
  }
}

export function showShortcuts() {
  dialog('Raccourcis et syntaxe', `
    <section class="set-group"><h3>Clavier</h3><table style="width:100%;font-size:13px;border-collapse:collapse">
      ${[['N', 'nouvel élément (langage naturel)'], ['Ctrl K ou /', 'rechercher, créer, commandes'], ['T', "aujourd'hui"], ['← → ou J K', 'période précédente / suivante'], ['D S M X', 'vues Jour, Semaine, Mois, Tâches'], ['Ctrl Z / Ctrl ⇧ Z', 'annuler / rétablir'], ['Tab (palette)', 'événement ⇄ tâche'], ['Ctrl Entrée (fiche)', 'enregistrer']]
        .map(([k, d]) => `<tr><td style="padding:5px 0"><span class="kbd">${k}</span></td><td>${d}</td></tr>`).join('')}</table></section>
    <section class="set-group"><h3>Saisie naturelle</h3><p class="note" style="margin:0;line-height:1.8">
      « Dentiste vendredi 14h-15h @Cabinet #santé »<br>« Sport tous les mardis 18h30 pendant 1h30 »<br>« Rapport demain !haute »<br>
      « Vacances du 20 au 24 octobre » · « Club le premier mardi du mois 20h » · « Loyer tous les 5 du mois »<br>
      <b>@</b>lieu · <b>#</b>mot-clé · <b>!</b>haute/basse · <b>+</b>calendrier · « rappel 15min » · « jusqu'au 19 décembre » · « 5 fois »</p></section>
    <section class="set-group"><h3>Souris et doigt</h3><p class="note" style="margin:0">Glisser un événement pour le déplacer, tirer son bord bas pour changer la durée, sélectionner un créneau vide pour créer. Au doigt : appui long puis glisser.</p></section>`);
}

// ------------------------------------------------------------ dossier
async function openPath(app, path, create) {
  try {
    await app.call('open', { path, create });
    await app.opened();
    app.toast(`Dossier ouvert : ${path}`);
    return true;
  } catch (e) {
    app.error(e);
    return false;
  }
}

export async function chooseFolder(app) {
  if (app.host.android || !app.host.tauri) return renderOnboarding(app, $('#view'), true);
  try {
    const path = await app.call('pick_folder');
    if (!path) return;
    try {
      await app.call('open', { path, create: false });
      await app.opened();
    } catch (e) {
      const ok = await app.choose('Nouveau dossier agenda ?', `${e.message}`, [{ label: 'Créer un agenda ici', value: true, primary: true }]);
      if (ok) await openPath(app, path, true);
    }
  } catch (e) {
    app.error(e);
  }
}

export function renderOnboarding(app, el, again = false) {
  const hi = app.state.hostInfo || {};
  const a = android();
  const ext = a ? a.externalStorage() : '/storage/emulated/0';
  const suggestions = a ? [`${ext}/Sync/agenda`, `${ext}/Syncthing/agenda`, `${ext}/Documents/agenda`, `${ext}/agenda`] : [];
  const desktop = app.host.tauri && !a;
  el.innerHTML = `<div class="onboard"><div class="onboard-card">
    <img src="icon.png" alt="">
    <h1>${again ? 'Changer de dossier' : 'Bienvenue'}</h1>
    <p>Votre agenda est un simple dossier de fichiers Markdown, synchronisé par vos soins (Syncthing…). Choisissez-le, ou créez-en un nouveau.</p>
    ${a && !a.hasStorageAccess() ? `<div class="warn-box">Pour lire le dossier Syncthing, l'application a besoin de l'accès à tous les fichiers.<br><button class="btn primary" data-perm style="margin-top:8px">Autoriser l'accès</button></div>` : ''}
    ${desktop ? `<button class="btn primary" data-pick>${icon('folder')}Choisir le dossier…</button>` : `
      <div class="field"><label for="ob-path">Chemin du dossier</label><input id="ob-path" class="input" value="${esc(hi.dir || suggestions[0] || '')}" placeholder="/chemin/vers/agenda" autocapitalize="off" spellcheck="false">
      <div class="suggest">${suggestions.map((s) => `<button data-s="${esc(s)}">${esc(s)}</button>`).join('')}</div></div>
      <div class="field-row" style="justify-content:center"><button class="btn primary" data-open>Ouvrir</button><button class="btn" data-create>Créer un agenda ici</button></div>`}
    ${again ? '<button class="btn ghost" data-cancel>Annuler</button>' : ''}
  </div></div>`;
  el.querySelector('[data-pick]')?.addEventListener('click', () => chooseFolder(app));
  el.querySelector('[data-perm]')?.addEventListener('click', () => {
    a.requestStorageAccess();
    const t = setInterval(() => {
      if (a.hasStorageAccess()) {
        clearInterval(t);
        renderOnboarding(app, el, again);
      }
    }, 1000);
  });
  el.querySelectorAll('[data-s]').forEach((b) => (b.onclick = () => (el.querySelector('#ob-path').value = b.dataset.s)));
  el.querySelector('[data-open]')?.addEventListener('click', () => openPath(app, el.querySelector('#ob-path').value.trim(), false));
  el.querySelector('[data-create]')?.addEventListener('click', () => openPath(app, el.querySelector('#ob-path').value.trim(), true));
  el.querySelector('[data-cancel]')?.addEventListener('click', () => app.render());
}
