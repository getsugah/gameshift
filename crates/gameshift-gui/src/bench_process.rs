//! A fixed internal worker command isolates winit's main thread from GTK.
use gameshift_bench::{RunConfig, RunResult};
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

pub fn worker(headless: bool) -> Result<RunResult, String> {
    let mut input = BufReader::new(std::io::stdin());
    let mut line = String::new();
    input.read_line(&mut line).map_err(|e| e.to_string())?;
    if line.len() > 16_384 {
        return Err("Requête de benchmark trop longue".into());
    }
    let config: RunConfig =
        serde_json::from_str(&line).map_err(|e| format!("Protocole invalide : {e}"))?;
    config.validate()?;
    let cancel = Arc::new(AtomicBool::new(false));
    let signal = cancel.clone();
    std::thread::Builder::new()
        .name("gameshift-bench-control".into())
        .spawn(move || {
            let mut message = String::new();
            // Cancel also on EOF: closing GameShift must not leave an orphan renderer running.
            let _ = input.read_line(&mut message);
            signal.store(true, Ordering::Relaxed);
        })
        .map_err(|e| format!("Canal de contrôle : {e}"))?;
    gameshift_bench::run(config, cancel, headless)
}

fn drain(mut stream: impl Read, limit: usize) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut block = [0u8; 8192];
    loop {
        let n = stream.read(&mut block)?;
        if n == 0 {
            break;
        }
        let keep = n.min(limit.saturating_sub(bytes.len()));
        bytes.extend_from_slice(&block[..keep]);
    }
    Ok(bytes)
}

fn collect_logs(
    stderr: impl Read,
    progress: gameshift_core::duration::ProgressSink,
) -> std::io::Result<Vec<u8>> {
    let mut kept = Vec::new();
    let mut reader = BufReader::new(stderr);
    let mut line = Vec::new();
    loop {
        line.clear();
        if reader
            .by_ref()
            .take(64 * 1024)
            .read_until(b'\n', &mut line)?
            == 0
        {
            break;
        }
        if let Some(json) = line.strip_prefix(b"GAMESHIFT_PROGRESS ") {
            if let Ok(value) = serde_json::from_slice(json) {
                gameshift_core::duration::publish(&progress, value);
                continue;
            }
        }
        let remaining = (64usize * 1024).saturating_sub(kept.len());
        kept.extend_from_slice(&line[..line.len().min(remaining)]);
    }
    Ok(kept)
}

/// Called only on a worker thread. No shell or arbitrary executable is accepted.
pub fn execute_with_progress(
    config: RunConfig,
    cancel: Arc<AtomicBool>,
    progress: gameshift_core::duration::ProgressSink,
) -> Result<RunResult, String> {
    config.validate()?;
    let _profile_guard = gameshift_control::benchmark::BenchmarkGuard::begin()?;
    let mut child = Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
        .arg("--bench-worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Démarrage du renderer : {e}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or("Canal de résultat indisponible")?;
    let stderr = child.stderr.take().ok_or("Canal de journal indisponible")?;
    let output = std::thread::spawn(move || drain(stdout, 1024 * 1024 * 1024));
    let logs = std::thread::spawn(move || collect_logs(stderr, progress));
    let mut input = child.stdin.take().ok_or("Canal de contrôle indisponible")?;
    let request = serde_json::to_string(&config).map_err(|e| e.to_string())?;
    if let Err(error) = writeln!(input, "{request}") {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("Envoi du protocole : {error}"));
    }
    let mut cancellation = None;
    let status = loop {
        if cancel.load(Ordering::Relaxed) && cancellation.is_none() {
            let _ = writeln!(input, "cancel");
            cancellation = Some(Instant::now());
        }
        if cancellation.is_some_and(|t: Instant| t.elapsed() > Duration::from_secs(7)) {
            let _ = child.kill();
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("Suivi du renderer : {e}"));
            }
        }
    };
    drop(input);
    let bytes = output
        .join()
        .map_err(|_| "Lecture du résultat interrompue")?
        .map_err(|e| e.to_string())?;
    let logs = logs
        .join()
        .map_err(|_| "Lecture des journaux interrompue")?
        .map_err(|e| e.to_string())?;
    if !logs.is_empty() {
        tracing::warn!(messages = %String::from_utf8_lossy(&logs), "Journal du renderer Vulkan");
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("Test annulé ; aucun résultat partiel conservé".into());
    }
    if !status.success() {
        return Err(format!(
            "Le test a été interrompu : {}",
            String::from_utf8_lossy(&logs)
        ));
    }
    let mut result: RunResult = serde_json::from_slice(&bytes)
        .map_err(|e| format!("Résultat incomplet ou invalide : {e}"))?;
    if result.schema_version != gameshift_bench::RESULT_SCHEMA_VERSION
        || result.config != config
        || result.scene_version != gameshift_bench::SCENE_VERSION
        || result.repetitions.len() != config.repetitions as usize
        || result.validation_errors != 0
    {
        return Err("Résultat incohérent avec le protocole demandé".into());
    }
    result.environment.gpu_profile_after = Some(gameshift_control::benchmark::profile_snapshot());
    Ok(result)
}

