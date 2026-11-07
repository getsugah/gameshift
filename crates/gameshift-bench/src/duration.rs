//! Duration protocol metadata and post-run one-second windows. No rendering changes.
use crate::{Repetition, RunConfig};
use gameshift_core::duration::DurationProfile;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PassDuration {
    pub repetition: u32,
    pub preparation_ms: f64,
    pub measurement_wall_ms: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowSample {
    pub repetition: u32,
    pub elapsed_seconds: f64,
    pub duration_ms: f64,
    pub frames: usize,
    pub fps: f64,
    pub cpu_ms: Option<f64>,
    pub gpu_ms: Option<f64>,
    pub presentation_wait_ms: Option<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DurationReport {
    #[serde(default)]
    pub gpu_sensors: gameshift_telemetry::sensors::SensorReport,
    pub engine_version: String,
    pub protocol_id: String,
    pub protocol_version: u32,
    pub profile: DurationProfile,
    pub camera_measurement_start_seconds: f64,
    pub preparation_policy: String,
    pub passes: Vec<PassDuration>,
    pub windows: Vec<WindowSample>,
    pub minimum_window_fps: Option<f64>,
    pub maximum_window_fps: Option<f64>,
    pub fps_extrema_method: String,
}
fn mean(values: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    let values = values.flatten().collect::<Vec<_>>();
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}
impl DurationReport {
    pub fn from_runs(config: &RunConfig, runs: &[Repetition], passes: Vec<PassDuration>) -> Self {
        let mut windows = Vec::new();
        for (index, run) in runs.iter().enumerate() {
            let mut start = 0;
            let mut ms = 0.;
            let mut elapsed = 0.;
            for (i, sample) in run.samples.iter().enumerate() {
                ms += sample.frame_interval_ms;
                if ms >= 1000. || i + 1 == run.samples.len() {
                    let slice = &run.samples[start..=i];
                    elapsed += ms / 1000.;
                    windows.push(WindowSample {
                        repetition: index as u32 + 1,
                        elapsed_seconds: elapsed,
                        duration_ms: ms,
                        frames: slice.len(),
                        fps: slice.len() as f64 * 1000. / ms,
                        cpu_ms: mean(slice.iter().map(|s| s.cpu_work_ms)),
                        gpu_ms: mean(slice.iter().map(|s| s.gpu_execution_ms)),
                        presentation_wait_ms: None,
                    });
                    ms = 0.;
                    start = i + 1;
                }
            }
        }
        let full = windows
            .iter()
            .filter(|w| w.duration_ms >= 1000.)
            .map(|w| w.fps)
            .collect::<Vec<_>>();
        Self{gpu_sensors:Default::default(),engine_version:"aurelia-raster-v3-duration-1".into(),protocol_id:if config.duration_profile.is_some(){"aurelia-duration-v1"}else{"aurelia-legacy-timed-v3"}.into(),protocol_version:1,
   profile:config.duration_profile.unwrap_or(DurationProfile::Custom),camera_measurement_start_seconds:if config.duration_profile.is_some(){3.}else{config.warmup_seconds as f64},preparation_policy:"same-active-render-preparation-before-each-repetition-ending-at-measured-camera-start-no-idle".into(),passes,windows,
   minimum_window_fps:full.iter().copied().reduce(f64::min),maximum_window_fps:full.iter().copied().reduce(f64::max),fps_extrema_method:"min/max des débits de fenêtres consécutives d’au moins une seconde ; fenêtre finale partielle exclue des extrema, incluse dans la chronologie".into()}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_keep_partial_samples_but_exclude_them_from_fps_extrema() {
        let config = RunConfig {
            gpu_uuid: "00".repeat(16),
            width: 320,
            height: 240,
            grid_side: 8,
            requested_msaa: 4,
            warmup_seconds: 20,
            measure_seconds: 60,
            repetitions: 5,
            require_validation: false,
            duration_profile: Some(DurationProfile::Thorough),
        };
        let samples = (0..25)
            .map(|_| crate::FrameSample {
                frame_interval_ms: 100.,
                cpu_work_ms: Some(1.),
                gpu_execution_ms: Some(2.),
            })
            .collect();
        let runs = vec![Repetition::from_samples(samples).unwrap()];
        let result = DurationReport::from_runs(&config, &runs, vec![]);
        assert_eq!(result.camera_measurement_start_seconds, 3.);
        assert_eq!(result.windows.len(), 3);
        assert_eq!(result.windows[2].duration_ms, 500.);
        assert_eq!(result.minimum_window_fps, Some(10.));
        assert!(result
            .windows
            .iter()
            .all(|s| s.presentation_wait_ms.is_none()));
    }
}
