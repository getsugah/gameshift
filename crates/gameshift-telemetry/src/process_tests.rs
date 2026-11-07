use super::{gpus, processes::*};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!("gameshift-fdinfo-{}-{n}", std::process::id()));
        fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn gpu(&self, pci: &str, vendor: &str, driver: &str) {
        let path = self.0.join("sys/bus/pci/devices").join(pci);
        fs::create_dir_all(path.join("drm/renderD140")).unwrap();
        fs::write(path.join("vendor"), vendor).unwrap();
        fs::write(path.join("device"), "0x1234").unwrap();
        fs::write(path.join("class"), "0x030000").unwrap();
        std::os::unix::fs::symlink(
            format!("/sys/bus/pci/drivers/{driver}"),
            path.join("driver"),
        )
        .unwrap();
    }
    fn process(&self, pid: u32, start: u64) {
        let path = self.0.join("proc").join(pid.to_string());
        fs::create_dir_all(path.join("fdinfo")).unwrap();
        fs::create_dir_all(path.join("fd")).unwrap();
        let mut fields = vec!["0".to_owned(); 20];
        fields[0] = "R".into();
        fields[19] = start.to_string();
        fs::write(
            path.join("stat"),
            format!("{pid} (a process with ) parentheses) {}", fields.join(" ")),
        )
        .unwrap();
        fs::write(path.join("comm"), "application").unwrap();
    }
    fn info(&self, pid: u32, fd: u32, pci: &str, id: u32, ns: u64) {
        fs::write(self.0.join(format!("proc/{pid}/fdinfo/{fd}")),format!("drm-driver: amdgpu\ndrm-pdev: {pci}\ndrm-client-id: {id}\ndrm-engine-gfx: {ns} ns\ndrm-engine-capacity-gfx: 2\ndrm-memory-vram: 10 KiB\ndrm-memory-gtt: 2 MiB\n")).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn parser_respects_units_resident_precedence_and_unknown_counters() {
    let c=parse_fdinfo("drm-driver: xe\ndrm-client-id: 7\ndrm-memory-vram: 9 KiB\ndrm-resident-vram: 2 MiB\ndrm-resident-system: 512\ndrm-engine-render: 100000 ns\ndrm-engine-capacity-render: 2\ndrm-engine-video: 20 bananas\ndrm-cycles-compute: 12\n").unwrap();
    assert_eq!(c.memory_bytes["vram"], 2 * 1048576);
    assert_eq!(c.memory_bytes["system"], 512);
    assert_eq!(c.engines_ns["render"], 100000);
    assert!(!c.engines_ns.contains_key("video"));
    assert!(!c.engines_ns.contains_key("compute"));
    assert!(parse_fdinfo("pos: 0\nflags: 0100\n").is_none());
}
#[test]
fn discovery_is_vendor_neutral_and_never_assumes_indices() {
    let f = Fixture::new();
    for (pci, vendor, driver) in [
        ("0000:04:00.0", "0x10de", "nvidia"),
        ("0000:02:00.0", "0x8086", "xe"),
        ("0000:09:00.0", "0x1002", "amdgpu"),
    ] {
        f.gpu(pci, vendor, driver);
    }
    let devices = gpus::discover_at(&f.0).unwrap();
    assert_eq!(devices.len(), 3);
    assert_eq!(devices[0].driver.as_deref(), Some("xe"));
    assert_eq!(devices[1].driver.as_deref(), Some("nvidia"));
    assert!(devices.iter().all(|g| g.nodes == ["/dev/dri/renderD140"]));
}
#[test]
fn duplicated_fds_shared_clients_and_same_ids_on_other_gpus_are_separate() {
    let f = Fixture::new();
    f.gpu("0000:09:00.0", "0x1002", "amdgpu");
    f.gpu("0000:04:00.0", "0x8086", "i915");
    for pid in [42, 43] {
        f.process(pid, 100);
        f.info(pid, 3, "0000:09:00.0", 7, 0);
    }
    f.info(42, 4, "0000:09:00.0", 7, 0);
    f.info(42, 5, "0000:04:00.0", 7, 0);
    let snapshot = Tracker::default().sample_at(&f.0, Instant::now());
    assert_eq!(snapshot.rows.len(), 3);
    let row = snapshot
        .rows
        .iter()
        .find(|r| r.pid == 42 && r.pci == "0000:09:00.0")
        .unwrap();
    assert_eq!(row.clients, 1);
    assert_eq!(row.memory_bytes["vram"], 10 * 1024);
    assert!(row.shared_clients);
    assert_eq!(row.activity(), None);
}
#[test]
fn deltas_use_capacity_keep_high_water_marks_and_reset_after_pause() {
    let f = Fixture::new();
    f.gpu("0000:09:00.0", "0x1002", "amdgpu");
    f.process(42, 100);
    f.info(42, 3, "0000:09:00.0", 7, 0);
    let mut tracker = Tracker::default();
    let now = Instant::now();
    assert_eq!(tracker.sample_at(&f.0, now).rows[0].activity(), None);
    f.info(42, 3, "0000:09:00.0", 7, 1_000_000_000);
    assert_eq!(
        tracker.sample_at(&f.0, now + Duration::from_secs(2)).rows[0].activity(),
        Some(25.)
    );
    f.info(42, 3, "0000:09:00.0", 7, 500_000_000);
    assert_eq!(
        tracker.sample_at(&f.0, now + Duration::from_secs(4)).rows[0].activity(),
        None
    );
    f.info(42, 3, "0000:09:00.0", 7, 1_400_000_000);
    assert_eq!(
        tracker.sample_at(&f.0, now + Duration::from_secs(6)).rows[0].activity(),
        Some(10.)
    );
    tracker.pause();
    assert_eq!(
        tracker.sample_at(&f.0, now + Duration::from_secs(10)).rows[0].activity(),
        None
    );
}
#[test]
fn exited_processes_and_pid_reuse_do_not_inherit_counters() {
    let f = Fixture::new();
    f.gpu("0000:09:00.0", "0x1002", "amdgpu");
    f.process(42, 100);
    f.info(42, 3, "0000:09:00.0", 7, 100);
    let mut tracker = Tracker::default();
    let now = Instant::now();
    tracker.sample_at(&f.0, now);
    f.process(42, 200);
    assert_eq!(
        tracker.sample_at(&f.0, now + Duration::from_secs(1)).rows[0].activity(),
        None
    );
    fs::remove_dir_all(f.0.join("proc/42")).unwrap();
    assert!(tracker
        .sample_at(&f.0, now + Duration::from_secs(2))
        .rows
        .is_empty());
}
