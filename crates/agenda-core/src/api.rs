//! API unique : `Api::call(méthode, JSON) -> JSON`.
//!
//! Toutes les interfaces (application Tauri, CLI, serveur HTTP, serveur MCP) passent
//! par ici. La réponse est toujours `{"ok": true, "result": …}` ou
//! `{"ok": false, "error": "…"}`.

use crate::date::{format_duration, human_date, now_utc, Date, DateTime, When, DAY, WEEKDAYS_FR};
use crate::json::Json;
use crate::model::{Calendar, Event, Task};
use crate::search::Query;
use crate::store::{check_id, rev_of, Kind, Occ, Op, Store};
use crate::tz::Tz;
use crate::watch::Watcher;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Liste des méthodes, avec une description courte (servie par `help`).
pub const METHODS: &[(&str, &str)] = &[
    ("version", "version du cœur"),
    ("open", "{path, create?, watch?, journal?} ouvre (ou crée) un dossier agenda ; journal : fichier d'annulation persistant"),
    ("info", "état : dossier, nombre d'éléments, conflits, fichiers invalides, annulation"),
    ("set_timezone", "{tz} fuseau IANA de l'appareil (ex. Europe/Paris)"),
    ("list", "{from?, to?, calendars?, include_hidden?, tasks?} occurrences d'événements (et tâches à échéance) sur une période"),
    ("get", "{id} un élément complet (champs, corps, révision)"),
    ("parse", "{text} analyse une phrase en français sans rien écrire (aperçu)"),
    ("add", "{text, kind?, fields?} crée depuis une phrase en français ; fields complète ou remplace"),
    ("create", "{kind: event|task|calendar, fields} crée un élément"),
    ("update", "{id, fields, rev?} modifie des champs (null supprime) avec fusion à trois voies"),
    ("move", "{id, start, end?, occurrence?, scope?: one|all} déplace/redimensionne un événement"),
    ("delete", "{id, occurrence?} met à la corbeille (ou supprime une seule occurrence)"),
    ("task_done", "{id, done?} termine ou rouvre une tâche (les tâches répétées avancent)"),
    ("undo", "annule la dernière opération"),
    ("redo", "rétablit l'opération annulée"),
    ("search", "{q, limit?} recherche insensible aux accents"),
    ("tasks", "{status?, include_done?} liste des tâches"),
    ("calendars", "liste des calendriers"),
    ("calendar_save", "{name, fields} crée ou modifie un calendrier"),
    ("free", "{from?, to?, min?, day_start?, day_end?} créneaux libres"),
    ("brief", "{days?} résumé court de la situation (pour les agents)"),
    ("alarms", "{from?, to?} rappels à déclencher dans l'intervalle"),
    ("conflicts", "conflits à résoudre (Syncthing ou fusion)"),
    ("conflict_resolve", "{path, keep: original|conflict|{champ: original|conflict}}"),
    ("ics_export", "{calendars?, include_subscriptions?} flux iCalendar"),
    ("ics_import", "{text, calendar?} importe un fichier iCalendar"),
    ("sub_targets", "abonnements à télécharger {name, url}"),
    ("sub_store", "{name, text} enregistre le contenu téléchargé d'un abonnement"),
    ("trash", "éléments dans la corbeille"),
    ("trash_restore", "{name} restaure un élément de la corbeille"),
    ("changes", "{since, wait?} attend (ms) un changement ; renvoie la nouvelle version"),
    ("help", "cette liste"),
];

struct Inner {
    store: Option<Store>,
    watcher: Option<Arc<Watcher>>,
    tz_override: Option<Tz>,
}

pub struct Api {
    inner: Mutex<Inner>,
    poll: Duration,
}

impl Default for Api {
    fn default() -> Self {
        Api::new()
    }
}

fn ok(v: Json) -> String {
    Json::obj().set("ok", true).set("result", v).to_string()
}

fn err(m: &str) -> String {
    Json::obj().set("ok", false).set("error", m).to_string()
}

type R = Result<Json, String>;

fn s<'a>(p: &'a Json, k: &str) -> Result<&'a str, String> {
    p.get(k).as_str().filter(|x| !x.is_empty()).ok_or_else(|| format!("paramètre « {k} » manquant"))
}

/// `2026-10-02` ou `2026-10-02 14:00` → heure murale.
fn parse_dt(v: &str) -> Result<DateTime, String> {
    match When::parse(v) {
        Some(When::Date(d)) => Ok(d.midnight()),
        Some(When::Local(dt)) => Ok(dt),
        Some(When::Utc(t)) => Ok(DateTime::from_secs(t)),
        None => Err(format!("date illisible : {v}")),
    }
}

fn date_label(d: Date, today: Date) -> String {
    match d.days() - today.days() {
        0 => "aujourd'hui".into(),
        1 => "demain".into(),
        -1 => "hier".into(),
        _ => format!("{} {}", &WEEKDAYS_FR[d.weekday() as usize][..3], d.d),
    }
}

impl Api {
    pub fn new() -> Api {
        Api { inner: Mutex::new(Inner { store: None, watcher: None, tz_override: None }), poll: Duration::from_secs(3) }
    }

