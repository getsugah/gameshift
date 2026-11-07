//! Optional read-only device-wide GPU utilization, never attributed to a process.
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// A real sensor sample, tagged with the measured repetition (warmup is omitted).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtilizationSample {
    pub elapsed_seconds: f64,
    pub repetition: u32,
    pub percent: u8,
}
/// This sensor includes other applications using the same GPU.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuUtilization {
    pub source: Option<String>,
    pub samples: Vec<UtilizationSample>,
    pub average_percent: Option<f64>,
    pub maximum_percent: Option<u8>,
    pub unavailable_reason: Option<String>,
}
impl GpuUtilization {
    fn unavailable(source: Option<String>, reason: impl Into<String>) -> Self {
        Self {
            source,
            samples: vec![],
            average_percent: None,
            maximum_percent: None,
            unavailable_reason: Some(reason.into()),
        }
    }
}
fn canonical_address(input: &str) -> Option<String> {
    let mut parts = input.split(':');
    let domain = u32::from_str_radix(parts.next()?, 16).ok()?;
    let bus = u8::from_str_radix(parts.next()?, 16).ok()?;
    let mut slot = parts.next()?.split('.');
    let device = u8::from_str_radix(slot.next()?, 16).ok()?;
    let function = u8::from_str_radix(slot.next()?, 16).ok()?;
    if parts.next().is_some() || slot.next().is_some() || device > 31 || function > 7 {
        return None;
    }
    Some(format!("{domain:04x}:{bus:02x}:{device:02x}.{function:x}"))
}
fn parse_percent(text: &str) -> Option<u8> {
    text.trim().parse::<u8>().ok().filter(|n| *n <= 100)
}

/// A 200 ms background sampler, stopped before returning or destroying a run.
pub struct GpuSampler {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<GpuUtilization>>,
    initial: Option<GpuUtilization>,
}
impl GpuSampler {
    /// The address comes from the selected Vulkan physical device. Phase 0 means warmup.
    pub fn start(address: Option<&str>, phase: Arc<AtomicU32>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let mut sampler = Self {
            stop: stop.clone(),
            worker: None,
            initial: None,
        };
        let Some(address) = address.and_then(canonical_address) else {
            sampler.initial = Some(GpuUtilization::unavailable(
                None,
                "Adresse PCI du GPU non disponible",
            ));
            return sampler;
        };
        let path = format!("/sys/bus/pci/devices/{address}/gpu_busy_percent");
        match std::fs::read_to_string(&path) {
            Ok(value) if parse_percent(&value).is_some() => {}
            Ok(_) => {
                sampler.initial = Some(GpuUtilization::unavailable(
                    Some(path),
                    "Valeur du capteur invalide",
                ));
                return sampler;
            }
            Err(error) => {
                sampler.initial = Some(GpuUtilization::unavailable(Some(path), error.to_string()));
                return sampler;
            }
        }
        let fallback_path = path.clone();
        match thread::Builder::new()
            .name("gameshift-gpu-utilization".into())
            .spawn(move || {
                let mut report = GpuUtilization {
                    source: Some(path.clone()),
                    samples: vec![],
                    average_percent: None,
                    maximum_percent: None,
                    unavailable_reason: None,
                };
                let started = Instant::now();
                while !stop.load(Ordering::Relaxed) {
                    let repetition = phase.load(Ordering::Relaxed);
                    if repetition > 0 {
                        match std::fs::read_to_string(&path)
                            .map_err(|e| e.to_string())
                            .and_then(|s| {
                                parse_percent(&s).ok_or_else(|| "Valeur du capteur invalide".into())
                            }) {
                            Ok(percent) if phase.load(Ordering::Relaxed) == repetition => {
                                report.samples.push(UtilizationSample {
                                    elapsed_seconds: started.elapsed().as_secs_f64(),
                                    repetition,
                                    percent,
                                })
                            }
                            Ok(_) => {}
                            Err(error) => {
                                report.unavailable_reason = Some(error);
                                break;
                            }
                        }
                    }
                    thread::sleep(Duration::from_millis(200));
                }
                if !report.samples.is_empty() {
                    report.average_percent = Some(
                        report
                            .samples
                            .iter()
                            .map(|s| f64::from(s.percent))
                            .sum::<f64>()
                            / report.samples.len() as f64,
                    );
                    report.maximum_percent = report.samples.iter().map(|s| s.percent).max();
                } else if report.unavailable_reason.is_none() {
                    report.unavailable_reason =
                        Some("Aucun échantillon d’occupation collecté pendant les mesures".into());
                }
                report
            }) {
            Ok(worker) => sampler.worker = Some(worker),
            Err(error) => {
                sampler.initial = Some(GpuUtilization::unavailable(
                    Some(fallback_path),
                    error.to_string(),
                ))
            }
        }
        sampler
    }
    /// Stops outside the measured frame loop; absent values stay absent.
    pub fn finish(mut self) -> GpuUtilization {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .unwrap_or_else(|_| GpuUtilization::unavailable(None, "Collecteur interrompu"))
        } else {
            self.initial
                .take()
                .unwrap_or_else(|| GpuUtilization::unavailable(None, "Collecteur indisponible"))
        }
    }
}
impl Drop for GpuSampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires GAMESHIFT_TEST_GPU_PCI_ADDRESS with an accessible real utilization sensor"]
    fn samples_real_sensor_only_during_measured_phase() {
        let address = std::env::var("GAMESHIFT_TEST_GPU_PCI_ADDRESS").expect("GPU PCI address");
        let phase = Arc::new(AtomicU32::new(0));
        let sampler = GpuSampler::start(Some(&address), phase.clone());
        thread::sleep(Duration::from_millis(250));
        phase.store(2, Ordering::Relaxed);
        thread::sleep(Duration::from_millis(450));
        phase.store(0, Ordering::Relaxed);
        let report = sampler.finish();
        assert!(
            report.unavailable_reason.is_none(),
            "{:?}",
            report.unavailable_reason
        );
        assert!(!report.samples.is_empty());
        assert!(report
            .samples
            .iter()
            .all(|s| s.repetition == 2 && s.percent <= 100));
        assert!(report.average_percent.is_some());
    }

    #[test]
    fn rejects_invalid_sensor_values_and_pci_addresses() {
        assert_eq!(parse_percent("100\n"), Some(100));
        assert_eq!(parse_percent("0"), Some(0));
        for bad in ["101", "-1", "NaN", "50%", ""] {
            assert_eq!(parse_percent(bad), None);
        }
        assert_eq!(
            canonical_address("0000:03:00.0").as_deref(),
            Some("0000:03:00.0")
        );
        for bad in [
            "../../etc/passwd",
            "0000:03:20.0",
            "0000:03:00.8",
            "0000:03:00.0/driver",
        ] {
            assert!(canonical_address(bad).is_none());
        }
    }
    #[test]
    fn absent_gpu_address_never_becomes_zero_utilization() {
        let report = GpuSampler::start(None, Arc::new(AtomicU32::new(1))).finish();
        assert!(report.average_percent.is_none());
        assert!(report.samples.is_empty());
        assert!(report.unavailable_reason.is_some());
    }
}

pub mod sensors;

pub mod device;

pub mod history;

pub mod capabilities;

pub mod gpus;
pub mod processes;

#[cfg(test)]
mod process_tests;
