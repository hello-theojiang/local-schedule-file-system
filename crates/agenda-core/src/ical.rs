//! iCalendar (RFC 5545) : lecture de VEVENT/VTODO et écriture d'un flux complet.
//!
//! La lecture produit des « brouillons » JSON dont les clés sont exactement celles
//! des en-têtes YAML (`title`, `start`, `repeat`…), pour réutiliser la même voie de
//! création que le reste de l'application.

use crate::date::{format_duration, parse_duration, Date, DateTime, When, DAY};
use crate::json::Json;
use crate::model::{Event, Task};
use crate::tz::Tz;

#[derive(Debug, Clone)]
struct Prop {
    name: String,
    params: Vec<(String, String)>,
    value: String,
}

impl Prop {
    fn param(&self, k: &str) -> Option<&str> {
        self.params.iter().find(|(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, Default)]
struct Comp {
    name: String,
    props: Vec<Prop>,
    children: Vec<Comp>,
}

impl Comp {
    fn get(&self, n: &str) -> Option<&Prop> {
        self.props.iter().find(|p| p.name == n)
    }
    fn all<'a>(&'a self, n: &'a str) -> impl Iterator<Item = &'a Prop> + 'a {
        self.props.iter().filter(move |p| p.name == n)
    }
}

fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let l = raw.strip_suffix('\r').unwrap_or(raw);
        if (l.starts_with(' ') || l.starts_with('\t')) && !lines.is_empty() {
            if let Some(last) = lines.last_mut() {
                last.push_str(&l[1..]);
            }
        } else if !l.is_empty() {
            lines.push(l.to_string());
        }
    }
    lines
}

fn parse_line(l: &str) -> Option<Prop> {
    let mut in_q = false;
    let mut colon = None;
    for (i, c) in l.char_indices() {
        match c {
            '"' => in_q = !in_q,
            ':' if !in_q => {
                colon = Some(i);
                break;
            }
            _ => {}
        }
    }
    let colon = colon?;
    let (head, value) = (&l[..colon], &l[colon + 1..]);
    let mut parts = Vec::new();
    let mut cur = String::new();
    in_q = false;
    for c in head.chars() {
        match c {
            '"' => in_q = !in_q,
            ';' if !in_q => {
                parts.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    parts.push(cur);
    let name = parts.first()?.to_ascii_uppercase();
    let params = parts[1..]
        .iter()
        .filter_map(|p| p.split_once('=').map(|(k, v)| (k.to_ascii_uppercase(), v.trim_matches('"').to_string())))
        .collect();
    Some(Prop { name, params, value: value.to_string() })
}

fn parse_comps(text: &str) -> Vec<Comp> {
    let mut stack: Vec<Comp> = vec![Comp::default()];
    for l in unfold(text) {
        let Some(p) = parse_line(&l) else { continue };
        match p.name.as_str() {
            "BEGIN" => stack.push(Comp { name: p.value.trim().to_ascii_uppercase(), ..Default::default() }),
            "END" => {
                if stack.len() > 1 {
                    let c = stack.pop().unwrap_or_default();
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(c);
                    }
                }
            }
            _ => {
                if let Some(top) = stack.last_mut() {
                    top.props.push(p);
                }
            }
        }
    }
    while stack.len() > 1 {
        let c = stack.pop().unwrap_or_default();
        if let Some(parent) = stack.last_mut() {
            parent.children.push(c);
        }
    }
    stack.pop().map(|r| r.children).unwrap_or_default()
}

pub fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace("\r\n", "\\n").replace('\n', "\\n")
}

/// Fuseaux Windows (Outlook, Exchange) les plus courants.
fn windows_tz(name: &str) -> Option<&'static str> {
    Some(match name {
        "Romance Standard Time" => "Europe/Paris",
        "W. Europe Standard Time" => "Europe/Berlin",
        "Central Europe Standard Time" => "Europe/Budapest",
        "Central European Standard Time" => "Europe/Warsaw",
        "GMT Standard Time" => "Europe/London",
        "E. Europe Standard Time" => "Europe/Bucharest",
        "Eastern Standard Time" => "America/New_York",
        "Central Standard Time" => "America/Chicago",
        "Mountain Standard Time" => "America/Denver",
        "Pacific Standard Time" => "America/Los_Angeles",
        "UTC" | "Coordinated Universal Time" => "UTC",
        _ => return None,
    })
}

