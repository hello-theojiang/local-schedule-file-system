//! Dates civiles (calendrier grégorien proleptique) et heures « murales ».

use std::fmt;

pub const DAY: i64 = 86_400;

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    pub y: i32,
    pub m: u32,
    pub d: u32,
}

pub fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

pub fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(y) => 29,
        2 => 28,
        _ => 0,
    }
}

impl Date {
    pub fn new(y: i32, m: u32, d: u32) -> Option<Date> {
        if (1..=12).contains(&m) && d >= 1 && d <= days_in_month(y, m) {
            Some(Date { y, m, d })
        } else {
            None
        }
    }

    /// Jours depuis le 1970-01-01 (algorithme de H. Hinnant).
    pub fn days(self) -> i64 {
        let y = self.y as i64 - if self.m <= 2 { 1 } else { 0 };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let m = self.m as i64;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + self.d as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    pub fn from_days(z: i64) -> Date {
        let z = z + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        Date { y: (y + if m <= 2 { 1 } else { 0 }) as i32, m, d }
    }

    /// 0 = lundi … 6 = dimanche.
    pub fn weekday(self) -> u32 {
        (self.days() + 3).rem_euclid(7) as u32
    }

    pub fn add_days(self, n: i64) -> Date {
        Date::from_days(self.days() + n)
    }

    /// Ajoute des mois en ramenant le jour au dernier jour du mois si besoin.
    pub fn add_months(self, n: i64) -> Date {
        let (y, m) = add_months_ym(self.y, self.m, n);
        Date { y, m, d: self.d.min(days_in_month(y, m)) }
    }

    pub fn ordinal(self) -> u32 {
        (self.days() - Date { y: self.y, m: 1, d: 1 }.days()) as u32 + 1
    }

    /// Lundi de la semaine.
    pub fn week_start(self, wkst: u32) -> Date {
        let back = (self.weekday() + 7 - wkst) % 7;
        self.add_days(-(back as i64))
    }

    pub fn first_of_month(self) -> Date {
        Date { d: 1, ..self }
    }

    /// `2026-10-02` ou `20261002`.
    pub fn parse(s: &str) -> Option<Date> {
        let s = s.trim();
        let b = s.as_bytes();
        if b.len() == 10 && b[4] == b'-' && b[7] == b'-' {
            Date::new(s[0..4].parse().ok()?, s[5..7].parse().ok()?, s[8..10].parse().ok()?)
        } else if b.len() == 8 && b.iter().all(u8::is_ascii_digit) {
            Date::new(s[0..4].parse().ok()?, s[4..6].parse().ok()?, s[6..8].parse().ok()?)
        } else {
            None
        }
    }

    pub fn compact(self) -> String {
        format!("{:04}{:02}{:02}", self.y, self.m, self.d)
    }

    pub fn at(self, h: u32, min: u32) -> DateTime {
        DateTime { date: self, sec: h * 3600 + min * 60 }
    }

    pub fn midnight(self) -> DateTime {
        DateTime { date: self, sec: 0 }
    }
}

pub fn add_months_ym(y: i32, m: u32, n: i64) -> (i32, u32) {
    let t = y as i64 * 12 + (m as i64 - 1) + n;
    (t.div_euclid(12) as i32, (t.rem_euclid(12) + 1) as u32)
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.y, self.m, self.d)
    }
}

/// Date et heure sans fuseau (heure « murale »).
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DateTime {
    pub date: Date,
    /// secondes depuis minuit
    pub sec: u32,
}

impl DateTime {
    /// Secondes « comme si c'était de l'UTC » : pratique pour l'arithmétique murale.
    pub fn secs(self) -> i64 {
        self.date.days() * DAY + self.sec as i64
    }

    pub fn from_secs(t: i64) -> DateTime {
        DateTime { date: Date::from_days(t.div_euclid(DAY)), sec: t.rem_euclid(DAY) as u32 }
    }

    pub fn add_secs(self, n: i64) -> DateTime {
        DateTime::from_secs(self.secs() + n)
    }

    pub fn hour(self) -> u32 {
        self.sec / 3600
    }

    pub fn minute(self) -> u32 {
        self.sec / 60 % 60
    }

    pub fn hm(self) -> String {
        format!("{:02}:{:02}", self.hour(), self.minute())
    }

    /// `2026-10-02 14:30`, `2026-10-02T14:30:00`, `20261002T143000`.
    pub fn parse(s: &str) -> Option<DateTime> {
        let s = s.trim();
        if let Some(d) = Date::parse(s) {
            return Some(d.midnight());
        }
        let (dpart, tpart) =
            if s.len() > 10 && (s.as_bytes()[10] == b'T' || s.as_bytes()[10] == b' ') && s.as_bytes()[4] == b'-' {
                (&s[..10], s[11..].trim())
            } else if s.len() >= 15 && s.as_bytes()[8] == b'T' {
                (&s[..8], &s[9..])
            } else {
                return None;
            };
        let date = Date::parse(dpart)?;
        let sec = parse_time(tpart)?;
        Some(DateTime { date, sec })
    }
}

