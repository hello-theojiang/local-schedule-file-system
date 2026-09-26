#!/usr/bin/env bash
# Adapte le projet Android généré par `tauri android init` (app/src-tauri/gen/android).
# Idempotent : peut être relancé sans dupliquer les modifications.
#  - permissions : accès à tous les fichiers (dossier Syncthing), notifications, alarmes exactes ;
#  - MainActivity : demande l'accès à tous les fichiers et expose un petit pont JS
#    (window.AgendaAndroid) pour l'état des permissions.
set -euo pipefail
cd "$(dirname "$0")/src-tauri/gen/android"

manifest=app/src/main/AndroidManifest.xml
[ -f "$manifest" ] || { echo "manifeste introuvable : lancez d'abord 'tauri android init'" >&2; exit 1; }

add_perm() {
  local line="$1" key="$2"
  if ! grep -q "$key" "$manifest"; then
    # insère juste après la permission INTERNET générée par Tauri
    sed -i "s#\(<uses-permission android:name=\"android.permission.INTERNET\" />\)#\1\n    $line#" "$manifest"
  fi
}
add_perm '<uses-permission android:name="android.permission.MANAGE_EXTERNAL_STORAGE" />' MANAGE_EXTERNAL_STORAGE
add_perm '<uses-permission android:name="android.permission.READ_EXTERNAL_STORAGE" android:maxSdkVersion="32" />' READ_EXTERNAL_STORAGE
add_perm '<uses-permission android:name="android.permission.WRITE_EXTERNAL_STORAGE" android:maxSdkVersion="29" />' WRITE_EXTERNAL_STORAGE
add_perm '<uses-permission android:name="android.permission.POST_NOTIFICATIONS" />' POST_NOTIFICATIONS
add_perm '<uses-permission android:name="android.permission.SCHEDULE_EXACT_ALARM" />' SCHEDULE_EXACT_ALARM
add_perm '<uses-permission android:name="android.permission.USE_EXACT_ALARM" />' USE_EXACT_ALARM
add_perm '<uses-permission android:name="android.permission.RECEIVE_BOOT_COMPLETED" />' RECEIVE_BOOT_COMPLETED

# Android 10 : accès « legacy » au stockage partagé.
if ! grep -q requestLegacyExternalStorage "$manifest"; then
  sed -i 's#<application#<application\n        android:requestLegacyExternalStorage="true"#' "$manifest"
fi

activity=$(find app/src/main/java -name MainActivity.kt | head -n1)
[ -n "$activity" ] || { echo "MainActivity.kt introuvable" >&2; exit 1; }
package=$(sed -n 's/^package \(.*\)$/\1/p' "$activity")

cat > "$activity" <<KOTLIN
package $package

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.provider.Settings
import android.webkit.JavascriptInterface
import android.webkit.WebView

class MainActivity : TauriActivity() {
  private var askedThisSession = false

  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    requestStorageAccess(false)
  }

  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    webView.addJavascriptInterface(Bridge(), "AgendaAndroid")
  }

  fun hasStorageAccess(): Boolean =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) Environment.isExternalStorageManager()
    else checkSelfPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE) == PackageManager.PERMISSION_GRANTED

  fun requestStorageAccess(force: Boolean) {
    if (hasStorageAccess() || (askedThisSession && !force)) return
    askedThisSession = true
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
      try {
        startActivity(Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION, Uri.parse("package:\$packageName")))
      } catch (e: Exception) {
        startActivity(Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION))
      }
    } else {
      requestPermissions(arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE, Manifest.permission.WRITE_EXTERNAL_STORAGE), 1)
    }
  }

  inner class Bridge {
    @JavascriptInterface fun hasStorageAccess(): Boolean = this@MainActivity.hasStorageAccess()
    @JavascriptInterface fun requestStorageAccess() { runOnUiThread { this@MainActivity.requestStorageAccess(true) } }
    @JavascriptInterface fun externalStorage(): String = Environment.getExternalStorageDirectory().absolutePath
  }
}
KOTLIN

echo "Projet Android adapté :"
grep -n 'uses-permission' "$manifest"
