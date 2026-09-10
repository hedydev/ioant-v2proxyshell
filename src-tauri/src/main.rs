mod backend;

use backend::{
    activate_routing_profile,
    control_v2rayu,
    create_routing_profile,
    delete_routing_profile,
    get_status,
    get_traffic_snapshot,
    list_routing_profiles,
    save_routing_profile,
    set_proxy_mode,
    MonitorState,
};

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
