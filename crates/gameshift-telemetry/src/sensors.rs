//! Low-frequency GPU sensors tied to the selected physical PCI device, no driver calls.
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorSample {
    pub elapsed_seconds: f64,
    pub repetition: u32,
    pub temperature_celsius: Option<f64>,
    pub core_clock_mhz: Option<f64>,
    #[serde(default)]
    pub measurements: Vec<crate::device::Measurement>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SensorReport {
    pub temperature_source: Option<String>,
    pub frequency_source: Option<String>,
    pub samples: Vec<SensorSample>,
    pub unavailable_reason: Option<String>,
    #[serde(default)]
    pub discovery_errors: Vec<String>,
    #[serde(default)]
    pub sampling_interval_millis: Option<u64>,
}
pub struct SensorSampler {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<SensorReport>>,
    initial: SensorReport,
}
pub(crate) fn active_mhz(text: &str) -> Option<f64> {
    let line = text.lines().find(|s| s.contains('*'))?;
    let value = line.split_once(':')?.1.trim().trim_end_matches('*').trim();
    let (n, scale) = if let Some(n) = value
        .strip_suffix("Mhz")
        .or_else(|| value.strip_suffix("MHz"))
    {
        (n, 1.)
    } else {
        let n = value
            .strip_suffix("Ghz")
            .or_else(|| value.strip_suffix("GHz"))?;
        (n, 1000.)
    };
    n.trim()
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite() && *n >= 0.)
        .map(|n| n * scale)
}
impl SensorSampler {
    pub fn start(address: Option<&str>, phase: Arc<AtomicU32>) -> Self {
        Self::start_with_interval(address, phase, Duration::from_millis(500))
    }
    /// Bounded cadence; default callers retain 2 Hz. Changes require a new sampler.
    pub fn start_with_interval(
        address: Option<&str>,
        phase: Arc<AtomicU32>,
        interval: Duration,
    ) -> Self {
        let interval = interval.clamp(Duration::from_millis(200), Duration::from_secs(60));
        let mut sampler = Self {
            stop: Arc::new(AtomicBool::new(false)),
            worker: None,
            initial: SensorReport::default(),
        };
        let Some(address) = address.and_then(super::canonical_address) else {
            sampler.initial.unavailable_reason =
                Some("Adresse PCI sélectionnée indisponible ; capteurs non attribués".into());
            return sampler;
        };
        let device = match crate::device::DeviceSensors::discover(&address) {
            Ok(device) => device,
            Err(error) => {
                sampler.initial.unavailable_reason = Some(error);
                return sampler;
            }
        };
        if device.sensors.is_empty() {
            sampler.initial.unavailable_reason =
                Some("Aucun capteur AMDGPU exposé par sysfs".into());
            sampler.initial.discovery_errors = device.discovery_errors;
            return sampler;
        }
        let stop = sampler.stop.clone();
        sampler.worker = thread::Builder::new()
            .name("gameshift-gpu-sensors".into())
            .spawn(move || {
                use crate::device::Metric;
                // Preserve legacy fields only when the sensor is identified unambiguously.
                let temperature = device.sensors.iter().find(|s| {
                    s.metric == Metric::Temperature && s.label.eq_ignore_ascii_case("edge")
                });
                let frequency = device
                    .sensors
                    .iter()
                    .find(|s| s.metric == Metric::CoreClock);
                let mut report = SensorReport {
                    temperature_source: temperature.map(|s| s.path.display().to_string()),
                    frequency_source: frequency.map(|s| s.path.display().to_string()),
                    discovery_errors: device.discovery_errors.clone(),
                    sampling_interval_millis: Some(interval.as_millis() as u64),
                    ..Default::default()
                };
                let start = Instant::now();
                while !stop.load(Ordering::Relaxed) {
                    let repetition = phase.load(Ordering::Relaxed);
                    let measurements = device.read();
                    let temperature = measurements
                        .iter()
                        .find(|m| Some(m.source.as_str()) == report.temperature_source.as_deref())
                        .and_then(|m| m.value);
                    let frequency = measurements
                        .iter()
                        .find(|m| Some(m.source.as_str()) == report.frequency_source.as_deref())
                        .and_then(|m| m.value);
                    if phase.load(Ordering::Relaxed) == repetition {
                        report.samples.push(SensorSample {
                            elapsed_seconds: start.elapsed().as_secs_f64(),
                            repetition,
                            temperature_celsius: temperature,
                            core_clock_mhz: frequency,
                            measurements,
                        });
                    }
                    let deadline = Instant::now() + interval;
                    while !stop.load(Ordering::Relaxed) {
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        if remaining.is_zero() {
                            break;
                        }
                        thread::sleep(remaining.min(Duration::from_millis(100)));
                    }
                }
                if report
                    .samples
                    .iter()
                    .all(|s| s.measurements.iter().all(|m| m.value.is_none()))
                {
                    report.unavailable_reason =
                        Some("Capteurs présents mais valeurs inaccessibles ou invalides".into());
                }
                report
            })
            .ok();
        if sampler.worker.is_none() {
            sampler.initial.unavailable_reason =
                Some("Impossible de démarrer la collecte des capteurs GPU".into());
        }
        sampler
    }
    pub fn finish(mut self) -> SensorReport {
        self.stop.store(true, Ordering::Relaxed);
        self.worker
            .take()
            .map(|w| {
                w.join().unwrap_or_else(|_| SensorReport {
                    unavailable_reason: Some("Collecteur interrompu".into()),
                    ..Default::default()
                })
            })
            .unwrap_or_else(|| self.initial.clone())
    }
}
impl Drop for SensorSampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn active_frequency_units() {
        assert_eq!(active_mhz("0: 100Mhz\n1: 2400Mhz *"), Some(2400.));
        assert_eq!(active_mhz("1: 2.4GHz *"), Some(2400.));
        assert_eq!(active_mhz("1: 100Mhz"), None);
    }
}
