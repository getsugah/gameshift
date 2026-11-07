//! Vulkan capabilities and measurement definitions for the integrated benchmark.
//! Capability inspection never produces scores or changes system settings.
mod statistics;
mod vulkan;

pub use statistics::{summarize, FrameSample, Statistics, StatisticsError};
pub use vulkan::{
    probe, GpuCapabilities, ProbeError, QueueCapabilities, ValidationMode, VulkanReport,
};

mod protocol;
mod renderer;
mod runner;
pub use protocol::{
    Repetition, RunConfig, RunEnvironment, RunResult, RESULT_SCHEMA_VERSION, SCENE_VERSION,
};
pub use runner::{run, smoke_offscreen};

mod report;
pub use report::{display_warning, format_text_report};

pub mod duration;
pub mod system_raster;
