// Thème appliqué avant le premier rendu (évite un flash clair/sombre).
try {
  var p = JSON.parse(localStorage.getItem('agenda.prefs') || '{}');
  var dark = p.theme === 'dark' || (p.theme !== 'light' && matchMedia('(prefers-color-scheme: dark)').matches);
  document.documentElement.dataset.theme = dark ? 'dark' : 'light';
  document.documentElement.dataset.accent = p.accent || 'violet';
} catch (e) {}
