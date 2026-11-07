//! Canonical 64-step epochs, useful simulation work, persistent Rayon pool.
use super::{metric, Category, State};
use gameshift_cpu::workload::{Pattern, Workload};
use serde::Serialize;
use std::time::{Duration, Instant};
const SEED: u64 = 0x415552454c494132;
#[derive(Debug, Serialize)]
pub struct Sample {
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub elapsed_seconds: f64,
    pub steps: u64,
    pub batches: u64,
    pub critical_seconds: f64,
    pub steps_per_second: f64,
    pub checksum: u64,
    pub verified: bool,
    pub windows: Vec<(f64, u64)>,
}
pub struct Load {
    baseline: Workload,
    work: Workload,
    pool: rayon::ThreadPool,
    scratch: Vec<gameshift_cpu::workload::JobTiming>,
    golden: u64,
}
impl Load {
    pub fn new(workers: usize) -> Result<Self, String> {
        let baseline = Workload::new(4 * 1048576, SEED, Pattern::Entities)?;
        let work = Workload::new(4 * 1048576, SEED, Pattern::Entities)?;
        let scratch = work.scratch();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
            .build()
            .map_err(|e| e.to_string())?;
        let mut load = Self {
            baseline,
            work,
            pool,
            scratch,
            golden: 0,
        };
        load.golden = load.epoch()?.0;
        Ok(load)
    }
    fn epoch(&mut self) -> Result<(u64, u64), String> {
        self.work.restore_template(&self.baseline)?;
        let mut checksum = 0;
        let mut critical = 0;
        for _ in 0..64 {
            let sample = self.work.step_reusing(
                &self.pool,
                Pattern::Entities,
                16_666_667,
                false,
                std::mem::take(&mut self.scratch),
            );
            checksum = sample.checksum;
            critical += sample.critical_wall_ns;
            self.scratch = sample.jobs;
        }
        Ok((checksum, critical))
    }
    pub fn sample(&mut self, duration: Duration, origin: Instant) -> Result<Sample, String> {
        let start = Instant::now();
        let mut batches = 0;
        let mut critical = 0;
        let mut windows = Vec::with_capacity(64);
        let mut next = Duration::from_millis(500);
        while start.elapsed() < duration {
            let (sum, ns) = self.epoch()?;
            if sum != self.golden {
                return Err("Empreinte CPU déterministe différente".into());
            }
            critical += ns;
            batches += 1;
            if start.elapsed() >= next {
                windows.push((start.elapsed().as_secs_f64(), batches * 64));
                next += Duration::from_millis(500);
            }
        }
        let end = Instant::now();
        let elapsed = end.duration_since(start).as_secs_f64();
        Ok(Sample {
            start_seconds: start.duration_since(origin).as_secs_f64(),
            end_seconds: end.duration_since(origin).as_secs_f64(),
            elapsed_seconds: elapsed,
            steps: batches * 64,
            batches,
            critical_seconds: critical as f64 / 1e9,
            steps_per_second: batches as f64 * 64. / elapsed,
            checksum: self.golden,
            verified: batches > 0,
            windows,
        })
    }
}
pub fn profile(workers: usize, seconds: u64) -> String {
    format!("canonical64/seed{SEED}/4MiB/{workers}/{seconds}s/prep1s")
}
fn workers_for(available: usize) -> usize {
    if available < 2 {
        1
    } else {
        available.saturating_sub(2).max(2).min(available)
    }
}
pub fn workers() -> usize {
    workers_for(
        gameshift_cpu::topology::Topology::detect()
            .allowed_logical_cpus
            .len(),
    )
}

pub fn run() -> Category {
    let started = Instant::now();
    let result = (|| {
        let origin = Instant::now();
        let mut raw = Vec::new();
        let mut metrics = Vec::new();
        for (name, workers, duration) in [("single", 1, 1), ("multi_sustained", workers(), 4)] {
            if name == "multi_sustained" && workers < 2 {
                raw.push(serde_json::json!({"name":name,"state":"unavailable","reason":"Un seul thread logique disponible : aucun résultat multithread inventé"}));
                continue;
            }
            let mut load = Load::new(workers)?;
            let mut samples = Vec::new();
            for _ in 0..3 {
                load.sample(Duration::from_secs(1), origin)?;
                samples.push(load.sample(Duration::from_secs(duration), origin)?);
            }
            metrics.push(metric(
                &format!("cpu/{name}/{workers}"),
                "cpu",
                &format!(
                    "CPU · {} · {workers} threads",
                    if name == "single" {
                        "Monothread"
                    } else {
                        "Multithread soutenu"
                    }
                ),
                "étapes/s",
                samples.iter().map(|s| s.steps_per_second).collect(),
                &profile(workers, duration),
            ));
            raw.push(serde_json::json!({"name":name,"state":"executed","workers":workers,"requested_seconds":duration,"samples":samples}));
        }
        Ok::<_, String>((metrics, serde_json::json!(raw)))
    })();
    match result{Ok((metrics,raw))=>Category{id:"cpu".into(),state:if workers()>=2{State::Executed}else{State::Unavailable},elapsed_seconds:started.elapsed().as_secs_f64(),metrics,raw,notes:vec!["Époques identiques de 64 étapes ; restauration des buffers incluse dans le débit mural, temps critique distinct. Pool conservé pendant le test.".into(),"Multithread soutenu sur trois passages de quatre secondes, sans pause de charge. Cela ne prouve pas un équilibre thermique.".into()]},Err(e)=>Category::missing("cpu",State::Failed,e)}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workers_policy_never_calls_one_worker_multithread() {
        assert_eq!(workers_for(1), 1);
        assert_eq!(workers_for(2), 2);
        assert_eq!(workers_for(4), 2);
        assert_eq!(workers_for(16), 14);
    }
    #[test]
    fn canonical_epochs_match_between_worker_counts() {
        let mut a = Load::new(1).unwrap();
        let mut b = Load::new(2).unwrap();
        let x = a.sample(Duration::from_millis(20), Instant::now()).unwrap();
        let y = b.sample(Duration::from_millis(20), Instant::now()).unwrap();
        assert!(x.verified && y.verified);
        assert_eq!(x.checksum, y.checksum);
    }
}
