use serde::Serialize;
use std::{
    io::{Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const GATEWAY_ADDR: &str = "127.0.0.1:1097";
const XRAY_HTTP_ADDR: &str = "127.0.0.1:1087";
const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_RECORDS: usize = 1000;

#[derive(Debug, Clone, Serialize)]
pub struct GatewayConnection {
    id: u64,
    timestamp_ms: u128,
    process: String,
    pid: Option<u32>,
    client_addr: String,
    domain: String,
    port: u16,
    protocol: String,
    upstream: String,
}

#[derive(Debug, Serialize)]
pub struct GatewayStatus {
    running: bool,
    listen_addr: String,
    upstream_addr: String,
    records: usize,
}

pub struct GatewayState {
    running: Arc<AtomicBool>,
    records: Arc<Mutex<Vec<GatewayConnection>>>,
}

impl Default for GatewayState {
    fn default() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            records: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn next_id() -> u64 {
    use std::sync::atomic::AtomicU64;
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

fn detect_client_process(client: SocketAddr) -> (String, Option<u32>) {
    let port = client.port().to_string();
    let output = Command::new("lsof")
        .args(["-nP", "-iTCP", &format!(":{port}"), "-sTCP:ESTABLISHED"])
        .output();
    let Ok(output) = output else {
        return ("Unknown".to_string(), None);
    };
    let raw = String::from_utf8_lossy(&output.stdout);
    for line in raw.lines().skip(1) {
        if !line.contains(&format!(":{port}->")) && !line.contains(&format!(":{port} ")) {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 2 {
            continue;
        }
        let pid = fields[1].parse::<u32>().ok();
        return (fields[0].to_string(), pid);
    }
    ("Unknown".to_string(), None)
}

fn read_http_header(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut result = Vec::with_capacity(2048);
    let mut byte = [0u8; 1];
    while result.len() < MAX_HEADER_BYTES {
        let n = stream.read(&mut byte)?;
        if n == 0 {
            break;
        }
        result.push(byte[0]);
        if result.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    Ok(result)
}

fn parse_target(header: &[u8]) -> Option<(String, u16, String)> {
    let text = String::from_utf8_lossy(header);
    let mut lines = text.lines();
    let request = lines.next()?.trim();
    let mut request_parts = request.split_whitespace();
    let method = request_parts.next()?.to_ascii_uppercase();
    let target = request_parts.next().unwrap_or_default();

    if method == "CONNECT" {
        let (host, port) = target.rsplit_once(':').unwrap_or((target, "443"));
        return Some((
            host.trim_matches(['[', ']']).to_string(),
            port.parse().unwrap_or(443),
            "HTTP CONNECT".to_string(),
        ));
    }

    if let Some(rest) = target.strip_prefix("http://") {
        let authority = rest.split('/').next().unwrap_or(rest);
        let (host, port) = authority.rsplit_once(':').unwrap_or((authority, "80"));
        return Some((host.to_string(), port.parse().unwrap_or(80), "HTTP".to_string()));
    }

    for line in lines {
        if let Some(host_value) = line.strip_prefix("Host:").or_else(|| line.strip_prefix("host:")) {
            let authority = host_value.trim();
            let (host, port) = authority.rsplit_once(':').unwrap_or((authority, "80"));
            return Some((host.to_string(), port.parse().unwrap_or(80), "HTTP".to_string()));
        }
    }
    None
}

fn record_connection(
    records: &Arc<Mutex<Vec<GatewayConnection>>>,
    client: SocketAddr,
    domain: String,
    port: u16,
    protocol: String,
) {
    let (process, pid) = detect_client_process(client);
    let record = GatewayConnection {
        id: next_id(),
        timestamp_ms: now_ms(),
        process,
        pid,
        client_addr: client.to_string(),
        domain,
        port,
        protocol,
        upstream: XRAY_HTTP_ADDR.to_string(),
    };
    eprintln!(
        "[V2Proxy][GATEWAY] {} pid={:?} {} -> {}:{} via {}",
        record.process, record.pid, record.client_addr, record.domain, record.port, record.upstream
    );
    let mut guard = records.lock().unwrap_or_else(|poison| poison.into_inner());
    guard.push(record);
    if guard.len() > MAX_RECORDS {
        let remove = guard.len() - MAX_RECORDS;
        guard.drain(0..remove);
    }
}

fn relay(mut client: TcpStream, mut upstream: TcpStream) {
    let client_to_upstream = match client.try_clone() {
        Ok(value) => value,
        Err(_) => return,
    };
    let upstream_to_client = match upstream.try_clone() {
        Ok(value) => value,
        Err(_) => return,
    };

    let a = thread::spawn(move || {
        let mut reader = client_to_upstream;
        let _ = std::io::copy(&mut reader, &mut upstream);
        let _ = upstream.shutdown(Shutdown::Write);
    });
    let b = thread::spawn(move || {
        let mut reader = upstream_to_client;
        let _ = std::io::copy(&mut reader, &mut client);
        let _ = client.shutdown(Shutdown::Write);
    });
    let _ = a.join();
    let _ = b.join();
}

fn handle_client(mut client: TcpStream, client_addr: SocketAddr, records: Arc<Mutex<Vec<GatewayConnection>>>) {
    let _ = client.set_read_timeout(Some(Duration::from_secs(10)));
    let header = match read_http_header(&mut client) {
        Ok(value) if !value.is_empty() => value,
        _ => return,
    };

    if let Some((domain, port, protocol)) = parse_target(&header) {
        record_connection(&records, client_addr, domain, port, protocol);
    }

    let mut upstream = match TcpStream::connect(XRAY_HTTP_ADDR) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("[V2Proxy][GATEWAY][ERROR] connect {XRAY_HTTP_ADDR}: {error}");
            let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\n\r\n");
            return;
        }
    };
    if upstream.write_all(&header).is_err() {
        return;
    }
    relay(client, upstream);
}

fn run_listener(running: Arc<AtomicBool>, records: Arc<Mutex<Vec<GatewayConnection>>>) {
    let listener = match TcpListener::bind(GATEWAY_ADDR) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("[V2Proxy][GATEWAY][ERROR] bind {GATEWAY_ADDR}: {error}");
            running.store(false, Ordering::SeqCst);
            return;
        }
    };
    if let Err(error) = listener.set_nonblocking(true) {
        eprintln!("[V2Proxy][GATEWAY][ERROR] nonblocking: {error}");
        running.store(false, Ordering::SeqCst);
        return;
    }

    eprintln!("[V2Proxy][GATEWAY] listening {GATEWAY_ADDR} -> {XRAY_HTTP_ADDR}");
    while running.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((client, address)) => {
                let records = records.clone();
                thread::spawn(move || handle_client(client, address, records));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(error) => {
                eprintln!("[V2Proxy][GATEWAY][ERROR] accept: {error}");
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
    eprintln!("[V2Proxy][GATEWAY] stopped");
}

#[tauri::command]
pub fn start_local_gateway(state: tauri::State<'_, GatewayState>) -> Result<GatewayStatus, String> {
    if state.running.swap(true, Ordering::SeqCst) {
        return Ok(gateway_status(state));
    }
    let running = state.running.clone();
    let records = state.records.clone();
    thread::spawn(move || run_listener(running, records));
    Ok(gateway_status(state))
}

#[tauri::command]
pub fn stop_local_gateway(state: tauri::State<'_, GatewayState>) -> Result<GatewayStatus, String> {
    state.running.store(false, Ordering::SeqCst);
    Ok(gateway_status(state))
}

#[tauri::command]
pub fn gateway_status(state: tauri::State<'_, GatewayState>) -> GatewayStatus {
    let records = state.records.lock().unwrap_or_else(|poison| poison.into_inner()).len();
    GatewayStatus {
        running: state.running.load(Ordering::SeqCst),
        listen_addr: GATEWAY_ADDR.to_string(),
        upstream_addr: XRAY_HTTP_ADDR.to_string(),
        records,
    }
}

#[tauri::command]
pub fn gateway_connections(state: tauri::State<'_, GatewayState>) -> Vec<GatewayConnection> {
    state.records.lock().unwrap_or_else(|poison| poison.into_inner()).clone()
}
