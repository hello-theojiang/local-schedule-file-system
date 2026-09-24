// Transport unique vers le cœur : Tauri (invoke) ou `agenda serve` (HTTP).

const T = window.__TAURI__;

export const host = {
  tauri: !!T,
  android: /Android/i.test(navigator.userAgent),
  mac: /Mac OS X/.test(navigator.userAgent) && !/Android/i.test(navigator.userAgent),
};

let token = null;
try {
  token = localStorage.getItem('agenda.token');
} catch (e) {}

export class ApiError extends Error {}

export async function call(method, params = {}) {
  let text;
  if (T) {
    text = await T.core.invoke('api', { method, params: JSON.stringify(params) });
  } else {
    const headers = { 'Content-Type': 'application/json' };
    if (token) headers.Authorization = 'Bearer ' + token;
    const r = await fetch('/api/' + method, { method: 'POST', headers, body: JSON.stringify(params), credentials: 'same-origin' });
    if (r.status === 401) {
      const t = prompt('Jeton d’accès à cet agenda (AGENDA_TOKEN) :');
      if (t) {
        token = t;
        try {
          localStorage.setItem('agenda.token', t);
        } catch (e) {}
        return call(method, params);
      }
    }
    text = await r.text();
  }
  let j;
  try {
    j = JSON.parse(text);
  } catch (e) {
    throw new ApiError('réponse illisible');
  }
  if (!j.ok) throw new ApiError(j.error || 'erreur');
  return j.result;
}

/** Notifications (plugin Tauri) si disponibles. */
export const notif = () => (T && T.notification) || null;
