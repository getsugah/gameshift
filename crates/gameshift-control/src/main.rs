//! On-demand Polkit guardian. Never starts at boot; never accepts filesystem paths.
mod install;
mod manager;
use gameshift_control::*;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
static INTERRUPTED: AtomicBool = AtomicBool::new(false);
extern "C" fn interrupted(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::Relaxed);
}
const CONFIRM_SECONDS: u64 = 30;
const LEASE_SECONDS: u64 = 3600;
#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
enum Decision {
    Confirm {},
    Restore {},
}
#[derive(Serialize, Deserialize)]
struct Recovery {
    schema_version: u32,
    armed: bool,
    prepared: Prepared,
    errors: Vec<String>,
}
fn secure_directory(path: &Path) -> Result<()> {
    if !path.exists() {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(path)
            .map_err(|e| e.to_string())?;
    }
    let m = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !m.is_dir() || m.uid() != 0 || m.mode() & 0o022 != 0 {
        return Err("Répertoire de contrôle non sûr".into());
    }
    Ok(())
}
fn secured_file(path: &Path) -> Result<fs::File> {
    let file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| e.to_string())?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if metadata.uid() != 0 || metadata.mode() & 0o077 != 0 || !metadata.is_file() {
        return Err("Journal de contrôle non sûr".into());
    }
    Ok(file)
}
fn emit(event: &str, data: serde_json::Value) -> Result<()> {
    let mut out = std::io::stdout().lock();
    writeln!(out, "{}", serde_json::json!({"event":event,"data":data}))
        .and_then(|()| out.flush())
        .map_err(|e| e.to_string())
}
fn audit(file: &mut fs::File, event: &str, data: serde_json::Value) -> Result<()> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis());
    writeln!(file,"{}",serde_json::json!({"unix_millis":now,"event":event,"caller_uid":std::env::var("PKEXEC_UID").ok(),"data":data})).and_then(|()|file.sync_all()).map_err(|e|e.to_string())
}
fn save_recovery(path: &Path, recovery: &Recovery) -> Result<()> {
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&temporary)
        .map_err(|e| e.to_string())?;
    let data = serde_json::to_vec(recovery).map_err(|e| e.to_string())?;
    file.write_all(&data)
        .and_then(|()| file.sync_all())
        .map_err(|e| e.to_string())?;
    fs::rename(&temporary, path).map_err(|e| e.to_string())?;
    fs::File::open(path.parent().unwrap())
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())
}
fn read_recovery(path: &Path) -> Result<Recovery> {
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| e.to_string())?;
    let m = file.metadata().map_err(|e| e.to_string())?;
    if !m.is_file() || m.uid() != 0 || m.mode() & 0o077 != 0 || m.len() > 1024 * 1024 {
        return Err("Fichier de récupération non sûr".into());
    }
    let mut data = Vec::new();
    file.read_to_end(&mut data).map_err(|e| e.to_string())?;
    serde_json::from_slice(&data).map_err(|e| e.to_string())
}
struct Input {
    pending: Vec<u8>,
    eof: bool,
}
impl Input {
    fn line(&mut self, deadline: Instant) -> Result<Option<String>> {
        loop {
            if self.pending.len() > 32768 {
                return Err("Message de contrôle trop volumineux".into());
            }
            if let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
                let bytes = self.pending.drain(..=end).collect::<Vec<_>>();
                return String::from_utf8(bytes)
                    .map(Some)
                    .map_err(|e| e.to_string());
            }
            if self.pending.len() > 32768 {
                return Err("Message de contrôle trop volumineux".into());
            }
            if INTERRUPTED.load(Ordering::Relaxed) || Instant::now() >= deadline {
                return Ok(None);
            }
            let mut fd = libc::pollfd {
                fd: 0,
                events: libc::POLLIN,
                revents: 0,
            };
            let result = unsafe { libc::poll(&mut fd, 1, 200) };
            if result < 0 {
                let e = std::io::Error::last_os_error();
                if e.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(e.to_string());
            }
            if result == 0 {
                continue;
            }
            let mut bytes = [0u8; 4096];
            let count = unsafe { libc::read(0, bytes.as_mut_ptr().cast(), bytes.len()) };
            if count == 0 {
                self.eof = true;
                return Ok(None);
            }
            if count < 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            self.pending.extend_from_slice(&bytes[..count as usize]);
        }
    }
}
struct Lease {
    io: Sysfs,
    recovery: Recovery,
    path: PathBuf,
    journal: fs::File,
    restored: bool,
    touched: bool,
}
impl Lease {
    fn undo(&mut self) -> Vec<String> {
        if self.restored {
            return self.recovery.errors.clone();
        }
        self.restored = true;
        let errors = if self.touched {
            restore(&mut self.io, &self.recovery.prepared)
        } else {
            Vec::new()
        };
        self.recovery.armed = !errors.is_empty();
        self.recovery.errors = errors.clone();
        let mut all_errors = errors;
        if let Err(error) = save_recovery(&self.path, &self.recovery) {
            all_errors.push(format!("Journal de récupération : {error}"));
        }
        if let Err(error) = audit(
            &mut self.journal,
            "restored",
            serde_json::json!({"errors":all_errors}),
        ) {
            all_errors.push(format!("Journal d’audit : {error}"));
        }
        all_errors
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        if !self.restored {
            let errors = self.undo();
            if !errors.is_empty() {
                eprintln!("Restauration incomplète : {}", errors.join(" ; "));
            }
        }
    }
}
struct Tracked<'a> {
    inner: &'a mut Sysfs,
    touched: &'a mut bool,
}
impl DeviceIo for Tracked<'_> {
    fn identity(&self) -> Result<Identity> {
        self.inner.identity()
    }
    fn read(&self, path: &str) -> Result<String> {
        self.inner.read(path)
    }
    fn power_paths(&self) -> Result<Vec<(u32, String)>> {
        self.inner.power_paths()
    }
    fn write(&mut self, path: &str, value: &str) -> Result<()> {
        *self.touched = true;
        self.inner.write(path, value)
    }
}
fn run() -> Result<()> {
    if unsafe { libc::geteuid() } != 0 {
        return Err("Helper privilégié : utiliser Polkit, jamais l’interface en root".into());
    }
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args != ["--session"] && !(args.len() == 2 && args[0] == "--recover") {
        return Err("Usage : gameshift-control --session | --recover ADRESSE_PCI".into());
    }
    unsafe {
        libc::signal(
            libc::SIGTERM,
            interrupted as *const () as libc::sighandler_t,
        );
        libc::signal(libc::SIGINT, interrupted as *const () as libc::sighandler_t);
        libc::signal(libc::SIGHUP, interrupted as *const () as libc::sighandler_t);
    }
    let root = Path::new("/run/gameshift-control");
    secure_directory(root)?;
    let mut input = Input {
        pending: vec![],
        eof: false,
    };
    let request = if args[0] == "--session" {
        let line = input
            .line(Instant::now() + Duration::from_secs(60))?
            .ok_or("Requête de contrôle absente")?;
        Some(serde_json::from_str::<Request>(&line).map_err(|e| e.to_string())?)
    } else {
        None
    };
    let pci = request
        .as_ref()
        .map(|r| r.identity.pci.as_str())
        .unwrap_or(&args[1.min(args.len() - 1)]);
    let io = Sysfs::open(pci)?;
    let identity = io.identity()?;
    let pci = &identity.pci;
    let lock = secured_file(&root.join(format!("{pci}.lock")))?;
    use std::os::fd::AsRawFd;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("Une session GameShift contrôle déjà ce GPU".into());
    }
    let path = root.join(format!("{pci}.json"));
    secure_directory(Path::new("/var/log/gameshift-control"))?;
    let mut journal = secured_file(&Path::new("/var/log/gameshift-control").join(format!(
        "{}-{}.jsonl",
        pci,
        std::process::id()
    )))?;
    let recovery = if let Some(request) = request {
        if path.exists() && read_recovery(&path)?.armed {
            return Err(format!("Session précédente non restaurée. Exécuter explicitement : pkexec /usr/libexec/gameshift-control --recover {pci}"));
        }
        Recovery {
            schema_version: 1,
            armed: true,
            prepared: prepare(&io, request)?,
            errors: vec![],
        }
    } else {
        let recovery = read_recovery(&path)?;
        if recovery.schema_version != 1 || recovery.prepared.before.identity != identity {
            return Err("Récupération incompatible avec le GPU courant".into());
        }
        recovery
    };
    audit(
        &mut journal,
        "prepared",
        serde_json::to_value(&recovery).map_err(|e| e.to_string())?,
    )?;
    save_recovery(&path, &recovery)?;
    let mut lease = Lease {
        io,
        recovery,
        path,
        journal,
        restored: false,
        touched: args[0] == "--recover",
    };
    if args[0] == "--recover" {
        let errors = lease.undo();
        emit("restored", serde_json::json!({"errors":errors}))?;
        return if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join(" ; "))
        };
    }
    let mut channel = libc::pollfd {
        fd: 0,
        events: 0,
        revents: 0,
    };
    if unsafe { libc::poll(&mut channel, 1, 0) } < 0
        || channel.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0
    {
        return Err("Canal de l’interface fermé avant l’application".into());
    }
    let thermal_guard = lease.io.thermal_alarm()?;
    if thermal_guard == Some(true) {
        return Err("Seuil thermique critique déjà atteint : modification refusée".into());
    }
    let mut tracked = Tracked {
        inner: &mut lease.io,
        touched: &mut lease.touched,
    };
    let after = match execute(&mut tracked, &lease.recovery.prepared) {
        Ok(after) => after,
        Err(error) => {
            let errors = lease.undo();
            let _ = emit(
                "failed",
                serde_json::json!({"error":error,"restoration_errors":errors}),
            );
            return Err(error);
        }
    };
    audit(
        &mut lease.journal,
        "applied",
        serde_json::to_value(&after).map_err(|e| e.to_string())?,
    )?;
    emit(
        "applied",
        serde_json::json!({"after":after,"before":lease.recovery.prepared.before,"confirm_seconds":CONFIRM_SECONDS,"lease_seconds":LEASE_SECONDS,"thermal_guard_available":thermal_guard.is_some(),"fan_mode_note":"Le mode interne PMFW n’est pas exposé de façon fiable ; la récupération suit le choix explicite automatique/courbe initiale."}),
    )?;
    let mut confirmed = false;
    let mut deadline = Instant::now() + Duration::from_secs(CONFIRM_SECONDS);
    loop {
        if input.eof || INTERRUPTED.load(Ordering::Relaxed) || Instant::now() >= deadline {
            break;
        }
        match lease.io.thermal_alarm() {
            Ok(Some(true)) => {
                audit(
                    &mut lease.journal,
                    "thermal_limit_reached",
                    serde_json::json!({"source":"driver hwmon critical limit"}),
                )?;
                break;
            }
            Ok(None) if thermal_guard.is_some() => {
                audit(
                    &mut lease.journal,
                    "thermal_sensor_lost",
                    serde_json::Value::Null,
                )?;
                break;
            }
            Err(error) => {
                audit(
                    &mut lease.journal,
                    "thermal_read_failed",
                    serde_json::json!({"error":error}),
                )?;
                break;
            }
            _ => (),
        }
        let Some(line) = input.line(deadline.min(Instant::now() + Duration::from_secs(1)))? else {
            continue;
        };
        let decision: Decision = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        match decision {
            Decision::Restore {} => break,
            Decision::Confirm {} if !confirmed => {
                confirmed = true;
                deadline = Instant::now() + Duration::from_secs(LEASE_SECONDS);
                audit(
                    &mut lease.journal,
                    "confirmed",
                    serde_json::json!({"lease_seconds":LEASE_SECONDS}),
                )?;
                emit(
                    "confirmed",
                    serde_json::json!({"lease_seconds":LEASE_SECONDS}),
                )?;
            }
            Decision::Confirm {} => return Err("Session déjà confirmée".into()),
        }
    }
    let errors = lease.undo();
    emit(
        "restored",
        serde_json::json!({"errors":errors,"after":Catalog::read(&lease.io).ok(),"reason":"fin de session, fermeture, signal ou expiration"}),
    )?;
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join(" ; "))
    }
}
fn main() {
    if std::env::args().skip(1).collect::<Vec<_>>() == ["--version"] {
        println!("gameshift-control {} protocol 1", env!("CARGO_PKG_VERSION"));
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("--component-version") {
        println!(
            "{}",
            serde_json::json!({"version":env!("CARGO_PKG_VERSION"),"manager_protocol":3})
        );
        return;
    }
    let result = match std::env::args().nth(1).as_deref() {
        Some("--install") => install::install(),
        Some("--uninstall") => install::uninstall(),
        Some("--daemon") => manager::run(),
        _ => run(),
    };
    if let Err(error) = result {
        let _ = emit("error", serde_json::json!({"error":error}));
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oversized_commands_are_rejected_before_reading_any_fd() {
        let mut input = Input {
            pending: vec![b'x'; 32769],
            eof: false,
        };
        input.pending.push(b'\n');
        assert!(input.line(Instant::now()).is_err());
    }
    #[test]
    fn unknown_decisions_and_extra_arguments_are_rejected() {
        assert!(serde_json::from_str::<Decision>(r#"{"command":"reset_gpu"}"#).is_err());
        assert!(
            serde_json::from_str::<Decision>(r#"{"command":"confirm","path":"/etc/passwd"}"#)
                .is_err()
        );
    }
}