    /// Période de scrutation (secours quand inotify est indisponible).
    pub fn with_poll(mut self, d: Duration) -> Api {
        self.poll = d;
        self
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        }
    }

    /// Ouvre un dossier (raccourci pour les hôtes).
    pub fn open_dir(&self, path: &Path, create: bool) -> Result<(), String> {
        self.call_json("open", &Json::obj().set("path", path.to_string_lossy().to_string()).set("create", create))
            .map(|_| ())
    }

    pub fn call(&self, method: &str, params: &str) -> String {
        let p = match Json::parse(params) {
            Ok(Json::Null) => Json::obj(),
            Ok(p) => p,
            Err(e) => return err(&e),
        };
        match self.call_json(method, &p) {
            Ok(v) => ok(v),
            Err(e) => err(&e),
        }
    }

    pub fn call_json(&self, method: &str, p: &Json) -> R {
        match method {
            "version" => return Ok(Json::from(crate::VERSION)),
            "help" => {
                return Ok(Json::Arr(
                    METHODS.iter().map(|(n, d)| Json::obj().set("method", *n).set("description", *d)).collect(),
                ));
            }
            "open" => return self.open(p),
            "changes" => return self.changes(p),
            "set_timezone" => {
                let name = s(p, "tz")?;
                let tz = Tz::load(name).ok_or_else(|| format!("fuseau inconnu : {name}"))?;
                let mut g = self.lock();
                g.tz_override = Some(tz.clone());
                if let Some(st) = g.store.as_mut() {
                    st.set_local(tz);
                }
                return Ok(Json::from(name));
            }
            _ => {}
        }
        let mut g = self.lock();
        let inner = &mut *g;
        // applique les changements externes en attente avant de répondre
        if let (Some(w), Some(st)) = (&inner.watcher, inner.store.as_mut()) {
            let (paths, full) = w.take();
            if full {
                st.rescan();
            } else if !paths.is_empty() {
                st.refresh(&paths);
            }
        }
        let st = inner.store.as_mut().ok_or("aucun dossier agenda ouvert (méthode open)")?;
        let res = dispatch(st, method, p);
        if let Some(w) = &inner.watcher {
            w.poke();
        }
        res
    }

    fn open(&self, p: &Json) -> R {
        let path = PathBuf::from(s(p, "path")?);
        let path = if let Some(rest) = path.to_str().and_then(|x| x.strip_prefix("~/")) {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join(rest)).unwrap_or(path)
        } else {
            path
        };
        let create = p.get("create").as_bool().unwrap_or(false);
        let tz = self.lock().tz_override.clone().unwrap_or_else(Tz::local);
        let started = Instant::now();
        let mut store = Store::open(&path, create, tz)?;
        store.journal = p.get("journal").as_str().map(PathBuf::from);
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        let watcher = if p.get("watch").as_bool().unwrap_or(true) {
            Some(Arc::new(Watcher::start(&path, self.poll)))
        } else {
            None
        };
        let mut g = self.lock();
        g.store = Some(store);
        g.watcher = watcher;
        let st = g.store.as_ref().ok_or("ouverture impossible")?;
        Ok(info(st).set("open_ms", (ms * 10.0).round() / 10.0))
    }

    fn changes(&self, p: &Json) -> R {
        let since = p.get("since").as_i64().unwrap_or(0).max(0) as u64;
        let wait = Duration::from_millis(p.get("wait").as_i64().unwrap_or(0).clamp(0, 60_000) as u64);
        let deadline = Instant::now() + wait;
        loop {
            let watcher = {
                let mut g = self.lock();
                let inner = &mut *g;
                let st = inner.store.as_mut().ok_or("aucun dossier agenda ouvert")?;
                if let Some(w) = &inner.watcher {
                    let (paths, full) = w.take();
                    if full {
                        st.rescan();
                    } else if !paths.is_empty() {
                        st.refresh(&paths);
                    }
                }
                if st.version > since || Instant::now() >= deadline {
                    let ids = st.changes_since(since);
                    return Ok(Json::obj()
                        .set("version", st.version as i64)
                        .set("full", ids.is_none())
                        .set("ids", ids.unwrap_or_default())
                        .set("conflicts", st.conflicts.len()));
                }
                inner.watcher.clone()
            };
            let left = deadline.saturating_duration_since(Instant::now());
            match watcher {
                Some(w) => {
                    w.wait(left.min(Duration::from_secs(5)));
                }
                None => std::thread::sleep(left.min(Duration::from_millis(500))),
            }
        }
    }
}

fn info(st: &Store) -> Json {
    let invalid: Vec<Json> = st.invalid().iter().map(|(id, e)| Json::obj().set("id", *id).set("error", *e)).collect();
    Json::obj()
        .set("path", st.root.to_string_lossy().to_string())
        .set("version", st.version as i64)
        .set("events", st.events().count())
        .set("tasks", st.tasks().count())
        .set("subscriptions", st.subs.values().map(|v| v.len()).sum::<usize>())
        .set("calendars", st.calendars.len())
        .set("conflicts", st.conflicts.len())
        .set("invalid", invalid)
        .set("timezone", st.local.name.as_str())
        .set("undo", st.undo_label().map(str::to_string))
        .set("redo", st.redo_label().map(str::to_string))
}

fn now_local(st: &Store) -> DateTime {
    st.local.to_local(now_utc())
}

fn calendar_of<'a>(st: &'a Store, name: Option<&str>) -> Option<&'a Calendar> {
    name.and_then(|n| st.calendars.get(n))
}

fn occ_json(st: &Store, o: &Occ) -> Json {
    let e = o.ev;
    let cal = calendar_of(st, e.calendar.as_deref());
    let fmt = |dt: DateTime| if o.all_day { dt.date.to_string() } else { dt.to_string() };
    let mut j = Json::obj()
        .set("id", e.id.as_str())
        .set("title", e.title.as_str())
        .set("start", fmt(o.start))
        .set("end", fmt(o.end))
        .set("all_day", o.all_day)
        .set("calendar", e.calendar.clone())
        .set(
            "color",
            cal.map(|c| c.color.clone())
                .unwrap_or_else(|| crate::model::default_color(e.calendar.as_deref().unwrap_or("")).to_string()),
        )
        .set("location", e.location.clone())
        .set("tags", e.tags.clone())
        .set("recurring", e.repeat.is_some())
        .set("readonly", e.readonly || cal.map(|c| c.url.is_some()).unwrap_or(false))
        .set("status", e.status.as_str());
    if let Some(k) = &o.key {
        j.insert("occurrence", k.as_str());
    }
    j
}

fn visible(st: &Store, cal: Option<&str>, filter: &Option<Vec<String>>, include_hidden: bool) -> bool {
    if let Some(f) = filter {
        return f.iter().any(|c| Some(c.as_str()) == cal || (c.is_empty() && cal.is_none()));
    }
    include_hidden || !calendar_of(st, cal).map(|c| c.hidden).unwrap_or(false)
}

fn range(st: &Store, p: &Json, default_days: i64) -> Result<(DateTime, DateTime), String> {
    let today = now_local(st).date;
    let from = match p.get("from").as_str() {
        Some(f) => parse_dt(f)?,
        None => today.midnight(),
    };
    let to = match p.get("to").as_str() {
        Some(t) => {
            let dt = parse_dt(t)?;
            // une date seule en borne de fin est incluse
            if t.trim().len() == 10 {
                dt.date.add_days(1).midnight()
            } else {
                dt
            }
        }
        None => from.date.add_days(p.get("days").as_i64().unwrap_or(default_days)).midnight(),
    };
    if to <= from {
        return Err("« to » doit être après « from »".into());
    }
    if to.date.days() - from.date.days() > 3700 {
        return Err("intervalle trop long (10 ans maximum)".into());
    }
    Ok((from, to))
}

