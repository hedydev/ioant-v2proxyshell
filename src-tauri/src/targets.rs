use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    env,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::Command,
};

const HTTP_PORT: u16 = 1087;
const SOCKS_PORT: u16 = 1080;
const MAX_LOG_BYTES: u64 = 512 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct ProcessTarget {
    domain: Option<String>,
    ip: Option<String>,
    port: u16,
    route: String,
    source: String,
    socket: String,
}

#[derive(Debug, Serialize)]
pub struct ProcessTargetDetails {
    targets: Vec<ProcessTarget>,
    xray_log: Option<String>,
    xray_log_status: String,
}

fn endpoint(value: &str) -> (String, u16) {
    let value = value.trim().trim_matches(|c| c == '(' || c == ')');
    if value.starts_with('[') {
        if let Some(i) = value.rfind("]:") {
            return (
                value[1..i].to_string(),
                value[i + 2..].parse().unwrap_or_default(),
            );
        }
    }
    value
        .rsplit_once(':')
        .map(|(host, port)| (host.to_string(), port.parse().unwrap_or_default()))
        .unwrap_or((value.to_string(), 0))
}

fn established_connections(pid: u32, numeric: bool) -> Result<Vec<(String, String)>, String> {
    let mut args = vec!["-P"];
    if numeric {
        args.push("-n");
    }
    args.extend(["-a", "-p", &pid.to_string(), "-iTCP", "-sTCP:ESTABLISHED"]);
    let output = Command::new("lsof")
        .args(args)
        .output()
        .map_err(|e| format!("lsof: {e}"))?;

    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if error.is_empty() { "lsof failed".to_string() } else { error });
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    let mut result = Vec::new();
    for line in raw.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let Some(conn) = fields.iter().find(|value| value.contains("->")) else { continue };
        let Some((left, right)) = conn.split_once("->") else { continue };
        result.push((left.to_string(), right.to_string()));
    }
    Ok(result)
}

fn config_path() -> Option<PathBuf> {
    env::var("HOME").ok().map(PathBuf::from).map(|home| home.join(".V2rayU/config.json"))
}

fn configured_access_log() -> Option<PathBuf> {
    let config = config_path()?;
    let raw = fs::read_to_string(&config).ok()?;
    let json: Value = serde_json::from_str(&raw).ok()?;
    let access = json.get("log")?.get("access")?.as_str()?.trim();
    if access.is_empty() {
        return None;
    }
    let path = PathBuf::from(access);
    if path.is_absolute() {
        Some(path)
    } else {
        Some(config.parent().unwrap_or(Path::new(".")).join(path))
    }
}

fn discover_access_log() -> Option<PathBuf> {
    if let Some(path) = configured_access_log() {
        if path.exists() {
            return Some(path);
        }
    }
    let home = env::var("HOME").ok().map(PathBuf::from)?;
    let root = home.join(".V2rayU");
    [
        "access.log",
        "xray_access.log",
        "v2ray_access.log",
        "xray.log",
        "v2ray.log",
    ]
    .into_iter()
    .map(|name| root.join(name))
    .find(|path| path.is_file())
}

fn read_tail(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    if size > MAX_LOG_BYTES {
        file.seek(SeekFrom::Start(size - MAX_LOG_BYTES))
            .map_err(|e| e.to_string())?;
    }
    let mut raw = String::new();
    file.read_to_string(&mut raw).map_err(|e| e.to_string())?;
    Ok(raw)
}

fn parse_port_after(line: &str, marker: &str) -> Option<u16> {
    let rest = line.split_once(marker)?.1;
    let token = rest.split_whitespace().next()?.trim_matches(|c| c == '[' || c == ']');
    token.rsplit_once(':')?.1.parse().ok()
}

fn parse_xray_target(line: &str) -> Option<(String, String)> {
    let marker = if line.contains("accepted tcp:") {
        "accepted tcp:"
    } else if line.contains("accepted udp:") {
        "accepted udp:"
    } else {
        return None;
    };
    let rest = line.split_once(marker)?.1.trim_start();
    let target = rest.split_whitespace().next()?.trim_matches(|c| c == '[' || c == ']');
    let route = line
        .rsplit_once('[')
        .and_then(|(_, tail)| tail.split_once(']').map(|(value, _)| value.to_string()))
        .unwrap_or_else(|| "XRAY".to_string());
    Some((target.to_string(), route))
}

