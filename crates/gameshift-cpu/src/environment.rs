//! Read-only endpoint telemetry. CPU load covers initialization, warmup and measurement.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Temperature {
    pub source: String,
    pub celsius: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub unix_time_ms: Option<u128>,
    pub frequency_khz: BTreeMap<usize, Option<u64>>,
    pub temperatures: Vec<Temperature>,
    pub cpu_ticks: BTreeMap<String, [u64; 2]>,
    pub process_cpu_ticks: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub before: Snapshot,
    pub after: Snapshot,
    pub cpu_busy_percent_over_case: BTreeMap<String, Option<f64>>,
    pub system_busy_outside_process_percent: Option<f64>,
}
impl Snapshot {
    pub fn read(cpus: &[usize]) -> Self {
        let frequency_khz = cpus
            .iter()
            .map(|&id| {
                (
                    id,
                    fs::read_to_string(format!(
                        "/sys/devices/system/cpu/cpu{id}/cpufreq/scaling_cur_freq"
                    ))
                    .ok()
                    .and_then(|s| s.trim().parse().ok()),
                )
            })
            .collect();
        let mut temperatures = Vec::new();
        if let Ok(entries) = fs::read_dir("/sys/class/hwmon") {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = fs::read_to_string(path.join("name")).unwrap_or_default();
                if !matches!(
                    name.trim(),
                    "k10temp" | "coretemp" | "zenpower" | "cpu_thermal"
                ) {
                    continue;
                }
                if let Ok(sensors) = fs::read_dir(&path) {
                    for sensor in sensors.flatten() {
                        let file = sensor.file_name().to_string_lossy().into_owned();
                        if !file.starts_with("temp") || !file.ends_with("_input") {
                            continue;
                        }
                        let value = fs::read_to_string(sensor.path())
                            .ok()
                            .and_then(|s| s.trim().parse::<f64>().ok());
                        if let Some(value) =
                            value.filter(|v| v.is_finite() && (-40_000. ..=150_000.).contains(v))
                        {
                            let label =
                                fs::read_to_string(path.join(file.replace("_input", "_label")))
                                    .unwrap_or_else(|_| file.clone());
                            temperatures.push(Temperature {
                                source: format!("{}/{}", name.trim(), label.trim()),
                                celsius: value / 1000.,
                            });
                        }
                    }
                }
            }
        }
        let cpu_ticks = parse_cpu_ticks(&fs::read_to_string("/proc/stat").unwrap_or_default());
        let process_cpu_ticks = fs::read_to_string("/proc/self/stat").ok().and_then(|s| {
            let fields = s.rsplit_once(')')?.1.split_whitespace().collect::<Vec<_>>();
            fields
                .get(11)?
                .parse::<u64>()
                .ok()?
                .checked_add(fields.get(12)?.parse::<u64>().ok()?)
        });
        Self {
            unix_time_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_millis()),
            frequency_khz,
            temperatures,
            cpu_ticks,
            process_cpu_ticks,
        }
    }
}
impl Environment {
    pub fn finish(before: Snapshot, cpus: &[usize]) -> Self {
        let after = Snapshot::read(cpus);
        let system_busy_outside_process_percent = (|| {
            let start = before.cpu_ticks.get("cpu")?;
            let end = after.cpu_ticks.get("cpu")?;
            let total = end[0].checked_sub(start[0])?;
            let idle = end[1].checked_sub(start[1])?;
            let process = after
                .process_cpu_ticks?
                .checked_sub(before.process_cpu_ticks?)?;
            (total > 0).then(|| {
                100. * total.saturating_sub(idle).saturating_sub(process) as f64 / total as f64
            })
        })();
        let cpu_busy_percent_over_case = before
            .cpu_ticks
            .iter()
            .map(|(cpu, start)| {
                let value = after
                    .cpu_ticks
                    .get(cpu)
                    .and_then(|end| busy_percent(*start, *end));
                (cpu.clone(), value)
            })
            .collect();
        Self {
            before,
            after,
            cpu_busy_percent_over_case,
            system_busy_outside_process_percent,
        }
    }
}
/// Occupation over an interval, excluding guest double counting in the parsed totals.
/// Counter resets, zero intervals and inconsistent idle deltas remain unavailable.
pub fn busy_percent(start: [u64; 2], end: [u64; 2]) -> Option<f64> {
    let total = end[0].checked_sub(start[0])?;
    let idle = end[1].checked_sub(start[1])?;
    (total > 0 && idle <= total).then(|| 100. * (total - idle) as f64 / total as f64)
}
pub fn parse_cpu_ticks(input: &str) -> BTreeMap<String, [u64; 2]> {
    input
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let name = fields.next()?;
            if name != "cpu"
                && !name
                    .strip_prefix("cpu")
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            {
                return None;
            }
            // guest/guest_nice already included in user/nice; exclude them from total.
            let times = fields
                .take(8)
                .map(str::parse::<u64>)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            if times.len() < 4 {
                return None;
            }
            let total = times
                .iter()
                .try_fold(0u64, |sum, value| sum.checked_add(*value))?;
            let idle = times[3].checked_add(times.get(4).copied().unwrap_or(0))?;
            Some((name.to_string(), [total, idle]))
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interval_load_handles_idle_resets_and_invalid_deltas() {
        assert_eq!(busy_percent([100, 50], [200, 75]), Some(75.));
        assert_eq!(busy_percent([100, 50], [200, 150]), Some(0.));
        for (a, b) in [
            ([100, 50], [100, 50]),
            ([100, 50], [90, 60]),
            ([100, 50], [200, 40]),
            ([100, 50], [200, 151]),
        ] {
            assert_eq!(busy_percent(a, b), None);
        }
    }
    #[test]
    fn guest_is_not_double_counted_and_invalid_sources_are_absent() {
        let parsed =
            parse_cpu_ticks("cpu 10 2 3 20 5 1 1 2 8 1\ncpu0 1 0 0 2\ncpu1 invalid\nintr 123\n");
        assert_eq!(parsed["cpu"], [44, 25]);
        assert_eq!(parsed["cpu0"], [3, 2]);
        assert_eq!(parsed.len(), 2);
    }
}
