//! Optional user-session barrier: never requires privileged GPU controls.
use crate::Result;
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, Read, Write},
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt},
        net::UnixStream,
    },
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
fn runtime() -> Option<PathBuf> {
    let uid = unsafe { libc::getuid() };
    let root = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{uid}")));
    let m = fs::symlink_metadata(&root).ok()?;
    (m.is_dir() && m.uid() == uid && m.mode() & 0o077 == 0).then(|| root.join("gameshift"))
}
fn ticks(pid: u32) -> Option<u64> {
    let s = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    s.rsplit_once(") ")?
        .1
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()
}
fn call(request: Value) -> Result<Option<Value>> {
    let Some(root) = runtime() else {
        return Ok(None);
    };
    let mut stream = match UnixStream::connect(root.join("profiles.sock")) {
        Ok(v) => v,
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Ok(None)
        }
        Err(e) => return Err(e.to_string()),
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(8)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(1)))
        .map_err(|e| e.to_string())?;
    writeln!(stream, "{request}").map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    std::io::BufReader::new(stream)
        .take(1024 * 1024 + 1)
        .read_until(b'\n', &mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 1024 * 1024 {
        return Err("Réponse de profils trop volumineuse".into());
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if value["ok"] != true {
        return Err("Mettre à jour ou arrêter l’ancien agent de profils avant la mesure".into());
    }
    Ok(Some(value["data"].clone()))
}
pub fn profile_snapshot() -> Value {
    match call(json!({"command":"get"})) {
        Ok(Some(state)) => {
            let profile = state["active"].as_str().and_then(|id| {
                state["config"]["profiles"]
                    .as_array()?
                    .iter()
                    .find(|p| p["id"].as_str() == Some(id))
            });
            json!({"available":true,"mode":state["config"]["mode"],"active_profile":profile,"error":state["error"]})
        }
        Ok(None) => json!({"available":false,"reason":"agent de profils absent"}),
        Err(error) => json!({"available":false,"error":error}),
    }
}
pub struct BenchmarkGuard {
    path: Option<PathBuf>,
}
impl BenchmarkGuard {
    pub fn begin() -> Result<Self> {
        let Some(dir) = runtime() else {
            return Ok(Self { path: None });
        };
        if !dir.exists() {
            return Ok(Self { path: None });
        }
        let m = fs::symlink_metadata(&dir).map_err(|e| e.to_string())?;
        if !m.is_dir() || m.uid() != unsafe { libc::getuid() } || m.mode() & 0o077 != 0 {
            return Err("Dossier de profils non sûr".into());
        }
        let pid = std::process::id();
        let start = ticks(pid).ok_or("Identité du benchmark indisponible")?;
        let seq = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let path = dir.join(format!("benchmark-{pid}-{seq}"));
        let guard = Self {
            path: Some(path.clone()),
        };
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .map_err(|e| e.to_string())?;
        writeln!(file, "{pid} {start}")
            .and_then(|()| file.sync_all())
            .map_err(|e| e.to_string())?;
        call(json!({"command":"benchmark_barrier"}))?;
        Ok(guard)
    }
}
impl Drop for BenchmarkGuard {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = fs::remove_file(path);
        }
    }
}
pub fn benchmark_active() -> bool {
    let Some(root) = runtime() else { return false };
    let Ok(entries) = fs::read_dir(root) else {
        return false;
    };
    entries.flatten().take(256).any(|entry| {
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with("benchmark-")
        {
            return false;
        }
        let Ok(m) = fs::symlink_metadata(entry.path()) else {
            return false;
        };
        if !m.is_file() || m.uid() != unsafe { libc::getuid() } || m.len() > 128 {
            return false;
        }
        let Ok(s) = fs::read_to_string(entry.path()) else {
            return false;
        };
        let mut parts = s.split_whitespace();
        let pid = parts.next().and_then(|v| v.parse().ok());
        let start = parts.next().and_then(|v| v.parse().ok());
        pid.zip(start)
            .is_some_and(|(pid, start)| ticks(pid) == Some(start))
    })
}
