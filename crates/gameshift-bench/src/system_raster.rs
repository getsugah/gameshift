//! Additive offscreen system-suite adapter; historical run/protocol unchanged.
use crate::{renderer::Renderer, FrameSample, RunConfig, ValidationMode};
use serde::Serialize;
use std::sync::atomic::Ordering;
#[derive(Debug, Serialize)]
pub struct Identity {
    pub name: String,
    pub software: bool,
    pub driver_name: String,
    pub driver_info: String,
    pub pci_address: Option<String>,
    pub msaa: u32,
    pub validation_enabled: bool,
    pub timestamp_period_ns: Option<f32>,
    pub timestamp_valid_bits: u32,
}
pub struct SystemRaster {
    inner: Renderer,
    config: RunConfig,
}
impl SystemRaster {
    /// Capture process overrides separately from frame timing.
    pub fn environment() -> crate::RunEnvironment {
        crate::RunEnvironment::capture()
    }
    pub fn new(config: RunConfig, validation: bool) -> Result<Self, String> {
        let inner = Renderer::new_with_validation(
            &config,
            None,
            Some(if validation {
                ValidationMode::Required
            } else {
                ValidationMode::Disabled
            }),
        )?;
        Ok(Self { inner, config })
    }
    pub fn frame(&mut self, seconds: f32) -> Result<FrameSample, String> {
        let sample = self.inner.draw(&self.config, seconds)?;
        if self.inner.validation_errors.load(Ordering::Relaxed) > 0 {
            return Err("Erreur de validation Vulkan".into());
        }
        Ok(sample)
    }
    pub fn identity(&self) -> Identity {
        let (period, bits) = self.inner.timestamp_metadata();
        Identity {
            name: self.inner.gpu_name.clone(),
            software: self.inner.software,
            driver_name: self.inner.driver_name.clone(),
            driver_info: self.inner.driver_info.clone(),
            pci_address: self.inner.pci_address.clone(),
            msaa: self.inner.sample_count.as_raw(),
            validation_enabled: self.inner.validation_enabled,
            timestamp_period_ns: period,
            timestamp_valid_bits: bits,
        }
    }
}
