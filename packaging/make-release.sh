#!/usr/bin/env bash
# Rassemble les artifacts de la CI dans dist/ sous des noms stables, génère le
# PKGBUILD -bin avec les sommes SHA-256 réelles, SHA256SUMS et les notes de version.
set -euo pipefail
tag=$1; art=$2; dist=$3
ver=${tag#v}
root=$(cd "$(dirname "$0")/.." && pwd)
mkdir -p "$dist"
find "$art" -type f \( -name 'agenda-linux-*' -o -name 'agenda-macos-*' -o -name 'agenda-app-linux-x86_64' \
  -o -name 'agenda-extras.tar.gz' -o -name 'Agenda_*' -o -name 'agenda.apk' -o -name 'capture-*.png' \) \
  -exec cp {} "$dist/" \;
signing=$(head -n1 "$art"/android/android-signing.txt 2>/dev/null || echo inconnue)
cert=$(sed -n 2p "$art"/android/android-signing.txt 2>/dev/null)

s() { sha256sum "$dist/$1" | cut -d' ' -f1; }
sed -e "s/^pkgver=.*/pkgver=$ver/" \
    -e "s/^sha256sums=.*/sha256sums=('$(s agenda-app-linux-x86_64)' '$(s agenda-linux-x86_64)' '$(s agenda-extras.tar.gz)')/" \
    -e '/^# Le PKGBUILD joint/d' \
    "$root/packaging/arch-bin/PKGBUILD" > "$dist/PKGBUILD"

(cd "$dist" && sha256sum -- * > SHA256SUMS)
ls -l "$dist"

repo=${GITHUB_REPOSITORY:-hello-theojiang/local-schedule-file-system}
{
  echo "## Agenda $tag"
  echo
  echo "Guide d'installation pas à pas : [INSTALLER.md](https://github.com/$repo/blob/$tag/INSTALLER.md)"
  echo
  echo "| Appareil | Fichier |"
  echo "|---|---|"
  echo "| Arch Linux | \`PKGBUILD\` puis \`makepkg -si\` |"
  echo "| Linux (autres) | \`Agenda_amd64.AppImage\`, \`Agenda_amd64.deb\` |"
  echo "| macOS (Apple Silicon et Intel) | \`Agenda_universal.dmg\` (app non signée : clic droit → Ouvrir) |"
  echo "| Android (arm64, armv7) | \`agenda.apk\` |"
  echo "| CLI / VPS | \`agenda-linux-x86_64\`, \`agenda-linux-aarch64\`, \`agenda-macos-arm64\`, \`agenda-macos-x86_64\` |"
  echo
  [ -n "$cert" ] && echo "Certificat de l'APK (SHA-256) : \`$cert\`" && echo
  if [ "$signing" != stable ]; then
    echo "> ⚠️ **APK signé avec une clé temporaire** : les secrets de signature ne sont pas encore configurés."
    echo "> Une future version signée avec la clé définitive demandera de désinstaller cette app une fois."
    echo
  fi
  echo "Vérification : \`sha256sum -c SHA256SUMS\`"
} > "$root/dist-notes.md"
