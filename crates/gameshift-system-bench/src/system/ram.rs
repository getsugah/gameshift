//! Native streaming kernels: explicit SSE2 on x86_64, no libc memory primitives.
use super::{metric, Category, State};
use serde::Serialize;
use std::{
    fs,
    hint::black_box,
    time::{Duration, Instant},
};

// Scalar fallback uses volatile accesses, and is a separate architecture fingerprint.
#[inline(never)]
#[no_mangle]
/// # Safety
/// `source` must address `words` readable u64 values; words must be a multiple of eight.
pub unsafe extern "C" fn gameshift_ram_read_v2(source: *const u64, words: usize) -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        let mut checksum: u64;
        let blocks = words / 8;
        if blocks == 0 {
            return 0;
        }
        std::arch::asm!("pxor xmm4,xmm4","2:","movdqu xmm0,[{src}]","movdqu xmm1,[{src}+16]","movdqu xmm2,[{src}+32]","movdqu xmm3,[{src}+48]","pxor xmm4,xmm0","pxor xmm4,xmm1","pxor xmm4,xmm2","pxor xmm4,xmm3","add {src},64","dec {n}","jnz 2b","movhlps xmm0,xmm4","pxor xmm4,xmm0","movq {out},xmm4",src=inout(reg)source=>_,n=inout(reg)blocks=>_,out=lateout(reg)checksum,lateout("xmm0")_,lateout("xmm1")_,lateout("xmm2")_,lateout("xmm3")_,lateout("xmm4")_,options(nostack,readonly));
        checksum
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let mut sum = 0;
        for i in 0..words {
            sum ^= source.add(i).read_volatile();
        }
        sum
    }
}
#[inline(never)]
#[no_mangle]
/// # Safety
/// `destination` must address `words` writable u64 values, exclusively; words is a multiple of eight.
pub unsafe extern "C" fn gameshift_ram_write_v2(destination: *mut u64, words: usize, value: u64) {
    #[cfg(target_arch = "x86_64")]
    {
        let blocks = words / 8;
        if blocks == 0 {
            return;
        }
        std::arch::asm!("movq xmm0,{value}","punpcklqdq xmm0,xmm0","2:","movdqu [{dst}],xmm0","movdqu [{dst}+16],xmm0","movdqu [{dst}+32],xmm0","movdqu [{dst}+48],xmm0","add {dst},64","dec {n}","jnz 2b","mfence",dst=inout(reg)destination=>_,n=inout(reg)blocks=>_,value=in(reg)value,lateout("xmm0")_,options(nostack));
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        for i in 0..words {
            destination.add(i).write_volatile(value);
        }
    }
}
#[inline(never)]
#[no_mangle]
/// # Safety
/// Both ranges must be valid for `words` u64 values and not overlap. Words is a multiple of eight.
pub unsafe extern "C" fn gameshift_ram_copy_v2(
    source: *const u64,
    destination: *mut u64,
    words: usize,
) {
    #[cfg(target_arch = "x86_64")]
    {
        let blocks = words / 8;
        if blocks == 0 {
            return;
        }
        std::arch::asm!("2:","movdqu xmm0,[{src}]","movdqu xmm1,[{src}+16]","movdqu xmm2,[{src}+32]","movdqu xmm3,[{src}+48]","movdqu [{dst}],xmm0","movdqu [{dst}+16],xmm1","movdqu [{dst}+32],xmm2","movdqu [{dst}+48],xmm3","add {src},64","add {dst},64","dec {n}","jnz 2b","mfence",src=inout(reg)source=>_,dst=inout(reg)destination=>_,n=inout(reg)blocks=>_,lateout("xmm0")_,lateout("xmm1")_,lateout("xmm2")_,lateout("xmm3")_,options(nostack));
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        for i in 0..words {
            destination
                .add(i)
                .write_volatile(source.add(i).read_volatile());
        }
    }
}
fn available_bytes() -> Result<u64, String> {
    let mut available = fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|line| line.strip_prefix("MemAvailable:"))
                .and_then(|v| v.split_whitespace().next()?.parse::<u64>().ok())
        })
        .map(|v| v * 1024)
        .ok_or("RAM disponible inconnue")?;
    let base = std::path::Path::new("/sys/fs/cgroup");
    let relative = fs::read_to_string("/proc/self/cgroup")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|line| line.strip_prefix("0::"))
                .map(str::to_owned)
        })
        .unwrap_or_default();
    let mut path = if relative.contains("..") {
        base.to_path_buf()
    } else {
        base.join(relative.trim_start_matches('/'))
    };
    loop {
        let max = fs::read_to_string(path.join("memory.max"))
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok());
        let current = fs::read_to_string(path.join("memory.current"))
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok());
        if let (Some(max), Some(current)) = (max, current) {
            available = available.min(max.saturating_sub(current));
        }
        if path == base || !path.pop() {
            break;
        }
    }
    Ok(available)
}
fn mix(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58476d1ce4e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}
#[derive(Debug, Clone, Serialize)]
pub struct Sample {
    pub operation: String,
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub elapsed_seconds: f64,
    pub passes: u64,
    pub logical_bytes: u64,
    pub gb_per_second: f64,
    pub verified: bool,
    pub checksum: Option<u64>,
}
pub struct Buffers {
    source: Vec<u64>,
    destination: Vec<u64>,
    expected: u64,
}
impl Buffers {
    pub fn new(bytes: usize) -> Result<Self, String> {
        if bytes < 64 || !bytes.is_multiple_of(64) {
            return Err("Taille RAM multiple de 64 requise".into());
        }
        if (bytes as u64).saturating_mul(2) > available_bytes()? / 2 {
            return Err("Mémoire disponible insuffisante pour les buffers natifs".into());
        }
        let words = bytes / 8;
        let mut source = Vec::new();
        source.try_reserve_exact(words).map_err(|e| e.to_string())?;
        for i in 0..words {
            source.push(mix(i as u64 ^ 0x415552454c494132));
        }
        let mut destination = Vec::new();
        destination
            .try_reserve_exact(words)
            .map_err(|e| e.to_string())?;
        destination.resize(words, 0);
        let expected = source.iter().fold(0, |sum, v| sum ^ v);
        Ok(Self {
            source,
            destination,
            expected,
        })
    }
    pub fn bytes(&self) -> usize {
        self.source.len() * 8
    }
    pub fn sample(
        &mut self,
        operation: &str,
        duration: Duration,
        origin: Instant,
    ) -> Result<Sample, String> {
        self.sample_synchronized(operation, duration, origin, None)
    }
    pub(crate) fn sample_synchronized(
        &mut self,
        operation: &str,
        duration: Duration,
        origin: Instant,
        done: Option<&std::sync::Barrier>,
    ) -> Result<Sample, String> {
        let words = self.source.len();
        let start = Instant::now();
        let mut passes = 0u64;
        let mut checksum = 0;
        let mut value = 0;
        while start.elapsed() < duration {
            unsafe {
                match operation {
                    "read" => {
                        checksum = black_box(gameshift_ram_read_v2(self.source.as_ptr(), words))
                    }
                    "write" => {
                        value = mix(passes);
                        gameshift_ram_write_v2(self.destination.as_mut_ptr(), words, value);
                    }
                    "copy" => gameshift_ram_copy_v2(
                        self.source.as_ptr(),
                        self.destination.as_mut_ptr(),
                        words,
                    ),
                    _ => return Err("Opération RAM inconnue".into()),
                }
            }
            passes += 1;
        }
        let end = Instant::now();
        if let Some(done) = done {
            done.wait();
        }
        let elapsed = end.duration_since(start).as_secs_f64();
        let verified = match operation {
            "read" => checksum == self.expected,
            "write" => self.destination.iter().all(|v| *v == value),
            "copy" => self.source == self.destination,
            _ => false,
        };
        if !verified {
            return Err(format!("Validation RAM échouée : {operation}"));
        }
        let logical_bytes = passes * self.bytes() as u64 * if operation == "copy" { 2 } else { 1 };
        Ok(Sample {
            operation: operation.into(),
            start_seconds: start.duration_since(origin).as_secs_f64(),
            end_seconds: end.duration_since(origin).as_secs_f64(),
            elapsed_seconds: elapsed,
            passes,
            logical_bytes,
            gb_per_second: logical_bytes as f64 / elapsed / 1e9,
            verified,
            checksum: if operation == "read" {
                Some(checksum)
            } else {
                None
            },
        })
    }
}
pub fn working_set() -> Result<(usize, u64), String> {
    let topology = gameshift_cpu::topology::Topology::detect();
    let mut caches = std::collections::BTreeMap::new();
    for cpu in &topology.allowed_logical_cpus {
        if let Ok(entries) = fs::read_dir(format!("/sys/devices/system/cpu/cpu{cpu}/cache")) {
            for e in entries.flatten() {
                let p = e.path();
                if fs::read_to_string(p.join("level")).is_ok_and(|s| s.trim() == "3") {
                    let shared = fs::read_to_string(p.join("shared_cpu_list")).unwrap_or_default();
                    let size = fs::read_to_string(p.join("size")).unwrap_or_default();
                    if let Some(kib) = size
                        .trim()
                        .strip_suffix('K')
                        .and_then(|s| s.parse::<u64>().ok())
                    {
                        caches.insert(shared, kib * 1024);
                    }
                }
            }
        }
    }
    let llc = caches.values().sum::<u64>();
    if llc == 0 {
        return Err(
            "Capacité de cache LLC inconnue : impossible de garantir un jeu RAM hors cache".into(),
        );
    }
    let bytes = (256 * 1024 * 1024u64)
        .max(llc.saturating_mul(2))
        .next_multiple_of(64);
    let available = available_bytes()?;
    if bytes.saturating_mul(3) > available / 2 || bytes > 2 * 1024 * 1024 * 1024 {
        return Err("Mémoire disponible insuffisante pour dépasser le LLC en sécurité".into());
    }
    Ok((bytes as usize, llc))
}
#[derive(Serialize)]
pub struct Latency {
    pub bytes: usize,
    pub accesses: u64,
    pub elapsed_seconds: f64,
    pub ns_per_access: f64,
    pub start_index: usize,
    pub final_index: usize,
    pub verified: bool,
}
#[inline(never)]
fn chase(data: &[usize], mut index: usize, count: u64) -> usize {
    for _ in 0..count {
        index = unsafe { data.as_ptr().add(index).read_volatile() };
    }
    black_box(index)
}
pub fn latency(bytes: usize, duration: Duration) -> Result<Vec<Latency>, String> {
    let n = (bytes / std::mem::size_of::<usize>()).next_power_of_two();
    let mask = n - 1;
    let perm = |mut x: usize| {
        x ^= x >> 13;
        x = x.wrapping_mul(0x9e3779b1) & mask;
        x ^= x >> 7;
        x & mask
    };
    let mut data = vec![0; n];
    for i in 0..n {
        data[perm(i)] = perm((i + 1) & mask);
    }
    let mut output = Vec::new();
    for _ in 0..3 {
        let mut index = 0;
        let warm = Instant::now();
        while warm.elapsed() < Duration::from_millis(50) {
            index = chase(&data, index, 4096);
        }
        let start_index = index;
        let started = Instant::now();
        let mut accesses = 0;
        while started.elapsed() < duration {
            index = chase(&data, index, 4096);
            accesses += 4096;
        }
        let elapsed = started.elapsed().as_secs_f64();
        let mut expected = start_index;
        for _ in 0..accesses {
            expected = data[expected];
        }
        if index != expected {
            return Err("Chaîne mémoire invalide".into());
        }
        output.push(Latency {
            bytes: n * std::mem::size_of::<usize>(),
            accesses,
            elapsed_seconds: elapsed,
            ns_per_access: elapsed * 1e9 / accesses as f64,
            start_index,
            final_index: index,
            verified: true,
        });
    }
    Ok(output)
}
pub fn profile(operation: &str, bytes: usize, seconds: u64) -> String {
    format!(
        "native-SSE2-or-volatile-v2/{bytes}/{operation}/{}",
        seconds * 1000
    )
}
pub fn run() -> Category {
    let started = Instant::now();
    let (bytes, llc) = match working_set() {
        Ok(v) => v,
        Err(e) => return Category::missing("ram", State::Unavailable, e),
    };
    let result = (|| {
        let mut buffers = Buffers::new(bytes)?;
        let origin = Instant::now();
        let mut samples = Vec::new();
        let mut metrics = Vec::new();
        for op in ["read", "write", "copy"] {
            let duration = Duration::from_secs(if op == "copy" { 4 } else { 1 });

            let mut values = Vec::new();
            for _ in 0..3 {
                buffers.sample(
                    op,
                    if op == "copy" {
                        Duration::from_secs(1)
                    } else {
                        Duration::from_millis(200)
                    },
                    origin,
                )?;
                let sample = buffers.sample(op, duration, origin)?;
                values.push(sample.gb_per_second);
                samples.push(sample);
            }
            metrics.push(metric(
                &format!("ram/{op}/{bytes}"),
                "ram",
                &format!(
                    "RAM · {}",
                    match op {
                        "read" => "Lecture",
                        "write" => "Écriture",
                        _ => "Copie",
                    }
                ),
                "Go/s logiques",
                values,
                &profile(op, bytes, duration.as_secs()),
            ));
        }
        drop(buffers);
        let latency = latency(bytes, Duration::from_millis(300))?;
        Ok::<_, String>((
            metrics,
            serde_json::json!({"buffer_bytes":bytes,"observed_llc_bytes":llc,"samples":samples,"latency":latency,"method":"logical bytes, copy=read+write; write uses mfence, no forced cache flush; buffers allocated and prefaulted before timing"}),
        ))
    })();
    match result{Ok((metrics,raw))=>Category{id:"ram".into(),state:State::Executed,elapsed_seconds:started.elapsed().as_secs_f64(),metrics,raw,notes:vec!["Débit logique de gros buffers, pas un compteur du bus DRAM. Latence de parcours dépendant incluant TLB et caches résiduels.".into()]},Err(e)=>Category::missing("ram",State::Failed,e)}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn kernels_validate_nonuniform_memory() {
        let mut b = Buffers::new(1024 * 1024).unwrap();
        for op in ["read", "write", "copy"] {
            let s = b
                .sample(op, Duration::from_millis(10), Instant::now())
                .unwrap();
            assert!(s.verified && s.passes > 0 && s.logical_bytes > 0);
        }
    }
    #[test]
    fn dependent_chain_is_valid() {
        assert!(latency(65536, Duration::from_millis(5))
            .unwrap()
            .iter()
            .all(|s| s.verified && s.ns_per_access > 0.));
    }
}
