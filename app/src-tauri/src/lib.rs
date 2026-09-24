use agenda_core::Api;

struct State(std::sync::Arc<Api>);

#[tauri::command]
async fn api(state: tauri::State<'_, State>, method: String, params: String) -> Result<String, String> {
    let api = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || api.call(&method, &params))
        .await
        .map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(State(std::sync::Arc::new(Api::new())))
        .invoke_handler(tauri::generate_handler![api])
        .run(tauri::generate_context!())
        .expect("impossible de démarrer l'application");
}
