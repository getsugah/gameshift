use gameshift_cpu::{
    runner::{run, Config},
    workload::{Pattern, Workload},
};

#[test]
fn identical_work_with_different_worker_counts() {
    for pattern in [
        Pattern::Sequential,
        Pattern::Random,
        Pattern::Dependent,
        Pattern::Entities,
        Pattern::Spatial,
        Pattern::Mixed,
    ] {
        let mut reference = None;
        for workers in [1, 2, 4] {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(workers)
                .build()
                .unwrap();
            let mut work = Workload::new(1024 * 1024, 42, pattern).unwrap();
            let results = (0..3)
                .map(|_| {
                    let sample = work.step(&pool, pattern, 16_666_667, false);
                    assert!(sample.critical_wall_ns > 0);
                    assert!(sample
                        .jobs
                        .iter()
                        .all(|j| j.completed_offset_ns <= sample.critical_wall_ns));
                    (sample.checksum, sample.visible_entities)
                })
                .collect::<Vec<_>>();
            let packets = work
                .packets()
                .iter()
                .flat_map(|p| p.transform.iter().chain(&p.color))
                .map(|v| v.to_bits())
                .collect::<Vec<_>>();
            let result = (results, packets);
            if let Some(ref expected) = reference {
                assert_eq!(&result, expected, "{pattern:?}");
            } else {
                reference = Some(result);
            }
        }
    }
}

#[test]
fn footprint_counts_spatial_storage_and_double_buffering() {
    let flat = Workload::new(1024 * 1024, 42, Pattern::Entities)
        .unwrap()
        .footprint();
    let spatial = Workload::new(1024 * 1024, 42, Pattern::Spatial)
        .unwrap()
        .footprint();
    assert!(flat.state_allocated_bytes >= flat.state_logical_bytes);
    assert_eq!(flat.spatial_allocated_bytes, 0);
    assert!(spatial.spatial_allocated_bytes > 0);
    assert_eq!(
        spatial.persistent_allocated_bytes,
        spatial.state_allocated_bytes
            + spatial.render_allocated_bytes
            + spatial.spatial_allocated_bytes
            + spatial.container_metadata_bytes
    );
    assert!(spatial.allocator_bookkeeping_bytes.is_none());
}

#[test]
fn runner_rejects_invalid_config_before_allocation() {
    let config = Config {
        workers: vec![0],
        ..Config::default()
    };
    assert!(run(config).is_err());
}

#[test]
fn cancellation_never_returns_a_complete_report() {
    let cancel = std::sync::atomic::AtomicBool::new(true);
    assert!(gameshift_cpu::runner::run_cancellable(Config::default(), &cancel).is_err());
}

#[test]
fn repeated_fixed_work_has_same_checksum() {
    let config = Config {
        sizes_mib: vec![1],
        repetitions: 3,
        measured_steps: 2,
        warmup_steps: 1,
        ..Config::default()
    };
    let report = run(config).unwrap();
    let case = &report.cases[0];
    assert_eq!(case.speedup, Some(1.));
    for repetition in &case.repetitions[1..] {
        assert_eq!(
            repetition.iter().map(|s| s.checksum).collect::<Vec<_>>(),
            case.repetitions[0]
                .iter()
                .map(|s| s.checksum)
                .collect::<Vec<_>>()
        );
    }
    let encoded = serde_json::to_string(&report).unwrap();
    let decoded: gameshift_cpu::runner::Report = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.protocol, "aurelia-cpu-fixed-work-v1");
    assert!(decoded.text().contains("graine :"));
}
