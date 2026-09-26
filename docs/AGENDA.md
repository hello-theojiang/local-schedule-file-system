# Format du dossier agenda — version 1

Ce fichier décrit le format du dossier. Il est copié à la racine de chaque dossier
agenda : un humain ou une IA peut lire et modifier les fichiers directement, sans
passer par l'application. L'application n'est qu'une vue sur ces fichiers.

## Arborescence

```
agenda/
├── AGENDA.md                    ce fichier
├── calendars/<nom>.md           un calendrier : couleur, rappel par défaut, abonnement
├── events/AAAA-MM/<fichier>.md  un événement (AAAA-MM = mois de création, indicatif)
├── tasks/<fichier>.md           une tâche
├── .cache/subscriptions/*.ics   copies des abonnements iCalendar (régénérables)
└── .trash/                      éléments supprimés (jamais effacés définitivement)
```

- Un fichier = un élément. Le nom de fichier est libre (lettres, chiffres, `-`).
  L'identifiant d'un élément est son chemin relatif, par exemple
  `events/2026-10/dentiste.md`.
- Le dossier `AAAA-MM` n'est qu'un rangement : déplacer un événement à une autre
  date **ne renomme pas** son fichier.
- Chaque fichier commence par un en-tête YAML entre deux lignes `---`, suivi d'un
  corps Markdown libre (notes, description, liste de sous-tâches…).
- **Les champs inconnus sont conservés** tels quels : vous pouvez ajouter vos propres
  champs (`projet: thèse`, `lien: …`), l'application ne les effacera jamais.
- Encodage UTF-8, fins de ligne `\n`.

## Dates et heures

| Forme                     | Sens                                                   |
|---------------------------|--------------------------------------------------------|
| `2026-10-02`              | une journée entière                                    |
| `2026-10-02 14:30`        | heure locale « flottante » (fuseau de l'appareil)      |
| `2026-10-02T14:30:00Z`    | instant absolu en UTC                                  |
| `2026-10-02 14:30+02:00`  | instant absolu avec décalage                           |

Une heure sans fuseau est interprétée dans le fuseau de l'appareil qui l'affiche
(ou dans celui du champ `tz:` s'il est présent). Sur un serveur en UTC, définissez
`TZ=Europe/Paris` dans l'environnement du service.

## Événement — `events/AAAA-MM/*.md`

```yaml
---
title: Dentiste
start: 2026-10-02 14:00
end: 2026-10-02 15:00
calendar: santé
location: Cabinet
tags: [santé]
alarm: [15m, 1d]
---
Apporter la carte vitale.
```

| Champ      | Obligatoire | Description |
|------------|-------------|-------------|
| `title`    | oui | titre |
| `start`    | oui | début : date (journée entière) ou date + heure |
| `end`      | non | fin. Journée entière : **dernier jour inclus** (défaut : `start`). Avec heure : fin exclusive (défaut : `start` + 1 h). `duration: 1h30` est accepté à la place |
| `calendar` | non | nom d'un fichier de `calendars/` (sans `.md`) |
| `location` | non | lieu |
| `tags`     | non | liste de mots-clés : `[a, b]` |
| `repeat`   | non | règle de répétition iCalendar (RRULE), par ex. `FREQ=WEEKLY;BYDAY=TU` |
| `except`   | non | occurrences supprimées d'une répétition : `[2026-10-06, 2026-10-13 18:30]` |
| `tz`       | non | fuseau IANA (`Europe/Paris`) des heures `start`/`end` |
| `alarm`    | non | rappels avant le début : `[15m, 1h, 2d, 1w]`. `alarm: none` désactive le rappel par défaut du calendrier |
| `status`   | non | `confirmed` (défaut), `tentative`, `cancelled` |
| `uid`      | non | identifiant iCalendar (import/export) |

Pour une journée entière, les rappels sont calculés à partir de 09:00 le jour même.

### Répétitions (RRULE, RFC 5545)

`FREQ` (`DAILY`, `WEEKLY`, `MONTHLY`, `YEARLY`), `INTERVAL`, `COUNT`, `UNTIL`,
`BYDAY` (avec rang : `1MO`, `-1FR`), `BYMONTHDAY` (négatif accepté), `BYMONTH`,
`BYSETPOS`, `WKST`. Exemples :

- tous les mardis : `FREQ=WEEKLY;BYDAY=TU`
- un jour ouvré sur deux : `FREQ=DAILY;INTERVAL=2;BYDAY=MO,TU,WE,TH,FR`
- dernier vendredi du mois : `FREQ=MONTHLY;BYDAY=-1FR`
- anniversaire : `FREQ=YEARLY`
- 10 fois : `FREQ=DAILY;COUNT=10` ; jusqu'au 1er juin : `UNTIL=20260601`

Pour modifier **une seule** occurrence : ajoutez sa date à `except` et créez un
événement séparé.

## Tâche — `tasks/*.md`

```yaml
---
title: Rendre le rapport
status: todo
due: 2026-10-03
priority: high
tags: [cours]
---
- [ ] plan
- [ ] relecture
```

| Champ      | Description |
|------------|-------------|
| `title`    | titre (obligatoire) |
| `status`   | `todo` (défaut), `doing`, `waiting`, `done`, `cancelled` |
| `due`      | échéance : date ou date + heure |
| `priority` | `high`, `medium`, `low` (défaut : aucune) |
| `done_at`  | date + heure de fin (écrite automatiquement) |
| `repeat`   | RRULE : terminer la tâche la reporte à l'échéance suivante |
| `calendar`, `tags`, `alarm` | comme pour les événements (`alarm` porte sur `due`) |

## Calendrier — `calendars/<nom>.md`

```yaml
---
title: Santé
color: "#e0527a"
alarm: [1h]
---
```

| Champ    | Description |
|----------|-------------|
| `title`  | nom affiché (défaut : nom du fichier) |
| `color`  | couleur `#rrggbb` |
| `alarm`  | rappel par défaut des événements de ce calendrier |
| `url`    | abonnement iCalendar en lecture seule (`https://…` ou `webcal://…`) |
| `hidden` | `true` pour masquer le calendrier |

Un calendrier avec `url:` est un abonnement : ses événements viennent de
`.cache/subscriptions/<nom>.ics`, mis à jour par l'application ou par
`agenda sub update`. Ils ne sont pas modifiables.

## Règles de sécurité des données

1. Écriture atomique : fichier temporaire `.syncthing.<nom>.tmp` puis renommage
   (Syncthing ignore ces fichiers temporaires).
2. Supprimer = déplacer vers `.trash/AAAAMMJJ-HHMMSS__<chemin>`.
3. Les conflits Syncthing (`*.sync-conflict-*.md`) sont signalés et résolus dans
   l'application ; ils ne sont jamais supprimés sans action explicite.
4. Si un fichier est modifié par un autre programme pendant qu'on l'édite, les
   modifications sont fusionnées champ par champ ; en cas de désaccord sur un même
   champ, **la version externe est conservée** et un fichier de conflit
   `*.agenda-conflict-*.md` contient votre version.
