//! `agenda mcp` : serveur MCP (Model Context Protocol) sur stdio.
//! Messages JSON-RPC 2.0, un par ligne.

use crate::Ctx;
use agenda_core::json::Json;
use std::io::{BufRead, Write};

const INSTRUCTIONS: &str = "Agenda et tâches de l'utilisateur, stockés dans un dossier de fichiers Markdown/YAML. \
Commencez par agenda_brief pour avoir la situation. Les dates s'écrivent AAAA-MM-JJ ou AAAA-MM-JJ HH:MM (heure locale). \
agenda_add accepte une phrase en français (« Dentiste vendredi 14h-15h @Cabinet #santé »). \
Les identifiants sont des chemins (events/2026-10/dentiste.md). Rien n'est effacé : les suppressions vont dans .trash/. \
Pour modifier, passez seulement les champs à changer ; une valeur null supprime le champ.";

fn prop(t: &str, d: &str) -> Json {
    Json::obj().set("type", t).set("description", d)
}

fn arr(d: &str) -> Json {
    Json::obj().set("type", "array").set("items", Json::obj().set("type", "string")).set("description", d)
}

fn schema(props: Vec<(&'static str, Json)>, required: &[&str]) -> Json {
    let mut p = Json::obj();
    for (k, v) in props {
        p.insert(k, v);
    }
    Json::obj()
        .set("type", "object")
        .set("properties", p)
        .set("required", required.iter().map(|s| s.to_string()).collect::<Vec<_>>())
}

fn tools() -> Vec<(&'static str, &'static str, Json)> {
    let date = "AAAA-MM-JJ ou AAAA-MM-JJ HH:MM";
    vec![
        ("agenda_brief", "Résumé de la situation (~500 tokens) : aujourd'hui, jours à venir, tâches en retard ou à échéance, conflits.", schema(vec![("days", prop("integer", "horizon en jours (défaut 7)"))], &[])),
        (
            "agenda_list",
            "Événements (occurrences, répétitions développées) et tâches à échéance sur une période.",
            schema(vec![("from", prop("string", date)), ("to", prop("string", &format!("{date} (date seule : incluse)"))), ("days", prop("integer", "si « to » absent (défaut 7)")), ("calendars", arr("filtrer par calendriers"))], &[]),
        ),
        ("agenda_search", "Recherche plein texte insensible aux accents dans les événements et les tâches.", schema(vec![("query", prop("string", "mots à chercher"))], &["query"])),
        (
            "agenda_free",
            "Créneaux libres entre deux dates, dans une plage horaire quotidienne.",
            schema(
                vec![("from", prop("string", date)), ("to", prop("string", date)), ("min_minutes", prop("integer", "durée minimale (défaut 30)")), ("day_start", prop("string", "HH:MM (défaut 08:00)")), ("day_end", prop("string", "HH:MM (défaut 20:00)"))],
                &[],
            ),
        ),
        ("agenda_add", "Ajoute un événement ou une tâche décrit en français naturel. Renvoie l'identifiant et un résumé à vérifier.", schema(vec![("text", prop("string", "ex. « Sport tous les mardis 18h30 pendant 1h30 », « Rapport demain !haute »"))], &["text"])),
        (
            "agenda_add_event",
            "Ajoute un événement avec des champs explicites.",
            schema(
                vec![
                    ("title", prop("string", "titre")),
                    ("start", prop("string", &format!("{date} ; date seule = journée entière"))),
                    ("end", prop("string", "fin ; journée entière : dernier jour inclus")),
                    ("location", prop("string", "lieu")),
                    ("calendar", prop("string", "nom du calendrier")),
                    ("tags", arr("mots-clés")),
                    ("repeat", prop("string", "RRULE, ex. FREQ=WEEKLY;BYDAY=TU")),
                    ("alarm", arr("rappels avant le début, ex. [\"15m\", \"1d\"]")),
                    ("notes", prop("string", "notes Markdown")),
                ],
                &["title", "start"],
            ),
        ),
        (
            "agenda_add_task",
            "Ajoute une tâche.",
            schema(
                vec![
                    ("title", prop("string", "titre")),
                    ("due", prop("string", date)),
                    ("priority", prop("string", "high, medium ou low")),
                    ("tags", arr("mots-clés")),
                    ("repeat", prop("string", "RRULE : terminer la tâche la reporte")),
                    ("notes", prop("string", "notes Markdown")),
                ],
                &["title"],
            ),
        ),
        ("agenda_get", "Élément complet (tous les champs, corps Markdown, révision).", schema(vec![("id", prop("string", "identifiant (chemin)"))], &["id"])),
        (
            "agenda_update",
            "Modifie des champs d'un élément. Fusion avec les modifications concurrentes : en cas de désaccord, la version déjà sur disque est gardée et un conflit est signalé.",
            schema(vec![("id", prop("string", "identifiant")), ("fields", Json::obj().set("type", "object").set("description", "champs à changer (null supprime), « body » pour les notes")), ("rev", prop("string", "révision lue (facultatif)"))], &["id", "fields"]),
        ),
        ("agenda_complete_task", "Termine une tâche (ou la rouvre avec done=false). Une tâche répétée passe à l'échéance suivante.", schema(vec![("id", prop("string", "identifiant")), ("done", prop("boolean", "défaut true"))], &["id"])),
        ("agenda_delete", "Met un élément à la corbeille (.trash/), ou supprime une seule occurrence d'une série.", schema(vec![("id", prop("string", "identifiant")), ("occurrence", prop("string", "occurrence à supprimer (champ « occurrence » de agenda_list)"))], &["id"])),
        ("agenda_tasks", "Liste des tâches ouvertes (ou toutes).", schema(vec![("include_done", prop("boolean", "inclure les tâches terminées"))], &[])),
    ]
}

fn call_tool(ctx: &Ctx, name: &str, a: &Json) -> Result<String, String> {
    let api = |m: &str, p: Json| ctx.api.call_json(m, &p);
    let copy = |keys: &[(&str, &str)]| {
        let mut p = Json::obj();
        for (from, to) in keys {
            if !a.get(from).is_null() {
                p.insert(to, a.get(from).clone());
            }
        }
        p
    };
    let v = match name {
        "agenda_brief" => return api("brief", copy(&[("days", "days")])).map(|v| v.get("text").str_or("").to_string()),
        "agenda_list" => api(
            "list",
            copy(&[("from", "from"), ("to", "to"), ("days", "days"), ("calendars", "calendars")]).set("tasks", true),
        )?,
        "agenda_search" => api("search", Json::obj().set("q", a.get("query").str_or("")))?,
        "agenda_free" => api(
            "free",
            copy(&[
                ("from", "from"),
                ("to", "to"),
                ("min_minutes", "min"),
                ("day_start", "day_start"),
                ("day_end", "day_end"),
            ]),
        )?,
        "agenda_add" => api("add", Json::obj().set("text", a.get("text").str_or("")))?,
        "agenda_add_event" | "agenda_add_task" => {
            let mut f = Json::obj();
            for (k, v) in a.as_obj() {
                let key = if k == "notes" { "body" } else { k.as_ref() };
                f.insert(key, v.clone());
            }
            api(
                "create",
                Json::obj().set("kind", if name == "agenda_add_event" { "event" } else { "task" }).set("fields", f),
            )?
        }
        "agenda_get" => api("get", copy(&[("id", "id")]))?,
        "agenda_update" => api("update", copy(&[("id", "id"), ("fields", "fields"), ("rev", "rev")]))?,
        "agenda_complete_task" => api("task_done", copy(&[("id", "id"), ("done", "done")]))?,
        "agenda_delete" => api("delete", copy(&[("id", "id"), ("occurrence", "occurrence")]))?,
        "agenda_tasks" => api("tasks", copy(&[("include_done", "include_done")]))?,
        _ => return Err(format!("outil inconnu : {name}")),
    };
    Ok(v.to_pretty())
}

fn reply(out: &mut impl Write, id: &Json, result: Json) {
    let msg = Json::obj().set("jsonrpc", "2.0").set("id", id.clone()).set("result", result);
    let _ = writeln!(out, "{msg}");
    let _ = out.flush();
}

fn error(out: &mut impl Write, id: &Json, code: i64, m: &str) {
    let msg = Json::obj()
        .set("jsonrpc", "2.0")
        .set("id", id.clone())
        .set("error", Json::obj().set("code", code).set("message", m));
    let _ = writeln!(out, "{msg}");
    let _ = out.flush();
}

pub fn handle(ctx: &Ctx, line: &str, out: &mut impl Write) {
    let msg = match Json::parse(line) {
        Ok(m) => m,
        Err(e) => return error(out, &Json::Null, -32700, &e),
    };
    let id = msg.get("id").clone();
    let is_notification = !msg.has("id");
    let method = msg.get("method").str_or("");
    let params = msg.get("params");
    match method {
        "initialize" => {
            let pv = params.get("protocolVersion").as_str().unwrap_or("2025-06-18").to_string();
            reply(
                out,
                &id,
                Json::obj()
                    .set("protocolVersion", pv)
                    .set("capabilities", Json::obj().set("tools", Json::obj().set("listChanged", false)))
                    .set("serverInfo", Json::obj().set("name", "agenda").set("version", agenda_core::VERSION))
                    .set("instructions", INSTRUCTIONS),
            );
        }
        "ping" => reply(out, &id, Json::obj()),
        "tools/list" => {
            let list: Vec<Json> = tools()
                .into_iter()
                .map(|(n, d, s)| Json::obj().set("name", n).set("description", d).set("inputSchema", s))
                .collect();
            reply(out, &id, Json::obj().set("tools", list));
        }
        "tools/call" => {
            let name = params.get("name").str_or("");
            let args = params.get("arguments");
            let (text, is_err) = match call_tool(ctx, name, args) {
                Ok(t) => (t, false),
                Err(e) => (e, true),
            };
            reply(
                out,
                &id,
                Json::obj()
                    .set("content", vec![Json::obj().set("type", "text").set("text", text)])
                    .set("isError", is_err),
            );
        }
        "resources/list" => reply(out, &id, Json::obj().set("resources", Json::Arr(vec![]))),
        "prompts/list" => reply(out, &id, Json::obj().set("prompts", Json::Arr(vec![]))),
        _ if is_notification => {}
        m => error(out, &id, -32601, &format!("méthode inconnue : {m}")),
    }
}

pub fn run(ctx: &Ctx) -> Result<(), String> {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        handle(ctx, &line, &mut out);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocole_mcp() {
        let dir = std::env::temp_dir().join(format!("agenda-mcp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ctx = Ctx { api: agenda_core::Api::new(), json: false, dir: dir.clone() };
        ctx.api.open_dir(&dir, true).unwrap();
        let run = |l: &str| {
            let mut out = Vec::new();
            handle(&ctx, l, &mut out);
            String::from_utf8(out).unwrap()
        };
        let r = run(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#);
        assert!(r.contains("\"protocolVersion\":\"2025-06-18\"") && r.contains("\"tools\""));
        assert_eq!(run(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#), "");
        let r = run(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
        assert!(r.contains("agenda_brief") && r.contains("agenda_add_event"));
        let r = run(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"agenda_add","arguments":{"text":"Rapport demain !haute"}}}"#,
        );
        assert!(r.contains("tasks/rapport.md") && r.contains("\"isError\":false"), "{r}");
        let r = run(
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"agenda_add_event","arguments":{"title":"Réunion","start":"2026-10-02 10:00","notes":"ordre du jour"}}}"#,
        );
        assert!(r.contains("events/2026-10/reunion.md"), "{r}");
        let raw = std::fs::read_to_string(dir.join("events/2026-10/reunion.md")).unwrap();
        assert!(raw.ends_with("---\nordre du jour\n"));
        let r =
            run(r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"agenda_brief","arguments":{}}}"#);
        assert!(r.contains("Rapport"));
        let r = run(
            r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"agenda_get","arguments":{"id":"../../etc/passwd"}}}"#,
        );
        assert!(r.contains("\"isError\":true"));
        let r = run(r#"{"jsonrpc":"2.0","id":7,"method":"nope"}"#);
        assert!(r.contains("-32601"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
