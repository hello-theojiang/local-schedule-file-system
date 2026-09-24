//! Saisie en langage naturel (français).
//!
//! « Dentiste vendredi 14h-15h @Cabinet #santé »
//! « Sport tous les mardis 18h30 pendant 1h30 »
//! « Rapport demain !haute »
//!
//! Le résultat est un brouillon (clés du format de fichier) accompagné d'un résumé
//! lisible, affiché en aperçu avant validation.

use crate::date::{days_in_month, format_duration, human_date, parse_duration, Date, DateTime, DAY, MONTHS_FR};
use crate::json::Json;
use crate::rrule::{RRule, DAY_CODES};
use crate::search::fold;

struct Tok {
    orig: String,
    f: String,
}

#[derive(Default)]
struct Found {
    date: Option<Date>,
    end_date: Option<Date>,
    time: Option<u32>,
    end_time: Option<u32>,
    duration: Option<i64>,
    /// heure « floue » (matin, soir…) utilisée seulement sans heure précise
    soft_time: Option<u32>,
    all_day: bool,
    repeat: Option<String>,
    until: Option<Date>,
    count: Option<u32>,
    location: Option<String>,
    tags: Vec<String>,
    priority: Option<&'static str>,
    calendar: Option<String>,
    alarm: Vec<i64>,
    kind: Option<&'static str>,
    moment: Option<DateTime>,
}

const WEEKDAYS: [&str; 7] = ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"];

fn weekday_of(f: &str) -> Option<u32> {
    let f = f.trim_end_matches(',');
    let base = f.strip_suffix('s').unwrap_or(f);
    WEEKDAYS.iter().position(|w| *w == base).map(|i| i as u32)
}

fn month_of(f: &str) -> Option<u32> {
    let f = f.trim_end_matches(['.', ',']);
    let folded: Vec<String> = MONTHS_FR.iter().map(|m| fold(m)).collect();
    if let Some(i) = folded.iter().position(|m| m == f) {
        return Some(i as u32 + 1);
    }
    let abbr = ["janv", "fevr", "mars", "avr", "mai", "juin", "juil", "aout", "sept", "oct", "nov", "dec"];
    if f.len() >= 3 {
        if let Some(i) =
            abbr.iter().position(|a| *a == f || (a.starts_with(f) && f.len() >= 3 && f != "ma" && f != "ju"))
        {
            return Some(i as u32 + 1);
        }
    }
    None
}

fn number(f: &str) -> Option<u32> {
    if let Ok(n) = f.trim_end_matches(['e', ',']).trim_end_matches("er").parse::<u32>() {
        return Some(n);
    }
    let words = [
        ("un", 1),
        ("une", 1),
        ("premier", 1),
        ("premiere", 1),
        ("deux", 2),
        ("trois", 3),
        ("quatre", 4),
        ("cinq", 5),
        ("six", 6),
        ("sept", 7),
        ("huit", 8),
        ("neuf", 9),
        ("dix", 10),
        ("onze", 11),
        ("douze", 12),
        ("quinze", 15),
        ("vingt", 20),
        ("trente", 30),
    ];
    words.iter().find(|(w, _)| *w == f).map(|(_, n)| *n)
}

fn ordinal(f: &str) -> Option<i32> {
    Some(match f {
        "premier" | "premiere" | "1er" | "1re" | "1ere" => 1,
        "deuxieme" | "second" | "seconde" | "2e" | "2eme" => 2,
        "troisieme" | "3e" | "3eme" => 3,
        "quatrieme" | "4e" | "4eme" => 4,
        "dernier" | "derniere" => -1,
        "avant-dernier" | "avant-derniere" => -2,
        _ => return None,
    })
}

/// `14h`, `14h30`, `14:30`, `9h`, `midi`, `minuit`.
fn time_of(f: &str) -> Option<u32> {
    let f = f.trim_end_matches([',', '.']);
    match f {
        "midi" => return Some(12 * 3600),
        "minuit" => return Some(0),
        _ => {}
    }
    let (h, m) = if let Some((h, m)) = f.split_once('h') {
        let m = m.trim_end_matches("min").trim_end_matches("mn");
        (h, m)
    } else if let Some((h, m)) = f.split_once(':') {
        (h, m)
    } else {
        return None;
    };
    if h.is_empty() || h.len() > 2 || !h.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if !(m.is_empty() || (m.len() == 2 && m.bytes().all(|b| b.is_ascii_digit()))) {
        return None;
    }
    let h: u32 = h.parse().ok()?;
    let m: u32 = if m.is_empty() { 0 } else { m.parse().ok()? };
    (h <= 23 && m <= 59).then_some(h * 3600 + m * 60)
}

/// `14h-15h`, `14h30-16h`, `9-11h`.
fn time_range(f: &str) -> Option<(u32, u32)> {
    let (a, b) = f.split_once(['-', '–', '—'])?;
    let end = time_of(b)?;
    let start = time_of(a).or_else(|| a.parse::<u32>().ok().filter(|h| *h <= 23).map(|h| h * 3600))?;
    Some((start, end))
}

