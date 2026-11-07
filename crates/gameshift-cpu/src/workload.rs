//! Fixed-work deterministic simulation and memory-access workloads.
use crate::instrument::{thread_cpu_ns, CounterSample, Counters, PhaseMeter, PhaseTiming};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::{hint::black_box, mem::size_of, time::Instant};
const CHUNK: usize = 2048;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pattern {
    Sequential,
    Random,
    Dependent,
    Entities,
    Spatial,
    Mixed,
}
#[repr(C, align(64))]
#[derive(Clone, Copy, Default)]
struct State {
    position: [f32; 4],
    velocity: [f32; 4],
    animation: [f32; 4],
    next: u64,
    state: u64,
}
/// Prepared column-major transform and color. Vulkan integration is a separate stage.
#[repr(C, align(16))]
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderPacket {
    pub transform: [f32; 16],
    pub color: [f32; 4],
}
#[repr(C, align(64))]
#[derive(Clone, Copy)]
struct Branch {
    min: [f32; 3],
    max: [f32; 3],
    first: u32,
    end: u32,
}
impl Default for Branch {
    fn default() -> Self {
        Self {
            min: [f32::INFINITY; 3],
            max: [f32::NEG_INFINITY; 3],
            first: 0,
            end: 0,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Footprint {
    pub requested_bytes: u64,
    pub state_logical_bytes: u64,
    pub state_allocated_bytes: u64,
    pub render_allocated_bytes: u64,
    pub spatial_allocated_bytes: u64,
    pub container_metadata_bytes: u64,
    pub persistent_allocated_bytes: u64,
    pub per_step_measurement_estimate_bytes: u64,
    pub allocator_bookkeeping_bytes: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobTiming {
    pub phases: Vec<PhaseTiming>,
    pub counters: Vec<CounterSample>,
    pub dispatch_offset_ns: u64,
    pub completed_offset_ns: u64,
    pub checksum: u64,
    pub visible_entities: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepTiming {
    pub critical_wall_ns: u64,
    pub cumulative_worker_cpu_ns: Option<u64>,
    pub coordinator_cpu_ns: Option<u64>,
    pub synchronization_tail_ns: u64,
    pub jobs: Vec<JobTiming>,
    pub checksum: u64,
    pub entities: u64,
    pub visible_entities: u64,
    pub logical_read_write_bytes: u64,
}
pub struct Workload {
    source: Vec<State>,
    destination: Vec<State>,
    packets: Vec<RenderPacket>,
    tree: Vec<Branch>,
    leaves: usize,
    seed: u64,
    step: u64,
    requested: u64,
}
fn mix(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58476d1ce4e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}
fn permutation(mut x: usize, n: usize, seed: u64) -> usize {
    let mask = n.next_power_of_two() - 1;
    loop {
        x ^= seed as usize & mask;
        x ^= x >> 7;
        x = x.wrapping_mul(0x9e3779b1) & mask;
        x ^= x >> 11;
        x = x.wrapping_mul(0x85ebca77) & mask;
        x ^= x >> 9;
        if x < n {
            return x;
        }
    }
}
fn origin(i: usize) -> [f32; 3] {
    let axis = |a: usize| {
        let mut n = 0;
        for bit in 0..7 {
            n |= ((i >> (bit * 3 + a)) & 1) << bit;
        }
        n as f32 * 4.0 - 128.0
    };
    [axis(0), axis(1), axis(2)]
}
impl Workload {
    pub fn new(bytes: u64, seed: u64, pattern: Pattern) -> Result<Self, String> {
        if !(1024 * 1024..=256 * 1024 * 1024).contains(&bytes) {
            return Err("Empreinte autorisée : 1 à 256 Mio".into());
        }
        let count = bytes as usize / (2 * size_of::<State>() + size_of::<RenderPacket>());
        let mut source = vec![State::default(); count];
        for (i, state) in source.iter_mut().enumerate() {
            let p = origin(i);
            let hash = mix(seed ^ i as u64);
            state.position = [p[0], p[1], p[2], 1.0];
            state.velocity = [
                ((hash & 255) as f32 - 128.0) / 256.0,
                0.05,
                (((hash >> 8) & 255) as f32 - 128.0) / 256.0,
                0.0,
            ];
            state.state = hash;
        }
        // A single dependency cycle, scrambled bijectively without an auxiliary permutation allocation.
        for i in 0..count {
            let from = permutation(i, count, seed);
            let to = permutation((i + 1) % count, count, seed);
            source[from].next = to as u64;
        }
        let leaves = count.div_ceil(16).next_power_of_two();
        let mut tree = if matches!(pattern, Pattern::Spatial | Pattern::Mixed) {
            vec![Branch::default(); leaves * 2 - 1]
        } else {
            vec![]
        };
        if !tree.is_empty() {
            for leaf in 0..leaves {
                let first = (leaf * 16).min(count);
                let end = ((leaf + 1) * 16).min(count);
                let branch = &mut tree[leaves - 1 + leaf];
                branch.first = first as u32;
                branch.end = end as u32;
                for state in &source[first..end] {
                    for a in 0..3 {
                        branch.min[a] = branch.min[a].min(state.position[a] - 2.0);
                        branch.max[a] = branch.max[a].max(state.position[a] + 2.0);
                    }
                }
            }
            for i in (0..leaves - 1).rev() {
                for a in 0..3 {
                    tree[i].min[a] = tree[i * 2 + 1].min[a].min(tree[i * 2 + 2].min[a]);
                    tree[i].max[a] = tree[i * 2 + 1].max[a].max(tree[i * 2 + 2].max[a]);
                }
            }
        }
        Ok(Self {
            destination: source.clone(),
            packets: vec![RenderPacket::default(); count],
            source,
            tree,
            leaves,
            seed,
            step: 0,
            requested: bytes,
        })
    }
    pub fn footprint(&self) -> Footprint {
        let states = (self.source.capacity() + self.destination.capacity()) * size_of::<State>();
        let render = self.packets.capacity() * size_of::<RenderPacket>();
        let spatial = self.tree.capacity() * size_of::<Branch>();
        let meta = size_of::<Self>();
        Footprint {
            requested_bytes: self.requested,
            state_logical_bytes: (self.source.len() * 2 * size_of::<State>()) as u64,
            state_allocated_bytes: states as u64,
            render_allocated_bytes: render as u64,
            spatial_allocated_bytes: spatial as u64,
            container_metadata_bytes: meta as u64,
            persistent_allocated_bytes: (states + render + spatial + meta) as u64,
            per_step_measurement_estimate_bytes: (self.source.len().div_ceil(CHUNK)
                * (size_of::<JobTiming>() + 5 * size_of::<PhaseTiming>()))
                as u64,
            allocator_bookkeeping_bytes: None,
        }
    }
    pub fn packets(&self) -> &[RenderPacket] {
        &self.packets
    }
    pub fn reset(&mut self, seed: u64, pattern: Pattern) -> Result<(), String> {
        *self = Self::new(self.requested, seed, pattern)?;
        Ok(())
    }
    /// Restore a canonical system-suite epoch without allocating. Historical reset is unchanged.
    pub fn restore_template(&mut self, baseline: &Self) -> Result<(), String> {
        if self.source.len() != baseline.source.len() || self.tree.len() != baseline.tree.len() {
            return Err("Empreintes de simulation différentes".into());
        }
        self.source.copy_from_slice(&baseline.source);
        self.destination.copy_from_slice(&baseline.destination);
        self.packets.copy_from_slice(&baseline.packets);
        self.tree.copy_from_slice(&baseline.tree);
        self.seed = baseline.seed;
        self.step = baseline.step;
        self.leaves = baseline.leaves;
        Ok(())
    }
    pub fn scratch(&self) -> Vec<JobTiming> {
        (0..self.source.len().div_ceil(CHUNK))
            .map(|_| JobTiming {
                phases: [
                    "simulation",
                    "animation",
                    "transforms",
                    "culling_spatial",
                    "render_preparation",
                ]
                .into_iter()
                .map(|phase| PhaseTiming {
                    phase: phase.into(),
                    wall_ns: 0,
                    thread_cpu_ns: None,
                    linux_tid: 0,
                    worker_index: 0,
                })
                .collect(),
                counters: vec![],
                dispatch_offset_ns: 0,
                completed_offset_ns: 0,
                checksum: 0,
                visible_entities: 0,
            })
            .collect()
    }
    pub fn step(
        &mut self,
        pool: &rayon::ThreadPool,
        pattern: Pattern,
        dt_ns: u64,
        perf: bool,
    ) -> StepTiming {
        let scratch = self.scratch();
        self.step_reusing(pool, pattern, dt_ns, perf, scratch)
    }
    /// Reuse per-block timing storage; the simulation calculations are identical.
    pub fn step_reusing(
        &mut self,
        pool: &rayon::ThreadPool,
        pattern: Pattern,
        dt_ns: u64,
        perf: bool,
        mut jobs: Vec<JobTiming>,
    ) -> StepTiming {
        if jobs.len() != self.source.len().div_ceil(CHUNK) {
            jobs = self.scratch();
        }
        let started = Instant::now();
        let coordinator = thread_cpu_ns();
        let source = &self.source;
        let tree = &self.tree;
        let leaves = self.leaves;
        let seed = self.seed;
        let tick = self.step;
        let n = source.len();
        let dt = dt_ns as f32 / 1_000_000_000.0;
        pool.install(|| {
            self.destination
                .par_chunks_mut(CHUNK)
                .zip(self.packets.par_chunks_mut(CHUNK))
                .zip(jobs.par_iter_mut())
                .enumerate()
                .for_each(|(chunk, ((output, packets), job))| {
                    let dispatch_offset_ns = started.elapsed().as_nanos() as u64;
                    let worker = rayon::current_thread_index().unwrap_or(usize::MAX);
                    let mut counters = Counters::open(perf);
                    counters.start();
                    let phases = &mut job.phases;
                    let meter = PhaseMeter::start("simulation", worker);
                    for (j, destination) in output.iter_mut().enumerate() {
                        let i = chunk * CHUNK + j;
                        let index = match pattern {
                            Pattern::Random => permutation(i, n, seed ^ tick),
                            Pattern::Dependent => {
                                let mut cursor = i;
                                for _ in 0..8 {
                                    cursor = source[black_box(cursor)].next as usize;
                                }
                                cursor
                            }
                            _ => i,
                        };
                        *destination = source[index];
                        destination.state = mix(destination.state ^ tick ^ i as u64);
                        if matches!(pattern, Pattern::Entities | Pattern::Mixed) {
                            let anchor = origin(i);
                            for (a, base) in anchor.iter().enumerate() {
                                destination.position[a] = base
                                    + (destination.position[a] - base
                                        + destination.velocity[a] * dt
                                        + 1.0)
                                        .rem_euclid(2.0)
                                    - 1.0;
                            }
                        }
                    }
                    meter.finish_into(&mut phases[0]);
                    let meter = PhaseMeter::start("animation", worker);
                    if matches!(pattern, Pattern::Entities | Pattern::Mixed) {
                        for node in output.iter_mut() {
                            let a = tick as f32 * dt + (node.state & 1023) as f32 * 0.001;
                            node.animation = [a.sin(), a.cos(), (a * 0.5).sin(), 1.0];
                        }
                    }
                    meter.finish_into(&mut phases[1]);
                    let meter = PhaseMeter::start("transforms", worker);
                    for (node, packet) in output.iter().zip(packets.iter_mut()) {
                        let (c, s) = if matches!(pattern, Pattern::Entities | Pattern::Mixed) {
                            (node.animation[1], node.animation[0])
                        } else {
                            (1.0, 0.0)
                        };
                        packet.transform = [
                            c,
                            0.0,
                            -s,
                            0.0,
                            0.0,
                            1.0,
                            0.0,
                            0.0,
                            s,
                            0.0,
                            c,
                            0.0,
                            node.position[0],
                            node.position[1],
                            node.position[2],
                            1.0,
                        ];
                        packet.color = [
                            (node.state & 255) as f32 / 255.0,
                            ((node.state >> 8) & 255) as f32 / 255.0,
                            0.6,
                            1.0,
                        ];
                    }
                    meter.finish_into(&mut phases[2]);
                    let meter = PhaseMeter::start("culling_spatial", worker);
                    let mut visible = 0u64;
                    let mut checksum = 0u64;
                    for node in output.iter_mut() {
                        let is_visible = node.position[0].abs() < 96.0
                            && node.position[1].abs() < 32.0
                            && node.position[2].abs() < 96.0;
                        visible += u64::from(is_visible);
                        if matches!(pattern, Pattern::Spatial | Pattern::Mixed) {
                            let mut stack = [0usize; 64];
                            let mut size = 1;
                            let mut hits = 0u64;
                            while size > 0 {
                                size -= 1;
                                let index = stack[size];
                                let b = &tree[index];
                                if (0..3).any(|a| {
                                    node.position[a] < b.min[a] || node.position[a] > b.max[a]
                                }) {
                                    continue;
                                }
                                if index >= leaves - 1 {
                                    hits += u64::from(b.end - b.first);
                                } else {
                                    stack[size] = index * 2 + 1;
                                    stack[size + 1] = index * 2 + 2;
                                    size += 2;
                                }
                            }
                            node.state ^= hits;
                        }
                        checksum ^= node.state
                            ^ u64::from(node.position[0].to_bits())
                            ^ u64::from(node.position[2].to_bits());
                    }
                    meter.finish_into(&mut phases[3]);
                    let meter = PhaseMeter::start("render_preparation", worker);
                    // Consume prepared transforms and flags as a stable render-list digest.
                    for packet in packets.iter() {
                        checksum ^= u64::from(packet.transform[12].to_bits()).rotate_left(17)
                            ^ u64::from(packet.color[0].to_bits());
                    }
                    black_box(checksum);
                    meter.finish_into(&mut phases[4]);
                    job.counters = counters.stop();
                    job.dispatch_offset_ns = dispatch_offset_ns;
                    job.completed_offset_ns = started.elapsed().as_nanos() as u64;
                    job.checksum = checksum;
                    job.visible_entities = visible;
                });
        });
        let critical_wall_ns = started.elapsed().as_nanos() as u64;
        let cumulative_worker_cpu_ns = jobs
            .iter()
            .flat_map(|j| &j.phases)
            .try_fold(0u64, |total, p| total.checked_add(p.thread_cpu_ns?));
        let last_end = jobs
            .iter()
            .map(|j| j.completed_offset_ns)
            .max()
            .unwrap_or(0);
        let checksum = jobs.iter().fold(0, |sum, j| sum ^ j.checksum);
        let visible_entities = jobs.iter().map(|j| j.visible_entities).sum();
        std::mem::swap(&mut self.source, &mut self.destination);
        self.step += 1;
        StepTiming {
            critical_wall_ns,
            cumulative_worker_cpu_ns,
            coordinator_cpu_ns: thread_cpu_ns()
                .zip(coordinator)
                .and_then(|(end, start)| end.checked_sub(start)),
            synchronization_tail_ns: critical_wall_ns.saturating_sub(last_end),
            jobs,
            checksum,
            entities: n as u64,
            visible_entities,
            logical_read_write_bytes: (n * size_of::<State>() * 2) as u64,
        }
    }
}
