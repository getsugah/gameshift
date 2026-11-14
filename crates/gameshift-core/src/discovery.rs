//! Path/metadata cache for read-only monitoring; measurements are never cached here.
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const CHECK: Duration = Duration::from_secs(1);
const RETRY: Duration = Duration::from_secs(5);
const AUDIT: Duration = Duration::from_secs(60);
type Stamp = Option<(u64, u64, i64, i64, i64, i64, u32)>;
fn stamp(path: &Path) -> Stamp {
    fs::metadata(path).ok().map(|m| {
        (
            m.dev(),
            m.ino(),
            m.mtime(),
            m.mtime_nsec(),
            m.ctime(),
            m.ctime_nsec(),
            m.mode(),
        )
    })
}
/// Watches directory/driver metadata without enumerating directories or opening labels.
/// A bounded audit handles sysfs drivers that do not update directory timestamps.
pub struct Cache<T> {
    root: PathBuf,
    value: Option<T>,
    watched: Vec<(PathBuf, Stamp)>,
    discovered: Option<Instant>,
    checked: Option<Instant>,
    failures: u8,
    retry: Duration,
}
impl<T> Default for Cache<T> {
    fn default() -> Self {
        Self {
            root: PathBuf::new(),
            value: None,
            watched: Vec::new(),
            discovered: None,
            checked: None,
            failures: 0,
            retry: RETRY,
        }
    }
}
impl<T> Cache<T> {
    pub fn get(
        &mut self,
        root: &Path,
        now: Instant,
        discover: impl FnOnce(&Path) -> (T, Vec<PathBuf>),
    ) -> &T {
        let age = self.discovered.map(|t| now.saturating_duration_since(t));
        let ready = age.is_none_or(|age| age >= RETRY);
        let check = ready
            && self
                .checked
                .is_none_or(|t| now.saturating_duration_since(t) >= CHECK);
        let changed = if check {
            self.checked = Some(now);
            self.watched.iter().any(|(path, old)| stamp(path) != *old)
        } else {
            false
        };
        let retry_due = self.failures >= 3 && age.is_some_and(|age| age >= self.retry);
        if self.value.is_none()
            || self.root != root
            || (ready && (changed || retry_due || age.is_some_and(|age| age >= AUDIT)))
        {
            self.retry = if retry_due && !changed {
                self.retry.saturating_mul(2).min(AUDIT)
            } else {
                RETRY
            };
            let (value, mut paths) = discover(root);
            paths.sort();
            paths.dedup();
            self.watched = paths
                .into_iter()
                .map(|path| {
                    let old = stamp(&path);
                    (path, old)
                })
                .collect();
            self.value = Some(value);
            self.root = root.to_owned();
            self.discovered = Some(now);
            self.checked = Some(now);
            self.failures = 0;
        }
        self.value.as_ref().expect("discovered value")
    }
    /// Invalid readings stay unavailable; three consecutive failures request a rebuild.
    pub fn failed(&mut self, failed: bool) {
        if !failed {
            self.retry = RETRY;
        }
        self.failures = if failed {
            self.failures.saturating_add(1)
        } else {
            0
        };
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    #[test]
    fn steady_state_errors_topology_and_audit_are_bounded() {
        let dir = std::env::temp_dir().join(format!("gameshift-discovery-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("topology");
        let count = Cell::new(0);
        let discover = |_: &Path| {
            count.set(count.get() + 1);
            (count.get(), vec![target.clone()])
        };
        let mut cache = Cache::default();
        let start = Instant::now();
        assert_eq!(*cache.get(&dir, start, discover), 1);
        for i in 1..10 {
            cache.get(&dir, start + Duration::from_millis(i * 500), discover);
        }
        assert_eq!(count.get(), 1);
        for _ in 0..3 {
            cache.failed(true);
        }
        cache.get(&dir, start + Duration::from_secs(4), discover);
        assert_eq!(count.get(), 1);
        cache.get(&dir, start + Duration::from_secs(5), discover);
        assert_eq!(count.get(), 2);
        fs::create_dir(&target).unwrap();
        cache.get(&dir, start + Duration::from_secs(9), discover);
        assert_eq!(count.get(), 2);
        cache.get(&dir, start + Duration::from_secs(10), discover);
        assert_eq!(count.get(), 3);
        cache.get(&dir, start + Duration::from_secs(69), discover);
        assert_eq!(count.get(), 3);
        cache.get(&dir, start + Duration::from_secs(70), discover);
        assert_eq!(count.get(), 4);
        cache.failed(true);
        cache.failed(false);
        cache.get(&dir, start + Duration::from_secs(75), discover);
        assert_eq!(count.get(), 4);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn persistent_errors_back_off_without_delaying_measurement_calls() {
        let root = Path::new("/nonexistent-gameshift-discovery-root");
        let now = Instant::now();
        let count = Cell::new(0);
        let discover = |_: &Path| {
            count.set(count.get() + 1);
            ((), Vec::new())
        };
        let mut cache = Cache::default();
        for second in 0..=59 {
            cache.get(root, now + Duration::from_secs(second), discover);
            for _ in 0..3 {
                cache.failed(true);
            }
        }
        // Discoveries at 0, 5, 15, 35 seconds, not at every failed sample.
        assert_eq!(count.get(), 4);
        cache.failed(false);
        for _ in 0..3 {
            cache.failed(true);
        }
        cache.get(root, now + Duration::from_secs(60), discover);
        assert_eq!(count.get(), 5);
    }
}