/// `14:30`, `14:30:15`, `143015`, `1430`, `14h30`, `14h`.
pub fn parse_time(t: &str) -> Option<u32> {
    let t = t.trim();
    let (h, m, s) = if t.contains(':') {
        let mut it = t.split(':');
        let h: u32 = it.next()?.parse().ok()?;
        let m: u32 = it.next()?.parse().ok()?;
        let s: u32 = match it.next() {
            Some(x) => x.split('.').next()?.parse().ok()?,
            None => 0,
        };
        (h, m, s)
    } else if let Some((h, m)) = t.split_once(['h', 'H']) {
        (h.parse().ok()?, if m.is_empty() { 0 } else { m.parse().ok()? }, 0)
    } else if t.len() == 6 && t.bytes().all(|b| b.is_ascii_digit()) {
        (t[0..2].parse().ok()?, t[2..4].parse().ok()?, t[4..6].parse().ok()?)
    } else if t.len() == 4 && t.bytes().all(|b| b.is_ascii_digit()) {
        (t[0..2].parse().ok()?, t[2..4].parse().ok()?, 0)
    } else {
        return None;
    };
    if h > 24 || m > 59 || s > 60 || (h == 24 && (m > 0 || s > 0)) {
        return None;
    }
    Some(h * 3600 + m * 60 + s.min(59))
}

impl fmt::Display for DateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {:02}:{:02}", self.date, self.hour(), self.minute())?;
        if self.sec % 60 != 0 {
            write!(f, ":{:02}", self.sec % 60)?;
        }
        Ok(())
    }
}

/// Valeur temporelle telle qu'écrite dans un fichier.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum When {
    /// Journée entière.
    Date(Date),
    /// Heure murale, interprétée dans le fuseau de l'événement ou de l'appareil.
    Local(DateTime),
    /// Instant absolu (secondes UTC).
    Utc(i64),
}

impl When {
    pub fn parse(s: &str) -> Option<When> {
        let s = s.trim();
        if let Some(d) = Date::parse(s) {
            return Some(When::Date(d));
        }
        if let Some(rest) = s.strip_suffix('Z').or_else(|| s.strip_suffix('z')) {
            return DateTime::parse(rest).map(|dt| When::Utc(dt.secs()));
        }
        // décalage final : ±hh:mm, ±hhmm ou ±hh
        for tail in [6usize, 5, 3] {
            if s.len() < 16 + tail || !s.is_char_boundary(s.len() - tail) {
                continue;
            }
            let (head, off) = s.split_at(s.len() - tail);
            let sign = match off.as_bytes()[0] {
                b'+' => 1,
                b'-' => -1,
                _ => continue,
            };
            let digits = off[1..].replace(':', "");
            if !(digits.len() == 4 || digits.len() == 2) || !digits.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            let h: i64 = digits[..2].parse().ok()?;
            let m: i64 = if digits.len() == 4 { digits[2..].parse().ok()? } else { 0 };
            let dt = DateTime::parse(head.trim())?;
            return Some(When::Utc(dt.secs() - sign * (h * 3600 + m * 60)));
        }
        DateTime::parse(s).map(When::Local)
    }

    pub fn is_date(self) -> bool {
        matches!(self, When::Date(_))
    }

    pub fn to_file_string(self) -> String {
        match self {
            When::Date(d) => d.to_string(),
            When::Local(dt) => dt.to_string(),
            When::Utc(t) => {
                let dt = DateTime::from_secs(t);
                format!("{}T{:02}:{:02}:{:02}Z", dt.date, dt.hour(), dt.minute(), dt.sec % 60)
            }
        }
    }
}

/// Durée : `15m`, `1h30`, `1h30m`, `2d`, `1w`, `90` (minutes), `PT15M` (ISO 8601).
pub fn parse_duration(s: &str) -> Option<i64> {
    let s = s.trim().trim_start_matches('-');
    if s.is_empty() {
        return None;
    }
    if let Some(iso) = s.strip_prefix('P').or_else(|| s.strip_prefix("-P")) {
        let mut total = 0i64;
        let mut num = String::new();
        let mut in_time = false;
        for c in iso.chars() {
            match c {
                'T' => in_time = true,
                '0'..='9' => num.push(c),
                _ => {
                    let n: i64 = num.parse().ok()?;
                    num.clear();
                    total += n * match (c, in_time) {
                        ('W', _) => 7 * DAY,
                        ('D', _) => DAY,
                        ('H', true) => 3600,
                        ('M', true) => 60,
                        ('S', true) => 1,
                        _ => return None,
                    };
                }
            }
        }
        return Some(total);
    }
    if s.bytes().all(|b| b.is_ascii_digit()) {
        return Some(s.parse::<i64>().ok()? * 60);
    }
    let mut total = 0i64;
    let mut num = String::new();
    let chars: Vec<char> = s.chars().filter(|c| !c.is_whitespace()).collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_digit() {
            num.push(c);
            i += 1;
            continue;
        }
        let mut unit = String::new();
        while i < chars.len() && chars[i].is_alphabetic() {
            unit.push(chars[i]);
            i += 1;
        }
        let n: i64 = num.parse().ok()?;
        num.clear();
        let mult = match unit.to_lowercase().as_str() {
            "w" | "sem" | "semaine" | "semaines" => 7 * DAY,
            "d" | "j" | "jour" | "jours" => DAY,
            "h" | "heure" | "heures" => 3600,
            "m" | "min" | "mn" | "minute" | "minutes" => 60,
            "s" => 1,
            _ => return None,
        };
        total += n * mult;
    }
    if !num.is_empty() {
        // « 1h30 » : minutes implicites après les heures
        let n: i64 = num.parse().ok()?;
        total += n * 60;
    }
    Some(total)
}

