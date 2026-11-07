//! Pure calculations over recorded samples, never a synthetic benchmark runner.
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};

/// A frame sample collected after warmup. CPU recording and GPU execution differ.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameSample {
    /// Monotonic interval between CPU-side frame completions, including GPU waits and presentation call waits. This is not the time of physical display scanout.
    pub frame_interval_ms: f64,
    /// CPU scene preparation/recording after command-buffer begin through queue submission,
    /// not total CPU frame time or GPU execution time.
    pub cpu_work_ms: Option<f64>,
    /// GPU timestamp difference converted with the device timestamp period.
    pub gpu_execution_ms: Option<f64>,
}

/// Statistical definition shared by future JSON/CSV exports and the GUI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Statistics {
    pub frames: usize,
    pub measured_ms: f64,
    pub average_fps: f64,
    pub average_frametime_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub p999_ms: f64,
    pub one_percent_low_fps: f64,
    pub point_one_percent_low_fps: f64,
    pub average_cpu_work_ms: Option<f64>,
    pub cpu_samples: usize,
    pub average_gpu_execution_ms: Option<f64>,
    pub gpu_samples: usize,
}

/// Invalid samples cannot silently enter a performance comparison.
#[derive(Debug, PartialEq, Eq)]
pub enum StatisticsError {
    NoFrames,
    InvalidSample(usize),
    Overflow,
}
impl fmt::Display for StatisticsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoFrames => write!(f, "Aucune frame mesurée"),
            Self::InvalidSample(index) => {
                write!(f, "Échantillon de frame invalide à l’index {index}")
            }
            Self::Overflow => write!(f, "Somme des durées non représentable"),
        }
    }
}
impl Error for StatisticsError {}

fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    sorted[(fraction * sorted.len() as f64).ceil() as usize - 1]
}
fn low(sorted: &[f64], fraction: f64) -> f64 {
    let count = (fraction * sorted.len() as f64).ceil() as usize;
    let mean = sorted[sorted.len() - count..].iter().sum::<f64>() / count as f64;
    1000.0 / mean
}
fn mean(values: impl Iterator<Item = f64>) -> (Option<f64>, usize) {
    let (total, count) = values.fold((0.0, 0), |(total, count), v| (total + v, count + 1));
    ((count > 0).then(|| total / count as f64), count)
}

/// Nearest-rank percentiles; lows are reciprocals of the slowest fraction's mean.
/// Repetition variability and evidence of improvement require multiple independent runs.
pub fn summarize(samples: &[FrameSample]) -> Result<Statistics, StatisticsError> {
    if samples.is_empty() {
        return Err(StatisticsError::NoFrames);
    }
    for (i, sample) in samples.iter().enumerate() {
        if !sample.frame_interval_ms.is_finite()
            || sample.frame_interval_ms <= 0.0
            || sample
                .cpu_work_ms
                .is_some_and(|n| !n.is_finite() || n < 0.0)
            || sample
                .gpu_execution_ms
                .is_some_and(|n| !n.is_finite() || n < 0.0)
        {
            return Err(StatisticsError::InvalidSample(i));
        }
    }
    let mut sorted: Vec<_> = samples.iter().map(|s| s.frame_interval_ms).collect();
    sorted.sort_by(f64::total_cmp);
    let measured_ms = sorted.iter().sum::<f64>();
    let (average_cpu_work_ms, cpu_samples) = mean(samples.iter().filter_map(|s| s.cpu_work_ms));
    let (average_gpu_execution_ms, gpu_samples) =
        mean(samples.iter().filter_map(|s| s.gpu_execution_ms));
    if !measured_ms.is_finite()
        || average_cpu_work_ms.is_some_and(|n| !n.is_finite())
        || average_gpu_execution_ms.is_some_and(|n| !n.is_finite())
    {
        return Err(StatisticsError::Overflow);
    }
    let result = Statistics {
        frames: samples.len(),
        measured_ms,
        average_fps: samples.len() as f64 * 1000.0 / measured_ms,
        average_frametime_ms: measured_ms / samples.len() as f64,
        p95_ms: percentile(&sorted, 0.95),
        p99_ms: percentile(&sorted, 0.99),
        p999_ms: percentile(&sorted, 0.999),
        one_percent_low_fps: low(&sorted, 0.01),
        point_one_percent_low_fps: low(&sorted, 0.001),
        average_cpu_work_ms,
        cpu_samples,
        average_gpu_execution_ms,
        gpu_samples,
    };
    if !result.average_fps.is_finite()
        || !result.one_percent_low_fps.is_finite()
        || !result.point_one_percent_low_fps.is_finite()
    {
        return Err(StatisticsError::Overflow);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_arithmetic_overflow_in_totals_or_reciprocals() {
        for durations in [vec![f64::MAX, f64::MAX], vec![f64::MIN_POSITIVE / 1000.0]] {
            let samples: Vec<_> = durations
                .into_iter()
                .map(|frame_interval_ms| FrameSample {
                    frame_interval_ms,
                    cpu_work_ms: None,
                    gpu_execution_ms: None,
                })
                .collect();
            assert_eq!(summarize(&samples).unwrap_err(), StatisticsError::Overflow);
        }
    }
    #[test]
    fn statistics_match_known_frametime_fixture_and_keep_gpu_absent() {
        // Mathematical test fixture, never exported as a hardware measurement.
        let mut samples = vec![
            FrameSample {
                frame_interval_ms: 10.0,
                cpu_work_ms: Some(2.0),
                gpu_execution_ms: None
            };
            100
        ];
        samples[99].frame_interval_ms = 100.0;
        let stats = summarize(&samples).unwrap();
        assert_eq!(stats.measured_ms, 1090.0);
        assert_eq!(stats.p95_ms, 10.0);
        assert_eq!(stats.p99_ms, 10.0);
        assert_eq!(stats.p999_ms, 100.0);
        assert_eq!(stats.one_percent_low_fps, 10.0);
        assert_eq!(stats.point_one_percent_low_fps, 10.0);
        assert_eq!(stats.average_cpu_work_ms, Some(2.0));
        assert_eq!(stats.average_gpu_execution_ms, None);
        assert_eq!(stats.gpu_samples, 0);
        assert!((stats.average_fps - 100_000.0 / 1090.0).abs() < 1e-10);
    }
    #[test]
    fn rejects_empty_non_finite_and_invalid_durations() {
        assert_eq!(summarize(&[]).unwrap_err(), StatisticsError::NoFrames);
        for duration in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let samples = [FrameSample {
                frame_interval_ms: duration,
                cpu_work_ms: None,
                gpu_execution_ms: None,
            }];
            assert_eq!(
                summarize(&samples).unwrap_err(),
                StatisticsError::InvalidSample(0)
            );
        }
        let samples = [FrameSample {
            frame_interval_ms: 1.0,
            cpu_work_ms: None,
            gpu_execution_ms: Some(-1.0),
        }];
        assert_eq!(
            summarize(&samples).unwrap_err(),
            StatisticsError::InvalidSample(0)
        );
    }
}
