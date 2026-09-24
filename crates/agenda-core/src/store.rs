//! Le dossier agenda : chargement, index en mémoire, écritures sûres.
//!
//! Garanties :
//! - écriture atomique (fichier temporaire ignoré par Syncthing, puis renommage) ;
//! - suppression = déplacement dans `.trash/` ;
//! - fusion champ par champ avec les modifications externes, sans jamais les écraser ;
//! - chaque opération peut être annulée tant que le fichier n'a pas changé depuis.

use crate::date::{Date, DateTime, When, DAY};
use crate::json::Json;
use crate::model::{instant, normalize_priority, slugify, Calendar, Event, Task, STATUSES};
use crate::rrule::RRule;
use crate::tz::Tz;
use crate::yaml::Doc;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const AGENDA_MD: &str = include_str!("../../../docs/AGENDA.md");

pub fn hash(b: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &x in b {
        h ^= x as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

pub fn rev_of(s: &str) -> String {
    format!("{:016x}", hash(s.as_bytes()))
}

#[derive(Clone, Debug)]
pub enum Kind {
    Event(Box<Event>),
    Task(Box<Task>),
    Invalid(String),
}

#[derive(Clone, Debug)]
pub struct Item {
    pub id: String,
    pub raw: String,
    pub rev: String,
    pub mtime: Option<SystemTime>,
    pub len: u64,
    pub kind: Kind,
}

#[derive(Clone, Debug)]
pub struct Change {
    pub path: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Op {
    pub label: String,
    pub changes: Vec<Change>,
}

impl Op {
    pub fn new(label: &str) -> Op {
        Op { label: label.into(), changes: vec![] }
    }
}

/// Une occurrence calculée d'un événement.
#[derive(Clone, Debug)]
pub struct Occ<'a> {
    pub ev: &'a Event,
    pub start_utc: i64,
    pub end_utc: i64,
    /// heure murale d'affichage (fuseau de l'appareil)
    pub start: DateTime,
    pub end: DateTime,
    pub all_day: bool,
    /// clé de l'occurrence (heure murale de l'événement) pour `except`
    pub key: Option<String>,
}

pub struct Store {
    pub root: PathBuf,
    pub items: HashMap<String, Item>,
    pub calendars: BTreeMap<String, Calendar>,
    pub subs: HashMap<String, Vec<Event>>,
    pub sub_mtime: HashMap<String, SystemTime>,
    pub conflicts: Vec<String>,
    pub local: Tz,
    zones: HashMap<String, Option<Tz>>,
    pub version: u64,
    changelog: Vec<(u64, String)>,
    history: HashMap<String, Vec<String>>,
    undo: Vec<Op>,
    redo: Vec<Op>,
    /// Journal d'annulation persistant (propre à la machine, hors du dossier synchronisé).
    pub journal: Option<PathBuf>,
}

fn is_conflict_name(name: &str) -> bool {
    name.contains(".sync-conflict-") || name.contains(".agenda-conflict-")
}

fn skip_name(name: &str) -> bool {
    name.starts_with('.') || name.starts_with('~') || name.ends_with(".tmp") || name.ends_with('~')
}

/// Fichier d'origine d'un fichier de conflit.
pub fn conflict_original(rel: &str) -> Option<String> {
    let (dir, name) = rel.rsplit_once('/').map(|(d, n)| (format!("{d}/"), n)).unwrap_or((String::new(), rel));
    let i = name.find(".sync-conflict-").or_else(|| name.find(".agenda-conflict-"))?;
    let ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("md");
    Some(format!("{dir}{}.{ext}", &name[..i]))
}

/// Écriture atomique : `.syncthing.<nom>.tmp` (ignoré par Syncthing), fsync, renommage.
pub fn atomic_write(path: &Path, content: &[u8]) -> Result<(), String> {
    let dir = path.parent().ok_or("chemin sans dossier parent")?;
    fs::create_dir_all(dir).map_err(|e| format!("création de {} : {e}", dir.display()))?;
    let name = path.file_name().and_then(|n| n.to_str()).ok_or("nom de fichier invalide")?;
    let tmp = dir.join(format!(".syncthing.{name}.{}.tmp", std::process::id()));
    let res = (|| -> std::io::Result<()> {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(content)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)?;
        // rend le renommage durable
        if let Ok(d) = fs::File::open(dir) {
            let _ = d.sync_all();
        }
        Ok(())
    })();
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res.map_err(|e| format!("écriture de {} : {e}", path.display()))
}

fn walk(root: &Path, rel: &str, out: &mut Vec<String>, conflicts: &mut Vec<String>) {
    let Ok(rd) = fs::read_dir(root.join(rel)) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if skip_name(&name) {
            continue;
        }
        let r = format!("{rel}/{name}");
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            walk(root, &r, out, conflicts);
        } else if name.ends_with(".md") {
            if is_conflict_name(&name) {
                conflicts.push(r);
            } else {
                out.push(r);
            }
        }
    }
}

fn now_local_string(local: &Tz) -> String {
    local.to_local(crate::date::now_utc()).to_string()
}

fn ts_compact(local: &Tz) -> String {
    let dt = local.to_local(crate::date::now_utc());
    format!("{}-{:02}{:02}{:02}", dt.date.compact(), dt.hour(), dt.minute(), dt.sec % 60)
}

pub fn parse_item(id: &str, raw: String, mtime: Option<SystemTime>, len: u64) -> Item {
    let doc = Doc::parse(&raw);
    let kind = if !doc.had_front_matter {
        Kind::Invalid("pas d'en-tête YAML".into())
    } else if id.starts_with("events/") {
        match Event::from_doc(id, &doc) {
            Ok(e) => Kind::Event(Box::new(e)),
            Err(m) => Kind::Invalid(m),
        }
    } else {
        match Task::from_doc(id, &doc) {
            Ok(t) => Kind::Task(Box::new(t)),
            Err(m) => Kind::Invalid(m),
        }
    };
    Item { id: id.to_string(), rev: rev_of(&raw), raw, mtime, len, kind }
}

fn read_item(root: &Path, rel: &str) -> Option<Item> {
    let p = root.join(rel);
    let meta = fs::metadata(&p).ok()?;
    let raw = fs::read_to_string(&p).ok()?;
    Some(parse_item(rel, raw, meta.modified().ok(), meta.len()))
}

/// Vérifie qu'un identifiant désigne bien un fichier de l'agenda (pas d'évasion du dossier).
pub fn check_id(id: &str) -> Result<(), String> {
    let ok = (id.starts_with("events/") || id.starts_with("tasks/") || id.starts_with("calendars/"))
        && id.ends_with(".md")
        && !id.split('/').any(|c| c.is_empty() || c == ".." || c == "." || c.starts_with('.'))
        && !id.contains('\\')
        && !id.contains('\0');
    if ok {
        Ok(())
    } else {
        Err(format!("identifiant invalide : {id}"))
    }
}

