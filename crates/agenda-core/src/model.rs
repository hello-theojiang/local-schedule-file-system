//! Éléments de l'agenda lus depuis leur en-tête YAML.

use crate::date::{parse_duration, Date, DateTime, When, DAY};
use crate::json::Json;
use crate::rrule::RRule;
use crate::tz::Tz;
use crate::yaml::{Doc, Value};

#[derive(Clone, Debug)]
pub struct Event {
    pub id: String,
    pub title: String,
    pub start: When,
    pub end: Option<When>,
    pub duration: Option<i64>,
    pub calendar: Option<String>,
    pub location: Option<String>,
    pub tags: Vec<String>,
    pub repeat: Option<Box<RRule>>,
    pub except: Vec<When>,
    pub tz: Option<String>,
    /// `None` : rappel par défaut du calendrier ; `Some(vec![])` : aucun rappel.
    pub alarm: Option<Vec<i64>>,
    /// `confirmed`, `tentative` ou `cancelled` (sans allocation)
    pub status: &'static str,
    pub uid: Option<String>,
    pub body: String,
    pub readonly: bool,
}

#[derive(Clone, Debug)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub status: String,
    pub due: Option<When>,
    pub priority: Option<String>,
    pub calendar: Option<String>,
    pub tags: Vec<String>,
    pub done_at: Option<String>,
    pub repeat: Option<RRule>,
    pub alarm: Option<Vec<i64>>,
    pub body: String,
}

#[derive(Clone, Debug)]
pub struct Calendar {
    pub name: String,
    pub title: String,
    pub color: String,
    pub alarm: Vec<i64>,
    pub url: Option<String>,
    pub hidden: bool,
}

pub const STATUSES: [&str; 5] = ["todo", "doing", "waiting", "done", "cancelled"];
pub const PRIORITIES: [&str; 3] = ["high", "medium", "low"];

const PALETTE: [&str; 10] =
    ["#6d5dfc", "#e0527a", "#1f9d8b", "#f08c2e", "#3b82f6", "#9b59b6", "#16a34a", "#d97706", "#0ea5e9", "#db2777"];

pub fn default_color(name: &str) -> &'static str {
    let h = name.bytes().fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    PALETTE[(h % PALETTE.len() as u32) as usize]
}

/// `alarm: [15m, 1d]`, `alarm: 15m`, `alarm: none`.
pub fn parse_alarms(v: &Value) -> Option<Vec<i64>> {
    match v {
        Value::Null => None,
        Value::Str(s) if matches!(s.trim().to_lowercase().as_str(), "none" | "non" | "aucun" | "false" | "off") => {
            Some(vec![])
        }
        _ => {
            let mut a: Vec<i64> = v.as_list().iter().filter_map(|s| parse_duration(s)).collect();
            a.sort();
            a.dedup();
            Some(a)
        }
    }
}