/// `12/10`, `12/10/2026`, `12/10/26`, `2026-10-12`.
fn numeric_date(f: &str, today: Date) -> Option<Date> {
    if let Some(d) = Date::parse(f) {
        return Some(d);
    }
    let parts: Vec<&str> = f.split(['/', '.']).collect();
    if parts.len() < 2
        || parts.len() > 3
        || !parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let d: u32 = parts[0].parse().ok()?;
    let m: u32 = parts[1].parse().ok()?;
    if parts.len() == 3 {
        let mut y: i32 = parts[2].parse().ok()?;
        if y < 100 {
            y += 2000;
        }
        return Date::new(y, m, d);
    }
    future_day_month(today, d, m)
}

fn future_day_month(today: Date, d: u32, m: u32) -> Option<Date> {
    let this = Date::new(today.y, m, d);
    match this {
        Some(x) if x >= today => Some(x),
        _ => Date::new(today.y + 1, m, d).or(this),
    }
}

fn next_weekday(today: Date, wd: u32, include_today: bool) -> Date {
    let delta = (wd + 7 - today.weekday()) % 7;
    let delta = if delta == 0 && !include_today { 7 } else { delta };
    today.add_days(delta as i64)
}

impl Found {
    fn set_date(&mut self, d: Date) -> bool {
        if self.date.is_some() {
            return false;
        }
        self.date = Some(d);
        true
    }
}

struct P<'a> {
    t: &'a [Tok],
    used: Vec<bool>,
    today: Date,
    now: DateTime,
    x: Found,
}

