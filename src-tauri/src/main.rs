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
use std::time::Instant;

fn log_result<T>(name: &str, started: Instant, result: &Result<T, String>) {
    match result {
        Ok(_) => eprintln!("[V2Proxy][OK] {name} completed in {} ms", started.elapsed().as_millis()),
        Err(error) => eprintln!(
            "[V2Proxy][ERROR] {name} failed in {} ms: {error}",
            started.elapsed().as_millis()
        ),
    }
}

#[tauri::command]
async fn get_status_async() -> Result<AppStatus, String> {
    let started = Instant::now();
    eprintln!("[V2Proxy][CALL] get_status");
    let result = backend::get_status();
    log_result("get_status", started, &result);
    result
}

#[tauri::command]
async fn list_routing_profiles_async() -> Result<Vec<RoutingProfile>, String> {
    let started = Instant::now();
    eprintln!("[V2Proxy][CALL] list_routing_profiles");
    let result = backend::list_routing_profiles();
    match &result {
        Ok(rows) => eprintln!(
            "[V2Proxy][OK] list_routing_profiles completed in {} ms · {} profiles",
            started.elapsed().as_millis(),
            rows.len()
        ),
        Err(error) => eprintln!(
            "[V2Proxy][ERROR] list_routing_profiles failed in {} ms: {error}",
            started.elapsed().as_millis()
        ),
    }
    result
}

#[tauri::command]
async fn get_traffic_snapshot_async(
    state: tauri::State<'_, MonitorState>,
) -> Result<Vec<TrafficRow>, String> {
    let started = Instant::now();
    eprintln!("[V2Proxy][CALL] traffic_snapshot · starting nettop/lsof sample");
    let result = backend::get_traffic_snapshot(state);
    match &result {
        Ok(rows) => eprintln!(
            "[V2Proxy][OK] traffic_snapshot completed in {} ms · {} processes",
            started.elapsed().as_millis(),
            rows.len()
        ),
        Err(error) => eprintln!(
            "[V2Proxy][ERROR] traffic_snapshot failed in {} ms: {error}",
            started.elapsed().as_millis()
        ),
    }
    result
}

fn main() {
    eprintln!("[V2Proxy][BOOT] V2Proxy Shell starting");
    eprintln!("[V2Proxy][BOOT] registering Tauri commands and monitor state");

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
        .setup(|_| {
            eprintln!("[V2Proxy][BOOT] Tauri setup complete · webview event loop starting");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run V2Proxy Shell");
}
