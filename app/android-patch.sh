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
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.provider.DocumentsContract
import android.provider.Settings
import android.webkit.JavascriptInterface
import android.webkit.WebView
import org.json.JSONObject

class MainActivity : TauriActivity() {
  private var askedThisSession = false
  private var webView: WebView? = null

  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    requestStorageAccess(false)
  }

  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    this.webView = webView
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

  // Sélecteur de dossier système (SAF). Le chemin est renvoyé à la page via le
  // callback window.__agendaPickedFolder({path}|{error}|null) : l'URI SAF n'est
  // pas utilisable telle quelle, le cœur travaille sur le système de fichiers
  // (accès « tous les fichiers » demandé à l'installation).
  private fun pickFolder() {
    try {
      val i = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
      }
      @Suppress("DEPRECATION")
      startActivityForResult(i, PICK_FOLDER_RC)
    } catch (e: Exception) {
      deliverPick(JSONObject().put("error", "sélecteur de dossier indisponible : \${e.message}"))
    }
  }

  @Deprecated("startActivityForResult suffit ici (activité unique, un seul appelant)")
  override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
    @Suppress("DEPRECATION")
    super.onActivityResult(requestCode, resultCode, data)
    if (requestCode != PICK_FOLDER_RC) return
    val uri = data?.data
    if (resultCode != Activity.RESULT_OK || uri == null) return deliverPick(null)
    val path = treeUriToPath(uri)
    if (path == null) deliverPick(JSONObject().put("error", "dossier hors du stockage partagé — indiquez le chemin à la main"))
    else deliverPick(JSONObject().put("path", path))
  }

  private fun deliverPick(result: JSONObject?) {
    val w = webView ?: return
    val arg = result?.toString() ?: "null"
    w.post { w.evaluateJavascript("window.__agendaPickedFolder && window.__agendaPickedFolder(\$arg)", null) }
  }

  // content://com.android.externalstorage.documents/tree/<volume>:<sous-chemin>
  // → chemin réel : "primary" = stockage interne, "home" = Documents, sinon
  // volume amovible sous /storage/<uuid>.
  private fun treeUriToPath(uri: Uri): String? {
    if (uri.authority != "com.android.externalstorage.documents") return null
    val docId = try {
      DocumentsContract.getTreeDocumentId(uri)
    } catch (e: Exception) {
      null
    } ?: return null
    val parts = docId.split(":", limit = 2)
    val base = when {
      parts[0].equals("primary", ignoreCase = true) ->
        Environment.getExternalStorageDirectory().absolutePath
      parts[0].equals("home", ignoreCase = true) ->
        Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOCUMENTS).absolutePath
      else -> "/storage/" + parts[0]
    }
    return if (parts.size < 2 || parts[1].isEmpty()) base else base + "/" + parts[1]
  }

  inner class Bridge {
    @JavascriptInterface fun hasStorageAccess(): Boolean = this@MainActivity.hasStorageAccess()
    @JavascriptInterface fun requestStorageAccess() { runOnUiThread { this@MainActivity.requestStorageAccess(true) } }
    @JavascriptInterface fun externalStorage(): String = Environment.getExternalStorageDirectory().absolutePath
    @JavascriptInterface fun pickFolder() { runOnUiThread { this@MainActivity.pickFolder() } }
  }

  private companion object {
    const val PICK_FOLDER_RC = 742
  }
}
KOTLIN

echo "Projet Android adapté :"
grep -n 'uses-permission' "$manifest"
