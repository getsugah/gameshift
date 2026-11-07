//! Duration presets describe protocol timing, never scene or workload complexity.
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DurationProfile {
    Quick,
    #[default]
    Standard,
    Thorough,
    Custom,
}
impl DurationProfile {
    pub fn label(self) -> &'static str {
        match self {
            Self::Quick => "Rapide",
            Self::Standard => "Standard",
            Self::Thorough => "Approfondi",
            Self::Custom => "Personnalisé",
        }
    }
    pub fn gpu(self) -> (u32, u32, u32) {
        match self {
            Self::Quick => (5, 10, 2),
            Self::Thorough => (20, 60, 5),
            _ => (10, 30, 3),
        }
    }
    pub fn cpu(self) -> (u64, u64, usize) {
        match self {
            Self::Quick => (500, 250, 3),
            Self::Thorough => (5000, 1000, 5),
            _ => (2000, 500, 3),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkProgress {
    pub phase: String,
    pub repetition: usize,
    pub repetitions: usize,
    pub case_index: usize,
    pub cases: usize,
    pub fraction: f64,
    pub elapsed_seconds: f64,
    pub remaining_seconds: Option<f64>,
    pub remaining_scope: String,
    pub fps: Option<f64>,
    pub cpu_ms: Option<f64>,
    pub gpu_ms: Option<f64>,
}
pub type ProgressSink = Arc<Mutex<Option<BenchmarkProgress>>>;
pub fn publish(sink: &ProgressSink, value: BenchmarkProgress) {
    if let Ok(mut slot) = sink.lock() {
        *slot = Some(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presets_match_documented_durations() {
        assert_eq!(DurationProfile::Quick.gpu(), (5, 10, 2));
        assert_eq!(DurationProfile::Standard.gpu(), (10, 30, 3));
        assert_eq!(DurationProfile::Thorough.gpu(), (20, 60, 5));
        assert_eq!(DurationProfile::Quick.cpu().0, 500);
        assert_eq!(DurationProfile::Standard.cpu().0, 2000);
        assert_eq!(DurationProfile::Thorough.cpu().0, 5000);
        assert_eq!(DurationProfile::default(), DurationProfile::Standard);
    }
}
