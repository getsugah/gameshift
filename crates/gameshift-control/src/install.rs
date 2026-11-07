//! Explicit Polkit bootstrap. Only fixed system destinations, never hardware writes here.
use super::*;
use std::os::unix::fs::PermissionsExt;
const OWNER: &str = "/etc/gameshift-control/owner";
const FILES: &[&str] = &[
    "/usr/libexec/gameshift-control",
    "/usr/libexec/gameshift-profiles",
    "/usr/lib/systemd/system/gameshift-control.socket",
    "/usr/lib/systemd/system/gameshift-control.service",
    "/usr/lib/systemd/user/gameshift-profiles.service",
    "/usr/share/polkit-1/actions/io.github.gameshift.control.policy",
];
pub fn owner() -> Result<u32> {
    let m = fs::symlink_metadata(OWNER).map_err(|e| e.to_string())?;
    if !m.is_file() || m.uid() != 0 || m.mode() & 0o077 != 0 {
        return Err("Propriétaire du contrôleur non sûr".into());
    }
    fs::read_to_string(OWNER)
        .map_err(|e| e.to_string())?
        .trim()
        .parse()
        .map_err(|_| "UID invalide".into())
}
fn caller() -> Result<u32> {
    if unsafe { libc::geteuid() } != 0 {
        return Err("Autorisation Polkit requise".into());
    }
    let uid = std::env::var("PKEXEC_UID")
        .map_err(|_| "Installation à déclencher depuis GameShift avec Polkit")?
        .parse::<u32>()
        .map_err(|_| "UID Polkit invalide")?;
    if uid == 0 {
        return Err("Utiliser GameShift avec le compte normal".into());
    }
    if Path::new(OWNER).exists() && owner()? != uid {
        return Err("Contrôles déjà associés à un autre compte".into());
    }
    Ok(uid)
}
fn systemctl(args: &[&str]) -> Result<()> {
    let status = std::process::Command::new("/usr/bin/systemctl")
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("systemctl {} : {status}", args.join(" ")))
    }
}
fn payload(path: &Path) -> Result<Vec<u8>> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| e.to_string())?;
    let m = file.metadata().map_err(|e| e.to_string())?;
    if !m.is_file() || m.len() > 128 * 1024 * 1024 {
        return Err("Composant d’installation invalide".into());
    }
    let mut bytes = Vec::new();
    file.take(128 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}
