//! Absolute, uncapped rate scores. K=1 in the recorded unit for each input.
use super::State;
use crate::session::Session;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub const VERSION: &str = "aurelia-rate-points-v1";
pub const PROTOCOL: &str = "aurelia-system-v3-storage";
#[derive(Debug, Serialize, Deserialize)]
pub struct Scores {
    pub version: String,
    pub categories: BTreeMap<String, Option<f64>>,
    pub global: Option<f64>,
    pub method: String,
}
fn geometric(values: &[f64]) -> Option<f64> {
    if values.is_empty() || values.iter().any(|v| !v.is_finite() || *v <= 0.) {
        return None;
    }
    Some((values.iter().map(|v| v.ln()).sum::<f64>() / values.len() as f64).exp())
}
pub fn calculate(s: &Session) -> Scores {
    let mut categories = BTreeMap::new();
    for (id, prefixes, unit) in [
        (
            "cpu",
            vec!["cpu/single/", "cpu/multi_sustained/"],
            "étapes/s",
        ),
        ("gpu", vec!["gpu/raster"], "images/s"),
        (
            "ram",
            vec!["ram/read/", "ram/write/", "ram/copy/"],
            "Go/s logiques",
        ),
        ("storage", vec!["storage/read", "storage/write"], "Go/s"),
    ] {
        let score = s
            .system
            .as_ref()
            .filter(|b| {
                b.protocol == s.protocol
                    && [super::PROTOCOL, PROTOCOL].contains(&b.protocol.as_str())
            })
            .and_then(|b| {
                b.categories
                    .iter()
                    .find(|c| c.id == id && c.state == State::Executed)
            })
            .and_then(|c| {
                let rates: Option<Vec<f64>> = prefixes
                    .iter()
                    .map(|prefix| {
                        let found: Vec<_> = c
                            .metrics
                            .iter()
                            .filter(|m| m.id.starts_with(prefix) && m.unit == unit)
                            .collect();
                        (found.len() == 1)
                            .then(|| found[0])
                            .filter(|m| {
                                m.values.len() == 3
                                    && m.values.iter().all(|v| v.is_finite() && *v > 0.)
                            })
                            .map(|m| m.mean())
                    })
                    .collect();
                rates.and_then(|rates| geometric(&rates))
            });
        categories.insert(id.to_owned(), score);
    }
    let global = if s.protocol == PROTOCOL {
        categories
            .values()
            .copied()
            .collect::<Option<Vec<_>>>()
            .and_then(|v| geometric(&v))
    } else {
        None
    };
    Scores {version:VERSION.into(),categories,global,method:"K=1 point par étape/s CPU, image/s GPU, Go/s logique RAM et Go/s stockage. Moyenne géométrique des sous-tests par catégorie ; global = (CPU × GPU × RAM × stockage)^(1/4). Poids égaux. Mesures manquantes : aucun global. Aucun plafond.".into()}
}
/// Compatibility is decided on the workloads, never on a score's magnitude.
pub fn compatible(a: &Session, b: &Session, id: &str, across_hardware: bool) -> bool {
    if a.protocol != b.protocol
        || a.fingerprint.engine_sha256 != b.fingerprint.engine_sha256
        || a.fingerprint.helper_sha256 != b.fingerprint.helper_sha256
    {
        return false;
    }
    let signature = |s: &Session| -> BTreeMap<String, (String, String)> {
        s.metrics
            .iter()
            .filter(|m| m.category == id)
            .map(|m| {
                (
                    super::reporting::metric_key(m, across_hardware),
                    (m.unit.clone(), super::reporting::work(m, across_hardware)),
                )
            })
            .collect()
    };
    let aa = signature(a);
    !aa.is_empty() && aa == signature(b)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn doubling_rates_doubles_geometric_score() {
        let a = geometric(&[10., 20., 40.]).unwrap();
        let b = geometric(&[20., 40., 80.]).unwrap();
        assert!((b / a - 2.).abs() < 1e-12);
    }
    #[test]
    fn missing_and_invalid_are_never_zero_scores() {
        assert_eq!(geometric(&[]), None);
        assert_eq!(geometric(&[1., f64::NAN]), None);
        assert_eq!(geometric(&[1., 0.]), None);
    }
}