/// Écrit une durée sous sa forme la plus courte : `15m`, `1h30`, `2d`, `1w`.
pub fn format_duration(secs: i64) -> String {
    let s = secs.abs();
    if s == 0 {
        return "0m".into();
    }
    if s % (7 * DAY) == 0 {
        return format!("{}w", s / (7 * DAY));
    }
    if s % DAY == 0 {
        return format!("{}d", s / DAY);
    }
    let h = s / 3600;
    let m = s % 3600 / 60;
    match (h, m) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h{m:02}"),
    }
}

/// Instant courant (secondes UTC).
pub fn now_utc() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub const WEEKDAYS_FR: [&str; 7] = ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"];
pub const MONTHS_FR: [&str; 12] = [
    "janvier",
    "février",
    "mars",
    "avril",
    "mai",
    "juin",
    "juillet",
    "août",
    "septembre",
    "octobre",
    "novembre",
    "décembre",
];

/// « vendredi 2 octobre »
pub fn human_date(d: Date) -> String {
    format!("{} {} {}", WEEKDAYS_FR[d.weekday() as usize], d.d, MONTHS_FR[d.m as usize - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jours() {
        assert_eq!(Date { y: 1970, m: 1, d: 1 }.days(), 0);
        assert_eq!(Date { y: 2000, m: 3, d: 1 }.days(), 11_017);
        for n in [-800_000i64, -1, 0, 1, 59, 60, 10_000, 20_000, 2_000_000] {
            assert_eq!(Date::from_days(n).days(), n);
        }
        assert_eq!(Date::parse("2026-09-24").unwrap().weekday(), 3); // jeudi
        assert_eq!(Date::parse("2024-02-29").unwrap().add_months(12), Date::parse("2025-02-28").unwrap());
        assert_eq!(Date::parse("2026-01-31").unwrap().add_months(1), Date::parse("2026-02-28").unwrap());
        assert_eq!(Date::parse("2026-12-31").unwrap().ordinal(), 365);
        assert!(Date::parse("2026-02-30").is_none());
        assert_eq!(Date::parse("2026-09-24").unwrap().week_start(0), Date::parse("2026-09-21").unwrap());
    }

    #[test]
    fn heures() {
        let dt = DateTime::parse("2026-10-02 14:30").unwrap();
        assert_eq!(dt.to_string(), "2026-10-02 14:30");
        assert_eq!(DateTime::parse("2026-10-02T14:30:05").unwrap().to_string(), "2026-10-02 14:30:05");
        assert_eq!(DateTime::parse("20261002T143000").unwrap(), dt);
        assert_eq!(DateTime::from_secs(dt.secs()), dt);
        assert_eq!(parse_time("9h"), Some(9 * 3600));
        assert_eq!(parse_time("18h30"), Some(18 * 3600 + 1800));
        assert_eq!(parse_time("25:00"), None);
    }

    #[test]
    fn when() {
        assert_eq!(When::parse("2026-10-02"), Some(When::Date(Date { y: 2026, m: 10, d: 2 })));
        let utc = When::parse("2026-10-02T12:00:00Z").unwrap();
        assert_eq!(When::parse("2026-10-02 14:00+02:00"), Some(utc));
        assert_eq!(When::parse("2026-10-02T07:00:00-05:00"), Some(utc));
        assert_eq!(utc.to_file_string(), "2026-10-02T12:00:00Z");
        assert!(matches!(When::parse("2026-10-02 14:00"), Some(When::Local(_))));
        assert_eq!(When::parse("n'importe"), None);
    }

    #[test]
    fn durees() {
        assert_eq!(parse_duration("15m"), Some(900));
        assert_eq!(parse_duration("1h30"), Some(5400));
        assert_eq!(parse_duration("1h30m"), Some(5400));
        assert_eq!(parse_duration("2d"), Some(2 * DAY));
        assert_eq!(parse_duration("1w"), Some(7 * DAY));
        assert_eq!(parse_duration("90"), Some(5400));
        assert_eq!(parse_duration("PT1H30M"), Some(5400));
        assert_eq!(parse_duration("-P1D"), Some(DAY));
        assert_eq!(parse_duration("abc"), None);
        assert_eq!(format_duration(5400), "1h30");
        assert_eq!(format_duration(900), "15m");
        assert_eq!(format_duration(DAY), "1d");
    }
}
