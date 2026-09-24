//! Téléchargement des abonnements iCalendar via `curl` (le cœur n'a pas de TLS).
//! Aucun shell : l'URL est passée en argument, et seuls http/https sont acceptés
//! (pas de `file://` qui permettrait de lire un fichier local).

use agenda_core::json::Json;
use agenda_core::Api;
use std::process::Command;

pub fn download(url: &str) -> Result<String, String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("seuls http:// et https:// sont acceptés".into());
    }
    let out = Command::new("curl")
        .args([
            "-fsSL",
            "--max-time",
            "60",
            "--max-filesize",
            "33554432",
            "--proto",
            "=http,https",
            "--proto-redir",
            "=http,https",
        ])
        .args(["-A", concat!("agenda/", env!("CARGO_PKG_VERSION"))])
        .arg("--")
        .arg(url)
        .output()
        .map_err(|e| format!("curl introuvable ({e}) : installez curl"))?;
    if !out.status.success() {
        return Err(format!("téléchargement échoué : {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    String::from_utf8(out.stdout).map_err(|_| "contenu non UTF-8".into())
}

/// Télécharge chaque abonnement et le range dans `.cache/subscriptions/`.
pub fn update_subscriptions(api: &Api) -> Json {
    let targets = api.call_json("sub_targets", &Json::obj()).unwrap_or(Json::Arr(vec![]));
    let mut out = Vec::new();
    for t in targets.as_arr() {
        let name = t.get("name").str_or("").to_string();
        let url = t.get("url").str_or("").to_string();
        let r = download(&url)
            .and_then(|text| api.call_json("sub_store", &Json::obj().set("name", name.as_str()).set("text", text)));
        out.push(match r {
            Ok(v) => Json::obj().set("name", name).set("events", v.get("events").clone()),
            Err(e) => Json::obj().set("name", name).set("error", e),
        });
    }
    Json::Arr(out)
}
