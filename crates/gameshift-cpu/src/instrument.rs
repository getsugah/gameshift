//! Thread CPU time and optional Linux hardware events. Never changes perf permissions.
use serde::{Deserialize, Serialize};
use std::{
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    time::Instant,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CounterSample {
    pub event: String,
    pub raw_count: Option<u64>,
    pub time_enabled_ns: Option<u64>,
    pub time_running_ns: Option<u64>,
    pub scaled_count: Option<f64>,
    pub unavailable_reason: Option<String>,
}
struct Event {
    name: &'static str,
    fd: Result<OwnedFd, String>,
}
/// Open on the thread whose work is being measured; descriptors do not inherit to other threads.
pub struct Counters {
    events: Vec<Event>,
}
impl Counters {
    pub fn open(enabled: bool) -> Self {
        let events = if enabled {
            [
                ("cpu-cycles-user", 0u64),
                ("instructions-user", 1),
                ("cache-references-generic-user", 2),
                ("cache-misses-generic-user", 3),
            ]
            .into_iter()
            .map(|(name, config)| Event {
                name,
                fd: open_event(config),
            })
            .collect()
        } else {
            vec![]
        };
        Self { events }
    }
    pub fn start(&mut self) {
        for event in &mut self.events {
            if let Ok(fd) = &event.fd {
                // SAFETY: ioctls operate only on the owned perf descriptor; no pointer argument.
                let ok = unsafe {
                    libc::ioctl(fd.as_raw_fd(), 0x2403, 0) == 0
                        && libc::ioctl(fd.as_raw_fd(), 0x2400, 0) == 0
                };
                if !ok {
                    event.fd = Err(std::io::Error::last_os_error().to_string());
                }
            }
        }
    }
    pub fn stop(&mut self) -> Vec<CounterSample> {
        self.events
            .iter_mut()
            .map(|event| {
                let mut sample = CounterSample {
                    event: event.name.into(),
                    raw_count: None,
                    time_enabled_ns: None,
                    time_running_ns: None,
                    scaled_count: None,
                    unavailable_reason: None,
                };
                match &event.fd {
                    Err(reason) => sample.unavailable_reason = Some(reason.clone()),
                    Ok(fd) => {
                        let mut values = [0u64; 3];
                        // SAFETY: fixed read format is value/time_enabled/time_running, 24 writable bytes.
                        let stopped = unsafe { libc::ioctl(fd.as_raw_fd(), 0x2401, 0) };
                        let read =
                            unsafe { libc::read(fd.as_raw_fd(), values.as_mut_ptr().cast(), 24) };
                        if stopped < 0 || read != 24 {
                            sample.unavailable_reason =
                                Some(std::io::Error::last_os_error().to_string());
                        } else {
                            sample.raw_count = Some(values[0]);
                            sample.time_enabled_ns = Some(values[1]);
                            sample.time_running_ns = Some(values[2]);
                            if values[2] > 0 {
                                sample.scaled_count =
                                    Some(values[0] as f64 * values[1] as f64 / values[2] as f64);
                            } else {
                                sample.unavailable_reason =
                                    Some("Événement non planifié pendant la mesure".into());
                            }
                        }
                    }
                }
                sample
            })
            .collect()
    }
}
fn open_event(config: u64) -> Result<OwnedFd, String> {
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    {
        #[repr(C, align(8))]
        struct Attr([u8; 128]);
        let mut attr = Attr([0; 128]);
        // Linux perf_event_attr ABI: type/size/config, read_format at 32, flags at 40.
        attr.0[4..8].copy_from_slice(&128u32.to_ne_bytes());
        attr.0[8..16].copy_from_slice(&config.to_ne_bytes());
        attr.0[32..40].copy_from_slice(&3u64.to_ne_bytes());
        attr.0[40..48].copy_from_slice(&(1u64 | 32 | 64).to_ne_bytes());
        // SAFETY: initialized aligned attr, calling thread only, no system-wide/kernel sampling.
        let fd = unsafe { libc::syscall(libc::SYS_perf_event_open, &attr, 0, -1, -1, 8) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        // SAFETY: successful syscall returns a new, owned file descriptor.
        Ok(unsafe { OwnedFd::from_raw_fd(fd as i32) })
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        let _ = config;
        Err("Collecteur perf ABI non validé sur cette architecture".into())
    }
}

pub fn thread_cpu_ns() -> Option<u64> {
    let mut value = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: valid writable timespec, clock does not mutate process policy.
    if unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut value) } != 0 {
        return None;
    }
    u64::try_from(value.tv_sec)
        .ok()?
        .checked_mul(1_000_000_000)?
        .checked_add(u64::try_from(value.tv_nsec).ok()?)
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseTiming {
    pub phase: String,
    pub wall_ns: u64,
    pub thread_cpu_ns: Option<u64>,
    pub linux_tid: i64,
    pub worker_index: usize,
}
/// Boundaries include clock overhead; no overhead subtraction or invented utilization.
pub struct PhaseMeter {
    wall: Instant,
    cpu: Option<u64>,
    phase: &'static str,
    worker: usize,
}
impl PhaseMeter {
    pub fn start(phase: &'static str, worker: usize) -> Self {
        Self {
            wall: Instant::now(),
            cpu: thread_cpu_ns(),
            phase,
            worker,
        }
    }
    pub fn finish(self) -> PhaseTiming {
        let mut target = PhaseTiming {
            phase: self.phase.into(),
            wall_ns: 0,
            thread_cpu_ns: None,
            linux_tid: 0,
            worker_index: 0,
        };
        self.finish_into(&mut target);
        target
    }
    pub fn finish_into(self, target: &mut PhaseTiming) {
        target.thread_cpu_ns = thread_cpu_ns()
            .zip(self.cpu)
            .and_then(|(end, start)| end.checked_sub(start));
        target.wall_ns = self.wall.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        target.linux_tid = unsafe { libc::syscall(libc::SYS_gettid) } as i64;
        target.worker_index = self.worker;
    }
}