fn item_json(st: &Store, id: &str) -> R {
    if let Some(e) = st.event(id).filter(|e| e.readonly) {
        return Ok(event_json(st, e).set("rev", Json::Null).set("readonly", true));
    }
    let it = st.items.get(id).ok_or_else(|| format!("{id} : introuvable"))?;
    let doc = crate::yaml::Doc::parse(&it.raw);
    let mut extra = Json::obj();
    let known = [
        "title", "start", "end", "calendar", "location", "tags", "repeat", "except", "tz", "alarm", "status", "due",
        "priority", "done_at", "duration", "uid",
    ];
    for k in doc.keys() {
        if !known.contains(&k) {
            extra.insert(
                k,
                doc.str(k).map(Json::from).unwrap_or_else(|| Json::from(doc.raw(k).unwrap_or("").trim().to_string())),
            );
        }
    }
    let base = match &it.kind {
        Kind::Event(e) => event_json(st, e),
        Kind::Task(t) => t.to_json(&st.local),
        Kind::Invalid(m) => Json::obj().set("id", id).set("kind", "invalid").set("error", m.as_str()),
    };
    Ok(base.set("rev", it.rev.as_str()).set("raw", it.raw.as_str()).set("extra", extra).set("readonly", false))
}

fn event_json(st: &Store, e: &Event) -> Json {
    let cal = calendar_of(st, e.calendar.as_deref());
    Json::obj()
        .set("id", e.id.as_str())
        .set("kind", "event")
        .set("title", e.title.as_str())
        .set("start", e.start.to_file_string())
        .set("end", e.end.map(|w| w.to_file_string()))
        .set("duration", e.duration.map(format_duration))
        .set("all_day", e.all_day())
        .set("calendar", e.calendar.clone())
        .set("color", cal.map(|c| c.color.clone()))
        .set("location", e.location.clone())
        .set("tags", e.tags.clone())
        .set("repeat", e.repeat.as_ref().map(|r| r.to_rule_string()))
        .set("repeat_text", e.repeat.as_ref().map(|r| r.describe()))
        .set("except", e.except.iter().map(|w| w.to_file_string()).collect::<Vec<_>>())
        .set("tz", e.tz.clone())
        .set(
            "alarm",
            match &e.alarm {
                None => Json::Null,
                Some(a) if a.is_empty() => Json::from("none"),
                Some(a) => Json::from(a.iter().map(|x| format_duration(*x)).collect::<Vec<_>>()),
            },
        )
        .set("status", e.status.as_str())
        .set("body", e.body.as_str())
}

fn done(st: &mut Store, op: Op) -> Json {
    let label = op.label.clone();
    st.commit(op);
    Json::obj().set("undo", label).set("version", st.version as i64)
}

