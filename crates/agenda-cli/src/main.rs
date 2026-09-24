//! `agenda` : CLI, serveur HTTP (`serve`), serveur MCP (`mcp`) et rappels (`remind`).

mod fetch;
mod mcp;
mod remind;
mod serve;

use agenda_core::date::{human_date, When};
use agenda_core::json::Json;
use agenda_core::Api;
use std::path::PathBuf;
use std::process::ExitCode;

const HELP: &str = "agenda — agenda et tâches local-first (Markdown + YAML)

Usage : agenda [--dir DOSSIER] [--json] <commande> [arguments]

Consulter
  today                      aujourd'hui (événements et tâches)
  week                       les 7 prochains jours
  list [--from D] [--to D] [--days N]
  tasks [--all]              tâches ouvertes (--all : toutes)
  search TEXTE               recherche (insensible aux accents)
  show ID                    un élément complet
  free [--from D] [--to D] [--min 30] [--day-start 08:00] [--day-end 20:00]
  brief [--days N]           résumé court de la situation (~500 tokens)
  conflicts                  conflits de synchronisation

Modifier
  add TEXTE [--dry-run]      ajout en langage naturel :
                               agenda add \"Dentiste vendredi 14h-15h @Cabinet #santé\"
  done ID | undone ID        terminer / rouvrir une tâche
  rm ID                      mettre à la corbeille
  undo | redo
  set ID CHAMP=VALEUR…       modifier des champs (VALEUR vide : supprimer)

iCalendar
  import FICHIER.ics [--calendar NOM]
  export [--output FICHIER]
  sub add NOM URL            abonnement en lecture seule (https:// ou webcal://)
  sub update                 télécharge les abonnements (via curl)

Services
  serve [--addr 127.0.0.1:8421] [--token JETON]   interface web + /agenda.ics
  mcp                        serveur MCP (stdio) pour les agents
  remind [--exec CMD | --ntfy URL] [--once]        rappels

Divers
  init [DOSSIER]             crée un dossier agenda
  call METHODE [JSON]        appel brut de l'API (voir « agenda call help »)
  config                     dossier utilisé et d'où il vient
  --version

ID : chemin d'un élément (events/2026-10/dentiste.md), ou un mot de son titre
s'il ne désigne qu'un seul élément.

Dossier : --dir, sinon $AGENDA_DIR, sinon la ligne « dir=… » de
~/.config/agenda/config, sinon ~/agenda.
";

pub struct Ctx {
    pub api: Api,
    pub json: bool,
    pub dir: PathBuf,
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

pub fn config_file() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
        .join("agenda/config")
}

/// État local à la machine (journal d'annulation, dernier rappel).
pub fn state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/state")).join("agenda")
}

fn expand(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(r) => home().join(r),
        None => PathBuf::from(p),
    }
}

/// Dossier agenda et provenance.
fn resolve_dir(flag: Option<String>) -> (PathBuf, &'static str) {
    if let Some(d) = flag {
        return (expand(&d), "--dir");
    }
    if let Some(d) = std::env::var("AGENDA_DIR").ok().filter(|d| !d.is_empty()) {
        return (expand(&d), "AGENDA_DIR");
    }
    if let Ok(c) = std::fs::read_to_string(config_file()) {
        for l in c.lines() {
            if let Some(d) = l.trim().strip_prefix("dir=").or_else(|| l.trim().strip_prefix("dir =")) {
                return (expand(d.trim().trim_matches('"')), "~/.config/agenda/config");
            }
        }
    }
    (home().join("agenda"), "défaut")
}

struct Args {
    pos: Vec<String>,
    flags: Vec<(String, Option<String>)>,
}