fn write_fixed(path: &str, data: &[u8], mode: u32) -> Result<()> {
    let path = Path::new(path);
    if fs::symlink_metadata(path).is_ok_and(|m| !m.is_file() || m.uid() != 0) {
        return Err(format!("Destination système non sûre : {}", path.display()));
    }
    fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let temp = path.with_extension(format!("gameshift-{}", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    file.set_permissions(fs::Permissions::from_mode(mode))
        .map_err(|e| e.to_string())?;
    file.write_all(data)
        .and_then(|()| file.sync_all())
        .map_err(|e| e.to_string())?;
    fs::rename(temp, path).map_err(|e| e.to_string())
}
fn installation_lock() -> Result<fs::File> {
    use std::os::fd::AsRawFd;
    let file = secured_file(Path::new("/run/gameshift-control-install.lock"))?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("Une activation ou désinstallation est déjà en cours".into());
    }
    Ok(file)
}
pub fn install() -> Result<()> {
    if std::env::args().count() != 2 {
        return Err("Paramètres d’installation inconnus".into());
    }
    if Path::new("/etc/NIXOS").exists() {
        return Err("Sur NixOS, installer les composants via une configuration déclarative ; aucun fichier système n’a été modifié".into());
    }
    let uid = caller()?;
    let _installation = installation_lock()?;
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let source = executable.parent().ok_or("Dossier des composants absent")?;
    // Authenticate the whole bundled release via Polkit; never execute a sibling as root.
    // Copy the running, authenticated helper, not a replaceable pathname.
    let self_file = fs::File::open("/proc/self/exe").map_err(|e| e.to_string())?;
    let mut helper = Vec::new();
    self_file
        .take(128 * 1024 * 1024 + 1)
        .read_to_end(&mut helper)
        .map_err(|e| e.to_string())?;
    if helper.len() > 128 * 1024 * 1024 {
        return Err("Composant exécuté trop volumineux".into());
    }
    let agent = payload(&source.join("gameshift-profiles"))?;
    if !helper.starts_with(b"\x7fELF\x02") || !agent.starts_with(b"\x7fELF\x02") {
        return Err("Binaires ELF x86_64 attendus".into());
    }
    let existing = Path::new(FILES[2]).exists();
    let backups = FILES
        .iter()
        .copied()
        .chain([OWNER])
        .map(|path| match fs::symlink_metadata(path) {
            Ok(m) if m.is_file() && m.uid() == 0 => {
                Ok((path, Some((payload(Path::new(path))?, m.mode() & 0o777))))
            }
            Ok(_) => Err(format!("Destination système non sûre : {path}")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((path, None)),
            Err(e) => Err(e.to_string()),
        })
        .collect::<Result<Vec<_>>>()?;
    if existing {
        systemctl(&[
            "stop",
            "gameshift-control.socket",
            "gameshift-control.service",
        ])?;
    }
    let installed = (|| -> Result<()> {
        write_fixed(FILES[0], &helper, 0o755)?;
        write_fixed(FILES[1], &agent, 0o755)?;
        write_fixed(
            FILES[2],
            include_bytes!("../../../packaging/control/gameshift-control.socket"),
            0o644,
        )?;
        write_fixed(
            FILES[3],
            include_bytes!("../../../packaging/control/gameshift-control.service"),
            0o644,
        )?;
        write_fixed(
            FILES[4],
            include_bytes!("../../../packaging/control/gameshift-profiles.service"),
            0o644,
        )?;
        write_fixed(
            FILES[5],
            include_bytes!("../../../packaging/control/io.github.gameshift.control.policy"),
            0o644,
        )?;
        write_fixed(OWNER, uid.to_string().as_bytes(), 0o600)?;
        systemctl(&["daemon-reload"])?;
        if Path::new("/usr/sbin/restorecon").exists() {
            let _ = std::process::Command::new("/usr/sbin/restorecon")
                .args(FILES)
                .status();
        }
        systemctl(&["enable", "--now", "gameshift-control.socket"])?;
        Ok(())
    })();
    if let Err(error) = installed {
        let mut failures = vec![];
        for (path, backup) in backups {
            let restored = match backup {
                Some((bytes, mode)) => write_fixed(path, &bytes, mode),
                None => match fs::remove_file(path) {
                    Ok(()) => Ok(()),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(e) => Err(e.to_string()),
                },
            };
            if let Err(e) = restored {
                failures.push(e);
            }
        }
        if let Err(e) = systemctl(&["daemon-reload"]) {
            failures.push(e);
        }
        if existing {
            if let Err(e) = systemctl(&["start", "gameshift-control.socket"]) {
                failures.push(e);
            }
        }
        return Err(format!(
            "Installation interrompue : {error}. Retour arrière : {}",
            if failures.is_empty() {
                "effectué".into()
            } else {
                failures.join(" ; ")
            }
        ));
    }
    println!("Contrôles GPU activés. Aucun profil appliqué par l’installation.");
    Ok(())
}
pub fn uninstall() -> Result<()> {
    if std::env::args().count() != 2 {
        return Err("Paramètres de désinstallation inconnus".into());
    }
    caller()?;
    let _installation = installation_lock()?;
    systemctl(&["disable", "--now", "gameshift-control.socket"])?;
    systemctl(&["stop", "gameshift-control.service"])?;
    // Only our fixed installed files. Personal profiles and recovery/audit are preserved.
    for path in FILES.iter().copied().chain([OWNER]) {
        match fs::remove_file(path) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.to_string()),
        }
    }
    systemctl(&["daemon-reload"])?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installation_has_only_fixed_system_destinations_and_exact_modes() {
        assert!(FILES
            .iter()
            .all(|p| p.starts_with("/usr/") && !p.contains("..")));
        let root =
            std::env::temp_dir().join(format!("gameshift-install-test-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("component");
        write_fixed(path.to_str().unwrap(), b"payload", 0o755).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o755);
        assert_eq!(fs::read(&path).unwrap(), b"payload");
        std::os::unix::fs::symlink(&path, root.join("link")).unwrap();
        assert!(payload(&root.join("link")).is_err());
        assert!(write_fixed(root.join("link").to_str().unwrap(), b"bad", 0o755).is_err());
        fs::remove_dir_all(root).unwrap();
        assert!(
            include_str!("../../../packaging/control/gameshift-control.service")
                .contains("ExecStart=/usr/libexec/gameshift-control --daemon")
        );
        assert!(
            include_str!("../../../packaging/control/gameshift-profiles.service")
                .contains("ExecStart=/usr/libexec/gameshift-profiles daemon")
        );
    }
}
