//! CPU-only benchmarks: real deterministic work, independent of GTK and Vulkan.
pub mod environment;
pub mod instrument;
pub mod runner;
pub mod topology;
pub mod workload;

pub mod reporting;

pub mod calibrated;