impl Args {
    fn parse(raw: Vec<String>, with_value: &[&str]) -> Args {
        let mut pos = Vec::new();
        let mut flags = Vec::new();
        let mut it = raw.into_iter();
        while let Some(a) = it.next() {
            if a == "--" {
                pos.extend(it.by_ref());
                break;
            }
            if let Some(f) = a.strip_prefix("--") {
                if let Some((k, v)) = f.split_once('=') {
                    flags.push((k.to_string(), Some(v.to_string())));
                } else if with_value.contains(&f) {
                    flags.push((f.to_string(), it.next()));
                } else {
                    flags.push((f.to_string(), None));
                }
            } else {
                pos.push(a);
            }
        }
        Args { pos, flags }
    }
    fn get(&self, k: &str) -> Option<&str> {
        self.flags.iter().rev().find(|(f, _)| f == k).and_then(|(_, v)| v.as_deref())
    }
    fn has(&self, k: &str) -> bool {
        self.flags.iter().any(|(f, _)| f == k)
    }
}

pub fn die(msg: &str) -> ExitCode {
    eprintln!("agenda : {msg}");
    ExitCode::from(1)
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = Args::parse(
        raw,
        &[
            "dir",
            "from",
            "to",
            "days",
            "min",
            "day-start",
            "day-end",
            "addr",
            "token",
            "exec",
            "ntfy",
            "calendar",
            "output",
            "limit",
            "tz",
        ],
    );
    if args.has("version") || args.pos.first().map(String::as_str) == Some("version") {
        println!("agenda {}", agenda_core::VERSION);
        return ExitCode::SUCCESS;
    }
    let cmd = args.pos.first().cloned().unwrap_or_else(|| "help".into());
    if args.has("help") || cmd == "help" || cmd == "-h" {
        print!("{HELP}");
        return ExitCode::SUCCESS;
    }
    let (dir, source) = resolve_dir(
        args.get("dir").map(str::to_string).or_else(|| (cmd == "init").then(|| args.pos.get(1).cloned()).flatten()),
    );
    let ctx = Ctx { api: Api::new(), json: args.has("json"), dir: dir.clone() };
    if let Some(tz) = args.get("tz") {
        if let Err(e) = ctx.api.call_json("set_timezone", &Json::obj().set("tz", tz)) {
            return die(&e);
        }
    }
    if cmd == "config" {
        println!("dossier : {}\nsource  : {source}\nconfig  : {}", dir.display(), config_file().display());
        return ExitCode::SUCCESS;
    }
    let create = cmd == "init";
    // pas de surveillance du dossier pour une commande ponctuelle
    let watch = matches!(cmd.as_str(), "serve" | "mcp" | "remind");
    let journal =
        state_dir().join(format!("undo-{:016x}.json", agenda_core::store::hash(dir.to_string_lossy().as_bytes())));
    let open = Json::obj()
        .set("path", dir.to_string_lossy().to_string())
        .set("create", create)
        .set("watch", watch)
        .set("journal", journal.to_string_lossy().to_string());
    if let Err(e) = ctx.api.call_json("open", &open) {
        return die(&format!("{e}\n(dossier : {}, source : {source} ; « agenda init » pour le créer)", dir.display()));
    }
    let rest: Vec<String> = args.pos[1..].to_vec();
    let r = match cmd.as_str() {
        "init" => {
            println!("Dossier agenda prêt : {}", dir.display());
            Ok(())
        }
        "today" => list(&ctx, &args, 1),
        "week" => list(&ctx, &args, 7),
        "list" | "ls" => list(&ctx, &args, 7),
        "tasks" => tasks(&ctx, args.has("all")),
        "search" => search(&ctx, &rest.join(" ")),
        "show" => show(&ctx, &rest.join(" ")),
        "free" => free(&ctx, &args),
        "brief" => {
            let p = Json::obj().set("days", args.get("days").and_then(|d| d.parse::<i64>().ok()).unwrap_or(7));
            call(&ctx, "brief", &p).map(|v| {
                if ctx.json {
                    println!("{}", v.to_pretty());
                } else {
                    println!("{}", v.get("text").as_str().unwrap_or(""));
                }
            })
        }
        "conflicts" => call(&ctx, "conflicts", &Json::obj()).map(|v| {
            if ctx.json {
                println!("{}", v.to_pretty());
            } else if v.as_arr().is_empty() {
                println!("Aucun conflit.");
            } else {
                for c in v.as_arr() {
                    println!("{}  ({}) → {}", c.get("path").str_or(""), c.get("source").str_or(""), c.get("original").str_or(""));
                    for f in c.get("fields").as_arr() {
                        println!("   {} : « {} » / « {} »", f.get("field").str_or(""), short(f.get("original")), short(f.get("conflict")));
                    }
                }
                println!("Résolution : agenda call conflict_resolve '{{\"path\":\"…\",\"keep\":\"original\"}}' (ou dans l'application)");
            }
        }),
        "add" => add(&ctx, &rest.join(" "), args.has("dry-run")),
        "done" | "undone" => resolve(&ctx, &rest.join(" "), Some("task")).and_then(|id| {
            call(&ctx, "task_done", &Json::obj().set("id", id.as_str()).set("done", cmd == "done")).map(|v| {
                if ctx.json {
                    println!("{}", v.to_pretty());
                } else if v.get("repeated").as_bool() == Some(true) {
                    println!("✓ {id} : reportée à l'échéance suivante");
                } else {
                    println!("✓ {id} : {}", if cmd == "done" { "terminée" } else { "rouverte" });
                }
            })
        }),
        "rm" | "delete" => resolve(&ctx, &rest.join(" "), None).and_then(|id| {
            call(&ctx, "delete", &Json::obj().set("id", id.as_str())).map(|_| println!("🗑 {id} déplacé dans .trash/ (agenda undo pour annuler)"))
        }),
        "undo" | "redo" => call(&ctx, &cmd, &Json::obj()).map(|v| {
            println!("↶ {}", v.get(if cmd == "undo" { "undone" } else { "redone" }).str_or(""));
        }),
        "set" => set(&ctx, &rest),
        "import" => import(&ctx, &rest, args.get("calendar")),
        "export" => call(&ctx, "ics_export", &Json::obj()).and_then(|v| {
            let text = v.as_str().unwrap_or("");
            match args.get("output") {
                Some(f) => std::fs::write(f, text).map_err(|e| format!("{f} : {e}")).map(|_| eprintln!("Exporté vers {f}")),
                None => {
                    print!("{text}");
                    Ok(())
                }
            }
        }),
        "sub" => sub(&ctx, &rest),
        "serve" => serve::run(ctx, args.get("addr").unwrap_or("127.0.0.1:8421"), args.get("token").map(str::to_string)),
        "mcp" => mcp::run(&ctx),
        "remind" => remind::run(&ctx, args.get("exec"), args.get("ntfy"), args.has("once"), args.has("quiet")),
        "call" => {
            let method = rest.first().cloned().unwrap_or_else(|| "help".into());
            let p = rest.get(1).cloned().unwrap_or_else(|| "{}".into());
            let out = ctx.api.call(&method, &p);
            match Json::parse(&out) {
                Ok(v) if v.get("ok").as_bool() == Some(true) => {
                    println!("{}", v.get("result").to_pretty());
                    Ok(())
                }
                Ok(v) => Err(v.get("error").str_or("erreur").to_string()),
                Err(e) => Err(e),
            }
        }
        other => Err(format!("commande inconnue : {other} (agenda help)")),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => die(&e),
    }
}

