//! Portable CPU/cache/glibc comparison protocol, independent of package managers.
use gameshift_core::duration::DurationProfile;
use gameshift_cpu::{
    calibrated::CpuDuration,
    runner::{Config, Report},
    topology::Topology,
    workload::Pattern,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

pub const SESSION_PROTOCOL: &str = "aurelia-distribution-cpu-glibc-v1";
pub const MAX_REPORT_BYTES: u64 = 128 * 1024 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SystemKind {
    Original,
    Optimized,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fingerprint {
    pub distribution: String,
    pub distribution_id: String,
    pub kernel: Option<String>,
    pub architecture: String,
    pub cpu_model: Option<String>,
    pub microcode: Option<String>,
    pub logical_cpus: Vec<usize>,
    pub physical_cores: Option<usize>,
    pub ram_kib: Option<u64>,
    pub pci_gpus: Vec<String>,
    pub governors: BTreeMap<String, String>,
    pub energy_preferences: BTreeMap<String, String>,
    pub engine_sha256: String,
    pub helper_sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metric {
    pub id: String,
    pub category: String,
    pub label: String,
    pub unit: String,
    pub values: Vec<f64>,
    pub confirmation_reasons: Vec<String>,
    pub work_description: String,
}
impl Metric {
    pub fn mean(&self) -> f64 {
        self.values
            .iter()
            .map(|v| v / self.values.len() as f64)
            .sum::<f64>()
    }
    pub fn cv(&self) -> f64 {
        let m = self.mean();
        if self.values.len() < 2 {
            return 0.;
        }
        (self
            .values
            .iter()
            .map(|v| (v / m - 1.).powi(2))
            .sum::<f64>()
            / (self.values.len() - 1) as f64)
            .sqrt()
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Session {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<crate::system::Body>,
    pub schema_version: u32,
    pub protocol: String,
    pub application_version: String,
    pub completed_unix_seconds: u64,
    pub kind: SystemKind,
    pub fingerprint: Fingerprint,
    pub elapsed_seconds: f64,
    pub metrics: Vec<Metric>,
    pub cpu_raw: Vec<Report>,
    pub glibc_raw: serde_json::Value,
    pub limitations: Vec<String>,
}
fn text(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}
/// sha256sum is invoked outside all measured regions, with no shell interpolation.
pub fn digest(bytes: &[u8]) -> Result<String, String> {
    let mut child = Command::new("sha256sum")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("sha256sum requis : {e}"))?;
    let written = child
        .stdin
        .take()
        .ok_or("Entrée SHA-256 absente")?
        .write_all(bytes);
    let result = child.wait_with_output().map_err(|e| e.to_string())?;
    written.map_err(|e| e.to_string())?;
    if !result.status.success() {
        return Err("Calcul SHA-256 échoué".into());
    }
    let result = String::from_utf8(result.stdout).map_err(|e| e.to_string())?;
    let hash = result.split_whitespace().next().ok_or("SHA-256 absent")?;
    if !result.is_empty() && hash.len() == 64 && hash.bytes().all(|c| c.is_ascii_hexdigit()) {
        Ok(hash.into())
    } else {
        Err("SHA-256 invalide".into())
    }
}
pub(crate) fn fingerprint(helper: &Path) -> Result<Fingerprint, String> {
    let topology = Topology::detect();
    let info = text("/proc/cpuinfo").unwrap_or_default();
    let field = |key: &str| {
        info.lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(k, _)| k.trim() == key)
            .map(|(_, v)| v.trim().to_owned())
    };
    let os = gameshift_distro::parse_os_release(
        &text("/etc/os-release")
            .or_else(|| text("/usr/lib/os-release"))
            .unwrap_or_default(),
    );
    let mut governors = BTreeMap::new();
    let mut energy_preferences = BTreeMap::new();
    if let Ok(entries) = fs::read_dir("/sys/devices/system/cpu/cpufreq") {
        for entry in entries.flatten() {
            let base = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(v) = text(base.join("scaling_governor")) {
                governors.insert(name.clone(), v);
            }
            if let Some(v) = text(base.join("energy_performance_preference")) {
                energy_preferences.insert(name, v);
            }
        }
    }
    let mut pci_gpus = Vec::new();
    if let Ok(entries) = fs::read_dir("/sys/bus/pci/devices") {
        for entry in entries.flatten() {
            let base = entry.path();
            if text(base.join("class")).is_some_and(|v| v.starts_with("0x03")) {
                let fields = [
                    "vendor",
                    "device",
                    "subsystem_vendor",
                    "subsystem_device",
                    "revision",
                ]
                .map(|name| text(base.join(name)).unwrap_or_else(|| "Indisponible".into()));
                pci_gpus.push(fields.join(":"));
            }
        }
    }
    pci_gpus.sort();
    let ram_kib = text("/proc/meminfo").and_then(|s| {
        s.lines()
            .find_map(|line| line.strip_prefix("MemTotal:"))
            .and_then(|v| v.split_whitespace().next()?.parse().ok())
    });
    Ok(Fingerprint {
        distribution: os
            .get("PRETTY_NAME")
            .cloned()
            .unwrap_or_else(|| "Distribution indisponible".into()),
        distribution_id: os.get("ID").cloned().unwrap_or_default(),
        kernel: text("/proc/sys/kernel/osrelease"),
        architecture: std::env::consts::ARCH.into(),
        cpu_model: field("model name").or_else(|| field("Hardware")),
        microcode: field("microcode"),
        logical_cpus: topology.allowed_logical_cpus,
        physical_cores: topology.physical_cores,
        ram_kib,
        pci_gpus,
        governors,
        energy_preferences,
        engine_sha256: digest(
            &fs::read(std::env::current_exe().map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?,
        )?,
        helper_sha256: digest(&fs::read(helper).map_err(|e| e.to_string())?)?,
    })
}
pub fn evaluate(kind: SystemKind, mut progress: impl FnMut(&str, f64)) -> Result<Session, String> {
    let started = Instant::now();
    let mut helper = std::env::current_exe().map_err(|e| e.to_string())?;
    helper.set_file_name("aurelia-host-bench");
    let fingerprint = fingerprint(&helper)?;
    let mut counts = vec![1, fingerprint.logical_cpus.len().max(1)];
    counts.dedup();
    let mut jobs = Vec::new();
    for workers in counts {
        jobs.push((Pattern::Entities, workers, 4, "cpu"));
    }
    for pattern in [Pattern::Sequential, Pattern::Dependent] {
        for size in [4, 32, 128] {
            jobs.push((pattern, 1, size, "cache"));
        }
    }
    let mut cpu_raw = Vec::new();
    let mut metrics = Vec::new();
    let total = jobs.len() + 1;
    for (index, (pattern, workers, size, category)) in jobs.into_iter().enumerate() {
        let name = format!(
            "{} · {size} Mio · {workers} threads",
            gameshift_cpu::reporting::pattern_name(pattern)
        );
        progress(&name, index as f64 / total as f64);
        let config = Config {
            workers: vec![workers],
            sizes_mib: vec![size],
            pattern,
            duration: Some(CpuDuration::preset(DurationProfile::Standard)),
            ..Config::default()
        };
        let report = gameshift_cpu::runner::run(config)?;
        let case = report.cases.first().ok_or("Mesure CPU absente")?;
        let values = case
            .repetitions
            .iter()
            .map(|samples| {
                samples.len() as f64 * 1e9
                    / samples
                        .iter()
                        .map(|s| s.critical_wall_ns as f64)
                        .sum::<f64>()
            })
            .collect();
        let mut reasons = Vec::new();
        if case.inconclusive {
            if case.repetition_cv > 0.05 {
                reasons.push(format!("Variation de {:.1} % entre les répétitions ; relancez en conditions plus stables.",case.repetition_cv*100.));
            }
            if case.repetitions.iter().any(|samples| {
                samples
                    .iter()
                    .map(|v| v.critical_wall_ns as u128)
                    .sum::<u128>()
                    < 100_000_000
            }) {
                reasons.push("Une répétition dure moins de 100 ms ; mesure trop courte.".into());
            }
            if reasons.is_empty() {
                reasons.push(
                    "Résultat nécessitant une confirmation ; détails dans le rapport CPU brut."
                        .into(),
                );
            }
        }
        let cal = case.calibration.as_ref();
        let work_description = format!(
            "{pattern:?};{size};{workers};seed={};dt={};steps={};preparation={}",
            report.config.seed,
            report.config.fixed_step_ns,
            cal.map(|c| c.measured_steps)
                .unwrap_or(report.config.measured_steps),
            cal.map(|c| c.preparation_steps)
                .unwrap_or(report.config.warmup_steps)
        );
        metrics.push(Metric {
            id: format!("{category}/{pattern:?}/{workers}/{size}"),
            category: category.into(),
            label: name,
            unit: "étapes/s".into(),
            values,
            confirmation_reasons: reasons,
            work_description,
        });
        cpu_raw.push(report);
    }
    progress(
        "Bibliothèque glibc de la distribution",
        (total - 1) as f64 / total as f64,
    );
    let output = crate::host_command(&helper)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let glibc_raw: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
    if glibc_raw["executable_sha256"].as_str() != Some(fingerprint.helper_sha256.as_str()) {
        return Err("Empreinte de l’auxiliaire différente du binaire attendu".into());
    }
    for summary in glibc_raw["summaries"]
        .as_array()
        .ok_or("Résumés glibc absents")?
    {
        let operation = summary["operation"]
            .as_str()
            .ok_or("Opération glibc absente")?;
        let size = summary["block_bytes"]
            .as_u64()
            .ok_or("Taille glibc absente")?;
        let samples = glibc_raw["samples"]
            .as_array()
            .ok_or("Échantillons glibc absents")?
            .iter()
            .filter(|s| {
                s["operation"].as_str() == Some(operation)
                    && s["block_bytes"].as_u64() == Some(size)
            })
            .collect::<Vec<_>>();
        if samples.iter().any(|s| s["verified"] != true) {
            return Err("Validation glibc échouée".into());
        }
        metrics.push(Metric {
            id: format!("glibc/{operation}/{size}"),
            category: "glibc".into(),
            label: format!("{operation} · {size} octets"),
            unit: "appels/s".into(),
            values: samples
                .iter()
                .filter_map(|s| s["calls_per_second"].as_f64())
                .collect(),
            confirmation_reasons: summary["confirmation_reasons"]
                .as_array()
                .ok_or("Fiabilité glibc absente")?
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect(),
            work_description: format!(
                "{}/{operation}/{size}",
                glibc_raw["protocol"].as_str().unwrap_or("inconnu")
            ),
        });
    }
    progress("Enregistrement et analyse", 1.);
    let report=Session{system:None,schema_version:1,protocol:SESSION_PROTOCOL.into(),application_version:env!("CARGO_PKG_VERSION").into(),completed_unix_seconds:SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e|e.to_string())?.as_secs(),kind,fingerprint,elapsed_seconds:started.elapsed().as_secs_f64(),metrics,cpu_raw,glibc_raw,limitations:vec!["Évaluation CPU/cache/glibc ; GPU, RAM dédiée et charge combinée exclus de cette synthèse.".into(),"Une signature matérielle compatible ne prouve pas à elle seule l’identité physique de la machine.".into(),"Les changements de versions logicielles ne prouvent pas la cause d’un écart de performances.".into()]};
    report.validate()?;
    Ok(report)
}
impl Session {
    pub fn validate(&self) -> Result<(), String> {
        if self.system.is_some() {
            return crate::system::reporting::validate(self);
        }
        if self.schema_version != 1 || self.protocol != SESSION_PROTOCOL {
            return Err("Protocole de comparaison non pris en charge".into());
        }
        if self.metrics.is_empty()
            || self.metrics.len() > 64
            || self.metrics.iter().any(|m| {
                m.values.len() < 3
                    || m.values.len() > 20
                    || m.values.iter().any(|v| !v.is_finite() || *v <= 0.)
                    || !["cpu", "cache", "glibc"].contains(&m.category.as_str())
            })
        {
            return Err("Mesures invalides ou incomplètes".into());
        }
        let unique: std::collections::BTreeSet<_> = self.metrics.iter().map(|m| &m.id).collect();
        if unique.len() != self.metrics.len() {
            return Err("Mesures dupliquées".into());
        }
        for hash in [
            &self.fingerprint.engine_sha256,
            &self.fingerprint.helper_sha256,
        ] {
            if hash.len() != 64 || !hash.bytes().all(|v| v.is_ascii_hexdigit()) {
                return Err("Empreinte moteur absente ou invalide".into());
            }
        }
        if self.fingerprint.cpu_model.is_none()
            || self.fingerprint.logical_cpus.is_empty()
            || self.fingerprint.ram_kib.is_none()
        {
            return Err("Signature CPU/mémoire insuffisante pour comparer".into());
        }
        Ok(())
    }
}
