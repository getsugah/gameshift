//! Constrained AMDGPU business operations. The IPC never accepts paths or shell commands.
pub mod model;
pub use model::*;
mod sysfs;
pub use sysfs::{canonical_pci, Sysfs};
mod transaction;
pub use transaction::{check_request, execute, prepare, restore, verify_applied, Prepared};
#[cfg(test)]
mod tests;
/// Private to the control protocol; production access is always anchored to one PCI device.
pub trait DeviceIo {
    fn identity(&self) -> Result<Identity>;
    fn read(&self, path: &str) -> Result<String>;
    fn write(&mut self, path: &str, value: &str) -> Result<()>;
    fn power_paths(&self) -> Result<Vec<(u32, String)>>;
}
impl Catalog {
    pub fn discover(pci: &str) -> Result<Self> {
        Self::read(&Sysfs::open(pci)?)
    }
    pub fn read(io: &impl DeviceIo) -> Result<Self> {
        let identity = io.identity()?;
        let mut out = Self {
            identity,
            scalars: vec![],
            curve: None,
            performance: None,
            workloads: vec![],
            clocks: vec![],
            unavailable: vec![],
        };
        for setting in FanSetting::ALL {
            let path = setting.path();
            match io.read(&path).and_then(|s| parse_fan(&s, setting)) {
                Ok((current, limits)) => {
                    let (_, _, _, label, unit) = setting.definition();
                    out.scalars.push(Scalar {
                        target: ScalarTarget::Fan { setting },
                        label: label.into(),
                        unit: unit.into(),
                        source: path,
                        current,
                        limits,
                    });
                }
                Err(error) => out
                    .unavailable
                    .push(format!("{} : {error}", setting.definition().3)),
            }
        }
        match io
            .read("gpu_od/fan_ctrl/fan_curve")
            .and_then(|s| parse_curve(&s))
        {
            Ok(curve) => out.curve = Some(curve),
            Err(e) => out.unavailable.push(format!("Courbe PMFW : {e}")),
        }
        if let Ok(value) = io.read("power_dpm_force_performance_level") {
            out.performance = Some(value.trim().to_owned());
        }
        if let Ok(value) = io.read("pp_power_profile_mode") {
            out.workloads = parse_workloads(&value);
        }
        match io.power_paths() {
            Ok(paths) => {
                for (channel, path) in paths {
                    let read = |suffix: &str| number(&io.read(&format!("{path}{suffix}"))?);
                    match (read(""), read("_min"), read("_max")) {
                        (Ok(current), Ok(minimum), Ok(maximum))
                            if 0 <= minimum && minimum <= maximum =>
                        {
                            out.scalars.push(Scalar {
                                target: ScalarTarget::PowerCap { channel },
                                label: format!("Limite de puissance · canal {channel}"),
                                unit: "µW".into(),
                                source: path,
                                current,
                                limits: Some(Limits { minimum, maximum }),
                            })
                        }
                        _ => out.unavailable.push(format!(
                            "Plage de puissance incomplète pour le canal {channel}"
                        )),
                    }
                }
            }
            Err(e) => out.unavailable.push(e),
        }
        match io
            .read("pp_od_clk_voltage")
            .and_then(|s| parse_overdrive(&s))
        {
            Ok((clocks, voltage)) => {
                out.clocks = clocks;
                if let Some((current, limits)) = voltage {
                    out.scalars.push(Scalar {
                        target: ScalarTarget::VoltageOffset {},
                        label: "Décalage de tension GPU".into(),
                        unit: "mV".into(),
                        source: "pp_od_clk_voltage".into(),
                        current,
                        limits: Some(limits),
                    });
                }
            }
            Err(error) => out.unavailable.push(format!("OverDrive : {error}")),
        }
        Ok(out)
    }
    pub fn scalar(&self, target: ScalarTarget) -> Result<&Scalar> {
        self.scalars
            .iter()
            .find(|s| s.target == target)
            .ok_or("Réglage non exposé ou format non pris en charge".into())
    }
}

/// Best-effort actual control state, collected outside measured benchmark regions.
pub fn snapshot_all() -> serde_json::Value {
    let mut devices = Vec::new();
    let mut errors = Vec::new();
    match std::fs::read_dir("/sys/bus/pci/devices") {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();
                if std::fs::read_link(path.join("driver"))
                    .ok()
                    .and_then(|p| p.file_name().map(|n| n == "amdgpu"))
                    .unwrap_or(false)
                {
                    let pci = entry.file_name().to_string_lossy().into_owned();
                    match Catalog::discover(&pci) {
                        Ok(catalog) => devices.push(catalog),
                        Err(error) => errors.push(format!("{pci} : {error}")),
                    }
                }
            }
        }
        Err(error) => errors.push(error.to_string()),
    }
    devices.sort_by(|a, b| a.identity.pci.cmp(&b.identity.pci));
    serde_json::json!({"schema_version":1,"observation_only":true,"devices":devices,"errors":errors})
}

pub mod benchmark;
