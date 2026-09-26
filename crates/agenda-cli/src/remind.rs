//! `agenda remind` : déclenche les rappels à l'heure dite.
//!
//! - par défaut : notify-send (Linux) ou osascript (macOS) ;
//! - `--exec CMD` : lance `sh -c CMD` ; titre, texte et heure sont transmis **uniquement**
//!   par variables d'environnement (AGENDA_TITLE, AGENDA_BODY, AGENDA_AT, AGENDA_START,
//!   AGENDA_ID, AGENDA_KIND, AGENDA_LOCATION) : aucune injection possible dans la commande ;
//! - `--ntfy URL` : envoie vers ntfy avec curl (arguments séparés, pas de shell).
//!
//! L'heure du dernier passage est conservée : un rappel manqué pendant une coupure de
//! moins d'une heure est rattrapé, jamais envoyé deux fois.

use crate::Ctx;
use agenda_core::date::now_utc;
use agenda_core::json::Json;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const CATCH_UP: i64 = 3600;

fn state_file(dir: &std::path::Path) -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).unwrap_or_else(|| {
        std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")).join(".local/state")
    });
    let h = agenda_core::store::hash(dir.to_string_lossy().as_bytes());
    base.join("agenda").join(format!("remind-{h:016x}"))
}

fn read_state(p: &std::path::Path) -> Option<i64> {
    std::fs::read_to_string(p).ok()?.trim().parse().ok()
}

fn write_state(p: &std::path::Path, t: i64) {
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = agenda_core::store::atomic_write(p, t.to_string().as_bytes());
}

fn pct(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

enum Sink {
    Desktop,
    Exec(String),
    Ntfy(String),
}

fn fire(sink: &Sink, a: &Json, quiet: bool) {
    let title = a.get("title").str_or("Rappel").to_string();
    let body = a.get("body").str_or("").to_string();
    let env = [
        ("AGENDA_TITLE", title.clone()),
        ("AGENDA_BODY", body.clone()),
        ("AGENDA_AT", a.get("at").str_or("").to_string()),
        ("AGENDA_START", a.get("start").str_or("").to_string()),
        ("AGENDA_ID", a.get("id").str_or("").to_string()),
        ("AGENDA_KIND", a.get("kind").str_or("").to_string()),
        ("AGENDA_LOCATION", a.get("location").str_or("").to_string()),
    ];
    let mut cmd = match sink {
        Sink::Exec(c) => {
            let mut k = Command::new("sh");
            k.arg("-c").arg(c);
            k
        }
        Sink::Ntfy(url) => {
            let sep = if url.contains('?') { '&' } else { '?' };
            let mut k = Command::new("curl");
            k.args([
                "-fsS",
                "--max-time",
                "20",
                "--proto",
                "=https,http",
                "-H",
                "Tags: calendar",
                "--data-binary",
                "@-",
            ]);
            k.arg("--").arg(format!("{url}{sep}title={}", pct(&title)));
            k
        }
        Sink::Desktop => {
            if cfg!(target_os = "macos") {
                let mut k = Command::new("osascript");
                k.args(["-e", "display notification (system attribute \"AGENDA_BODY\") with title (system attribute \"AGENDA_TITLE\") sound name \"default\""]);
                k
            } else {
                let mut k = Command::new("notify-send");
                k.args(["-a", "Agenda", "-i", "agenda", "--"]).arg(&title).arg(&body);
                k
            }
        }
    };
    cmd.envs(env);
    let piped = matches!(sink, Sink::Ntfy(_));
    cmd.stdin(if piped { Stdio::piped() } else { Stdio::null() });
    match cmd.spawn() {
        Ok(mut child) => {
            if piped {
                if let Some(mut w) = child.stdin.take() {
                    use std::io::Write;
                    let _ = w.write_all(body.as_bytes());
                }
            }
            let status = child.wait();
            if !quiet {
                eprintln!(
                    "[{}] rappel : {title} — {body}{}",
                    a.get("at").str_or(""),
                    if status.map(|s| s.success()).unwrap_or(false) { "" } else { " (échec de l'envoi)" }
                );
            }
        }
        Err(e) => eprintln!("agenda remind : impossible de lancer la notification : {e}"),
    }
}

pub fn run(ctx: &Ctx, exec: Option<&str>, ntfy: Option<&str>, once: bool, quiet: bool) -> Result<(), String> {
    let sink = match (exec, ntfy) {
        (Some(_), Some(_)) => return Err("--exec et --ntfy sont exclusifs".into()),
        (Some(c), None) => Sink::Exec(c.to_string()),
        (None, Some(u)) => {
            if !(u.starts_with("https://") || u.starts_with("http://")) {
                return Err("--ntfy attend une URL http(s)://…".into());
            }
            Sink::Ntfy(u.to_string())
        }
        (None, None) => Sink::Desktop,
    };
    let sf = state_file(&ctx.dir);
    if !quiet {
        eprintln!("agenda remind : dossier {} (état : {})", ctx.dir.display(), sf.display());
    }
    let mut last = read_state(&sf).unwrap_or_else(now_utc).max(now_utc() - CATCH_UP);
    let mut version = 0i64;
    loop {
        let now = now_utc();
        if now > last {
            let due = ctx.api.call_json("alarms", &Json::obj().set("from_utc", last + 1).set("to_utc", now + 1))?;
            for a in due.as_arr() {
                fire(&sink, a, quiet);
            }
            last = now;
        }
        // toujours mémoriser le passage, même sans rappel : rien ne partira deux fois
        write_state(&sf, last);
        if once {
            return Ok(());
        }
        // prochain rappel dans les 24 h, sinon nouvelle vérification dans 5 minutes
        let next = ctx
            .api
            .call_json("alarms", &Json::obj().set("from_utc", now + 1).set("to_utc", now + 86_400))?
            .as_arr()
            .first()
            .and_then(|a| a.get("at_utc").as_i64())
            .unwrap_or(now + 300);
        let wait_ms = (next - now).clamp(1, 300) * 1000;
        // attend le prochain rappel, ou une modification du dossier (qui peut en ajouter)
        let r = ctx.api.call_json("changes", &Json::obj().set("since", version).set("wait", wait_ms))?;
        version = r.get("version").as_i64().unwrap_or(version);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_sans_injection() {
        let dir = std::env::temp_dir().join(format!("agenda-remind-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("out.txt");
        // un titre malveillant ne doit jamais être exécuté
        let a = Json::obj()
            .set("title", "$(touch pwned); `touch pwned2`")
            .set("body", "corps")
            .set("at", "2026-10-02 13:45");
        let cmd = format!("printf '%s|%s' \"$AGENDA_TITLE\" \"$AGENDA_BODY\" > {}", out.display());
        fire(&Sink::Exec(cmd), &a, true);
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "$(touch pwned); `touch pwned2`|corps");
        assert!(!std::path::Path::new("pwned").exists() && !std::path::Path::new("pwned2").exists());
        assert_eq!(pct("Réunion & co"), "R%C3%A9union%20%26%20co");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
