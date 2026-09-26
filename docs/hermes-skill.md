---
name: agenda
description: Lire et modifier l'agenda et les tâches de l'utilisateur (dossier Markdown/YAML local-first). À utiliser pour « qu'est-ce que j'ai demain », planifier un rendez-vous, trouver un créneau libre, ajouter ou terminer une tâche, préparer la journée.
---

# Skill « agenda »

L'agenda de l'utilisateur est un **dossier de fichiers Markdown** synchronisé par
Syncthing entre son PC, son Mac, son téléphone et ce serveur. Tu y accèdes par le
serveur MCP `agenda` (recommandé) ou par la CLI `agenda --json`. Tu n'as besoin
d'aucun autre accès : ni réseau, ni reste du disque.

## Installation (une fois, sur le VPS)

1. CLI : `sudo curl -Lo /usr/local/bin/agenda https://github.com/hello-theojiang/local-schedule-file-system/releases/latest/download/agenda-linux-x86_64 && sudo chmod +x /usr/local/bin/agenda`
   (`agenda-linux-aarch64` sur un serveur ARM).
2. Déclarer le serveur MCP dans `~/.hermes/config.yaml` (fichier
   `vps/hermes-mcp.yaml` de `agenda-extras.tar.gz`) :

   ```yaml
   mcp_servers:
     agenda:
       command: /usr/local/bin/agenda
       args: ["--dir", "/home/<user>/agenda", "mcp"]
       env:
         TZ: Europe/Paris
   ```
3. Copier ce fichier en `~/.hermes/skills/agenda/SKILL.md`.
4. Moindre privilège : si Hermes tourne dans un conteneur, ne monter que
   `/home/<user>/agenda` (lecture-écriture). Le serveur MCP n'écrit que dans ce
   dossier et dans `~/.local/state/agenda/` (journal d'annulation).

## Outils MCP

| Outil | Usage |
|---|---|
| `agenda_brief` | **Toujours en premier** : résumé (~500 tokens) d'aujourd'hui, des 7 jours à venir, des tâches en retard/à échéance, des conflits |
| `agenda_list` | occurrences d'événements sur une période (`from`, `to` ou `days`) + tâches à échéance |
| `agenda_free` | créneaux libres (`from`, `to`, `min_minutes`, `day_start`, `day_end`) |
| `agenda_search` | recherche plein texte, insensible aux accents |
| `agenda_add` | ajout en **français naturel** : « Dentiste vendredi 14h-15h @Cabinet #santé » |
| `agenda_add_event` / `agenda_add_task` | ajout avec des champs explicites |
| `agenda_get` | un élément complet (champs, notes, révision) |
| `agenda_update` | modifier des champs (`null` supprime) |
| `agenda_complete_task` | terminer / rouvrir une tâche |
| `agenda_delete` | mettre à la corbeille, ou supprimer une seule occurrence (`occurrence`) |
| `agenda_tasks` | tâches ouvertes |

Équivalents CLI : `agenda brief`, `agenda --json list --from 2026-10-01 --to 2026-10-07`,
`agenda --json free --min 60`, `agenda add "…"`, `agenda done "mot du titre"`,
`agenda --json call <méthode> '<json>'` (voir `agenda call help`).

## Règles

1. **Commence par `agenda_brief`**, puis précise avec `agenda_list` si besoin.
2. Dates : `AAAA-MM-JJ` (journée entière) ou `AAAA-MM-JJ HH:MM` (heure locale de
   l'utilisateur, Europe/Paris). Ne convertis pas en UTC.
3. Avant d'ajouter un rendez-vous, vérifie les conflits avec `agenda_free` ou
   `agenda_list`. Signale un chevauchement plutôt que de l'ignorer.
4. Après `agenda_add`, **relis le résumé renvoyé** (date, heure, type) et corrige
   avec `agenda_update` si la phrase a été mal comprise.
5. Pour modifier, passe **uniquement les champs à changer**. Si la réponse contient
   `conflicts`, quelqu'un d'autre a modifié le même champ : sa version est gardée,
   la tienne est dans un fichier de conflit que l'utilisateur tranchera dans l'app.
   Préviens-le.
6. Ne supprime rien sans demande explicite. Une suppression va dans `.trash/` et
   reste récupérable.
7. Événement répété : pour une seule date, utilise `agenda_delete` avec
   `occurrence` (valeur du champ `occurrence` de `agenda_list`), ou ajoute la date
   au champ `except`. Pour décaler une seule occurrence, supprime-la et crée un
   événement séparé.
8. Priorités de tâches : `high`, `medium`, `low`. Statuts : `todo`, `doing`,
   `waiting`, `done`, `cancelled`.
9. Rappels : `alarm: ["15m", "1d"]` (avant le début), `"none"` pour aucun. Les
   rappels partent vers le téléphone de l'utilisateur via ntfy (service
   `agenda-remind`), inutile de les envoyer toi-même.
10. Tu peux aussi lire ou écrire les fichiers directement (format dans
    `AGENDA.md` à la racine du dossier) : écris de façon atomique (fichier
    temporaire puis renommage) et conserve les champs que tu ne connais pas.

## Exemples

- « Qu'est-ce que j'ai demain ? » → `agenda_brief` (ou `agenda_list` avec `days: 1`
  à partir de demain), puis réponse courte.
- « Cale-moi 1 h de sport cette semaine le soir » → `agenda_free` (`day_start:
  "17:00"`, `day_end: "21:00"`, `min_minutes: 60`), propose un créneau, puis
  `agenda_add_event` après accord.
- « J'ai fini le rapport » → `agenda_search` (« rapport ») → `agenda_complete_task`.
- Chaque matin (tâche planifiée Hermes) : `agenda_brief` puis message de 5 lignes
  maximum à l'utilisateur.
