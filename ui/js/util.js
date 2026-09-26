// Utilitaires : DOM, échappement, dates (numéros de jour comme dans le cœur).

export const $ = (s, r = document) => r.querySelector(s);
export const $$ = (s, r = document) => [...r.querySelectorAll(s)];

const ESC = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };
export const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ESC[c]);

export const icon = (n, cls = '') => `<svg class="${cls}"><use href="#i-${n}"/></svg>`;

const p2 = (n) => String(n).padStart(2, '0');

export const MONTHS = ['janvier', 'février', 'mars', 'avril', 'mai', 'juin', 'juillet', 'août', 'septembre', 'octobre', 'novembre', 'décembre'];
export const MONTHS_SHORT = ['janv.', 'févr.', 'mars', 'avr.', 'mai', 'juin', 'juil.', 'août', 'sept.', 'oct.', 'nov.', 'déc.'];
export const DAYS = ['lundi', 'mardi', 'mercredi', 'jeudi', 'vendredi', 'samedi', 'dimanche'];
export const DAYS_SHORT = ['lun.', 'mar.', 'mer.', 'jeu.', 'ven.', 'sam.', 'dim.'];
export const DAYS_1 = ['L', 'M', 'M', 'J', 'V', 'S', 'D'];

/** « 2026-09-24 » → numéro de jour depuis 1970-01-01. */
export function dn(s) {
  return Math.floor(Date.UTC(+s.slice(0, 4), +s.slice(5, 7) - 1, +s.slice(8, 10)) / 864e5);
}
/** numéro de jour → « 2026-09-24 ». */
export function ds(n) {
  const t = new Date(n * 864e5);
  return `${t.getUTCFullYear()}-${p2(t.getUTCMonth() + 1)}-${p2(t.getUTCDate())}`;
}
export const ymd = (n) => {
  const t = new Date(n * 864e5);
  return { y: t.getUTCFullYear(), m: t.getUTCMonth() + 1, d: t.getUTCDate() };
};
/** 0 = lundi … 6 = dimanche */
export const wd = (n) => (((n + 3) % 7) + 7) % 7;
export const weekStart = (n) => n - wd(n);
export function todayN() {
  const t = new Date();
  return Math.floor(Date.UTC(t.getFullYear(), t.getMonth(), t.getDate()) / 864e5);
}
export function nowMin() {
  const t = new Date();
  return t.getHours() * 60 + t.getMinutes();
}
/** minutes depuis minuit d'une valeur « AAAA-MM-JJ HH:MM » */
export const mins = (s) => (s && s.length > 10 ? +s.slice(11, 13) * 60 + +s.slice(14, 16) : 0);
export const dayOf = (s) => dn(s.slice(0, 10));
/** position absolue en minutes (jour × 1440 + minutes) */
export const abs = (s) => dayOf(s) * 1440 + mins(s);
export function fromAbs(a) {
  const d = Math.floor(a / 1440);
  const m = a - d * 1440;
  return `${ds(d)} ${p2(Math.floor(m / 60))}:${p2(m % 60)}`;
}
export const hm = (m) => `${p2(Math.floor(m / 60) % 24)}:${p2(m % 60)}`;
export const timeOf = (s) => (s && s.length > 10 ? s.slice(11, 16) : '');

export function monthStart(n) {
  const { y, m } = ymd(n);
  return Math.floor(Date.UTC(y, m - 1, 1) / 864e5);
}
export function addMonths(n, k) {
  const { y, m, d } = ymd(n);
  const t = new Date(Date.UTC(y, m - 1 + k, 1));
  const dim = new Date(Date.UTC(t.getUTCFullYear(), t.getUTCMonth() + 1, 0)).getUTCDate();
  return Math.floor(Date.UTC(t.getUTCFullYear(), t.getUTCMonth(), Math.min(d, dim)) / 864e5);
}

/** « jeudi 24 septembre » */
export function fmtDay(n, withYear = false) {
  const { y, m, d } = ymd(n);
  return `${DAYS[wd(n)]} ${d === 1 ? '1er' : d} ${MONTHS[m - 1]}${withYear ? ' ' + y : ''}`;
}
export function fmtShort(n) {
  const { m, d } = ymd(n);
  return `${DAYS_SHORT[wd(n)]} ${d} ${MONTHS_SHORT[m - 1]}`;
}
export function relDay(n) {
  const t = todayN();
  if (n === t) return "Aujourd'hui";
  if (n === t + 1) return 'Demain';
  if (n === t - 1) return 'Hier';
  if (n === t + 2) return 'Après-demain';
  return '';
}
/** durée lisible : 90 → « 1 h 30 » */
export function fmtDur(m) {
  if (m < 60) return `${m} min`;
  const h = Math.floor(m / 60);
  const r = m % 60;
  return r ? `${h} h ${p2(r)}` : `${h} h`;
}
export const cap = (s) => (s ? s[0].toUpperCase() + s.slice(1) : s);

export function debounce(fn, ms) {
  let t;
  return (...a) => {
    clearTimeout(t);
    t = setTimeout(() => fn(...a), ms);
  };
}

export function hash31(s) {
  let h = 0;
  for (let i = 0; i < s.length; i++) h = (Math.imul(31, h) + s.charCodeAt(i)) | 0;
  return Math.abs(h) % 2147483000 || 1;
}

export function h(html) {
  const t = document.createElement('template');
  t.innerHTML = html.trim();
  return t.content.firstElementChild;
}

export const isNarrow = () => matchMedia('(max-width: 860px)').matches;
export const isTouch = () => matchMedia('(pointer: coarse)').matches;
