//! JSON minimal : analyse, sérialisation, accès pratique. Objets ordonnés.

use std::borrow::Cow;
use std::fmt::Write as _;

/// Clé d'objet : les clés littérales ne sont pas allouées.
pub type Key = Cow<'static, str>;

#[derive(Clone, Debug, PartialEq, Default)]
pub enum Json {
    #[default]
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(Key, Json)>),
}

impl Json {
    pub fn obj() -> Json {
        Json::Obj(Vec::with_capacity(8))
    }

    /// Constructeur chaînable : ajoute une clé **nouvelle** (pas de recherche de doublon,
    /// pour la vitesse). Pour remplacer une clé existante, utiliser [`Json::insert`].
    pub fn set(mut self, k: impl Into<Key>, v: impl Into<Json>) -> Json {
        if let Json::Obj(m) = &mut self {
            let k = k.into();
            debug_assert!(!m.iter().any(|(kk, _)| *kk == k), "clé en double : {k}");
            m.push((k, v.into()));
        }
        self
    }

    pub fn insert(&mut self, k: &str, v: impl Into<Json>) {
        if let Json::Obj(m) = self {
            let v = v.into();
            if let Some(e) = m.iter_mut().find(|(kk, _)| kk.as_ref() == k) {
                e.1 = v;
            } else {
                m.push((Cow::Owned(k.to_string()), v));
            }
        }
    }

    pub fn push(&mut self, v: impl Into<Json>) {
        if let Json::Arr(a) = self {
            a.push(v.into());
        }
    }

    pub fn get(&self, k: &str) -> &Json {
        static NULL: Json = Json::Null;
        match self {
            Json::Obj(m) => m.iter().find(|(kk, _)| kk.as_ref() == k).map(|(_, v)| v).unwrap_or(&NULL),
            _ => &NULL,
        }
    }

    pub fn has(&self, k: &str) -> bool {
        matches!(self, Json::Obj(m) if m.iter().any(|(kk, _)| kk.as_ref() == k))
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn str_or<'a>(&'a self, d: &'a str) -> &'a str {
        self.as_str().unwrap_or(d)
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            Json::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        self.as_f64().map(|f| f as i64)
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_arr(&self) -> &[Json] {
        match self {
            Json::Arr(a) => a,
            _ => &[],
        }
    }

    pub fn as_obj(&self) -> &[(Key, Json)] {
        match self {
            Json::Obj(m) => m,
            _ => &[],
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }

    /// Liste de chaînes : accepte un tableau ou une chaîne séparée par des virgules.
    pub fn str_list(&self) -> Option<Vec<String>> {
        match self {
            Json::Arr(a) => Some(a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()),
            Json::Str(s) => Some(s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()),
            _ => None,
        }
    }

    pub fn parse(s: &str) -> Result<Json, String> {
        let mut p = Parser { b: s.as_bytes(), i: 0 };
        p.ws();
        if p.i == p.b.len() {
            return Ok(Json::Null);
        }
        let v = p.value(0)?;
        p.ws();
        if p.i != p.b.len() {
            return Err(format!("JSON : caractères en trop à la position {}", p.i));
        }
        Ok(v)
    }

    pub fn to_pretty(&self) -> String {
        let mut s = String::new();
        self.write(&mut s, Some(0));
        s
    }

    fn write(&self, out: &mut String, indent: Option<usize>) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Num(n) => {
                if n.is_finite() {
                    if n.fract() == 0.0 && n.abs() < 1e15 {
                        let _ = write!(out, "{}", *n as i64);
                    } else {
                        let _ = write!(out, "{n}");
                    }
                } else {
                    out.push_str("null");
                }
            }
            Json::Str(s) => write_str(out, s),
            Json::Arr(a) => {
                if a.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push('[');
                for (i, v) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    nl(out, indent.map(|n| n + 1));
                    v.write(out, indent.map(|n| n + 1));
                }
                nl(out, indent);
                out.push(']');
            }
            Json::Obj(m) => {
                if m.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push('{');
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    nl(out, indent.map(|n| n + 1));
                    write_str(out, k);
                    out.push(':');
                    if indent.is_some() {
                        out.push(' ');
                    }
                    v.write(out, indent.map(|n| n + 1));
                }
                nl(out, indent);
                out.push('}');
            }
        }
    }
}

