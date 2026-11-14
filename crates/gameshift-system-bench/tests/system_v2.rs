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

fn storage_fixture() -> Session {
    let mut s = fixture();
    s.schema_version=3;
    s.protocol=system::scoring::PROTOCOL.into();
    let b=s.system.as_mut().unwrap();
    b.protocol=system::scoring::PROTOCOL.into();
    for c in &mut b.categories {
        let (names,unit) = match c.id.as_str() {
            "cpu"=>(vec!["cpu/single/1","cpu/multi_sustained/2"],"étapes/s"),
            "gpu"=>(vec!["gpu/raster"],"images/s"),
            "ram"=>(vec!["ram/read/268435456","ram/write/268435456","ram/copy/268435456"],"Go/s logiques"),
            _=>continue,
        };
        c.metrics=names.into_iter().map(|id|metric(id,&c.id,id,unit,vec![100.;3],"fixture-work")).collect();
    }
    b.categories.push(Category{id:"storage".into(),state:State::Executed,elapsed_seconds:1.,metrics:vec![metric("storage/read","storage","read","Go/s",vec![100.;3],"fixture-work"),metric("storage/write","storage","write","Go/s",vec![100.;3],"fixture-work")],raw:serde_json::json!({"fixture":true}),notes:vec![]});
    s.metrics=b.categories.iter().flat_map(|c|c.metrics.clone()).collect();
    refresh_points(&mut s);
    s
}
fn refresh_points(s:&mut Session) {
    let scores=serde_json::to_value(system::scoring::calculate(s)).unwrap();
    s.system.as_mut().unwrap().parameters=serde_json::json!({"rate_scores":scores,"score_version":system::scoring::VERSION});
}
#[test] fn storage_protocol_export_import_preserves_exact_original_bytes() {
    use gameshift_system_bench::store;
    let mut s=storage_fixture();
    for c in &mut s.system.as_mut().unwrap().categories {for m in &mut c.metrics {m.values=vec![0.123456789012345,1.987654321098765,9.314159265358979];}}
    s.metrics=s.system.as_ref().unwrap().categories.iter().flat_map(|c|c.metrics.clone()).collect();refresh_points(&mut s);
    let root=std::env::temp_dir().join(format!("aurelia-v3-export-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let entry=store::save(&root,&s).unwrap();
    let original=std::fs::read(root.join(&entry.id).join("result.json")).unwrap();
    let export=root.join("export.gsbench");store::export(&root,&entry.id,&export).unwrap();
    assert_eq!(std::fs::read(&export).unwrap(),original);
    assert_eq!(store::import(&root,&export).unwrap().id,entry.id);
    assert!(store::load(&root,&entry.id).unwrap().system.is_some());
    assert_eq!(std::fs::read(root.join(&entry.id).join("result.json")).unwrap(),original);
    std::fs::remove_dir_all(root).unwrap();
}
#[test] fn absolute_scores_double_without_reference_or_ceiling() {
    let a=storage_fixture();a.validate().unwrap();
    let mut b=storage_fixture();
    for c in &mut b.system.as_mut().unwrap().categories { for m in &mut c.metrics { for value in &mut m.values {*value*=2.;} } }
    b.metrics=b.system.as_ref().unwrap().categories.iter().flat_map(|c|c.metrics.clone()).collect();
    refresh_points(&mut b);b.validate().unwrap();
    assert!((system::scoring::calculate(&b).global.unwrap()/system::scoring::calculate(&a).global.unwrap()-2.).abs()<1e-12);
}
#[test] fn missing_storage_never_creates_global_and_old_protocol_stays_separate() {
    let mut s=storage_fixture();s.system.as_mut().unwrap().categories.last_mut().unwrap().state=State::Unavailable;
    refresh_points(&mut s);s.validate().unwrap();assert!(system::scoring::calculate(&s).global.is_none());
    assert!(reporting::compare(&fixture(),&s).is_err());
}
#[test] fn scores_are_derived_without_mutating_original_measurements() {
    let s=storage_fixture();let before=serde_json::to_vec(&s).unwrap();
    let _=system::scoring::calculate(&s);assert_eq!(before,serde_json::to_vec(&s).unwrap());
    let mut corrupt=storage_fixture();corrupt.system.as_mut().unwrap().parameters["rate_scores"]["global"]=999999.into();assert!(corrupt.validate().is_err());
}
#[test] fn workload_changes_disable_points_comparison() {
    let a=storage_fixture();let mut b=storage_fixture();
    assert!(system::scoring::compatible(&a,&b,"storage",true));
    for m in &mut b.metrics {if m.category=="storage"{m.work_description="different block size".into();}}
    assert!(!system::scoring::compatible(&a,&b,"storage",true));
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
