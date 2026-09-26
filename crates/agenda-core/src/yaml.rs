//! En-tête YAML (« front matter ») d'un fichier Markdown.
//!
//! Le document est conservé comme une suite d'entrées de premier niveau avec leur
//! texte brut : une entrée non modifiée est réécrite **à l'octet près**, ce qui
//! garantit que les champs inconnus, les commentaires et les structures complexes
//! survivent à toutes les modifications faites par l'application.

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Str(String),
    List(Vec<String>),
    /// Valeur que l'on ne sait pas interpréter (dictionnaire imbriqué…).
    Complex,
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    /// Liste : accepte aussi une valeur simple (`tags: santé`).
    pub fn as_list(&self) -> Vec<String> {
        match self {
            Value::List(l) => l.clone(),
            Value::Str(s) if !s.is_empty() => vec![s.clone()],
            _ => Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Entry {
    /// Clé ; vide pour une ligne de commentaire ou une ligne vide isolée.
    key: String,
    /// Texte brut de l'entrée, lignes suivantes comprises, avec le `\n` final.
    raw: String,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Doc {
    entries: Vec<Entry>,
    pub body: String,
    /// Le fichier avait-il un en-tête ?
    pub had_front_matter: bool,
}

impl Doc {
    pub fn new() -> Doc {
        Doc { had_front_matter: true, ..Default::default() }
    }

    /// Analyse un fichier complet. Sans en-tête, tout le texte devient le corps.
    pub fn parse(text: &str) -> Doc {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let first_nl = text.find('\n');
        let first = first_nl.map(|i| &text[..i]).unwrap_or(text);
        if first.trim_end() != "---" {
            return Doc { entries: Vec::new(), body: text.to_string(), had_front_matter: false };
        }
        let rest = &text[first_nl.map(|i| i + 1).unwrap_or(text.len())..];
        // cherche la ligne de fermeture
        let mut off = 0;
        let mut end = None;
        for line in rest.split_inclusive('\n') {
            let t = line.trim_end();
            if t == "---" || t == "..." {
                end = Some((off, off + line.len()));
                break;
            }
            off += line.len();
        }
        let (fm, body) = match end {
            Some((a, b)) => (&rest[..a], &rest[b..]),
            None => return Doc { entries: Vec::new(), body: text.to_string(), had_front_matter: false },
        };
        let mut entries: Vec<Entry> = Vec::new();
        for line in fm.split_inclusive('\n') {
            let line_owned;
            let line = if line.ends_with('\n') {
                line
            } else {
                line_owned = format!("{line}\n");
                &line_owned
            };
            let t = line.trim_end();
            let starts_entry = !line.starts_with([' ', '\t', '-', '#']) && !t.is_empty() && top_key(t).is_some();
            if starts_entry {
                entries.push(Entry { key: top_key(t).unwrap_or_default(), raw: line.to_string() });
            } else if line.starts_with('#') {
                entries.push(Entry { key: String::new(), raw: line.to_string() });
            } else if let Some(last) = entries.last_mut() {
                last.raw.push_str(line);
            } else {
                entries.push(Entry { key: String::new(), raw: line.to_string() });
            }
        }
        Doc { entries, body: body.to_string(), had_front_matter: true }
    }

    pub fn to_text(&self) -> String {
        let mut s = String::with_capacity(256 + self.body.len());
        if self.had_front_matter || !self.entries.is_empty() {
            s.push_str("---\n");
            for e in &self.entries {
                s.push_str(&e.raw);
            }
            s.push_str("---\n");
        }
        s.push_str(&self.body);
        s
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().filter(|e| !e.key.is_empty()).map(|e| e.key.as_str())
    }

    pub fn has(&self, key: &str) -> bool {
        self.entries.iter().any(|e| e.key == key)
    }

    pub fn get(&self, key: &str) -> Value {
        match self.entries.iter().find(|e| e.key == key) {
            Some(e) => parse_value(&e.raw),
            None => Value::Null,
        }
    }

    pub fn str(&self, key: &str) -> Option<String> {
        match self.get(key) {
            Value::Str(s) if !s.is_empty() => Some(s),
            _ => None,
        }
    }

    /// Texte brut d'une entrée (pour comparer deux versions d'un champ).
    pub fn raw(&self, key: &str) -> Option<&str> {
        self.entries.iter().find(|e| e.key == key).map(|e| e.raw.as_str())
    }

    pub fn set_raw(&mut self, key: &str, raw: Option<&str>) {
        match raw {
            None => self.remove(key),
            Some(r) => {
                if let Some(e) = self.entries.iter_mut().find(|e| e.key == key) {
                    e.raw = r.to_string();
                } else {
                    self.entries.push(Entry { key: key.to_string(), raw: r.to_string() });
                }
            }
        }
    }

    pub fn set_str(&mut self, key: &str, v: &str) {
        let raw = format!("{key}: {}\n", quote(v));
        self.replace(key, raw);
    }

    /// Écrit une date ou heure sans guillemets (format contrôlé par l'appelant).
    pub fn set_plain(&mut self, key: &str, v: &str) {
        let raw = format!("{key}: {v}\n");
        self.replace(key, raw);
    }

    pub fn set_list(&mut self, key: &str, items: &[String]) {
        let inner: Vec<String> = items.iter().map(|s| quote_flow(s)).collect();
        let raw = format!("{key}: [{}]\n", inner.join(", "));
        self.replace(key, raw);
    }

    pub fn remove(&mut self, key: &str) {
        self.entries.retain(|e| e.key != key);
    }

    fn replace(&mut self, key: &str, raw: String) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.key == key) {
            if e.raw != raw {
                // conserve les lignes vides / commentaires qui suivaient l'entrée
                let trailing: String = trailing_blank(&e.raw);
                e.raw = raw + &trailing;
            }
        } else {
            self.entries.push(Entry { key: key.to_string(), raw });
            self.had_front_matter = true;
        }
    }
}

fn trailing_blank(raw: &str) -> String {
    let lines: Vec<&str> = raw.split_inclusive('\n').collect();
    let mut n = lines.len();
    while n > 1 && lines[n - 1].trim().is_empty() {
        n -= 1;
    }
    lines[n..].concat()
}

/// Clé d'une ligne `clé: valeur` de premier niveau.
fn top_key(t: &str) -> Option<String> {
    let (k, rest) = if let Some(stripped) = t.strip_prefix('"') {
        let e = stripped.find('"')?;
        (stripped[..e].to_string(), &stripped[e + 1..])
    } else {
        let i = t.find(':')?;
        (t[..i].trim_end().to_string(), &t[i..])
    };
    let rest = rest.trim_start();
    if !rest.starts_with(':') || k.is_empty() {
        return None;
    }
    let after = &rest[1..];
    if !(after.is_empty() || after.starts_with([' ', '\t'])) {
        return None;
    }
    Some(k)
}

fn strip_comment(s: &str) -> &str {
    // un « # » précédé d'un espace commence un commentaire (hors guillemets)
    let b = s.as_bytes();
    let mut q: Option<u8> = None;
    for i in 0..b.len() {
        match q {
            Some(c) if b[i] == c => q = None,
            Some(_) => {}
            None if b[i] == b'"' || b[i] == b'\'' => q = Some(b[i]),
            None if b[i] == b'#' && (i == 0 || b[i - 1] == b' ' || b[i - 1] == b'\t') => return s[..i].trim_end(),
            None => {}
        }
    }
    s.trim_end()
}

fn parse_value(raw: &str) -> Value {
    let mut lines = raw.split('\n');
    let first = lines.next().unwrap_or("");
    let colon = match first.strip_prefix('"') {
        Some(s) => s.find('"').map(|e| 2 + e + s[e + 1..].find(':').unwrap_or(0)).unwrap_or(0),
        None => first.find(':').unwrap_or(0),
    };
    let inline = strip_comment(first[colon + 1..].trim());
    let rest: Vec<&str> = lines.filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#')).collect();
    if inline.is_empty() {
        if rest.is_empty() {
            return Value::Null;
        }
        if rest.iter().all(|l| l.trim_start().starts_with("- ") || l.trim() == "-") {
            return Value::List(rest.iter().map(|l| scalar(strip_comment(l.trim_start()[1..].trim()))).collect());
        }
        return Value::Complex;
    }
    if inline == "|"
        || inline == ">"
        || inline.starts_with("|-")
        || inline.starts_with(">-")
        || inline.starts_with("|+")
        || inline.starts_with(">+")
    {
        let indent = rest.iter().map(|l| l.len() - l.trim_start().len()).min().unwrap_or(0);
        let parts: Vec<&str> =
            raw.split('\n').skip(1).map(|l| if l.len() >= indent { &l[indent..] } else { l.trim() }).collect();
        let mut parts: Vec<&str> = parts;
        while parts.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
            parts.pop();
        }
        let joined = if inline.starts_with('>') { parts.join(" ") } else { parts.join("\n") };
        return Value::Str(joined);
    }
    if inline.starts_with('[') {
        let mut all = inline.to_string();
        for l in &rest {
            all.push(' ');
            all.push_str(l.trim());
        }
        if let Some(inner) = all.trim().strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            return Value::List(split_flow(inner).into_iter().map(|s| scalar(&s)).filter(|s| !s.is_empty()).collect());
        }
        return Value::Complex;
    }
    if inline.starts_with('{') {
        return Value::Complex;
    }
    if !rest.is_empty() && !inline.starts_with(['"', '\'']) {
        // scalaire plié sur plusieurs lignes
        let mut s = inline.to_string();
        for l in rest {
            s.push(' ');
            s.push_str(l.trim());
        }
        return Value::Str(s);
    }
    match inline {
        "~" | "null" | "Null" | "NULL" => Value::Null,
        _ => Value::Str(scalar(inline)),
    }
}

