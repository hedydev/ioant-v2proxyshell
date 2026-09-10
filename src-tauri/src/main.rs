mod backend;

use backend::{
    activate_routing_profile,
    control_v2rayu,
    create_routing_profile,
    delete_routing_profile,
    get_status as get_status_blocking,
    get_traffic_snapshot as get_traffic_snapshot_blocking,
    list_routing_profiles as list_routing_profiles_blocking,
    save_routing_profile,
    set_proxy_mode,
    AppStatus,
    MonitorState,
    RoutingProfile,
    TrafficRow,
};

#[tauri::command]
async fn get_status() -> Result<AppStatus, String> {
    get_status_blocking()
}

#[tauri::command]
async fn list_routing_profiles() -> Result<Vec<RoutingProfile>, String> {
    list_routing_profiles_blocking()
}

#[tauri::command]
async fn get_traffic_snapshot(
    state: tauri::State<'_, MonitorState>,
) -> Result<Vec<TrafficRow>, String> {
    get_traffic_snapshot_blocking(state)
}

fn main() {
    tauri::Builder::default()
        .manage(MonitorState::default())
        .invoke_handler(tauri::generate_handler![
            get_status,
            list_routing_profiles,
            save_routing_profile,
            activate_routing_profile,
            create_routing_profile,
            delete_routing_profile,
            set_proxy_mode,
            get_traffic_snapshot,
            control_v2rayu,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run V2Proxy Shell");
}
