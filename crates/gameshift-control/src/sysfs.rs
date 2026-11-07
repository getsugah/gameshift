use crate::*;
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
};
pub struct Sysfs {
    base: PathBuf,
    identity: Identity,
}
fn text(path: &std::path::Path) -> Result<String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() {
        return Err("Attribut non régulier refusé".into());
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| e.to_string())?;
    let mut data = Vec::new();
    file.take(32769)
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    if data.len() > 32768 {
        return Err("Attribut trop volumineux".into());
    }
    String::from_utf8(data)
        .map(|s| s.trim().to_owned())
        .map_err(|e| e.to_string())
}
pub fn canonical_pci(value: &str) -> Result<String> {
    let (domain, tail) = value.split_once(':').ok_or("Adresse PCI invalide")?;
    let (bus, tail) = tail.split_once(':').ok_or("Adresse PCI invalide")?;
    let (slot, function) = tail.split_once('.').ok_or("Adresse PCI invalide")?;
    if domain.len() != 4 || bus.len() != 2 || slot.len() != 2 || function.len() != 1 {
        return Err("Adresse PCI non canonique".into());
    }
    let parse = |s: &str| u16::from_str_radix(s, 16).map_err(|_| "Adresse PCI invalide".to_owned());
    let (domain, bus, slot, function) =
        (parse(domain)?, parse(bus)?, parse(slot)?, parse(function)?);
    if slot > 31 || function > 7 {
        return Err("Adresse PCI hors limites".into());
    }
    Ok(format!("{domain:04x}:{bus:02x}:{slot:02x}.{function:x}"))
}
impl Sysfs {
    pub fn open(pci: &str) -> Result<Self> {
        let pci = canonical_pci(pci)?;
        let base = fs::canonicalize(PathBuf::from("/sys/bus/pci/devices").join(&pci))
            .map_err(|e| e.to_string())?;
        if !base.starts_with("/sys/devices") {
            return Err("Racine PCI inattendue".into());
        }
        let read = |name: &str| text(&base.join(name));
        if fs::read_link(base.join("driver"))
            .map_err(|e| e.to_string())?
            .file_name()
            .is_none_or(|n| n != "amdgpu")
            || read("vendor")? != "0x1002"
            || !read("class")?.starts_with("0x03")
        {
            return Err("Périphérique Radeon AMDGPU non reconnu".into());
        }
        let identity = Identity {
            pci,
            vendor: read("vendor")?,
            device: read("device")?,
            revision: read("revision")?,
            subsystem_vendor: read("subsystem_vendor")?,
            subsystem_device: read("subsystem_device")?,
            unique_id: read("unique_id").ok(),
        };
        Ok(Self { base, identity })
    }
    fn check(&self) -> Result<()> {
        let current = Self::open(&self.identity.pci)?;
        if current.identity != self.identity || current.base != self.base {
            return Err("Identité du GPU modifiée : opération refusée".into());
        }
        Ok(())
    }
    fn allowed(&self, path: &str) -> bool {
        if [
            "power_dpm_force_performance_level",
            "pp_power_profile_mode",
            "pp_od_clk_voltage",
            "gpu_od/fan_ctrl/fan_curve",
        ]
        .contains(&path)
            || FanSetting::ALL.iter().any(|s| s.path() == path)
        {
            return true;
        }
        self.power_paths()
            .is_ok_and(|paths| paths.iter().any(|(_, candidate)| candidate == path))
    }
}
impl DeviceIo for Sysfs {
    fn identity(&self) -> Result<Identity> {
        self.check()?;
        Ok(self.identity.clone())
    }
    fn read(&self, path: &str) -> Result<String> {
        if path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
            || path.starts_with('/')
        {
            return Err("Chemin refusé".into());
        }
        text(&self.base.join(path))
    }
    fn write(&mut self, path: &str, value: &str) -> Result<()> {
        self.check()?;
        if !self.allowed(path) {
            return Err("Destination non autorisée".into());
        }
        let path = self.base.join(path);
        if !fs::symlink_metadata(&path)
            .map_err(|e| e.to_string())?
            .is_file()
        {
            return Err("Attribut non régulier refusé".into());
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .map_err(|e| e.to_string())?;
        let count = file.write(value.as_bytes()).map_err(|e| e.to_string())?;
        if count != value.len() {
            return Err("Écriture sysfs incomplète".into());
        }
        Ok(())
    }
    fn power_paths(&self) -> Result<Vec<(u32, String)>> {
        let mut paths = Vec::new();
        let entries = fs::read_dir(self.base.join("hwmon")).map_err(|e| e.to_string())?;
        for entry in entries.take(128) {
            let entry = entry.map_err(|e| e.to_string())?;
            if text(&entry.path().join("name")).ok().as_deref() != Some("amdgpu") {
                continue;
            }
            for file in fs::read_dir(entry.path())
                .map_err(|e| e.to_string())?
                .take(256)
            {
                let file = file.map_err(|e| e.to_string())?;
                let name = file.file_name().to_string_lossy().into_owned();
                if let Some(channel) = name
                    .strip_prefix("power")
                    .and_then(|s| s.strip_suffix("_cap"))
                    .and_then(|s| s.parse::<u32>().ok())
                {
                    if paths.iter().any(|(id, _)| *id == channel) {
                        return Err("Canal de puissance ambigu entre plusieurs hwmon".into());
                    }
                    let path = file.path();
                    let canonical = fs::canonicalize(&path).map_err(|e| e.to_string())?;
                    if !canonical.starts_with(&self.base) {
                        return Err("Capteur de puissance hors du périphérique PCI".into());
                    }
                    paths.push((
                        channel,
                        path.strip_prefix(&self.base)
                            .map_err(|e| e.to_string())?
                            .to_string_lossy()
                            .into_owned(),
                    ));
                }
            }
        }
        paths.sort();
        Ok(paths)
    }
}

fn scan_thermal(base: &std::path::Path) -> Result<Option<bool>> {
    let entries = match fs::read_dir(base.join("hwmon")) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let mut available = false;
    for entry in entries.take(128) {
        let entry = entry.map_err(|e| e.to_string())?;
        if text(&entry.path().join("name")).ok().as_deref() != Some("amdgpu") {
            continue;
        }
        for file in fs::read_dir(entry.path())
            .map_err(|e| e.to_string())?
            .take(256)
        {
            let file = file.map_err(|e| e.to_string())?;
            let name = file.file_name().to_string_lossy().into_owned();
            let Some(index) = name
                .strip_prefix("temp")
                .and_then(|s| s.strip_suffix("_input"))
            else {
                continue;
            };
            if index.parse::<u32>().is_err() {
                continue;
            }
            let temperature = number(&text(&file.path())?)?;
            let critical = ["crit", "emergency"].into_iter().find_map(|suffix| {
                text(&entry.path().join(format!("temp{index}_{suffix}")))
                    .ok()
                    .and_then(|v| number(&v).ok())
                    .filter(|v| *v > 0)
            });
            if let Some(critical) = critical {
                available = true;
                if temperature >= critical {
                    return Ok(Some(true));
                }
            }
        }
    }
    Ok(available.then_some(false))
}
impl Sysfs {
    /// A safety watchdog uses driver-exposed millidegree limits, never an invented temperature.
    pub fn thermal_alarm(&self) -> Result<Option<bool>> {
        self.check()?;
        scan_thermal(&self.base)
    }
}
#[cfg(test)]
mod thermal_tests {
    use super::*;
    #[test]
    fn thermal_guard_distinguishes_missing_limits_and_real_critical_values() {
        let root = std::env::temp_dir().join(format!("gameshift-thermal-{}", std::process::id()));
        let monitor = root.join("hwmon/hwmon73");
        fs::create_dir_all(&monitor).unwrap();
        fs::write(monitor.join("name"), "amdgpu").unwrap();
        fs::write(monitor.join("temp3_input"), "95000").unwrap();
        assert_eq!(scan_thermal(&root).unwrap(), None);
        fs::write(monitor.join("temp3_crit"), "95000").unwrap();
        assert_eq!(scan_thermal(&root).unwrap(), Some(true));
        fs::write(monitor.join("temp3_input"), "94000").unwrap();
        assert_eq!(scan_thermal(&root).unwrap(), Some(false));
        fs::remove_dir_all(root).unwrap();
    }
}
