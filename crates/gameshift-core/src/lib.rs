//! Shared diagnostic models. Missing readings always carry a reason.
use serde::{Deserialize, Serialize};

/// One observation with an explicit source and optional failure reason.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reading {
    pub label: String,
    pub source: String,
    pub value: Option<String>,
    pub error: Option<String>,
}

impl Reading {
    /// Construct a successful observation.
    pub fn available(label: &str, source: &str, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            source: source.into(),
            value: Some(value.into()),
            error: None,
        }
    }
    /// Construct an unavailable observation, retaining its cause.
    pub fn unavailable(label: &str, source: &str, error: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            source: source.into(),
            value: None,
            error: Some(error.into()),
        }
    }
}

/// Related observations, independent of GUI rendering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Section {
    pub title: String,
    pub readings: Vec<Reading>,
}

/// A read-only snapshot; never an optimization or benchmark result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub schema_version: u32,
    pub captured_unix_seconds: u64,
    pub sections: Vec<Section>,
}

pub mod duration;

pub mod preferences;
