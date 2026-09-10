mod backend;

use backend::{
    activate_routing_profile,
    control_v2rayu,
    create_routing_profile,
    delete_routing_profile,
    save_routing_profile,
    set_proxy_mode,
    AppStatus,
    MonitorState,
    RoutingProfile,
    TrafficRow,
};

#[tauri::command]
async fn get_status_async() -> Result<AppStatus, String> {
    backend::get_status()
}

#[tauri::command]
async fn list_routing_profiles_async() -> Result<Vec<RoutingProfile>, String> {
    backend::list_routing_profiles()
}

#[tauri::command]
async fn get_traffic_snapshot_async(
    state: tauri::State<'_, MonitorState>,
) -> Result<Vec<TrafficRow>, String> {
    backend::get_traffic_snapshot(state)
}

fn main() {
    tauri::Builder::default()
        .manage(MonitorState::default())
        .invoke_handler(tauri::generate_handler![
            get_status_async,
            list_routing_profiles_async,
            save_routing_profile,
            activate_routing_profile,
            create_routing_profile,
            delete_routing_profile,
            set_proxy_mode,
            get_traffic_snapshot_async,
            control_v2rayu,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run V2Proxy Shell");
}
