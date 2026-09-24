// Rappels dans l'application.
// - Android (plugin Tauri) : notifications programmées dans le système pour 7 jours,
//   elles arrivent même application fermée ;
// - bureau (Tauri) : notification système à l'heure dite tant que l'application tourne ;
// - navigateur (agenda serve) : API Notification tant que la page est ouverte.
import { notif } from './api.js';
import { hash31 } from './util.js';

const timers = [];
const SCHED_KEY = 'agenda.scheduled';
let busy = false;

async function allowed(n) {
  try {
    if (await n.isPermissionGranted()) return true;
    return (await n.requestPermission()) === 'granted';
  } catch (e) {
    return false;
  }
}

export async function scheduleReminders(app) {
  if (busy || !app.state.info) return;
  busy = true;
  try {
    const now = Math.floor(Date.now() / 1000);
    const horizon = app.host.android ? 7 * 86400 : 86400;
    const list = await app.call('alarms', { from_utc: now + 1, to_utc: now + horizon });
    for (const t of timers.splice(0)) clearTimeout(t);
    const n = notif();
    if (n && app.host.android) {
      if (!(await allowed(n))) return;
      let old = [];
      try {
        old = JSON.parse(localStorage.getItem(SCHED_KEY) || '[]');
      } catch (e) {}
      if (old.length) {
        try {
          await n.cancel(old);
        } catch (e) {}
      }
      const ids = [];
      for (const a of list.slice(0, 64)) {
        const id = hash31(a.key);
        ids.push(id);
        try {
          await n.sendNotification({ id, title: a.title, body: a.body, schedule: n.Schedule.at(new Date(a.at_utc * 1000), false, true) });
        } catch (e) {
          console.warn('programmation impossible', e);
        }
      }
      localStorage.setItem(SCHED_KEY, JSON.stringify(ids));
      return;
    }
    for (const a of list) {
      const delay = a.at_utc * 1000 - Date.now();
      if (delay < 0 || delay > 86400e3) continue;
      timers.push(setTimeout(() => fire(app, a), delay));
    }
  } catch (e) {
    console.warn('rappels', e);
  } finally {
    busy = false;
  }
}

async function fire(app, a) {
  app.toast(`🔔 ${a.title} — ${a.body}`);
  const n = notif();
  if (n) {
    if (await allowed(n)) n.sendNotification({ title: a.title, body: a.body });
  } else if ('Notification' in window && Notification.permission === 'granted') {
    new Notification(a.title, { body: a.body, icon: 'icon.png', tag: a.key });
  }
}

export async function testNotification(app) {
  const n = notif();
  if (n) {
    if (!(await allowed(n))) return app.error('Notifications refusées dans les réglages du système');
    if (app.host.android) {
      await n.sendNotification({ id: 1, title: 'Agenda', body: 'Rappel de test (dans 5 secondes)', schedule: n.Schedule.at(new Date(Date.now() + 5000), false, true) });
      return app.toast('Notification programmée dans 5 secondes : vous pouvez fermer l’application');
    }
    n.sendNotification({ title: 'Agenda', body: 'Les notifications fonctionnent.' });
    return;
  }
  if (!('Notification' in window)) return app.error('Ce navigateur ne gère pas les notifications');
  const p = Notification.permission === 'granted' ? 'granted' : await Notification.requestPermission();
  if (p !== 'granted') return app.error('Notifications refusées par le navigateur');
  new Notification('Agenda', { body: 'Les notifications fonctionnent.', icon: 'icon.png' });
}
