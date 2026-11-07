//! Fixed-work runs. Allocation, initialization and pool creation are outside samples.
use crate::{
    topology::Topology,
    workload::{Footprint, Pattern, StepTiming, Workload},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub workers: Vec<usize>,
    pub sizes_mib: Vec<u64>,
    pub pattern: Pattern,
    pub seed: u64,
    pub warmup_steps: usize,
    pub measured_steps: usize,
    pub repetitions: usize,
    pub fixed_step_ns: u64,
    pub hardware_counters: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<crate::calibrated::CpuDuration>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            workers: vec![1],
            sizes_mib: vec![4],
            pattern: Pattern::Entities,
            seed: 0x415552454c494134,
            warmup_steps: 3,
            measured_steps: 20,
            repetitions: 3,
            fixed_step_ns: 16_666_667,
            hardware_counters: false,
            duration: None,
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Case {
    pub workers: usize,
    pub size_mib: u64,
    pub footprint: Footprint,
    pub retained_sample_allocation_bytes: u64,
    pub process_rss_after_bytes: Option<u64>,
    pub environment: crate::environment::Environment,
    pub repetitions: Vec<Vec<StepTiming>>,
    pub mean_step_ms: f64,
    pub repetition_cv: f64,
    pub entities_per_second: f64,
    pub speedup: Option<f64>,
    pub parallel_efficiency: Option<f64>,
    pub inconclusive: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibration: Option<crate::calibrated::Calibration>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: u32,
    pub protocol: String,
    #[serde(default)]
    pub protocol_version: u32,
    pub config: Config,
    pub topology: Topology,
    pub cases: Vec<Case>,
    pub warnings: Vec<String>,
    #[serde(default)]
    pub engine_version: String,
}
impl Report {
    /// Human-readable report; JSON and CSV retain the existing schema.
    pub fn text(&self) -> String {
        crate::reporting::format(self)
    }
}

pub fn run(config: Config) -> Result<Report, String> {
    run_cancellable(config, &std::sync::atomic::AtomicBool::new(false))
}
/// Cancellation is checked between steps; partial runs are never returned as complete.
pub fn run_cancellable(
    config: Config,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<Report, String> {
    if config.duration.is_some() {
        return crate::calibrated::run(config, cancel, &Default::default());
    }
    let topology = Topology::detect();
    if config.workers.is_empty()
        || config.sizes_mib.is_empty()
        || config.workers.len() > 16
        || config.sizes_mib.len() > 16
        || config
            .workers
            .iter()
            .any(|n| *n == 0 || *n > topology.allowed_logical_cpus.len())
        || config.sizes_mib.iter().any(|n| !(1..=256).contains(n))
        || !(1..=1000).contains(&config.measured_steps)
        || !(1..=20).contains(&config.repetitions)
        || config.warmup_steps > 1000
        || !(1..=1_000_000_000).contains(&config.fixed_step_ns)
    {
        return Err("Configuration hors limites (workers autorisés, 1–256 MiB, 1–1000 pas, 1–20 répétitions)".into());
    }
    let mut cases = Vec::<Case>::new();
    // Conservative admission budget for retained raw results, not a promise of RSS.
    let blocks: u64 = config
        .sizes_mib
        .iter()
        .map(|mib| (mib * 1_048_576 / 208).div_ceil(2048))
        .sum();
    let estimated = blocks
        * config.workers.len() as u64
        * config.repetitions as u64
        * config.measured_steps as u64
        * if config.hardware_counters { 2048 } else { 1024 };
    if estimated > 512 * 1024 * 1024 {
        return Err("Échantillons bruts trop volumineux (budget prudent 512 Mio) : réduire les pas, tailles ou paliers".into());
    }
    for &size in &config.sizes_mib {
        for &workers in &config.workers {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(workers)
                .thread_name(|i| format!("gameshift-cpu-{i}"))
                .build()
                .map_err(|e| e.to_string())?;
            let mut workload = Workload::new(size * 1024 * 1024, config.seed, config.pattern)?;
            let footprint = workload.footprint();
            let environment_before =
                crate::environment::Snapshot::read(&topology.allowed_logical_cpus);
            let mut repetitions = Vec::new();
            for _ in 0..config.repetitions {
                workload.reset(config.seed, config.pattern)?;
                for _ in 0..config.warmup_steps {
                    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                        return Err("Test annulé".into());
                    }
                    workload.step(&pool, config.pattern, config.fixed_step_ns, false);
                }
                let samples = (0..config.measured_steps)
                    .map(|_| {
                        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                            return Err("Test annulé".to_string());
                        }
                        Ok(workload.step(
                            &pool,
                            config.pattern,
                            config.fixed_step_ns,
                            config.hardware_counters,
                        ))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                repetitions.push(samples);
            }
            let means = repetitions
                .iter()
                .map(|samples| {
                    samples
                        .iter()
                        .map(|s| s.critical_wall_ns as f64 / 1e6)
                        .sum::<f64>()
                        / samples.len() as f64
                })
                .collect::<Vec<_>>();
            let mean = means.iter().sum::<f64>() / means.len() as f64;
            let cv = (means.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / means.len() as f64)
                .sqrt()
                / mean;
            let entities = repetitions[0][0].entities;
            let retained_sample_allocation_bytes = sample_bytes(&repetitions);
            let process_rss_after_bytes = std::fs::read_to_string("/proc/self/status")
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
                });
            cases.push(Case {
                workers,
                size_mib: size,
                footprint,
                calibration: None,
                retained_sample_allocation_bytes,
                process_rss_after_bytes,
                environment: crate::environment::Environment::finish(
                    environment_before,
                    &topology.allowed_logical_cpus,
                ),
                repetitions,
                mean_step_ms: mean,
                repetition_cv: cv,
                entities_per_second: entities as f64 * 1000. / mean,
                speedup: None,
                parallel_efficiency: None,
                inconclusive: config.repetitions < 3
                    || cv > 0.05
                    || means
                        .iter()
                        .any(|mean| mean * (config.measured_steps as f64) < 100.),
            });
        }
    }
    // A baseline is valid only for identical data and protocol; order need not start at 1.
    for i in 0..cases.len() {
        if let Some(base) = cases
            .iter()
            .find(|c| c.workers == 1 && c.size_mib == cases[i].size_mib)
        {
            let same_work = base
                .repetitions
                .iter()
                .flatten()
                .map(|s| s.checksum)
                .eq(cases[i].repetitions.iter().flatten().map(|s| s.checksum));
            if !same_work {
                return Err("Résultats déterministes divergents entre nombres de workers".into());
            }
            let speedup = base.mean_step_ms / cases[i].mean_step_ms;
            let inconclusive = base.inconclusive || cases[i].inconclusive;
            cases[i].speedup = Some(speedup);
            cases[i].parallel_efficiency = Some(speedup / cases[i].workers as f64);
            cases[i].inconclusive = inconclusive;
        }
    }
    Ok(Report { engine_version: "gameshift-cpu-instrumentation-2".into(), schema_version: 1, protocol_version:1, protocol: "aurelia-cpu-fixed-work-v1".into(), config, topology, cases,
        warnings: vec!["Prototype CPU indépendant ; aucune mesure GPU et aucune comparaison directe avec Aurelia v3.".into(),
            "CV > 5 %, moins de trois répétitions ou une répétition mesurée < 100 ms : résultat non concluant ; aucun test statistique de gain implémenté.".into(),
            "Empreinte des buffers comptabilisée ; allocator, Rayon et échantillons conservés ajoutent de la mémoire hors de cette empreinte.".into(),
            "Compteurs cache génériques : ne représentent pas des événements L3 spécifiques. Instrumentation incluse dans le temps critique.".into()] })
}

pub(crate) fn sample_bytes(repetitions: &Vec<Vec<StepTiming>>) -> u64 {
    use std::mem::size_of;
    let mut bytes = repetitions.capacity() * size_of::<Vec<StepTiming>>();
    for samples in repetitions {
        bytes += samples.capacity() * size_of::<StepTiming>();
        for step in samples {
            bytes += step.jobs.capacity() * size_of::<crate::workload::JobTiming>();
            for job in &step.jobs {
                bytes += job.phases.capacity() * size_of::<crate::instrument::PhaseTiming>();
                bytes += job.phases.iter().map(|p| p.phase.capacity()).sum::<usize>();
                bytes += job.counters.capacity() * size_of::<crate::instrument::CounterSample>();
                bytes += job
                    .counters
                    .iter()
                    .map(|c| {
                        c.event.capacity()
                            + c.unavailable_reason.as_ref().map_or(0, |s| s.capacity())
                    })
                    .sum::<usize>();
            }
        }
    }
    bytes as u64
}
