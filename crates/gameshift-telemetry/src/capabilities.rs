//! Bounded AMDGPU inventory. Unknown/debug action files are NEVER opened.
//! File permissions describe metadata, not a promise that a hardware operation is safe.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    os::unix::fs::PermissionsExt,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
const MAX_FILES: usize = 512;
const MAX_TEXT: u64 = 4096;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Observation,
    #[serde(alias = "control")]
    Standard,
    Advanced,
    #[serde(alias = "experimental")]
    Critical,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadPolicy {
    PassiveText,
    MetadataOnly,
    Forbidden,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Range {
    pub minimum: u64,
    pub maximum: u64,
    pub unit: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReadState {
    #[default]
    NotRead,
    Readable,
    Denied,
    Unavailable,
    Unsupported,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WriteState {
    NotApplicable,
    Allowed,
    NotImplemented,
    Unavailable,
    Unsupported,
    Forbidden,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    pub id: String,
    pub source: String,
    pub category: Category,
    pub write_state: WriteState,
    pub read_policy: ReadPolicy,
    pub description: String,
    pub value: Option<String>,
    pub read_error: Option<String>,
    pub present: Option<bool>,
    pub read_access: Option<bool>,
    #[serde(default)]
    pub read_state: ReadState,
    pub permission_write_bits: Option<bool>,
    pub range: Option<Range>,
    pub observed_choices: Vec<String>,
    pub conditions: String,
    pub risk: String,
    pub restoration: String,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Inventory {
    pub schema_version: u32,
    pub pci_address: String,
    pub driver: String,
    pub captured_unix_millis: Option<u128>,
    pub capabilities: Vec<Capability>,
    pub warnings: Vec<String>,
}
fn io_error(error: std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        format!("permission_denied: {error}")
    } else {
        error.to_string()
    }
}
fn text(path: &Path) -> Result<String, String> {
    if !fs::symlink_metadata(path).map_err(io_error)?.is_file() {
        return Err("Objet non régulier : lecture refusée".into());
    }
    let file = fs::File::open(path).map_err(io_error)?;
    let mut bytes = Vec::new();
    file.take(MAX_TEXT + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 > MAX_TEXT {
        return Err("Lecture limitée à 4 Kio ; contenu non affiché".into());
    }
    String::from_utf8(bytes)
        .map(|s| s.trim().to_owned())
        .map_err(|_| "Contenu non textuel ; aucun décodage automatique".into())
}
fn numbered(name: &str, prefix: &str, suffix: &str) -> bool {
    name.strip_prefix(prefix)
        .and_then(|s| s.strip_suffix(suffix))
        .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
}
fn classify(name: &str, debug: bool, hwmon: bool) -> (Category, ReadPolicy, &'static str) {
    use Category::*;
    use ReadPolicy::*;
    if name.contains("recover")
        || name.contains("reset")
        || name.contains("evict")
        || name.starts_with("amdgpu_regs")
        || name.starts_with("amdgpu_error")
        || matches!(
            name,
            "amdgpu_test_ib" | "amdgpu_benchmark" | "amdgpu_vram" | "amdgpu_iomem"
        )
    {
        return (
            Critical,
            Forbidden,
            "Interface à effet de bord ou accès bas niveau : contenu jamais ouvert par GameShift",
        );
    }
    if debug {
        return match name {
            "amdgpu_pm_info" | "amdgpu_firmware_info" | "amdgpu_fence_info" => (
                Observation,
                PassiveText,
                "Diagnostic du pilote, lecture explicite et bornée",
            ),
            _ => (
                Critical,
                MetadataOnly,
                "Interface debugfs découverte ; sémantique de lecture non validée",
            ),
        };
    }
    match name {
        "power_dpm_force_performance_level"
        | "pp_power_profile_mode"
        | "soc_pstate"
        | "xgmi_plpd" => return (Standard, PassiveText, "Politique énergétique du pilote"),
        "pp_od_clk_voltage" => {
            return (
                Advanced,
                PassiveText,
                "OverDrive : fréquences/tensions et plages dépendantes du GPU et du firmware",
            )
        }
        "pp_features" => {
            return (
                Critical,
                PassiveText,
                "État des fonctionnalités SMU ; modification du masque non proposée",
            )
        }
        "gpu_metrics" => {
            return (
                Observation,
                MetadataOnly,
                "Table binaire versionnée ; décodage non activé pour cette interface",
            )
        }
        "fan_curve"
        | "acoustic_limit_rpm_threshold"
        | "acoustic_target_rpm_threshold"
        | "fan_target_temperature"
        | "fan_minimum_pwm"
        | "fan_zero_rpm_enable"
        | "fan_zero_rpm_stop_temperature" => {
            return (
                Standard,
                PassiveText,
                "Ventilation PMFW : contrôle selon les limites et formats exposés par le pilote",
            )
        }
        "gpu_busy_percent" | "mem_busy_percent" | "power_dpm_state" | "pcie_bw"
        | "pcie_replay_count" | "current_link_speed" | "current_link_width" | "max_link_speed"
        | "max_link_width" | "vbios_version" => {
            return (
                Observation,
                PassiveText,
                "État matériel exposé par le pilote ou PCI ; aucune modification",
            )
        }
        _ => (),
    }
    if matches!(
        name,
        "pp_dpm_sclk"
            | "pp_dpm_mclk"
            | "pp_dpm_socclk"
            | "pp_dpm_fclk"
            | "pp_dpm_dcefclk"
            | "pp_dpm_pcie"
    ) {
        return (
            Advanced,
            PassiveText,
            "États de fréquence exposés ; modification conditionnée au mode manuel et au GPU",
        );
    }
    if matches!(
        name,
        "mem_info_vram_total"
            | "mem_info_vram_used"
            | "mem_info_vis_vram_total"
            | "mem_info_vis_vram_used"
            | "mem_info_gtt_total"
            | "mem_info_gtt_used"
    ) {
        return (
            Observation,
            PassiveText,
            "Comptabilité mémoire du périphérique",
        );
    }
    if hwmon {
        if numbered(name, "power", "_cap")
            || numbered(name, "pwm", "")
            || numbered(name, "pwm", "_enable")
            || numbered(name, "fan", "_target")
        {
            return (
                Standard,
                PassiveText,
                "Réglage hwmon observé ; droits d’écriture distincts de sa sûreté",
            );
        }
        for prefix in ["temp", "freq", "power", "fan", "in"] {
            for suffix in [
                "_input",
                "_average",
                "_label",
                "_min",
                "_max",
                "_crit",
                "_emergency",
                "_cap_min",
                "_cap_max",
            ] {
                if numbered(name, prefix, suffix) {
                    return (
                        Observation,
                        PassiveText,
                        "Capteur ou limite hwmon exposé par le pilote",
                    );
                }
            }
        }
    }
    (
        Critical,
        MetadataOnly,
        "Interface découverte ; aucun lecteur validé, contenu non ouvert",
    )
}
fn inspect(
    path: &Path,
    id: String,
    debug: bool,
    hwmon: bool,
    reader: &impl Fn(&Path) -> Result<String, String>,
) -> Capability {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let (category, mut policy, description) = classify(name, debug, hwmon);
    let metadata = fs::symlink_metadata(path);
    let present = match &metadata {
        Ok(_) => Some(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(false),
        Err(_) => None,
    };
    let risk = if policy == ReadPolicy::Forbidden {
        "Lecture et écriture interdites dans GameShift ; peut affecter le GPU ou la session graphique."
    } else {
        "La présence de cette interface ne garantit pas une écriture autorisée ; consulter le niveau de contrôle et les limites."
    };
    let mut result = Capability {
        id,
        source: path.display().to_string(),
        category,
        write_state: WriteState::NotApplicable,
        read_policy: policy,
        description: description.into(),
        value: None,
        read_error: None,
        present,
        read_access: None,
        read_state:if present==Some(false){ReadState::Unavailable}else{ReadState::NotRead},
        permission_write_bits: metadata.as_ref().ok().map(|m| m.permissions().mode() & 0o222 != 0),
        range: None,
        observed_choices: vec![],
        conditions: "La présence et les bits de permission ne prouvent ni l’autorisation effective ni la compatibilité d’une écriture.".into(),
        risk: risk.into(),
        restoration: "Sans objet en lecture seule. La restauration doit être définie avant activation d’un contrôle.".into(),
    };
    match metadata {
        Ok(m) if !m.is_file() => {
            policy = if policy == ReadPolicy::Forbidden {
                policy
            } else {
                ReadPolicy::MetadataOnly
            };
            result.read_error = Some("Lien ou objet non régulier : contenu non ouvert".into());
        }
        Err(e) => {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                result.read_state = ReadState::Denied;
            }
            result.read_error = Some(e.to_string());
            if policy != ReadPolicy::Forbidden {
                policy = ReadPolicy::MetadataOnly;
            }
        }
        _ => (),
    }
    result.read_policy = policy;
    if policy == ReadPolicy::PassiveText {
        match reader(path) {
            Ok(value) => {
                result.read_access = Some(true);
                result.read_state = ReadState::Readable;
                result.value = Some(value);
            }
            Err(error) => {
                result.read_access = Some(false);
                result.read_state = if error.starts_with("permission_denied:") {
                    ReadState::Denied
                } else {
                    ReadState::Unsupported
                };
                result.read_error = Some(error);
            }
        }
        if name == "pp_power_profile_mode" {
            if let Some(value) = &result.value {
                result.observed_choices = value
                    .lines()
                    .filter(|l| {
                        l.split_whitespace()
                            .next()
                            .is_some_and(|v| v.parse::<u32>().is_ok())
                    })
                    .map(str::to_owned)
                    .collect();
            }
        }
        if hwmon && numbered(name, "power", "_cap") {
            let parent = path.parent().unwrap();
            let read_number = |suffix: &str| {
                reader(&parent.join(format!("{name}{suffix}")))
                    .ok()?
                    .trim()
                    .parse::<u64>()
                    .ok()
            };
            if let (Some(minimum), Some(maximum)) = (read_number("_min"), read_number("_max")) {
                if minimum <= maximum {
                    result.range = Some(Range {
                        minimum,
                        maximum,
                        unit: "µW".into(),
                    });
                }
            }
        }
    }
    result.write_state = if result.present == Some(false) {
        WriteState::Unavailable
    } else if result.read_policy == ReadPolicy::Forbidden {
        WriteState::Forbidden
    } else if let Some(raw) = result.value.as_deref() {
        use gameshift_control::{
            parse_curve, parse_fan, parse_overdrive, parse_workloads, FanSetting, PERFORMANCE_MODES,
        };
        let fan = FanSetting::ALL
            .into_iter()
            .find(|s| s.definition().0 == name && result.id == s.path());
        let supported = if let Some(setting) = fan {
            result.category = Category::Standard;
            parse_fan(raw, setting).is_ok_and(|(_, range)| range.is_some())
        } else {
            match name {
                "fan_curve" => {
                    result.category = Category::Standard;
                    parse_curve(raw)
                        .is_ok_and(|c| c.temperature.is_some() && c.pwm_percent.is_some())
                }
                "power_dpm_force_performance_level" => {
                    result.category = Category::Standard;
                    PERFORMANCE_MODES.contains(&raw.trim())
                }
                "pp_power_profile_mode" => {
                    result.category = Category::Standard;
                    parse_workloads(raw).iter().filter(|p| p.active).count() == 1
                }
                "pp_od_clk_voltage" => {
                    result.category = Category::Advanced;
                    parse_overdrive(raw).is_ok_and(|(clocks, voltage)| {
                        clocks.iter().any(|c| c.limits.is_some()) || voltage.is_some()
                    })
                }
                _ => result.range.is_some() && numbered(name, "power", "_cap"),
            }
        };
        if supported && result.permission_write_bits == Some(true) {
            WriteState::Allowed
        } else if result.category == Category::Standard || result.category == Category::Advanced {
            WriteState::NotImplemented
        } else if result.category == Category::Critical {
            WriteState::Forbidden
        } else {
            WriteState::NotApplicable
        }
    } else if result.category == Category::Critical {
        WriteState::Forbidden
    } else {
        WriteState::Unsupported
    };
    if matches!(result.write_state, WriteState::Allowed) {
        result.conditions="Écriture via helper installé et autorisation Polkit. Identité, limites et dépendances revérifiées avant application ; état relu après écriture.".into();
        result.risk="Le risque dépend du réglage ; fréquences/tensions nécessitent une confirmation avancée. Aucun accès de reset GPU ou de registre n’est autorisé.".into();
        result.restoration="Valeurs lues sauvegardées par le helper ; récupération indépendante de l’interface. Le mode de récupération de ventilation est explicite, le mode PMFW n’étant pas exposé fiablement.".into();
    }
    result
}
impl Inventory {
    pub fn discover(pci: &str, include_debugfs: bool) -> Result<Self, String> {
        Self::discover_at(Path::new("/"), pci, include_debugfs)
    }
    pub fn discover_at(root: &Path, pci: &str, include_debugfs: bool) -> Result<Self, String> {
        let pci = super::canonical_address(pci).ok_or("Adresse PCI invalide")?;
        let base = root.join("sys/bus/pci/devices").join(&pci);
        let driver = fs::read_link(base.join("driver")).map_err(|e| e.to_string())?;
        if driver.file_name().is_none_or(|name| name != "amdgpu") {
            return Err("Ce périphérique n’est pas lié au pilote amdgpu".into());
        }
        let mut result = Self {
            schema_version: 2,
            pci_address: pci.clone(),
            driver: "amdgpu".into(),
            captured_unix_millis: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .map(|d| d.as_millis()),
            capabilities: vec![],
            warnings: vec![],
        };
        result.scan(&base, "pci", false, false, 0);
        for name in [
            "power_dpm_force_performance_level",
            "pp_power_profile_mode",
            "pp_od_clk_voltage",
            "pp_features",
            "gpu_metrics",
            "pp_dpm_sclk",
            "pp_dpm_mclk",
        ] {
            if result.capabilities.len() < MAX_FILES
                && !result
                    .capabilities
                    .iter()
                    .any(|c| c.id == format!("pci/{name}"))
            {
                result.capabilities.push(inspect(
                    &base.join(name),
                    format!("pci/{name}"),
                    false,
                    false,
                    &text,
                ));
            }
        }
        // hwmon directories are discovered under the owning PCI device, not globally.
        if let Ok(entries) = fs::read_dir(base.join("hwmon")) {
            for entry in entries.flatten() {
                if text(&entry.path().join("name")).is_ok_and(|n| n == "amdgpu") {
                    result.scan(
                        &entry.path(),
                        &format!("hwmon/{}", entry.file_name().to_string_lossy()),
                        false,
                        true,
                        0,
                    );
                }
            }
        }
        for directory in ["gpu_od", "pm_policy"] {
            let path = base.join(directory);
            if path.is_dir() {
                result.scan(&path, directory, false, false, 2);
            }
        }
        if include_debugfs {
            let mut visited = std::collections::BTreeSet::new();
            match fs::read_dir(root.join("sys/kernel/debug/dri")) {
                Err(error) => result
                    .warnings
                    .push(format!("debugfs indisponible : {error}")),
                Ok(entries) => {
                    for entry in entries.flatten() {
                        // DRM's documented name file associates a minor with its PCI device.
                        let path = entry.path();
                        if let Ok(canonical) = fs::canonicalize(&path) {
                            if !visited.insert(canonical) {
                                continue;
                            }
                        }
                        if text(&path.join("name")).is_ok_and(|n| {
                            n.split_whitespace()
                                .any(|part| part.strip_prefix("dev=") == Some(pci.as_str()))
                        }) {
                            result.scan(
                                &path,
                                &format!("debugfs/{}", entry.file_name().to_string_lossy()),
                                true,
                                false,
                                0,
                            );
                        }
                    }
                }
            }
        }
        result.capabilities.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(result)
    }
    fn scan(&mut self, directory: &Path, prefix: &str, debug: bool, hwmon: bool, depth: usize) {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) => {
                self.warnings
                    .push(format!("{} : {error}", directory.display()));
                return;
            }
        };
        let mut paths = Vec::new();
        for entry in entries {
            match entry {
                Ok(entry) => paths.push(entry.path()),
                Err(error) => self.warnings.push(error.to_string()),
            }
            if paths.len() >= MAX_FILES {
                self.warnings
                    .push("Inventaire du dossier limité à 512 entrées".into());
                break;
            }
        }
        paths.sort();
        for path in paths {
            if self.capabilities.len() >= MAX_FILES {
                if !self
                    .warnings
                    .iter()
                    .any(|w| w == "Inventaire limité à 512 interfaces")
                {
                    self.warnings
                        .push("Inventaire limité à 512 interfaces".into());
                }
                return;
            }
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if matches!(name.as_ref(), "driver" | "hwmon" | "gpu_od" | "pm_policy") {
                continue;
            }
            let nested = fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir());
            if nested && depth > 0 {
                self.scan(&path, &format!("{prefix}/{name}"), debug, hwmon, depth - 1);
            } else if !nested {
                self.capabilities.push(inspect(
                    &path,
                    format!("{prefix}/{name}"),
                    debug,
                    hwmon,
                    &text,
                ));
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "gameshift-capabilities-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self(root)
        }
        fn device(&self, pci: &str) -> PathBuf {
            let path = self.0.join("sys/bus/pci/devices").join(pci);
            fs::create_dir_all(&path).unwrap();
            std::os::unix::fs::symlink("/sys/bus/pci/drivers/amdgpu", path.join("driver")).unwrap();
            path
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn dangerous_and_unknown_files_are_not_read_even_if_writable() {
        let fixture = Fixture::new();
        for name in [
            "amdgpu_gpu_recover",
            "amdgpu_evict_vram",
            "amdgpu_test_ib",
            "amdgpu_regs",
            "unknown_file",
        ] {
            let path = fixture.0.join(name);
            fs::write(&path, "NEVER READ").unwrap();
            let capability = inspect(&path, name.into(), true, false, &|_| {
                panic!("Forbidden read")
            });
            assert!(capability.value.is_none());
            assert_ne!(capability.read_policy, ReadPolicy::PassiveText);
        }
    }
    #[test]
    fn discovers_ranges_and_keeps_pci_and_debugfs_devices_separate() {
        let fixture = Fixture::new();
        let device = fixture.device("0000:09:00.0");
        fs::write(device.join("pp_od_clk_voltage"), "OD_RANGE: fixture").unwrap();
        let hwmon = device.join("hwmon/hwmon73");
        fs::create_dir_all(&hwmon).unwrap();
        for (file, value) in [
            ("name", "amdgpu"),
            ("power3_cap", "100000000"),
            ("power3_cap_min", "50000000"),
            ("power3_cap_max", "200000000"),
        ] {
            fs::write(hwmon.join(file), value).unwrap();
        }
        for (id, pci) in [("8", "0000:09:00.0"), ("9", "0000:0a:00.0")] {
            let dir = fixture.0.join("sys/kernel/debug/dri").join(id);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("name"), format!("amdgpu dev={pci}")).unwrap();
            fs::write(dir.join("amdgpu_pm_info"), pci).unwrap();
            fs::write(dir.join("amdgpu_gpu_recover"), "never").unwrap();
        }
        let result = Inventory::discover_at(&fixture.0, "0000:09:00.0", true).unwrap();
        let cap = result
            .capabilities
            .iter()
            .find(|c| c.id.ends_with("power3_cap"))
            .unwrap();
        assert_eq!(cap.range.as_ref().unwrap().maximum, 200000000);
        assert!(result
            .capabilities
            .iter()
            .any(|c| c.value.as_deref() == Some("0000:09:00.0")));
        assert!(!result
            .capabilities
            .iter()
            .any(|c| c.value.as_deref() == Some("0000:0a:00.0")));
        assert!(result
            .capabilities
            .iter()
            .find(|c| c.id.ends_with("amdgpu_gpu_recover"))
            .unwrap()
            .value
            .is_none());
        assert!(Inventory::discover_at(&fixture.0, "../etc", false).is_err());
        assert_eq!(
            result
                .capabilities
                .iter()
                .find(|c| c.id == "pci/pp_features")
                .unwrap()
                .present,
            Some(false)
        );
    }
    #[test]
    fn recognized_pmfw_controls_are_standard_and_unknown_files_stay_unread() {
        let fixture = Fixture::new();
        let device = fixture.device("0000:0b:00.0");
        let fan = device.join("gpu_od/fan_ctrl");
        fs::create_dir_all(&fan).unwrap();
        fs::write(
            fan.join("fan_zero_rpm_enable"),
            "FAN_ZERO_RPM_ENABLE:\n1\nOD_RANGE:\nZERO_RPM_ENABLE: 0 1\n",
        )
        .unwrap();
        fs::write(device.join("pp_dpm_mclk"), "0: 96Mhz *\n1: 1200Mhz\n").unwrap();
        fs::write(fan.join("undocumented_switch"), "DO NOT OPEN").unwrap();
        let result = Inventory::discover_at(&fixture.0, "0000:0b:00.0", false).unwrap();
        let zero = result
            .capabilities
            .iter()
            .find(|c| c.id == "gpu_od/fan_ctrl/fan_zero_rpm_enable")
            .unwrap();
        assert_eq!(zero.category, Category::Standard);
        assert!(matches!(zero.write_state, WriteState::Allowed));
        assert_eq!(zero.read_state, ReadState::Readable);
        let dpm = result
            .capabilities
            .iter()
            .find(|c| c.id == "pci/pp_dpm_mclk")
            .unwrap();
        assert_eq!(dpm.category, Category::Advanced);
        assert!(matches!(dpm.write_state, WriteState::NotImplemented));
        let unknown = result
            .capabilities
            .iter()
            .find(|c| c.id.ends_with("undocumented_switch"))
            .unwrap();
        assert!(unknown.value.is_none());
        assert_eq!(unknown.read_state, ReadState::NotRead);
        assert!(matches!(unknown.write_state, WriteState::Forbidden));
    }
    #[test]
    fn symlinks_and_oversized_text_are_not_silently_accepted() {
        let fixture = Fixture::new();
        let base = fixture.device("0000:01:00.0");
        let target = base.join("unknown");
        fs::write(&target, "target").unwrap();
        std::os::unix::fs::symlink(&target, base.join("pp_features")).unwrap();
        fs::write(base.join("pp_od_clk_voltage"), vec![b'x'; 5000]).unwrap();
        let result = Inventory::discover_at(&fixture.0, "0000:01:00.0", false).unwrap();
        assert!(result
            .capabilities
            .iter()
            .find(|c| c.id == "pci/pp_features")
            .unwrap()
            .value
            .is_none());
        assert!(result
            .capabilities
            .iter()
            .find(|c| c.id == "pci/pp_od_clk_voltage")
            .unwrap()
            .read_error
            .is_some());
    }
}
