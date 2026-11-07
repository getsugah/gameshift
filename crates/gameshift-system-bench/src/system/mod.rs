//! Full system protocol; all historical runners remain independent.
use crate::session::Metric;
use serde::{Deserialize, Serialize};
pub mod cpu;
pub mod evaluation;
pub mod gpu;
pub mod ram;
pub mod reporting;
pub const PROTOCOL: &str = "aurelia-system-v2";
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Executed,
    Unavailable,
    Failed,
    NotImplemented,
}
impl State {
    pub fn label(self) -> &'static str {
        match self {
            Self::Executed => "Exécuté",
            Self::Unavailable => "Indisponible",
            Self::Failed => "Échoué",
            Self::NotImplemented => "Non implémenté",
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Category {
    pub id: String,
    pub state: State,
    pub elapsed_seconds: f64,
    pub metrics: Vec<Metric>,
    pub raw: serde_json::Value,
    pub notes: Vec<String>,
}
impl Category {
    pub fn missing(id: &str, state: State, reason: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            state,
            elapsed_seconds: 0.,
            metrics: vec![],
            raw: serde_json::Value::Null,
            notes: vec![reason.into()],
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Body {
    pub protocol: String,
    pub target_seconds: f64,
    pub elapsed_seconds: f64,
    pub overrun_seconds: f64,
    pub categories: Vec<Category>,
    pub parameters: serde_json::Value,
}
pub fn metric(
    id: &str,
    category: &str,
    label: &str,
    unit: &str,
    values: Vec<f64>,
    description: &str,
) -> Metric {
    Metric {
        id: id.into(),
        category: category.into(),
        label: label.into(),
        unit: unit.into(),
        values,
        confirmation_reasons: vec![],
        work_description: description.into(),
    }
}