fn xray_recent_by_source_port(path: &Path) -> HashMap<u16, (String, String)> {
    let Ok(raw) = read_tail(path) else { return HashMap::new() };
    let mut map = HashMap::new();
    for line in raw.lines().rev() {
        let source_port = parse_port_after(line, "from tcp:127.0.0.1:")
            .or_else(|| parse_port_after(line, "from tcp:[::1]:"));
        let (Some(port), Some(target)) = (source_port, parse_xray_target(line)) else { continue };
        map.entry(port).or_insert(target);
    }
    map
}

fn resolved_by_local_port(pid: u32) -> HashMap<u16, String> {
    let Ok(rows) = established_connections(pid, false) else { return HashMap::new() };
    let mut result = HashMap::new();
    for (left, right) in rows {
        let (_, local_port) = endpoint(&left);
        let (host, port) = endpoint(&right);
        if local_port > 0 && port > 0 {
            result.insert(local_port, host);
        }
    }
    result
}

#[tauri::command]
pub async fn get_process_target_details(pid: u32) -> Result<ProcessTargetDetails, String> {
    eprintln!("[V2Proxy][CALL] get_process_target_details pid={pid} · socket + DNS + Xray correlation");
    let numeric = established_connections(pid, true)?;
    let resolved = resolved_by_local_port(pid);
    let access_log = discover_access_log();
    let xray = access_log
        .as_deref()
        .map(xray_recent_by_source_port)
        .unwrap_or_default();

    let mut targets = Vec::new();
    for (left, right) in numeric {
        let (_, local_port) = endpoint(&left);
        let (remote_host, remote_port) = endpoint(&right);
        if remote_port == 0 {
            continue;
        }

        let is_proxy_socket = matches!(remote_host.as_str(), "127.0.0.1" | "::1")
            && (remote_port == HTTP_PORT || remote_port == SOCKS_PORT);

        if is_proxy_socket {
            if let Some((target, route)) = xray.get(&local_port) {
                let (host, port) = endpoint(target);
                let is_ip = host.parse::<std::net::IpAddr>().is_ok();
                targets.push(ProcessTarget {
                    domain: (!is_ip).then_some(host.clone()),
                    ip: is_ip.then_some(host),
                    port,
                    route: route.clone(),
                    source: "XRAY".to_string(),
                    socket: format!("{} -> {}", left, right),
                });
            } else {
                targets.push(ProcessTarget {
                    domain: None,
                    ip: None,
                    port: remote_port,
                    route: "PROXY".to_string(),
                    source: "SOCKET".to_string(),
                    socket: format!("{} -> {}", left, right),
                });
            }
            continue;
        }

        let resolved_host = resolved.get(&local_port).cloned();
        let numeric_is_ip = remote_host.parse::<std::net::IpAddr>().is_ok();
        let resolved_domain = resolved_host.filter(|host| host != &remote_host && host.parse::<std::net::IpAddr>().is_err());
        targets.push(ProcessTarget {
            domain: resolved_domain,
            ip: numeric_is_ip.then_some(remote_host.clone()),
            port: remote_port,
            route: "DIRECT".to_string(),
            source: "SOCKET/RDNS".to_string(),
            socket: format!("{} -> {}", left, right),
        });
    }

    targets.sort_by(|a, b| {
        a.domain
            .as_deref()
            .unwrap_or(a.ip.as_deref().unwrap_or(""))
            .cmp(b.domain.as_deref().unwrap_or(b.ip.as_deref().unwrap_or("")))
    });
    targets.dedup_by(|a, b| a.domain == b.domain && a.ip == b.ip && a.port == b.port && a.route == b.route);

    let log_status = match &access_log {
        Some(path) if !xray.is_empty() => format!("Xray access log active · {}", path.display()),
        Some(path) => format!("Xray log found but no recent correlatable entries · {}", path.display()),
        None => "Xray access log not found; proxy domains may be unavailable".to_string(),
    };
    eprintln!("[V2Proxy][TARGETS] pid={pid} · {} targets · {log_status}", targets.len());

    Ok(ProcessTargetDetails {
        targets,
        xray_log: access_log.map(|path| path.display().to_string()),
        xray_log_status: log_status,
    })
}

#[tauri::command]
pub async fn get_process_targets(pid: u32) -> Result<Vec<String>, String> {
    let details = get_process_target_details(pid).await?;
    Ok(details
        .targets
        .into_iter()
        .map(|target| {
            let name = target
                .domain
                .or(target.ip)
                .unwrap_or_else(|| "local proxy".to_string());
            format!("{}:{} · {} · {}", name, target.port, target.route, target.source)
        })
        .collect())
}
