// Glisser-déposer unifié : souris (après 4 px de mouvement) et doigt (appui long).
// Au doigt, le défilement reste possible tant que l'appui long n'a pas démarré.

const LONG_PRESS = 380;

/**
 * @param {HTMLElement} el
 * @param {{start?: Function, move?: Function, end?: Function, cancel?: Function, click?: Function, filter?: Function}} o
 */
export function draggable(el, o) {
  let active = false;
  el.addEventListener(
    'touchmove',
    (e) => {
      if (active && e.cancelable) e.preventDefault();
    },
    { passive: false },
  );
  el.addEventListener('contextmenu', (e) => {
    if (active || el.dataset.lp) e.preventDefault();
  });
  el.addEventListener('pointerdown', (e) => {
    if (e.button !== 0 || (o.filter && !o.filter(e))) return;
    const touch = e.pointerType !== 'mouse';
    const sx = e.clientX;
    const sy = e.clientY;
    const pid = e.pointerId;
    let started = false;
    let timer = null;
    const begin = (ev) => {
      started = true;
      active = true;
      try {
        el.setPointerCapture(pid);
      } catch (err) {}
      if (touch && navigator.vibrate) navigator.vibrate(12);
      o.start && o.start(ev, { x: sx, y: sy });
    };
    const move = (ev) => {
      if (ev.pointerId !== pid) return;
      if (!started) {
        const d = Math.hypot(ev.clientX - sx, ev.clientY - sy);
        if (touch) {
          if (d > 10) cleanup();
        } else if (d > 4) begin(e);
        if (!started) return;
      }
      ev.preventDefault();
      o.move && o.move(ev);
    };
    const up = (ev) => {
      if (ev.pointerId !== pid) return;
      const was = started;
      cleanup();
      if (was) o.end && o.end(ev);
      else if (o.click && Math.hypot(ev.clientX - sx, ev.clientY - sy) < 10) o.click(ev);
    };
    const cancel = (ev) => {
      if (ev.pointerId !== pid) return;
      const was = started;
      cleanup();
      if (was) o.cancel && o.cancel(ev);
    };
    const cleanup = () => {
      clearTimeout(timer);
      active = false;
      delete el.dataset.lp;
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
      window.removeEventListener('pointercancel', cancel);
    };
    if (touch) {
      el.dataset.lp = '1';
      timer = setTimeout(() => begin(e), LONG_PRESS);
    }
    window.addEventListener('pointermove', move, { passive: false });
    window.addEventListener('pointerup', up);
    window.addEventListener('pointercancel', cancel);
  });
}

/** Défilement automatique quand le pointeur approche des bords d'un conteneur. */
export function autoScroll(container, ev) {
  const r = container.getBoundingClientRect();
  const edge = 48;
  if (ev.clientY < r.top + edge) container.scrollTop -= Math.ceil((r.top + edge - ev.clientY) / 4);
  else if (ev.clientY > r.bottom - edge) container.scrollTop += Math.ceil((ev.clientY - (r.bottom - edge)) / 4);
}
