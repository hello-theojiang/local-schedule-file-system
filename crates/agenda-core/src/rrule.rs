//! Règles de répétition iCalendar (RFC 5545, sous-ensemble courant).
//!
//! L'expansion se fait en heure murale : « tous les mardis à 18h30 » reste à 18h30
//! après un changement d'heure.

use crate::date::{add_months_ym, days_in_month, Date, DateTime, When};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RRule {
    pub freq: Freq,
    pub interval: u32,
    pub count: Option<u32>,
    pub until: Option<When>,
    /// (rang, jour) ; rang 0 = tous ; jour 0 = lundi
    pub byday: Vec<(i32, u32)>,
    pub bymonthday: Vec<i32>,
    pub bymonth: Vec<u32>,
    pub bysetpos: Vec<i32>,
    pub byhour: Vec<u32>,
    pub byminute: Vec<u32>,
    pub wkst: u32,
}

pub const DAY_CODES: [&str; 7] = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];

fn list<T: std::str::FromStr>(v: &str) -> Result<Vec<T>, String> {
    v.split(',').map(|x| x.trim().parse::<T>().map_err(|_| format!("valeur invalide : {x}"))).collect()
}

impl RRule {
    pub fn parse(s: &str) -> Result<RRule, String> {
        let s = s.trim();
        let s = s.strip_prefix("RRULE:").unwrap_or(s);
        let mut r = RRule {
            freq: Freq::Daily,
            interval: 1,
            count: None,
            until: None,
            byday: vec![],
            bymonthday: vec![],
            bymonth: vec![],
            bysetpos: vec![],
            byhour: vec![],
            byminute: vec![],
            wkst: 0,
        };
        let mut freq = None;
        for part in s.split(';').filter(|p| !p.trim().is_empty()) {
            let (k, v) = part.split_once('=').ok_or_else(|| format!("RRULE : « {part} » sans « = »"))?;
            let v = v.trim();
            match k.trim().to_ascii_uppercase().as_str() {
                "FREQ" => {
                    freq = Some(match v.to_ascii_uppercase().as_str() {
                        "DAILY" => Freq::Daily,
                        "WEEKLY" => Freq::Weekly,
                        "MONTHLY" => Freq::Monthly,
                        "YEARLY" => Freq::Yearly,
                        o => return Err(format!("RRULE : fréquence non gérée : {o}")),
                    })
                }
                "INTERVAL" => r.interval = v.parse().ok().filter(|&n| n >= 1).ok_or("RRULE : INTERVAL invalide")?,
                "COUNT" => r.count = Some(v.parse().map_err(|_| "RRULE : COUNT invalide")?),
                "UNTIL" => r.until = Some(parse_until(v).ok_or("RRULE : UNTIL invalide")?),
                "BYDAY" => {
                    for d in v.split(',') {
                        let d = d.trim().to_ascii_uppercase();
                        if d.len() < 2 {
                            return Err(format!("RRULE : BYDAY invalide : {d}"));
                        }
                        let (ord, code) = d.split_at(d.len() - 2);
                        let wd = DAY_CODES
                            .iter()
                            .position(|c| *c == code)
                            .ok_or_else(|| format!("RRULE : jour inconnu : {code}"))?
                            as u32;
                        let ord: i32 = if ord.is_empty() {
                            0
                        } else {
                            ord.trim_start_matches('+').parse().map_err(|_| "RRULE : rang BYDAY invalide")?
                        };
                        r.byday.push((ord, wd));
                    }
                }
                "BYMONTHDAY" => r.bymonthday = list(v)?,
                "BYMONTH" => r.bymonth = list(v)?,
                "BYSETPOS" => r.bysetpos = list(v)?,
                "BYHOUR" => r.byhour = list(v)?,
                "BYMINUTE" => r.byminute = list(v)?,
                "WKST" => r.wkst = DAY_CODES.iter().position(|c| c.eq_ignore_ascii_case(v)).unwrap_or(0) as u32,
                // BYSECOND, BYYEARDAY, BYWEEKNO… : ignorés (rares dans les agendas personnels)
                _ => {}
            }
        }
        r.freq = freq.ok_or("RRULE : FREQ manquant")?;
        if r.bymonth.iter().any(|m| !(1..=12).contains(m)) || r.bymonthday.iter().any(|d| *d == 0 || d.abs() > 31) {
            return Err("RRULE : valeur hors limites".into());
        }
        Ok(r)
    }

