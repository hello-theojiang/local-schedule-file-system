//! Recherche insensible à la casse et aux accents.

/// Minuscules sans accents : « Été à l'Œuvre » → « ete a l'oeuvre ».
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii() {
            out.push(c.to_ascii_lowercase());
            continue;
        }
        let r: &str = match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' | 'ā' | 'ă' | 'ą' | 'Ā' | 'Ă'
            | 'Ą' => "a",
            'æ' | 'Æ' => "ae",
            'ç' | 'Ç' | 'ć' | 'č' | 'Ć' | 'Č' | 'ĉ' | 'ċ' => "c",
            'ď' | 'đ' | 'Ď' | 'Đ' => "d",
            'è' | 'é' | 'ê' | 'ë' | 'È' | 'É' | 'Ê' | 'Ë' | 'ē' | 'ė' | 'ę' | 'ě' | 'Ē' | 'Ė' | 'Ę' | 'Ě' => {
                "e"
            }
            'ğ' | 'Ğ' | 'ģ' => "g",
            'ì' | 'í' | 'î' | 'ï' | 'Ì' | 'Í' | 'Î' | 'Ï' | 'ī' | 'į' | 'ı' | 'İ' => "i",
            'ł' | 'Ł' | 'ľ' | 'ĺ' => "l",
            'ñ' | 'Ñ' | 'ń' | 'ň' | 'Ń' | 'Ň' => "n",
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' | 'ō' | 'ő' | 'Ō' | 'Ő' => {
                "o"
            }
            'œ' | 'Œ' => "oe",
            'ř' | 'Ř' | 'ŕ' => "r",
            'ś' | 'š' | 'ş' | 'Ś' | 'Š' | 'Ş' | 'ș' | 'Ș' => "s",
            'ß' => "ss",
            'ť' | 'ţ' | 'Ť' | 'ț' | 'Ț' => "t",
            'ù' | 'ú' | 'û' | 'ü' | 'Ù' | 'Ú' | 'Û' | 'Ü' | 'ū' | 'ů' | 'ű' | 'ų' | 'Ū' | 'Ů' => "u",
            'ý' | 'ÿ' | 'Ý' | 'Ÿ' => "y",
            'ž' | 'ź' | 'ż' | 'Ž' | 'Ź' | 'Ż' => "z",
            '’' | '‘' | 'ʼ' => "'",
            '«' | '»' | '“' | '”' => "\"",
            '–' | '—' => "-",
            '\u{a0}' | '\u{202f}' => " ",
            _ => {
                for l in c.to_lowercase() {
                    out.push(l);
                }
                continue;
            }
        };
        out.push_str(r);
    }
    out
}

/// Requête découpée en termes ; tous doivent apparaître.
pub struct Query {
    terms: Vec<String>,
}

impl Query {
    pub fn new(q: &str) -> Query {
        Query {
            terms: fold(q)
                .split_whitespace()
                .map(|t| t.trim_start_matches('#').to_string())
                .filter(|t| !t.is_empty())
                .collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// Score > 0 si tous les termes sont présents ; bonus si dans le titre.
    pub fn score(&self, title: &str, rest: &str) -> u32 {
        if self.terms.is_empty() {
            return 1;
        }
        let t = fold(title);
        let r = fold(rest);
        let mut score = 0;
        for term in &self.terms {
            if t.contains(term.as_str()) {
                score += if t.starts_with(term.as_str()) || t.contains(&format!(" {term}")) { 4 } else { 3 };
            } else if r.contains(term.as_str()) {
                score += 1;
            } else {
                return 0;
            }
        }
        score
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repli() {
        assert_eq!(fold("Été à l’Œuvre ÇA"), "ete a l'oeuvre ca");
        let q = Query::new("reunion ÉQUIPE");
        assert!(q.score("Réunion d'équipe", "") > 0);
        assert_eq!(q.score("Réunion", "rien"), 0);
        assert!(q.score("Point", "réunion équipe") > 0);
        assert!(Query::new("  ").is_empty());
    }
}