fn opt(doc: &Doc, k: &str) -> Option<String> {
    doc.str(k).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

impl Event {
    pub fn from_doc(id: &str, doc: &Doc) -> Result<Event, String> {
        let title = opt(doc, "title").unwrap_or_else(|| "(sans titre)".into());
        let start_s = opt(doc, "start").ok_or("champ start manquant")?;
        let start = When::parse(&start_s).ok_or_else(|| format!("start illisible : {start_s}"))?;
        let end = match opt(doc, "end") {
            Some(s) => Some(When::parse(&s).ok_or_else(|| format!("end illisible : {s}"))?),
            None => None,
        };
        let repeat = match opt(doc, "repeat").or_else(|| opt(doc, "rrule")) {
            Some(r) => Some(Box::new(RRule::parse(&r)?)),
            None => None,
        };
        Ok(Event {
            id: id.to_string(),
            title,
            start,
            end,
            duration: opt(doc, "duration").and_then(|d| parse_duration(&d)),
            calendar: opt(doc, "calendar"),
            location: opt(doc, "location"),
            tags: doc.get("tags").as_list(),
            repeat,
            except: doc.get("except").as_list().iter().filter_map(|s| When::parse(s)).collect(),
            tz: opt(doc, "tz"),
            alarm: parse_alarms(&doc.get("alarm")),
            status: match opt(doc, "status").map(|s| s.to_lowercase()).as_deref() {
                Some("cancelled" | "annulé" | "annule") => "cancelled",
                Some("tentative" | "provisoire") => "tentative",
                _ => "confirmed",
            },
            uid: opt(doc, "uid"),
            body: doc.body.clone(),
            readonly: false,
        })
    }

    pub fn all_day(&self) -> bool {
        self.start.is_date()
    }

    /// Durée en secondes (journées entières : nombre de jours × 86400).
    pub fn duration_secs(&self, zone: &Tz) -> i64 {
        match (self.start, self.end) {
            (When::Date(s), Some(When::Date(e))) => ((e.days() - s.days()).max(0) + 1) * DAY,
            (When::Date(_), _) => self.duration.map(|d| d.max(DAY)).unwrap_or(DAY),
            (s, Some(e)) if !e.is_date() => (instant(e, zone) - instant(s, zone)).max(0),
            _ => self.duration.unwrap_or(3600),
        }
    }

    /// Heure murale de départ dans le fuseau de l'événement.
    pub fn wall_start(&self) -> DateTime {
        match self.start {
            When::Date(d) => d.midnight(),
            When::Local(dt) => dt,
            When::Utc(t) => DateTime::from_secs(t),
        }
    }

    pub fn search_text(&self) -> String {
        format!(
            "{} {} {} {} {}",
            self.title,
            self.location.as_deref().unwrap_or(""),
            self.tags.join(" "),
            self.calendar.as_deref().unwrap_or(""),
            self.body
        )
    }
}

/// Instant d'une valeur temporelle dans un fuseau.
pub fn instant(w: When, zone: &Tz) -> i64 {
    match w {
        When::Date(d) => zone.to_utc(d.midnight()),
        When::Local(dt) => zone.to_utc(dt),
        When::Utc(t) => t,
    }
}

impl Task {
    pub fn from_doc(id: &str, doc: &Doc) -> Result<Task, String> {
        let mut status = opt(doc, "status").unwrap_or_else(|| "todo".into()).to_lowercase();
        if !STATUSES.contains(&status.as_str()) {
            status = match status.as_str() {
                "à faire" | "a faire" | "open" | "needs-action" => "todo",
                "en cours" | "in-process" | "in progress" => "doing",
                "fait" | "fini" | "terminé" | "completed" => "done",
                "annulé" => "cancelled",
                "attente" | "en attente" => "waiting",
                _ => "todo",
            }
            .into();
        }
        let due = match opt(doc, "due") {
            Some(s) => Some(When::parse(&s).ok_or_else(|| format!("due illisible : {s}"))?),
            None => None,
        };
        let priority = opt(doc, "priority").map(|p| normalize_priority(&p));
        Ok(Task {
            id: id.to_string(),
            title: opt(doc, "title").unwrap_or_else(|| "(sans titre)".into()),
            status,
            due,
            priority,
            calendar: opt(doc, "calendar"),
            tags: doc.get("tags").as_list(),
            done_at: opt(doc, "done_at"),
            repeat: match opt(doc, "repeat") {
                Some(r) => Some(RRule::parse(&r)?),
                None => None,
            },
            alarm: parse_alarms(&doc.get("alarm")),
            body: doc.body.clone(),
        })
    }

    pub fn is_open(&self) -> bool {
        matches!(self.status.as_str(), "todo" | "doing" | "waiting")
    }

    pub fn due_date(&self) -> Option<Date> {
        self.due.map(|w| match w {
            When::Date(d) => d,
            When::Local(dt) => dt.date,
            When::Utc(t) => DateTime::from_secs(t).date,
        })
    }

    pub fn search_text(&self) -> String {
        format!("{} {} {} {}", self.title, self.tags.join(" "), self.calendar.as_deref().unwrap_or(""), self.body)
    }

    pub fn to_json(&self, local: &Tz) -> Json {
        let due = self.due.map(|w| match w {
            When::Utc(t) => local.to_local(t).to_string(),
            o => o.to_file_string(),
        });
        Json::obj()
            .set("id", &self.id)
            .set("kind", "task")
            .set("title", &self.title)
            .set("status", &self.status)
            .set("due", due)
            .set("priority", self.priority.clone())
            .set("calendar", self.calendar.clone())
            .set("tags", self.tags.clone())
            .set("done_at", self.done_at.clone())
            .set("repeat", self.repeat.as_ref().map(|r| r.to_rule_string()))
            .set("body", &self.body)
    }
}

pub fn normalize_priority(p: &str) -> String {
    match p.trim().to_lowercase().as_str() {
        "high" | "haute" | "h" | "1" | "urgent" | "urgente" | "!" | "!!!" | "a" => "high",
        "low" | "basse" | "b" | "3" | "faible" | "c" => "low",
        _ => "medium",
    }
    .into()
}

impl Calendar {
    pub fn from_doc(name: &str, doc: &Doc) -> Calendar {
        let color = opt(doc, "color")
            .filter(|c| {
                c.starts_with('#') && (c.len() == 7 || c.len() == 4) && c[1..].chars().all(|x| x.is_ascii_hexdigit())
            })
            .unwrap_or_else(|| default_color(name).to_string());
        Calendar {
            name: name.to_string(),
            title: opt(doc, "title").unwrap_or_else(|| name.to_string()),
            color,
            alarm: parse_alarms(&doc.get("alarm")).unwrap_or_default(),
            url: opt(doc, "url"),
            hidden: matches!(opt(doc, "hidden").as_deref(), Some("true" | "yes" | "oui")),
        }
    }

    pub fn default_named(name: &str) -> Calendar {
        Calendar {
            name: name.into(),
            title: name.into(),
            color: default_color(name).into(),
            alarm: vec![],
            url: None,
            hidden: false,
        }
    }

    pub fn to_json(&self) -> Json {
        Json::obj()
            .set("name", &self.name)
            .set("title", &self.title)
            .set("color", &self.color)
            .set("alarm", self.alarm.iter().map(|a| crate::date::format_duration(*a)).collect::<Vec<_>>())
            .set("url", self.url.clone())
            .set("hidden", self.hidden)
            .set("readonly", self.url.is_some())
    }
}

/// Nom de fichier lisible : `rendez-vous-dentiste`.
pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    for c in crate::search::fold(s).chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
        if out.len() >= 48 {
            break;
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "element".into()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evenement() {
        let d = Doc::parse("---\ntitle: Sport\nstart: 2026-09-22 18:30\nend: 2026-09-22 20:00\nrepeat: FREQ=WEEKLY;BYDAY=TU\nexcept: [2026-10-06]\nalarm: [1h, 15m]\ntags: sport\n---\n");
        let e = Event::from_doc("events/x.md", &d).unwrap();
        assert_eq!(e.duration_secs(&Tz::utc()), 5400);
        assert_eq!(e.alarm, Some(vec![900, 3600]));
        assert_eq!(e.tags, vec!["sport"]);
        assert_eq!(e.except.len(), 1);
        assert!(e.repeat.is_some());
        let d = Doc::parse("---\ntitle: Vacances\nstart: 2026-10-20\nend: 2026-10-24\nalarm: none\n---\n");
        let e = Event::from_doc("x", &d).unwrap();
        assert!(e.all_day());
        assert_eq!(e.duration_secs(&Tz::utc()), 5 * DAY);
        assert_eq!(e.alarm, Some(vec![]));
        assert!(Event::from_doc("x", &Doc::parse("---\ntitle: x\n---\n")).is_err());
    }

    #[test]
    fn tache() {
        let d = Doc::parse("---\ntitle: Rapport\nstatus: fait\npriority: haute\ndue: 2026-09-25\n---\n");
        let t = Task::from_doc("tasks/r.md", &d).unwrap();
        assert_eq!(t.status, "done");
        assert_eq!(t.priority.as_deref(), Some("high"));
        assert_eq!(t.due_date(), Date::new(2026, 9, 25));
    }

    #[test]
    fn slug() {
        assert_eq!(slugify("Réunion d'équipe : budget 2027 !"), "reunion-d-equipe-budget-2027");
        assert_eq!(slugify("!!!"), "element");
    }
}
