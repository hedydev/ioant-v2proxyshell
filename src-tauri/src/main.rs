use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    env,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

const V2RAYU_DOMAIN: &str = "net.yanue.V2rayU";
const DEFAULT_HTTP_PORT: u16 = 1087;
const DEFAULT_SOCKS_PORT: u16 = 1080;

#[derive(Default)]
struct MonitorState {
    counters: Mutex<HashMap<u32, (u64, u64, Instant)>>,
}

#[derive(Debug, Serialize)]
struct AppStatus {
    v2rayu_running: bool,
    xray_running: bool,
    config_path: String,
    routing_db_path: Option<String>,
    active_routing_uuid: String,
    proxy_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RoutingProfile {
    uuid: String,
    name: String,
    remark: String,
    domain_strategy: String,
    domain_matcher: String,
    block: Vec<String>,
    proxy: Vec<String>,
    direct: Vec<String>,
    sort: i64,
    active: bool,
}

#[derive(Debug, Serialize)]
struct TrafficRow {
    process: String,
    pid: u32,
    mode: String,
    proxy_connections: usize,
    direct_connections: usize,
    local_connections: usize,
    down_bps: f64,
    up_bps: f64,
    direct_targets: Vec<String>,
}

#[derive(Default)]
struct ConnAgg {
    process: String,
    proxy: usize,
    direct: usize,
    local: usize,
    system: usize,
    xray: usize,
    direct_targets: HashSet<String>,
}

fn home() -> Result<PathBuf, String> {
    env::var("HOME").map(PathBuf::from).map_err(|_| "HOME is unavailable".to_string())
}

fn config_path() -> Result<PathBuf, String> {
    Ok(home()?.join(".V2rayU").join("config.json"))
}

fn output(program: &str, args: &[&str]) -> Result<String, String> {
    let result = Command::new(program).args(args).output().map_err(|e| format!("{program}: {e}"))?;
    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&result.stdout).into_owned())
}

fn output_lossy(program: &str, args: &[&str]) -> String {
    output(program, args).unwrap_or_default()
}

fn process_running(pattern: &str) -> bool {
    Command::new("pgrep")
        .args(["-f", pattern])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn defaults_read(key: &str) -> String {
    output_lossy("defaults", &["read", V2RAYU_DOMAIN, key]).trim().to_string()
}

fn defaults_write(key: &str, value: &str) -> Result<(), String> {
    output("defaults", &["write", V2RAYU_DOMAIN, key, "-string", value]).map(|_| ())
}

fn detect_proxy_mode() -> String {
    let persisted = defaults_read("runMode");
    if !persisted.is_empty() {
        return persisted;
    }
    let raw = output_lossy("scutil", &["--proxy"]);
    if raw.contains("HTTPEnable : 1") || raw.contains("HTTPSEnable : 1") || raw.contains("SOCKSEnable : 1") {
        "global/system-proxy".to_string()
    } else {
        "manual/off".to_string()
    }
}

fn collect_files(root: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth == 0 || !root.exists() {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, depth - 1, out);
        } else if path.is_file() {
            let name = path.file_name().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase();
            if name.ends_with(".db") || name.ends_with(".sqlite") || name.ends_with(".sqlite3") || name.contains("v2ray") {
                out.push(path);
            }
        }
    }
}

fn is_routing_db(path: &Path) -> bool {
    let Some(p) = path.to_str() else { return false };
    output_lossy("sqlite3", &[p, "SELECT name FROM sqlite_master WHERE type='table' AND name='routing';"])
        .lines()
        .any(|x| x.trim() == "routing")
}

fn discover_routing_db() -> Result<PathBuf, String> {
    let h = home()?;
    let mut candidates = vec![];
    collect_files(&h.join(".V2rayU"), 3, &mut candidates);
    collect_files(&h.join("Library/Application Support/V2rayU"), 4, &mut candidates);
    collect_files(&h.join("Library/Application Support/net.yanue.V2rayU"), 4, &mut candidates);
    candidates.sort();
    candidates.dedup();
    candidates
        .into_iter()
        .find(|p| is_routing_db(p))
        .ok_or_else(|| "V2rayU routing database was not found".to_string())
}

