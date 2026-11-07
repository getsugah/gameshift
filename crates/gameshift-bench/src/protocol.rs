//! Versioned run protocol and exports; rendering samples are always retained.
use crate::{summarize, FrameSample, Statistics};
use serde::{Deserialize, Serialize};

pub const RESULT_SCHEMA_VERSION: u32 = 3;

pub const SCENE_VERSION: &str = "aurelia-raster-v3-msaa";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunConfig {
    pub gpu_uuid: String,
    pub width: u32,
    pub height: u32,
    pub grid_side: u32,
    #[serde(default = "default_msaa")]
    pub requested_msaa: u32,
    pub warmup_seconds: u32,
    pub measure_seconds: u32,
    pub repetitions: u32,
    pub require_validation: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_profile: Option<gameshift_core::duration::DurationProfile>,
}
fn default_msaa() -> u32 {
    8
}

impl RunConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.gpu_uuid.len() != 32 || !self.gpu_uuid.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err("UUID GPU invalide".into());
        }
        if !(320..=3840).contains(&self.width) || !(240..=2160).contains(&self.height) {
            return Err("Résolution hors des limites autorisées".into());
        }
        if ![1, 2, 4, 8, 16].contains(&self.requested_msaa) {
            return Err("Niveau MSAA invalide".into());
        }
        if !(8..=48).contains(&self.grid_side)
            || !(1..=300).contains(&self.warmup_seconds)
            || !(1..=600).contains(&self.measure_seconds)
            || !(1..=20).contains(&self.repetitions)
        {
            return Err("Protocole hors des limites autorisées".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repetition {
    pub samples: Vec<FrameSample>,
    pub statistics: Statistics,
}
impl Repetition {
    pub(crate) fn from_samples(samples: Vec<FrameSample>) -> Result<Self, String> {
        let statistics = summarize(&samples).map_err(|e| e.to_string())?;
        Ok(Self {
            samples,
            statistics,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResult {
    pub schema_version: u32,
    pub scene_version: String,
    pub application_version: String,
    pub frames_in_flight: u32,
    pub msaa_samples: u32,
    pub gpu_utilization: gameshift_telemetry::GpuUtilization,
    pub gpu_timestamp_period_ns: Option<f32>,
    pub gpu_timestamp_valid_bits: u32,
    pub config: RunConfig,
    pub gpu_name: String,
    pub driver_name: String,
    pub driver_info: String,
    pub software_device: bool,
    pub headless: bool,
    pub present_mode: String,
    pub actual_width: u32,
    pub actual_height: u32,
    pub presentation_width: u32,
    pub presentation_height: u32,
    pub validation_enabled: bool,
    pub validation_errors: usize,
    pub completed_unix_seconds: u64,
    pub environment: RunEnvironment,
    pub repetitions: Vec<Repetition>,
    /// Sample standard deviation / mean across repetition mean FPS values, in percent.
    pub repetition_cv_percent: Option<f64>,
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<crate::duration::DurationReport>,
}
impl RunResult {
    pub fn csv(&self) -> String {
        let mut csv =
            String::from("repetition,frame,frame_interval_ms,cpu_work_ms,gpu_execution_ms\n");
        for (run, repetition) in self.repetitions.iter().enumerate() {
            for (frame, sample) in repetition.samples.iter().enumerate() {
                csv.push_str(&format!(
                    "{},{},{:.6},{},{}\n",
                    run + 1,
                    frame + 1,
                    sample.frame_interval_ms,
                    sample
                        .cpu_work_ms
                        .map(|v| format!("{v:.6}"))
                        .unwrap_or_default(),
                    sample
                        .gpu_execution_ms
                        .map(|v| format!("{v:.6}"))
                        .unwrap_or_default()
                ));
            }
        }
        if let Some(duration) = &self.duration {
            let profile = serde_json::to_string(&duration.profile)
                .unwrap_or_default()
                .trim_matches('"')
                .to_owned();
            let mut versioned = String::new();
            let quote = |value: &str| format!("\"{}\"", value.replace('"', "\"\""));
            let metadata = format!(
                "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                self.schema_version,
                quote(&profile),
                quote(&duration.protocol_id),
                quote(&duration.engine_version),
                quote(&self.scene_version),
                self.actual_width,
                self.actual_height,
                self.msaa_samples,
                quote(&self.present_mode),
                self.config.warmup_seconds,
                self.config.measure_seconds,
                self.config.repetitions,
                duration.camera_measurement_start_seconds,
                self.config.grid_side,
                self.config.requested_msaa,
                self.validation_enabled
            );
            for (index, line) in csv.lines().enumerate() {
                if index == 0 {
                    versioned.push_str(&format!(
                        "{line},schema_version,duration_profile,duration_protocol,engine_version,scene_version,width,height,msaa,present_mode,preparation_seconds,measure_seconds,repetitions,camera_start_seconds,grid_side,requested_msaa,validation_enabled,preparation_actual_ms,measurement_wall_actual_ms\n"
                    ));
                } else {
                    let pass = line
                        .split(',')
                        .next()
                        .and_then(|n| n.parse::<usize>().ok())
                        .and_then(|n| n.checked_sub(1))
                        .and_then(|n| duration.passes.get(n));
                    let prep = pass
                        .map(|p| p.preparation_ms.to_string())
                        .unwrap_or_default();
                    let measured = pass
                        .map(|p| p.measurement_wall_ms.to_string())
                        .unwrap_or_default();
                    versioned.push_str(&format!("{line},{metadata},{prep},{measured}\n"));
                }
            }
            return versioned;
        }
        csv
    }
}

pub(crate) fn variability(runs: &[Repetition]) -> Option<f64> {
    if runs.len() < 2 {
        return None;
    }
    let mean = runs.iter().map(|r| r.statistics.average_fps).sum::<f64>() / runs.len() as f64;
    let variance = runs
        .iter()
        .map(|r| (r.statistics.average_fps - mean).powi(2))
        .sum::<f64>()
        / (runs.len() - 1) as f64;
    Some(variance.sqrt() / mean * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protocol_refuses_invalid_uuid_and_excessive_load() {
        let mut c = RunConfig {
            gpu_uuid: "00".repeat(16),
            width: 1280,
            height: 720,
            grid_side: 24,
            requested_msaa: 8,
            warmup_seconds: 3,
            measure_seconds: 8,
            repetitions: 3,
            require_validation: true,
            duration_profile: None,
        };
        assert!(c.validate().is_ok());
        c.grid_side = 1000;
        assert!(c.validate().is_err());
        c.grid_side = 24;
        c.gpu_uuid = "not-a-gpu".into();
        assert!(c.validate().is_err());
    }
}

/// Minimal local context, captured outside the measured frame loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunEnvironment {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_profile_before: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_profile_after: Option<serde_json::Value>,
    pub kernel: Option<String>,
    pub os_release: Option<String>,
    pub cpu_model: Option<String>,
    pub session_type: Option<String>,
    pub overrides: std::collections::BTreeMap<String, String>,
}
impl RunEnvironment {
    pub(crate) fn capture() -> Self {
        let read = |path: &str| {
            std::fs::read_to_string(path)
                .ok()
                .map(|s| s.trim().to_owned())
        };
        let cpu_model = read("/proc/cpuinfo").and_then(|text| {
            text.lines()
                .filter_map(|l| l.split_once(':'))
                .find(|(k, _)| k.trim() == "model name")
                .map(|(_, v)| v.trim().to_owned())
        });
        let overrides = [
            "MESA_VK_WSI_PRESENT_MODE",
            "MESA_VK_WSI_DEBUG",
            "VK_INSTANCE_LAYERS",
            "VK_LAYER_VALIDATE_SYNC",
            "VK_LAYER_ENABLES",
            "VK_LAYER_DISABLES",
            "VK_DRIVER_FILES",
            "VK_ICD_FILENAMES",
            "vblank_mode",
            "MANGOHUD",
            "MANGOHUD_CONFIG",
        ]
        .into_iter()
        .filter_map(|name| {
            std::env::var(name)
                .ok()
                .map(|value| (name.into(), value.chars().take(4096).collect()))
        })
        .collect();
        Self {
            gpu_profile_before: Some(gameshift_control::benchmark::profile_snapshot()),
            gpu_profile_after: None,
            kernel: read("/proc/sys/kernel/osrelease"),
            os_release: read("/etc/os-release").or_else(|| read("/usr/lib/os-release")),
            cpu_model,
            session_type: std::env::var("XDG_SESSION_TYPE").ok(),
            overrides,
        }
    }
}