fn dispatch(st: &mut Store, method: &str, p: &Json) -> R {
    match method {
        "info" => {
            st.load_journal();
            Ok(info(st))
        }
        "list" => {
            let (from, to) = range(st, p, 7)?;
            let filter = p.get("calendars").str_list();
            let hidden = p.get("include_hidden").as_bool().unwrap_or(false);
            let events: Vec<Json> = st
                .occurrences(from, to)
                .iter()
                .filter(|o| visible(st, o.ev.calendar.as_deref(), &filter, hidden))
                .take(p.get("limit").as_i64().unwrap_or(5000) as usize)
                .map(|o| occ_json(st, o))
                .collect();
            let mut out = Json::obj().set("from", from.to_string()).set("to", to.to_string()).set("events", events);
            if p.get("tasks").as_bool().unwrap_or(false) {
                let tasks: Vec<Json> = st
                    .tasks()
                    .filter(|t| {
                        t.due_date()
                            .map(|d| d >= from.date && d < to.date.add_days(if to.sec > 0 { 1 } else { 0 }))
                            .unwrap_or(false)
                    })
                    .filter(|t| visible(st, t.calendar.as_deref(), &filter, hidden))
                    .map(|t| t.to_json(&st.local))
                    .collect();
                out.insert("tasks", tasks);
            }
            Ok(out)
        }
        "get" => item_json(st, s(p, "id")?),
        "parse" => Ok(crate::nlp::parse(s(p, "text")?, now_local(st))),
        "add" => {
            let parsed = crate::nlp::parse(s(p, "text")?, now_local(st));
            let kind = p.get("kind").as_str().unwrap_or(parsed.get("kind").as_str().unwrap_or("task")).to_string();
            let mut fields = parsed.get("fields").clone();
            // passage événement ↔ tâche demandé par l'aperçu
            if kind == "task" && fields.has("start") {
                let st_ = fields.get("start").as_str().unwrap_or("").to_string();
                fields = Json::Obj(
                    fields
                        .as_obj()
                        .iter()
                        .filter(|(k, _)| k != "start" && k != "end" && k != "repeat")
                        .cloned()
                        .collect(),
                );
                if !st_.is_empty() {
                    fields.insert("due", st_);
                }
            } else if kind == "event" && !fields.has("start") {
                let d =
                    fields.get("due").as_str().map(str::to_string).unwrap_or_else(|| now_local(st).date.to_string());
                fields =
                    Json::Obj(fields.as_obj().iter().filter(|(k, _)| k != "due" && k != "priority").cloned().collect());
                fields.insert("start", d);
            }
            for (k, v) in p.get("fields").as_obj() {
                fields.insert(k, v.clone());
            }
            let mut op = Op::new(&format!("ajouter « {} »", fields.get("title").as_str().unwrap_or("")));
            let id = st.create(&kind, &fields, &mut op)?;
            Ok(done(st, op).set("id", id.as_str()).set("kind", kind).set("summary", parsed.get("summary").clone()))
        }
        "create" => {
            let kind = s(p, "kind")?;
            let fields = p.get("fields");
            let mut op = Op::new(&format!("créer « {} »", fields.get("title").as_str().unwrap_or("")));
            let id = st.create(kind, fields, &mut op)?;
            let rev = st.items.get(&id).map(|i| i.rev.clone());
            Ok(done(st, op).set("id", id.as_str()).set("rev", rev))
        }
        "update" => {
            let id = s(p, "id")?;
            if id.starts_with("sub:") {
                return Err("abonnement en lecture seule".into());
            }
            check_id(id)?;
            let title = st.items.get(id).map(|i| match &i.kind {
                Kind::Event(e) => e.title.clone(),
                Kind::Task(t) => t.title.clone(),
                _ => id.to_string(),
            });
            let mut op = Op::new(&format!("modifier « {} »", title.unwrap_or_default()));
            let (rev, conflicts) = st.update(id, p.get("fields"), p.get("rev").as_str(), &mut op)?;
            Ok(done(st, op).set("id", id).set("rev", rev).set("conflicts", conflicts))
        }
        "move" => move_event(st, p),
        "delete" => {
            let id = s(p, "id")?;
            if id.starts_with("sub:") {
                return Err("abonnement en lecture seule".into());
            }
            check_id(id)?;
            if let Some(occ) = p.get("occurrence").as_str() {
                let ev = st.event(id).ok_or("événement introuvable")?.clone();
                let mut ex: Vec<String> = ev.except.iter().map(|w| w.to_file_string()).collect();
                ex.push(occ.to_string());
                let mut op = Op::new(&format!("supprimer une occurrence de « {} »", ev.title));
                st.update(id, &Json::obj().set("except", ex), None, &mut op)?;
                return Ok(done(st, op));
            }
            let title = st.items.get(id).map(|i| match &i.kind {
                Kind::Event(e) => e.title.clone(),
                Kind::Task(t) => t.title.clone(),
                _ => id.to_string(),
            });
            let mut op = Op::new(&format!("supprimer « {} »", title.unwrap_or_default()));
            st.trash(id, &mut op)?;
            Ok(done(st, op))
        }
        "task_done" => {
            let id = s(p, "id")?;
            let d = p.get("done").as_bool().unwrap_or(true);
            let title = st.task(id).map(|t| t.title.clone()).unwrap_or_default();
            let mut op = Op::new(&format!("{} « {title} »", if d { "terminer" } else { "rouvrir" }));
            let r = st.task_done(id, d, &mut op)?;
            Ok(done(st, op).set("repeated", r.get("repeated").clone()))
        }
        "undo" => {
            let l = st.undo()?;
            Ok(Json::obj().set("undone", l).set("version", st.version as i64))
        }
        "redo" => {
            let l = st.redo()?;
            Ok(Json::obj().set("redone", l).set("version", st.version as i64))
        }
        "search" => {
            let q = Query::new(s(p, "q")?);
            let limit = p.get("limit").as_i64().unwrap_or(50) as usize;
            let today = now_local(st).date;
            let mut hits: Vec<(u32, i64, Json)> = Vec::new();
            for e in st.all_events() {
                let sc = q.score(&e.title, &e.search_text());
                if sc > 0 {
                    let d = match e.start {
                        When::Date(d) => d,
                        When::Local(dt) => dt.date,
                        When::Utc(t) => st.local.to_local(t).date,
                    };
                    let dist = (d.days() - today.days()).abs();
                    hits.push((
                        sc,
                        dist,
                        Json::obj()
                            .set("id", e.id.as_str())
                            .set("kind", "event")
                            .set("title", e.title.as_str())
                            .set("when", e.start.to_file_string())
                            .set("recurring", e.repeat.is_some())
                            .set("calendar", e.calendar.clone()),
                    ));
                }
            }
            for t in st.tasks() {
                let sc = q.score(&t.title, &t.search_text());
                if sc > 0 {
                    let dist = t.due_date().map(|d| (d.days() - today.days()).abs()).unwrap_or(9999)
                        + if t.is_open() { 0 } else { 5000 };
                    hits.push((
                        sc,
                        dist,
                        Json::obj()
                            .set("id", t.id.as_str())
                            .set("kind", "task")
                            .set("title", t.title.as_str())
                            .set("when", t.due.map(|w| w.to_file_string()))
                            .set("status", t.status.as_str()),
                    ));
                }
            }
            hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            Ok(Json::Arr(hits.into_iter().take(limit).map(|h| h.2).collect()))
        }
        "tasks" => {
            let incl = p.get("include_done").as_bool().unwrap_or(false);
            let status = p.get("status").as_str();
            let mut v: Vec<&Task> = st
                .tasks()
                .filter(|t| match status {
                    Some(s) => t.status == s,
                    None => incl || t.is_open(),
                })
                .collect();
            v.sort_by_key(|a| task_order(a));
            Ok(Json::Arr(v.iter().map(|t| t.to_json(&st.local)).collect()))
        }
        "calendars" => {
            let mut v: Vec<Json> = st
                .calendars
                .values()
                .map(|c| {
                    let n = st.events().filter(|e| e.calendar.as_deref() == Some(&c.name)).count()
                        + st.subs.get(&c.name).map(|x| x.len()).unwrap_or(0);
                    c.to_json().set("count", n)
                })
                .collect();
            // calendriers cités par des événements mais sans fichier
            let mut extra: Vec<String> =
                st.events().filter_map(|e| e.calendar.clone()).filter(|c| !st.calendars.contains_key(c)).collect();
            extra.sort();
            extra.dedup();
            for c in extra {
                v.push(Calendar::default_named(&c).to_json().set("missing_file", true));
            }
            Ok(Json::Arr(v))
        }
        "calendar_save" => {
            let name = crate::model::slugify(s(p, "name")?);
            let rel = format!("calendars/{name}.md");
            let mut op = Op::new(&format!("calendrier « {name} »"));
            if st.calendars.contains_key(&name) || st.root.join(&rel).exists() {
                st.update(&rel, p.get("fields"), None, &mut op)?;
            } else {
                let mut f = p.get("fields").clone();
                if !f.has("title") {
                    f.insert("title", name.as_str());
                }
                f.insert("name", name.as_str());
                st.create("calendar", &f, &mut op)?;
            }
            Ok(done(st, op).set("name", name))
        }
        "free" => free(st, p),
        "brief" => brief(st, p),
        "alarms" => alarms(st, p),
        "conflicts" => Ok(Json::Arr(st.conflict_list())),
        "conflict_resolve" => {
            let path = s(p, "path")?;
            let mut op = Op::new("résoudre un conflit");
            st.conflict_resolve(path, p.get("keep"), &mut op)?;
            Ok(done(st, op))
        }
        "ics_export" => {
            let filter = p.get("calendars").str_list();
            let subs = p.get("include_subscriptions").as_bool().unwrap_or(false);
            let evs: Vec<&Event> = (if subs {
                Box::new(st.all_events()) as Box<dyn Iterator<Item = &Event>>
            } else {
                Box::new(st.events())
            })
            .filter(|e| visible(st, e.calendar.as_deref(), &filter, true))
            .collect();
            let tasks: Vec<&Task> = if p.get("tasks").as_bool().unwrap_or(true) {
                st.tasks().filter(|t| visible(st, t.calendar.as_deref(), &filter, true)).collect()
            } else {
                vec![]
            };
            Ok(Json::from(crate::ical::export(
                &evs,
                &tasks,
                &st.local,
                p.get("name").as_str().unwrap_or("Agenda"),
                now_utc(),
            )))
        }
        "ics_import" => {
            let items = crate::ical::parse(s(p, "text")?);
            let cal = p.get("calendar").as_str().map(str::to_string);
            let uids: std::collections::HashSet<String> = st.events().filter_map(|e| e.uid.clone()).collect();
            let mut op = Op::new("importer un fichier iCalendar");
            let (mut created, mut skipped, mut errors) = (0, 0, Vec::new());
            for mut it in items {
                if it.get("uid").as_str().map(|u| uids.contains(u)).unwrap_or(false) {
                    skipped += 1;
                    continue;
                }
                let kind = it.get("kind").as_str().unwrap_or("event").to_string();
                if let Some(c) = &cal {
                    it.insert("calendar", c.as_str());
                }
                match st.create(&kind, &it, &mut op) {
                    Ok(_) => created += 1,
                    Err(e) => errors.push(Json::from(format!("{} : {e}", it.get("title").as_str().unwrap_or("?")))),
                }
            }
            Ok(done(st, op).set("created", created).set("skipped", skipped).set("errors", errors))
        }
        "sub_targets" => Ok(Json::Arr(
            st.calendars
                .values()
                .filter_map(|c| c.url.as_ref().map(|u| (c, u)))
                .map(|(c, u)| {
                    let url =
                        if let Some(r) = u.strip_prefix("webcal://") { format!("https://{r}") } else { u.clone() };
                    let age = st.sub_mtime.get(&c.name).and_then(|m| m.elapsed().ok()).map(|d| d.as_secs() as i64);
                    Json::obj()
                        .set("name", c.name.as_str())
                        .set("url", url)
                        .set("age", age)
                        .set("events", st.subs.get(&c.name).map(|v| v.len()).unwrap_or(0))
                })
                .collect(),
        )),
        "sub_store" => {
            let name = s(p, "name")?;
            let text = s(p, "text")?;
            if !st.calendars.get(name).map(|c| c.url.is_some()).unwrap_or(false) {
                return Err(format!("{name} n'est pas un abonnement"));
            }
            if !text.contains("BEGIN:VCALENDAR") {
                return Err("le contenu reçu n'est pas un calendrier iCalendar".into());
            }
            crate::store::atomic_write(&st.sub_path(name), text.as_bytes())?;
            st.load_sub(name);
            let n = st.subs.get(name).map(|v| v.len()).unwrap_or(0);
            st.refresh(&[format!(".cache/subscriptions/{name}.ics")]);
            Ok(Json::obj().set("name", name).set("events", n))
        }
        "trash" => {
            let mut v = Vec::new();
            if let Ok(rd) = std::fs::read_dir(st.root.join(".trash")) {
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().to_string();
                    let raw = std::fs::read_to_string(e.path()).unwrap_or_default();
                    let doc = crate::yaml::Doc::parse(&raw);
                    v.push(
                        Json::obj()
                            .set("name", name.as_str())
                            .set("original", trash_original(&name))
                            .set("title", doc.str("title")),
                    );
                }
            }
            v.sort_by(|a, b| b.get("name").as_str().cmp(&a.get("name").as_str()));
            Ok(Json::Arr(v))
        }
        "trash_restore" => {
            let name = s(p, "name")?;
            if name.contains('/') || name.contains("..") {
                return Err("nom invalide".into());
            }
            let src = st.root.join(".trash").join(name);
            let raw = std::fs::read_to_string(&src).map_err(|e| format!("{name} : {e}"))?;
            let mut orig = trash_original(name).ok_or("nom de corbeille inattendu")?;
            check_id(&orig)?;
            if st.root.join(&orig).exists() {
                orig = orig.replace(".md", "-restaure.md");
            }
            let mut op = Op::new("restaurer depuis la corbeille");
            st.write(&orig, &raw, &mut op)?;
            std::fs::remove_file(&src).map_err(|e| e.to_string())?;
            Ok(done(st, op).set("id", orig))
        }
        _ => Err(format!("méthode inconnue : {method} (voir « help »)")),
    }
}