impl Store {
    /// Ouvre un dossier agenda. `create` : crée la structure si besoin.
    pub fn open(root: &Path, create: bool, local: Tz) -> Result<Store, String> {
        if !root.exists() {
            if !create {
                return Err(format!("le dossier {} n'existe pas", root.display()));
            }
            fs::create_dir_all(root).map_err(|e| format!("création de {} : {e}", root.display()))?;
        }
        if !root.is_dir() {
            return Err(format!("{} n'est pas un dossier", root.display()));
        }
        let looks_like_agenda = ["events", "tasks", "calendars", "AGENDA.md"].iter().any(|d| root.join(d).exists());
        let empty = fs::read_dir(root).map(|mut r| r.next().is_none()).unwrap_or(false);
        if !looks_like_agenda && !empty && !create {
            return Err(format!(
                "{} ne ressemble pas à un dossier agenda (ni events/, ni tasks/, ni AGENDA.md). Utilisez « créer » pour l'initialiser.",
                root.display()
            ));
        }
        for d in ["events", "tasks", "calendars"] {
            let p = root.join(d);
            if !p.exists() {
                fs::create_dir_all(&p).map_err(|e| format!("création de {} : {e}", p.display()))?;
            }
        }
        if !root.join("AGENDA.md").exists() {
            atomic_write(&root.join("AGENDA.md"), AGENDA_MD.as_bytes())?;
        }
        let has_calendar = fs::read_dir(root.join("calendars"))
            .map(|r| r.flatten().any(|e| e.file_name().to_string_lossy().ends_with(".md")))
            .unwrap_or(false);
        if !has_calendar {
            atomic_write(
                &root.join("calendars/perso.md"),
                b"---\ntitle: Perso\ncolor: \"#6d5dfc\"\nalarm: [15m]\n---\n",
            )?;
        }
        let mut s = Store {
            root: root.to_path_buf(),
            items: HashMap::new(),
            calendars: BTreeMap::new(),
            subs: HashMap::new(),
            sub_mtime: HashMap::new(),
            conflicts: vec![],
            local,
            zones: HashMap::new(),
            version: 1,
            changelog: vec![],
            history: HashMap::new(),
            undo: vec![],
            redo: vec![],
            journal: None,
        };
        s.load_all();
        Ok(s)
    }