fn clean_tzid(t: &str) -> String {
    let t = t.trim().trim_start_matches('/');
    if let Some(w) = windows_tz(t) {
        return w.to_string();
    }
    // « /mozilla.org/20050126_1/Europe/Paris » → « Europe/Paris »
    let parts: Vec<&str> = t.split('/').collect();
    if parts.len() > 2 {
        return parts[parts.len() - 2..].join("/");
    }
    t.to_string()
}

/// Valeur de date iCalendar → (When, fuseau éventuel).
fn parse_dt(p: &Prop) -> Option<(When, Option<String>)> {
    let v = p.value.trim();
    let v = v.split(',').next().unwrap_or(v);
    if p.param("VALUE").map(|x| x.eq_ignore_ascii_case("DATE")).unwrap_or(false) || v.len() == 8 {
        return Date::parse(v).map(|d| (When::Date(d), None));
    }
    if let Some(stripped) = v.strip_suffix('Z') {
        return DateTime::parse(stripped).map(|dt| (When::Utc(dt.secs()), None));
    }
    let dt = DateTime::parse(v)?;
    let tz = p.param("TZID").map(clean_tzid).filter(|t| t != "UTC");
    if p.param("TZID").map(clean_tzid).as_deref() == Some("UTC") {
        return Some((When::Utc(dt.secs()), None));
    }
    Some((When::Local(dt), tz))
}

fn dt_list(p: &Prop) -> Vec<When> {
    p.value
        .split(',')
        .filter_map(|v| {
            let q = Prop { name: p.name.clone(), params: p.params.clone(), value: v.to_string() };
            parse_dt(&q).map(|x| x.0)
        })
        .collect()
}

