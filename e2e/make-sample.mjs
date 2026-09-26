// Génère un dossier agenda d'exemple, daté autour d'aujourd'hui.
// Usage : node make-sample.mjs <dossier> [AAAA-MM-JJ]
import fs from 'node:fs';
import path from 'node:path';

const dir = process.argv[2];
if (!dir) throw new Error('dossier manquant');
const base = process.argv[3] ? new Date(process.argv[3] + 'T12:00:00') : new Date();
const p2 = (n) => String(n).padStart(2, '0');
const day = (k) => {
  const d = new Date(base.getFullYear(), base.getMonth(), base.getDate() + k);
  return `${d.getFullYear()}-${p2(d.getMonth() + 1)}-${p2(d.getDate())}`;
};
// lundi de la semaine courante
const dow = (base.getDay() + 6) % 7;
const wk = (k) => day(k - dow);
const write = (rel, text) => {
  const f = path.join(dir, rel);
  fs.mkdirSync(path.dirname(f), { recursive: true });
  fs.writeFileSync(f, text);
};
fs.rmSync(dir, { recursive: true, force: true });
write('calendars/perso.md', '---\ntitle: Perso\ncolor: "#6d5dfc"\nalarm: [15m]\n---\n');
write('calendars/travail.md', '---\ntitle: Travail\ncolor: "#2f7cf6"\n---\n');
write('calendars/sante.md', '---\ntitle: Santé\ncolor: "#e0467c"\nalarm: [1h]\n---\n');
write('calendars/cours.md', '---\ntitle: Cours\ncolor: "#14a37f"\n---\n');
const ev = (slug, f, body = '') => {
  const m = f.start.slice(0, 7);
  const lines = Object.entries(f).map(([k, v]) => `${k}: ${Array.isArray(v) ? '[' + v.join(', ') + ']' : v}`);
  write(`events/${m}/${slug}.md`, `---\n${lines.join('\n')}\n---\n${body}`);
};
ev('cours-algo', { title: "Cours d'algorithmique", start: `${wk(0)} 08:30`, end: `${wk(0)} 10:30`, calendar: 'cours', location: 'Amphi B', repeat: 'FREQ=WEEKLY;BYDAY=MO,TH' });
ev('td-maths', { title: 'TD de maths', start: `${wk(1)} 10:00`, end: `${wk(1)} 12:00`, calendar: 'cours', location: 'Salle 204', repeat: 'FREQ=WEEKLY;BYDAY=TU' });
ev('sport', { title: 'Sport', start: `${wk(1)} 18:30`, end: `${wk(1)} 20:00`, calendar: 'perso', location: 'Gymnase', repeat: 'FREQ=WEEKLY;BYDAY=TU', tags: ['sport'] });
ev('dentiste', { title: 'Dentiste', start: `${wk(4)} 14:00`, end: `${wk(4)} 15:00`, calendar: 'sante', location: 'Cabinet', tags: ['santé'] }, 'Apporter la carte vitale.\n');
ev('stage-reunion', { title: 'Réunion de stage', start: `${wk(2)} 14:00`, end: `${wk(2)} 15:30`, calendar: 'travail', location: 'Visio' });
ev('dejeuner', { title: 'Déjeuner avec Léa', start: `${wk(2)} 12:15`, end: `${wk(2)} 13:30`, calendar: 'perso', location: 'Le Petit Zinc' });
ev('point-projet', { title: 'Point projet', start: `${wk(2)} 14:30`, end: `${wk(2)} 15:00`, calendar: 'travail' });
ev('standup', { title: 'Standup', start: `${wk(0)} 11:00`, end: `${wk(0)} 11:15`, calendar: 'travail', repeat: 'FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR' });
ev('concert', { title: 'Concert', start: `${wk(5)} 20:30`, end: `${wk(5)} 23:00`, calendar: 'perso', location: 'La Cigale' });
ev('week-end', { title: 'Week-end à la mer', start: wk(12), end: wk(13), calendar: 'perso' });
ev('partiels', { title: 'Partiels', start: wk(15), end: wk(19), calendar: 'cours' });
ev('anniv', { title: 'Anniversaire de Maman', start: day(9), calendar: 'perso', repeat: 'FREQ=YEARLY' });
ev('revisions', { title: 'Révisions', start: `${wk(6)} 10:00`, end: `${wk(6)} 12:30`, calendar: 'cours' });
ev('medecin', { title: 'Médecin', start: `${wk(8)} 09:15`, end: `${wk(8)} 09:45`, calendar: 'sante' });
ev('soiree', { title: 'Soirée jeux', start: `${wk(10)} 19:30`, end: `${wk(10)} 23:30`, calendar: 'perso' });
const task = (slug, f, body = '') => {
  const lines = Object.entries(f).map(([k, v]) => `${k}: ${Array.isArray(v) ? '[' + v.join(', ') + ']' : v}`);
  write(`tasks/${slug}.md`, `---\n${lines.join('\n')}\n---\n${body}`);
};
task('rapport', { title: 'Rendre le rapport de stage', status: 'doing', due: day(1), priority: 'high', tags: ['stage'] }, '- [x] plan\n- [ ] relecture\n');
task('dm', { title: 'DM de probabilités', status: 'todo', due: day(3), tags: ['cours'] });
task('loyer', { title: 'Payer le loyer', status: 'todo', due: day(-1), priority: 'high', repeat: 'FREQ=MONTHLY' });
task('courses', { title: 'Courses de la semaine', status: 'todo', due: day(0) });
task('velo', { title: 'Réparer le vélo', status: 'waiting' });
task('lecture', { title: 'Lire « Le Mythe de Sisyphe »', status: 'todo', priority: 'low' });
task('inscription', { title: 'Inscription au semestre', status: 'done', done_at: `${day(-2)} 10:00` });
task('mutuelle', { title: 'Envoyer le dossier mutuelle', status: 'todo', due: day(6) });
console.log('Dossier d’exemple :', dir);