fn decode_hex(value: &str) -> String {
    let bytes: Vec<u8> = value
        .as_bytes()
        .chunks(2)
        .filter_map(|pair| {
            if pair.len() != 2 { return None; }
            let s = std::str::from_utf8(pair).ok()?;
            u8::from_str_radix(s, 16).ok()
        })
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn lines(value: String) -> Vec<String> {
    value
        .lines()
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(str::to_string)
        .collect()
}

fn sql_quote(value: &str) -> String {
    value.replace(''', "''")
}

fn backup_database(path: &Path) -> Result<PathBuf, String> {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let backup = path.with_extension(format!("v2proxyshell-{stamp}.bak"));
    let db = path.to_str().ok_or_else(|| "invalid database path".to_string())?;
    let dest = sql_quote(backup.to_str().ok_or_else(|| "invalid backup path".to_string())?);
    output("sqlite3", &[db, &format!(".backup '{dest}'")])?;
    Ok(backup)
}

#[tauri::command]
fn get_status() -> Result<AppStatus, String> {
    let cfg = config_path()?;
    let db = discover_routing_db().ok();
    Ok(AppStatus {
        v2rayu_running: process_running("/Applications/V2rayU.app/Contents/MacOS/V2rayU"),
        xray_running: process_running("xray-arm64 run"),
        config_path: cfg.display().to_string(),
        routing_db_path: db.map(|x| x.display().to_string()),
        active_routing_uuid: defaults_read("runningRouting"),
        proxy_mode: detect_proxy_mode(),
    })
}

#[tauri::command]
fn list_routing_profiles() -> Result<Vec<RoutingProfile>, String> {
    let db = discover_routing_db()?;
    let dbs = db.to_str().ok_or_else(|| "invalid routing database path".to_string())?;
    let sql = "SELECT hex(uuid),hex(name),hex(remark),hex(domainStrategy),hex(domainMatcher),hex(block),hex(proxy),hex(direct),sort FROM routing ORDER BY sort,rowid;";
    let raw = output("sqlite3", &["-separator", "|", dbs, sql])?;
    let active = defaults_read("runningRouting");
    let mut profiles = vec![];

    for row in raw.lines() {
        let cols: Vec<&str> = row.split('|').collect();
        if cols.len() != 9 { continue; }
        let uuid = decode_hex(cols[0]);
        profiles.push(RoutingProfile {
            uuid: uuid.clone(),
            name: decode_hex(cols[1]),
            remark: decode_hex(cols[2]),
            domain_strategy: decode_hex(cols[3]),
            domain_matcher: decode_hex(cols[4]),
            block: lines(decode_hex(cols[5])),
            proxy: lines(decode_hex(cols[6])),
            direct: lines(decode_hex(cols[7])),
            sort: cols[8].parse().unwrap_or(0),
            active: uuid == active,
        });
    }
    Ok(profiles)
}

fn restart_v2rayu() -> Result<(), String> {
    let _ = Command::new("osascript")
        .args(["-e", "tell application \"V2rayU\" to quit"])
        .status();
    std::thread::sleep(std::time::Duration::from_millis(700));
    let status = Command::new("open").args(["-a", "V2rayU"]).status().map_err(|e| e.to_string())?;
    if status.success() { Ok(()) } else { Err("failed to restart V2rayU".to_string()) }
}

#[tauri::command]
fn save_routing_profile(profile: RoutingProfile, activate: bool) -> Result<String, String> {
    let db = discover_routing_db()?;
    let backup = backup_database(&db)?;
    let dbs = db.to_str().ok_or_else(|| "invalid routing database path".to_string())?;
    let direct = sql_quote(&profile.direct.join("\n"));
    let proxy = sql_quote(&profile.proxy.join("\n"));
    let block = sql_quote(&profile.block.join("\n"));
    let sql = format!(
        "UPDATE routing SET name='{}',remark='{}',domainStrategy='{}',domainMatcher='{}',block='{}',proxy='{}',direct='{}' WHERE uuid='{}';",
        sql_quote(&profile.name), sql_quote(&profile.remark), sql_quote(&profile.domain_strategy),
        sql_quote(&profile.domain_matcher), block, proxy, direct, sql_quote(&profile.uuid)
    );
    output("sqlite3", &[dbs, &sql])?;

    if activate || profile.active {
        // V2rayU keeps this value in AppState/UserDefaults. Restarting after the
        // persisted profile update makes it regenerate Xray config from the DB.
        let _ = Command::new("osascript")
            .args(["-e", "tell application \"V2rayU\" to quit"])
            .status();
        std::thread::sleep(std::time::Duration::from_millis(350));
        defaults_write("runningRouting", &profile.uuid)?;
        let status = Command::new("open").args(["-a", "V2rayU"]).status().map_err(|e| e.to_string())?;
        if !status.success() { return Err("routing saved but V2rayU failed to relaunch".to_string()); }
    }

    Ok(format!("saved {} · backup {}", profile.remark, backup.display()))
}

#[tauri::command]
fn activate_routing_profile(uuid: String) -> Result<String, String> {
    let _ = Command::new("osascript")
        .args(["-e", "tell application \"V2rayU\" to quit"])
        .status();
    std::thread::sleep(std::time::Duration::from_millis(350));
    defaults_write("runningRouting", &uuid)?;
    let status = Command::new("open").args(["-a", "V2rayU"]).status().map_err(|e| e.to_string())?;
    if !status.success() { return Err("failed to relaunch V2rayU".to_string()); }
    Ok(uuid)
}

fn parse_nettop_counters() -> HashMap<u32, (u64, u64)> {
    let raw = output_lossy("nettop", &["-P", "-L", "1", "-J", "bytes_in,bytes_out"]);
    let mut map = HashMap::new();
    for line in raw.lines() {
        let fields: Vec<&str> = line.trim().trim_end_matches(',').split(',').collect();
        if fields.len() < 3 { continue; }
        let Some((_, pid)) = fields[0].rsplit_once('.') else { continue };
        let (Ok(pid), Ok(down), Ok(up)) = (pid.parse::<u32>(), fields[1].parse::<u64>(), fields[2].parse::<u64>()) else { continue };
        map.insert(pid, (down, up));
    }
    map
}

fn endpoint(value: &str) -> (String, u16) {
    let value = value.trim_matches(|c| c == '(' || c == ')');
    if value.starts_with('[') {
        if let Some(i) = value.rfind("]:" ) {
            return (value[1..i].to_string(), value[i + 2..].parse().unwrap_or(0));
        }
    }
    value.rsplit_once(':').map(|(h,p)| (h.to_string(), p.parse().unwrap_or(0))).unwrap_or((value.to_string(), 0))
}

fn local_host(host: &str) -> bool {
    if host == "localhost" || host == "127.0.0.1" || host == "::1" { return true; }
    let Ok(ip) = host.parse::<std::net::IpAddr>() else { return false };
    match ip {
        std::net::IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_private() || v4.is_loopback() || v4.is_link_local() || (o[0] == 100 && (64..=127).contains(&o[1]))
        }
        std::net::IpAddr::V6(v6) => v6.is_loopback() || v6.is_unicast_link_local(),
    }
}

fn system_process(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["tailscale", "mdnsresponder", "nesession", "networkserviceproxy"].iter().any(|x| n.contains(x))
}

fn aggregate_connections() -> HashMap<u32, ConnAgg> {
    let raw = output_lossy("lsof", &["-nP", "-iTCP", "-sTCP:ESTABLISHED"]);
    let mut map: HashMap<u32, ConnAgg> = HashMap::new();
    for line in raw.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 9 { continue; }
        let Ok(pid) = fields[1].parse::<u32>() else { continue };
        let Some(conn) = fields.iter().find(|x| x.contains("->")) else { continue };
        let Some((left, right)) = conn.split_once("->") else { continue };
        let (lh, lp) = endpoint(left);
        let (rh, rp) = endpoint(right);
        let process = fields[0].to_string();
        let agg = map.entry(pid).or_default();
        agg.process = process.clone();

        if (rh == "127.0.0.1" || rh == "::1") && (rp == DEFAULT_HTTP_PORT || rp == DEFAULT_SOCKS_PORT) {
            agg.proxy += 1;
        } else if (lh == "127.0.0.1" || lh == "::1") && (lp == DEFAULT_HTTP_PORT || lp == DEFAULT_SOCKS_PORT) {
            agg.xray += 1;
        } else if process.to_ascii_lowercase().contains("xray") || process.to_ascii_lowercase().contains("v2ray") {
            agg.xray += 1;
        } else if system_process(&process) {
            agg.system += 1;
        } else if local_host(&rh) {
            agg.local += 1;
        } else {
            agg.direct += 1;
            agg.direct_targets.insert(format!("{rh}:{rp}"));
        }
    }
    map
}

#[tauri::command]
fn get_traffic_snapshot(state: tauri::State<'_, MonitorState>) -> Result<Vec<TrafficRow>, String> {
    let now = Instant::now();
    let counters = parse_nettop_counters();
    let conns = aggregate_connections();
    let mut previous = state.counters.lock().map_err(|_| "traffic state lock failed".to_string())?;
    let mut pids: HashSet<u32> = counters.keys().copied().collect();
    pids.extend(conns.keys().copied());
    let mut rows = vec![];

    for pid in pids {
        let c = conns.get(&pid);
        let process = c.map(|x| x.process.clone()).unwrap_or_else(|| format!("pid-{pid}"));
        let (proxy, direct, local, system, xray, targets) = c.map(|x| (
            x.proxy, x.direct, x.local, x.system, x.xray, x.direct_targets.iter().cloned().collect::<Vec<_>>()
        )).unwrap_or((0,0,0,0,0,vec![]));

        let mode = if proxy > 0 && direct > 0 { "MIXED" }
        else if direct > 0 { "DIRECT" }
        else if proxy > 0 { "PROXY" }
        else if system > 0 { "SYSTEM" }
        else if xray > 0 || process.to_ascii_lowercase().contains("xray") || process.to_ascii_lowercase().contains("v2ray") { "XRAY" }
        else { "LOCAL" };

        let (down_bps, up_bps) = match (counters.get(&pid), previous.get(&pid)) {
            (Some(&(bi, bo)), Some(&(obi, obo, at))) => {
                let dt = now.duration_since(at).as_secs_f64().max(0.001);
                (bi.saturating_sub(obi) as f64 / dt, bo.saturating_sub(obo) as f64 / dt)
            }
            _ => (0.0, 0.0),
        };
        if let Some(&(bi, bo)) = counters.get(&pid) { previous.insert(pid, (bi, bo, now)); }
        rows.push(TrafficRow { process, pid, mode: mode.to_string(), proxy_connections: proxy, direct_connections: direct, local_connections: local, down_bps, up_bps, direct_targets: targets });
    }
    rows.sort_by(|a,b| (b.down_bps + b.up_bps).partial_cmp(&(a.down_bps + a.up_bps)).unwrap_or(std::cmp::Ordering::Equal));
    rows.truncate(120);
    Ok(rows)
}

#[tauri::command]
fn control_v2rayu(action: String) -> Result<String, String> {
    match action.as_str() {
        "start" => {
            let s = Command::new("open").args(["-a", "V2rayU"]).status().map_err(|e| e.to_string())?;
            if !s.success() { return Err("failed to start V2rayU".to_string()); }
        }
        "stop" => {
            let s = Command::new("osascript").args(["-e", "tell application \"V2rayU\" to quit"]).status().map_err(|e| e.to_string())?;
            if !s.success() { return Err("failed to stop V2rayU".to_string()); }
        }
        "restart" => restart_v2rayu()?,
        _ => return Err(format!("unsupported action: {action}")),
    }
    Ok(action)
}

fn main() {
    tauri::Builder::default()
        .manage(MonitorState::default())
        .invoke_handler(tauri::generate_handler![
            get_status,
            list_routing_profiles,
            save_routing_profile,
            activate_routing_profile,
            get_traffic_snapshot,
            control_v2rayu,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run V2Proxy Shell");
}