pub fn call(ctx: &Ctx, m: &str, p: &Json) -> Result<Json, String> {
    ctx.api.call_json(m, p)
}

fn short(v: &Json) -> String {
    let s = match v {
        Json::Str(s) => s.clone(),
        Json::Null => "∅".into(),
        o => o.to_string(),
    };
    let s = s.replace('\n', "⏎");
    if s.chars().count() > 50 {
        format!("{}…", s.chars().take(50).collect::<String>())
    } else {
        s
    }
}

/// Trouve un identifiant à partir d'un chemin ou d'un bout de titre.
fn resolve(ctx: &Ctx, q: &str, kind: Option<&str>) -> Result<String, String> {
    let q = q.trim();
    if q.is_empty() {
        return Err("identifiant manquant".into());
    }
    if call(ctx, "get", &Json::obj().set("id", q)).is_ok() {
        return Ok(q.to_string());
    }
    let hits = call(ctx, "search", &Json::obj().set("q", q).set("limit", 20i64))?;
    let hits: Vec<&Json> = hits
        .as_arr()
        .iter()
        .filter(|h| kind.map(|k| h.get("kind").as_str() == Some(k)).unwrap_or(true))
        .filter(|h| !h.get("id").str_or("").starts_with("sub:"))
        .collect();
    // une tâche ouverte l'emporte sur les tâches terminées
    let open: Vec<&&Json> =
        hits.iter().filter(|h| !matches!(h.get("status").as_str(), Some("done" | "cancelled"))).collect();
    match (hits.len(), open.len()) {
        (0, _) => Err(format!("rien ne correspond à « {q} »")),
        (1, _) => Ok(hits[0].get("id").str_or("").to_string()),
        (_, 1) => Ok(open[0].get("id").str_or("").to_string()),
        _ => {
            let list: Vec<String> = hits
                .iter()
                .take(8)
                .map(|h| format!("  {}  ({})", h.get("id").str_or(""), h.get("title").str_or("")))
                .collect();
            Err(format!("« {q} » est ambigu :\n{}", list.join("\n")))
        }
    }
}

