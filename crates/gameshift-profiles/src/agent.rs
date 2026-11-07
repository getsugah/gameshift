use crate::*;
use serde_json::json;
use std::{
    os::{fd::AsRawFd, unix::net::UnixListener},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};
static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn stop(_: libc::c_int) {
    STOP.store(true, Ordering::Relaxed);
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Status {
    pub config: Config,
    pub revision: u64,
    pub active: Option<String>,
    pub reason: String,
    pub error: Option<String>,
    pub tray_visible: bool,
    pub tray_available: bool,
    pub pending: bool,
}
#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Get {},
    BenchmarkBarrier {},
    Configure { config: Config, revision: u64 },
    Manual { profile: String },
    Automatic {},
    DriverAutomatic {},
    Retry {},
    Shutdown {},
    Tray { visible: bool },
}
fn root_connection() -> Result<UnixStream> {
    let stream = UnixStream::connect("/run/gameshift-control-manager.sock")
        .map_err(|e| format!("Contrôleur système indisponible : {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    Ok(stream)
}
fn handle(state: &mut Status, command: Command) -> Result<bool> {
    match command {
        Command::Get {} | Command::BenchmarkBarrier {} => return Ok(false),
        Command::Configure { config, revision } => {
            if revision != state.revision {
                return Err("Les profils ont changé ; actualiser avant d’enregistrer".into());
            }
            config.validate()?;
            if config.preferences.autostart != state.config.preferences.autostart {
                configure_autostart(config.preferences.autostart)?;
            }
            config.save()?;
            state.config = config;
        }
        Command::Manual { profile } => {
            if !state.config.profiles.iter().any(|p| p.id == profile) {
                return Err("Profil absent".into());
            }
            let mut next = state.config.clone();
            next.mode = Mode::Manual { profile };
            next.save()?;
            state.config = next;
        }
        Command::Automatic {} => {
            let mut next = state.config.clone();
            next.mode = Mode::Automatic {};
            next.save()?;
            state.config = next;
        }
        Command::DriverAutomatic {} => {
            let mut next = state.config.clone();
            next.mode = Mode::Driver {};
            next.save()?;
            state.config = next;
        }
        Command::Shutdown {} => {
            state.tray_visible = false;
            STOP.store(true, Ordering::Relaxed);
            return Ok(false);
        }
        Command::Retry {} => (),
        Command::Tray { visible } => {
            state.tray_visible = visible;
            return Ok(false);
        }
    }
    state.pending = true;
    state.revision += 1;
    state.error = None;
    Ok(true)
}
fn user_peer(stream: &UnixStream) -> bool {
    let mut c: libc::ucred = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut c as *mut libc::ucred).cast(),
            &mut size,
        ) == 0
            && c.uid == libc::getuid()
    }
}
pub fn run() -> Result<()> {
    if unsafe { libc::geteuid() } == 0 {
        return Err("L’agent de profils doit fonctionner dans la session utilisateur".into());
    }
    let root = runtime()?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(root.join("profiles.lock"))
        .map_err(|e| e.to_string())?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("Un agent GameShift est déjà actif".into());
    }
    let config = Config::load()?;
    let socket = root.join("profiles.sock");
    if socket.exists() {
        fs::remove_file(&socket).map_err(|e| e.to_string())?;
    }
    let listener = UnixListener::bind(&socket).map_err(|e| e.to_string())?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let state = Arc::new(Mutex::new(Status {
        config,
        revision: 0,
        active: None,
        reason: "Initialisation".into(),
        error: None,
        tray_visible: true,
        tray_available: false,
        pending: false,
    }));
    let tray_state = state.clone();
    std::thread::spawn(move || {
        if let Err(e) = crate::tray::run(tray_state) {
            eprintln!("Icône de notification indisponible : {e}");
        }
    });
    unsafe {
        libc::signal(libc::SIGTERM, stop as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, stop as *const () as libc::sighandler_t);
    }
    let start = Instant::now();
    let mut timer = Instant::now() - Duration::from_secs(3);
    let mut connection = None;
    let mut selected_request = None::<Value>;
    let mut debounce = crate::process::Debounce::default();
    let mut failed = None::<(Option<String>, u64)>;
    let mut force = false;
    while !STOP.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                if !user_peer(&stream) {
                    continue;
                }
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                let mut bytes = Vec::new();
                let read = std::io::BufReader::new(&mut stream)
                    .take(1024 * 1024 + 1)
                    .read_until(b'\n', &mut bytes);
                let result = if read.is_err() || bytes.len() > 1024 * 1024 {
                    Err("Requête invalide ou trop volumineuse".into())
                } else {
                    serde_json::from_slice::<Command>(&bytes)
                        .map_err(|e| e.to_string())
                        .and_then(|cmd| {
                            let mut state = state.lock().map_err(|e| e.to_string())?;
                            if matches!(cmd, Command::BenchmarkBarrier {}) {
                                if let Some(stream) = connection.as_mut() {
                                    exchange(stream, &json!({"command":"ping","benchmark":true}))?;
                                }
                            }
                            let changed = handle(&mut state, cmd)?;
                            if changed {
                                force = true;
                            }
                            serde_json::to_value(&*state).map_err(|e| e.to_string())
                        })
                };
                let value = match result {
                    Ok(data) => json!({"ok":true,"data":data}),
                    Err(error) => json!({"ok":false,"error":error}),
                };
                let _ = writeln!(stream, "{value}");
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
            Err(e) => return Err(e.to_string()),
        }
        if STOP.load(Ordering::Relaxed) {
            break;
        }
        if timer.elapsed() >= Duration::from_secs(1) || force {
            timer = Instant::now();
            let config = state.lock().map_err(|e| e.to_string())?.config.clone();
            let processes = if matches!(config.mode, Mode::Automatic {}) {
                crate::process::scan(&config)
            } else {
                vec![]
            };
            let benchmark = gameshift_control::benchmark::benchmark_active();
            let wanted = crate::process::select_for_benchmark(
                &config,
                &processes,
                state.lock().unwrap().active.clone(),
                benchmark,
            );
            let now = start.elapsed().as_secs();
            if connection.is_none() {
                match root_connection() {
                    Ok(stream) => connection = Some(stream),
                    Err(error) => state.lock().unwrap().error = Some(error),
                }
            }
            if let Some(stream) = connection.as_mut() {
                match exchange(stream, &json!({"command":"ping","benchmark":benchmark})) {
                    Ok(data) => {
                        if data["manager_protocol"] != 3 {
                            connection = None;
                            state.lock().unwrap().error =
                                Some("Protocole du contrôleur incompatible".into());
                            continue;
                        }
                        if data["warning"].is_null() && data["fault"].is_null() && failed.is_none()
                        {
                            state.lock().unwrap().error = None;
                        }
                        if let Some(warning) = data["warning"].as_str() {
                            state.lock().unwrap().error = Some(warning.into());
                        }

                        if data["active"] != true
                            && selected_request
                                .as_ref()
                                .is_some_and(|v| v.get("identity").is_some())
                        {
                            state.lock().unwrap().active = None;
                            selected_request = None;
                        }
                        if let Some(error) = data["fault"].as_str() {
                            let mut s = state.lock().unwrap();
                            s.error = Some(error.into());
                            if data["active"] != true {
                                s.active = None;
                                selected_request = None;
                            }
                            if failed.is_none() {
                                failed = Some((wanted.profile.clone(), s.revision));
                            }
                        }
                    }
                    Err(error) => {
                        connection = None;
                        selected_request = None;
                        let mut s = state.lock().unwrap();
                        s.active = None;
                        s.error = Some(error);
                    }
                }
            }
            let revision = state.lock().unwrap().revision;
            let request = wanted
                .profile
                .as_ref()
                .and_then(|id| config.profiles.iter().find(|p| &p.id == id))
                .map(|p| serde_json::to_value(&p.request).unwrap());
            let request = if matches!(config.mode, Mode::Driver {}) {
                Some(json!({"driver_automatic":true}))
            } else {
                request
            };
            let active = state.lock().unwrap().active.clone();
            let needs_change = active != wanted.profile || selected_request != request;
            let inhibited = failed.as_ref() == Some(&(wanted.profile.clone(), revision)) && !force;
            if needs_change
                && !inhibited
                && debounce.ready(
                    &wanted.profile,
                    now,
                    force || !matches!(config.mode, Mode::Automatic { .. }),
                )
            {
                if let Some(stream) = connection.as_mut() {
                    let command = if matches!(config.mode, Mode::Driver {}) {
                        json!({"command":"driver_automatic"})
                    } else {
                        request
                            .as_ref()
                            .map(|r| json!({"command":"apply","request":r}))
                            .unwrap_or_else(|| json!({"command":"restore"}))
                    };
                    match exchange(stream, &command) {
                        Ok(_) => {
                            let mut s = state.lock().unwrap();
                            s.active = wanted.profile.clone();
                            s.reason = wanted.reason.clone();
                            s.error = None;
                            selected_request = request;
                            debounce.changed(now);
                            failed = None;
                        }
                        Err(error) => {
                            state.lock().unwrap().error = Some(error);
                            failed = Some((wanted.profile.clone(), revision));
                            selected_request = None;
                        }
                    }
                }
            } else if active == wanted.profile {
                state.lock().unwrap().reason = wanted.reason;
            }
            state.lock().unwrap().pending = false;
            force = false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if let Some(mut stream) = connection {
        let _ = exchange(&mut stream, &json!({"command":"restore"}));
    }
    let _ = fs::remove_file(socket);
    drop(lock);
    Ok(())
}
