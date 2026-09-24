#!/usr/bin/env bash
# Aligne et signe l'APK universel (arm64 + armv7) produit par `tauri android build`.
# La clé vient des secrets du dépôt (ANDROID_KEYSTORE_B64, ANDROID_KEYSTORE_PASSWORD,
# ANDROID_KEY_ALIAS). Sans eux, une clé TEMPORAIRE est générée et l'APK est marqué
# comme tel : il ne pourra pas être mis à jour par un APK signé avec la clé stable.
set -euo pipefail
out=${1:-out}
mkdir -p "$out"
apk=$(find app/src-tauri/gen/android/app/build/outputs/apk -name '*-release-unsigned.apk' | head -n1)
[ -n "$apk" ] || { echo "APK non signé introuvable" >&2; find app/src-tauri/gen/android/app/build/outputs -name '*.apk' >&2; exit 1; }
bt=$(ls -d "$ANDROID_HOME"/build-tools/* | sort -V | tail -n1)
work=$(mktemp -d)
"$bt/zipalign" -p -f 4 "$apk" "$work/aligned.apk"

if [ -n "${KEYSTORE_B64:-}" ] && [ -n "${KEYSTORE_PASSWORD:-}" ] && [ -n "${KEY_ALIAS:-}" ]; then
  echo "$KEYSTORE_B64" | base64 -d > "$work/release.jks"
  echo stable > "$out/android-signing.txt"
else
  echo "::warning title=Signature Android::Secrets ANDROID_KEYSTORE_* absents : APK signé avec une clé TEMPORAIRE."
  KEYSTORE_PASSWORD=$(head -c 24 /dev/urandom | base64 | tr -dc 'A-Za-z0-9')
  KEY_ALIAS=temporaire
  keytool -genkeypair -keystore "$work/release.jks" -storepass "$KEYSTORE_PASSWORD" -keypass "$KEYSTORE_PASSWORD" \
    -alias "$KEY_ALIAS" -keyalg RSA -keysize 4096 -validity 36500 -dname "CN=Agenda (cle temporaire)" > /dev/null
  echo temporaire > "$out/android-signing.txt"
fi
export KEYSTORE_PASSWORD
"$bt/apksigner" sign --ks "$work/release.jks" --ks-pass env:KEYSTORE_PASSWORD --key-pass env:KEYSTORE_PASSWORD \
  --ks-key-alias "$KEY_ALIAS" --out "$out/agenda.apk" "$work/aligned.apk"
"$bt/apksigner" verify --print-certs "$out/agenda.apk" | grep -E 'Signer #1 certificate (DN|SHA-256)'
unzip -l "$out/agenda.apk" | grep -E 'lib/.*\.so' || true
ls -l "$out"
rm -rf "$work"
