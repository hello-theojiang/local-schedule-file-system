//! `agenda serve` : interface web, API JSON et flux `/agenda.ics`.
//!
//! Sécurité :
//! - écoute sur 127.0.0.1 par défaut ;
//! - jeton optionnel (`--token` ou `AGENDA_TOKEN`) : en-tête `Authorization: Bearer`,
//!   paramètre `?token=` (qui pose un cookie HttpOnly) ;
//! - en écoute locale, l'en-tête Host doit être local (protection contre le DNS rebinding) ;
//! - les appels à l'API exigent `Content-Type: application/json` et une origine identique
//!   (protection CSRF).

use crate::Ctx;
use agenda_core::json::Json;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

include!(concat!(env!("OUT_DIR"), "/assets.rs"));

const MAX_BODY: usize = 16 * 1024 * 1024;
const MAX_CONN: usize = 64;

struct Server {
    ctx: Ctx,
    token: Option<String>,
    local_only: bool,
    port: u16,
}

struct Req {
    method: String,
    path: String,
    query: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Req {
    fn header(&self, k: &str) -> Option<&str> {
        self.headers.iter().find(|(h, _)| h.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str())
    }
    fn query_param(&self, k: &str) -> Option<String> {
        self.query.split('&').filter_map(|kv| kv.split_once('=')).find(|(a, _)| *a == k).map(|(_, v)| url_decode(v))
    }
}

fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 2;
                } else {
                    out.push(b'%');
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" | "webmanifest" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "txt" | "md" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn read_req(s: &mut TcpStream) -> Result<Req, String> {
    let mut r = BufReader::new(s.try_clone().map_err(|e| e.to_string())?);
    let mut line = String::new();
    r.read_line(&mut line).map_err(|e| e.to_string())?;
    let mut parts = line.split_whitespace();
    let method = parts.next().ok_or("requête vide")?.to_string();
    let target = parts.next().ok_or("requête invalide")?.to_string();
    let (path, query) =
        target.split_once('?').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or((target, String::new()));
    let mut headers = Vec::new();
    let mut total = 0;
    loop {
        let mut h = String::new();
        let n = r.read_line(&mut h).map_err(|e| e.to_string())?;
        total += n;
        if n == 0 || h == "\r\n" || h == "\n" || total > 64 * 1024 {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let len: usize = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    if len > MAX_BODY {
        return Err("corps trop volumineux".into());
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body).map_err(|e| e.to_string())?;
    Ok(Req { method, path: url_decode(&path), query, headers, body })
}

fn respond(s: &mut TcpStream, code: u16, ctype: &str, body: &[u8], extra: &[(&str, String)]) {
    let reason = match code {
        200 => "OK",
        204 => "No Content",
        302 => "Found",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        503 => "Service Unavailable",
        _ => "Error",
    };
    let mut head = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nX-Frame-Options: DENY\r\n",
        body.len()
    );
    for (k, v) in extra {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    let _ = s.write_all(head.as_bytes());
    let _ = s.write_all(body);
    let _ = s.flush();
}

fn json_err(s: &mut TcpStream, code: u16, msg: &str) {
    respond(s, code, "application/json", Json::obj().set("ok", false).set("error", msg).to_string().as_bytes(), &[]);
}

impl Server {
    fn authorized(&self, r: &Req) -> bool {
        let Some(t) = &self.token else { return true };
        let bearer = r.header("authorization").and_then(|h| h.strip_prefix("Bearer ")).map(str::trim);
        let cookie = r
            .header("cookie")
            .and_then(|c| c.split(';').map(str::trim).find_map(|kv| kv.strip_prefix("agenda_token=")));
        let q = r.query_param("token");
        [bearer.map(str::to_string), cookie.map(str::to_string), q]
            .iter()
            .flatten()
            .any(|v| ct_eq(v.as_bytes(), t.as_bytes()))
    }

    fn host_ok(&self, r: &Req) -> bool {
        if !self.local_only {
            return true;
        }
        let host = r.header("host").unwrap_or("");
        let name = if host.starts_with('[') {
            host.split(']').next().unwrap_or("").trim_start_matches('[')
        } else {
            host.split(':').next().unwrap_or("")
        };
        matches!(name, "localhost" | "127.0.0.1" | "::1") || name.ends_with(".localhost")
    }

    fn origin_ok(&self, r: &Req) -> bool {
        match r.header("origin") {
            None => true,
            Some(o) => {
                let host = r.header("host").unwrap_or("");
                o == format!("http://{host}") || o == format!("https://{host}") || o == "tauri://localhost"
            }
        }
    }

    fn handle(&self, mut s: TcpStream) {
        let _ = s.set_read_timeout(Some(Duration::from_secs(30)));
        let r = match read_req(&mut s) {
            Ok(r) => r,
            Err(e) => return json_err(&mut s, 400, &e),
        };
        if !self.host_ok(&r) {
            return json_err(&mut s, 403, "hôte non autorisé (utilisez http://127.0.0.1)");
        }
        // jeton passé dans l'URL : cookie puis redirection pour le retirer de l'adresse
        if r.method == "GET"
            && (r.path == "/" || r.path == "/index.html")
            && r.query_param("token").is_some()
            && self.authorized(&r)
        {
            let t = r.query_param("token").unwrap_or_default();
            return respond(
                &mut s,
                302,
                "text/plain",
                b"",
                &[
                    ("Location", "/".into()),
                    ("Set-Cookie", format!("agenda_token={t}; HttpOnly; SameSite=Strict; Path=/; Max-Age=31536000")),
                ],
            );
        }
        let public_asset = r.method == "GET" && !r.path.starts_with("/api/") && r.path != "/agenda.ics";
        if !public_asset && !self.authorized(&r) {
            return json_err(&mut s, 401, "jeton manquant ou invalide");
        }
        match (r.method.as_str(), r.path.as_str()) {
            ("GET", "/health") => respond(&mut s, 200, "text/plain", b"ok", &[]),
            ("GET", "/agenda.ics") => {
                let mut p = Json::obj().set("include_subscriptions", r.query_param("subs").as_deref() == Some("1"));
                if let Some(c) = r.query_param("calendars") {
                    p.insert("calendars", c);
                }
                match self.ctx.api.call_json("ics_export", &p) {
                    Ok(v) => respond(
                        &mut s,
                        200,
                        "text/calendar; charset=utf-8",
                        v.as_str().unwrap_or("").as_bytes(),
                        &[("Cache-Control", "no-cache".into())],
                    ),
                    Err(e) => json_err(&mut s, 500, &e),
                }
            }
            ("POST", p) if p.starts_with("/api/") => {
                if !r.header("content-type").map(|c| c.starts_with("application/json")).unwrap_or(false)
                    || !self.origin_ok(&r)
                {
                    return json_err(&mut s, 403, "requête refusée (Content-Type JSON et même origine requis)");
                }
                let method = &p[5..];
                let body = String::from_utf8_lossy(&r.body);
                let out = match method {
                    // méthodes propres à l'hôte
                    "sub_update" => Json::obj()
                        .set("ok", true)
                        .set("result", crate::fetch::update_subscriptions(&self.ctx.api))
                        .to_string(),
                    "host_info" => Json::obj()
                        .set("ok", true)
                        .set(
                            "result",
                            Json::obj()
                                .set("host", "serve")
                                .set("version", agenda_core::VERSION)
                                .set("dir", self.ctx.dir.to_string_lossy().to_string())
                                .set("port", self.port as i64),
                        )
                        .to_string(),
                    "open" => Json::obj()
                        .set("ok", false)
                        .set("error", "le dossier est fixé au lancement de « agenda serve »")
                        .to_string(),
                    m => self.ctx.api.call(m, &body),
                };
                respond(&mut s, 200, "application/json", out.as_bytes(), &[("Cache-Control", "no-store".into())]);
            }
            ("GET", path) => {
                let rel = path.trim_start_matches('/');
                let rel = if rel.is_empty() { "index.html" } else { rel };
                match ASSETS.iter().find(|(p, _)| *p == rel) {
                    Some((p, data)) => {
                        let cache = if p.ends_with(".woff2") || p.ends_with(".png") {
                            "public, max-age=604800"
                        } else {
                            "no-cache"
                        };
                        let mut extra = vec![("Cache-Control", cache.to_string())];
                        if p.ends_with(".html") {
                            extra.push(("Content-Security-Policy", "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; font-src 'self'; connect-src 'self'; frame-ancestors 'none'".into()));
                        }
                        respond(&mut s, 200, content_type(p), data, &extra)
                    }
                    None => json_err(&mut s, 404, "introuvable"),
                }
            }
            _ => json_err(&mut s, 405, "méthode non autorisée"),
        }
    }
}

/// Comparaison en temps constant (jeton).
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn run(ctx: Ctx, addr: &str, token: Option<String>) -> Result<(), String> {
    let token = token.or_else(|| std::env::var("AGENDA_TOKEN").ok()).filter(|t| !t.is_empty());
    let listener = TcpListener::bind(addr).map_err(|e| format!("écoute sur {addr} impossible : {e}"))?;
    let local = listener.local_addr().map_err(|e| e.to_string())?;
    let local_only = local.ip().is_loopback();
    if !local_only && token.is_none() {
        eprintln!(
            "⚠ agenda serve écoute sur {local} sans jeton : toute personne du réseau peut lire et modifier l'agenda."
        );
    }
    eprintln!(
        "Agenda : http://{local}/  (dossier {}){}",
        ctx.dir.display(),
        if token.is_some() { " — jeton requis" } else { "" }
    );
    eprintln!("Flux iCalendar : http://{local}/agenda.ics{}", if token.is_some() { "?token=…" } else { "" });
    let server = Arc::new(Server { ctx, token, local_only, port: local.port() });
    // abonnements : mise à jour au démarrage puis toutes les 6 heures
    {
        let srv = server.clone();
        std::thread::spawn(move || loop {
            let _ = crate::fetch::update_subscriptions(&srv.ctx.api);
            std::thread::sleep(Duration::from_secs(6 * 3600));
        });
    }
    let active = Arc::new(AtomicUsize::new(0));
    for conn in listener.incoming() {
        let Ok(mut stream) = conn else { continue };
        if active.load(Ordering::Relaxed) >= MAX_CONN {
            json_err(&mut stream, 503, "trop de connexions");
            continue;
        }
        active.fetch_add(1, Ordering::Relaxed);
        let srv = server.clone();
        let act = active.clone();
        let spawned = std::thread::Builder::new().stack_size(256 * 1024).spawn(move || {
            srv.handle(stream);
            act.fetch_sub(1, Ordering::Relaxed);
        });
        if spawned.is_err() {
            active.fetch_sub(1, Ordering::Relaxed);
        }
    }
    Ok(())
}
