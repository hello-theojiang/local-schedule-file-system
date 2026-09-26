//! Enveloppe Tauri 2 : délègue tout au cœur (`Api::call`), et ajoute seulement
//! ce qui dépend de l'hôte : choix du dossier, mémorisation, téléchargement des
//! abonnements, enregistrement d'un fichier exporté.

use agenda_core::json::Json;
use agenda_core::Api;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::Manager;

#[derive(Clone)]
struct Host {
    api: Arc<Api>,
    /// fichier de configuration de l'application (dossier choisi)
    config: PathBuf,
    /// journal d'annulation (propre à l'appareil)
    journal: PathBuf,
}

fn ok(v: Json) -> String {
    Json::obj().set("ok", true).set("result", v).to_string()
}

fn err(m: &str) -> String {
    Json::obj().set("ok", false).set("error", m).to_string()
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Fichier partagé avec la CLI (`agenda remind` utilise ainsi le même dossier).
fn cli_config() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| home().map(|h| h.join(".config"))).map(|c| c.join("agenda/config"))
}

fn saved_dir(h: &Host) -> Option<String> {
    let from_app = std::fs::read_to_string(&h.config).ok().and_then(|t| Json::parse(&t).ok()).and_then(|j| j.get("dir").as_str().map(str::to_string));
    from_app
        .or_else(|| std::env::var("AGENDA_DIR").ok().filter(|d| !d.is_empty()))
        .or_else(|| {
            let t = std::fs::read_to_string(cli_config()?).ok()?;
            t.lines().find_map(|l| l.trim().strip_prefix("dir=").map(|d| d.trim().trim_matches('"').to_string()))
        })
}

fn open(h: &Host, path: &str, create: bool) -> String {
    let p = Json::obj().set("path", path).set("create", create).set("journal", h.journal.to_string_lossy().to_string());
    let r = h.api.call("open", &p.to_string());
    if r.starts_with("{\"ok\":true") {
        if let Some(d) = h.config.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = agenda_core::store::atomic_write(&h.config, Json::obj().set("dir", path).to_string().as_bytes());
        #[cfg(desktop)]
        if let Some(c) = cli_config() {
            if !c.exists() {
                if let Some(d) = c.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                let _ = std::fs::write(&c, format!("# écrit par l'application Agenda (utilisé par « agenda remind »)\ndir={path}\n"));
            }
        }
    }
    r
}

fn download(url: &str) -> Result<String, String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("seuls http:// et https:// sont acceptés".into());
    }
    #[cfg(mobile)]
    {
        let resp = ureq::get(url).timeout(std::time::Duration::from_secs(60)).call().map_err(|e| e.to_string())?;
        let mut s = String::new();
        use std::io::Read;
        resp.into_reader().take(32 << 20).read_to_string(&mut s).map_err(|e| e.to_string())?;
        Ok(s)
    }
    #[cfg(desktop)]
    {
        let out = std::process::Command::new("curl")
            .args(["-fsSL", "--max-time", "60", "--max-filesize", "33554432", "--proto", "=http,https", "--proto-redir", "=http,https", "--"])
            .arg(url)
            .output()
            .map_err(|e| format!("curl introuvable : {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        String::from_utf8(out.stdout).map_err(|_| "contenu non UTF-8".into())
    }
}

fn sub_update(h: &Host) -> Json {
    let targets = h.api.call_json("sub_targets", &Json::obj()).unwrap_or(Json::Arr(vec![]));
    let mut out = Vec::new();
    for t in targets.as_arr() {
        let name = t.get("name").str_or("").to_string();
        let r = download(t.get("url").str_or("")).and_then(|text| h.api.call_json("sub_store", &Json::obj().set("name", name.as_str()).set("text", text)));
        out.push(match r {
            Ok(v) => Json::obj().set("name", name).set("events", v.get("events").clone()),
            Err(e) => Json::obj().set("name", name).set("error", e),
        });
    }
    Json::Arr(out)
}

fn host_call(app: &tauri::AppHandle, h: &Host, method: &str, params: &str) -> String {
    let p = Json::parse(params).unwrap_or_else(|_| Json::obj());
    match method {
        "host_info" => ok(Json::obj()
            .set("host", "tauri")
            .set("platform", std::env::consts::OS)
            .set("version", agenda_core::VERSION)
            .set("dir", saved_dir(h))),
        "open" => match p.get("path").as_str() {
            Some(path) if !path.trim().is_empty() => open(h, path.trim(), p.get("create").as_bool().unwrap_or(false)),
            _ => err("chemin manquant"),
        },
        "sub_update" => ok(sub_update(h)),
        "pick_folder" => {
            #[cfg(desktop)]
            {
                use tauri_plugin_dialog::DialogExt;
                match app.dialog().file().set_title("Dossier de l'agenda").blocking_pick_folder() {
                    Some(f) => match f.into_path() {
                        Ok(p) => ok(Json::from(p.to_string_lossy().to_string())),
                        Err(e) => err(&e.to_string()),
                    },
                    None => ok(Json::Null),
                }
            }
            #[cfg(mobile)]
            {
                let _ = app;
                err("indiquez le chemin du dossier")
            }
        }
        "save_file" => {
            let name = p.get("name").str_or("agenda.ics").replace(['/', '\\'], "_");
            let text = p.get("text").str_or("");
            #[cfg(desktop)]
            let target: Option<PathBuf> = {
                use tauri_plugin_dialog::DialogExt;
                app.dialog().file().set_file_name(&name).blocking_save_file().and_then(|f| f.into_path().ok())
            };
            #[cfg(mobile)]
            let target: Option<PathBuf> = {
                let _ = app;
                Some(Path::new("/storage/emulated/0/Download").join(&name))
            };
            match target {
                Some(t) => match agenda_core::store::atomic_write(&t, text.as_bytes()) {
                    Ok(()) => ok(Json::from(t.to_string_lossy().to_string())),
                    Err(e) => err(&e),
                },
                None => ok(Json::Null),
            }
        }
        _ => h.api.call(method, params),
    }
}

#[tauri::command]
async fn api(app: tauri::AppHandle, state: tauri::State<'_, Host>, method: String, params: String) -> Result<String, String> {
    let h = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || host_call(&app, &h, &method, &params)).await.map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Pilote NVIDIA sous Linux : sans cela, la fenêtre WebKitGTK reste souvent blanche.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() && Path::new("/proc/driver/nvidia/version").exists() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let cfg_dir = app.path().app_config_dir()?;
            let data_dir = app.path().app_data_dir()?;
            let host = Host { api: Arc::new(Api::new()), config: cfg_dir.join("config.json"), journal: data_dir.join("undo.json") };
            if let Some(dir) = saved_dir(&host) {
                // un dossier devenu inaccessible renvoie simplement à l'écran d'accueil
                let _ = open(&host, &dir, false);
            }
            app.manage(host);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![api])
        .run(tauri::generate_context!())
        .expect("impossible de démarrer l'application");
}
