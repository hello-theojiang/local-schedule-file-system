//! Fuseaux horaires : fichiers TZif (Linux, macOS), base `tzdata` d'Android,
//! règles POSIX (pied de page TZif, variable TZ) et petite table de secours.

use crate::date::{Date, DateTime, DAY};
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
struct Rule {
    std_off: i32,
    dst: Option<Dst>,
}

#[derive(Clone, Debug, PartialEq)]
struct Dst {
    off: i32,
    start: (Trans, i32),
    end: (Trans, i32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Trans {
    /// Mm.w.d : mois, semaine (5 = dernière), jour (0 = dimanche)
    M(u32, u32, u32),
    /// Jn : jour julien 1..365 sans 29 février
    J(u32),
    /// n : jour 0..365 avec 29 février
    N(u32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tz {
    pub name: String,
    trans: Vec<i64>,
    idx: Vec<u8>,
    offs: Vec<i32>,
    rule: Option<Rule>,
}

const SEARCH_DIRS: [&str; 4] =
    ["/usr/share/zoneinfo", "/usr/lib/zoneinfo", "/usr/share/lib/zoneinfo", "/var/db/timezone/zoneinfo"];
const ANDROID_TZDATA: [&str; 3] = [
    "/apex/com.android.tzdata/etc/tz/tzdata",
    "/apex/com.android.runtime/etc/tz/tzdata",
    "/system/usr/share/zoneinfo/tzdata",
];

/// Règles POSIX de secours quand aucune base de fuseaux n'est disponible.
const FALLBACK: [(&str, &str); 24] = [
    ("UTC", "UTC0"),
    ("Europe/Paris", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/Brussels", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/Berlin", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/Madrid", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/Rome", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/Zurich", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/Amsterdam", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/Luxembourg", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/London", "GMT0BST,M3.5.0/1,M10.5.0"),
    ("Europe/Lisbon", "WET0WEST,M3.5.0/1,M10.5.0"),
    ("Europe/Athens", "EET-2EEST,M3.5.0/3,M10.5.0/4"),
    ("America/New_York", "EST5EDT,M3.2.0,M11.1.0"),
    ("America/Chicago", "CST6CDT,M3.2.0,M11.1.0"),
    ("America/Denver", "MST7MDT,M3.2.0,M11.1.0"),
    ("America/Los_Angeles", "PST8PDT,M3.2.0,M11.1.0"),
    ("America/Montreal", "EST5EDT,M3.2.0,M11.1.0"),
    ("America/Toronto", "EST5EDT,M3.2.0,M11.1.0"),
    ("Africa/Casablanca", "<+01>-1"),
    ("Africa/Dakar", "GMT0"),
    ("Indian/Reunion", "<+04>-4"),
    ("America/Martinique", "AST4"),
    ("Asia/Tokyo", "JST-9"),
    ("Australia/Sydney", "AEST-10AEDT,M10.1.0,M4.1.0/3"),
];

impl Tz {
    pub fn utc() -> Tz {
        Tz { name: "UTC".into(), trans: vec![], idx: vec![], offs: vec![0], rule: None }
    }

    pub fn fixed(name: &str, off: i32) -> Tz {
        Tz { name: name.into(), trans: vec![], idx: vec![], offs: vec![off], rule: None }
    }

    /// Charge un fuseau IANA (`Europe/Paris`), une règle POSIX ou `UTC`.
    pub fn load(name: &str) -> Option<Tz> {
        let name = name.trim().trim_start_matches(':');
        if name.is_empty() {
            return None;
        }
        if matches!(name, "UTC" | "Etc/UTC" | "GMT" | "Z" | "Etc/GMT" | "UTC0") {
            return Some(Tz::utc());
        }
        if name.contains("..") {
            return None;
        }
        if name.starts_with('/') {
            let data = std::fs::read(name).ok()?;
            return Tz::from_tzif(&zone_name_from_path(name).unwrap_or_else(|| name.to_string()), &data);
        }
        let mut dirs: Vec<String> = Vec::new();
        if let Ok(d) = std::env::var("TZDIR") {
            dirs.push(d);
        }
        dirs.extend(SEARCH_DIRS.iter().map(|s| s.to_string()));
        for d in &dirs {
            let p = Path::new(d).join(name);
            if let Ok(data) = std::fs::read(&p) {
                if let Some(tz) = Tz::from_tzif(name, &data) {
                    return Some(tz);
                }
            }
        }
        for f in ANDROID_TZDATA {
            if let Some(tz) = android_lookup(f, name) {
                return Some(tz);
            }
        }
        if let Some((_, rule)) = FALLBACK.iter().find(|(n, _)| *n == name) {
            return Some(Tz { name: name.into(), trans: vec![], idx: vec![], offs: vec![0], rule: parse_posix(rule) });
        }
        // Une règle POSIX brute (TZ=CET-1CEST,…)
        parse_posix(name).map(|r| Tz {
            name: name.into(),
            trans: vec![],
            idx: vec![],
            offs: vec![r.std_off],
            rule: Some(r),
        })
    }

    /// Fuseau de l'appareil : $TZ, /etc/localtime, propriété Android, sinon UTC.
    pub fn local() -> Tz {
        if let Ok(tz) = std::env::var("TZ") {
            if let Some(t) = Tz::load(&tz) {
                return t;
            }
        }
        if let Ok(target) = std::fs::read_link("/etc/localtime") {
            if let Some(name) = zone_name_from_path(&target.to_string_lossy()) {
                if let Some(t) = Tz::load(&name) {
                    return t;
                }
            }
        }
        if let Ok(data) = std::fs::read("/etc/localtime") {
            let name = std::fs::read_to_string("/etc/timezone")
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|_| "Local".into());
            if let Some(t) = Tz::from_tzif(&name, &data) {
                return t;
            }
        }
        if Path::new("/system/bin/getprop").exists() {
            if let Ok(out) = std::process::Command::new("/system/bin/getprop").arg("persist.sys.timezone").output() {
                if let Some(t) = Tz::load(String::from_utf8_lossy(&out.stdout).trim()) {
                    return t;
                }
            }
        }
        Tz::utc()
    }

    pub fn from_tzif(name: &str, data: &[u8]) -> Option<Tz> {
        if data.len() < 44 || &data[0..4] != b"TZif" {
            return None;
        }
        let rd = |o: usize| -> Option<usize> {
            data.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
        };
        let counts = |o: usize| -> Option<[usize; 6]> {
            Some([rd(o + 20)?, rd(o + 24)?, rd(o + 28)?, rd(o + 32)?, rd(o + 36)?, rd(o + 40)?])
        };
        let version = data[4];
        let [isut, isstd, leap, timecnt, typecnt, charcnt] = counts(0)?;
        let mut o = 44;
        let tsize;
        if version >= b'2' {
            // saute le bloc v1 (temps sur 32 bits) pour lire le bloc v2 (64 bits)
            o += timecnt * 5 + typecnt * 6 + charcnt + leap * 8 + isstd + isut;
            if data.get(o..o + 4)? != b"TZif" {
                return None;
            }
            let c = counts(o)?;
            o += 44;
            tsize = 8;
            return Tz::parse_block(name, data, o, tsize, c, true);
        }
        tsize = 4;
        Tz::parse_block(name, data, o, tsize, [isut, isstd, leap, timecnt, typecnt, charcnt], false)
    }

    fn parse_block(name: &str, data: &[u8], mut o: usize, tsize: usize, c: [usize; 6], footer: bool) -> Option<Tz> {
        let [isut, isstd, leap, timecnt, typecnt, charcnt] = c;
        let mut trans = Vec::with_capacity(timecnt);
        for i in 0..timecnt {
            let b = data.get(o + i * tsize..o + (i + 1) * tsize)?;
            trans.push(if tsize == 8 {
                i64::from_be_bytes(b.try_into().ok()?)
            } else {
                i32::from_be_bytes(b.try_into().ok()?) as i64
            });
        }
        o += timecnt * tsize;
        let idx = data.get(o..o + timecnt)?.to_vec();
        o += timecnt;
        let mut offs = Vec::with_capacity(typecnt);
        for i in 0..typecnt {
            let b = data.get(o + i * 6..o + i * 6 + 4)?;
            offs.push(i32::from_be_bytes(b.try_into().ok()?));
        }
        o += typecnt * 6 + charcnt + leap * (tsize + 4) + isstd + isut;
        if offs.is_empty() || idx.iter().any(|&i| i as usize >= offs.len()) {
            return None;
        }
        let mut rule = None;
        if footer {
            if let Some(rest) = data.get(o..) {
                if rest.first() == Some(&b'\n') {
                    if let Some(end) = rest[1..].iter().position(|&b| b == b'\n') {
                        rule = std::str::from_utf8(&rest[1..1 + end]).ok().and_then(parse_posix);
                    }
                }
            }
        }
        Some(Tz { name: name.into(), trans, idx, offs, rule })
    }

    /// Décalage (secondes à ajouter à l'UTC) en vigueur à l'instant `t`.
    pub fn offset_at(&self, t: i64) -> i32 {
        if self.trans.is_empty() || t >= *self.trans.last().unwrap_or(&i64::MAX) {
            if let Some(r) = &self.rule {
                return r.offset_at(t);
            }
            if self.trans.is_empty() {
                return self.offs[0];
            }
        }
        match self.trans.binary_search(&t) {
            Ok(i) => self.offs[self.idx[i] as usize],
            Err(0) => self.offs[0],
            Err(i) => self.offs[self.idx[i - 1] as usize],
        }
    }

    pub fn to_local(&self, t: i64) -> DateTime {
        DateTime::from_secs(t + self.offset_at(t) as i64)
    }

    /// Heure murale → instant. Dans un « trou » (passage à l'heure d'été), l'heure
    /// est décalée vers l'avant ; en cas d'ambiguïté, la première occurrence est retenue.
    pub fn to_utc(&self, dt: DateTime) -> i64 {
        let w = dt.secs();
        let a = self.offset_at(w - 43_200) as i64;
        let b = self.offset_at(w + 43_200) as i64;
        let ok = |o: i64| self.offset_at(w - o) as i64 == o;
        match (ok(a), ok(b)) {
            (true, true) => (w - a).min(w - b),
            (true, false) => w - a,
            (false, true) => w - b,
            (false, false) => w - a,
        }
    }
}

fn zone_name_from_path(p: &str) -> Option<String> {
    p.find("zoneinfo/").map(|i| p[i + 9..].to_string()).filter(|s| !s.is_empty())
}

fn android_lookup(file: &str, name: &str) -> Option<Tz> {
    let data = std::fs::read(file).ok()?;
    if !data.starts_with(b"tzdata") || data.len() < 24 {
        return None;
    }
    let rd = |o: usize| -> Option<usize> {
        data.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
    };
    let index = rd(12)?;
    let data_off = rd(16)?;
    let mut o = index;
    while o + 52 <= data_off {
        let raw = &data[o..o + 40];
        let n = raw.iter().position(|&b| b == 0).unwrap_or(40);
        if &raw[..n] == name.as_bytes() {
            let start = data_off + rd(o + 40)?;
            let len = rd(o + 44)?;
            return Tz::from_tzif(name, data.get(start..start + len)?);
        }
        o += 52;
    }
    None
}

impl Rule {
    fn offset_at(&self, t: i64) -> i32 {
        let Some(dst) = &self.dst else {
            return self.std_off;
        };
        let y = DateTime::from_secs(t + self.std_off as i64).date.y;
        // début > fin : hémisphère sud, l'heure d'été chevauche le nouvel an
        let start = trans_local(dst.start.0, y) + dst.start.1 as i64 - self.std_off as i64;
        let end = trans_local(dst.end.0, y) + dst.end.1 as i64 - dst.off as i64;
        let in_dst = if start < end { t >= start && t < end } else { !(t >= end && t < start) };
        if in_dst {
            dst.off
        } else {
            self.std_off
        }
    }
}

/// Minuit (heure murale, en secondes) du jour de transition de l'année `y`.
fn trans_local(tr: Trans, y: i32) -> i64 {
    let d = match tr {
        Trans::M(m, w, wd) => {
            let first = Date { y, m, d: 1 };
            // wd : 0 = dimanche ; weekday() : 0 = lundi
            let target = (wd + 6) % 7;
            let delta = (target + 7 - first.weekday()) % 7;
            let mut day = 1 + delta + (w - 1) * 7;
            let dim = crate::date::days_in_month(y, m);
            while day > dim {
                day -= 7;
            }
            Date { y, m, d: day }
        }
        Trans::J(n) => {
            let mut doy = n as i64 - 1;
            if crate::date::is_leap(y) && n >= 60 {
                doy += 1;
            }
            Date { y, m: 1, d: 1 }.add_days(doy)
        }
        Trans::N(n) => Date { y, m: 1, d: 1 }.add_days(n as i64),
    };
    d.days() * DAY
}

fn parse_posix(s: &str) -> Option<Rule> {
    let b = s.as_bytes();
    let mut i = 0;
    let name = |i: &mut usize| -> Option<()> {
        if b.get(*i) == Some(&b'<') {
            let e = s[*i..].find('>')?;
            *i += e + 1;
        } else {
            let st = *i;
            while *i < b.len() && b[*i].is_ascii_alphabetic() {
                *i += 1;
            }
            if *i - st < 3 {
                return None;
            }
        }
        Some(())
    };
    let num = |i: &mut usize| -> Option<i32> {
        let mut sign = 1;
        if b.get(*i) == Some(&b'+') {
            *i += 1;
        } else if b.get(*i) == Some(&b'-') {
            sign = -1;
            *i += 1;
        }
        let mut parts = [0i32; 3];
        for (k, p) in parts.iter_mut().enumerate() {
            if k > 0 {
                if b.get(*i) != Some(&b':') {
                    break;
                }
                *i += 1;
            }
            let st = *i;
            while *i < b.len() && b[*i].is_ascii_digit() {
                *i += 1;
            }
            if st == *i {
                return None;
            }
            *p = s[st..*i].parse().ok()?;
        }
        Some(sign * (parts[0] * 3600 + parts[1] * 60 + parts[2]))
    };
    name(&mut i)?;
    let std_off = -num(&mut i)?;
    if i >= b.len() {
        return Some(Rule { std_off, dst: None });
    }
    name(&mut i)?;
    let mut dst_off = std_off + 3600;
    if i < b.len() && b[i] != b',' {
        dst_off = -num(&mut i)?;
    }
    if b.get(i) != Some(&b',') {
        // sans règle : règles américaines par défaut (comportement de la glibc)
        return Some(Rule {
            std_off,
            dst: Some(Dst { off: dst_off, start: (Trans::M(3, 2, 0), 7200), end: (Trans::M(11, 1, 0), 7200) }),
        });
    }
    let trans = |i: &mut usize| -> Option<(Trans, i32)> {
        *i += 1; // ,
        let t = if b.get(*i) == Some(&b'M') {
            *i += 1;
            let rest = &s[*i..];
            let end = rest.find([',', '/']).unwrap_or(rest.len());
            let mut it = rest[..end].split('.');
            let m: u32 = it.next()?.parse().ok()?;
            let w: u32 = it.next()?.parse().ok()?;
            let d: u32 = it.next()?.parse().ok()?;
            *i += end;
            if !(1..=12).contains(&m) || !(1..=5).contains(&w) || d > 6 {
                return None;
            }
            Trans::M(m, w, d)
        } else {
            let j = b.get(*i) == Some(&b'J');
            if j {
                *i += 1;
            }
            let st = *i;
            while *i < b.len() && b[*i].is_ascii_digit() {
                *i += 1;
            }
            let n: u32 = s[st..*i].parse().ok()?;
            if j {
                Trans::J(n)
            } else {
                Trans::N(n)
            }
        };
        let mut time = 7200;
        if b.get(*i) == Some(&b'/') {
            *i += 1;
            time = num(i)?;
        }
        Some((t, time))
    };
    let start = trans(&mut i)?;
    let end = trans(&mut i)?;
    Some(Rule { std_off, dst: Some(Dst { off: dst_off, start, end }) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paris() -> Tz {
        Tz {
            name: "test".into(),
            trans: vec![],
            idx: vec![],
            offs: vec![3600],
            rule: parse_posix("CET-1CEST,M3.5.0,M10.5.0/3"),
        }
    }

    #[test]
    fn posix_paris() {
        let tz = paris();
        // 2026 : passage à l'heure d'été le 29 mars à 01:00 UTC, retour le 25 octobre à 01:00 UTC
        let t = |s: &str| DateTime::parse(s).unwrap().secs();
        assert_eq!(tz.offset_at(t("2026-01-15 12:00")), 3600);
        assert_eq!(tz.offset_at(t("2026-07-15 12:00")), 7200);
        assert_eq!(tz.offset_at(t("2026-03-29 00:59")), 3600);
        assert_eq!(tz.offset_at(t("2026-03-29 01:00")), 7200);
        assert_eq!(tz.offset_at(t("2026-10-25 00:59")), 7200);
        assert_eq!(tz.offset_at(t("2026-10-25 01:00")), 3600);
        // heure murale → UTC
        assert_eq!(tz.to_utc(DateTime::parse("2026-07-15 14:00").unwrap()), t("2026-07-15 12:00"));
        // trou : 02:30 n'existe pas le 29 mars → 03:30 CEST = 01:30 UTC
        assert_eq!(tz.to_utc(DateTime::parse("2026-03-29 02:30").unwrap()), t("2026-03-29 01:30"));
        // ambiguïté : 02:30 existe deux fois le 25 octobre → la première (CEST)
        assert_eq!(tz.to_utc(DateTime::parse("2026-10-25 02:30").unwrap()), t("2026-10-25 00:30"));
        assert_eq!(tz.to_local(t("2026-07-15 12:00")).to_string(), "2026-07-15 14:00");
    }

    #[test]
    fn posix_sud_et_divers() {
        let r = parse_posix("AEST-10AEDT,M10.1.0,M4.1.0/3").unwrap();
        let t = |s: &str| DateTime::parse(s).unwrap().secs();
        assert_eq!(r.offset_at(t("2026-01-15 00:00")), 11 * 3600);
        assert_eq!(r.offset_at(t("2026-07-15 00:00")), 10 * 3600);
        assert_eq!(parse_posix("<+0330>-3:30").unwrap().std_off, 3 * 3600 + 1800);
        assert_eq!(parse_posix("EST5EDT,M3.2.0,M11.1.0").unwrap().std_off, -5 * 3600);
        assert!(parse_posix("n'importe quoi").is_none());
    }

    #[test]
    fn tzif_systeme() {
        // Le test ne s'exécute que si la base système est présente.
        if Path::new("/usr/share/zoneinfo/Europe/Paris").exists() {
            assert!(!Tz::load("Europe/Paris").unwrap().trans.is_empty(), "TZif non lu");
        }
        if let Some(tz) = Tz::load("Europe/Paris").filter(|t| !t.trans.is_empty()) {
            let t = |s: &str| DateTime::parse(s).unwrap().secs();
            assert_eq!(tz.offset_at(t("2026-07-15 12:00")), 7200);
            assert_eq!(tz.offset_at(t("2090-07-15 12:00")), 7200); // pied de page POSIX
            assert_eq!(tz.offset_at(t("1990-01-15 12:00")), 3600);
            assert_eq!(tz.offset_at(t("2026-10-25 01:00")), 3600);
        }
        if let Some(tz) = Tz::load("America/New_York").filter(|t| !t.trans.is_empty()) {
            assert_eq!(tz.offset_at(DateTime::parse("2026-07-01 12:00").unwrap().secs()), -4 * 3600);
        }
        assert_eq!(Tz::load("UTC").unwrap().offset_at(0), 0);
        assert!(Tz::load("../../etc/passwd").is_none());
    }

    #[test]
    fn table_de_secours() {
        let r = parse_posix(FALLBACK.iter().find(|(n, _)| *n == "Europe/Paris").unwrap().1).unwrap();
        assert_eq!(r.std_off, 3600);
        for (_, rule) in FALLBACK {
            assert!(parse_posix(rule).is_some(), "{rule}");
        }
    }
}