impl P<'_> {
    fn f(&self, i: usize) -> &str {
        if i < self.t.len() && !self.used[i] {
            &self.t[i].f
        } else {
            ""
        }
    }

    fn mark(&mut self, i: usize, n: usize) {
        for k in i..(i + n).min(self.used.len()) {
            self.used[k] = true;
        }
    }

    /// Consomme un mot de liaison juste avant `i` (« le », « à », « de »…).
    fn eat_before(&mut self, i: usize, words: &[&str]) {
        if i > 0 && !self.used[i - 1] && words.contains(&self.t[i - 1].f.as_str()) {
            self.used[i - 1] = true;
        }
    }

    /// Essaie de lire une date à la position `i`. Retourne (date, nb de mots).
    fn date_at(&self, i: usize) -> Option<(Date, usize)> {
        let f = self.f(i);
        let today = self.today;
        match f {
            "aujourd'hui" | "aujourdhui" | "auj" | "ajd" => return Some((today, 1)),
            "demain" => return Some((today.add_days(1), 1)),
            "apres-demain" => return Some((today.add_days(2), 1)),
            "hier" => return Some((today.add_days(-1), 1)),
            _ => {}
        }
        if f == "apres" && self.f(i + 1) == "demain" {
            return Some((today.add_days(2), 2));
        }
        if let Some(d) = numeric_date(f, today) {
            return Some((d, 1));
        }
        if let Some(wd) = weekday_of(f) {
            if WEEKDAYS.contains(&f.trim_end_matches(',')) {
                if self.f(i + 1) == "prochain" {
                    let d = next_weekday(today, wd, false);
                    // « lundi prochain » : la semaine suivante si on est déjà dans la semaine
                    return Some((
                        if (d.days() - today.days()) < 7 && today.weekday() < wd { d.add_days(7) } else { d },
                        2,
                    ));
                }
                // « lundi 12 octobre » : la date explicite prime
                if let Some((d, n)) = self.day_month_at(i + 1) {
                    return Some((d, n + 1));
                }
                return Some((next_weekday(today, wd, true), 1));
            }
        }
        if let Some((d, n)) = self.day_month_at(i) {
            return Some((d, n));
        }
        if f == "dans" {
            let n = number(self.f(i + 1)).or((self.f(i + 1) == "une" || self.f(i + 1) == "un").then_some(1))?;
            let unit = self.f(i + 2);
            let d = match unit.trim_end_matches('s') {
                "jour" | "j" => today.add_days(n as i64),
                "semaine" | "sem" => today.add_days(7 * n as i64),
                "moi" | "mois" => today.add_months(n as i64),
                "an" | "annee" => today.add_months(12 * n as i64),
                _ => return None,
            };
            return Some((d, 3));
        }
        if (f == "semaine" || f == "sem") && self.f(i + 1) == "prochaine" {
            return Some((next_weekday(today, 0, false), 2));
        }
        if (f == "week-end" || f == "weekend" || f == "we")
            && (i == 0 || matches!(self.t[i - 1].f.as_str(), "ce" | "le"))
        {
            return Some((next_weekday(today, 5, true), 1));
        }
        if f == "fin" && self.f(i + 1) == "du" && self.f(i + 2) == "mois" {
            return Some((Date { y: today.y, m: today.m, d: days_in_month(today.y, today.m) }, 3));
        }
        if f == "le" {
            if let Some(n) = number(self.f(i + 1)).filter(|n| (1..=31).contains(n)) {
                if month_of(self.f(i + 2)).is_none() && time_of(self.f(i + 1)).is_none() {
                    // « le 12 » : ce mois-ci ou le suivant
                    let mut d = Date::new(today.y, today.m, n);
                    if d.map(|x| x < today).unwrap_or(true) {
                        let nm = today.first_of_month().add_months(1);
                        d = Date::new(nm.y, nm.m, n);
                    }
                    return d.map(|d| (d, 2));
                }
            }
        }
        None
    }

    /// `12 octobre`, `1er oct 2027`.
    fn day_month_at(&self, i: usize) -> Option<(Date, usize)> {
        let d = number(self.f(i)).filter(|n| (1..=31).contains(n))?;
        if time_of(self.f(i)).is_some() {
            return None;
        }
        let m = month_of(self.f(i + 1))?;
        if let Ok(y) = self.f(i + 2).parse::<i32>() {
            if (1970..=2200).contains(&y) {
                return Date::new(y, m, d).map(|x| (x, 3));
            }
        }
        future_day_month(self.today, d, m).map(|x| (x, 2))
    }

    fn weekday_list(&self, mut i: usize) -> (Vec<u32>, usize) {
        let st = i;
        let mut days = Vec::new();
        loop {
            let f = self.f(i);
            if let Some(wd) =
                weekday_of(f).filter(|_| WEEKDAYS.iter().any(|w| f.trim_end_matches(',').trim_end_matches('s') == *w))
            {
                days.push(wd);
                i += 1;
                if matches!(self.f(i), "et" | ",") {
                    i += 1;
                }
                continue;
            }
            break;
        }
        if days.is_empty() {
            return (days, 0);
        }
        // ne pas consommer un « et » final
        if matches!(self.f(i - 1), "et" | ",") {
            i -= 1;
        }
        (days, i - st)
    }

    fn byday(days: &[u32]) -> String {
        let mut d = days.to_vec();
        d.sort();
        d.dedup();
        d.iter().map(|x| DAY_CODES[*x as usize]).collect::<Vec<_>>().join(",")
    }

    /// Répétitions : « tous les mardis », « chaque mois », « toutes les 2 semaines »…
    fn repeat_at(&mut self, i: usize) -> Option<usize> {
        let f = self.f(i).to_string();
        let g = self.f(i + 1).to_string();
        let every = matches!(f.as_str(), "tous" | "toutes" | "chaque");
        let wk = "MO,TU,WE,TH,FR";
        if matches!(f.as_str(), "quotidien" | "quotidienne" | "quotidiennement") {
            self.x.repeat = Some("FREQ=DAILY".into());
            return Some(1);
        }
        if matches!(f.as_str(), "hebdo" | "hebdomadaire") {
            self.x.repeat = Some("FREQ=WEEKLY".into());
            return Some(1);
        }
        if matches!(f.as_str(), "mensuel" | "mensuelle" | "mensuellement") {
            self.x.repeat = Some("FREQ=MONTHLY".into());
            return Some(1);
        }
        if matches!(f.as_str(), "annuel" | "annuelle" | "annuellement") {
            self.x.repeat = Some("FREQ=YEARLY".into());
            return Some(1);
        }
        if f == "en" && g == "semaine" {
            self.x.repeat = Some(format!("FREQ=WEEKLY;BYDAY={wk}"));
            return Some(2);
        }
        if f == "une" && g == "semaine" && self.f(i + 2) == "sur" && self.f(i + 3) == "deux" {
            self.x.repeat = Some("FREQ=WEEKLY;INTERVAL=2".into());
            return Some(4);
        }
        if f == "du" && weekday_of(&g).is_some() && self.f(i + 2) == "au" {
            if let (Some(a), Some(b)) = (weekday_of(&g), weekday_of(self.f(i + 3))) {
                if self.x.repeat.is_none()
                    && (self.x.time.is_some() || (0..self.t.len()).any(|k| matches!(self.f(k), "tous" | "chaque")))
                {
                    let days: Vec<u32> = (a..=b).collect();
                    self.x.repeat = Some(format!("FREQ=WEEKLY;BYDAY={}", Self::byday(&days)));
                    return Some(4);
                }
            }
        }
        if !every {
            // « le premier lundi du mois »
            if let Some(o) = ordinal(&f) {
                if let Some(wd) = weekday_of(&g) {
                    if self.f(i + 2) == "du" && self.f(i + 3) == "mois" {
                        self.x.repeat = Some(format!("FREQ=MONTHLY;BYDAY={o}{}", DAY_CODES[wd as usize]));
                        return Some(4);
                    }
                }
            }
            return None;
        }
        // chaque / tous / toutes …
        let n_idx = i + 1;
        if f == "chaque" {
            if let Some(o) = ordinal(&g) {
                if let Some(wd) = weekday_of(self.f(i + 2)) {
                    if self.f(i + 3) == "du" && self.f(i + 4) == "mois" {
                        self.x.repeat = Some(format!("FREQ=MONTHLY;BYDAY={o}{}", DAY_CODES[wd as usize]));
                        return Some(5);
                    }
                }
            }
            let (days, n) = self.weekday_list(n_idx);
            if n > 0 {
                self.x.repeat = Some(format!("FREQ=WEEKLY;BYDAY={}", Self::byday(&days)));
                return Some(1 + n);
            }
            let unit = g.as_str();
            let r = match unit {
                "jour" | "matin" | "soir" => {
                    if unit == "matin" {
                        self.x.soft_time.get_or_insert(9 * 3600);
                    }
                    if unit == "soir" {
                        self.x.soft_time.get_or_insert(19 * 3600);
                    }
                    "FREQ=DAILY"
                }
                "semaine" => "FREQ=WEEKLY",
                "mois" => "FREQ=MONTHLY",
                "annee" | "an" => "FREQ=YEARLY",
                _ => return None,
            };
            self.x.repeat = Some(r.into());
            return Some(2);
        }
        // tous / toutes les …
        if g != "les" {
            return None;
        }
        let a = self.f(i + 2).to_string();
        // « tous les premiers lundis du mois »
        if let Some(o) = ordinal(a.trim_end_matches('s')) {
            if let Some(wd) = weekday_of(self.f(i + 3)) {
                if self.f(i + 4) == "du" && self.f(i + 5) == "mois" {
                    self.x.repeat = Some(format!("FREQ=MONTHLY;BYDAY={o}{}", DAY_CODES[wd as usize]));
                    return Some(6);
                }
            }
        }
        let (days, n) = self.weekday_list(i + 2);
        if n > 0 {
            self.x.repeat = Some(format!("FREQ=WEEKLY;BYDAY={}", Self::byday(&days)));
            return Some(2 + n);
        }
        if a == "jours" && self.f(i + 3) == "de" && self.f(i + 4) == "semaine" {
            self.x.repeat = Some(format!("FREQ=WEEKLY;BYDAY={wk}"));
            return Some(5);
        }
        let simple = |u: &str| -> Option<&'static str> {
            Some(match u {
                "jours" | "jour" | "matins" | "soirs" => "DAILY",
                "semaines" | "semaine" => "WEEKLY",
                "mois" => "MONTHLY",
                "ans" | "annees" => "YEARLY",
                _ => return None,
            })
        };
        if let Some(fr) = simple(&a) {
            if a == "matins" {
                self.x.soft_time.get_or_insert(9 * 3600);
            }
            if a == "soirs" {
                self.x.soft_time.get_or_insert(19 * 3600);
            }
            self.x.repeat = Some(format!("FREQ={fr}"));
            return Some(3);
        }
        // « toutes les 2 semaines », « tous les 3 jours », « tous les 15 du mois »
        if let Some(n) = number(&a).filter(|n| *n >= 1) {
            let u = self.f(i + 3).to_string();
            if u == "du" && self.f(i + 4) == "mois" && n <= 31 {
                self.x.repeat = Some(format!("FREQ=MONTHLY;BYMONTHDAY={n}"));
                return Some(5);
            }
            if let Some(fr) = simple(&u) {
                self.x.repeat = Some(if n == 1 { format!("FREQ={fr}") } else { format!("FREQ={fr};INTERVAL={n}") });
                return Some(4);
            }
        }
        None
    }

    fn run(&mut self) {
        let n = self.t.len();
        // 1. marqueurs explicites : @lieu, #tag, !priorité, +calendrier
        let mut i = 0;
        while i < n {
            let o = self.t[i].orig.clone();
            if let Some(rest) = o.strip_prefix('@').filter(|r| !r.is_empty()) {
                let mut loc = rest.to_string();
                let mut k = 1;
                if let Some(q) = rest.strip_prefix('"') {
                    loc = q.to_string();
                    while !loc.ends_with('"') && i + k < n {
                        loc.push(' ');
                        loc.push_str(&self.t[i + k].orig);
                        k += 1;
                    }
                    loc = loc.trim_end_matches('"').to_string();
                }
                self.x.location = Some(loc.replace('_', " ").trim_end_matches([',', '.']).to_string());
                self.mark(i, k);
                i += k;
                continue;
            }
            if let Some(tag) = o.strip_prefix('#').filter(|r| !r.is_empty() && !r.starts_with('#')) {
                self.x.tags.push(tag.trim_end_matches([',', '.']).to_string());
                self.mark(i, 1);
            } else if let Some(p) = self.t[i].f.strip_prefix('!') {
                let pr = match p {
                    "" | "!" | "!!" | "haute" | "haut" | "high" | "urgent" | "urgente" | "important" | "1" => {
                        Some("high")
                    }
                    "moyenne" | "moyen" | "normale" | "medium" | "2" => Some("medium"),
                    "basse" | "bas" | "faible" | "low" | "3" => Some("low"),
                    _ => None,
                };
                if let Some(pr) = pr {
                    self.x.priority = Some(pr);
                    self.mark(i, 1);
                }
            } else if let Some(cal) =
                o.strip_prefix('+').filter(|r| r.chars().next().map(char::is_alphabetic).unwrap_or(false))
            {
                self.x.calendar = Some(cal.trim_end_matches([',', '.']).to_string());
                self.mark(i, 1);
            }
            i += 1;
        }
        // 2. préfixe de type
        if n > 0 {
            let f0 = self.t[0].f.trim_end_matches(':').to_string();
            if self.t[0].f.ends_with(':') || n > 1 {
                match f0.as_str() {
                    "tache" | "todo" | "t" | "a faire" if self.t[0].f.ends_with(':') => {
                        self.x.kind = Some("task");
                        self.mark(0, 1);
                    }
                    "evenement" | "event" | "rdv:" | "e" if self.t[0].f.ends_with(':') => {
                        self.x.kind = Some("event");
                        self.mark(0, 1);
                    }
                    _ => {}
                }
            }
        }
        // 3. rappels et durées (avant les heures : « pendant 1h30 »)
        let mut i = 0;
        while i < n {
            let f = self.f(i).to_string();
            if f == "rappel" || f == "rappeler" {
                let g = self.f(i + 1).to_string();
                if g == "la" && self.f(i + 2) == "veille" {
                    self.x.alarm.push(DAY);
                    self.mark(i, 3);
                } else if let Some(d) = parse_duration(&g).filter(|d| *d > 0) {
                    self.x.alarm.push(d);
                    let extra = if self.f(i + 2) == "avant" { 1 } else { 0 };
                    self.mark(i, 2 + extra);
                } else if let (Some(nb), Some(d)) = (number(&g), parse_duration(&format!("1{}", self.f(i + 2)))) {
                    self.x.alarm.push(nb as i64 * d);
                    let extra = if self.f(i + 3) == "avant" { 1 } else { 0 };
                    self.mark(i, 3 + extra);
                }
            } else if matches!(f.as_str(), "pendant" | "durant" | "pour") {
                let g = self.f(i + 1).to_string();
                let unit = self.f(i + 2).to_string();
                if let Some(d) = parse_duration(&g).filter(|_| g.chars().any(|c| c.is_alphabetic()) && !g.contains(':'))
                {
                    self.x.duration = Some(d);
                    self.mark(i, 2);
                } else if let Some(nb) = number(&g) {
                    let mult = match unit.trim_end_matches('s') {
                        "heure" | "h" => Some(3600),
                        "minute" | "min" | "mn" => Some(60),
                        "jour" | "j" => Some(DAY),
                        "semaine" => Some(7 * DAY),
                        _ => None,
                    };
                    if let Some(m) = mult {
                        self.x.duration = Some(nb as i64 * m);
                        self.mark(i, 3);
                    }
                }
            } else if f == "toute" && self.f(i + 1) == "la" && self.f(i + 2) == "journee" {
                self.x.all_day = true;
                self.mark(i, 3);
            } else if f == "journee" && i > 0 && matches!(self.t[i - 1].f.as_str(), "la" | "toute") {
                self.x.all_day = true;
                self.mark(i, 1);
            }
            i += 1;
        }
        // 4. répétitions
        let mut i = 0;
        while i < n {
            if !self.used[i] {
                if let Some(k) = self.repeat_at(i) {
                    self.mark(i, k);
                    i += k;
                    continue;
                }
                let f = self.f(i).to_string();
                if f == "jusqu'au" || (f == "jusqu" && self.f(i + 1) == "au") {
                    let off = if f == "jusqu'au" { 1 } else { 2 };
                    if let Some((d, k)) = self.date_at(i + off) {
                        self.x.until = Some(d);
                        self.mark(i, off + k);
                        i += off + k;
                        continue;
                    }
                }
                if let Some(nb) = number(&f) {
                    if self.f(i + 1) == "fois" && self.x.repeat.is_some() {
                        self.x.count = Some(nb);
                        self.mark(i, 2);
                    }
                }
            }
            i += 1;
        }
        // 5. plages de dates « du 20 au 24 octobre »
        let mut i = 0;
        while i + 3 < n {
            if self.f(i) == "du" && !self.used[i] {
                if let Some(a) = number(self.f(i + 1)).filter(|d| (1..=31).contains(d)) {
                    if self.f(i + 2) == "au" {
                        if let Some((end, k)) = self.day_month_at(i + 3) {
                            if let Some(start) =
                                Date::new(end.y, end.m, a).map(|d| if d > end { d.add_months(-1) } else { d })
                            {
                                self.x.date = Some(start);
                                self.x.end_date = Some(end);
                                self.mark(i, 3 + k);
                            }
                        }
                    }
                }
                if self.x.date.is_none() {
                    if let Some((a, ka)) = self.date_at(i + 1) {
                        if self.f(i + 1 + ka) == "au" {
                            if let Some((b, kb)) = self.date_at(i + 2 + ka) {
                                let b = if b < a { b.add_days(7) } else { b };
                                self.x.date = Some(a);
                                self.x.end_date = Some(b);
                                self.mark(i, 2 + ka + kb);
                            }
                        }
                    }
                }
            }
            i += 1;
        }
        // 6. heures
        let mut i = 0;
        while i < n {
            if self.used[i] {
                i += 1;
                continue;
            }
            let f = self.f(i).to_string();
            if let Some((a, b)) = time_range(&f) {
                self.x.time = Some(a);
                self.x.end_time = Some(b);
                self.mark(i, 1);
                self.eat_before(i, &["de", "a", "entre"]);
            } else if matches!(f.as_str(), "de" | "entre")
                && time_of(self.f(i + 1)).is_some()
                && matches!(self.f(i + 2), "a" | "et" | "-" | "jusqu'a")
                && time_of(self.f(i + 3)).is_some()
            {
                self.x.time = time_of(self.f(i + 1));
                self.x.end_time = time_of(self.f(i + 3));
                self.mark(i, 4);
            } else if let Some(t) = time_of(&f) {
                if self.x.time.is_none() {
                    self.x.time = Some(t);
                    self.mark(i, 1);
                    self.eat_before(i, &["a", "vers", "des", "pour", "de"]);
                    // « 14h à 16h », « 14h - 16h »
                    if matches!(self.f(i + 1), "a" | "-" | "jusqu'a") {
                        if let Some(e) = time_of(self.f(i + 2)) {
                            self.x.end_time = Some(e);
                            self.mark(i + 1, 2);
                        }
                    }
                }
            } else if f == "dans" {
                if let Some(nb) = number(self.f(i + 1)) {
                    let u = self.f(i + 2).to_string();
                    let secs = match u.trim_end_matches('s') {
                        "heure" | "h" => Some(3600),
                        "minute" | "min" | "mn" => Some(60),
                        _ => parse_duration(&format!("{nb}{u}"))
                            .filter(|_| u.starts_with(|c: char| c.is_alphabetic()) && u.len() <= 3)
                            .map(|d| d / nb as i64),
                    };
                    if let Some(s) = secs.filter(|s| *s < DAY) {
                        self.x.moment = Some(self.now.add_secs(nb as i64 * s));
                        self.mark(i, 3);
                    }
                } else if let Some(d) = parse_duration(self.f(i + 1))
                    .filter(|d| *d < DAY && self.f(i + 1).chars().any(|c| c.is_alphabetic()))
                {
                    self.x.moment = Some(self.now.add_secs(d));
                    self.mark(i, 2);
                }
            }
            i += 1;
        }
        // 7. dates
        let mut i = 0;
        while i < n {
            if self.used[i] {
                i += 1;
                continue;
            }
            if let Some((d, k)) = self.date_at(i) {
                if self.x.set_date(d) {
                    self.mark(i, k);
                    self.eat_before(i, &["le", "ce", "cette", "pour", "des", "a", "au"]);
                    i += k;
                    continue;
                }
            }
            // moments flous
            let f = self.f(i).to_string();
            let soft = match f.as_str() {
                "matin" => Some(9 * 3600),
                "midi" => Some(12 * 3600),
                "apres-midi" | "aprem" => Some(14 * 3600),
                "soir" | "soiree" => Some(19 * 3600),
                _ => None,
            };
            if let Some(s) = soft {
                if i > 0 && matches!(self.t[i - 1].f.as_str(), "ce" | "cet" | "cette") && self.x.date.is_none() {
                    self.x.date = Some(self.today);
                    self.used[i - 1] = true;
                }
                if self.x.date.is_some() || (i > 0 && self.used[i - 1]) || i + 1 == n {
                    self.x.soft_time = Some(s);
                    self.mark(i, 1);
                }
            }
            i += 1;
        }
    }
}

