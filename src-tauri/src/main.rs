mod backend;

use backend::{
    activate_routing_profile,
    control_v2rayu,
    create_routing_profile,
    delete_routing_profile,
    save_routing_profile,
    set_proxy_mode,
    MonitorState,
};

mod async_commands {
    use crate::backend::{
        get_status as get_status_blocking,
        get_traffic_snapshot as get_traffic_snapshot_blocking,
        list_routing_profiles as list_routing_profiles_blocking,
        AppStatus,
        MonitorState,
        RoutingProfile,
        TrafficRow,
    };

    #[tauri::command]
    pub async fn get_status() -> Result<AppStatus, String> {
        get_status_blocking()
    }

    #[tauri::command]
    pub async fn list_routing_profiles() -> Result<Vec<RoutingProfile>, String> {
        list_routing_profiles_blocking()
    }

    #[tauri::command]
    pub async fn get_traffic_snapshot(
        state: tauri::State<'_, MonitorState>,
    ) -> Result<Vec<TrafficRow>, String> {
        get_traffic_snapshot_blocking(state)
    }
}

fn main() {
    tauri::Builder::default()
        .manage(MonitorState::default())
        .invoke_handler(tauri::generate_handler![
            async_commands::get_status,
            async_commands::list_routing_profiles,
            save_routing_profile,
            activate_routing_profile,
            create_routing_profile,
            delete_routing_profile,
            set_proxy_mode,
            async_commands::get_traffic_snapshot,
            control_v2rayu,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run V2Proxy Shell");
}