fn split_flow(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut q: Option<char> = None;
    for c in s.chars() {
        match q {
            Some(qc) if c == qc => {
                q = None;
                cur.push(c);
            }
            Some(_) => cur.push(c),
            None if c == '"' || c == '\'' => {
                q = Some(c);
                cur.push(c);
            }
            None if c == ',' => {
                out.push(cur.trim().to_string());
                cur.clear();
            }
            None => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// Interprète un scalaire YAML (guillemets simples ou doubles).
pub fn scalar(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('\'') && s.ends_with('\'') {
        return s[1..s.len() - 1].replace("''", "'");
    }
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        let inner = &s[1..s.len() - 1];
        let mut out = String::new();
        let mut it = inner.chars();
        while let Some(c) = it.next() {
            if c == '\\' {
                match it.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('r') => out.push('\r'),
                    Some('0') => out.push('\0'),
                    Some('u') => {
                        let h: String = it.by_ref().take(4).collect();
                        if let Some(ch) = u32::from_str_radix(&h, 16).ok().and_then(char::from_u32) {
                            out.push(ch);
                        }
                    }
                    Some(o) => out.push(o),
                    None => {}
                }
            } else {
                out.push(c);
            }
        }
        return out;
    }
    s.to_string()
}

fn needs_quotes(s: &str) -> bool {
    if s.is_empty() || s != s.trim() {
        return true;
    }
    let lower = s.to_ascii_lowercase();
    if matches!(lower.as_str(), "true" | "false" | "yes" | "no" | "on" | "off" | "null" | "~" | "y" | "n") {
        return true;
    }
    if s.parse::<f64>().is_ok()
        || s.starts_with(|c: char| c.is_ascii_digit() || c == '.' || c == '+' || c == '-') && looks_numeric(s)
    {
        return true;
    }
    if s.starts_with(['-', '?', ':', ',', '[', ']', '{', '}', '#', '&', '*', '!', '|', '>', '\'', '"', '%', '@', '`']) {
        return true;
    }
    s.contains(": ")
        || s.contains(" #")
        || s.ends_with(':')
        || s.contains('\n')
        || s.contains('\t')
        || s.chars().any(|c| c.is_control())
}

fn looks_numeric(s: &str) -> bool {
    s.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | '_' | '+' | '-' | 'e' | 'E' | 'x' | 'o'))
}