fn trash_original(name: &str) -> Option<String> {
    let (_, rest) = name.split_once("__")?;
    Some(rest.replace("__", "/"))
}

fn task_order(t: &Task) -> (u8, i64, u8, String) {
    let st = match t.status.as_str() {
        "doing" => 0,
        "todo" => 1,
        "waiting" => 2,
        "done" => 3,
        _ => 4,
    };
    let due = t.due_date().map(|d| d.days()).unwrap_or(i64::MAX);
    let pr = match t.priority.as_deref() {
        Some("high") => 0,
        Some("medium") => 1,
        Some("low") => 3,
        _ => 2,
    };
    (st, due, pr, t.title.clone())
}

fn move_event(st: &mut Store, p: &Json) -> R {
    let id = s(p, "id")?;
    let ev = st.event(id).ok_or_else(|| format!("{id} : événement introuvable"))?.clone();
    if ev.readonly {
        return Err("abonnement en lecture seule".into());
    }
    let new_start = When::parse(s(p, "start")?).ok_or("start illisible")?;
    let new_end = match p.get("end").as_str() {
        Some(e) => Some(When::parse(e).ok_or("end illisible")?),
        None => None,
    };
    let scope = p.get("scope").as_str().unwrap_or("one");
    let occurrence = p.get("occurrence").as_str();
    let mut op = Op::new(&format!("déplacer « {} »", ev.title));
    let mut patch = Json::obj().set("start", new_start.to_file_string());
    let zone = st.zone(ev.tz.as_deref()).clone();
    // fin : conservée si non fournie (même durée)
    let end = new_end.map(|e| e.to_file_string()).or_else(|| {
        let dur = ev.duration_secs(&zone);
        match new_start {
            When::Date(d) => (dur > DAY).then(|| d.add_days(dur / DAY - 1).to_string()),
            When::Local(dt) => Some(dt.add_secs(dur).to_string()),
            When::Utc(t) => Some(When::Utc(t + dur).to_file_string()),
        }
    });
    patch.insert("end", end.map(Json::from).unwrap_or(Json::Null));
    if ev.duration.is_some() {
        patch.insert("duration", Json::Null);
    }
    match (&ev.repeat, occurrence, scope) {
        (Some(_), Some(occ), "one") => {
            let new = st.detach(id, occ, &patch, &mut op)?;
            Ok(done(st, op).set("id", new))
        }
        (Some(_), Some(occ), _) => {
            // toute la série : même décalage appliqué au début de la série
            let occ_w = When::parse(occ).ok_or("occurrence illisible")?;
            let shift = |w: When| -> i64 {
                match w {
                    When::Date(d) => d.days() * DAY,
                    When::Local(dt) => dt.secs(),
                    When::Utc(t) => t,
                }
            };
            let delta = shift(new_start) - shift(occ_w);
            let ms = match (ev.start, new_start) {
                (When::Date(d), When::Date(_)) => When::Date(d.add_days(delta / DAY)),
                (When::Date(d), When::Local(nd)) => {
                    When::Local(DateTime { date: d.add_days(delta.div_euclid(DAY)), sec: nd.sec })
                }
                (When::Date(_), When::Utc(t)) => When::Utc(t),
                (When::Local(dt), When::Date(_)) => When::Date(dt.date.add_days(delta.div_euclid(DAY))),
                (When::Local(dt), _) => When::Local(dt.add_secs(delta)),
                (When::Utc(t), _) => When::Utc(t + delta),
            };
            let dur = match (new_start, new_end) {
                (When::Local(a), Some(When::Local(b))) => b.secs() - a.secs(),
                (When::Date(a), Some(When::Date(b))) => (b.days() - a.days() + 1) * DAY,
                _ => ev.duration_secs(&zone),
            };
            let mut patch = Json::obj().set("start", ms.to_file_string());
            patch.insert(
                "end",
                match ms {
                    When::Date(d) => {
                        if dur > DAY {
                            Json::from(d.add_days(dur / DAY - 1).to_string())
                        } else {
                            Json::Null
                        }
                    }
                    When::Local(dt) => Json::from(dt.add_secs(dur).to_string()),
                    When::Utc(t) => Json::from(When::Utc(t + dur).to_file_string()),
                },
            );
            // les exceptions suivent le décalage
            let ex: Vec<String> = ev
                .except
                .iter()
                .map(|w| match *w {
                    When::Date(d) => When::Date(d.add_days(delta.div_euclid(DAY))).to_file_string(),
                    When::Local(dt) => When::Local(dt.add_secs(delta)).to_file_string(),
                    When::Utc(t) => When::Utc(t + delta).to_file_string(),
                })
                .collect();
            if !ex.is_empty() {
                patch.insert("except", ex);
            }
            st.update(id, &patch, p.get("rev").as_str(), &mut op)?;
            Ok(done(st, op).set("id", id))
        }
        _ => {
            let (_, conflicts) = st.update(id, &patch, p.get("rev").as_str(), &mut op)?;
            Ok(done(st, op).set("id", id).set("conflicts", conflicts))
        }
    }
}

