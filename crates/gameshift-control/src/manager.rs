//! Socket-activated hardware guardian. One enrolled user's agent owns the lease.
use super::*;
use serde_json::{json, Value};
use std::{
    os::fd::{AsRawFd, FromRawFd},
    os::unix::net::{UnixListener, UnixStream},
    sync::Arc,
};
const LIMIT: usize = 65536;
fn peer(stream: &UnixStream) -> Result<u32> {
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(cred.uid)
}
fn reply(stream: &mut UnixStream, value: Value) -> Result<()> {
    writeln!(stream, "{value}")
        .and_then(|()| stream.flush())
        .map_err(|e| e.to_string())
}
fn conflict() -> Option<String> {
    for entry in fs::read_dir("/proc").ok()?.flatten() {
        if entry.file_name().to_string_lossy().parse::<u32>().is_err() {
            continue;
        }
        if let Ok(name) = fs::read_to_string(entry.path().join("comm")) {
            if ["lact", "lactd", "corectrl"].contains(&name.trim().to_ascii_lowercase().as_str()) {
                return Some(name.trim().into());
            }
        }
    }
    None
}
fn begin(request: Request) -> Result<(Lease, Catalog)> {
    if let Some(name) = conflict() {
        return Err(format!(
            "{name} est actif : arrêter son contrôle GPU avant GameShift"
        ));
    }
    let io = Sysfs::open(&request.identity.pci)?;
    let identity = io.identity()?;
    secure_directory(Path::new("/run/gameshift-control"))?;
    secure_directory(Path::new("/var/log/gameshift-control"))?;
    let path = Path::new("/run/gameshift-control").join(format!("{}.json", identity.pci));
    if path.exists() && read_recovery(&path)?.armed {
        return Err("Récupération précédente requise avant un nouveau profil".into());
    }
    let recovery = Recovery {
        schema_version: 1,
        armed: true,
        prepared: prepare(&io, request)?,
        errors: vec![],
    };
    let mut journal = secured_file(
        &Path::new("/var/log/gameshift-control").join(format!("{}-manager.jsonl", identity.pci)),
    )?;
    audit(
        &mut journal,
        "prepared",
        json!({"owner_uid":super::install::owner()?,"recovery":recovery}),
    )?;
    save_recovery(&path, &recovery)?;
    let mut lease = Lease {
        io,
        recovery,
        path,
        journal,
        restored: false,
        touched: false,
    };
    if lease.io.thermal_alarm()? == Some(true) {
        return Err("Seuil thermique critique déjà atteint".into());
    }
    let mut tracked = Tracked {
        inner: &mut lease.io,
        touched: &mut lease.touched,
    };
    let after = execute(&mut tracked, &lease.recovery.prepared)?;
    audit(
        &mut lease.journal,
        "profile_applied",
        serde_json::to_value(&after).map_err(|e| e.to_string())?,
    )?;
    Ok((lease, after))
}
fn end(lease: &mut Option<Lease>) -> Result<()> {
    if let Some(mut lease) = lease.take() {
        let errors = lease.undo();
        if !errors.is_empty() {
            return Err(format!("Restauration incomplète : {}", errors.join(" ; ")));
        }
    }
    Ok(())
}
#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Ping {
        #[serde(default)]
        benchmark: bool,
    },
    Apply {
        request: Request,
    },
    Restore {},
    DriverAutomatic {},
}
fn session(mut stream: UnixStream) -> Result<()> {
    if peer(&stream)? != super::install::owner()? {
        return Err("Compte non autorisé pour les contrôles GPU".into());
    }
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .map_err(|e| e.to_string())?;
    // On early errors, drop/restore the lease before releasing the GPU lock.
    let mut lock = None::<fs::File>;
    let mut lease = None::<Lease>;
    let mut pending = Vec::new();
    let mut last_message = Instant::now();
    let mut last_check = Instant::now();
    let mut fault = None::<String>;
    let mut warning = None::<String>;
    let mut unavailable_since = None::<Instant>;
    let mut thermal_available = false;
    let mut benchmark_held = false;
    loop {
        if INTERRUPTED.load(Ordering::Relaxed) || last_message.elapsed() > Duration::from_secs(15) {
            break;
        }
        if last_check.elapsed() >= Duration::from_secs(2) {
            last_check = Instant::now();
            if let Some(current) = lease.as_mut() {
                let check = (|| -> Result<()> {
                    if let Some(name) = conflict() {
                        return Err(format!("Conflit détecté avec {name}"));
                    }
                    let alarm = current
                        .io
                        .thermal_alarm()
                        .map_err(|e| format!("GPU temporairement indisponible : {e}"))?;
                    if thermal_available && alarm.is_none() {
                        return Err("Seuil thermique précédemment disponible disparu".into());
                    }
                    if alarm == Some(true) {
                        return Err("Seuil thermique critique atteint".into());
                    }
                    if !benchmark_held
                        && verify_applied(&current.io, &current.recovery.prepared.request).is_err()
                    {
                        // The same validated profile is reapplied after resume/reset/drift, without changing its recovery baseline.
                        let fresh =
                            prepare(&current.io, current.recovery.prepared.request.clone())?;
                        audit(
                            &mut current.journal,
                            "profile_reapply",
                            json!({"reason":"réglages modifiés, reprise ou réinitialisation du pilote"}),
                        )?;
                        let mut tracked = Tracked {
                            inner: &mut current.io,
                            touched: &mut current.touched,
                        };
                        execute(&mut tracked, &fresh)?;
                    }
                    Ok(())
                })();
                if let Err(error) = check {
                    let transient = error.starts_with("GPU temporairement indisponible");
                    let since = unavailable_since.get_or_insert_with(Instant::now);
                    if transient && since.elapsed() < Duration::from_secs(15) {
                        warning = Some(error);
                    } else {
                        let recovery = end(&mut lease).err();
                        lock = None;
                        warning = None;
                        fault = Some(format!(
                            "{error}{}",
                            recovery.map(|e| format!(" ; {e}")).unwrap_or_default()
                        ));
                    }
                } else {
                    unavailable_since = None;
                    warning = None;
                }
            }
        }
        let mut bytes = [0u8; 4096];
        match stream.read(&mut bytes) {
            Ok(0) => break,
            Ok(n) => pending.extend_from_slice(&bytes[..n]),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted
                ) =>
            {
                continue
            }
            Err(e) => return Err(e.to_string()),
        }
        if pending.len() > LIMIT {
            return Err("Message trop volumineux".into());
        }
        while let Some(endline) = pending.iter().position(|b| *b == b'\n') {
            let bytes = pending.drain(..=endline).collect::<Vec<_>>();
            last_message = Instant::now();
            let result = (|| -> Result<Value> {
                match serde_json::from_slice::<Command>(&bytes).map_err(|e| e.to_string())? {
                    Command::Ping { benchmark } => {
                        benchmark_held = benchmark;
                        Ok(
                            json!({"manager_protocol":3,"version":env!("CARGO_PKG_VERSION"),"active":lease.is_some(),"fault":fault,"warning":warning}),
                        )
                    }
                    Command::DriverAutomatic {} => {
                        let identity = lease
                            .as_ref()
                            .map(|l| l.recovery.prepared.before.identity.clone());
                        end(&mut lease)?;
                        if let Some(identity) = identity {
                            let catalog = Catalog::discover(&identity.pci)?;
                            let mut operations = Vec::new();
                            if catalog.curve.is_some() {
                                operations.push(Operation::FanAutomatic {});
                            }
                            if catalog
                                .performance
                                .as_deref()
                                .is_some_and(|m| PERFORMANCE_MODES.contains(&m))
                            {
                                operations.push(Operation::Performance {
                                    mode: "auto".into(),
                                });
                            }
                            if !operations.is_empty() {
                                let request = Request {
                                    schema_version: 1,
                                    identity,
                                    advanced_confirmed: false,
                                    fan_recovery: FanRecovery::Automatic,
                                    operations,
                                };
                                let (mut automatic, _) = begin(request)?;
                                automatic.recovery.armed = false;
                                save_recovery(&automatic.path, &automatic.recovery)?;
                                automatic.restored = true;
                                audit(
                                    &mut automatic.journal,
                                    "driver_automatic",
                                    json!({"fan":"PMFW automatic when exposed","dpm":"auto when exposed"}),
                                )?;
                            }
                        }
                        lock = None;
                        fault = None;
                        Ok(json!({"driver_automatic":true}))
                    }
                    Command::Restore {} => {
                        end(&mut lease)?;
                        lock = None;
                        fault = None;
                        Ok(json!({"restored":true}))
                    }
                    Command::Apply { request } => {
                        if let Some(name) = conflict() {
                            return Err(format!("Conflit avec {name}"));
                        }
                        // Validate the new request before disturbing a working profile.
                        let io = Sysfs::open(&request.identity.pci)?;
                        prepare(&io, request.clone())?;
                        end(&mut lease)?;
                        lock = None;
                        let pci = canonical_pci_for_lock(&request.identity.pci)?;
                        let file = secured_file(
                            &Path::new("/run/gameshift-control").join(format!("{pci}.lock")),
                        )?;
                        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }
                            != 0
                        {
                            return Err("GPU déjà contrôlé par une autre session GameShift".into());
                        }
                        lock = Some(file);
                        thermal_available = Sysfs::open(&request.identity.pci)?
                            .thermal_alarm()?
                            .is_some();
                        let (new_lease, after) = begin(request)?;
                        lease = Some(new_lease);
                        unavailable_since = None;
                        warning = None;
                        fault = None;
                        Ok(json!({"applied":true,"after":after}))
                    }
                }
            })();
            let value = match result {
                Ok(data) => json!({"ok":true,"data":data}),
                Err(error) => {
                    fault = Some(error.clone());
                    json!({"ok":false,"error":error})
                }
            };
            reply(&mut stream, value)?;
        }
    }
    let result = end(&mut lease);
    drop(lock);
    result
}
fn canonical_pci_for_lock(pci: &str) -> Result<String> {
    // Opening a real GPU also canonicalizes and validates its PCI identity.
    Ok(Sysfs::open(pci)?.identity()?.pci)
}
pub fn run() -> Result<()> {
    if unsafe { libc::geteuid() } != 0 {
        return Err("Service système privilégié requis".into());
    }
    if std::env::args().count() != 2
        || std::env::var("LISTEN_FDS").as_deref() != Ok("1")
        || std::env::var("LISTEN_PID")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            != Some(std::process::id())
    {
        return Err("Activer gameshift-control.socket via systemd".into());
    }
    super::install::owner()?;
    unsafe {
        libc::signal(
            libc::SIGTERM,
            interrupted as *const () as libc::sighandler_t,
        );
        libc::signal(libc::SIGINT, interrupted as *const () as libc::sighandler_t);
    }
    let listener = unsafe { UnixListener::from_raw_fd(3) };
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let busy = Arc::new(AtomicBool::new(false));
    let mut worker = None::<std::thread::JoinHandle<()>>;
    while !INTERRUPTED.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                if peer(&stream).ok() != super::install::owner().ok() {
                    let _ = reply(
                        &mut stream,
                        json!({"ok":false,"error":"Compte non autorisé"}),
                    );
                    continue;
                }
                if busy.swap(true, Ordering::Relaxed) {
                    let _ = reply(
                        &mut stream,
                        json!({"ok":false,"error":"Un agent de profils est déjà connecté"}),
                    );
                    continue;
                }
                if let Some(previous) = worker.take() {
                    let _: std::thread::Result<()> = previous.join();
                }
                let busy = busy.clone();
                worker = Some(std::thread::spawn(move || {
                    if let Err(error) = session(stream) {
                        eprintln!("{error}");
                    }
                    busy.store(false, Ordering::Relaxed);
                }));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100))
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    if let Some(worker) = worker {
        let _ = worker.join();
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manager_rejects_arbitrary_paths_and_commands() {
        for text in [
            r#"{"command":"apply","path":"/sys/reset"}"#,
            r#"{"command":"ping","path":"/etc/shadow"}"#,
            r#"{"command":"reset_gpu"}"#,
        ] {
            assert!(serde_json::from_str::<Command>(text).is_err());
        }
    }
}
