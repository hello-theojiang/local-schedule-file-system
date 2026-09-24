//! Mesures sur un dossier de 6 000 fichiers : `cargo run --release --example bench`.
use agenda_core::json::Json;
use agenda_core::Api;
use std::time::Instant;

fn rss_kb() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    s.lines().find(|l| l.starts_with("VmRSS:"))?.split_whitespace().nth(1)?.parse().ok()
}

fn main() {
    let dir = std::env::temp_dir().join(format!("agenda-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let titles =
        ["Réunion d'équipe", "Dentiste", "Sport", "Cours de maths", "Déjeuner", "Appel client", "Révisions", "Concert"];
    let t0 = Instant::now();
    for i in 0..5000 {
        let m = 1 + (i % 12);
        let d = 1 + (i % 28);
        let h = 8 + (i % 11);
        let sub = dir.join(format!("events/2026-{m:02}"));
        std::fs::create_dir_all(&sub).unwrap();
        let repeat = if i % 50 == 0 { "repeat: FREQ=WEEKLY;BYDAY=MO,WE\n" } else { "" };
        std::fs::write(
            sub.join(format!("ev-{i}.md")),
            format!("---\ntitle: {} {i}\nstart: 2026-{m:02}-{d:02} {h:02}:00\nend: 2026-{m:02}-{d:02} {h:02}:45\ncalendar: perso\ntags: [bench, t{}]\n{repeat}alarm: [15m]\nprojet: champ inconnu\n---\nNotes de l'événement {i}.\n", titles[i % titles.len()], i % 7),
        )
        .unwrap();
    }
    std::fs::create_dir_all(dir.join("tasks")).unwrap();
    for i in 0..1000 {
        let st = ["todo", "doing", "done", "waiting"][i % 4];
        std::fs::write(
            dir.join(format!("tasks/t-{i}.md")),
            format!(
                "---\ntitle: Tâche {i}\nstatus: {st}\ndue: 2026-10-{:02}\npriority: high\n---\n- [ ] sous-tâche\n",
                1 + i % 28
            ),
        )
        .unwrap();
    }
    println!("génération de 6 000 fichiers : {:.0} ms", t0.elapsed().as_secs_f64() * 1e3);
    let rss0 = rss_kb();

    let api = Api::new();
    api.call_json("set_timezone", &Json::obj().set("tz", "Europe/Paris")).unwrap();
    let t = Instant::now();
    let info = api.call_json("open", &Json::obj().set("path", dir.to_string_lossy().to_string())).unwrap();
    let open_ms = t.elapsed().as_secs_f64() * 1e3;
    assert_eq!(info.get("events").as_i64(), Some(5000));
    assert_eq!(info.get("tasks").as_i64(), Some(1000));

    let mut best = f64::MAX;
    let mut n = 0;
    for _ in 0..20 {
        let t = Instant::now();
        let r = api.call("list", r#"{"from":"2026-10-01","to":"2026-10-31"}"#);
        best = best.min(t.elapsed().as_secs_f64() * 1e3);
        n = r.matches("\"id\"").count();
    }
    let t = Instant::now();
    let s = api.call_json("search", &Json::obj().set("q", "reunion equipe 42")).unwrap();
    let search_ms = t.elapsed().as_secs_f64() * 1e3;
    let t = Instant::now();
    api.call_json("brief", &Json::obj()).unwrap();
    let brief_ms = t.elapsed().as_secs_f64() * 1e3;
    let t = Instant::now();
    let parsed = api.call_json("parse", &Json::obj().set("text", "Dentiste vendredi 14h-15h @Cabinet #santé")).unwrap();
    let parse_us = t.elapsed().as_secs_f64() * 1e6;
    let t = Instant::now();
    api.call_json("alarms", &Json::obj().set("from", "2026-10-01").set("to", "2026-10-08")).unwrap();
    let alarms_ms = t.elapsed().as_secs_f64() * 1e3;
    let t = Instant::now();
    let ics = api.call_json("ics_export", &Json::obj()).unwrap();
    let export_ms = t.elapsed().as_secs_f64() * 1e3;
    let rss1 = rss_kb();

    println!("ouverture (6 000 fichiers)   : {open_ms:.1} ms");
    println!("calcul d'un mois ({n} occ.)  : {best:.2} ms (meilleur de 20)");
    println!("recherche                    : {search_ms:.2} ms ({} résultats)", s.as_arr().len());
    println!("brief                        : {brief_ms:.2} ms");
    println!("langage naturel              : {parse_us:.0} µs ({})", parsed.get("summary").as_str().unwrap_or(""));
    println!("rappels sur 7 jours          : {alarms_ms:.2} ms");
    println!("export iCalendar             : {export_ms:.1} ms ({} Ko)", ics.as_str().unwrap_or("").len() / 1024);
    if let (Some(a), Some(b)) = (rss0, rss1) {
        println!(
            "mémoire (RSS)                : {:.1} Mo avant ouverture, {:.1} Mo après",
            a as f64 / 1024.0,
            b as f64 / 1024.0
        );
    }
    if std::env::var("GARDER").is_ok() {
        println!("dossier conservé : {}", dir.display());
    } else {
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