    fn load_all(&mut self) {
        let mut files = Vec::new();
        let mut conflicts = Vec::new();
        walk(&self.root, "events", &mut files, &mut conflicts);
        walk(&self.root, "tasks", &mut files, &mut conflicts);
        let root = self.root.clone();
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).clamp(1, 8);
        let chunk = files.len().div_ceil(threads).max(64);
        let items: Vec<Item> = std::thread::scope(|sc| {
            let handles: Vec<_> = files
                .chunks(chunk)
                .map(|c| {
                    let root = &root;
                    sc.spawn(move || c.iter().filter_map(|rel| read_item(root, rel)).collect::<Vec<_>>())
                })
                .collect();
            handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
        });
        self.items = items.into_iter().map(|i| (i.id.clone(), i)).collect();
        conflicts.sort();
        self.conflicts = conflicts;
        self.load_calendars();
        let ids: Vec<String> = self.items.keys().cloned().collect();
        for id in ids {
            self.resolve_zone_of(&id);
        }
    }

    fn resolve_zone_of(&mut self, id: &str) {
        if let Some(Item { kind: Kind::Event(e), .. }) = self.items.get(id) {
            if let Some(z) = e.tz.clone() {
                self.zones.entry(z.clone()).or_insert_with(|| Tz::load(&z));
            }
        }
    }

    pub fn load_calendars(&mut self) {
        self.calendars.clear();
        if let Ok(rd) = fs::read_dir(self.root.join("calendars")) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                if skip_name(&n) || is_conflict_name(&n) {
                    if is_conflict_name(&n) && !self.conflicts.contains(&format!("calendars/{n}")) {
                        self.conflicts.push(format!("calendars/{n}"));
                    }
                    continue;
                }
                if let Some(stem) = n.strip_suffix(".md") {
                    if let Ok(raw) = fs::read_to_string(e.path()) {
                        self.calendars.insert(stem.to_string(), Calendar::from_doc(stem, &Doc::parse(&raw)));
                    }
                }
            }
        }
        let names: Vec<String> = self.calendars.values().filter(|c| c.url.is_some()).map(|c| c.name.clone()).collect();
        self.subs.retain(|k, _| names.contains(k));
        for n in names {
            self.load_sub(&n);
        }
    }

    pub fn sub_path(&self, name: &str) -> PathBuf {
        self.root.join(".cache/subscriptions").join(format!("{name}.ics"))
    }

    pub fn load_sub(&mut self, name: &str) {
        let p = self.sub_path(name);
        let Ok(text) = fs::read_to_string(&p) else {
            self.subs.remove(name);
            return;
        };
        if let Ok(m) = fs::metadata(&p).and_then(|m| m.modified()) {
            self.sub_mtime.insert(name.to_string(), m);
        }
        let mut evs = Vec::new();
        for (i, j) in crate::ical::parse(&text).into_iter().enumerate() {
            if j.get("kind").as_str() != Some("event") {
                continue;
            }
            let mut doc = Doc::new();
            for (k, v) in j.as_obj() {
                if k != "kind" {
                    let _ = set_field(&mut doc, k, v);
                }
            }
            let id = format!("sub:{name}/{i}");
            if let Ok(mut e) = Event::from_doc(&id, &doc) {
                e.calendar = Some(name.to_string());
                e.readonly = true;
                if let Some(z) = e.tz.clone() {
                    self.zones.entry(z.clone()).or_insert_with(|| Tz::load(&z));
                }
                evs.push(e);
            }
        }
        self.subs.insert(name.to_string(), evs);
    }

    pub fn zone(&self, name: Option<&str>) -> &Tz {
        name.and_then(|n| self.zones.get(n)).and_then(|z| z.as_ref()).unwrap_or(&self.local)
    }

    pub fn set_local(&mut self, tz: Tz) {
        self.local = tz;
        self.bump("*");
    }

    // ------------------------------------------------------------ lecture

    pub fn events(&self) -> impl Iterator<Item = &Event> {
        self.items.values().filter_map(|i| match &i.kind {
            Kind::Event(e) => Some(e.as_ref()),
            _ => None,
        })
    }

    pub fn all_events(&self) -> impl Iterator<Item = &Event> {
        self.events().chain(self.subs.values().flatten())
    }

    pub fn tasks(&self) -> impl Iterator<Item = &Task> {
        self.items.values().filter_map(|i| match &i.kind {
            Kind::Task(t) => Some(t.as_ref()),
            _ => None,
        })
    }

    pub fn event(&self, id: &str) -> Option<&Event> {
        if let Some(rest) = id.strip_prefix("sub:") {
            let (name, _) = rest.split_once('/')?;
            return self.subs.get(name)?.iter().find(|e| e.id == id);
        }
        match &self.items.get(id)?.kind {
            Kind::Event(e) => Some(e),
            _ => None,
        }
    }

    pub fn task(&self, id: &str) -> Option<&Task> {
        match &self.items.get(id)?.kind {
            Kind::Task(t) => Some(t),
            _ => None,
        }
    }

    pub fn invalid(&self) -> Vec<(&str, &str)> {
        let mut v: Vec<(&str, &str)> = self
            .items
            .values()
            .filter_map(|i| match &i.kind {
                Kind::Invalid(m) => Some((i.id.as_str(), m.as_str())),
                _ => None,
            })
            .collect();
        v.sort();
        v
    }

    /// Occurrences dont l'intervalle croise `[from, to)` (heures murales locales).
    pub fn occurrences(&self, from: DateTime, to: DateTime) -> Vec<Occ<'_>> {
        let fu = self.local.to_utc(from);
        let tu = self.local.to_utc(to);
        let utc = Tz::utc();
        let mut out = Vec::new();
        let (fw, tw) = (from.secs() - 2 * DAY, to.secs() + 2 * DAY);
        for ev in self.all_events() {
            // préfiltre en heure murale (les décalages horaires restent sous 26 h) :
            // évite tout calcul de fuseau pour les événements ponctuels hors fenêtre
            if ev.repeat.is_none() {
                let ws = ev.wall_start().secs();
                let we = match ev.end {
                    Some(When::Local(e)) => e.secs(),
                    Some(When::Utc(t)) => t,
                    Some(When::Date(d)) => d.add_days(1).midnight().secs(),
                    None => ws + ev.duration.unwrap_or(DAY).max(DAY),
                };
                if ws > tw || we.max(ws) < fw {
                    continue;
                }
            }
            let zone = self.zone(ev.tz.as_deref());
            let wzone = if matches!(ev.start, When::Utc(_)) { &utc } else { zone };
            let dur = ev.duration_secs(zone);
            if ev.all_day() {
                let When::Date(sd) = ev.start else { continue };
                let ndays = (dur / DAY).max(1);
                let mut push = |d: Date, key: Option<String>| {
                    let last_excl = d.add_days(ndays);
                    if d < to.date.add_days(if to.sec > 0 { 1 } else { 0 }) && last_excl > from.date {
                        out.push(Occ {
                            ev,
                            start_utc: self.local.to_utc(d.midnight()),
                            end_utc: self.local.to_utc(last_excl.midnight()),
                            start: d.midnight(),
                            end: last_excl.midnight(),
                            all_day: true,
                            key,
                        });
                    }
                };
                match &ev.repeat {
                    None => push(sd, None),
                    Some(r) => {
                        let occs = r.between(
                            sd.midnight(),
                            from.date.add_days(-ndays).midnight(),
                            to.date.add_days(1).midnight(),
                            None,
                            5000,
                        );
                        for o in occs {
                            if ev.except.iter().any(|x| except_matches(*x, o, wzone)) {
                                continue;
                            }
                            push(o.date, Some(o.date.to_string()));
                        }
                    }
                }
                continue;
            }
            match &ev.repeat {
                None => {
                    let s = instant(ev.start, zone);
                    let e = s + dur;
                    if s < tu && (e > fu || (e == s && s >= fu)) {
                        out.push(Occ {
                            ev,
                            start_utc: s,
                            end_utc: e,
                            start: self.local.to_local(s),
                            end: self.local.to_local(e),
                            all_day: false,
                            key: None,
                        });
                    }
                }
                Some(r) => {
                    let wf = wzone.to_local(fu - dur - 1);
                    let wt = wzone.to_local(tu).add_secs(1);
                    let until = match r.until {
                        Some(When::Utc(t)) => Some(wzone.to_local(t)),
                        _ => None,
                    };
                    for o in r.between(ev.wall_start(), wf, wt, until, 5000) {
                        if ev.except.iter().any(|x| except_matches(*x, o, wzone)) {
                            continue;
                        }
                        let s = wzone.to_utc(o);
                        let e = s + dur;
                        if s < tu && (e > fu || (e == s && s >= fu)) {
                            out.push(Occ {
                                ev,
                                start_utc: s,
                                end_utc: e,
                                start: self.local.to_local(s),
                                end: self.local.to_local(e),
                                all_day: false,
                                key: Some(o.to_string()),
                            });
                        }
                    }
                }
            }
        }
        out.sort_by(|a, b| (!a.all_day, a.start_utc, &a.ev.title).cmp(&(!b.all_day, b.start_utc, &b.ev.title)));
        out
    }

    // ------------------------------------------------------------ écriture

    fn read_disk(&self, rel: &str) -> Option<String> {
        fs::read_to_string(self.root.join(rel)).ok()
    }

    fn bump(&mut self, id: &str) {
        self.version += 1;
        self.changelog.push((self.version, id.to_string()));
        if self.changelog.len() > 2000 {
            self.changelog.drain(..1000);
        }
    }

    /// Identifiants modifiés depuis `since` ; `None` si l'historique ne remonte pas assez loin.
    pub fn changes_since(&self, since: u64) -> Option<Vec<String>> {
        if since >= self.version {
            return Some(vec![]);
        }
        if self.changelog.first().map(|(v, _)| *v > since + 1).unwrap_or(true) && since + 1 < self.version {
            return None;
        }
        let mut ids: Vec<String> = self.changelog.iter().filter(|(v, _)| *v > since).map(|(_, i)| i.clone()).collect();
        ids.sort();
        ids.dedup();
        Some(ids)
    }

    fn remember(&mut self, id: &str, raw: &str) {
        let h = self.history.entry(id.to_string()).or_default();
        if h.last().map(|l| l != raw).unwrap_or(true) {
            h.push(raw.to_string());
            if h.len() > 8 {
                h.remove(0);
            }
        }
    }

    fn index(&mut self, rel: &str, content: &str) {
        if rel.starts_with("calendars/") {
            self.load_calendars();
        } else {
            let meta = fs::metadata(self.root.join(rel)).ok();
            let item = parse_item(
                rel,
                content.to_string(),
                meta.as_ref().and_then(|m| m.modified().ok()),
                meta.map(|m| m.len()).unwrap_or(0),
            );
            self.remember(rel, content);
            self.items.insert(rel.to_string(), item);
            self.resolve_zone_of(rel);
        }
        self.bump(rel);
    }

    /// Écrit un fichier et consigne le changement pour l'annulation.
    pub fn write(&mut self, rel: &str, content: &str, op: &mut Op) -> Result<(), String> {
        let before = self.read_disk(rel);
        if before.as_deref() == Some(content) {
            return Ok(());
        }
        atomic_write(&self.root.join(rel), content.as_bytes())?;
        op.changes.push(Change { path: rel.into(), before, after: Some(content.into()) });
        if is_conflict_name(rel) {
            if !self.conflicts.contains(&rel.to_string()) {
                self.conflicts.push(rel.into());
                self.conflicts.sort();
            }
            self.bump(rel);
        } else {
            self.index(rel, content);
        }
        Ok(())
    }

    /// Déplace un fichier dans `.trash/`.
    pub fn trash(&mut self, rel: &str, op: &mut Op) -> Result<(), String> {
        let src = self.root.join(rel);
        let before = self.read_disk(rel);
        if before.is_none() {
            return Err(format!("{rel} n'existe pas"));
        }
        let tdir = self.root.join(".trash");
        fs::create_dir_all(&tdir).map_err(|e| format!("corbeille : {e}"))?;
        let base = format!("{}__{}", ts_compact(&self.local), rel.replace('/', "__"));
        let mut dst = tdir.join(&base);
        let mut n = 2;
        while dst.exists() {
            dst = tdir.join(format!("{n}-{base}"));
            n += 1;
        }
        fs::rename(&src, &dst).map_err(|e| format!("déplacement vers la corbeille : {e}"))?;
        op.changes.push(Change { path: rel.into(), before, after: None });
        self.items.remove(rel);
        self.conflicts.retain(|c| c != rel);
        if rel.starts_with("calendars/") {
            self.load_calendars();
        }
        self.bump(rel);
        Ok(())
    }

    pub fn commit(&mut self, op: Op) {
        if !op.changes.is_empty() {
            self.load_journal();
            self.undo.push(op);
            if self.undo.len() > 100 {
                self.undo.remove(0);
            }
            self.redo.clear();
            self.save_journal();
        }
    }

    pub fn load_journal(&mut self) {
        let Some(p) = &self.journal else { return };
        let Ok(text) = fs::read_to_string(p) else { return };
        let Ok(j) = Json::parse(&text) else { return };
        if j.get("root").as_str() != Some(&*self.root.to_string_lossy()) {
            return;
        }
        let ops = |k: &str| -> Vec<Op> {
            j.get(k)
                .as_arr()
                .iter()
                .map(|o| Op {
                    label: o.get("label").str_or("").to_string(),
                    changes: o
                        .get("changes")
                        .as_arr()
                        .iter()
                        .map(|c| Change {
                            path: c.get("path").str_or("").to_string(),
                            before: c.get("before").as_str().map(str::to_string),
                            after: c.get("after").as_str().map(str::to_string),
                        })
                        .collect(),
                })
                .filter(|o: &Op| o.changes.iter().all(|c| check_id(&c.path).is_ok() || is_conflict_name(&c.path)))
                .collect()
        };
        self.undo = ops("undo");
        self.redo = ops("redo");
    }

    fn save_journal(&self) {
        let Some(p) = &self.journal else { return };
        let ser = |v: &[Op]| -> Json {
            // on ne journalise pas les très grosses opérations (import massif)
            let keep = v.iter().filter(|o| {
                o.changes
                    .iter()
                    .map(|c| c.before.as_ref().map_or(0, |b| b.len()) + c.after.as_ref().map_or(0, |a| a.len()))
                    .sum::<usize>()
                    < 4 << 20
            });
            Json::Arr(
                keep.map(|o| {
                    Json::obj().set("label", o.label.as_str()).set(
                        "changes",
                        Json::Arr(
                            o.changes
                                .iter()
                                .map(|c| {
                                    Json::obj()
                                        .set("path", c.path.as_str())
                                        .set("before", c.before.clone())
                                        .set("after", c.after.clone())
                                })
                                .collect(),
                        ),
                    )
                })
                .collect(),
            )
        };
        let j = Json::obj()
            .set("root", self.root.to_string_lossy().to_string())
            .set("undo", ser(&self.undo))
            .set("redo", ser(&self.redo));
        if let Some(d) = p.parent() {
            let _ = fs::create_dir_all(d);
        }
        let _ = atomic_write(p, j.to_string().as_bytes());
    }

    fn replay(&mut self, op: Op) -> Result<Op, (Op, String)> {
        for c in &op.changes {
            if self.read_disk(&c.path) != c.after {
                let m =
                    format!("« {} » a été modifié depuis : annulation impossible sans écraser ce changement", c.path);
                return Err((op, m));
            }
        }
        let mut inverse = Op::new(&op.label);
        for c in op.changes.iter().rev() {
            let r = match &c.before {
                Some(b) => self.write(&c.path, b, &mut inverse),
                None => self.trash(&c.path, &mut inverse),
            };
            if let Err(e) = r {
                return Err((op, e));
            }
        }
        inverse.changes.reverse();
        Ok(inverse)
    }

    pub fn undo(&mut self) -> Result<String, String> {
        self.load_journal();
        let op = self.undo.pop().ok_or("rien à annuler")?;
        let r = match self.replay(op) {
            Ok(inv) => {
                let l = inv.label.clone();
                self.redo.push(inv);
                Ok(l)
            }
            Err((op, m)) => {
                self.undo.push(op);
                Err(m)
            }
        };
        self.save_journal();
        r
    }

    pub fn redo(&mut self) -> Result<String, String> {
        self.load_journal();
        let op = self.redo.pop().ok_or("rien à rétablir")?;
        let r = match self.replay(op) {
            Ok(inv) => {
                let l = inv.label.clone();
                self.undo.push(inv);
                Ok(l)
            }
            Err((op, m)) => {
                self.redo.push(op);
                Err(m)
            }
        };
        self.save_journal();
        r
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|o| o.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|o| o.label.as_str())
    }

    /// Chemin libre pour un nouvel élément.
    pub fn new_path(&self, kind: &str, title: &str, date: Option<Date>) -> String {
        let slug = slugify(title);
        let dir = match kind {
            "task" => "tasks".to_string(),
            "calendar" => "calendars".to_string(),
            _ => {
                let d = date.unwrap_or_else(|| self.local.to_local(crate::date::now_utc()).date);
                format!("events/{:04}-{:02}", d.y, d.m)
            }
        };
        let mut n = 1;
        loop {
            let rel = if n == 1 { format!("{dir}/{slug}.md") } else { format!("{dir}/{slug}-{n}.md") };
            if !self.items.contains_key(&rel) && !self.root.join(&rel).exists() {
                return rel;
            }
            n += 1;
        }
    }

    /// Crée un élément à partir de champs JSON (clés du format de fichier).
    pub fn create(&mut self, kind: &str, fields: &Json, op: &mut Op) -> Result<String, String> {
        let mut doc = Doc::new();
        let order = [
            "title", "start", "end", "due", "status", "priority", "calendar", "location", "tags", "repeat", "except",
            "tz", "alarm",
        ];
        for k in order {
            if fields.has(k) && !fields.get(k).is_null() {
                set_field(&mut doc, k, fields.get(k))?;
            }
        }
        for (k, v) in fields.as_obj() {
            if !order.contains(&k.as_ref()) && !matches!(k.as_ref(), "kind" | "id" | "rev") && !v.is_null() {
                set_field(&mut doc, k, v)?;
            }
        }
        let title = doc.str("title").ok_or("un titre est nécessaire")?;
        let text = doc.to_text();
        let date = match kind {
            "event" => {
                let e = Event::from_doc("events/x.md", &doc)?;
                Some(match e.start {
                    When::Date(d) => d,
                    When::Local(dt) => dt.date,
                    When::Utc(t) => self.local.to_local(t).date,
                })
            }
            "task" => {
                Task::from_doc("tasks/x.md", &doc)?;
                None
            }
            "calendar" => None,
            _ => return Err(format!("type inconnu : {kind}")),
        };
        let rel = match (kind, fields.get("name").as_str()) {
            ("calendar", Some(n)) => format!("calendars/{}.md", slugify(n)),
            _ => self.new_path(kind, &title, date),
        };
        if kind == "calendar" && self.root.join(&rel).exists() {
            return Err(format!("le calendrier {rel} existe déjà"));
        }
        self.write(&rel, &text, op)?;
        Ok(rel)
    }

    /// Modifie des champs avec fusion à trois voies.
    /// Retourne la nouvelle révision et la liste des champs en conflit (version externe conservée).
    pub fn update(
        &mut self,
        id: &str,
        patch: &Json,
        base_rev: Option<&str>,
        op: &mut Op,
    ) -> Result<(String, Vec<String>), String> {
        check_id(id)?;
        let disk = self.read_disk(id).ok_or_else(|| format!("{id} n'existe plus (supprimé ou renommé entre-temps)"))?;
        let theirs = Doc::parse(&disk);
        let base_raw = match base_rev {
            Some(r) if r == rev_of(&disk) => disk.clone(),
            Some(r) => self
                .history
                .get(id)
                .and_then(|h| h.iter().rev().find(|x| rev_of(x) == r).cloned())
                .or_else(|| self.items.get(id).map(|i| i.raw.clone()))
                .unwrap_or_else(|| disk.clone()),
            None => disk.clone(),
        };
        let base = Doc::parse(&base_raw);
        let mut merged = theirs.clone();
        let mut ours_full = base.clone();
        let mut conflicts = Vec::new();
        for (k, v) in patch.as_obj() {
            if matches!(k.as_ref(), "id" | "rev" | "kind") {
                continue;
            }
            let mut probe = base.clone();
            set_field(&mut probe, k, v)?;
            set_field(&mut ours_full, k, v)?;
            let b = field_repr(&base, k);
            let t = field_repr(&theirs, k);
            let o = field_repr(&probe, k);
            if t != b && t != o {
                conflicts.push(k.to_string());
            } else {
                set_field(&mut merged, k, v)?;
            }
        }
        let text = merged.to_text();
        validate(id, &merged)?;
        self.write(id, &text, op)?;
        if !conflicts.is_empty() {
            let (dir, name) = id.rsplit_once('/').unwrap_or(("", id));
            let stem = name.strip_suffix(".md").unwrap_or(name);
            let cpath = format!("{dir}/{stem}.agenda-conflict-{}.md", ts_compact(&self.local));
            self.write(&cpath, &ours_full.to_text(), op)?;
        }
        Ok((rev_of(&text), conflicts))
    }

    /// Détache une occurrence d'une série : ajoute l'exception et crée un événement séparé.
    pub fn detach(&mut self, id: &str, key: &str, patch: &Json, op: &mut Op) -> Result<String, String> {
        let ev = self.event(id).ok_or_else(|| format!("{id} : événement introuvable"))?.clone();
        if ev.readonly {
            return Err("abonnement en lecture seule".into());
        }
        let occ = When::parse(key).ok_or("occurrence invalide")?;
        let zone = self.zone(ev.tz.as_deref()).clone();
        let dur = ev.duration_secs(&zone);
        let mut ex: Vec<String> = ev.except.iter().map(|w| w.to_file_string()).collect();
        ex.push(occ.to_file_string());
        self.update(id, &Json::obj().set("except", ex), None, op)?;
        let item = self.items.get(id).ok_or("élément disparu")?;
        let mut doc = Doc::parse(&item.raw);
        for k in ["repeat", "rrule", "except", "uid"] {
            doc.remove(k);
        }
        match occ {
            When::Date(d) => {
                doc.set_plain("start", &d.to_string());
                if dur > DAY {
                    doc.set_plain("end", &d.add_days(dur / DAY - 1).to_string());
                } else {
                    doc.remove("end");
                }
            }
            When::Local(dt) => {
                doc.set_plain("start", &dt.to_string());
                doc.set_plain("end", &dt.add_secs(dur).to_string());
            }
            When::Utc(t) => {
                doc.set_plain("start", &When::Utc(t).to_file_string());
                doc.set_plain("end", &When::Utc(t + dur).to_file_string());
            }
        }
        doc.remove("duration");
        for (k, v) in patch.as_obj() {
            set_field(&mut doc, k, v)?;
        }
        validate("events/x.md", &doc)?;
        let date = match When::parse(&doc.str("start").unwrap_or_default()) {
            Some(When::Date(d)) => Some(d),
            Some(When::Local(dt)) => Some(dt.date),
            _ => None,
        };
        let rel = self.new_path("event", &doc.str("title").unwrap_or_default(), date);
        self.write(&rel, &doc.to_text(), op)?;
        Ok(rel)
    }

    /// Termine (ou rouvre) une tâche. Une tâche répétée passe à l'échéance suivante.
    pub fn task_done(&mut self, id: &str, done: bool, op: &mut Op) -> Result<Json, String> {
        let t = self.task(id).ok_or_else(|| format!("{id} : tâche introuvable"))?.clone();
        let now = now_local_string(&self.local);
        let mut patch = Json::obj();
        if done {
            if let (Some(r), Some(due)) = (&t.repeat, t.due) {
                let wall = match due {
                    When::Date(d) => d.midnight(),
                    When::Local(dt) => dt,
                    When::Utc(u) => self.local.to_local(u),
                };
                let far = wall.add_secs(400 * 366 * DAY);
                if let Some(next) = r.between(wall, wall.add_secs(1), far, None, 1).first() {
                    let nd = match due {
                        When::Date(_) => next.date.to_string(),
                        _ => next.to_string(),
                    };
                    patch.insert("due", nd);
                    patch.insert("done_at", now);
                    patch.insert("status", "todo");
                    self.update(id, &patch, None, op)?;
                    return Ok(Json::obj().set("repeated", true));
                }
            }
            patch.insert("status", "done");
            patch.insert("done_at", now);
        } else {
            patch.insert("status", "todo");
            patch.insert("done_at", Json::Null);
        }
        self.update(id, &patch, None, op)?;
        Ok(Json::obj().set("repeated", false))
    }

    // ------------------------------------------------------------ conflits

    pub fn conflict_list(&self) -> Vec<Json> {
        let mut out = Vec::new();
        for c in &self.conflicts {
            let orig = conflict_original(c).unwrap_or_default();
            let a = self.read_disk(&orig);
            let b = self.read_disk(c).unwrap_or_default();
            let da = Doc::parse(a.as_deref().unwrap_or(""));
            let db = Doc::parse(&b);
            let mut keys: Vec<String> = da.keys().chain(db.keys()).map(str::to_string).collect();
            keys.sort();
            keys.dedup();
            keys.push("body".into());
            let fields: Vec<Json> = keys
                .iter()
                .filter(|k| field_repr(&da, k) != field_repr(&db, k))
                .map(|k| {
                    let val = |d: &Doc| {
                        if k == "body" {
                            Json::from(d.body.clone())
                        } else {
                            Json::from(d.str(k).or_else(|| d.raw(k).map(|r| r.trim().to_string())))
                        }
                    };
                    Json::obj().set("field", k.as_str()).set("original", val(&da)).set("conflict", val(&db))
                })
                .collect();
            out.push(
                Json::obj()
                    .set("path", c.as_str())
                    .set("original", orig.as_str())
                    .set("original_exists", a.is_some())
                    .set("title", da.str("title").or_else(|| db.str("title")))
                    .set("source", if c.contains(".sync-conflict-") { "syncthing" } else { "agenda" })
                    .set("fields", fields),
            );
        }
        out
    }

    /// `keep` : "original", "conflict", ou un objet {champ: "original"|"conflict"}.
    pub fn conflict_resolve(&mut self, path: &str, keep: &Json, op: &mut Op) -> Result<(), String> {
        if !self.conflicts.iter().any(|c| c == path) {
            return Err(format!("{path} n'est pas un conflit connu"));
        }
        let orig = conflict_original(path).ok_or("nom de conflit invalide")?;
        check_id(&orig)?;
        let theirs = self.read_disk(path).ok_or("fichier de conflit introuvable")?;
        match keep {
            Json::Str(s) if s == "original" => {}
            Json::Str(s) if s == "conflict" => {
                self.write(&orig, &theirs, op)?;
            }
            Json::Obj(fields) => {
                let mut doc = Doc::parse(&self.read_disk(&orig).unwrap_or_default());
                let other = Doc::parse(&theirs);
                for (k, v) in fields {
                    if v.as_str() == Some("conflict") {
                        if k == "body" {
                            doc.body = other.body.clone();
                        } else {
                            doc.set_raw(k, other.raw(k));
                        }
                    }
                }
                self.write(&orig, &doc.to_text(), op)?;
            }
            _ => return Err("keep : « original », « conflict » ou choix par champ".into()),
        }
        self.trash(path, op)?;
        Ok(())
    }

    // ------------------------------------------------------------ changements externes

    /// Relit les fichiers indiqués (chemins relatifs). Retourne les identifiants modifiés.
    pub fn refresh(&mut self, rels: &[String]) -> Vec<String> {
        let mut changed = Vec::new();
        let mut cal = false;
        for rel in rels {
            let name = rel.rsplit('/').next().unwrap_or(rel);
            if rel.starts_with(".cache/subscriptions/") {
                if let Some(n) = name.strip_suffix(".ics") {
                    self.load_sub(n);
                    changed.push(format!("sub:{n}"));
                }
                continue;
            }
            if rel.starts_with("calendars/") {
                cal = true;
                continue;
            }
            if !(rel.starts_with("events/") || rel.starts_with("tasks/")) || skip_name(name) {
                continue;
            }
            if !name.ends_with(".md") {
                // un dossier créé ou supprimé : on rescanne
                changed.extend(self.rescan());
                continue;
            }
            if is_conflict_name(name) {
                let exists = self.root.join(rel).exists();
                let known = self.conflicts.contains(rel);
                if exists && !known {
                    self.conflicts.push(rel.clone());
                    self.conflicts.sort();
                } else if !exists && known {
                    self.conflicts.retain(|c| c != rel);
                } else {
                    continue;
                }
                self.bump(rel);
                changed.push(rel.clone());
                continue;
            }
            match read_item(&self.root, rel) {
                Some(item) => {
                    if self.items.get(rel).map(|i| i.rev == item.rev).unwrap_or(false) {
                        if let Some(i) = self.items.get_mut(rel) {
                            i.mtime = item.mtime;
                        }
                        continue;
                    }
                    self.remember(rel, &item.raw);
                    self.items.insert(rel.clone(), item);
                    self.resolve_zone_of(rel);
                    self.bump(rel);
                    changed.push(rel.clone());
                }
                None => {
                    if self.items.remove(rel).is_some() {
                        self.bump(rel);
                        changed.push(rel.clone());
                    }
                }
            }
        }
        if cal {
            self.load_calendars();
            self.bump("calendars");
            changed.push("calendars".into());
        }
        changed
    }

    /// Compare le disque à l'index (taille et date de modification) et relit ce qui a changé.
    pub fn rescan(&mut self) -> Vec<String> {
        let mut files = Vec::new();
        let mut conflicts = Vec::new();
        walk(&self.root, "events", &mut files, &mut conflicts);
        walk(&self.root, "tasks", &mut files, &mut conflicts);
        let mut todo: Vec<String> = Vec::new();
        for f in &files {
            let meta = fs::metadata(self.root.join(f)).ok();
            let (mt, len) = (meta.as_ref().and_then(|m| m.modified().ok()), meta.map(|m| m.len()).unwrap_or(0));
            match self.items.get(f) {
                Some(i) if i.mtime == mt && i.len == len => {}
                _ => todo.push(f.clone()),
            }
        }
        let present: std::collections::HashSet<&String> = files.iter().collect();
        todo.extend(self.items.keys().filter(|k| !present.contains(k)).cloned());
        conflicts.sort();
        let cal_conf: Vec<String> = self.conflicts.iter().filter(|c| c.starts_with("calendars/")).cloned().collect();
        conflicts.extend(cal_conf);
        let mut changed = Vec::new();
        if conflicts != self.conflicts {
            self.conflicts = conflicts;
            self.bump("conflicts");
            changed.push("conflicts".into());
        }
        // calendriers et abonnements : peu nombreux, on compare les dates
        let cal_changed =
            fs::read_dir(self.root.join("calendars")).map(|r| r.count()).unwrap_or(0) != self.calendars.len()
                || self
                    .sub_mtime
                    .iter()
                    .any(|(n, m)| fs::metadata(self.sub_path(n)).and_then(|x| x.modified()).ok() != Some(*m))
                || self.calendars.values().any(|c| {
                    c.url.is_some() && !self.sub_mtime.contains_key(&c.name) && self.sub_path(&c.name).exists()
                });
        if cal_changed {
            self.load_calendars();
            self.bump("calendars");
            changed.push("calendars".into());
        }
        if !todo.is_empty() {
            changed.extend(self.refresh(&todo));
        }
        changed
    }
}

