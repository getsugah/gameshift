//! Synthetic score/coverage fixtures only; never used by a benchmark runner.
use gameshift_system_bench::{
    session::{Fingerprint, Session, SystemKind},
    system::{self, metric, reporting, Body, Category, State},
};
fn fixture() -> Session {
    let categories = [
        "cpu",
        "cache",
        "ram",
        "gpu",
        "combined",
        "glibc",
        "applications",
    ]
    .into_iter()
    .map(|id| Category {
        id: id.into(),
        state: State::Executed,
        elapsed_seconds: 1.,
        metrics: vec![metric(
            &format!("{id}/fixture"),
            id,
            "fixture",
            "unit/s",
            vec![100.; 3],
            "same-work",
        )],
        raw: serde_json::json!({"identity":{"software":false}}),
        notes: vec![],
    })
    .collect::<Vec<_>>();
    Session {
        schema_version: 2,
        protocol: system::PROTOCOL.into(),
        application_version: "test fixture".into(),
        completed_unix_seconds: 1,
        kind: SystemKind::Original,
        fingerprint: Fingerprint {
            distribution: "fixture".into(),
            distribution_id: "fixture".into(),
            kernel: None,
            architecture: "x86_64".into(),
            cpu_model: Some("fixture CPU".into()),
            microcode: None,
            logical_cpus: vec![0, 1],
            physical_cores: Some(1),
            ram_kib: Some(1024),
            pci_gpus: vec![],
            governors: Default::default(),
            energy_preferences: Default::default(),
            engine_sha256: "a".repeat(64),
            helper_sha256: "b".repeat(64),
        },
        elapsed_seconds: 7.,
        metrics: categories.iter().flat_map(|c| c.metrics.clone()).collect(),
        cpu_raw: vec![],
        glibc_raw: serde_json::Value::Null,
        limitations: vec![],
        system: Some(Body {
            protocol: system::PROTOCOL.into(),
            target_seconds: 120.,
            elapsed_seconds: 7.,
            overrun_seconds: 0.,
            categories,
            parameters: serde_json::Value::Null,
        }),
    }
}
#[test]
fn missing_application_does_not_invalidate_complete_native_score() {
    let a = fixture();
    let mut b = fixture();
    b.system
        .as_mut()
        .unwrap()
        .categories
        .iter_mut()
        .find(|c| c.id == "applications")
        .unwrap()
        .state = State::Unavailable;
    let scores = reporting::compare(&a, &b).unwrap();
    assert!((scores.native.unwrap() - 100.).abs() < 1e-8);
    assert!(scores.application.is_none());
    assert!(scores.synthesis.is_none());
}
#[test]
fn missing_gpu_does_not_get_zero_or_complete_native_score() {
    let a = fixture();
    let mut b = fixture();
    b.system
        .as_mut()
        .unwrap()
        .categories
        .iter_mut()
        .find(|c| c.id == "gpu")
        .unwrap()
        .state = State::Unavailable;
    b.system
        .as_mut()
        .unwrap()
        .categories
        .iter_mut()
        .find(|c| c.id == "gpu")
        .unwrap()
        .metrics
        .clear();
    b.metrics.retain(|m| m.category != "gpu");
    let scores = reporting::compare(&a, &b).unwrap();
    assert!(scores.native.is_none());
    assert!(scores.categories["gpu"].is_none());
    assert!((scores.application.unwrap() - 100.).abs() < 1e-8);
}
#[test]
fn individual_report_requires_neither_reference_nor_comparison() {
    let mut a = fixture();
    a.system.as_mut().unwrap().categories[3].state = State::Failed;
    let text = reporting::individual(&a);
    assert!(
        text.contains("RAPPORT INDIVIDUEL")
            && text.contains("Échoué")
            && text.contains("RAM native")
            && text.contains("Bibliothèques système")
    );
}
#[test]
fn software_gpu_cannot_be_published_as_a_hardware_measurement() {
    let mut a = fixture();
    a.system.as_mut().unwrap().categories[3].raw["identity"]["software"] = true.into();
    assert!(a.validate().is_err());
}
#[test]
fn all_seven_states_are_required_even_for_partial_results() {
    let mut a = fixture();
    a.system.as_mut().unwrap().categories.pop();
    assert!(a.validate().is_err());
}
#[test]
fn incompatible_parameters_do_not_produce_a_native_index() {
    let a = fixture();
    let mut b = fixture();
    b.system.as_mut().unwrap().categories[3].metrics[0].work_description =
        "different quality".into();
    let score = reporting::compare(&a, &b).unwrap();
    assert!(score.native.is_none());
    assert!(score.categories["gpu"].is_none());
}

#[test]
fn hardware_comparison_is_explicit_and_keeps_workload_checks() {
    let a = fixture();
    let mut b = fixture();
    b.fingerprint.cpu_model = Some("Another CPU".into());
    b.fingerprint.pci_gpus = vec!["Another GPU".into()];
    assert!(reporting::compare(&a, &b).is_err());
    assert!(reporting::compare_with_hardware(&a, &b, true)
        .unwrap()
        .synthesis
        .is_some());
    b.system.as_mut().unwrap().categories[2].metrics[0].work_description = "different size".into();
    assert!(reporting::compare_with_hardware(&a, &b, true)
        .unwrap()
        .native
        .is_none());
}
#[test]
fn contribution_points_add_to_the_global_difference() {
    let a = fixture();
    let mut b = fixture();
    for c in &mut b.system.as_mut().unwrap().categories {
        for m in &mut c.metrics {
            m.values = vec![110.; 3];
        }
    }
    b.metrics = b
        .system
        .as_ref()
        .unwrap()
        .categories
        .iter()
        .flat_map(|c| c.metrics.clone())
        .collect();
    let scores = reporting::compare(&a, &b).unwrap();
    assert!(
        (scores.contributions.values().sum::<f64>() - (scores.synthesis.unwrap() - 100.)).abs()
            < 1e-9
    );
    assert!(reporting::compare_text(&a, &b)
        .unwrap()
        .contains("FAITS MESURÉS"));
}
