//! DRM fdinfo accounting, independent of privileged controls and vendor libraries.
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::Path,
    time::{Duration, Instant},
};
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Client {
    pub driver: String,
    pub pci: Option<String>,
    pub id: Option<u64>,
    pub engines_ns: BTreeMap<String, u64>,
    pub capacities: BTreeMap<String, u64>,
    pub memory_bytes: BTreeMap<String, u64>,
}
fn quantity(raw: &str, unit: &str) -> Option<u64> {
    let mut p = raw.split_whitespace();
    let value = p.next()?.parse::<u64>().ok()?;
    let actual = p.next().unwrap_or("");
    if actual != unit || p.next().is_some() {
        return None;
    }
    Some(value)
}
fn memory(raw: &str) -> Option<u64> {
    let mut p = raw.split_whitespace();
    let value = p.next()?.parse::<u64>().ok()?;
    let factor = match p.next().unwrap_or("") {
        "" => 1,
        "KiB" => 1024,
        "MiB" => 1048576,
        _ => return None,
    };
    if p.next().is_some() {
        return None;
    }
    value.checked_mul(factor)
}
pub fn parse_fdinfo(text: &str) -> Option<Client> {
    let mut out = Client::default();
    let mut aliases = BTreeMap::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "drm-driver" => out.driver = value.into(),
            "drm-pdev" => out.pci = gameshift_control::canonical_pci(value).ok(),
            "drm-client-id" => out.id = value.parse().ok(),
            _ => {
                if let Some(name) = key.strip_prefix("drm-engine-capacity-") {
                    if let Ok(v) = value.parse::<u64>() {
                        if v > 0 {
                            out.capacities.insert(name.into(), v);
                        }
                    }
                } else if let Some(name) = key.strip_prefix("drm-engine-") {
                    if let Some(v) = quantity(value, "ns") {
                        out.engines_ns.insert(name.into(), v);
                    }
                } else if let Some(name) = key.strip_prefix("drm-resident-") {
                    if let Some(v) = memory(value) {
                        out.memory_bytes.insert(name.into(), v);
                    }
                } else if let Some(name) = key.strip_prefix("drm-memory-") {
                    if let Some(v) = memory(value) {
                        aliases.insert(name.into(), v);
                    }
                }
            }
        }
    }
    for (k, v) in aliases {
        out.memory_bytes.entry(k).or_insert(v);
    }
    (!out.driver.is_empty()).then_some(out)
}
#[derive(Debug, Clone, Serialize)]
pub struct Engine {
    pub name: String,
    pub cumulative_ns: u64,
    pub busy_percent: Option<f64>,
    pub capacity: u64,
}
#[derive(Debug, Clone, Serialize)]
pub struct ProcessGpu {
    pub pid: u32,
    pub start_ticks: u64,
    pub name: String,
    pub executable: Option<String>,
    pub pci: String,
    pub driver: String,
    pub clients: usize,
    pub shared_clients: bool,
    pub memory_bytes: BTreeMap<String, u64>,
    pub engines: Vec<Engine>,
    pub history: Vec<Option<f64>>,
}
impl ProcessGpu {
    pub fn activity(&self) -> Option<f64> {
        self.engines
            .iter()
            .filter_map(|e| e.busy_percent)
            .reduce(f64::max)
    }
}
#[derive(Debug, Clone, Serialize, Default)]
pub struct Snapshot {
    pub rows: Vec<ProcessGpu>,
    pub denied: usize,
    pub vanished: usize,
    pub ambiguous: usize,
    pub truncated: bool,
    pub elapsed_seconds: f64,
}
#[derive(Default)]
pub struct Tracker {
    previous: BTreeMap<(u32, u64, String, u64), Client>,
    history: BTreeMap<(u32, u64, String), Vec<Option<f64>>>,
    time: Option<Instant>,
}
struct Observed {
    pid: u32,
    start: u64,
    name: String,
    exe: Option<String>,
    client: Client,
}
fn text(path: &Path, limit: u64) -> std::io::Result<String> {
    let mut s = String::new();
    fs::File::open(path)?
        .take(limit + 1)
        .read_to_string(&mut s)?;
    if s.len() as u64 > limit {
        return Err(std::io::Error::other("bounded read"));
    }
    Ok(s)
}
fn start_ticks(stat: &str) -> Option<u64> {
    stat.rsplit_once(") ")?
        .1
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()
}
impl Tracker {
    pub fn sample(&mut self) -> Snapshot {
        self.sample_at(Path::new("/"), Instant::now())
    }
    pub fn sample_at(&mut self, root: &Path, now: Instant) -> Snapshot {
        let began = Instant::now();
        let mut result = Snapshot::default();
        let devices = crate::gpus::discover_at(root).unwrap_or_default();
        let known = devices
            .iter()
            .map(|g| g.pci.as_str())
            .collect::<BTreeSet<_>>();
        let mut observations = Vec::new();
        let mut descriptors = 0usize;
        if let Ok(entries) = fs::read_dir(root.join("proc")) {
            for process in entries.flatten() {
                if began.elapsed() > Duration::from_millis(300) || descriptors >= 32768 {
                    result.truncated = true;
                    break;
                }
                let Ok(pid) = process.file_name().to_string_lossy().parse::<u32>() else {
                    continue;
                };
                let Some(start) = text(&process.path().join("stat"), 8192)
                    .ok()
                    .and_then(|s| start_ticks(&s))
                else {
                    result.vanished += 1;
                    continue;
                };
                let name = text(&process.path().join("comm"), 256)
                    .unwrap_or_else(|_| "Processus".into())
                    .trim()
                    .to_owned();
                let exe = fs::read_link(process.path().join("exe"))
                    .ok()
                    .map(|p| p.display().to_string());
                let entries = match fs::read_dir(process.path().join("fdinfo")) {
                    Ok(v) => v,
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::PermissionDenied {
                            result.denied += 1;
                        } else {
                            result.vanished += 1;
                        }
                        continue;
                    }
                };
                let mut seen = BTreeSet::new();
                for file in entries.flatten().take(4096) {
                    descriptors += 1;
                    if descriptors >= 32768 || began.elapsed() > Duration::from_millis(300) {
                        result.truncated = true;
                        break;
                    }
                    let raw = match text(&file.path(), 32768) {
                        Ok(v) => v,
                        Err(e) => {
                            if e.kind() == std::io::ErrorKind::PermissionDenied {
                                result.denied += 1;
                            }
                            continue;
                        }
                    };
                    let Some(mut client) = parse_fdinfo(&raw) else {
                        continue;
                    };
                    if client.pci.is_none() {
                        let node =
                            fs::read_link(process.path().join("fd").join(file.file_name())).ok();
                        client.pci = node.as_ref().and_then(|node| {
                            devices
                                .iter()
                                .find(|g| g.nodes.iter().any(|n| Path::new(n) == node.as_path()))
                                .map(|g| g.pci.clone())
                        });
                    }
                    let (Some(pci), Some(id)) = (client.pci.clone(), client.id) else {
                        result.ambiguous += 1;
                        continue;
                    };
                    if !known.contains(pci.as_str()) {
                        result.ambiguous += 1;
                        continue;
                    }
                    if !seen.insert((pci, id)) {
                        continue;
                    }
                    observations.push(Observed {
                        pid,
                        start,
                        name: name.clone(),
                        exe: exe.clone(),
                        client,
                    });
                }
                // PID reuse/exit while inspecting fdinfo must not join unrelated processes.
                if text(&process.path().join("stat"), 8192)
                    .ok()
                    .and_then(|s| start_ticks(&s))
                    != Some(start)
                {
                    observations.retain(|o| o.pid != pid);
                    result.vanished += 1;
                }
            }
        }
        let dt = self
            .time
            .map(|t| now.saturating_duration_since(t).as_secs_f64())
            .filter(|v| *v > 0.);
        let mut next = BTreeMap::new();
        let mut grouped = BTreeMap::<(u32, u64, String), ProcessGpu>::new();
        let mut owners = BTreeMap::<(String, u64), usize>::new();
        for o in &observations {
            *owners
                .entry((o.client.pci.clone().unwrap(), o.client.id.unwrap()))
                .or_default() += 1;
        }
        for mut o in observations {
            let pci = o.client.pci.clone().unwrap();
            let id = o.client.id.unwrap();
            let key = (o.pid, o.start, pci.clone(), id);
            let mut regressed = BTreeSet::new();
            if let Some(old) = self.previous.get(&key) {
                for (name, value) in &mut o.client.engines_ns {
                    if let Some(previous) = old.engines_ns.get(name) {
                        if *value < *previous {
                            *value = *previous;
                            regressed.insert(name.clone());
                        }
                    }
                }
            }

            let row = grouped
                .entry((o.pid, o.start, pci.clone()))
                .or_insert_with(|| ProcessGpu {
                    pid: o.pid,
                    start_ticks: o.start,
                    name: o.name,
                    executable: o.exe,
                    pci: pci.clone(),
                    driver: o.client.driver.clone(),
                    clients: 0,
                    shared_clients: false,
                    memory_bytes: BTreeMap::new(),
                    engines: vec![],
                    history: vec![],
                });
            row.clients += 1;
            row.shared_clients |= owners[&(pci, id)] > 1;
            for (name, bytes) in &o.client.memory_bytes {
                let old = row.memory_bytes.entry(name.clone()).or_default();
                *old = old.saturating_add(*bytes);
            }
            for (name, ns) in &o.client.engines_ns {
                let capacity = *o.client.capacities.get(name).unwrap_or(&1);
                let busy = self
                    .previous
                    .get(&key)
                    .filter(|_| !regressed.contains(name))
                    .and_then(|p| p.engines_ns.get(name))
                    .and_then(|old| ns.checked_sub(*old))
                    .zip(dt)
                    .map(|(delta, dt)| delta as f64 / (dt * 1e9 * capacity as f64) * 100.)
                    .filter(|v| v.is_finite() && *v <= 100.1)
                    .map(|v| v.min(100.));
                if let Some(engine) = row.engines.iter_mut().find(|e| e.name == *name) {
                    engine.cumulative_ns = engine.cumulative_ns.saturating_add(*ns);
                    engine.busy_percent = engine.busy_percent.zip(busy).map(|(a, b)| a + b);
                    engine.capacity = engine.capacity.max(capacity);
                } else {
                    row.engines.push(Engine {
                        name: name.clone(),
                        cumulative_ns: *ns,
                        busy_percent: busy,
                        capacity,
                    });
                }
            }
            next.insert(key, o.client);
        }
        let mut history = BTreeMap::new();
        for (key, mut row) in grouped {
            for engine in &mut row.engines {
                engine.busy_percent = engine
                    .busy_percent
                    .filter(|v| *v <= 100.1)
                    .map(|v| v.min(100.));
            }
            let mut h = self.history.remove(&key).unwrap_or_default();
            h.push(row.activity());
            if h.len() > 60 {
                h.remove(0);
            }
            row.history = h.clone();
            history.insert(key, h);
            result.rows.push(row);
        }
        result.rows.sort_by_key(|r| r.pid);
        self.history = history;
        self.previous = next;
        self.time = Some(now);
        result.elapsed_seconds = began.elapsed().as_secs_f64();
        result
    }
    pub fn pause(&mut self) {
        self.previous.clear();
        self.time = None;
        for values in self.history.values_mut() {
            values.push(None);
        }
    }
}