fn except_matches(x: When, occ: DateTime, wzone: &Tz) -> bool {
    match x {
        When::Date(d) => d == occ.date,
        When::Local(dt) => dt == occ,
        When::Utc(t) => wzone.to_utc(occ) == t,
    }
}

fn field_repr(d: &Doc, k: &str) -> Option<String> {
    if k == "body" {
        return Some(d.body.clone());
    }
    d.raw(k).map(|r| r.trim_end().to_string())
}

fn validate(id: &str, doc: &Doc) -> Result<(), String> {
    if id.starts_with("events/") {
        Event::from_doc(id, doc).map(|_| ())
    } else if id.starts_with("tasks/") {
        Task::from_doc(id, doc).map(|_| ())
    } else {
        Ok(())
    }
}

/// Écrit un champ dans un en-tête, avec validation et normalisation.
pub fn set_field(doc: &mut Doc, k: &str, v: &Json) -> Result<(), String> {
    if k.is_empty() || k.contains([':', '\n', '#']) || k.starts_with(['-', ' ']) {
        return Err(format!("nom de champ invalide : {k:?}"));
    }
    if v.is_null() {
        if k == "body" {
            doc.body.clear();
        } else {
            doc.remove(k);
        }
        return Ok(());
    }
    match k {
        "body" => {
            let mut b = v.as_str().unwrap_or_default().to_string();
            if !b.is_empty() && !b.ends_with('\n') {
                b.push('\n');
            }
            doc.body = b;
        }
        "start" | "end" | "due" | "done_at" => {
            let s = v.as_str().ok_or_else(|| format!("{k} : texte attendu"))?;
            if s.trim().is_empty() {
                doc.remove(k);
            } else {
                let w = When::parse(s).ok_or_else(|| format!("{k} : date illisible « {s} »"))?;
                doc.set_plain(k, &w.to_file_string());
            }
        }
        "repeat" => {
            let s = v.as_str().ok_or("repeat : texte attendu")?;
            if s.trim().is_empty() {
                doc.remove(k);
            } else {
                let r = RRule::parse(s)?;
                doc.set_str(k, &r.to_rule_string());
            }
        }
        "status" => {
            let s = v.as_str().ok_or("status : texte attendu")?.trim().to_lowercase();
            if !STATUSES.contains(&s.as_str()) && !matches!(s.as_str(), "confirmed" | "tentative") {
                return Err(format!("status inconnu : {s}"));
            }
            doc.set_str(k, &s);
        }
        "priority" => {
            let s = v.as_str().ok_or("priority : texte attendu")?;
            if s.trim().is_empty() {
                doc.remove(k);
            } else {
                doc.set_str(k, &normalize_priority(s));
            }
        }
        "duration" => {
            let s = match v {
                Json::Num(n) => format!("{}m", *n as i64),
                _ => v.as_str().unwrap_or_default().to_string(),
            };
            let d = crate::date::parse_duration(&s).ok_or("duration illisible")?;
            doc.set_plain(k, &crate::date::format_duration(d));
        }
        "alarm" => match v {
            Json::Str(s) if matches!(s.trim().to_lowercase().as_str(), "none" | "aucun" | "non") => {
                doc.set_plain(k, "none")
            }
            _ => {
                let items = v.str_list().ok_or("alarm : liste attendue")?;
                let mut out = Vec::new();
                for i in items {
                    let d = crate::date::parse_duration(&i).ok_or_else(|| format!("rappel illisible : {i}"))?;
                    out.push(crate::date::format_duration(d));
                }
                if out.is_empty() {
                    doc.set_plain(k, "none");
                } else {
                    doc.set_list(k, &out);
                }
            }
        },
        "except" => {
            let items = v.str_list().ok_or("except : liste attendue")?;
            let mut out = Vec::new();
            for i in items {
                out.push(When::parse(&i).ok_or_else(|| format!("exception illisible : {i}"))?.to_file_string());
            }
            out.sort();
            out.dedup();
            if out.is_empty() {
                doc.remove(k);
            } else {
                doc.set_list(k, &out);
            }
        }
        "tags" => {
            let mut items: Vec<String> = v
                .str_list()
                .ok_or("tags : liste attendue")?
                .into_iter()
                .map(|t| t.trim_start_matches('#').to_string())
                .filter(|t| !t.is_empty())
                .collect();
            items.dedup();
            if items.is_empty() {
                doc.remove(k);
            } else {
                doc.set_list(k, &items);
            }
        }
        _ => match v {
            Json::Str(s) => doc.set_str(k, s),
            Json::Bool(b) => doc.set_plain(k, if *b { "true" } else { "false" }),
            Json::Num(_) => doc.set_plain(k, &v.to_string()),
            Json::Arr(_) => doc.set_list(k, &v.str_list().unwrap_or_default()),
            _ => return Err(format!("{k} : valeur non gérée")),
        },
    }
    Ok(())
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn tmpdir(name: &str) -> PathBuf {
        let p =
            std::env::temp_dir().join(format!("agenda-test-{name}-{}-{}", std::process::id(), crate::date::now_utc()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn open(p: &Path) -> Store {
        Store::open(p, true, Tz::fixed("Europe/Paris", 7200)).unwrap()
    }

    #[test]
    fn creation_modification_annulation() {
        let dir = tmpdir("crud");
        let mut s = open(&dir);
        assert!(dir.join("AGENDA.md").exists());
        assert!(dir.join("calendars/perso.md").exists());
        let mut op = Op::new("créer");
        let id = s
            .create(
                "event",
                &Json::obj()
                    .set("title", "Dentiste")
                    .set("start", "2026-10-02 14:00")
                    .set("end", "2026-10-02 15:00")
                    .set("tags", vec!["santé"]),
                &mut op,
            )
            .unwrap();
        s.commit(op);
        assert_eq!(id, "events/2026-10/dentiste.md");
        let raw = fs::read_to_string(dir.join(&id)).unwrap();
        assert_eq!(raw, "---\ntitle: Dentiste\nstart: 2026-10-02 14:00\nend: 2026-10-02 15:00\ntags: [santé]\n---\n");
        // un champ inconnu ajouté à la main est conservé
        fs::write(dir.join(&id), raw.replace("tags:", "projet: thèse\ntags:")).unwrap();
        s.refresh(std::slice::from_ref(&id));
        let mut op = Op::new("déplacer");
        let (_, conf) = s
            .update(&id, &Json::obj().set("start", "2026-10-03 09:00").set("end", "2026-10-03 10:00"), None, &mut op)
            .unwrap();
        s.commit(op);
        assert!(conf.is_empty());
        let raw = fs::read_to_string(dir.join(&id)).unwrap();
        assert!(raw.contains("projet: thèse\n") && raw.contains("start: 2026-10-03 09:00\n"));
        assert_eq!(s.undo().unwrap(), "déplacer");
        assert!(fs::read_to_string(dir.join(&id)).unwrap().contains("start: 2026-10-02 14:00"));
        assert_eq!(s.redo().unwrap(), "déplacer");
        assert!(fs::read_to_string(dir.join(&id)).unwrap().contains("start: 2026-10-03 09:00"));
        // suppression = corbeille, et annulable
        let mut op = Op::new("supprimer");
        s.trash(&id, &mut op).unwrap();
        s.commit(op);
        assert!(!dir.join(&id).exists());
        assert_eq!(fs::read_dir(dir.join(".trash")).unwrap().count(), 1);
        s.undo().unwrap();
        assert!(dir.join(&id).exists());
        // annulation refusée si le fichier a changé entre-temps
        let mut op = Op::new("renommer");
        s.update(&id, &Json::obj().set("title", "Dentiste !"), None, &mut op).unwrap();
        s.commit(op);
        fs::write(dir.join(&id), "---\ntitle: Modifié ailleurs\nstart: 2026-10-03\n---\n").unwrap();
        assert!(s.undo().is_err());
        assert!(fs::read_to_string(dir.join(&id)).unwrap().contains("Modifié ailleurs"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn fusion_trois_voies() {
        let dir = tmpdir("merge");
        let mut s = open(&dir);
        let mut op = Op::new("c");
        let id = s.create("task", &Json::obj().set("title", "Rapport").set("due", "2026-10-02"), &mut op).unwrap();
        let rev0 = s.items[&id].rev.clone();
        // modification externe du titre (Hermes, Syncthing…)
        fs::write(dir.join(&id), "---\ntitle: Rapport final\ndue: 2026-10-02\n---\n").unwrap();
        // l'interface, qui voyait rev0, change l'échéance : pas de conflit, les deux sont gardés
        let (_, c) = s.update(&id, &Json::obj().set("due", "2026-10-05"), Some(&rev0), &mut op).unwrap();
        assert!(c.is_empty());
        let raw = fs::read_to_string(dir.join(&id)).unwrap();
        assert!(raw.contains("title: Rapport final") && raw.contains("due: 2026-10-05"));
        // même champ modifié des deux côtés : la version externe est conservée, conflit signalé
        let rev1 = rev_of(&raw);
        fs::write(dir.join(&id), raw.replace("Rapport final", "Rapport (Hermes)")).unwrap();
        let (_, c) = s.update(&id, &Json::obj().set("title", "Rapport (moi)"), Some(&rev1), &mut op).unwrap();
        assert_eq!(c, vec!["title"]);
        assert!(fs::read_to_string(dir.join(&id)).unwrap().contains("Rapport (Hermes)"));
        assert_eq!(s.conflicts.len(), 1);
        let list = s.conflict_list();
        assert_eq!(list[0].get("fields").as_arr()[0].get("field").as_str(), Some("title"));
        // résolution : on choisit « ma » version pour le titre
        let path = s.conflicts[0].clone();
        s.conflict_resolve(&path, &Json::obj().set("title", "conflict"), &mut op).unwrap();
        assert!(fs::read_to_string(dir.join(&id)).unwrap().contains("Rapport (moi)"));
        assert!(s.conflicts.is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn conflits_syncthing_et_rescan() {
        let dir = tmpdir("sync");
        let mut s = open(&dir);
        fs::create_dir_all(dir.join("tasks")).unwrap();
        fs::write(dir.join("tasks/a.md"), "---\ntitle: A\n---\n").unwrap();
        fs::write(dir.join("tasks/a.sync-conflict-20260924-101010-ABCDEFG.md"), "---\ntitle: A bis\n---\n").unwrap();
        let ch = s.rescan();
        assert!(ch.contains(&"tasks/a.md".to_string()));
        assert_eq!(s.conflicts, vec!["tasks/a.sync-conflict-20260924-101010-ABCDEFG.md"]);
        assert_eq!(conflict_original(&s.conflicts[0]).as_deref(), Some("tasks/a.md"));
        let mut op = Op::new("r");
        s.conflict_resolve("tasks/a.sync-conflict-20260924-101010-ABCDEFG.md", &Json::from("original"), &mut op)
            .unwrap();
        assert!(!dir.join("tasks/a.sync-conflict-20260924-101010-ABCDEFG.md").exists());
        assert!(s.rescan().is_empty());
        fs::remove_file(dir.join("tasks/a.md")).unwrap();
        assert_eq!(s.rescan(), vec!["tasks/a.md"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn occurrences_et_detachement() {
        let dir = tmpdir("occ");
        let mut s = open(&dir);
        let mut op = Op::new("c");
        let id = s
            .create(
                "event",
                &Json::obj()
                    .set("title", "Sport")
                    .set("start", "2026-09-22 18:30")
                    .set("end", "2026-09-22 20:00")
                    .set("repeat", "FREQ=WEEKLY;BYDAY=TU"),
                &mut op,
            )
            .unwrap();
        s.create(
            "event",
            &Json::obj().set("title", "Vacances").set("start", "2026-10-20").set("end", "2026-10-24"),
            &mut op,
        )
        .unwrap();
        let from = DateTime::parse("2026-10-01 00:00").unwrap();
        let to = DateTime::parse("2026-11-01 00:00").unwrap();
        let occ = s.occurrences(from, to);
        assert_eq!(occ.iter().filter(|o| o.ev.title == "Sport").count(), 4);
        let v = occ.iter().find(|o| o.ev.title == "Vacances").unwrap();
        assert!(v.all_day && v.end.date == Date::new(2026, 10, 25).unwrap());
        let new = s
            .detach(
                &id,
                "2026-10-13 18:30",
                &Json::obj().set("start", "2026-10-14 18:30").set("end", "2026-10-14 20:00"),
                &mut op,
            )
            .unwrap();
        let occ = s.occurrences(from, to);
        let sport: Vec<String> = occ.iter().filter(|o| o.ev.title == "Sport").map(|o| o.start.to_string()).collect();
        assert_eq!(sport, ["2026-10-06 18:30", "2026-10-14 18:30", "2026-10-20 18:30", "2026-10-27 18:30"]);
        assert!(s.event(&new).unwrap().repeat.is_none());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn taches_repetees() {
        let dir = tmpdir("tasks");
        let mut s = open(&dir);
        let mut op = Op::new("c");
        let id = s
            .create(
                "task",
                &Json::obj().set("title", "Loyer").set("due", "2026-10-05").set("repeat", "FREQ=MONTHLY"),
                &mut op,
            )
            .unwrap();
        s.task_done(&id, true, &mut op).unwrap();
        let t = s.task(&id).unwrap();
        assert_eq!(t.due, When::parse("2026-11-05"));
        assert_eq!(t.status, "todo");
        let id2 = s.create("task", &Json::obj().set("title", "Unique"), &mut op).unwrap();
        s.task_done(&id2, true, &mut op).unwrap();
        assert_eq!(s.task(&id2).unwrap().status, "done");
        s.task_done(&id2, false, &mut op).unwrap();
        assert!(s.task(&id2).unwrap().done_at.is_none());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn securite() {
        assert!(check_id("events/2026-10/a.md").is_ok());
        for bad in ["../x.md", "events/../../etc/passwd.md", "events/.trash/x.md", "/etc/x.md", "events/a.txt", "x.md"]
        {
            assert!(check_id(bad).is_err(), "{bad}");
        }
        let dir = tmpdir("refus");
        fs::write(dir.join("autre.txt"), "x").unwrap();
        assert!(Store::open(&dir, false, Tz::utc()).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
