//! Linux diagnostics using read-only procfs and sysfs access.
mod gpu_names;

use gameshift_core::{Reading, Section, Snapshot};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

type CachedRead = Result<String, (std::io::ErrorKind, String)>;

/// Injectable filesystem root for offline fixtures and restricted environments.
pub struct Collector {
    root: PathBuf,
    reads: RefCell<BTreeMap<PathBuf, CachedRead>>,
}

impl Default for Collector {
    fn default() -> Self {
        Self::new("/")
    }
}

impl Collector {
    /// Create a collector rooted at a Linux filesystem or a test fixture.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            reads: RefCell::new(BTreeMap::new()),
        }
    }
    fn path(&self, path: &str) -> PathBuf {
        self.root.join(path.trim_start_matches('/'))
    }
    fn read(&self, path: &str) -> Result<String, std::io::Error> {
        let requested = self.path(path);
        let key = fs::canonicalize(&requested).unwrap_or_else(|_| requested.clone());
        let mut reads = self.reads.borrow_mut();
        let result = reads.entry(key).or_insert_with(|| {
            fs::read_to_string(requested)
                .map(|s| s.trim().to_owned())
                .map_err(|e| (e.kind(), e.to_string()))
        });
        result
            .clone()
            .map_err(|(kind, message)| std::io::Error::new(kind, message))
    }
    fn reading(&self, label: &str, path: &str) -> Reading {
        match self.read(path) {
            Ok(value) if !value.is_empty() => Reading::available(label, path, value),
            Ok(_) => Reading::unavailable(label, path, "Source vide"),
            Err(error) => Reading::unavailable(label, path, error.to_string()),
        }
    }
    fn field(&self, label: &str, path: &str, key: &str) -> Reading {
        match self.read(path) {
            Ok(text) => match text
                .lines()
                .filter_map(|s| s.split_once(':'))
                .find(|(k, _)| k.trim() == key)
            {
                Some((_, value)) => Reading::available(label, path, value.trim()),
                None => Reading::unavailable(label, path, format!("Champ {key} absent")),
            },
            Err(error) => Reading::unavailable(label, path, error.to_string()),
        }
    }
    fn entries(&self, path: &str) -> Result<Vec<String>, std::io::Error> {
        let mut entries = fs::read_dir(self.path(path))?
            .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect::<Result<Vec<_>, _>>()?;
        entries.sort();
        Ok(entries)
    }
    fn database(&self, paths: &[&str]) -> Option<(String, String)> {
        paths
            .iter()
            .find_map(|path| self.read(path).ok().map(|text| ((*path).into(), text)))
    }

    fn gpu_identity(
        &self,
        base: &str,
        entry: &str,
        pci: Option<&(String, String)>,
        amd: Option<&(String, String)>,
        readings: &mut Vec<Reading>,
    ) {
        let id = |file: &str| {
            self.read(&format!("{base}/{file}"))
                .ok()
                .and_then(|s| gpu_names::hex(&s))
        };
        let (Some(vendor), Some(device)) = (id("vendor"), id("device")) else {
            readings.push(Reading::unavailable(
                &format!("{entry} — Modèle graphique"),
                base,
                "Identifiants matériels inaccessibles",
            ));
            return;
        };
        let ids = gpu_names::Ids {
            vendor,
            device,
            revision: id("revision").and_then(|n| u8::try_from(n).ok()),
            subsystem: id("subsystem_vendor").zip(id("subsystem_device")),
        };
        let names = pci
            .map(|(_, text)| gpu_names::pci_names(text, &ids))
            .unwrap_or_default();
        let model = amd
            .and_then(|(path, text)| {
                gpu_names::amd_name(text, &ids).map(|name| (path.as_str(), name.to_owned()))
            })
            .or_else(|| {
                pci.and_then(|(path, _)| {
                    names
                        .subsystem
                        .as_ref()
                        .or(names.device.as_ref())
                        .map(|name| (path.as_str(), name.clone()))
                })
            });
        readings.push(match model {
            Some((source, model)) => {
                Reading::available(&format!("{entry} — Modèle graphique"), source, model)
            }
            None => Reading::unavailable(
                &format!("{entry} — Modèle graphique"),
                "Bases matérielles locales",
                "Modèle inconnu ou bases locales indisponibles",
            ),
        });
        if let Some((source, text)) = pci {
            if let Some(vendor) = names.vendor {
                readings.push(Reading::available(
                    &format!("{entry} — Nom du constructeur"),
                    source,
                    vendor,
                ));
            }
            if let Some((board_vendor, _)) = ids.subsystem {
                let board_ids = gpu_names::Ids {
                    vendor: board_vendor,
                    device: 0,
                    revision: None,
                    subsystem: None,
                };
                if let Some(name) = gpu_names::pci_names(text, &board_ids).vendor {
                    readings.push(Reading::available(
                        &format!("{entry} — Fabricant de la carte"),
                        source,
                        name,
                    ));
                }
            }
        }
        readings.push(self.reading(
            &format!("{entry} — Mémoire vidéo (octets)"),
            &format!("{base}/mem_info_vram_total"),
        ));
    }

    /// Collect a snapshot. Individual failures remain visible in the output.
    pub fn collect(&self) -> Snapshot {
        self.reads.borrow_mut().clear(); // Fresh on explicit refresh; shared within this snapshot only.
        tracing::info!("Collecting read-only Linux diagnostic");
        let mut system = Vec::new();
        let release = self
            .read("/etc/os-release")
            .or_else(|_| self.read("/usr/lib/os-release"));
        match release {
            Ok(text) => {
                let values = gameshift_distro::parse_os_release(&text);
                for (label, key) in [
                    ("Distribution", "PRETTY_NAME"),
                    ("Famille", "ID"),
                    ("Version", "VERSION_ID"),
                    ("Variante", "VARIANT_ID"),
                ] {
                    system.push(match values.get(key) {
                        Some(value) => Reading::available(label, "os-release", value),
                        None => {
                            Reading::unavailable(label, "os-release", format!("Champ {key} absent"))
                        }
                    });
                }
            }
            Err(error) => system.push(Reading::unavailable(
                "Distribution",
                "os-release",
                error.to_string(),
            )),
        }
        system.push(self.reading("Kernel", "/proc/sys/kernel/osrelease"));
        system.push(self.reading("Démarrage", "/proc/cmdline"));
        system.push(self.reading("Modules chargés", "/proc/modules"));
        system.push(Reading::available(
            "Architecture du binaire",
            "std::env::consts::ARCH",
            std::env::consts::ARCH,
        ));
        system.push(Reading::available(
            "Marqueur OSTree",
            "/run/ostree-booted",
            if self.path("/run/ostree-booted").exists() {
                "Présent"
            } else {
                "Absent (ne prouve pas un système mutable)"
            },
        ));
        if self.root == std::path::Path::new("/") {
            for (label, key) in [
                ("Environnement de bureau", "XDG_CURRENT_DESKTOP"),
                ("Session graphique", "XDG_SESSION_TYPE"),
            ] {
                system.push(
                    match std::env::var(key).ok().filter(|s| !s.trim().is_empty()) {
                        Some(value) => Reading::available(label, key, value),
                        None => Reading::unavailable(label, key, "Variable de session absente"),
                    },
                );
            }
            system.push(
                match std::process::Command::new("uname").arg("-m").output() {
                    Ok(output) if output.status.success() && !output.stdout.is_empty() => {
                        Reading::available(
                            "Architecture système",
                            "uname -m",
                            String::from_utf8_lossy(&output.stdout).trim(),
                        )
                    }
                    _ => Reading::unavailable(
                        "Architecture système",
                        "uname -m",
                        "Commande indisponible",
                    ),
                },
            );
        }
        let mut cpu = vec![
            self.field("Modèle", "/proc/cpuinfo", "model name"),
            self.reading("CPU en ligne", "/sys/devices/system/cpu/online"),
        ];
        let policies = "/sys/devices/system/cpu/cpufreq";
        match self.entries(policies) {
            Ok(entries) => {
                for entry in entries.into_iter().filter(|s| s.starts_with("policy")) {
                    for (label, file) in [
                        ("CPU associés", "related_cpus"),
                        ("Pilote", "scaling_driver"),
                        ("Gouverneur", "scaling_governor"),
                        ("Gouverneurs disponibles", "scaling_available_governors"),
                        ("Fréquence courante (kHz)", "scaling_cur_freq"),
                        ("Fréquence minimale (kHz)", "cpuinfo_min_freq"),
                        ("Fréquence maximale (kHz)", "cpuinfo_max_freq"),
                        ("EPP", "energy_performance_preference"),
                    ] {
                        cpu.push(self.reading(
                            &format!("{entry} — {label}"),
                            &format!("{policies}/{entry}/{file}"),
                        ));
                    }
                }
            }
            Err(e) => cpu.push(Reading::unavailable(
                "Politiques CPU",
                policies,
                e.to_string(),
            )),
        }
        match self.read("/proc/cpuinfo") {
            Ok(text) => {
                let mut threads = 0;
                let mut cores = std::collections::BTreeSet::new();
                for block in text.split("\n\n") {
                    let fields = block
                        .lines()
                        .filter_map(|s| s.split_once(':'))
                        .map(|(k, v)| (k.trim(), v.trim()))
                        .collect::<std::collections::BTreeMap<_, _>>();
                    if fields.contains_key("processor") {
                        threads += 1;
                    }
                    if let (Some(socket), Some(core)) =
                        (fields.get("physical id"), fields.get("core id"))
                    {
                        cores.insert((socket.to_string(), core.to_string()));
                    }
                }
                cpu.push(Reading::available(
                    "Threads exposés par procfs",
                    "/proc/cpuinfo",
                    threads.to_string(),
                ));
                cpu.push(if cores.is_empty() {
                    Reading::unavailable(
                        "Cœurs physiques exposés",
                        "/proc/cpuinfo",
                        "Identifiants de topologie absents",
                    )
                } else {
                    Reading::available(
                        "Cœurs physiques exposés",
                        "/proc/cpuinfo",
                        cores.len().to_string(),
                    )
                });
            }
            Err(e) => cpu.push(Reading::unavailable(
                "Topologie CPU",
                "/proc/cpuinfo",
                e.to_string(),
            )),
        }
        // CPU0 cache descriptions are kept per index, never summed across shared domains.
        let cache_base = "/sys/devices/system/cpu/cpu0/cache";
        if let Ok(entries) = self.entries(cache_base) {
            for entry in entries.into_iter().filter(|s| s.starts_with("index")) {
                for (label, file) in [
                    ("Niveau", "level"),
                    ("Type", "type"),
                    ("Capacité", "size"),
                    ("CPU partageant le cache", "shared_cpu_list"),
                ] {
                    cpu.push(self.reading(
                        &format!("{entry} — Cache {label}"),
                        &format!("{cache_base}/{entry}/{file}"),
                    ));
                }
            }
        }
        if let Ok(entries) = self.entries("/sys/class/hwmon") {
            for entry in entries {
                let base = format!("/sys/class/hwmon/{entry}");
                if !self.read(&format!("{base}/name")).is_ok_and(|s| {
                    matches!(
                        s.as_str(),
                        "k10temp" | "coretemp" | "zenpower" | "cpu_thermal"
                    )
                }) {
                    continue;
                }
                for file in self
                    .entries(&base)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|s| s.starts_with("temp") && s.ends_with("_input"))
                {
                    let sensor = self
                        .read(&format!("{base}/{}", file.replace("_input", "_label")))
                        .unwrap_or_else(|_| file.clone());
                    cpu.push(self.reading(
                        &format!("{entry}/{sensor} — Température (millidegrés Celsius)"),
                        &format!("{base}/{file}"),
                    ));
                }
            }
        }
        let memory = vec![
            self.field("RAM totale", "/proc/meminfo", "MemTotal"),
            self.field("RAM disponible", "/proc/meminfo", "MemAvailable"),
            self.field("Swap totale", "/proc/meminfo", "SwapTotal"),
            self.field("Swap libre", "/proc/meminfo", "SwapFree"),
            self.reading("Swap / ZRAM actives", "/proc/swaps"),
            self.reading("Swappiness", "/proc/sys/vm/swappiness"),
            self.reading(
                "Transparent Huge Pages",
                "/sys/kernel/mm/transparent_hugepage/enabled",
            ),
        ];
        let mut gpu = Vec::new();
        let drm = "/sys/class/drm";
        match self.entries(drm) {
            Ok(entries) => {
                for entry in entries.into_iter().filter(|s| {
                    s.strip_prefix("card")
                        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
                }) {
                    let base = format!("{drm}/{entry}/device");
                    if let Ok(path) = fs::canonicalize(self.path(&base)) {
                        let pci = path.file_name().unwrap_or_default().to_string_lossy();
                        if self
                            .path(&format!("/sys/bus/pci/devices/{pci}/class"))
                            .exists()
                        {
                            gpu.push(Reading::available(
                                &format!("{entry} — Adresse PCI"),
                                &base,
                                pci.to_string(),
                            ));
                            continue; // The PCI pass below owns the device readings.
                        }
                    }
                    for (label, file) in [
                        ("Constructeur PCI", "vendor"),
                        ("Périphérique PCI", "device"),
                        ("VRAM (octets)", "mem_info_vram_total"),
                        ("Fréquences GPU", "pp_dpm_sclk"),
                    ] {
                        gpu.push(
                            self.reading(&format!("{entry} — {label}"), &format!("{base}/{file}")),
                        );
                    }
                    let driver = format!("{base}/driver");
                    gpu.push(match fs::read_link(self.path(&driver)) {
                        Ok(path) => Reading::available(
                            &format!("{entry} — pilote kernel"),
                            &driver,
                            path.file_name().unwrap_or_default().to_string_lossy(),
                        ),
                        Err(e) => Reading::unavailable(
                            &format!("{entry} — pilote kernel"),
                            &driver,
                            e.to_string(),
                        ),
                    });
                }
            }
            Err(e) => gpu.push(Reading::unavailable("GPU DRM", drm, e.to_string())),
        }
        if gpu.is_empty() {
            gpu.push(Reading::unavailable("GPU DRM", drm, "Aucune carte exposée"));
        }
        let pci_names = self.database(&[
            "/usr/share/hwdata/pci.ids",
            "/usr/share/misc/pci.ids",
            "/usr/share/pci.ids",
        ]);
        let amd_names =
            self.database(&["/usr/share/libdrm/amdgpu.ids", "/usr/share/drm/amdgpu.ids"]);
        // PCI identification remains useful when DRM devices are hidden by a sandbox.
        match self.entries("/sys/bus/pci/devices") {
            Ok(entries) => {
                for entry in entries {
                    let base = format!("/sys/bus/pci/devices/{entry}");
                    if self
                        .read(&format!("{base}/class"))
                        .is_ok_and(|class| class.starts_with("0x03"))
                    {
                        for (label, file) in [
                            ("Constructeur PCI", "vendor"),
                            ("Périphérique PCI", "device"),
                            ("Révision PCI", "revision"),
                            ("Fréquences GPU", "pp_dpm_sclk"),
                            ("Sous-système constructeur", "subsystem_vendor"),
                            ("Sous-système", "subsystem_device"),
                        ] {
                            gpu.push(
                                self.reading(
                                    &format!("{entry} — {label}"),
                                    &format!("{base}/{file}"),
                                ),
                            );
                        }
                        self.gpu_identity(
                            &base,
                            &entry,
                            pci_names.as_ref(),
                            amd_names.as_ref(),
                            &mut gpu,
                        );
                        let driver = format!("{base}/driver");
                        gpu.push(match fs::read_link(self.path(&driver)) {
                            Ok(path) => Reading::available(
                                &format!("{entry} — pilote PCI"),
                                &driver,
                                path.file_name().unwrap_or_default().to_string_lossy(),
                            ),
                            Err(e) => Reading::unavailable(
                                &format!("{entry} — pilote PCI"),
                                &driver,
                                e.to_string(),
                            ),
                        });
                    }
                }
            }
            Err(e) => gpu.push(Reading::unavailable(
                "GPU PCI",
                "/sys/bus/pci/devices",
                e.to_string(),
            )),
        }
        gpu.push(Reading::unavailable(
            "Pilote et extensions Vulkan",
            "Vulkan",
            "Analyse Vulkan accessible dans le laboratoire",
        ));
        gpu.push(Reading::unavailable(
            "Version Mesa",
            "Gestionnaire de paquets",
            "Collecteur de versions non implémenté",
        ));
        let mut storage = vec![self.reading("Montages et options", "/proc/self/mounts")];
        // A physical root disk is reported only for a direct, single-device filesystem.
        if let Ok(mounts) = self.read("/proc/self/mountinfo") {
            if let Some((prefix, suffix)) = mounts
                .lines()
                .filter_map(|l| l.split_once(" - "))
                .find(|(p, _)| p.split_whitespace().nth(4) == Some("/"))
            {
                let fields = suffix.split_whitespace().collect::<Vec<_>>();
                if fields.len() >= 2 {
                    storage.push(Reading::available(
                        "Système de fichiers racine",
                        "/proc/self/mountinfo",
                        fields[0],
                    ));
                    storage.push(Reading::available(
                        "Volume racine",
                        "/proc/self/mountinfo",
                        fields[1],
                    ));
                    if matches!(fields[0], "ext2" | "ext3" | "ext4" | "xfs" | "f2fs") {
                        if let Some(dev) = prefix.split_whitespace().nth(2) {
                            if let Ok(mut path) =
                                fs::canonicalize(self.path(&format!("/sys/dev/block/{dev}")))
                            {
                                if path.join("partition").exists() {
                                    path.pop();
                                }
                                let name = path.file_name().unwrap_or_default().to_string_lossy();
                                if !name.starts_with("dm-")
                                    && !name.starts_with("md")
                                    && !name.starts_with("loop")
                                    && self.path(&format!("/sys/block/{name}")).exists()
                                {
                                    storage.push(Reading::available(
                                        "Disque principal",
                                        "/proc/self/mountinfo + /sys/dev/block",
                                        name.to_string(),
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
        match self.entries("/sys/block") {
            Ok(entries) => {
                for entry in entries {
                    for (label, file) in [
                        ("Modèle", "device/model"),
                        ("Rotatif (0=non, 1=oui)", "queue/rotational"),
                        ("Capacité (secteurs de 512 octets)", "size"),
                        ("Scheduler I/O", "queue/scheduler"),
                        ("Discard maximal (octets)", "queue/discard_max_bytes"),
                    ] {
                        storage.push(self.reading(
                            &format!("{entry} — {label}"),
                            &format!("/sys/block/{entry}/{file}"),
                        ));
                    }
                }
            }
            Err(e) => storage.push(Reading::unavailable(
                "Périphériques bloc",
                "/sys/block",
                e.to_string(),
            )),
        }
        Snapshot {
            schema_version: 1,
            captured_unix_seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            sections: vec![
                Section {
                    title: "Système".into(),
                    readings: system,
                },
                Section {
                    title: "Processeur".into(),
                    readings: cpu,
                },
                Section {
                    title: "Carte graphique".into(),
                    readings: gpu,
                },
                Section {
                    title: "Mémoire".into(),
                    readings: memory,
                },
                Section {
                    title: "Stockage".into(),
                    readings: storage,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resolves_gpu_from_local_databases_and_keeps_unknown_explicit() {
        let root = std::env::temp_dir().join(format!(
            "gameshift-gpu-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let device = root.join("sys/bus/pci/devices/0000:03:00.0");
        fs::create_dir_all(&device).unwrap();
        fs::create_dir_all(root.join("usr/share/hwdata")).unwrap();
        fs::create_dir_all(root.join("usr/share/libdrm")).unwrap();
        for (file, contents) in [
            ("class", "0x030000"),
            ("vendor", "0x1002"),
            ("device", "0x747e"),
            ("revision", "0xc8"),
            ("subsystem_vendor", "0x1eae"),
            ("subsystem_device", "0x7801"),
            ("mem_info_vram_total", "17179869184"),
        ] {
            fs::write(device.join(file), contents).unwrap();
        }
        let pci = root.join("usr/share/hwdata/pci.ids");
        let amd = root.join("usr/share/libdrm/amdgpu.ids");
        fs::write(
            &pci,
            "1002  AMD\n\t747e  Radeon RX 7700 XT / 7800 XT\n1eae  XFX Limited\n",
        )
        .unwrap();
        fs::write(
            &amd,
            "747E, C8, AMD Radeon RX 7800 XT\n747E, FF, AMD Radeon RX 7700 XT\n",
        )
        .unwrap();
        let collector = Collector::new(&root);
        let model = |snapshot: Snapshot| {
            snapshot
                .sections
                .into_iter()
                .find(|s| s.title == "Carte graphique")
                .unwrap()
                .readings
                .into_iter()
                .find(|r| r.label.ends_with(" — Modèle graphique"))
                .unwrap()
        };
        let precise = model(collector.collect());
        assert_eq!(precise.value.as_deref(), Some("AMD Radeon RX 7800 XT"));
        assert_eq!(precise.source, "/usr/share/libdrm/amdgpu.ids");
        let snapshot = collector.collect();
        let gpu = snapshot
            .sections
            .iter()
            .find(|s| s.title == "Carte graphique")
            .unwrap();
        assert!(gpu
            .readings
            .iter()
            .any(|r| r.label.ends_with(" — Fabricant de la carte")
                && r.value.as_deref() == Some("XFX Limited")));
        fs::remove_file(amd).unwrap();
        assert_eq!(
            model(collector.collect()).value.as_deref(),
            Some("Radeon RX 7700 XT / 7800 XT")
        );
        fs::remove_file(pci).unwrap();
        let unknown = model(collector.collect());
        assert!(unknown.value.is_none());
        assert!(unknown.error.is_some());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn absent_sources_are_not_invented() {
        let result = Collector::new("/a-gameshift-fixture-that-does-not-exist").collect();
        assert_eq!(result.sections.len(), 5);
        let kernel = result.sections[0]
            .readings
            .iter()
            .find(|r| r.label == "Kernel")
            .unwrap();
        assert!(kernel.value.is_none());
        assert!(kernel.error.is_some());
    }
    #[test]
    fn reads_fixture_memory_and_release() {
        let root = std::env::temp_dir().join(format!("gameshift-test-{}", std::process::id()));
        fs::create_dir_all(root.join("etc")).unwrap();
        fs::create_dir_all(root.join("proc")).unwrap();
        fs::write(
            root.join("etc/os-release"),
            "PRETTY_NAME=Fixture\nID=test\n",
        )
        .unwrap();
        fs::write(
            root.join("proc/meminfo"),
            "MemTotal: 1024 kB\nMemAvailable: 512 kB\n",
        )
        .unwrap();
        let snapshot = Collector::new(&root).collect();
        assert_eq!(
            snapshot.sections[0].readings[0].value.as_deref(),
            Some("Fixture")
        );
        assert_eq!(
            snapshot.sections[3].readings[0].value.as_deref(),
            Some("1024 kB")
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod collection_cache_tests {
    use super::*;
    #[test]
    fn repeated_reads_are_shared_but_explicit_refresh_is_fresh() {
        let root = std::env::temp_dir().join(format!(
            "gameshift-cache-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("proc")).unwrap();
        fs::write(
            root.join("proc/meminfo"),
            "MemTotal: 1024 kB\nMemAvailable: 512 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n",
        )
        .unwrap();
        let collector = Collector::new(&root);
        let first = collector.collect();
        assert_eq!(
            first.sections[3].readings[0].value.as_deref(),
            Some("1024 kB")
        );
        fs::write(
            root.join("proc/meminfo"),
            "MemTotal: 2048 kB\nMemAvailable: 1000 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n",
        )
        .unwrap();
        assert_eq!(
            collector
                .field("RAM totale", "/proc/meminfo", "MemTotal")
                .value
                .as_deref(),
            Some("1024 kB")
        );
        let second = collector.collect();
        assert_eq!(
            second.sections[3].readings[0].value.as_deref(),
            Some("2048 kB")
        );
        assert_eq!(
            collector
                .reads
                .borrow()
                .keys()
                .filter(|p| p.ends_with("proc/meminfo"))
                .count(),
            1
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod root_disk_tests {
    use super::*;
    #[test]
    fn direct_root_partition_resolves_but_btrfs_is_not_assumed_single_device() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!(
            "gameshift-root-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        for dir in [
            "proc/self",
            "sys/block",
            "sys/dev/block",
            "sys/devices/nvme0n1/nvme0n1p2",
        ] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        fs::write(root.join("sys/devices/nvme0n1/nvme0n1p2/partition"), "2").unwrap();
        symlink("../devices/nvme0n1", root.join("sys/block/nvme0n1")).unwrap();
        symlink(
            "../../devices/nvme0n1/nvme0n1p2",
            root.join("sys/dev/block/259:2"),
        )
        .unwrap();
        let mount = root.join("proc/self/mountinfo");
        fs::write(&mount, "1 0 259:2 / / rw - ext4 /dev/nvme0n1p2 rw\n").unwrap();
        let collector = Collector::new(&root);
        let snapshot = collector.collect();
        assert!(snapshot.sections[4]
            .readings
            .iter()
            .any(|r| r.label == "Disque principal" && r.value.as_deref() == Some("nvme0n1")));
        fs::write(&mount, "1 0 259:2 / / rw - btrfs /dev/nvme0n1p2 rw\n").unwrap();
        let snapshot = collector.collect();
        assert!(!snapshot.sections[4]
            .readings
            .iter()
            .any(|r| r.label == "Disque principal"));
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod drm_alias_tests {
    use super::*;
    #[test]
    fn drm_alias_keeps_mapping_without_a_duplicate_gpu() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!(
            "gameshift-drm-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let device = root.join("sys/bus/pci/devices/0000:03:00.0");
        fs::create_dir_all(&device).unwrap();
        fs::create_dir_all(root.join("sys/class/drm/card0")).unwrap();
        for (file, value) in [
            ("class", "0x030000"),
            ("vendor", "0x1002"),
            ("device", "0x747e"),
            ("mem_info_vram_total", "17179869184"),
        ] {
            fs::write(device.join(file), value).unwrap();
        }
        symlink(
            "../../../bus/pci/devices/0000:03:00.0",
            root.join("sys/class/drm/card0/device"),
        )
        .unwrap();
        let collector = Collector::new(&root);
        let snapshot = collector.collect();
        let gpu = &snapshot.sections[2];
        assert!(gpu.readings.iter().any(
            |r| r.label == "card0 — Adresse PCI" && r.value.as_deref() == Some("0000:03:00.0")
        ));
        assert_eq!(
            gpu.readings
                .iter()
                .filter(|r| r.label.ends_with(" — Périphérique PCI"))
                .count(),
            1
        );
        assert_eq!(
            gpu.readings
                .iter()
                .filter(|r| r.label.ends_with(" — Mémoire vidéo (octets)"))
                .count(),
            1
        );
        assert!(!gpu
            .readings
            .iter()
            .any(|r| r.label == "card0 — VRAM (octets)"));
        fs::remove_dir_all(root).unwrap();
    }
}