impl std::fmt::Display for Json {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = String::new();
        self.write(&mut s, None);
        f.write_str(&s)
    }
}

fn nl(out: &mut String, indent: Option<usize>) {
    if let Some(n) = indent {
        out.push('\n');
        for _ in 0..n {
            out.push_str("  ");
        }
    }
}

pub fn write_str(out: &mut String, s: &str) {
    out.reserve(s.len() + 2);
    out.push('"');
    let b = s.as_bytes();
    let mut start = 0;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let esc: Option<&str> = match c {
            b'"' => Some("\\\""),
            b'\\' => Some("\\\\"),
            b'\n' => Some("\\n"),
            b'\r' => Some("\\r"),
            b'\t' => Some("\\t"),
            0..=0x1f => Some(""),
            // U+2028 / U+2029 (E2 80 A8/A9) cassent certains analyseurs JavaScript
            0xE2 if i + 2 < b.len() && b[i + 1] == 0x80 && (b[i + 2] == 0xA8 || b[i + 2] == 0xA9) => Some(""),
            _ => None,
        };
        match esc {
            None => i += 1,
            Some(e) => {
                out.push_str(&s[start..i]);
                if e.is_empty() {
                    let (cp, len) =
                        if c == 0xE2 { (if b[i + 2] == 0xA8 { 0x2028 } else { 0x2029 }, 3) } else { (c as u32, 1) };
                    let _ = write!(out, "\\u{cp:04x}");
                    i += len;
                } else {
                    out.push_str(e);
                    i += 1;
                }
                start = i;
            }
        }
    }
    out.push_str(&s[start..]);
    out.push('"');
}

