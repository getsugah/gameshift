//! Fixed work after calibration. Probe/preparation never enter measured repetitions.
use crate::{
    runner::{sample_bytes, Case, Config, Report},
    topology::Topology,
    workload::{StepTiming, Workload},
};
use gameshift_core::duration::{publish, BenchmarkProgress, DurationProfile, ProgressSink};
use serde::{Deserialize, Serialize};
use std::{
    mem::size_of,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CpuDuration {
    pub profile: DurationProfile,
    pub target_ms: u64,
    pub preparation_ms: u64,
}
impl CpuDuration {
    pub fn preset(profile: DurationProfile) -> Self {
        let (target_ms, preparation_ms, _) = profile.cpu();
        Self {
            profile,
            target_ms,
            preparation_ms,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Calibration {
    pub measured_steps: usize,
    pub preparation_steps: usize,
    pub probe_steps: usize,
    pub probe_compute_ms: f64,
    pub target_ms: u64,
    pub predicted_measurement_ms: f64,
    pub preparation_actual_ms: Vec<f64>,
    pub measurement_wall_ms: Vec<f64>,
    pub detailed_job_interval_ms: u64,
    pub method: String,
}
fn checked(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        Err("Test annulé ; aucun résultat partiel conservé".into())
    } else {
        Ok(())
    }
}
fn pool(workers: usize) -> Result<rayon::ThreadPool, String> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .thread_name(|i| format!("gameshift-cpu-{i}"))
        .build()
        .map_err(|e| e.to_string())
}
fn notify(
    sink: &ProgressSink,
    start: Instant,
    phase: &str,
    repetition: (usize, usize),
    position: (usize, usize),
    fraction: f64,
    remaining: Option<f64>,
) {
    let (rep, reps) = repetition;
    let (index, cases) = position;
    publish(
        sink,
        BenchmarkProgress {
            phase: phase.into(),
            repetition: rep,
            repetitions: reps,
            case_index: index,
            cases,
            fraction,
            elapsed_seconds: start.elapsed().as_secs_f64(),
            remaining_seconds: remaining,
            remaining_scope: "répétition en cours".into(),
            fps: None,
            cpu_ms: None,
            gpu_ms: None,
        },
    );
}
pub fn run(config: Config, cancel: &AtomicBool, sink: &ProgressSink) -> Result<Report, String> {
    let duration = config.duration.clone().ok_or("Durée calibrée absente")?;
    let topology = Topology::detect();
    if !(100..=60_000).contains(&duration.target_ms)
        || duration.preparation_ms > 10_000
        || !(2..=20).contains(&config.repetitions)
        || config.warmup_steps > 1000
        || !(1..=1000).contains(&config.measured_steps)
        || config.workers.is_empty()
        || config.workers.len() > 16
        || config
            .workers
            .iter()
            .any(|n| *n == 0 || *n > topology.allowed_logical_cpus.len())
        || config.sizes_mib.is_empty()
        || config.sizes_mib.len() > 16
        || config.sizes_mib.iter().any(|n| !(1..=256).contains(n))
        || !(1..=1_000_000_000).contains(&config.fixed_step_ns)
    {
        return Err("Paramètres du protocole calibré hors limites".into());
    }
    let start = Instant::now();
    let total_cases = config.workers.len() * config.sizes_mib.len();
    let mut cases = Vec::<Case>::new();
    let mut retained = 0u64;
    let mut warnings=vec!["Calibration et préparation exclues des mesures. Même nombre d’étapes et même préparation pour tous les nombres de threads d’une taille ; le plus rapide détermine le travail commun. Les configurations plus lentes peuvent dépasser la durée cible.".into(),"Le protocole calibré réutilise ses buffers de chronométrage. Les détails par bloc/thread sont conservés par instantanés espacés d’au moins 250 ms ; toutes les étapes et durées totales sont conservées. La copie des détails et les notifications sont hors du chronométrage d’une étape, mais incluses dans la durée murale du passage.".into(),"Les compteurs matériels activés ajoutent leur propre coût. Des mesures plus longues ne suppriment pas les effets des fréquences, températures et activités concurrentes.".into()];
    for &size in &config.sizes_mib {
        checked(cancel)?;
        let mut plans = Vec::new();
        let mut common_steps = 1usize;
        let mut preparation_steps = config.warmup_steps;
        for &workers in &config.workers {
            notify(
                sink,
                start,
                "calibration",
                (0, config.repetitions),
                (cases.len() + 1, total_cases),
                0.,
                None,
            );
            let pool = pool(workers)?;
            let mut work = Workload::new(size * 1048576, config.seed, config.pattern)?;
            let mut scratch = work.scratch();
            let prime = work.step_reusing(
                &pool,
                config.pattern,
                config.fixed_step_ns,
                config.hardware_counters,
                scratch,
            );
            scratch = prime.jobs;
            let mut ns = 0u64;
            let mut count = 0usize;
            while ns < 50_000_000 && count < 10_000 {
                checked(cancel)?;
                let sample = work.step_reusing(
                    &pool,
                    config.pattern,
                    config.fixed_step_ns,
                    config.hardware_counters,
                    scratch,
                );
                ns = ns.saturating_add(sample.critical_wall_ns);
                scratch = sample.jobs;
                count += 1;
            }
            let per_step = ns as f64 / count.max(1) as f64;
            let steps = (duration.target_ms as f64 * 1e6 / per_step * 1.10)
                .ceil()
                .max(1.) as usize;
            if steps > 200_000 {
                return Err("Calibration trop rapide pour le budget d’échantillons ; augmentez la taille des données ou réduisez la durée cible".into());
            }
            common_steps = common_steps.max(steps);
            preparation_steps = preparation_steps
                .max((duration.preparation_ms as f64 * 1e6 / per_step).ceil() as usize);
            plans.push((workers, per_step, count, ns));
        }
        // One fixed count for comparable workers; never adapt during a measured repetition.
        let estimated = common_steps as u64
            * config.repetitions as u64
            * plans.len() as u64
            * size_of::<StepTiming>() as u64;
        if retained.saturating_add(estimated) > 512 * 1024 * 1024 {
            return Err("Résultats calibrés trop volumineux : réduire la durée cible ou le nombre de configurations".into());
        }
        for (workers, per_step, probe_steps, probe_ns) in plans {
            checked(cancel)?;
            let pool = pool(workers)?;
            let mut work = Workload::new(size * 1048576, config.seed, config.pattern)?;
            let footprint = work.footprint();
            let mut scratch = work.scratch();
            let before = crate::environment::Snapshot::read(&topology.allowed_logical_cpus);
            let mut repetitions = Vec::with_capacity(config.repetitions);
            let mut prep_times = Vec::new();
            let mut wall_times = Vec::new();
            for rep in 0..config.repetitions {
                checked(cancel)?;
                work.reset(config.seed, config.pattern)?;
                // Commit trace storage before preparation, not while measuring.
                // These empty placeholders are cleared and never exported as samples.
                let mut samples = (0..common_steps)
                    .map(|_| StepTiming {
                        critical_wall_ns: 0,
                        cumulative_worker_cpu_ns: None,
                        coordinator_cpu_ns: None,
                        synchronization_tail_ns: 0,
                        jobs: vec![],
                        checksum: 0,
                        entities: 0,
                        visible_entities: 0,
                        logical_read_write_bytes: 0,
                    })
                    .collect::<Vec<_>>();
                samples.clear();
                let prep = Instant::now();
                let mut last_update = Instant::now() - Duration::from_secs(1);
                for step in 0..preparation_steps {
                    checked(cancel)?;
                    if last_update.elapsed() >= Duration::from_millis(250) {
                        notify(
                            sink,
                            start,
                            "préparation",
                            (rep + 1, config.repetitions),
                            (cases.len() + 1, total_cases),
                            step as f64 / preparation_steps.max(1) as f64,
                            None,
                        );
                        last_update = Instant::now();
                    }
                    scratch = work
                        .step_reusing(
                            &pool,
                            config.pattern,
                            config.fixed_step_ns,
                            config.hardware_counters,
                            scratch,
                        )
                        .jobs;
                }
                prep_times.push(prep.elapsed().as_secs_f64() * 1000.);
                let wall = Instant::now();
                let mut details = Instant::now() - Duration::from_secs(1);
                for step in 0..common_steps {
                    checked(cancel)?;
                    if last_update.elapsed() >= Duration::from_millis(250) {
                        let fraction = step as f64 / common_steps as f64;
                        let remaining = if step > 0 {
                            Some(
                                wall.elapsed().as_secs_f64() * (common_steps - step) as f64
                                    / step as f64,
                            )
                        } else {
                            None
                        };
                        notify(
                            sink,
                            start,
                            "mesure",
                            (rep + 1, config.repetitions),
                            (cases.len() + 1, total_cases),
                            fraction,
                            remaining,
                        );
                        if let Ok(mut slot) = sink.lock() {
                            if let Some(p) = slot.as_mut() {
                                let count = samples.len().min(32);
                                if count > 0 {
                                    p.cpu_ms = Some(
                                        samples
                                            .iter()
                                            .rev()
                                            .take(count)
                                            .map(|s| s.critical_wall_ns as f64 / 1e6)
                                            .sum::<f64>()
                                            / count as f64,
                                    );
                                }
                            }
                        }
                        last_update = Instant::now();
                    }
                    let mut sample = work.step_reusing(
                        &pool,
                        config.pattern,
                        config.fixed_step_ns,
                        config.hardware_counters,
                        scratch,
                    );
                    scratch = std::mem::take(&mut sample.jobs);
                    if details.elapsed() >= Duration::from_millis(250) {
                        sample.jobs = scratch.clone();
                        details = Instant::now();
                    }
                    samples.push(sample);
                }
                wall_times.push(wall.elapsed().as_secs_f64() * 1000.);
                repetitions.push(samples);
            }
            notify(
                sink,
                start,
                "analyse",
                (config.repetitions, config.repetitions),
                (cases.len() + 1, total_cases),
                1.,
                None,
            );
            let means = repetitions
                .iter()
                .map(|s| {
                    s.iter()
                        .map(|s| s.critical_wall_ns as f64 / 1e6)
                        .sum::<f64>()
                        / s.len() as f64
                })
                .collect::<Vec<_>>();
            let mean = means.iter().sum::<f64>() / means.len() as f64;
            let cv = (means.iter().map(|n| (n - mean).powi(2)).sum::<f64>() / means.len() as f64)
                .sqrt()
                / mean;
            let inconclusive = config.repetitions < 3
                || cv > 0.05
                || means.iter().any(|n| n * (common_steps as f64) < 100.);
            if means
                .iter()
                .any(|n| n * (common_steps as f64) < (duration.target_ms as f64) * 0.9)
            {
                warnings.push(format!("{size} Mio / {workers} threads : au moins une répétition sous 90 % de la durée cible après calibration ; durée réelle conservée, refaire une calibration en conditions plus stables."));
            }
            let entities = repetitions[0][0].entities;
            let sample_bytes = sample_bytes(&repetitions);
            retained += sample_bytes;
            if retained > 512 * 1024 * 1024 {
                return Err(
                    "Budget des échantillons calibrés dépassé ; aucun résultat partiel publié"
                        .into(),
                );
            }
            cases.push(Case {
                workers,
                size_mib: size,
                footprint,
                retained_sample_allocation_bytes: sample_bytes,
                process_rss_after_bytes: std::fs::read_to_string("/proc/self/status")
                    .ok()
                    .and_then(|s| {
                        s.lines().find_map(|line| {
                            line.strip_prefix("VmRSS:").and_then(|v| {
                                v.split_whitespace()
                                    .next()?
                                    .parse::<u64>()
                                    .ok()?
                                    .checked_mul(1024)
                            })
                        })
                    }),
                environment: crate::environment::Environment::finish(
                    before,
                    &topology.allowed_logical_cpus,
                ),
                repetitions,
                mean_step_ms: mean,
                repetition_cv: cv,
                entities_per_second: entities as f64 * 1000. / mean,
                speedup: None,
                parallel_efficiency: None,
                inconclusive,
                calibration: Some(Calibration {
                    measured_steps: common_steps,
                    preparation_steps,
                    probe_steps,
                    probe_compute_ms: probe_ns as f64 / 1e6,
                    target_ms: duration.target_ms,
                    predicted_measurement_ms: per_step * common_steps as f64 / 1e6,
                    preparation_actual_ms: prep_times,
                    measurement_wall_ms: wall_times,
                    detailed_job_interval_ms: 250,
                    method: "fastest-worker-count-per-size-fixed-work-10-percent-margin".into(),
                }),
            });
        }
    }
    for i in 0..cases.len() {
        if let Some(base) = cases
            .iter()
            .find(|b| b.workers == 1 && b.size_mib == cases[i].size_mib)
        {
            if !base
                .repetitions
                .iter()
                .flatten()
                .map(|s| s.checksum)
                .eq(cases[i].repetitions.iter().flatten().map(|s| s.checksum))
            {
                return Err(
                    "Calcul non déterministe entre nombres de threads ; résultats refusés".into(),
                );
            }
            let speed = base.mean_step_ms / cases[i].mean_step_ms;
            let uncertain = base.inconclusive || cases[i].inconclusive;
            cases[i].speedup = Some(speed);
            cases[i].parallel_efficiency = Some(speed / cases[i].workers as f64);
            cases[i].inconclusive = uncertain;
        }
    }
    Ok(Report {
        schema_version: 2,
        protocol: "aurelia-cpu-calibrated-v2".into(),
        protocol_version: 2,
        engine_version: format!(
            "gameshift-cpu-{}-instrumentation-2",
            env!("CARGO_PKG_VERSION")
        ),
        config,
        topology,
        cases,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn calibration_uses_fixed_comparable_work_and_resets_each_repetition() {
        let allowed = Topology::detect().allowed_logical_cpus.len();
        let config = Config {
            workers: [1, 2].into_iter().filter(|n| *n <= allowed).collect(),
            sizes_mib: vec![1],
            pattern: crate::workload::Pattern::Sequential,
            repetitions: 2,
            duration: Some(CpuDuration {
                profile: DurationProfile::Custom,
                target_ms: 100,
                preparation_ms: 0,
            }),
            ..Config::default()
        };
        let report = run(config, &AtomicBool::new(false), &Default::default()).unwrap();
        assert_eq!(report.schema_version, 2);
        let reference = &report.cases[0];
        for case in &report.cases {
            let cal = case.calibration.as_ref().unwrap();
            assert!(cal.probe_steps > 0);
            assert!(cal.measured_steps > 1);
            assert_eq!(
                cal.measured_steps,
                reference.calibration.as_ref().unwrap().measured_steps
            );
            for samples in &case.repetitions {
                assert_eq!(samples.len(), cal.measured_steps);
                assert!(samples
                    .iter()
                    .map(|s| s.checksum)
                    .eq(reference.repetitions[0].iter().map(|s| s.checksum)));
            }
        }
    }
    #[test]
    fn cancellation_during_calibration_or_measurement_discards_report() {
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let sink: ProgressSink = Default::default();
        let observed = sink.clone();
        let worker = std::thread::spawn(move || {
            run(
                Config {
                    workers: vec![1],
                    sizes_mib: vec![1],
                    duration: Some(CpuDuration::preset(DurationProfile::Quick)),
                    ..Config::default()
                },
                &flag,
                &observed,
            )
        });
        let timeout = Instant::now();
        loop {
            if sink
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|p| p.phase == "mesure")
            {
                cancel.store(true, Ordering::Relaxed);
                break;
            }
            if timeout.elapsed() > Duration::from_secs(5) {
                cancel.store(true, Ordering::Relaxed);
                panic!("La phase de mesure n’a pas démarré");
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(worker.join().unwrap().unwrap_err().contains("annulé"));
    }
}