/// Analyse une phrase. `today`/`now` : date et heure locales courantes.
pub fn parse(text: &str, now: DateTime) -> Json {
    let toks: Vec<Tok> = text
        .split_whitespace()
        .map(|w| Tok {
            orig: w.to_string(),
            f: fold(w).trim_matches(|c: char| matches!(c, '(' | ')' | '«' | '»' | '"' | ';')).to_string(),
        })
        .collect();
    let mut p = P { t: &toks, used: vec![false; toks.len()], today: now.date, now, x: Found::default() };
    p.run();
    let x = p.x;
    let mut title: Vec<&str> = toks.iter().zip(&p.used).filter(|(_, u)| !**u).map(|(t, _)| t.orig.as_str()).collect();
    // mots de liaison orphelins en fin de titre
    while let Some(last) = title.last() {
        let lf = fold(last);
        if matches!(
            lf.as_str(),
            "a" | "le" | "la" | "de" | "du" | "au" | "et" | "pour" | "-" | "," | "ce" | "cette" | "des" | "vers"
        ) {
            title.pop();
        } else {
            break;
        }
    }
    let mut title = title.join(" ").trim_matches([',', ' ', '-']).to_string();
    if let Some(c) = title.chars().next() {
        title = c.to_uppercase().collect::<String>() + &title[c.len_utf8()..];
    }
    let mut warnings: Vec<String> = Vec::new();
    if title.is_empty() {
        warnings.push("titre manquant".into());
    }

    let has_time = x.time.is_some() || x.moment.is_some();
    let kind = x.kind.unwrap_or(if x.priority.is_some() {
        "task"
    } else if has_time
        || x.all_day
        || x.end_date.is_some()
        || x.duration.is_some()
        || x.repeat.is_some()
        || x.soft_time.is_some()
    {
        "event"
    } else {
        "task"
    });

    let mut j = Json::obj().set("title", title.as_str());
    let mut summary: Vec<String> = Vec::new();
    // date de départ d'une répétition sans date : premier jour correspondant
    let mut date = x.date;
    if date.is_none() {
        if let Some(r) = x.repeat.as_deref().and_then(|r| RRule::parse(r).ok()) {
            let start_time = x.time.or(x.soft_time).unwrap_or(0);
            let from = if x.time.is_some() && now.sec > start_time { now.date.add_days(1) } else { now.date };
            date = r
                .between(
                    Date { y: from.y, m: from.m, d: from.d }.midnight(),
                    from.midnight(),
                    from.add_days(800).midnight(),
                    None,
                    1,
                )
                .first()
                .map(|d| d.date);
            if let (None, Some(wd)) = (date, r.byday.first()) {
                date = Some(next_weekday(from, wd.1, true));
            }
        }
    }
    let date = date.or_else(|| x.moment.map(|m| m.date));
    let date_for_time = date.unwrap_or_else(|| {
        // une heure seule : aujourd'hui, ou demain si l'heure est passée
        match x.time {
            Some(t) if t < now.sec => now.date.add_days(1),
            _ => now.date,
        }
    });

    if kind == "event" {
        summary.push("Événement".into());
        let start_time =
            x.moment.map(|m| m.sec).or(x.time).or(if x.all_day || x.end_date.is_some() { None } else { x.soft_time });
        match start_time {
            Some(t) if !x.all_day => {
                let start = DateTime { date: date_for_time, sec: t };
                let end = match (x.end_time, x.duration) {
                    (Some(e), _) => {
                        let mut e = DateTime { date: x.end_date.unwrap_or(date_for_time), sec: e };
                        if e <= start {
                            e = e.add_secs(DAY);
                        }
                        e
                    }
                    (None, Some(d)) => start.add_secs(d),
                    (None, None) => start.add_secs(3600),
                };
                j.insert("start", start.to_string());
                j.insert("end", end.to_string());
                summary.push(human_date(start.date));
                summary.push(format!(
                    "{} → {}",
                    start.hm(),
                    if end.date == start.date { end.hm() } else { format!("{} {}", human_date(end.date), end.hm()) }
                ));
            }
            _ => {
                let d = date.unwrap_or(now.date);
                j.insert("start", d.to_string());
                let mut end = x.end_date;
                if end.is_none() {
                    if let Some(dur) = x.duration.filter(|d| *d >= DAY) {
                        end = Some(d.add_days(dur / DAY - 1));
                    }
                }
                match end.filter(|e| *e > d) {
                    Some(e) => {
                        j.insert("end", e.to_string());
                        summary.push(format!("du {} au {}", human_date(d), human_date(e)));
                    }
                    None => summary.push(human_date(d)),
                }
                summary.push("toute la journée".into());
            }
        }
    } else {
        summary.push("Tâche".into());
        let due_time = x.moment.map(|m| m.sec).or(x.time);
        match (date, due_time) {
            (_, Some(t)) => {
                let due = DateTime { date: date_for_time, sec: t };
                j.insert("due", due.to_string());
                summary.push(format!("échéance {} à {}", human_date(due.date), due.hm()));
            }
            (Some(d), None) => {
                j.insert("due", d.to_string());
                summary.push(format!("échéance {}", human_date(d)));
            }
            (None, None) => {}
        }
        if let Some(p) = x.priority {
            j.insert("priority", p);
            summary.push(format!(
                "priorité {}",
                match p {
                    "high" => "haute",
                    "low" => "basse",
                    _ => "moyenne",
                }
            ));
        }
    }
    if let Some(r) = &x.repeat {
        let mut rule = r.clone();
        if let Some(u) = x.until {
            rule.push_str(&format!(";UNTIL={}", u.compact()));
        }
        if let Some(c) = x.count {
            rule.push_str(&format!(";COUNT={c}"));
        }
        if let Ok(rr) = RRule::parse(&rule) {
            summary.push(rr.describe());
            j.insert("repeat", rr.to_rule_string());
        }
    }
    if let Some(l) = &x.location {
        j.insert("location", l.as_str());
        summary.push(format!("@{l}"));
    }
    if !x.tags.is_empty() {
        summary.push(x.tags.iter().map(|t| format!("#{t}")).collect::<Vec<_>>().join(" "));
        j.insert("tags", x.tags.clone());
    }
    if let Some(c) = &x.calendar {
        j.insert("calendar", c.as_str());
        summary.push(format!("+{c}"));
    }
    if !x.alarm.is_empty() {
        let a: Vec<String> = x.alarm.iter().map(|d| format_duration(*d)).collect();
        summary.push(format!("rappel {}", a.join(", ")));
        j.insert("alarm", a);
    }
    Json::obj().set("kind", kind).set("fields", j).set("summary", summary.join(" · ")).set("warnings", warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    // jeudi 24 septembre 2026, 10:00
    fn now() -> DateTime {
        DateTime::parse("2026-09-24 10:00").unwrap()
    }

    fn p(s: &str) -> Json {
        parse(s, now())
    }

    fn f<'a>(j: &'a Json, k: &str) -> Option<&'a str> {
        j.get("fields").get(k).as_str()
    }

    #[test]
    fn exemples_de_la_specification() {
        let j = p("Dentiste vendredi 14h-15h @Cabinet #santé");
        assert_eq!(j.get("kind").as_str(), Some("event"));
        assert_eq!(f(&j, "title"), Some("Dentiste"));
        assert_eq!(f(&j, "start"), Some("2026-09-25 14:00"));
        assert_eq!(f(&j, "end"), Some("2026-09-25 15:00"));
        assert_eq!(f(&j, "location"), Some("Cabinet"));
        assert_eq!(j.get("fields").get("tags").str_list().unwrap(), ["santé"]);

        let j = p("Sport tous les mardis 18h30 pendant 1h30");
        assert_eq!(f(&j, "title"), Some("Sport"));
        assert_eq!(f(&j, "start"), Some("2026-09-29 18:30"));
        assert_eq!(f(&j, "end"), Some("2026-09-29 20:00"));
        assert_eq!(f(&j, "repeat"), Some("FREQ=WEEKLY;BYDAY=TU"));

        let j = p("Rapport demain !haute");
        assert_eq!(j.get("kind").as_str(), Some("task"));
        assert_eq!(f(&j, "title"), Some("Rapport"));
        assert_eq!(f(&j, "due"), Some("2026-09-25"));
        assert_eq!(f(&j, "priority"), Some("high"));
        assert!(j.get("summary").as_str().unwrap().contains("vendredi 25 septembre"));
    }

    #[test]
    fn dates() {
        assert_eq!(f(&p("Anniv Marie le 12 octobre"), "start"), None);
        assert_eq!(f(&p("Anniv Marie le 12 octobre"), "due"), Some("2026-10-12"));
        assert_eq!(f(&p("Réunion 3/10 à 9h"), "start"), Some("2026-10-03 09:00"));
        assert_eq!(f(&p("Réunion lundi prochain 9h"), "start"), Some("2026-09-28 09:00"));
        assert_eq!(f(&p("Appel dans 2 heures"), "start"), Some("2026-09-24 12:00"));
        assert_eq!(f(&p("Appel dans 3 jours à 11h"), "start"), Some("2026-09-27 11:00"));
        assert_eq!(f(&p("Payer facture le 5"), "due"), Some("2026-10-05"));
        assert_eq!(f(&p("Payer facture le 30"), "due"), Some("2026-09-30"));
        assert_eq!(f(&p("Dîner chez Paul après-demain à 20h30"), "start"), Some("2026-09-26 20:30"));
        assert_eq!(f(&p("Dîner chez Paul après-demain à 20h30"), "title"), Some("Dîner chez Paul"));
        assert_eq!(f(&p("Examen 1er juin"), "due"), Some("2027-06-01"));
        assert_eq!(f(&p("Café 9h"), "start"), Some("2026-09-25 09:00")); // 9h est passé : demain
        assert_eq!(f(&p("Conférence le 2026-11-03 de 10h à 12h"), "end"), Some("2026-11-03 12:00"));
        assert_eq!(f(&p("Film ce soir"), "start"), Some("2026-09-24 19:00"));
    }

    #[test]
    fn journees_et_plages() {
        let j = p("Vacances du 20 au 24 octobre");
        assert_eq!(j.get("kind").as_str(), Some("event"));
        assert_eq!(f(&j, "start"), Some("2026-10-20"));
        assert_eq!(f(&j, "end"), Some("2026-10-24"));
        assert_eq!(f(&j, "title"), Some("Vacances"));
        let j = p("Séminaire lundi toute la journée");
        assert_eq!(f(&j, "start"), Some("2026-09-28"));
        assert_eq!(f(&p("Stage lundi pendant 3 jours"), "end"), Some("2026-09-30"));
    }

    #[test]
    fn repetitions() {
        assert_eq!(f(&p("Yoga tous les lundis et jeudis à 7h"), "repeat"), Some("FREQ=WEEKLY;BYDAY=MO,TH"));
        assert_eq!(f(&p("Yoga tous les lundis et jeudis à 7h"), "title"), Some("Yoga"));
        assert_eq!(f(&p("Standup en semaine 9h15"), "repeat"), Some("FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR"));
        assert_eq!(f(&p("Point toutes les 2 semaines jeudi 14h"), "repeat"), Some("FREQ=WEEKLY;INTERVAL=2"));
        assert_eq!(f(&p("Loyer tous les 5 du mois !haute"), "repeat"), Some("FREQ=MONTHLY;BYMONTHDAY=5"));
        assert_eq!(f(&p("Loyer tous les 5 du mois !haute"), "due"), Some("2026-10-05"));
        assert_eq!(f(&p("Club le premier mardi du mois 20h"), "repeat"), Some("FREQ=MONTHLY;BYDAY=1TU"));
        assert_eq!(f(&p("Club le premier mardi du mois 20h"), "start"), Some("2026-10-06 20:00"));
        assert_eq!(f(&p("Anniversaire Léa 3 mars tous les ans"), "repeat"), Some("FREQ=YEARLY"));
        assert_eq!(f(&p("Anniversaire Léa 3 mars tous les ans"), "start"), Some("2027-03-03"));
        assert_eq!(
            f(&p("Piscine chaque samedi 10h jusqu'au 19 décembre"), "repeat"),
            Some("FREQ=WEEKLY;BYDAY=SA;UNTIL=20261219")
        );
        assert_eq!(f(&p("Cours tous les jours 8h 5 fois"), "repeat"), Some("FREQ=DAILY;COUNT=5"));
    }

    #[test]
    fn marqueurs() {
        let j = p("Réunion demain 10h @\"Salle B\" #projet #équipe +travail rappel 15min");
        assert_eq!(f(&j, "location"), Some("Salle B"));
        assert_eq!(j.get("fields").get("tags").str_list().unwrap(), ["projet", "équipe"]);
        assert_eq!(f(&j, "calendar"), Some("travail"));
        assert_eq!(j.get("fields").get("alarm").str_list().unwrap(), ["15m"]);
        assert_eq!(f(&j, "title"), Some("Réunion"));
        let j = p("tâche: appeler maman");
        assert_eq!(j.get("kind").as_str(), Some("task"));
        assert_eq!(f(&j, "title"), Some("Appeler maman"));
        let j = p("Acheter du pain");
        assert_eq!(j.get("kind").as_str(), Some("task"));
        assert_eq!(f(&j, "title"), Some("Acheter du pain"));
        // pas de fausse date dans un titre
        assert_eq!(f(&p("Lire le chapitre 3"), "title"), Some("Lire le chapitre 3"));
        assert_eq!(f(&p("Appeler Sam"), "title"), Some("Appeler Sam"));
        assert_eq!(f(&p("Appeler Sam"), "due"), None);
        assert_eq!(f(&p("Voyage en mars"), "due"), None);
        assert_eq!(f(&p("Voyage 12 mars"), "due"), Some("2027-03-12"));
    }
}