impl From<&str> for Json {
    fn from(s: &str) -> Json {
        Json::Str(s.to_string())
    }
}
impl From<String> for Json {
    fn from(s: String) -> Json {
        Json::Str(s)
    }
}
impl From<&String> for Json {
    fn from(s: &String) -> Json {
        Json::Str(s.clone())
    }
}
impl From<bool> for Json {
    fn from(b: bool) -> Json {
        Json::Bool(b)
    }
}
impl From<i64> for Json {
    fn from(n: i64) -> Json {
        Json::Num(n as f64)
    }
}
impl From<i32> for Json {
    fn from(n: i32) -> Json {
        Json::Num(n as f64)
    }
}
impl From<u32> for Json {
    fn from(n: u32) -> Json {
        Json::Num(n as f64)
    }
}
impl From<usize> for Json {
    fn from(n: usize) -> Json {
        Json::Num(n as f64)
    }
}
impl From<f64> for Json {
    fn from(n: f64) -> Json {
        Json::Num(n)
    }
}
impl<T: Into<Json>> From<Option<T>> for Json {
    fn from(o: Option<T>) -> Json {
        o.map(Into::into).unwrap_or(Json::Null)
    }
}
impl<T: Into<Json>> From<Vec<T>> for Json {
    fn from(v: Vec<T>) -> Json {
        Json::Arr(v.into_iter().map(Into::into).collect())
    }
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn err(&self, m: &str) -> String {
        format!("JSON : {m} à la position {}", self.i)
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > 128 {
            return Err(self.err("imbrication trop profonde"));
        }
        self.ws();
        match self.b.get(self.i) {
            None => Err(self.err("fin inattendue")),
            Some(b'{') => {
                self.i += 1;
                let mut m = Vec::new();
                self.ws();
                if self.b.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(Json::Obj(m));
                }
                loop {
                    self.ws();
                    if self.b.get(self.i) != Some(&b'"') {
                        return Err(self.err("clé attendue"));
                    }
                    let k = self.string()?;
                    self.ws();
                    if self.b.get(self.i) != Some(&b':') {
                        return Err(self.err("« : » attendu"));
                    }
                    self.i += 1;
                    let v = self.value(depth + 1)?;
                    if let Some(e) = m.iter_mut().find(|(kk, _): &&mut (Key, Json)| *kk == k) {
                        e.1 = v;
                    } else {
                        m.push((Cow::Owned(k), v));
                    }
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Json::Obj(m));
                        }
                        _ => return Err(self.err("« , » ou « } » attendu")),
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut a = Vec::new();
                self.ws();
                if self.b.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(Json::Arr(a));
                }
                loop {
                    a.push(self.value(depth + 1)?);
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Json::Arr(a));
                        }
                        _ => return Err(self.err("« , » ou « ] » attendu")),
                    }
                }
            }
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') if self.b[self.i..].starts_with(b"true") => {
                self.i += 4;
                Ok(Json::Bool(true))
            }
            Some(b'f') if self.b[self.i..].starts_with(b"false") => {
                self.i += 5;
                Ok(Json::Bool(false))
            }
            Some(b'n') if self.b[self.i..].starts_with(b"null") => {
                self.i += 4;
                Ok(Json::Null)
            }
            Some(c) if *c == b'-' || c.is_ascii_digit() => {
                let st = self.i;
                self.i += 1;
                while self.i < self.b.len() && matches!(self.b[self.i], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
                {
                    self.i += 1;
                }
                let s = std::str::from_utf8(&self.b[st..self.i]).unwrap_or("");
                s.parse::<f64>().map(Json::Num).map_err(|_| self.err("nombre invalide"))
            }
            _ => Err(self.err("valeur attendue")),
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let h = self.b.get(self.i..self.i + 4).ok_or_else(|| self.err("\\u incomplet"))?;
        let s = std::str::from_utf8(h).map_err(|_| self.err("\\u invalide"))?;
        let v = u32::from_str_radix(s, 16).map_err(|_| self.err("\\u invalide"))?;
        self.i += 4;
        Ok(v)
    }

    fn string(&mut self) -> Result<String, String> {
        self.i += 1; // "
        let mut out = String::new();
        loop {
            let st = self.i;
            while self.i < self.b.len() && self.b[self.i] != b'"' && self.b[self.i] != b'\\' {
                self.i += 1;
            }
            out.push_str(std::str::from_utf8(&self.b[st..self.i]).map_err(|_| self.err("UTF-8 invalide"))?);
            match self.b.get(self.i) {
                None => return Err(self.err("chaîne non terminée")),
                Some(b'"') => {
                    self.i += 1;
                    return Ok(out);
                }
                _ => {
                    self.i += 1;
                    let c = *self.b.get(self.i).ok_or_else(|| self.err("échappement incomplet"))?;
                    self.i += 1;
                    match c {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let mut cp = self.hex4()?;
                            if (0xD800..0xDC00).contains(&cp) && self.b[self.i..].starts_with(b"\\u") {
                                self.i += 2;
                                let lo = self.hex4()?;
                                cp = 0x10000 + ((cp - 0xD800) << 10) + (lo.wrapping_sub(0xDC00) & 0x3FF);
                            }
                            out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                        }
                        _ => return Err(self.err("échappement inconnu")),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aller_retour() {
        let src = r#"{"a":1,"b":[true,false,null],"c":"é\"\né😀","d":-2.5e3,"e":{}}"#;
        let v = Json::parse(src).unwrap();
        assert_eq!(v.get("c").as_str(), Some("é\"\né😀"));
        assert_eq!(v.get("d").as_f64(), Some(-2500.0));
        let again = Json::parse(&v.to_string()).unwrap();
        assert_eq!(v, again);
        assert_eq!(Json::parse(&v.to_pretty()).unwrap(), v);
    }

    #[test]
    fn echappements() {
        let s = "a\u{1}b\u{2028}c\"d\\é";
        let out = Json::from(s).to_string();
        assert_eq!(out, "\"a\\u0001b\\u2028c\\\"d\\\\é\"");
        assert_eq!(Json::parse(&out).unwrap().as_str(), Some(s));
    }

    #[test]
    fn erreurs() {
        assert!(Json::parse("{").is_err());
        assert!(Json::parse("[1,]").is_err());
        assert!(Json::parse("\"abc").is_err());
        assert!(Json::parse("{} x").is_err());
        assert_eq!(Json::parse("").unwrap(), Json::Null);
    }

    #[test]
    fn construction() {
        let mut v = Json::obj().set("x", 3i64).set("y", "z");
        v.insert("x", 4i64);
        assert_eq!(v.to_string(), r#"{"x":4,"y":"z"}"#);
    }
}