/// Représentation YAML d'une chaîne (guillemets seulement si nécessaire).
pub fn quote(s: &str) -> String {
    if !needs_quotes(s) {
        return s.to_string();
    }
    dq(s)
}

fn dq(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn quote_flow(s: &str) -> String {
    if needs_quotes(s) || s.contains([',', '[', ']', '{', '}']) {
        dq(s)
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "---\ntitle: Dentiste\n# un commentaire\nstart: 2026-10-02 14:00  # heure locale\ntags: [santé, \"a, b\"]\ncustom:\n  nested: 1\n  other: [x]\nlist:\n  - un\n  - 'deux'\nempty:\nquoted: \"a: b\"\n---\nCorps *markdown*\n---\npas un en-tête\n";

    #[test]
    fn lecture() {
        let d = Doc::parse(SRC);
        assert_eq!(d.str("title").as_deref(), Some("Dentiste"));
        assert_eq!(d.str("start").as_deref(), Some("2026-10-02 14:00"));
        assert_eq!(d.get("tags"), Value::List(vec!["santé".into(), "a, b".into()]));
        assert_eq!(d.get("custom"), Value::Complex);
        assert_eq!(d.get("list"), Value::List(vec!["un".into(), "deux".into()]));
        assert_eq!(d.get("empty"), Value::Null);
        assert_eq!(d.str("quoted").as_deref(), Some("a: b"));
        assert_eq!(d.body, "Corps *markdown*\n---\npas un en-tête\n");
    }

    #[test]
    fn aller_retour_exact() {
        let d = Doc::parse(SRC);
        assert_eq!(d.to_text(), SRC);
    }

    #[test]
    fn modification_conserve_le_reste() {
        let mut d = Doc::parse(SRC);
        d.set_str("title", "Dentiste (déplacé)");
        d.set_plain("start", "2026-10-03 09:00");
        d.set_list("tags", &["santé".into()]);
        d.set_str("location", "Cabinet: 2e étage");
        let t = d.to_text();
        assert!(t.contains("custom:\n  nested: 1\n  other: [x]\n"));
        assert!(t.contains("# un commentaire\n"));
        assert!(t.contains("start: 2026-10-03 09:00\n"));
        assert!(t.contains("location: \"Cabinet: 2e étage\"\n"));
        let d2 = Doc::parse(&t);
        assert_eq!(d2.str("location").as_deref(), Some("Cabinet: 2e étage"));
        assert_eq!(d2.str("title").as_deref(), Some("Dentiste (déplacé)"));
        d.remove("custom");
        assert!(!d.to_text().contains("nested"));
    }

    #[test]
    fn guillemets() {
        for s in ["true", "12", "-x", "a: b", "#x", "", " x", "a #b", "[x]", "l'été", "2026", "ligne\nsuite", "\"q\""]
        {
            let mut d = Doc::new();
            d.set_str("k", s);
            let d2 = Doc::parse(&d.to_text());
            assert_eq!(
                d2.get("k"),
                if s.is_empty() { Value::Str(String::new()) } else { Value::Str(s.into()) },
                "{s:?}"
            );
        }
        let mut d = Doc::new();
        d.set_list("t", &["a,b".into(), "c".into(), "yes".into()]);
        assert_eq!(Doc::parse(&d.to_text()).get("t").as_list(), vec!["a,b", "c", "yes"]);
    }

    #[test]
    fn sans_entete() {
        let d = Doc::parse("# Titre\ntexte");
        assert!(!d.had_front_matter);
        assert_eq!(d.to_text(), "# Titre\ntexte");
        let d = Doc::parse("---\nnon fermé\n");
        assert!(!d.had_front_matter);
    }

    #[test]
    fn blocs() {
        let d = Doc::parse("---\nnote: |\n  ligne 1\n  ligne 2\nplie: >\n  a\n  b\ncrlf: x\r\n---\n");
        assert_eq!(d.str("note").as_deref(), Some("ligne 1\nligne 2"));
        assert_eq!(d.str("plie").as_deref(), Some("a b"));
        assert_eq!(d.str("crlf").as_deref(), Some("x"));
    }
}