fn day_header(d: &str) -> String {
    agenda_core::date::Date::parse(&d[..10.min(d.len())])
        .map(|d| {
            let s = human_date(d);
            let mut c = s.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .unwrap_or_default()
}

fn list(ctx: &Ctx, args: &Args, default_days: i64) -> Result<(), String> {
    let mut p = Json::obj().set("tasks", true);
    if let Some(f) = args.get("from") {
        p.insert("from", f);
    }
    if let Some(t) = args.get("to") {
        p.insert("to", t);
    }
    p.insert("days", args.get("days").and_then(|d| d.parse::<i64>().ok()).unwrap_or(default_days));
    let v = call(ctx, "list", &p)?;
    if ctx.json {
        println!("{}", v.to_pretty());
        return Ok(());
    }
    let evs = v.get("events").as_arr();
    let tasks = v.get("tasks").as_arr();
    if evs.is_empty() && tasks.is_empty() {
        println!("Rien de prévu.");
    }
    let mut cur = String::new();
    for e in evs {
        let start = e.get("start").str_or("");
        let day = &start[..10.min(start.len())];
        if day != cur {
            cur = day.to_string();
            println!("\n{}", day_header(day));
        }
        let t = if e.get("all_day").as_bool() == Some(true) {
            "journée    ".to_string()
        } else {
            let end = e.get("end").str_or("");
            format!("{}–{}", &start[11..16.min(start.len())], &end[11.min(end.len())..16.min(end.len())])
        };
        let mut line = format!("  {t}  {}", e.get("title").str_or(""));
        if let Some(l) = e.get("location").as_str() {
            line.push_str(&format!("  @{l}"));
        }
        if e.get("recurring").as_bool() == Some(true) {
            line.push_str("  ↻");
        }
        println!("{line}");
    }
    if !tasks.is_empty() {
        println!("\nTâches à échéance");
        for t in tasks {
            print_task(t);
        }
    }
    Ok(())
}

fn print_task(t: &Json) {
    let mark = match t.get("status").as_str() {
        Some("done") => "✓",
        Some("doing") => "◐",
        Some("waiting") => "…",
        Some("cancelled") => "✗",
        _ => "○",
    };
    let mut line = format!("  {mark} {}", t.get("title").str_or(""));
    if let Some(d) = t.get("due").as_str() {
        line.push_str(&format!("  ({d})"));
    }
    match t.get("priority").as_str() {
        Some("high") => line.push_str("  !haute"),
        Some("low") => line.push_str("  !basse"),
        _ => {}
    }
    println!("{line}   {}", t.get("id").str_or(""));
}

fn tasks(ctx: &Ctx, all: bool) -> Result<(), String> {
    let v = call(ctx, "tasks", &Json::obj().set("include_done", all))?;
    if ctx.json {
        println!("{}", v.to_pretty());
    } else if v.as_arr().is_empty() {
        println!("Aucune tâche ouverte.");
    } else {
        for t in v.as_arr() {
            print_task(t);
        }
    }
    Ok(())
}

fn search(ctx: &Ctx, q: &str) -> Result<(), String> {
    let v = call(ctx, "search", &Json::obj().set("q", q))?;
    if ctx.json {
        println!("{}", v.to_pretty());
    } else if v.as_arr().is_empty() {
        println!("Aucun résultat.");
    } else {
        for h in v.as_arr() {
            let kind = if h.get("kind").as_str() == Some("task") { "tâche" } else { "évén." };
            println!(
                "  {kind}  {:<16}  {}   {}",
                h.get("when").str_or(""),
                h.get("title").str_or(""),
                h.get("id").str_or("")
            );
        }
    }
    Ok(())
}

fn show(ctx: &Ctx, q: &str) -> Result<(), String> {
    let id = resolve(ctx, q, None)?;
    let v = call(ctx, "get", &Json::obj().set("id", id.as_str()))?;
    if ctx.json {
        println!("{}", v.to_pretty());
    } else {
        println!("# {id}\n{}", v.get("raw").as_str().unwrap_or(""));
    }
    Ok(())
}

fn free(ctx: &Ctx, args: &Args) -> Result<(), String> {
    let mut p = Json::obj();
    for (flag, key) in [("from", "from"), ("to", "to"), ("day-start", "day_start"), ("day-end", "day_end")] {
        if let Some(v) = args.get(flag) {
            p.insert(key, v);
        }
    }
    if let Some(m) = args.get("min").and_then(|m| m.parse::<i64>().ok()) {
        p.insert("min", m);
    }
    let v = call(ctx, "free", &p)?;
    if ctx.json {
        println!("{}", v.to_pretty());
        return Ok(());
    }
    let mut cur = String::new();
    for s in v.as_arr() {
        let a = s.get("start").str_or("");
        let b = s.get("end").str_or("");
        if a[..10] != cur {
            cur = a[..10].to_string();
            println!("{}", day_header(&cur));
        }
        let m = s.get("minutes").as_i64().unwrap_or(0);
        println!("  {}–{}  ({}h{:02})", &a[11..16], &b[11..16], m / 60, m % 60);
    }
    if v.as_arr().is_empty() {
        println!("Aucun créneau libre.");
    }
    Ok(())
}

fn add(ctx: &Ctx, text: &str, dry: bool) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("texte manquant : agenda add \"Dentiste vendredi 14h-15h\"".into());
    }
    let preview = call(ctx, "parse", &Json::obj().set("text", text))?;
    if dry {
        if ctx.json {
            println!("{}", preview.to_pretty());
        } else {
            println!("{}", preview.get("summary").str_or(""));
            println!("{}", preview.get("fields").to_pretty());
        }
        return Ok(());
    }
    let v = call(ctx, "add", &Json::obj().set("text", text))?;
    if ctx.json {
        println!("{}", v.to_pretty());
    } else {
        println!("✓ {}\n  {}", v.get("summary").str_or(""), v.get("id").str_or(""));
    }
    Ok(())
}

