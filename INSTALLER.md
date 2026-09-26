# Installer Agenda

Rien à compiler : tout se télécharge depuis la **dernière Release**.
Liens directs (toujours la dernière version) :

| Fichier | Pour |
|---|---|
| [`PKGBUILD`](https://github.com/hello-theojiang/test/releases/latest/download/PKGBUILD) | Arch Linux (`makepkg -si`) |
| [`Agenda_amd64.AppImage`](https://github.com/hello-theojiang/test/releases/latest/download/Agenda_amd64.AppImage) · [`Agenda_amd64.deb`](https://github.com/hello-theojiang/test/releases/latest/download/Agenda_amd64.deb) | autres Linux |
| [`Agenda_universal.dmg`](https://github.com/hello-theojiang/test/releases/latest/download/Agenda_universal.dmg) | Mac (Apple Silicon et Intel) |
| [`agenda.apk`](https://github.com/hello-theojiang/test/releases/latest/download/agenda.apk) | Android (arm64, armv7) |
| [`agenda-linux-x86_64`](https://github.com/hello-theojiang/test/releases/latest/download/agenda-linux-x86_64) · [`agenda-linux-aarch64`](https://github.com/hello-theojiang/test/releases/latest/download/agenda-linux-aarch64) | CLI Linux statique (VPS) |
| [`agenda-macos-arm64`](https://github.com/hello-theojiang/test/releases/latest/download/agenda-macos-arm64) · [`agenda-macos-x86_64`](https://github.com/hello-theojiang/test/releases/latest/download/agenda-macos-x86_64) | CLI Mac |
| [`agenda-extras.tar.gz`](https://github.com/hello-theojiang/test/releases/latest/download/agenda-extras.tar.gz) | unités systemd, LaunchAgent, icônes, déclaration MCP |
| [`SHA256SUMS`](https://github.com/hello-theojiang/test/releases/latest/download/SHA256SUMS) | sommes de contrôle |

> **Le dépôt doit être public** pour que ces liens marchent sans compte GitHub.
> Tant qu'il est privé, GitHub répond « 404 » à toute personne non connectée.

Tous les appareils pointent vers **le même dossier**, synchronisé par Syncthing.
Dans les exemples : `~/Sync/agenda` sur les ordinateurs,
`/storage/emulated/0/Sync/agenda` sur le téléphone, `/home/<user>/agenda` sur le VPS.
Adaptez à vos chemins.

---

## 1. PC sous Arch Linux

1. Outils de construction de paquets (si ce n'est pas déjà fait) :
   ```sh
   sudo pacman -S --needed base-devel
   ```
2. Récupérer le PKGBUILD et installer (application + CLI + entrée de menu + service de rappels) :
   ```sh
   mkdir -p ~/src/agenda-bin && cd ~/src/agenda-bin
   curl -LO https://github.com/hello-theojiang/test/releases/latest/download/PKGBUILD
   makepkg -si
   ```
3. Indiquer le dossier (partagé par l'application, la CLI et le service de rappels) :
   ```sh
   mkdir -p ~/.config/agenda
   echo "dir=$HOME/Sync/agenda" > ~/.config/agenda/config
   agenda init        # seulement si le dossier n'existe pas encore
   agenda today
   ```
4. Activer les rappels (notifications de bureau, même application fermée) :
   ```sh
   systemctl --user enable --now agenda-remind
   ```
5. Lancer **Agenda** depuis le menu des applications (ou `agenda-app`).
   Avec un pilote NVIDIA, l'application règle d'elle-même `WEBKIT_DISABLE_DMABUF_RENDERER=1`.
6. Mise à jour : relancer l'étape 2 (le nouveau PKGBUILD pointe vers la nouvelle version).

Variante sans paquet : télécharger `Agenda_amd64.AppImage`, puis
`chmod +x Agenda_amd64.AppImage && ./Agenda_amd64.AppImage`.

## 2. Mac

1. Télécharger [`Agenda_universal.dmg`](https://github.com/hello-theojiang/test/releases/latest/download/Agenda_universal.dmg), l'ouvrir et glisser **Agenda** dans **Applications**.
2. L'application n'est pas signée par Apple. Au premier lancement : **clic droit
   sur Agenda → Ouvrir → Ouvrir**. Si macOS refuse encore (« endommagée ») :
   ```sh
   xattr -cr /Applications/Agenda.app
   ```
3. Dans l'application, **Choisir le dossier…** → votre dossier Syncthing.
4. CLI (facultative, mais nécessaire pour les rappels application fermée) :
   ```sh
   sudo mkdir -p /usr/local/bin
   ARCH=$( [ "$(uname -m)" = arm64 ] && echo arm64 || echo x86_64 )
   sudo curl -Lo /usr/local/bin/agenda https://github.com/hello-theojiang/test/releases/latest/download/agenda-macos-$ARCH
   sudo chmod +x /usr/local/bin/agenda
   sudo xattr -d com.apple.quarantine /usr/local/bin/agenda 2>/dev/null; agenda --version
   ```
5. Rappels application fermée (LaunchAgent) :
   ```sh
   mkdir -p ~/.config/agenda && echo "dir=$HOME/Sync/agenda" > ~/.config/agenda/config
   curl -L https://github.com/hello-theojiang/test/releases/latest/download/agenda-extras.tar.gz | tar xz -C /tmp
   cp /tmp/agenda-extras/launchd/dev.localfirst.agenda.remind.plist ~/Library/LaunchAgents/
   launchctl load -w ~/Library/LaunchAgents/dev.localfirst.agenda.remind.plist
   ```
   Les notifications apparaissent au nom de « Éditeur de script » : autorisez-les
   dans **Réglages système → Notifications**.

## 3. Téléphone Android

1. Sur le téléphone, ouvrir ce lien et télécharger l'APK :
   <https://github.com/hello-theojiang/test/releases/latest/download/agenda.apk>
2. Ouvrir le fichier. Android demande d'**autoriser l'installation depuis cette
   source** (navigateur ou gestionnaire de fichiers) : accepter, puis **Installer**.
3. Ouvrir **Agenda** → **Autoriser l'accès** → activer **« Autoriser l'accès pour
   gérer tous les fichiers »**, puis revenir dans l'application.
   (Nécessaire pour lire le dossier de Syncthing, qui est hors de l'application.)
4. Choisir le dossier : trouver son chemin dans Syncthing (touchez le dossier → le
   chemin est affiché, souvent `/storage/emulated/0/Sync/agenda`), le saisir ou
   toucher une suggestion, puis **Ouvrir**.
5. Rappels : accepter les **notifications**. Si Android le demande, autoriser
   **« Alarmes et rappels »**. Pour une ponctualité maximale : **Paramètres →
   Applications → Agenda → Batterie → Sans restriction**.
   Les rappels des 7 prochains jours sont programmés dans le système et arrivent
   même application fermée ; ouvrez l'application de temps en temps pour prendre
   en compte ce qui a été ajouté ailleurs (le VPS envoie de toute façon les
   rappels via ntfy, voir ci-dessous).
6. Mise à jour : télécharger le nouvel `agenda.apk` et l'installer par-dessus
   l'ancien (les réglages sont conservés tant que la clé de signature est la même,
   voir les notes de la Release).

## 4. VPS Linux

1. CLI statique (aucune dépendance) :
   ```sh
   ARCH=$(uname -m)   # x86_64 ou aarch64
   sudo curl -Lo /usr/local/bin/agenda https://github.com/hello-theojiang/test/releases/latest/download/agenda-linux-$ARCH
   sudo chmod +x /usr/local/bin/agenda && agenda --version
   ```
2. Le dossier synchronisé par Syncthing (remplacer `<user>`) :
   ```sh
   TZ=Europe/Paris agenda --dir /home/<user>/agenda brief
   ```
3. Rappels vers le téléphone avec [ntfy](https://ntfy.sh) : installer l'application
   ntfy sur le téléphone et s'abonner à un sujet **long et imprévisible**
   (`agenda-CHANGEZ-MOI` est un exemple à remplacer). Tester :
   ```sh
   TZ=Europe/Paris agenda --dir /home/<user>/agenda remind --ntfy https://ntfy.sh/agenda-CHANGEZ-MOI --once
   ```
4. Service permanent :
   ```sh
   curl -L https://github.com/hello-theojiang/test/releases/latest/download/agenda-extras.tar.gz | tar xz -C /tmp
   sudo cp /tmp/agenda-extras/vps/agenda-remind-ntfy.service /etc/systemd/system/
   sudo nano /etc/systemd/system/agenda-remind-ntfy.service   # <user>, sujet ntfy, TZ
   sudo systemctl daemon-reload && sudo systemctl enable --now agenda-remind-ntfy
   journalctl -u agenda-remind-ntfy -f
   ```
   Commande personnalisée à la place de ntfy : `--exec 'ma-commande'`. Le titre et
   le texte arrivent **uniquement** dans les variables `AGENDA_TITLE`, `AGENDA_BODY`,
   `AGENDA_AT`, `AGENDA_START`, `AGENDA_ID` (jamais insérés dans la commande).
5. (Facultatif) interface web et flux `/agenda.ics` : `vps/agenda-serve.service`,
   écoute sur `127.0.0.1:8421` avec un jeton `AGENDA_TOKEN` ; exposez-le derrière un
   reverse proxy TLS (Caddy, nginx). Flux à ajouter dans un autre calendrier :
   `https://votre-domaine/agenda.ics?token=…`.
6. Hermes : suivre [`docs/hermes-skill.md`](docs/hermes-skill.md) (déclaration MCP
   dans `~/.hermes/config.yaml`, skill dans `~/.hermes/skills/agenda/SKILL.md`).

## Vérifier un téléchargement

```sh
curl -LO https://github.com/hello-theojiang/test/releases/latest/download/SHA256SUMS
sha256sum -c SHA256SUMS --ignore-missing
```