    pub fn to_rule_string(&self) -> String {
        let mut p = vec![format!(
            "FREQ={}",
            match self.freq {
                Freq::Daily => "DAILY",
                Freq::Weekly => "WEEKLY",
                Freq::Monthly => "MONTHLY",
                Freq::Yearly => "YEARLY",
            }
        )];
        if self.interval > 1 {
            p.push(format!("INTERVAL={}", self.interval));
        }
        if !self.byday.is_empty() {
            let d: Vec<String> = self
                .byday
                .iter()
                .map(|(o, d)| {
                    if *o == 0 {
                        DAY_CODES[*d as usize].to_string()
                    } else {
                        format!("{o}{}", DAY_CODES[*d as usize])
                    }
                })
                .collect();
            p.push(format!("BYDAY={}", d.join(",")));
        }
        let join = |v: &[i32]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",");
        let joinu = |v: &[u32]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",");
        if !self.bymonthday.is_empty() {
            p.push(format!("BYMONTHDAY={}", join(&self.bymonthday)));
        }
        if !self.bymonth.is_empty() {
            p.push(format!("BYMONTH={}", joinu(&self.bymonth)));
        }
        if !self.bysetpos.is_empty() {
            p.push(format!("BYSETPOS={}", join(&self.bysetpos)));
        }
        if !self.byhour.is_empty() {
            p.push(format!("BYHOUR={}", joinu(&self.byhour)));
        }
        if !self.byminute.is_empty() {
            p.push(format!("BYMINUTE={}", joinu(&self.byminute)));
        }
        if let Some(c) = self.count {
            p.push(format!("COUNT={c}"));
        }
        if let Some(u) = self.until {
            p.push(format!(
                "UNTIL={}",
                match u {
                    When::Date(d) => d.compact(),
                    When::Local(dt) =>
                        format!("{}T{:02}{:02}{:02}", dt.date.compact(), dt.hour(), dt.minute(), dt.sec % 60),
                    When::Utc(t) => {
                        let dt = DateTime::from_secs(t);
                        format!("{}T{:02}{:02}{:02}Z", dt.date.compact(), dt.hour(), dt.minute(), dt.sec % 60)
                    }
                }
            ));
        }
        p.join(";")
    }

    /// Candidats (dates) d'une période donnée, avant BYSETPOS.
    fn period_dates(&self, start: Date, k: i64) -> Vec<Date> {
        let mut out = Vec::new();
        match self.freq {
            Freq::Daily => {
                let d = start.add_days(k * self.interval as i64);
                if self.day_filter(d) {
                    out.push(d);
                }
            }
            Freq::Weekly => {
                let ws = start.week_start(self.wkst).add_days(k * 7 * self.interval as i64);
                for i in 0..7 {
                    let d = ws.add_days(i);
                    let ok_day = if self.byday.is_empty() {
                        d.weekday() == start.weekday()
                    } else {
                        self.byday.iter().any(|(_, w)| *w == d.weekday())
                    };
                    if ok_day && (self.bymonth.is_empty() || self.bymonth.contains(&d.m)) {
                        out.push(d);
                    }
                }
            }
            Freq::Monthly => {
                let (y, m) = add_months_ym(start.y, start.m, k * self.interval as i64);
                if self.bymonth.is_empty() || self.bymonth.contains(&m) {
                    self.month_dates(y, m, start.d, &mut out);
                }
            }
            Freq::Yearly => {
                let y = start.y + (k * self.interval as i64) as i32;
                if self.bymonth.is_empty() && !self.byday.is_empty() && self.bymonthday.is_empty() {
                    // BYDAY avec rang relatif à l'année (ex. 20MO)
                    let first = Date { y, m: 1, d: 1 };
                    let n = if crate::date::is_leap(y) { 366 } else { 365 };
                    let all: Vec<Date> = (0..n).map(|i| first.add_days(i)).collect();
                    out = select_byday(&all, &self.byday);
                } else {
                    let months: Vec<u32> = if self.bymonth.is_empty() { vec![start.m] } else { self.bymonth.clone() };
                    for m in months {
                        self.month_dates(y, m, start.d, &mut out);
                    }
                }
            }
        }
        out.sort();
        out.dedup();
        if !self.bysetpos.is_empty() && !out.is_empty() {
            let n = out.len() as i32;
            let mut sel: Vec<Date> = self
                .bysetpos
                .iter()
                .filter_map(|&p| {
                    let i = if p > 0 { p - 1 } else { n + p };
                    (0..n).contains(&i).then(|| out[i as usize])
                })
                .collect();
            sel.sort();
            sel.dedup();
            out = sel;
        }
        out
    }

    fn day_filter(&self, d: Date) -> bool {
        (self.bymonth.is_empty() || self.bymonth.contains(&d.m))
            && (self.bymonthday.is_empty() || self.bymonthday.iter().any(|&md| resolve_md(d.y, d.m, md) == Some(d.d)))
            && (self.byday.is_empty() || self.byday.iter().any(|(_, w)| *w == d.weekday()))
    }

    fn month_dates(&self, y: i32, m: u32, default_day: u32, out: &mut Vec<Date>) {
        let dim = days_in_month(y, m);
        if self.bymonthday.is_empty() && self.byday.is_empty() {
            if default_day <= dim {
                out.push(Date { y, m, d: default_day });
            }
            return;
        }
        let all: Vec<Date> = (1..=dim).map(|d| Date { y, m, d }).collect();
        let by_md: Option<Vec<Date>> = (!self.bymonthday.is_empty())
            .then(|| self.bymonthday.iter().filter_map(|&md| resolve_md(y, m, md)).map(|d| Date { y, m, d }).collect());
        let by_wd: Option<Vec<Date>> = (!self.byday.is_empty()).then(|| select_byday(&all, &self.byday));
        match (by_md, by_wd) {
            (Some(a), Some(b)) => out.extend(a.into_iter().filter(|d| b.contains(d))),
            (Some(a), None) => out.extend(a),
            (None, Some(b)) => out.extend(b),
            (None, None) => {}
        }
    }

    fn times(&self, dtstart: DateTime) -> Vec<u32> {
        if self.byhour.is_empty() && self.byminute.is_empty() {
            return vec![dtstart.sec];
        }
        let hours = if self.byhour.is_empty() { vec![dtstart.hour()] } else { self.byhour.clone() };
        let mins = if self.byminute.is_empty() { vec![dtstart.minute()] } else { self.byminute.clone() };
        let mut t: Vec<u32> =
            hours.iter().flat_map(|h| mins.iter().map(move |m| h * 3600 + m * 60)).filter(|s| *s < 86_400).collect();
        t.sort();
        t.dedup();
        t
    }

    /// Occurrences (heures murales) dont le début est dans `[from, to)`.
    /// `until_local` : UNTIL converti en heure murale par l'appelant si c'est un instant UTC.
    pub fn between(
        &self,
        dtstart: DateTime,
        from: DateTime,
        to: DateTime,
        until_local: Option<DateTime>,
        max: usize,
    ) -> Vec<DateTime> {
        let mut out = Vec::new();
        let times = self.times(dtstart);
        let until = until_local.or(match self.until {
            Some(When::Date(d)) => Some(DateTime { date: d, sec: 86_399 }),
            Some(When::Local(dt)) => Some(dt),
            _ => None,
        });
        // Sans COUNT, on peut sauter directement près de la fenêtre.
        let mut k: i64 = 0;
        if self.count.is_none() && from > dtstart {
            let days = from.date.days() - dtstart.date.days();
            let periods = match self.freq {
                Freq::Daily => days / self.interval as i64,
                Freq::Weekly => days / (7 * self.interval as i64),
                Freq::Monthly => {
                    ((from.date.y - dtstart.date.y) as i64 * 12 + from.date.m as i64 - dtstart.date.m as i64)
                        / self.interval as i64
                }
                Freq::Yearly => (from.date.y - dtstart.date.y) as i64 / self.interval as i64,
            };
            k = (periods - 1).max(0);
        }
        let mut emitted: u32 = 0;
        let mut empty_run = 0u32;
        for _ in 0..200_000 {
            let dates = self.period_dates(dtstart.date, k);
            k += 1;
            if dates.is_empty() {
                empty_run += 1;
                // garde-fou : règle impossible (ex. 30 février)
                if empty_run > 5_000 {
                    break;
                }
                // la période est-elle déjà au-delà de la fenêtre ?
                if self.period_start(dtstart.date, k - 1) > to.date {
                    break;
                }
                continue;
            }
            empty_run = 0;
            for d in dates {
                for &sec in &times {
                    let occ = DateTime { date: d, sec };
                    if occ < dtstart {
                        continue;
                    }
                    if let Some(u) = until {
                        if occ > u {
                            return out;
                        }
                    }
                    if let Some(c) = self.count {
                        if emitted >= c {
                            return out;
                        }
                    }
                    emitted += 1;
                    if occ >= to {
                        return out;
                    }
                    if occ >= from {
                        out.push(occ);
                        if out.len() >= max {
                            return out;
                        }
                    }
                }
            }
        }
        out
    }

    fn period_start(&self, start: Date, k: i64) -> Date {
        match self.freq {
            Freq::Daily => start.add_days(k * self.interval as i64),
            Freq::Weekly => start.week_start(self.wkst).add_days(k * 7 * self.interval as i64),
            Freq::Monthly => {
                let (y, m) = add_months_ym(start.y, start.m, k * self.interval as i64);
                Date { y, m, d: 1 }
            }
            Freq::Yearly => Date { y: start.y + (k * self.interval as i64) as i32, m: 1, d: 1 },
        }
    }

    /// La répétition est-elle infinie ?
    pub fn is_infinite(&self) -> bool {
        self.count.is_none() && self.until.is_none()
    }

    /// Description courte en français : « chaque semaine le mardi ».
    pub fn describe(&self) -> String {
        let days: Vec<String> = self
            .byday
            .iter()
            .map(|(o, d)| {
                let name = crate::date::WEEKDAYS_FR[*d as usize];
                match o {
                    0 => name.to_string(),
                    1 => format!("1er {name}"),
                    -1 => format!("dernier {name}"),
                    n if *n > 0 => format!("{n}e {name}"),
                    n => format!("{}e {name} avant la fin", -n),
                }
            })
            .collect();
        let every = |one: &str, many: &str| {
            if self.interval == 1 {
                one.to_string()
            } else {
                format!("toutes les {} {many}", self.interval)
            }
        };
        let mut s = match self.freq {
            Freq::Daily => {
                if self.byday.len() == 5 && self.byday.iter().all(|(_, d)| *d < 5) {
                    "chaque jour de semaine".into()
                } else if self.interval == 1 {
                    "chaque jour".into()
                } else {
                    format!("tous les {} jours", self.interval)
                }
            }
            Freq::Weekly => every("chaque semaine", "semaines"),
            Freq::Monthly => every("chaque mois", "mois"),
            Freq::Yearly => every("chaque année", "années"),
        };
        if !(days.is_empty() || self.freq == Freq::Daily && s.contains("semaine")) {
            s.push_str(&format!(" le {}", days.join(", ")));
        }
        if !self.bymonthday.is_empty() {
            let d: Vec<String> =
                self.bymonthday.iter().map(|d| if *d == -1 { "dernier jour".into() } else { d.to_string() }).collect();
            s.push_str(&format!(" le {}", d.join(", ")));
        }
        if let Some(c) = self.count {
            s.push_str(&format!(", {c} fois"));
        }
        if let Some(u) = self.until {
            let d = match u {
                When::Date(d) => d,
                When::Local(dt) => dt.date,
                When::Utc(t) => DateTime::from_secs(t).date,
            };
            s.push_str(&format!(", jusqu'au {}", crate::date::human_date(d)));
        }
        s
    }
}