fn free(st: &Store, p: &Json) -> R {
    let (from, to) = range(st, p, 7)?;
    let min = p
        .get("min")
        .as_i64()
        .or_else(|| p.get("min").as_str().and_then(crate::date::parse_duration).map(|s| s / 60))
        .unwrap_or(30)
        .max(5);
    let ds =
        crate::date::parse_time(p.get("day_start").as_str().unwrap_or("08:00")).ok_or("day_start illisible")? as i64;
    let de = crate::date::parse_time(p.get("day_end").as_str().unwrap_or("20:00")).ok_or("day_end illisible")? as i64;
    let now = now_local(st);
    let mut busy: Vec<(i64, i64)> = st
        .occurrences(from, to)
        .iter()
        .filter(|o| {
            !o.all_day
                && o.ev.status != "cancelled"
                && !calendar_of(st, o.ev.calendar.as_deref()).map(|c| c.hidden).unwrap_or(false)
        })
        .map(|o| (o.start.secs(), o.end.secs()))
        .collect();
    busy.sort();
    let mut slots = Vec::new();
    let mut d = from.date;
    while d < to.date || (d == to.date && to.sec > 0) {
        let day0 = d.midnight().secs();
        let mut cur = (day0 + ds).max(from.secs()).max(now.secs());
        let end = (day0 + de).min(to.secs());
        // arrondi au quart d'heure suivant
        cur = (cur + 899) / 900 * 900;
        for &(bs, be) in busy.iter() {
            if be <= cur || bs >= end {
                continue;
            }
            if bs - cur >= min * 60 {
                slots.push((cur, bs));
            }
            cur = cur.max(be);
        }
        if end - cur >= min * 60 {
            slots.push((cur, end));
        }
        d = d.add_days(1);
    }
    Ok(Json::Arr(
        slots
            .into_iter()
            .map(|(a, b)| {
                Json::obj()
                    .set("start", DateTime::from_secs(a).to_string())
                    .set("end", DateTime::from_secs(b).to_string())
                    .set("minutes", (b - a) / 60)
            })
            .collect(),
    ))
}

fn brief(st: &Store, p: &Json) -> R {
    let now = now_local(st);
    let today = now.date;
    let days = p.get("days").as_i64().unwrap_or(7).clamp(1, 31);
    let occ = st.occurrences(today.midnight(), today.add_days(days).midnight());
    let mut lines: Vec<String> =
        vec![format!("Agenda — {} {}, {} ({})", human_date(today), today.y, now.hm(), st.local.name)];
    let fmt_occ = |o: &Occ| -> String {
        let t = if o.all_day {
            if o.end.date.days() - o.start.date.days() > 1 {
                format!("jusqu'au {}", date_label(o.end.date.add_days(-1), today))
            } else {
                "journée".into()
            }
        } else {
            format!("{}–{}", o.start.hm(), o.end.hm())
        };
        let mut s = format!("- {t} {}", o.ev.title);
        if let Some(l) = &o.ev.location {
            s.push_str(&format!(" @{l}"));
        }
        if o.ev.status == "cancelled" {
            s.push_str(" (annulé)");
        }
        s
    };
    let todays: Vec<&Occ> = occ.iter().filter(|o| o.start.date <= today && o.end > today.midnight()).collect();
    if todays.is_empty() {
        lines.push("Aujourd'hui : rien de prévu.".into());
    } else {
        lines.push("Aujourd'hui :".into());
        for o in &todays {
            let mut l = fmt_occ(o);
            if !o.all_day && o.end_utc <= now_utc() {
                l.push_str(" ✓");
            } else if !o.all_day && o.start_utc <= now_utc() {
                l.push_str(" (en cours)");
            }
            lines.push(l);
        }
    }
    let later: Vec<&Occ> = occ.iter().filter(|o| o.start.date > today).collect();
    if !later.is_empty() {
        lines.push(format!("{} prochains jours ({} événements) :", days - 1, later.len()));
        let mut cur = None;
        for o in later.iter().take(14) {
            if cur != Some(o.start.date) {
                cur = Some(o.start.date);
                lines.push(format!("{} :", date_label(o.start.date, today)));
            }
            lines.push(fmt_occ(o));
        }
        if later.len() > 14 {
            lines.push(format!("… et {} autres", later.len() - 14));
        }
    }
    let mut open: Vec<&Task> = st.tasks().filter(|t| t.is_open()).collect();
    open.sort_by_key(|a| task_order(a));
    let fmt_task = |t: &Task| -> String {
        let mut s = format!("- {}", t.title);
        if let Some(d) = t.due_date() {
            s.push_str(&format!(" ({})", date_label(d, today)));
        }
        match t.priority.as_deref() {
            Some("high") => s.push_str(" !haute"),
            Some("low") => s.push_str(" !basse"),
            _ => {}
        }
        if t.status == "doing" {
            s.push_str(" [en cours]");
        }
        if t.status == "waiting" {
            s.push_str(" [en attente]");
        }
        s
    };
    let overdue: Vec<&&Task> = open.iter().filter(|t| t.due_date().map(|d| d < today).unwrap_or(false)).collect();
    let soon: Vec<&&Task> =
        open.iter().filter(|t| t.due_date().map(|d| d >= today && d < today.add_days(days)).unwrap_or(false)).collect();
    let doing: Vec<&&Task> = open
        .iter()
        .filter(|t| t.status == "doing" && t.due_date().map(|d| d >= today.add_days(days)).unwrap_or(true))
        .collect();
    if !overdue.is_empty() {
        lines.push(format!("Tâches en retard ({}) :", overdue.len()));
        lines.extend(overdue.iter().take(8).map(|t| fmt_task(t)));
    }
    if !soon.is_empty() {
        lines.push(format!("Tâches à échéance ({}) :", soon.len()));
        lines.extend(soon.iter().take(10).map(|t| fmt_task(t)));
    }
    if !doing.is_empty() {
        lines.push("En cours :".into());
        lines.extend(doing.iter().take(5).map(|t| fmt_task(t)));
    }
    let undated = open.iter().filter(|t| t.due.is_none()).count();
    if undated > 0 {
        let high: Vec<&&Task> =
            open.iter().filter(|t| t.due.is_none() && t.priority.as_deref() == Some("high")).collect();
        lines.push(format!(
            "{undated} tâche(s) sans échéance{}",
            if high.is_empty() {
                String::new()
            } else {
                format!(
                    ", dont prioritaires : {}",
                    high.iter().take(5).map(|t| t.title.as_str()).collect::<Vec<_>>().join(", ")
                )
            }
        ));
    }
    if !st.conflicts.is_empty() {
        lines.push(format!("⚠ {} conflit(s) de synchronisation à résoudre dans l'application.", st.conflicts.len()));
    }
    let inv = st.invalid();
    if !inv.is_empty() {
        lines.push(format!(
            "⚠ {} fichier(s) illisible(s) : {}",
            inv.len(),
            inv.iter().take(3).map(|(i, _)| *i).collect::<Vec<_>>().join(", ")
        ));
    }
    let text = lines.join("\n");
    Ok(Json::obj()
        .set("text", text)
        .set("events", occ.len())
        .set("overdue", overdue.len())
        .set("due_soon", soon.len())
        .set("conflicts", st.conflicts.len()))
}