/// Save completed JSON and raw-frame CSV to the user's state directory.
pub fn save(result: &RunResult) -> Result<std::path::PathBuf, String> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::{
        fs,
        fs::OpenOptions,
        time::{SystemTime, UNIX_EPOCH},
    };
    let dir = results_directory()?;
    fs::create_dir_all(&dir).map_err(|e| format!("Création du dossier de résultats : {e}"))?;
    let id = format!(
        "{}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        std::process::id()
    );
    let json = serde_json::to_vec_pretty(result).map_err(|e| e.to_string())?;
    let data = serde_json::to_value(result).map_err(|e| e.to_string())?;
    let text = gameshift_bench::format_text_report(&data, &report_date(&data))?;
    for (extension, bytes) in [
        ("json", json),
        ("csv", result.csv().into_bytes()),
        ("txt", text.into_bytes()),
    ] {
        let partial = dir.join(format!("{id}.{extension}.partial"));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&partial)
            .map_err(|e| format!("Export : {e}"))?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| format!("Écriture du résultat : {e}"))?;
        fs::rename(partial, dir.join(format!("{id}.{extension}")))
            .map_err(|e| format!("Finalisation de l’export : {e}"))?;
    }
    Ok(dir.join(format!("{id}.json")))
}

fn results_directory() -> Result<std::path::PathBuf, String> {
    use std::path::PathBuf;
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/state")))
        .ok_or("Dossier d’état utilisateur indisponible")?;
    Ok(state.join("gameshift/runs"))
}

fn report_date(data: &serde_json::Value) -> String {
    data["completed_unix_seconds"]
        .as_i64()
        .and_then(|t| gtk::glib::DateTime::from_unix_local(t).ok())
        .and_then(|date| date.format("%d/%m/%Y à %H:%M:%S %Z").ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "Indisponible".into())
}

/// Backfill readable reports without altering JSON/CSV or replacing user-edited text files.
pub fn export_existing_text(
    destination: Option<&std::path::Path>,
) -> Result<Vec<std::path::PathBuf>, String> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::{fs, fs::OpenOptions};
    let source = results_directory()?;
    let target = destination.unwrap_or(&source);
    fs::create_dir_all(target).map_err(|e| e.to_string())?;
    let mut paths = Vec::new();
    let mut entries = fs::read_dir(&source)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let data: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| format!("{} : {e}", path.display()))?;
        let report = gameshift_bench::format_text_report(&data, &report_date(&data))?;
        let name = path.file_stem().ok_or("Nom d’export invalide")?;
        let output = target.join(name).with_extension("txt");
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&output)
        {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                paths.push(output);
                continue;
            }
            Err(e) => return Err(e.to_string()),
        };
        file.write_all(report.as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        paths.push(output);
    }
    Ok(paths)
}

#[cfg(test)]
mod progress_tests {
    use super::*;
    #[test]
    fn binary_and_large_driver_logs_do_not_break_progress_or_exceed_log_budget() {
        let progress: gameshift_core::duration::ProgressSink = Default::default();
        let packet = serde_json::json!({"phase":"mesure","repetition":1,"repetitions":3,"case_index":1,"cases":1,"fraction":0.5,"elapsed_seconds":20.,"remaining_seconds":20.,"remaining_scope":"test","fps":null,"cpu_ms":null,"gpu_ms":null});
        let mut data = vec![0xff, b'\n'];
        data.extend(vec![b'x'; 70_000]);
        data.push(b'\n');
        data.extend(format!("GAMESHIFT_PROGRESS {packet}\n").as_bytes());
        let logs = collect_logs(std::io::Cursor::new(data), progress.clone()).unwrap();
        assert_eq!(logs.len(), 64 * 1024);
        assert_eq!(progress.lock().unwrap().as_ref().unwrap().fraction, 0.5);
    }
}
