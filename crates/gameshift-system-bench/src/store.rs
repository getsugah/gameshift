//! Content-addressed immutable reports; references are independent metadata.
use crate::session::{digest, Session, SystemKind, MAX_REPORT_BYTES};
use serde::{Deserialize, Serialize};
use std::os::unix::ffi::OsStrExt;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub distribution: String,
    #[serde(default)]
    pub protocol: String,
    pub completed_unix_seconds: u64,
    pub kind: SystemKind,
}
pub fn root() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".local/state")
        })
        .join("gameshift/distribution-runs")
}
fn valid_id(id: &str) -> Result<(), String> {
    if id.len() == 64 && id.bytes().all(|c| c.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err("Identifiant de session invalide".into())
    }
}
fn bounded_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > MAX_REPORT_BYTES {
        return Err("Rapport trop volumineux (maximum 128 Mio)".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_REPORT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_REPORT_BYTES {
        return Err("Rapport trop volumineux".into());
    }
    Ok(bytes)
}
fn decode(bytes: &[u8]) -> Result<Session, String> {
    let session: Session =
        serde_json::from_slice(bytes).map_err(|e| format!("Rapport invalide : {e}"))?;
    session.validate()?;
    Ok(session)
}
pub fn read(path: &Path) -> Result<Session, String> {
    decode(&bounded_bytes(path)?)
}
pub fn import(root: &Path, path: &Path) -> Result<Entry, String> {
    import_bytes(root, &bounded_bytes(path)?)
}
pub fn import_bytes(root: &Path, bytes: &[u8]) -> Result<Entry, String> {
    if bytes.len() as u64 > MAX_REPORT_BYTES {
        return Err("Rapport trop volumineux".into());
    }
    let session = decode(bytes)?;
    save_encoded(root, &session, bytes)
}
pub fn load(root: &Path, id: &str) -> Result<Session, String> {
    valid_id(id)?;
    let path = root.join(id).join("result.json");
    let bytes = bounded_bytes(&path)?;
    if bytes.len() as u64 > MAX_REPORT_BYTES || digest(&bytes)? != id {
        return Err("Intégrité du rapport incorrecte".into());
    }
    decode(&bytes)
}
pub fn save(root: &Path, session: &Session) -> Result<Entry, String> {
    session.validate()?;
    let bytes = serde_json::to_vec(session).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_REPORT_BYTES {
        return Err("Rapport trop volumineux".into());
    }
    save_encoded(root, session, &bytes)
}
fn save_encoded(root: &Path, session: &Session, bytes: &[u8]) -> Result<Entry, String> {
    let id = digest(bytes)?;
    let entry = Entry {
        id: id.clone(),
        distribution: session.fingerprint.distribution.clone(),
        protocol: session.protocol.clone(),
        completed_unix_seconds: session.completed_unix_seconds,
        kind: session.kind.clone(),
    };
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let destination = root.join(&id);
    if destination.exists() {
        load(root, &id)?;
        return Ok(entry);
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let temporary = root.join(format!(
        ".partial-{}-{nonce}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&temporary).map_err(|e| e.to_string())?;
    let result = (|| {
        for (name, contents) in [
            ("result.json", bytes.to_vec()),
            (
                "report.txt",
                crate::system::reporting::individual(session).into_bytes(),
            ),
            (
                "summary.json",
                serde_json::to_vec(&entry).map_err(|e| e.to_string())?,
            ),
        ] {
            let mut f = fs::File::create(temporary.join(name)).map_err(|e| e.to_string())?;
            f.write_all(&contents).map_err(|e| e.to_string())?;
            f.sync_all().map_err(|e| e.to_string())?;
        }
        fs::File::open(&temporary)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        match fs::rename(&temporary, &destination) {
            Ok(()) => (),
            Err(_) if destination.exists() => {
                load(root, &id)?;
            }
            Err(e) => return Err(e.to_string()),
        }
        fs::File::open(root)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        Ok(())
    })();
    let _ = fs::remove_dir_all(&temporary);
    result?;
    Ok(entry)
}
pub fn list(root: &Path) -> Result<Vec<Entry>, String> {
    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.to_string()),
    };
    let mut result = Vec::new();
    for entry in entries.flatten() {
        let id = entry.file_name().to_string_lossy().into_owned();
        if valid_id(&id).is_err() {
            continue;
        }
        let path = entry.path().join("summary.json");
        if fs::metadata(&path).is_ok_and(|s| s.len() < 65536) {
            if let Ok(bytes) = fs::read(&path) {
                if let Ok(summary) = serde_json::from_slice::<Entry>(&bytes) {
                    if summary.id == id {
                        result.push(summary);
                    }
                }
            }
        }
    }
    result.sort_by_key(|e| std::cmp::Reverse(e.completed_unix_seconds));
    Ok(result)
}
fn atomic_new(target: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let temporary = parent.join(format!(
        ".gameshift-partial-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let outcome = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|e| e.to_string())?;
        let from =
            std::ffi::CString::new(temporary.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
        let to =
            std::ffi::CString::new(target.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
        // Atomic no-replace rename works on filesystems without hard links too.
        if unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                from.as_ptr(),
                libc::AT_FDCWD,
                to.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        } != 0
        {
            return Err(format!(
                "Enregistrement sans écrasement : {}",
                std::io::Error::last_os_error()
            ));
        }
        fs::File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())
    })();
    let _ = fs::remove_file(temporary);
    outcome
}
pub fn export(root: &Path, id: &str, target: &Path) -> Result<(), String> {
    valid_id(id)?;
    let bytes = bounded_bytes(&root.join(id).join("result.json"))?;
    if digest(&bytes)? != id {
        return Err("Intégrité du rapport incorrecte".into());
    }
    decode(&bytes)?;
    atomic_new(target, &bytes)
}
/// Multiple reference markers; no modification of any raw report.
pub fn reference(root: &Path, id: &str) -> Result<(), String> {
    let session = load(root, id)?;
    let dir = root.join("references");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let target = dir.join(format!("{id}.json"));
    let value=serde_json::to_vec(&serde_json::json!({"session_id":id,"reference_index":100,"score_version":if session.system.is_some(){crate::system::reporting::SCORE_VERSION}else{crate::comparison::SCORE_VERSION}})).map_err(|e|e.to_string())?;
    if target.exists() {
        if fs::read(&target).map_err(|e| e.to_string())? == value {
            return Ok(());
        }
        return Err("Référence existante incorrecte".into());
    }
    atomic_new(&target, &value)
}

/// Remove a result from the active history without deleting its data.
/// References remain intact so a manual restoration also restores reference status.
pub fn archive(root: &Path, id: &str) -> Result<(), String> {
    load(root, id)?;
    let source = root.join(id);
    if !fs::symlink_metadata(&source)
        .map_err(|e| e.to_string())?
        .is_dir()
    {
        return Err("Le résultat n’est pas un dossier interne".into());
    }
    let trash = root.join("removed");
    fs::create_dir_all(&trash).map_err(|e| e.to_string())?;
    let destination = trash.join(id);
    if destination.try_exists().map_err(|e| e.to_string())? {
        return Err("Une copie retirée existe déjà ; aucun résultat écrasé".into());
    }
    fs::rename(source, destination).map_err(|e| e.to_string())?;
    fs::File::open(root)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())
}
