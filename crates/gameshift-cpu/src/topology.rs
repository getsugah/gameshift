//! CPU topology from the affinity mask and sysfs, not hard-coded for a 7800X3D.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Topology {
    pub allowed_logical_cpus: Vec<usize>,
    pub physical_cores: Option<usize>,
    pub l3_bytes: Option<u64>,
    pub warnings: Vec<String>,
}
impl Topology {
    pub fn detect() -> Self {
        // SAFETY: zero-initialization creates an empty cpu_set_t; kernel fills within its size.
        let mut mask: libc::cpu_set_t = unsafe { std::mem::zeroed() };
        let ok =
            unsafe { libc::sched_getaffinity(0, std::mem::size_of_val(&mask), &mut mask) } == 0;
        let mut warnings = vec![];
        let cpus = if ok {
            (0..libc::CPU_SETSIZE as usize)
                .filter(|id| unsafe { libc::CPU_ISSET(*id, &mask) })
                .collect::<Vec<_>>()
        } else {
            warnings.push("Masque d’affinité inaccessible ; topologie autorisée approximée par available_parallelism".into());
            (0..std::thread::available_parallelism()
                .map(usize::from)
                .unwrap_or(1))
                .collect()
        };
        let mut cores = BTreeSet::new();
        let mut complete = true;
        for id in &cpus {
            let base = format!("/sys/devices/system/cpu/cpu{id}/topology");
            match (
                fs::read_to_string(format!("{base}/physical_package_id")),
                fs::read_to_string(format!("{base}/core_id")),
            ) {
                (Ok(socket), Ok(core)) => {
                    cores.insert((socket.trim().to_owned(), core.trim().to_owned()));
                }
                _ => complete = false,
            }
        }
        let l3_bytes = cpus.first().and_then(|id| {
            let base = format!("/sys/devices/system/cpu/cpu{id}/cache");
            fs::read_dir(base)
                .ok()?
                .filter_map(Result::ok)
                .find_map(|entry| {
                    if fs::read_to_string(entry.path().join("level")).ok()?.trim() != "3" {
                        return None;
                    }
                    parse_size(&fs::read_to_string(entry.path().join("size")).ok()?)
                })
        });
        Self {
            allowed_logical_cpus: cpus,
            physical_cores: complete.then_some(cores.len()),
            l3_bytes,
            warnings,
        }
    }
    pub fn scaling_steps(&self) -> Vec<usize> {
        let count = self.allowed_logical_cpus.len().max(1);
        let mut values = [1, 2, 4, 6, 8, 12, 16]
            .into_iter()
            .filter(|n| *n <= count)
            .collect::<Vec<_>>();
        if count < 16 && !values.contains(&count) {
            values.push(count);
        }
        values.sort();
        values
    }
}
fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim();
    if let Some(k) = s.strip_suffix('K') {
        k.parse::<u64>().ok()?.checked_mul(1024)
    } else if let Some(m) = s.strip_suffix('M') {
        m.parse::<u64>().ok()?.checked_mul(1024 * 1024)
    } else {
        s.parse().ok()
    }
}