fn resolve_md(y: i32, m: u32, md: i32) -> Option<u32> {
    let dim = days_in_month(y, m) as i32;
    let d = if md > 0 { md } else { dim + md + 1 };
    (1..=dim).contains(&d).then_some(d as u32)
}

fn select_byday(all: &[Date], byday: &[(i32, u32)]) -> Vec<Date> {
    let mut out = Vec::new();
    for &(ord, wd) in byday {
        let matching: Vec<Date> = all.iter().copied().filter(|d| d.weekday() == wd).collect();
        if ord == 0 {
            out.extend(matching);
        } else {
            let n = matching.len() as i32;
            let i = if ord > 0 { ord - 1 } else { n + ord };
            if (0..n).contains(&i) {
                out.push(matching[i as usize]);
            }
        }
    }
    out
}

fn parse_until(v: &str) -> Option<When> {
    let v = v.trim();
    if let Some(d) = Date::parse(v) {
        return Some(When::Date(d));
    }
    When::parse(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(s: &str) -> DateTime {
        DateTime::parse(s).unwrap()
    }

    fn occ(rule: &str, start: &str, from: &str, to: &str) -> Vec<String> {
        RRule::parse(rule)
            .unwrap()
            .between(dt(start), dt(from), dt(to), None, 1000)
            .iter()
            .map(|d| d.to_string())
            .collect()
    }

    #[test]
    fn hebdomadaire() {
        let o = occ("FREQ=WEEKLY;BYDAY=TU", "2026-09-22 18:30", "2026-09-01", "2026-10-14");
        assert_eq!(o, ["2026-09-22 18:30", "2026-09-29 18:30", "2026-10-06 18:30", "2026-10-13 18:30"]);
        let o = occ("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,TH;COUNT=5", "2026-09-21 09:00", "2026-01-01", "2027-01-01");
        assert_eq!(
            o,
            ["2026-09-21 09:00", "2026-09-24 09:00", "2026-10-05 09:00", "2026-10-08 09:00", "2026-10-19 09:00"]
        );
    }

    #[test]
    fn mensuel() {
        assert_eq!(
            occ("FREQ=MONTHLY;BYDAY=-1FR", "2026-01-30 10:00", "2026-01-01", "2026-05-01"),
            ["2026-01-30 10:00", "2026-02-27 10:00", "2026-03-27 10:00", "2026-04-24 10:00"]
        );
        // le 31 : les mois courts sont sautés (RFC 5545)
        assert_eq!(
            occ("FREQ=MONTHLY", "2026-01-31", "2026-01-01", "2026-06-01"),
            ["2026-01-31 00:00", "2026-03-31 00:00", "2026-05-31 00:00"]
        );
        assert_eq!(
            occ("FREQ=MONTHLY;BYMONTHDAY=-1", "2026-01-31", "2026-01-01", "2026-04-01"),
            ["2026-01-31 00:00", "2026-02-28 00:00", "2026-03-31 00:00"]
        );
        // dernier jour ouvré du mois
        assert_eq!(
            occ("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1", "2026-01-01", "2026-01-01", "2026-04-01"),
            ["2026-01-30 00:00", "2026-02-27 00:00", "2026-03-31 00:00"]
        );
        assert_eq!(
            occ("FREQ=MONTHLY;BYDAY=FR;BYMONTHDAY=13", "2026-01-01", "2026-01-01", "2027-01-01"),
            ["2026-02-13 00:00", "2026-03-13 00:00", "2026-11-13 00:00"]
        );
    }

    #[test]
    fn annuel_et_quotidien() {
        assert_eq!(
            occ("FREQ=YEARLY", "2024-02-29", "2024-01-01", "2029-01-01"),
            ["2024-02-29 00:00", "2028-02-29 00:00"]
        );
        assert_eq!(
            occ("FREQ=YEARLY;BYMONTH=11;BYDAY=4TH", "2026-11-26", "2026-01-01", "2028-01-01"),
            ["2026-11-26 00:00", "2027-11-25 00:00"]
        );
        assert_eq!(
            occ("FREQ=DAILY;UNTIL=20260925", "2026-09-22 08:00", "2026-09-01", "2026-12-01"),
            ["2026-09-22 08:00", "2026-09-23 08:00", "2026-09-24 08:00", "2026-09-25 08:00"]
        );
        assert_eq!(occ("FREQ=DAILY;BYDAY=MO,TU,WE,TH,FR", "2026-09-25", "2026-09-25", "2026-09-30").len(), 3);
        assert_eq!(occ("FREQ=DAILY;BYHOUR=9,14", "2026-09-25 09:00", "2026-09-25", "2026-09-27").len(), 4);
    }

    #[test]
    fn saut_rapide_et_garde_fous() {
        // un événement quotidien commencé en 2000 : la fenêtre de 2026 est calculée directement
        let o = occ("FREQ=DAILY", "2000-01-01 07:00", "2026-09-24", "2026-09-26");
        assert_eq!(o, ["2026-09-24 07:00", "2026-09-25 07:00"]);
        assert!(occ("FREQ=YEARLY;BYMONTH=2;BYMONTHDAY=30", "2026-01-01", "2026-01-01", "2100-01-01").is_empty());
        // COUNT compté depuis le début même si la fenêtre est plus tard
        assert_eq!(
            occ("FREQ=DAILY;COUNT=3", "2026-09-20", "2026-09-21", "2026-12-01"),
            ["2026-09-21 00:00", "2026-09-22 00:00"]
        );
    }

    #[test]
    fn texte() {
        let r = RRule::parse("RRULE:FREQ=WEEKLY;INTERVAL=2;BYDAY=TU,-1FR;COUNT=4;UNTIL=20261231T235959Z").unwrap();
        assert_eq!(RRule::parse(&r.to_rule_string()).unwrap(), r);
        assert!(RRule::parse("FREQ=HOURLY").is_err());
        assert!(RRule::parse("BYDAY=MO").is_err());
        assert_eq!(RRule::parse("FREQ=WEEKLY;BYDAY=TU").unwrap().describe(), "chaque semaine le mardi");
        assert_eq!(RRule::parse("FREQ=DAILY;BYDAY=MO,TU,WE,TH,FR").unwrap().describe(), "chaque jour de semaine");
    }
}
