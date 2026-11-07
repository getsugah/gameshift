//! User-owned profiles and deterministic rules; no GTK and no privileged filesystem writes.
pub mod agent;
pub mod process;
pub mod tray;
use gameshift_control::Request;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, Read, Write},
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        net::UnixStream,
    },
    path::{Path, PathBuf},
    time::Duration,
};
pub type Result<T> = std::result::Result<T, String>;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub request: Request,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Matcher {
    Native {
        path: String,
    },
    WindowsExecutable {
        basename: String,
        steam_app_id: Option<u32>,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub id: String,
    pub profile: String,
    pub matcher: Matcher,
    pub priority: i32,
    pub enabled: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Mode {
    Automatic {},
    Manual { profile: String },
    Driver {},
}
impl Default for Mode {
    fn default() -> Self {
        Self::Automatic {}
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Preferences {
    pub autostart: bool,
    pub keep_tray_on_close: bool,
    pub gui_launcher: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub profiles: Vec<Profile>,
    pub rules: Vec<Rule>,
    pub default_profile: Option<String>,
    pub mode: Mode,
    pub preferences: Preferences,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: 1,
            profiles: vec![],
            rules: vec![],
            default_profile: None,
            mode: Mode::default(),
            preferences: Preferences::default(),
        }
    }
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 || self.profiles.len() > 128 || self.rules.len() > 512 {
            return Err("Version ou taille des profils invalide".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        for p in &self.profiles {
            if !valid_id(&p.id)
                || !ids.insert(&p.id)
                || p.name.trim().is_empty()
                || p.name.len() > 120
                || p.name.chars().any(char::is_control)
            {
                return Err("Nom ou identifiant de profil invalide/dupliqué".into());
            }
            if p.request.schema_version != 1
                || p.request.operations.is_empty()
                || p.request.operations.len() > 32
                || gameshift_control::canonical_pci(&p.request.identity.pci).is_err()
            {
                return Err("Paramètres GPU du profil invalides".into());
            }
            if p.request
                .operations
                .iter()
                .any(gameshift_control::Operation::advanced)
                && !p.request.advanced_confirmed
            {
                return Err(
                    "Le profil avancé doit être autorisé lors de son enregistrement".into(),
                );
            }
        }
        let exists = |id: &str| self.profiles.iter().any(|p| p.id == id);
        if self
            .default_profile
            .as_deref()
            .is_some_and(|id| !exists(id))
        {
            return Err("Profil par défaut absent".into());
        }
        if matches!(&self.mode,Mode::Manual{profile} if !exists(profile)) {
            return Err("Profil manuel absent".into());
        }
        let mut rules = std::collections::BTreeSet::new();
        for r in &self.rules {
            if !valid_id(&r.id) || !rules.insert(&r.id) || !exists(&r.profile) {
                return Err("Règle invalide ou profil absent".into());
            }
            let launcher = match &r.matcher {
                Matcher::Native { path } => Path::new(path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_ascii_lowercase(),
                Matcher::WindowsExecutable { basename, .. } => basename.to_ascii_lowercase(),
            };
            if [
                "steam",
                "steam.sh",
                "steamwebhelper",
                "steam.exe",
                "steamwebhelper.exe",
            ]
            .contains(&launcher.as_str())
            {
                return Err("Steam seul ne désigne pas un jeu ; choisir son exécutable .exe et éventuellement son App ID".into());
            }
            match &r.matcher{
                Matcher::Native{path} if Path::new(path).is_absolute()&&path.len()<=4096&&!path.contains('\0')=>(),
                Matcher::WindowsExecutable{basename,steam_app_id} if basename.to_ascii_lowercase().ends_with(".exe")&&!basename.contains(['\\','/','\0'])&&basename.len()<=255&&steam_app_id.is_none_or(|v|v>0)=>(),
                _=>return Err("Identifiant d’application non fiable : choisir un chemin absolu ou un nom .exe".into())
            }
        }
        Ok(())
    }
    pub fn load() -> Result<Self> {
        load_at(&config_path()?)
    }
    pub fn save(&self) -> Result<()> {
        self.validate()?;
        save_at(&config_path()?, self)
    }
}
pub fn config_path() -> Result<PathBuf> {
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
        .ok_or("Dossier utilisateur absent")?;
    Ok(root.join("gameshift/gpu-profiles.json"))
}
pub fn load_at(path: &Path) -> Result<Config> {
    match fs::File::open(path) {
        Ok(file) => {
            let mut data = Vec::new();
            file.take(1024 * 1024 + 1)
                .read_to_end(&mut data)
                .map_err(|e| e.to_string())?;
            if data.len() > 1024 * 1024 {
                return Err("Fichier profils trop volumineux".into());
            }
            let config: Config = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
            config.validate()?;
            Ok(config)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(e.to_string()),
    }
}
pub fn save_at(path: &Path, config: &Config) -> Result<()> {
    config.validate()?;
    let data = serde_json::to_vec_pretty(config).map_err(|e| e.to_string())?;
    if data.len() > 900 * 1024 {
        return Err("Configuration trop volumineuse".into());
    }
    fs::create_dir_all(path.parent().ok_or("Dossier absent")?).map_err(|e| e.to_string())?;
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    file.write_all(&data)
        .and_then(|()| file.sync_all())
        .map_err(|e| e.to_string())?;
    fs::rename(temp, path)
        .and_then(|()| fs::File::open(path.parent().unwrap())?.sync_all())
        .map_err(|e| e.to_string())
}
pub fn new_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| e.to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
pub fn runtime() -> Result<PathBuf> {
    let uid = unsafe { libc::getuid() };
    let root = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{uid}")));
    let meta = fs::symlink_metadata(&root).map_err(|e| e.to_string())?;
    if !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o077 != 0 {
        return Err("Répertoire de session utilisateur non sûr".into());
    }
    let dir = root.join("gameshift");
    match fs::create_dir(&dir) {
        Ok(()) => fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e.to_string()),
    }
    let meta = fs::symlink_metadata(&dir).map_err(|e| e.to_string())?;
    if !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o077 != 0 {
        return Err("Socket utilisateur non sûr".into());
    }
    Ok(dir)
}
pub fn exchange(stream: &mut UnixStream, request: &Value) -> Result<Value> {
    writeln!(stream, "{request}")
        .and_then(|()| stream.flush())
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    std::io::BufReader::new(stream)
        .take(1024 * 1024 + 1)
        .read_until(b'\n', &mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.is_empty() || bytes.len() > 1024 * 1024 {
        return Err("Réponse absente ou trop volumineuse".into());
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if value["ok"] != true {
        return Err(value["error"]
            .as_str()
            .unwrap_or("Erreur du service")
            .into());
    }
    Ok(value["data"].clone())
}
pub fn call(request: Value) -> Result<Value> {
    let mut stream = UnixStream::connect(runtime()?.join("profiles.sock"))
        .map_err(|e| format!("Agent de profils indisponible : {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(8)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    exchange(&mut stream, &request)
}
pub fn start() -> Result<()> {
    let status = std::process::Command::new("/usr/bin/systemctl")
        .env_remove("LD_LIBRARY_PATH")
        .env_remove("LD_PRELOAD")
        .env_remove("LD_AUDIT")
        .args(["--user", "start", "gameshift-profiles.service"])
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("Impossible de démarrer l’agent utilisateur systemd".into());
    }
    for _ in 0..20 {
        if let Ok(mut stream) = UnixStream::connect(runtime()?.join("profiles.sock")) {
            let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
            let _ = stream.set_write_timeout(Some(Duration::from_millis(200)));
            if exchange(&mut stream, &json!({"command":"get"})).is_ok() {
                return Ok(());
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(
        "L’agent ne répond pas après démarrage ; consulter journalctl --user -u gameshift-profiles"
            .into(),
    )
}
pub fn configure_autostart(enabled: bool) -> Result<()> {
    let action = if enabled { "enable" } else { "disable" };
    let status = std::process::Command::new("/usr/bin/systemctl")
        .env_remove("LD_LIBRARY_PATH")
        .env_remove("LD_PRELOAD")
        .env_remove("LD_AUDIT")
        .args(["--user", action, "gameshift-profiles.service"])
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("Démarrage automatique systemd refusé".into())
    }
}
pub fn component() -> Result<Value> {
    let path = Path::new("/usr/libexec/gameshift-control");
    let m = fs::symlink_metadata(path).map_err(|_| "Contrôles GPU non activés")?;
    if !m.is_file() || m.uid() != 0 || m.mode() & 0o022 != 0 {
        return Err("Composant privilégié non sûr".into());
    }
    for required in [
        "/usr/libexec/gameshift-profiles",
        "/usr/lib/systemd/system/gameshift-control.socket",
        "/usr/lib/systemd/system/gameshift-control.service",
        "/usr/lib/systemd/user/gameshift-profiles.service",
    ] {
        let m =
            fs::symlink_metadata(required).map_err(|_| "Installation des contrôles incomplète")?;
        if !m.is_file() || m.uid() != 0 || m.mode() & 0o022 != 0 {
            return Err("Composants système non sûrs".into());
        }
    }
    let out = std::process::Command::new(path)
        .arg("--component-version")
        .env_remove("LD_LIBRARY_PATH")
        .env_remove("LD_PRELOAD")
        .env_remove("LD_AUDIT")
        .output()
        .map_err(|e| e.to_string())?;
    let version: Value = serde_json::from_slice(&out.stdout)
        .map_err(|_| "Composant ancien : mise à jour nécessaire")?;
    if !out.status.success() || version["manager_protocol"] != 3 {
        return Err("Version du contrôleur incompatible".into());
    }
    let agent = std::process::Command::new("/usr/libexec/gameshift-profiles")
        .arg("--version")
        .env_remove("LD_LIBRARY_PATH")
        .env_remove("LD_PRELOAD")
        .env_remove("LD_AUDIT")
        .output()
        .map_err(|e| e.to_string())?;
    if !agent.status.success()
        || String::from_utf8_lossy(&agent.stdout).trim()
            != format!(
                "gameshift-profiles {} protocol 1",
                version["version"].as_str().unwrap_or("")
            )
    {
        return Err("Versions des composants incohérentes ; réactiver les contrôles".into());
    }
    Ok(version)
}
/// Bootstrap outside FUSE: root may be denied access to a user-mounted AppImage.
struct StagedBundle {
    directory: PathBuf,
}
impl Drop for StagedBundle {
    fn drop(&mut self) {
        for name in ["gameshift-control", "gameshift-profiles"] {
            let _ = fs::remove_file(self.directory.join(name));
        }
        let _ = fs::remove_dir(&self.directory);
    }
}
fn stage_bundle_at(source: &Path, parent: &Path) -> Result<StagedBundle> {
    use std::os::unix::fs::DirBuilderExt;
    let directory = parent.join(format!("activation-{}", new_id()?));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .map_err(|e| e.to_string())?;
    let staged = StagedBundle { directory };
    for name in ["gameshift-control", "gameshift-profiles"] {
        let input = source.join(name);
        let m = fs::symlink_metadata(&input).map_err(|e| e.to_string())?;
        if !m.is_file() || m.len() > 128 * 1024 * 1024 {
            return Err("Composant AppImage absent ou invalide".into());
        }
        fs::copy(input, staged.directory.join(name)).map_err(|e| e.to_string())?;
        fs::set_permissions(
            staged.directory.join(name),
            fs::Permissions::from_mode(0o755),
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(staged)
}
pub fn activate() -> Result<()> {
    if component().is_ok_and(|v| v["version"] == env!("CARGO_PKG_VERSION")) {
        return start();
    }
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let bundle = stage_bundle_at(
        executable.parent().ok_or("Dossier AppImage absent")?,
        &runtime()?,
    )?;
    let helper = bundle.directory.join("gameshift-control");
    let out = std::process::Command::new("/usr/bin/pkexec")
        .env_remove("LD_LIBRARY_PATH")
        .env_remove("LD_PRELOAD")
        .env_remove("LD_AUDIT")
        .arg("--disable-internal-agent")
        .arg(helper)
        .arg("--install")
        .env_remove("LD_LIBRARY_PATH")
        .env_remove("LD_PRELOAD")
        .env_remove("LD_AUDIT")
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "Activation refusée ou incomplète : {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let status = std::process::Command::new("/usr/bin/systemctl")
        .env_remove("LD_LIBRARY_PATH")
        .env_remove("LD_PRELOAD")
        .env_remove("LD_AUDIT")
        .args(["--user", "daemon-reload"])
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("Actualisation du service utilisateur refusée".into());
    }
    if Config::load()?.preferences.autostart {
        configure_autostart(true)?;
    }
    let status = std::process::Command::new("/usr/bin/systemctl")
        .env_remove("LD_LIBRARY_PATH")
        .env_remove("LD_PRELOAD")
        .env_remove("LD_AUDIT")
        .args(["--user", "restart", "gameshift-profiles.service"])
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("Composants installés ; redémarrage de l’agent refusé".into());
    }
    start()
}
pub fn deactivate() -> Result<()> {
    let stop = std::process::Command::new("/usr/bin/systemctl")
        .env_remove("LD_LIBRARY_PATH")
        .env_remove("LD_PRELOAD")
        .env_remove("LD_AUDIT")
        .args(["--user", "disable", "--now", "gameshift-profiles.service"])
        .status()
        .map_err(|e| e.to_string())?;
    if !stop.success() {
        return Err("Impossible d’arrêter l’agent ; désinstallation annulée".into());
    }
    let status = std::process::Command::new("/usr/bin/pkexec")
        .env_remove("LD_LIBRARY_PATH")
        .env_remove("LD_PRELOAD")
        .env_remove("LD_AUDIT")
        .args([
            "--disable-internal-agent",
            "/usr/libexec/gameshift-control",
            "--uninstall",
        ])
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("Désinstallation refusée ; profils personnels conservés".into())
    }
}
pub fn import_profile(data: &[u8]) -> Result<Profile> {
    if data.len() > 65536 {
        return Err("Profil importé trop volumineux".into());
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Export {
        schema_version: u32,
        profile: Profile,
    }
    let mut export: Export = serde_json::from_slice(data).map_err(|e| e.to_string())?;
    if export.schema_version != 1 {
        return Err("Format de profil incompatible".into());
    }
    export.profile.id = new_id()?;
    let check = Config {
        profiles: vec![export.profile.clone()],
        ..Config::default()
    };
    check.validate()?;
    Ok(export.profile)
}
pub fn export_profile(profile: &Profile) -> Result<Vec<u8>> {
    serde_json::to_vec_pretty(&json!({"schema_version":1,"profile":profile}))
        .map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests;

/// Wait for the agent's transition (which includes privileged readback), outside the UI thread.
pub fn wait_transition() -> Result<()> {
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    loop {
        let state = call(json!({"command":"get"}))?;
        if state["pending"] != true {
            if let Some(error) = state["error"].as_str() {
                return Err(error.into());
            }
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(
                "Application du profil encore en cours ; vérifier son état dans Profils".into(),
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