fn set(ctx: &Ctx, rest: &[String]) -> Result<(), String> {
    let (id_part, pairs): (Vec<&String>, Vec<&String>) = rest.iter().partition(|a| !a.contains('='));
    let id = resolve(ctx, &id_part.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" "), None)?;
    let mut fields = Json::obj();
    for p in pairs {
        let (k, v) = p.split_once('=').unwrap_or((p, ""));
        let val = if v.is_empty() {
            Json::Null
        } else if matches!(k, "tags" | "alarm" | "except") && v != "none" {
            Json::from(v.split(',').map(|x| x.trim().to_string()).collect::<Vec<_>>())
        } else {
            Json::from(v)
        };
        fields.insert(k, val);
    }
    let v = call(ctx, "update", &Json::obj().set("id", id.as_str()).set("fields", fields))?;
    if ctx.json {
        println!("{}", v.to_pretty());
    } else {
        println!("✓ {id} modifié");
        for c in v.get("conflicts").as_arr() {
            println!("  ⚠ conflit sur « {} » : la version externe est conservée", c.str_or(""));
        }
    }
    Ok(())
}

fn import(ctx: &Ctx, rest: &[String], calendar: Option<&str>) -> Result<(), String> {
    let f = rest.first().ok_or("fichier .ics manquant")?;
    let text = std::fs::read_to_string(f).map_err(|e| format!("{f} : {e}"))?;
    let mut p = Json::obj().set("text", text);
    if let Some(c) = calendar {
        p.insert("calendar", c);
    }
    let v = call(ctx, "ics_import", &p)?;
    if ctx.json {
        println!("{}", v.to_pretty());
    } else {
        println!(
            "{} élément(s) importé(s), {} déjà présent(s).",
            v.get("created").as_i64().unwrap_or(0),
            v.get("skipped").as_i64().unwrap_or(0)
        );
        for e in v.get("errors").as_arr() {
            println!("  ⚠ {}", e.str_or(""));
        }
    }
    Ok(())
}