fn alarms(st: &Store, p: &Json) -> R {
    let now = now_utc();
    let from_utc = match p.get("from").as_str() {
        Some(f) => st.local.to_utc(parse_dt(f)?),
        None => p.get("from_utc").as_i64().unwrap_or(now),
    };
    let to_utc = match p.get("to").as_str() {
        Some(t) => st.local.to_utc(parse_dt(t)?),
        None => p.get("to_utc").as_i64().unwrap_or(from_utc + DAY),
    };
    if to_utc <= from_utc || to_utc - from_utc > 400 * DAY {
        return Err("intervalle de rappels invalide".into());
    }
    let max_off = st
        .all_events()
        .filter_map(|e| e.alarm.as_ref())
        .flatten()
        .chain(st.calendars.values().flat_map(|c| c.alarm.iter()))
        .copied()
        .max()
        .unwrap_or(0)
        .clamp(0, 60 * DAY);
    let wf = st.local.to_local(from_utc - DAY);
    let wt = st.local.to_local(to_utc + max_off + DAY);
    let mut out: Vec<(i64, Json)> = Vec::new();
    for o in st.occurrences(wf, wt) {
        if o.ev.status == "cancelled" {
            continue;
        }
        let cal = calendar_of(st, o.ev.calendar.as_deref());
        if cal.map(|c| c.hidden).unwrap_or(false) {
            continue;
        }
        let offsets = o.ev.alarm.clone().unwrap_or_else(|| cal.map(|c| c.alarm.clone()).unwrap_or_default());
        let base = if o.all_day { st.local.to_utc(o.start.date.at(9, 0)) } else { o.start_utc };
        for off in offsets {
            let at = base - off;
            if at >= from_utc && at < to_utc {
                let when = if o.all_day {
                    format!("{} (journée)", human_date(o.start.date))
                } else {
                    format!("{} à {}", date_label(o.start.date, st.local.to_local(at).date), o.start.hm())
                };
                let key = format!("{}|{}|{}", o.ev.id, o.key.clone().unwrap_or_default(), off);
                out.push((
                    at,
                    Json::obj()
                        .set("key", key)
                        .set("id", o.ev.id.as_str())
                        .set("kind", "event")
                        .set("title", o.ev.title.as_str())
                        .set("at", st.local.to_local(at).to_string())
                        .set("at_utc", at)
                        .set("start", if o.all_day { o.start.date.to_string() } else { o.start.to_string() })
                        .set("location", o.ev.location.clone())
                        .set(
                            "body",
                            format!("{when}{}", o.ev.location.as_ref().map(|l| format!(" · {l}")).unwrap_or_default()),
                        )
                        .set("offset", format_duration(off)),
                ));
            }
        }
    }
    for t in st.tasks().filter(|t| t.is_open()) {
        let Some(due) = t.due else { continue };
        if due.is_date() && t.alarm.is_none() {
            continue;
        }
        let base = match due {
            When::Date(d) => st.local.to_utc(d.at(9, 0)),
            When::Local(dt) => st.local.to_utc(dt),
            When::Utc(u) => u,
        };
        let offsets = t.alarm.clone().unwrap_or_else(|| vec![0]);
        for off in offsets {
            let at = base - off;
            if at >= from_utc && at < to_utc {
                out.push((
                    at,
                    Json::obj()
                        .set("key", format!("{}|{}|{}", t.id, due.to_file_string(), off))
                        .set("id", t.id.as_str())
                        .set("kind", "task")
                        .set("title", t.title.as_str())
                        .set("at", st.local.to_local(at).to_string())
                        .set("at_utc", at)
                        .set("start", due.to_file_string())
                        .set(
                            "body",
                            format!(
                                "Échéance : {}",
                                match due {
                                    When::Date(d) => human_date(d),
                                    _ => st.local.to_local(base).to_string(),
                                }
                            ),
                        )
                        .set("offset", format_duration(off)),
                ));
            }
        }
    }
    out.sort_by_key(|x| x.0);
    Ok(Json::Arr(out.into_iter().map(|x| x.1).collect()))
}

