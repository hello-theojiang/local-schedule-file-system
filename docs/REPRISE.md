# Reprise du projet — état au 24 septembre 2026

Ce document dit ce qui est fait, ce qui est vérifié, ce qui ne l'est pas, et la
suite. À lire avant toute nouvelle session de travail.

## Où en est-on

| Couche | État | Vérifié par |
|---|---|---|
| Format de fichiers (`docs/AGENDA.md`) | fait | tests du cœur (YAML conservateur, aller-retour exact) |
| Dates, fuseaux (TZif v1/v2, pied POSIX, tzdata Android, table de secours), RRULE | fait | tests du cœur ; TZif testé sur `/usr/share/zoneinfo` ; **tzdata Android non testé sur appareil** |
| Stockage : écriture atomique, corbeille, annulation persistante, fusion 3 voies, conflits Syncthing | fait | tests du cœur + e2e (conflit résolu dans l'interface) |
| Langage naturel français | fait | 5 groupes de tests (≈ 45 phrases) |
| iCalendar : import, export, abonnements, `/agenda.ics` | fait | tests (aller-retour, RECURRENCE-ID, EXDATE) ; **abonnement réel (Google, etc.) non testé** |
| API unique `Api::call` (32 méthodes, `agenda call help`) | fait | tests d'API |
| CLI, `serve`, `mcp`, `remind` | fait | tests MCP et `--exec` sans injection ; essais manuels ; protections HTTP (401/403) vérifiées au curl |
| Interface (Jour, Semaine, Mois, Tâches, palette, éditeur, réglages, conflits) | fait | 11 parcours Playwright (souris et appui long au doigt) |
| Enveloppe Tauri (bureau + Android) | fait | compilée en CI ; l'app Linux démarre sous Xvfb et affiche le dossier (capture) |
| CI : tests, clippy, CLI musl x86_64/aarch64, .deb/.AppImage, build Arch + PKGBUILD, .dmg universel, APK arm64+armv7, Playwright | fait | runs verts |
| Release (tag `v*` ou commit `[release]`) | fait | voir la dernière Release |

## Mesures (bench, 6 000 fichiers)

`cargo run --release -p agenda-core --example bench` :
ouverture 35–44 ms · occurrences d'un mois 0,7 ms (appel `list` complet avec JSON
≈ 2,9 ms pour 1 088 occurrences) · recherche ≈ 5 ms · brief ≈ 1 ms ·
langage naturel ≈ 25 µs.
CLI statique musl : 1,7 Mo (interface web et polices incluses).
`agenda serve` : ≈ 8,5 Mo de RSS avec les 6 000 fichiers chargés (la référence
annonçait 4 Mo : **non atteint** ; le tas vivant est de 5,4 Mo, le reste est le
surcoût de l'allocateur sur ~50 000 petites chaînes). Piste : index compact
(titre/début/fin/règle) au lieu de structures analysées, `Arc<str>` pour les
identifiants (aujourd'hui copiés trois fois). Mesurer avec
`cargo run --release -p agenda-core --example memtest <dossier>`.

## Ce qui n'a pas pu être vérifié ici

- **Téléphone réel** : installation de l'APK, accès à tous les fichiers, lecture
  du dossier Syncthing, notifications programmées application fermée, redémarrage
  du téléphone (les rappels programmés doivent être restaurés par le plugin),
  détection des changements Syncthing (inotify sur le stockage partagé ; la
  scrutation toutes les 3 s sert de filet).
- **Mac réel** : ouverture du `.dmg` non signé, `xattr -cr`, LaunchAgent et
  notifications `osascript`.
- **Arch réel avec interface graphique** et pilote NVIDIA (vérifié seulement en
  conteneur Arch sous Xvfb : paquet installé, `ldd` complet, fenêtre vivante 5 s).
- **Hermes** : la déclaration `mcp_servers` et l'emplacement des skills suivent la
  documentation d'Hermes Agent connue à ce jour ; à confirmer sur la version
  installée. Le protocole MCP lui-même est testé (initialize, tools/list,
  tools/call).
- Abonnement à un vrai calendrier distant et envoi réel vers ntfy.

## Décisions et contraintes

- Nom « Agenda », identifiant Android `dev.localfirst.agenda` : définitifs dès
  qu'un APK est installé (changer l'identifiant = nouvelle application).
- **Signature Android** : une clé stable a été générée hors du dépôt et remise à
  l'utilisateur. Tant que les secrets `ANDROID_KEYSTORE_B64`,
  `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS` ne sont pas dans le dépôt, la CI
  signe avec une clé **temporaire** (et le dit dans les notes de Release).
- **Visibilité** : le dépôt était privé ; les liens de Release ne marchent sans
  compte qu'une fois le dépôt public. Le job Release vérifie chaque lien sans
  authentification et avertit sinon.
- Le proxy git de l'environnement de développement refusait les tags : la Release
  se déclenche aussi par un commit contenant `[release]` (la CI crée le tag
  `v<version du Cargo.toml>`). Versions à tenir alignées : `Cargo.toml`
  (workspace), `app/src-tauri/Cargo.toml`, `app/src-tauri/tauri.conf.json`,
  `packaging/arch*/PKGBUILD` (la CI vérifie `tauri.conf.json`).
- Abonnements : `curl` (CLI et bureau), `ureq` + rustls sur Android.
- Les secrets ne passent jamais par le dépôt ; aucune donnée personnelle
  (`<user>`, `agenda-CHANGEZ-MOI` sont des exemples).

## Suite possible

1. Retours de l'utilisateur sur téléphone et Mac (liste ci-dessus).
2. Mémoire du serveur (voir « Mesures »).
3. Occurrence modifiée « en place » (aujourd'hui : exception + événement séparé).
4. Rappels Android reprogrammés en tâche de fond (WorkManager) quand Syncthing
   apporte des changements application fermée.
5. Signature Apple (compte développeur payant) pour supprimer l'étape `xattr`.
6. Vue « année » et impression.
