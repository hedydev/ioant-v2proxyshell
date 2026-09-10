use std::process::Command;

#[tauri::command]
pub async fn get_process_targets(pid: u32) -> Result<Vec<String>, String> {
    eprintln!("[V2Proxy][CALL] get_process_targets pid={pid}");
    let output = Command::new("lsof")
        .args(["-P", "-a", "-p", &pid.to_string(), "-iTCP", "-sTCP:ESTABLISHED"])
        .output()
        .map_err(|e| format!("lsof: {e}"))?;

    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if error.is_empty() { "lsof failed".to_string() } else { error });
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    let mut targets = Vec::new();
    for line in raw.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let Some(conn) = fields.iter().find(|x| x.contains("->")) else { continue };
        let Some((_, right)) = conn.split_once("->") else { continue };
        let target = right.trim_end_matches("(ESTABLISHED)").trim().to_string();
        if !target.is_empty() && !targets.contains(&target) {
            targets.push(target);
        }
    }
    targets.sort();
    eprintln!("[V2Proxy][OK] get_process_targets pid={pid} · {} targets", targets.len());
    Ok(targets)
}