fn sub(ctx: &Ctx, rest: &[String]) -> Result<(), String> {
    match rest.first().map(String::as_str) {
        Some("add") => {
            let name = rest.get(1).ok_or("nom manquant")?;
            let url = rest.get(2).ok_or("URL manquante")?;
            if !(url.starts_with("https://") || url.starts_with("http://") || url.starts_with("webcal://")) {
                return Err("l'URL doit commencer par https://, http:// ou webcal://".into());
            }
            call(
                ctx,
                "calendar_save",
                &Json::obj()
                    .set("name", name.as_str())
                    .set("fields", Json::obj().set("title", name.as_str()).set("url", url.as_str())),
            )?;
            println!("Abonnement « {name} » ajouté. Téléchargement…");
            sub_update(ctx)
        }
        Some("update") | None => sub_update(ctx),
        Some(o) => Err(format!("sous-commande inconnue : sub {o}")),
    }
}

fn sub_update(ctx: &Ctx) -> Result<(), String> {
    let r = fetch::update_subscriptions(&ctx.api);
    if ctx.json {
        println!("{}", r.to_pretty());
    } else {
        for x in r.as_arr() {
            match x.get("error").as_str() {
                Some(e) => println!("  ✗ {} : {e}", x.get("name").str_or("")),
                None => println!(
                    "  ✓ {} : {} événement(s)",
                    x.get("name").str_or(""),
                    x.get("events").as_i64().unwrap_or(0)
                ),
            }
        }
        if r.as_arr().is_empty() {
            println!("Aucun abonnement (agenda sub add NOM URL).");
        }
    }
    Ok(())
}

/// Instant lisible d'une valeur de fichier.
pub fn when_label(s: &str) -> String {
    match When::parse(s) {
        Some(When::Date(d)) => human_date(d),
        Some(When::Local(dt)) => format!("{} à {}", human_date(dt.date), dt.hm()),
        _ => s.to_string(),
    }
}