fn alarms(c: &Comp) -> Vec<String> {
    let mut out = Vec::new();
    for a in c.children.iter().filter(|x| x.name == "VALARM") {
        if let Some(t) = a.get("TRIGGER") {
            if t.param("VALUE").map(|v| v.eq_ignore_ascii_case("DATE-TIME")).unwrap_or(false) {
                continue;
            }
            let v = t.value.trim();
            if v.starts_with('+') || (!v.starts_with('-') && v != "PT0S" && v != "P0D") {
                continue; // rappel après le début : non géré
            }
            if let Some(d) = parse_duration(v) {
                out.push(format_duration(d));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn text(c: &Comp, n: &str) -> Option<String> {
    c.get(n).map(|p| unescape(p.value.trim())).filter(|s| !s.is_empty())
}

fn categories(c: &Comp) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for p in c.all("CATEGORIES") {
        for x in p.value.split(',') {
            let t = unescape(x.trim());
            if !t.is_empty() && !v.contains(&t) {
                v.push(t);
            }
        }
    }
    v
}

fn when_str(w: When, zone: Option<&str>) -> (String, Option<String>) {
    (w.to_file_string(), zone.map(str::to_string))
}

/// Analyse un calendrier iCalendar. Chaque élément est un brouillon JSON
/// `{kind: "event"|"task", title, start, …}` avec les clés du format de fichier.
pub fn parse(src: &str) -> Vec<Json> {
    let comps = parse_comps(src);
    let mut out = Vec::new();
    let cals: Vec<&Comp> = comps.iter().filter(|c| c.name == "VCALENDAR").collect();
    let all: Vec<&Comp> =
        if cals.is_empty() { comps.iter().collect() } else { cals.iter().flat_map(|c| c.children.iter()).collect() };
    // exceptions portées par des occurrences modifiées (RECURRENCE-ID)
    let mut overrides: Vec<(String, When)> = Vec::new();
    for c in &all {
        if c.name == "VEVENT" {
            if let (Some(uid), Some(rid)) = (text(c, "UID"), c.get("RECURRENCE-ID").and_then(parse_dt)) {
                overrides.push((uid, rid.0));
            }
        }
    }
    for c in all {
        match c.name.as_str() {
            "VEVENT" => {
                let Some((start, tz)) = c.get("DTSTART").and_then(parse_dt) else { continue };
                let mut j = Json::obj()
                    .set("kind", "event")
                    .set("title", text(c, "SUMMARY").unwrap_or_else(|| "(sans titre)".into()));
                let (s, zone) = when_str(start, tz.as_deref());
                j.insert("start", s);
                let end = c.get("DTEND").and_then(parse_dt).map(|x| x.0);
                match (start, end) {
                    (When::Date(sd), Some(When::Date(ed))) => {
                        // DTEND exclusif en iCalendar → dernier jour inclus dans nos fichiers
                        let last = ed.add_days(-1);
                        if last > sd {
                            j.insert("end", last.to_string());
                        }
                    }
                    (_, Some(e)) => j.insert("end", e.to_file_string()),
                    (_, None) => {
                        if let Some(d) = c.get("DURATION").and_then(|p| parse_duration(&p.value)) {
                            if start.is_date() {
                                if d > DAY {
                                    if let When::Date(sd) = start {
                                        j.insert("end", sd.add_days(d / DAY - 1).to_string());
                                    }
                                }
                            } else {
                                j.insert("duration", format_duration(d));
                            }
                        }
                    }
                }
                if let Some(z) = zone {
                    j.insert("tz", z);
                }
                if let Some(r) = text(c, "RRULE") {
                    j.insert("repeat", r);
                }
                let uid = text(c, "UID");
                let mut ex: Vec<String> = c.all("EXDATE").flat_map(dt_list).map(|w| w.to_file_string()).collect();
                if c.get("RECURRENCE-ID").is_none() {
                    if let Some(u) = &uid {
                        ex.extend(overrides.iter().filter(|(ou, _)| ou == u).map(|(_, w)| w.to_file_string()));
                    }
                }
                if !ex.is_empty() && j.has("repeat") {
                    j.insert("except", ex);
                }
                if let Some(v) = text(c, "LOCATION") {
                    j.insert("location", v);
                }
                let cats = categories(c);
                if !cats.is_empty() {
                    j.insert("tags", cats);
                }
                let al = alarms(c);
                if !al.is_empty() {
                    j.insert("alarm", al);
                }
                if let Some(st) = text(c, "STATUS") {
                    let st = st.to_lowercase();
                    if st == "cancelled" || st == "tentative" {
                        j.insert("status", st);
                    }
                }
                if let Some(u) = uid {
                    // une occurrence modifiée garde un UID distinct pour ne pas écraser la série
                    let u = match c.get("RECURRENCE-ID").and_then(parse_dt) {
                        Some((rid, _)) => format!("{u}#{}", rid.to_file_string()),
                        None => u,
                    };
                    j.insert("uid", u);
                }
                j.insert("body", text(c, "DESCRIPTION").map(|d| d + "\n").unwrap_or_default());
                out.push(j);
            }
            "VTODO" => {
                let mut j = Json::obj()
                    .set("kind", "task")
                    .set("title", text(c, "SUMMARY").unwrap_or_else(|| "(sans titre)".into()));
                let status = match text(c, "STATUS").unwrap_or_default().to_uppercase().as_str() {
                    "COMPLETED" => "done",
                    "IN-PROCESS" => "doing",
                    "CANCELLED" => "cancelled",
                    _ => "todo",
                };
                j.insert("status", status);
                if let Some((d, _)) = c.get("DUE").and_then(parse_dt) {
                    j.insert("due", d.to_file_string());
                }
                if let Some(p) = text(c, "PRIORITY").and_then(|p| p.parse::<u32>().ok()).filter(|p| *p > 0) {
                    j.insert(
                        "priority",
                        if p <= 4 {
                            "high"
                        } else if p == 5 {
                            "medium"
                        } else {
                            "low"
                        },
                    );
                }
                if let Some((d, _)) = c.get("COMPLETED").and_then(parse_dt) {
                    j.insert("done_at", d.to_file_string());
                }
                let cats = categories(c);
                if !cats.is_empty() {
                    j.insert("tags", cats);
                }
                if let Some(u) = text(c, "UID") {
                    j.insert("uid", u);
                }
                j.insert("body", text(c, "DESCRIPTION").map(|d| d + "\n").unwrap_or_default());
                out.push(j);
            }
            _ => {}
        }
    }
    out
}

// ------------------------------------------------------------------ écriture

fn fold_line(out: &mut String, line: &str) {
    let mut len = 0;
    for c in line.chars() {
        let cl = c.len_utf8();
        if len + cl > 74 {
            out.push_str("\r\n ");
            len = 1;
        }
        out.push(c);
        len += cl;
    }
    out.push_str("\r\n");
}

fn fmt_dt(key: &str, w: When, tz: Option<&str>) -> String {
    match w {
        When::Date(d) => format!("{key};VALUE=DATE:{}", d.compact()),
        When::Local(dt) => {
            let v = format!("{}T{:02}{:02}{:02}", dt.date.compact(), dt.hour(), dt.minute(), dt.sec % 60);
            match tz {
                Some(z) if z.contains('/') => format!("{key};TZID={z}:{v}"),
                _ => format!("{key}:{v}"),
            }
        }
        When::Utc(t) => {
            let dt = DateTime::from_secs(t);
            format!("{key}:{}T{:02}{:02}{:02}Z", dt.date.compact(), dt.hour(), dt.minute(), dt.sec % 60)
        }
    }
}

fn stamp(now: i64) -> String {
    fmt_dt("DTSTAMP", When::Utc(now), None)
}

fn uid_for(id: &str, uid: &Option<String>) -> String {
    uid.clone().unwrap_or_else(|| format!("{:016x}@agenda.localfirst.dev", crate::store::hash(id.as_bytes())))
}

pub fn export(events: &[&Event], tasks: &[&Task], local: &Tz, name: &str, now: i64) -> String {
    let mut s = String::new();
    for l in ["BEGIN:VCALENDAR", "VERSION:2.0", "PRODID:-//localfirst//Agenda//FR", "CALSCALE:GREGORIAN"] {
        fold_line(&mut s, l);
    }
    fold_line(&mut s, &format!("X-WR-CALNAME:{}", escape(name)));
    if local.name.contains('/') {
        fold_line(&mut s, &format!("X-WR-TIMEZONE:{}", local.name));
    }
    let local_name = local.name.contains('/').then_some(local.name.as_str());
    for e in events {
        let zone = e.tz.as_deref().or(local_name);
        fold_line(&mut s, "BEGIN:VEVENT");
        fold_line(&mut s, &format!("UID:{}", escape(&uid_for(&e.id, &e.uid))));
        fold_line(&mut s, &stamp(now));
        fold_line(&mut s, &format!("SUMMARY:{}", escape(&e.title)));
        fold_line(&mut s, &fmt_dt("DTSTART", e.start, zone));
        match (e.start, e.end) {
            (When::Date(sd), end) => {
                let last = match end {
                    Some(When::Date(ed)) if ed >= sd => ed,
                    _ => sd,
                };
                fold_line(&mut s, &fmt_dt("DTEND", When::Date(last.add_days(1)), None));
            }
            (_, Some(end)) => fold_line(&mut s, &fmt_dt("DTEND", end, zone)),
            (_, None) => fold_line(&mut s, &format!("DURATION:PT{}S", e.duration.unwrap_or(3600))),
        }
        if let Some(r) = &e.repeat {
            fold_line(&mut s, &format!("RRULE:{}", r.to_rule_string()));
            for x in &e.except {
                // les exceptions sans heure désignent l'occurrence de ce jour-là
                let w = match (*x, e.start) {
                    (When::Date(d), When::Local(st)) => When::Local(DateTime { date: d, sec: st.sec }),
                    (w, _) => w,
                };
                fold_line(&mut s, &fmt_dt("EXDATE", w, zone));
            }
        }
        if let Some(l) = &e.location {
            fold_line(&mut s, &format!("LOCATION:{}", escape(l)));
        }
        if !e.tags.is_empty() {
            let t: Vec<String> = e.tags.iter().map(|t| escape(t)).collect();
            fold_line(&mut s, &format!("CATEGORIES:{}", t.join(",")));
        }
        let body = e.body.trim();
        if !body.is_empty() {
            fold_line(&mut s, &format!("DESCRIPTION:{}", escape(body)));
        }
        if e.status != "confirmed" {
            fold_line(&mut s, &format!("STATUS:{}", e.status.to_uppercase()));
        }
        for a in e.alarm.clone().unwrap_or_default() {
            fold_line(&mut s, "BEGIN:VALARM");
            fold_line(&mut s, "ACTION:DISPLAY");
            fold_line(&mut s, &format!("DESCRIPTION:{}", escape(&e.title)));
            fold_line(&mut s, &format!("TRIGGER:-PT{}M", a / 60));
            fold_line(&mut s, "END:VALARM");
        }
        fold_line(&mut s, "END:VEVENT");
    }
    for t in tasks {
        fold_line(&mut s, "BEGIN:VTODO");
        fold_line(&mut s, &format!("UID:{}", escape(&uid_for(&t.id, &None))));
        fold_line(&mut s, &stamp(now));
        fold_line(&mut s, &format!("SUMMARY:{}", escape(&t.title)));
        if let Some(d) = t.due {
            fold_line(&mut s, &fmt_dt("DUE", d, local_name));
        }
        let st = match t.status.as_str() {
            "done" => "COMPLETED",
            "doing" => "IN-PROCESS",
            "cancelled" => "CANCELLED",
            _ => "NEEDS-ACTION",
        };
        fold_line(&mut s, &format!("STATUS:{st}"));
        if let Some(p) = &t.priority {
            fold_line(
                &mut s,
                &format!(
                    "PRIORITY:{}",
                    if p == "high" {
                        1
                    } else if p == "low" {
                        9
                    } else {
                        5
                    }
                ),
            );
        }
        if !t.tags.is_empty() {
            let tg: Vec<String> = t.tags.iter().map(|x| escape(x)).collect();
            fold_line(&mut s, &format!("CATEGORIES:{}", tg.join(",")));
        }
        let body = t.body.trim();
        if !body.is_empty() {
            fold_line(&mut s, &format!("DESCRIPTION:{}", escape(body)));
        }
        fold_line(&mut s, "END:VTODO");
    }
    fold_line(&mut s, "END:VCALENDAR");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yaml::Doc;

    const ICS: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTIMEZONE\r\nTZID:Europe/Paris\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nUID:abc@x\r\nSUMMARY:Cours de\r\n  maths\\, salle 2\r\nDTSTART;TZID=Europe/Paris:20260922T083000\r\nDTEND;TZID=Europe/Paris:20260922T100000\r\nRRULE:FREQ=WEEKLY;BYDAY=TU\r\nEXDATE;TZID=Europe/Paris:20260929T083000,20261006T083000\r\nLOCATION:Amphi A\r\nCATEGORIES:cours,maths\r\nBEGIN:VALARM\r\nTRIGGER:-PT15M\r\nACTION:DISPLAY\r\nEND:VALARM\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:abc@x\r\nRECURRENCE-ID;TZID=Europe/Paris:20261013T083000\r\nSUMMARY:Cours déplacé\r\nDTSTART;TZID=Europe/Paris:20261013T140000\r\nDTEND;TZID=Europe/Paris:20261013T153000\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nSUMMARY:Vacances\r\nDTSTART;VALUE=DATE:20261020\r\nDTEND;VALUE=DATE:20261025\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nSUMMARY:Appel\r\nDTSTART:20261001T120000Z\r\nDURATION:PT30M\r\nEND:VEVENT\r\nBEGIN:VTODO\r\nSUMMARY:Rendre DM\r\nDUE;VALUE=DATE:20261002\r\nPRIORITY:1\r\nSTATUS:NEEDS-ACTION\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";

    #[test]
    fn lecture() {
        let items = parse(ICS);
        assert_eq!(items.len(), 5);
        let c = &items[0];
        assert_eq!(c.get("title").as_str(), Some("Cours de maths, salle 2"));
        assert_eq!(c.get("start").as_str(), Some("2026-09-22 08:30"));
        assert_eq!(c.get("tz").as_str(), Some("Europe/Paris"));
        assert_eq!(c.get("repeat").as_str(), Some("FREQ=WEEKLY;BYDAY=TU"));
        assert_eq!(c.get("except").str_list().unwrap(), ["2026-09-29 08:30", "2026-10-06 08:30", "2026-10-13 08:30"]);
        assert_eq!(c.get("alarm").str_list().unwrap(), ["15m"]);
        assert_eq!(c.get("tags").str_list().unwrap(), ["cours", "maths"]);
        assert_eq!(items[1].get("uid").as_str(), Some("abc@x#2026-10-13 08:30"));
        let v = &items[2];
        assert_eq!(v.get("start").as_str(), Some("2026-10-20"));
        assert_eq!(v.get("end").as_str(), Some("2026-10-24"));
        assert_eq!(items[3].get("start").as_str(), Some("2026-10-01T12:00:00Z"));
        assert_eq!(items[3].get("duration").as_str(), Some("30m"));
        assert_eq!(items[4].get("kind").as_str(), Some("task"));
        assert_eq!(items[4].get("priority").as_str(), Some("high"));
    }

    #[test]
    fn aller_retour() {
        let src = "---\ntitle: Sport, muscu; cardio\nstart: 2026-09-22 18:30\nend: 2026-09-22 20:00\nrepeat: FREQ=WEEKLY;BYDAY=TU\nexcept: [2026-10-06]\nalarm: [15m]\ntags: [sport]\nlocation: Gymnase\n---\nNotes\nsur deux lignes\n";
        let e = Event::from_doc("events/2026-09/sport.md", &Doc::parse(src)).unwrap();
        let v = Event::from_doc(
            "events/v.md",
            &Doc::parse("---\ntitle: Vacances\nstart: 2026-10-20\nend: 2026-10-24\n---\n"),
        )
        .unwrap();
        let tz = Tz::fixed("Europe/Paris", 7200);
        let out = export(&[&e, &v], &[], &tz, "Agenda", 0);
        assert!(out.lines().all(|l| l.len() <= 76));
        assert!(out.contains("DTSTART;TZID=Europe/Paris:20260922T183000"));
        assert!(out.contains("EXDATE;TZID=Europe/Paris:20261006T183000"));
        assert!(out.contains("DTEND;VALUE=DATE:20261025"));
        let back = parse(&out);
        assert_eq!(back[0].get("title").as_str(), Some("Sport, muscu; cardio"));
        assert_eq!(back[0].get("end").as_str(), Some("2026-09-22 20:00"));
        assert_eq!(back[0].get("body").as_str(), Some("Notes\nsur deux lignes\n"));
        assert_eq!(back[1].get("end").as_str(), Some("2026-10-24"));
    }

    #[test]
    fn repliement_utf8() {
        let mut s = String::new();
        fold_line(&mut s, &format!("SUMMARY:{}", "é".repeat(100)));
        assert!(s.split("\r\n").all(|l| l.len() <= 75));
        assert_eq!(unfold(&s)[0], format!("SUMMARY:{}", "é".repeat(100)));
    }
}