/// Chaîne JSON d'un identifiant de révision (utilisé par les hôtes).
pub fn rev(s: &str) -> String {
    rev_of(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api(name: &str) -> (Api, PathBuf) {
        let dir = crate::store::tests::tmpdir(name);
        let api = Api::new();
        api.call_json("set_timezone", &Json::obj().set("tz", "Europe/Paris")).unwrap();
        api.call_json("open", &Json::obj().set("path", dir.to_string_lossy().to_string()).set("create", true)).unwrap();
        (api, dir)
    }

    #[test]
    fn protocole() {
        let a = Api::new();
        assert_eq!(a.call("version", ""), format!("{{\"ok\":true,\"result\":\"{}\"}}", crate::VERSION));
        assert!(a.call("list", "{}").contains("aucun dossier"));
        assert!(a.call("x", "{").contains("\"ok\":false"));
        assert!(a.call("inconnue", "{}").contains("\"ok\":false"));
    }

    #[test]
    fn parcours_complet() {
        let (a, dir) = api("api");
        let r =
            a.call_json("add", &Json::obj().set("text", "Sport tous les mardis 18h30 pendant 1h30 #sport")).unwrap();
        let id = r.get("id").as_str().unwrap().to_string();
        let r = a.call_json("add", &Json::obj().set("text", "Rapport demain !haute")).unwrap();
        let tid = r.get("id").as_str().unwrap().to_string();
        assert!(tid.starts_with("tasks/"));
        let today = Tz::load("Europe/Paris").unwrap().to_local(now_utc()).date;
        let l = a.call_json("list", &Json::obj().set("from", today.to_string()).set("days", 15i64)).unwrap();
        assert!(l.get("events").as_arr().len() >= 2);
        let occ = l.get("events").as_arr()[0].clone();
        assert_eq!(occ.get("recurring").as_bool(), Some(true));
        // déplacer une seule occurrence
        let o = occ.get("occurrence").as_str().unwrap().to_string();
        let start = DateTime::parse(&o).unwrap();
        let ns = start.add_secs(3600);
        let r = a
            .call_json(
                "move",
                &Json::obj()
                    .set("id", id.as_str())
                    .set("occurrence", o.as_str())
                    .set("start", ns.to_string())
                    .set("end", ns.add_secs(5400).to_string())
                    .set("scope", "one"),
            )
            .unwrap();
        assert_ne!(r.get("id").as_str(), Some(id.as_str()));
        a.call_json("undo", &Json::obj()).unwrap();
        // recherche insensible aux accents
        let s = a.call_json("search", &Json::obj().set("q", "RAPPORT")).unwrap();
        assert_eq!(s.as_arr()[0].get("id").as_str(), Some(tid.as_str()));
        // tâche terminée puis annulation
        a.call_json("task_done", &Json::obj().set("id", tid.as_str())).unwrap();
        assert_eq!(
            a.call_json("get", &Json::obj().set("id", tid.as_str())).unwrap().get("status").as_str(),
            Some("done")
        );
        a.call_json("undo", &Json::obj()).unwrap();
        assert_eq!(
            a.call_json("get", &Json::obj().set("id", tid.as_str())).unwrap().get("status").as_str(),
            Some("todo")
        );
        // brief, free, alarms
        let b = a.call_json("brief", &Json::obj()).unwrap();
        assert!(b.get("text").as_str().unwrap().contains("Rapport"));
        assert!(b.get("text").as_str().unwrap().len() < 4000);
        let f = a
            .call_json(
                "free",
                &Json::obj().set("from", today.add_days(1).to_string()).set("to", today.add_days(1).to_string()),
            )
            .unwrap();
        assert!(!f.as_arr().is_empty());
        let al = a
            .call_json("alarms", &Json::obj().set("from", today.to_string()).set("to", today.add_days(14).to_string()))
            .unwrap();
        // calendrier par défaut « perso » sans rattachement : pas de rappel ; on en ajoute un
        assert!(al.as_arr().is_empty());
        a.call_json("update", &Json::obj().set("id", id.as_str()).set("fields", Json::obj().set("alarm", vec!["15m"])))
            .unwrap();
        let al = a
            .call_json("alarms", &Json::obj().set("from", today.to_string()).set("to", today.add_days(14).to_string()))
            .unwrap();
        assert!(al.as_arr().len() >= 2);
        // export / import
        let ics = a.call_json("ics_export", &Json::obj()).unwrap();
        assert!(ics.as_str().unwrap().contains("RRULE:FREQ=WEEKLY;BYDAY=TU"));
        let (b2, dir2) = api("api2");
        let r = b2.call_json("ics_import", &Json::obj().set("text", ics.as_str().unwrap())).unwrap();
        assert_eq!(r.get("created").as_i64(), Some(2));
        let r = b2.call_json("ics_import", &Json::obj().set("text", ics.as_str().unwrap())).unwrap();
        assert_eq!(r.get("created").as_i64(), Some(1)); // les tâches n'ont pas d'UID stocké
                                                        // suppression + corbeille
        a.call_json("delete", &Json::obj().set("id", tid.as_str())).unwrap();
        let t = a.call_json("trash", &Json::obj()).unwrap();
        let name = t.as_arr()[0].get("name").as_str().unwrap().to_string();
        a.call_json("trash_restore", &Json::obj().set("name", name.as_str())).unwrap();
        assert!(dir.join(&tid).exists());
        // sécurité des identifiants
        assert!(a.call_json("update", &Json::obj().set("id", "../../etc/passwd").set("fields", Json::obj())).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(&dir2).unwrap();
    }

    #[test]
    fn changements_externes() {
        let (a, dir) = api("changes");
        let v0 = a.call_json("info", &Json::obj()).unwrap().get("version").as_i64().unwrap();
        let h = std::thread::spawn({
            let dir = dir.clone();
            move || {
                std::thread::sleep(Duration::from_millis(150));
                std::fs::write(dir.join("tasks/externe.md"), "---\ntitle: Ajoutée par Hermes\n---\n").unwrap();
            }
        });
        let r = a.call_json("changes", &Json::obj().set("since", v0).set("wait", 5000i64)).unwrap();
        h.join().unwrap();
        assert!(r.get("version").as_i64().unwrap() > v0);
        assert!(r.get("ids").str_list().unwrap().contains(&"tasks/externe.md".to_string()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn abonnement() {
        let (a, dir) = api("sub");
        a.call_json(
            "calendar_save",
            &Json::obj().set("name", "Fériés").set("fields", Json::obj().set("url", "webcal://example.org/f.ics")),
        )
        .unwrap();
        let t = a.call_json("sub_targets", &Json::obj()).unwrap();
        assert_eq!(t.as_arr()[0].get("url").as_str(), Some("https://example.org/f.ics"));
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:1\r\nSUMMARY:Noël\r\nDTSTART;VALUE=DATE:20261225\r\nRRULE:FREQ=YEARLY\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        a.call_json("sub_store", &Json::obj().set("name", "feries").set("text", ics)).unwrap();
        let l = a.call_json("list", &Json::obj().set("from", "2026-12-01").set("to", "2026-12-31")).unwrap();
        let e = &l.get("events").as_arr()[0];
        assert_eq!(e.get("title").as_str(), Some("Noël"));
        assert_eq!(e.get("readonly").as_bool(), Some(true));
        let id = e.get("id").as_str().unwrap();
        assert!(a.call_json("delete", &Json::obj().set("id", id)).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
