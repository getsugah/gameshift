//! Synthetic fixtures verify arithmetic and rejection paths, never exported as measurements.
use gameshift_system_bench::{
    comparison::compare,
    session::{Fingerprint, Metric, Session, SystemKind, SESSION_PROTOCOL},
    store,
};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
fn fixture() -> Session {
    Session {
        system: None,
        schema_version: 1,
        protocol: SESSION_PROTOCOL.into(),
        application_version: "test".into(),
        completed_unix_seconds: 1,
        kind: SystemKind::Original,
        fingerprint: Fingerprint {
            distribution: "A".into(),
            distribution_id: "a".into(),
            kernel: Some("kernel-a".into()),
            architecture: "x86_64".into(),
            cpu_model: Some("CPU fixture".into()),
            microcode: None,
            logical_cpus: vec![0, 1],
            physical_cores: Some(1),
            ram_kib: Some(1024),
            pci_gpus: vec![],
            governors: BTreeMap::new(),
            energy_preferences: BTreeMap::new(),
            engine_sha256: "a".repeat(64),
            helper_sha256: "b".repeat(64),
        },
        elapsed_seconds: 1.,
        metrics: ["cpu", "cache", "glibc"]
            .into_iter()
            .map(|category| Metric {
                id: category.into(),
                category: category.into(),
                label: category.into(),
                unit: "op/s".into(),
                values: vec![100.; 3],
                confirmation_reasons: vec![],
                work_description: "fixed".into(),
            })
            .collect(),
        cpu_raw: vec![],
        glibc_raw: serde_json::json!({"glibc_version":"fixture"}),
        limitations: vec![],
    }
}
fn directory() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "gameshift-distribution-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&path).unwrap();
    path
}
#[test]
fn relative_indices_and_reciprocal_winners() {
    let a = fixture();
    let mut b = fixture();
    b.fingerprint.distribution = "B".into();
    for metric in &mut b.metrics {
        metric.values = vec![110.; 3];
    }
    let c = compare(&a, &b).unwrap();
    assert!((c.synthesis_index - 110.).abs() < 1e-8);
    assert!(!c.confirmation_required);
    assert!(c.text().contains("B en tête de 10.0 %"));
    let back = compare(&b, &a).unwrap();
    assert!((back.synthesis_index - 10000. / 110.).abs() < 1e-8);
    assert!(back.text().contains("B en tête de 10.0 %"));
}
#[test]
fn equal_scores_and_variation_are_not_declared_clear_wins() {
    let a = fixture();
    assert!(compare(&a, &a).unwrap().text().contains("Égalité"));
    let mut b = fixture();
    b.metrics[0].values = vec![95., 101., 107.];
    let c = compare(&a, &b).unwrap();
    assert!(c.confirmation_required);
    assert!(c.text().contains("Écart à confirmer"));
}
#[test]
fn incompatible_hardware_binary_protocol_or_coverage_are_refused() {
    let a = fixture();
    let mut b = fixture();
    b.fingerprint.cpu_model = Some("different".into());
    assert!(compare(&a, &b).is_err());
    b = fixture();
    b.fingerprint.engine_sha256 = "c".repeat(64);
    assert!(compare(&a, &b).is_err());
    b = fixture();
    b.protocol = "different".into();
    assert!(compare(&a, &b).is_err());
    b = fixture();
    b.metrics.pop();
    assert!(compare(&a, &b).is_err());
    b = fixture();
    b.metrics[0].values[0] = f64::NAN;
    assert!(compare(&a, &b).is_err());
}
#[test]
fn fixed_category_weights_preserve_native_application_separation() {
    let a = fixture();
    let mut b = fixture();
    b.metrics[2].values = vec![400.; 3];
    let c = compare(&a, &b).unwrap();
    assert!((c.native_partial_index - 100.).abs() < 1e-8);
    assert!((c.application_index - 400.).abs() < 1e-8);
    assert!((c.synthesis_index - 200.).abs() < 1e-8);
}
#[test]
fn immutable_history_roundtrip_references_and_non_overwriting_export() {
    let root = directory();
    let session = fixture();
    let a = store::save(&root, &session).unwrap();
    let before = fs::read(root.join(&a.id).join("result.json")).unwrap();
    assert_eq!(store::save(&root, &session).unwrap().id, a.id);
    assert_eq!(store::list(&root).unwrap().len(), 1);
    store::reference(&root, &a.id).unwrap();
    store::reference(&root, &a.id).unwrap();
    let mut other = fixture();
    other.completed_unix_seconds = 2;
    let b = store::save(&root, &other).unwrap();
    store::reference(&root, &b.id).unwrap();
    assert_eq!(
        fs::read(root.join(&a.id).join("result.json")).unwrap(),
        before
    );
    let export = root.join("export.json");
    store::export(&root, &a.id, &export).unwrap();
    assert!(store::export(&root, &a.id, &export).is_err());
    assert_eq!(fs::read(&export).unwrap(), before);
    assert_eq!(store::import(&root, &export).unwrap().id, a.id);
    assert!(store::load(&root, "../escape").is_err());
    fs::write(root.join(&a.id).join("result.json"), b"{}").unwrap();
    assert!(store::load(&root, &a.id).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn parent_death_helper_worker() {
    let Some(path) = std::env::var_os("GAMESHIFT_TEST_PDEATH_FILE") else {
        return;
    };
    let child = gameshift_system_bench::host_command(std::path::Path::new("/bin/sleep"))
        .arg("30")
        .spawn()
        .unwrap();
    fs::write(path, child.id().to_string()).unwrap();
    // This branch runs only in the dedicated subprocess of the following test.
    std::process::exit(0);
}
#[test]
fn auxiliary_stops_when_its_parent_exits_abruptly() {
    let root = directory();
    let pidfile = root.join("pid");
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "parent_death_helper_worker"])
        .env("GAMESHIFT_TEST_PDEATH_FILE", &pidfile)
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    let pid = fs::read_to_string(pidfile).unwrap();
    let mut alive = true;
    for _ in 0..100 {
        let status = fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
        alive = !(status.is_empty()
            || status
                .lines()
                .any(|s| s.starts_with("State:") && s.contains('Z')));
        if !alive {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(!alive, "helper survived parent exit");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_export_and_archive_preserve_original_bytes_and_references() {
    let root = directory();
    let session = fixture();
    let entry = store::save(&root, &session).unwrap();
    store::reference(&root, &entry.id).unwrap();
    let target = root.join("copy.gsbench");
    store::export(&root, &entry.id, &target).unwrap();
    let original = fs::read(root.join(&entry.id).join("result.json")).unwrap();
    assert_eq!(original, fs::read(&target).unwrap());
    assert_eq!(store::import(&root, &target).unwrap().id, entry.id);
    assert_eq!(store::list(&root).unwrap().len(), 1);
    assert!(store::archive(&root, "../other").is_err());
    store::archive(&root, &entry.id).unwrap();
    assert!(store::list(&root).unwrap().is_empty());
    assert_eq!(
        fs::read(root.join("removed").join(&entry.id).join("result.json")).unwrap(),
        original
    );
    assert!(root
        .join("references")
        .join(format!("{}.json", entry.id))
        .is_file());
    fs::remove_dir_all(root).unwrap();
}
