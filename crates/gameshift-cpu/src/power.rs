//! CPU package interval power. Other CPU telemetry and benchmark protocols stay unchanged.
use gameshift_core::rapl::{self, Counter, Snapshot};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::Path,
    time::{Duration, Instant},
};
#[derive(Clone, Debug, Default)]
pub struct Reading {
    pub watts: Option<f64>,
    pub detail: String,
}
#[derive(Default)]
pub struct Tracker {
    clock_id: String,
    previous: BTreeMap<String, Counter>,
}
pub fn watts(before: &Counter, after: &Counter) -> Option<f64> {
    let ns = after.at_ns.checked_sub(before.at_ns)?;
    if !(100_000_000..=10_000_000_000).contains(&ns)
        || before.range_uj != after.range_uj
        || before.inode != after.inode
        || after.range_uj == 0
        || before.energy_uj >= before.range_uj
        || after.energy_uj >= after.range_uj
    {
        return None;
    }
    let delta = if after.energy_uj >= before.energy_uj {
        after.energy_uj - before.energy_uj
    } else {
        let delta = before
            .range_uj
            .checked_sub(before.energy_uj)?
            .checked_add(after.energy_uj)?;
        if delta > before.range_uj / 2 {
            return None;
        }
        delta
    };
    Some(delta as f64 * 1000. / ns as f64).filter(|w| w.is_finite() && *w >= 0.)
}
impl Tracker {
    pub fn reset(&mut self) {
        self.previous.clear();
        self.clock_id.clear();
    }
    pub fn update(&mut self, snapshot: Snapshot) -> Reading {
        if snapshot.clock_id != self.clock_id {
            self.previous.clear();
            self.clock_id = snapshot.clock_id;
        }
        let mut ready = !snapshot.packages.is_empty() && snapshot.errors.is_empty();
        let mut total = 0.;
        let mut detail = snapshot.errors;
        let mut next = BTreeMap::new();
        let mut names = std::collections::BTreeSet::new();
        for package in snapshot.packages {
            if !names.insert(package.name.clone()) {
                ready = false;
                detail.push("Packages redondants : total refusé".into());
            }
            if let Some(counter) = package.counter {
                let power = self
                    .previous
                    .get(&package.id)
                    .and_then(|old| watts(old, &counter));
                if let Some(power) = power {
                    total += power;
                    let old = &self.previous[&package.id];
                    detail.push(format!(
                        "{} : {power:.1} W moyens sur {:.2} s · {}",
                        package.name,
                        (counter.at_ns - old.at_ns) as f64 / 1e9,
                        package.id
                    ));
                } else {
                    ready = false;
                    detail.push(format!(
                        "{} : premier relevé ou intervalle/compteur invalide",
                        package.name
                    ));
                }
                next.insert(package.id, counter);
            } else {
                ready = false;
                detail.push(format!(
                    "{} : {}",
                    package.name,
                    package.error.as_deref().unwrap_or("Compteur inaccessible")
                ));
            }
        }
        if self.previous.keys().any(|id| !next.contains_key(id)) {
            ready = false;
            detail.push("Un domaine package a disparu ; nouvelle référence nécessaire".into());
        }
        self.previous = next;
        Reading {
            watts: ready.then_some(total).filter(|w| w.is_finite()),
            detail: detail.join("\n"),
        }
    }
}
#[derive(Default)]
pub struct Sampler {
    tracker: Tracker,
    last: Option<Instant>,
    cached: Reading,
}
impl Sampler {
    pub fn reset(&mut self) {
        self.tracker.reset();
        self.last = None;
        self.cached = Reading::default();
    }
    pub fn sample(&mut self) -> Reading {
        if self
            .last
            .is_some_and(|t| t.elapsed() < Duration::from_secs(1))
        {
            return self.cached.clone();
        }
        self.last = Some(Instant::now());
        let direct = rapl::read(Path::new("/"));
        let (snapshot, source) = if direct.permission_denied {
            match helper() {
                Ok(s) => (s, "RAPL via le helper autorisé"),
                Err(error) => {
                    self.tracker.reset();
                    self.cached = Reading {
                        watts: None,
                        detail: format!(
                            "Lecture RAPL refusée. {error}. Mettre à jour/activer le composant système GameShift via Polkit, sans lancer l’application en root."
                        ),
                    };
                    return self.cached.clone();
                }
            }
        } else {
            (direct, "RAPL powercap, lecture directe")
        };
        self.cached = self.tracker.update(snapshot);
        self.cached.detail = format!(
            "Puissance moyenne du ou des packages CPU · {source}\n{}",
            self.cached.detail
        );
        self.cached.clone()
    }
}
fn helper() -> Result<Snapshot, String> {
    let mut stream =
        UnixStream::connect("/run/gameshift-control-manager.sock").map_err(|e| e.to_string())?;
    use std::os::fd::AsRawFd;
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    } != 0
        || credentials.uid != 0
    {
        return Err("Socket RAPL non détenu par le service système".into());
    }
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_millis(250)))
        .map_err(|e| e.to_string())?;
    stream.write_all(b"RAPL1\n").map_err(|e| e.to_string())?;
    let mut response = Vec::new();
    stream
        .take(65537)
        .read_to_end(&mut response)
        .map_err(|e| e.to_string())?;
    if response.len() > 65536 {
        return Err("Réponse RAPL trop volumineuse".into());
    }
    let snapshot:Snapshot=serde_json::from_slice(&response).map_err(|_|"Helper installé incompatible avec la lecture RAPL ; utiliser Activer / mettre à jour les composants".to_owned())?;
    if snapshot.clock_id.is_empty() || snapshot.packages.len() > 64 || snapshot.errors.len() > 512 {
        return Err("Réponse RAPL invalide".into());
    }
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gameshift_core::rapl::Package;
    fn counter(energy: u64, time: u64) -> Counter {
        Counter {
            energy_uj: energy,
            range_uj: 65_532_610_987,
            at_ns: time,
            inode: 1,
        }
    }
    fn snapshot(energies: &[u64], time: u64) -> Snapshot {
        Snapshot {
            clock_id: "fixture".into(),
            packages: energies
                .iter()
                .enumerate()
                .map(|(i, e)| Package {
                    id: format!("package-zone-{i}"),
                    name: format!("package-{i}"),
                    counter: Some(counter(*e, time)),
                    error: None,
                })
                .collect(),
            ..Default::default()
        }
    }
    #[test]
    fn computes_measured_watts_and_real_zero() {
        let a = counter(0, 1_000_000_000);
        let b = counter(34_480_000, 2_000_000_000);
        assert!((watts(&a, &b).unwrap() - 34.48).abs() < 1e-9);
        assert_eq!(watts(&a, &counter(0, 2_000_000_000)), Some(0.));
    }
    #[test]
    fn handles_one_wrap_with_each_domains_actual_range() {
        for range in [1000, 65_532_610_987] {
            let a = Counter {
                energy_uj: range - 100,
                range_uj: range,
                at_ns: 0,
                inode: 2,
            };
            let b = Counter {
                energy_uj: 100,
                at_ns: 1_000_000_000,
                ..a.clone()
            };
            assert_eq!(watts(&a, &b), Some(0.0002));
        }
    }
    #[test]
    fn invalid_intervals_resets_changed_ranges_and_inode_changes_are_gaps() {
        let a = counter(100, 1_000_000_000);
        for b in [
            counter(200, 1_000_000_000),
            counter(200, 0),
            counter(200, 20_000_000_000),
            Counter {
                range_uj: 0,
                ..counter(200, 2_000_000_000)
            },
            Counter {
                range_uj: 1000,
                ..counter(200, 2_000_000_000)
            },
            Counter {
                inode: 2,
                ..counter(200, 2_000_000_000)
            },
            counter(a.range_uj, 2_000_000_000),
        ] {
            assert_eq!(watts(&a, &b), None);
        }
        let a = counter(100_000, 1_000_000_000);
        assert_eq!(watts(&a, &counter(0, 2_000_000_000)), None);
    }
    #[test]
    fn sums_only_distinct_complete_packages_and_warms_up_after_pause() {
        let mut tracker = Tracker::default();
        assert_eq!(tracker.update(snapshot(&[0, 0], 0)).watts, None);
        assert_eq!(
            tracker
                .update(snapshot(&[10_000_000, 20_000_000], 1_000_000_000))
                .watts,
            Some(30.)
        );
        tracker.reset();
        assert_eq!(
            tracker
                .update(snapshot(&[10_000_000, 20_000_000], 2_000_000_000))
                .watts,
            None
        );
    }
    #[test]
    fn missing_failed_and_appearing_packages_never_report_a_partial_total() {
        let mut tracker = Tracker::default();
        tracker.update(snapshot(&[0, 0], 0));
        assert_eq!(
            tracker.update(snapshot(&[10_000_000], 1_000_000_000)).watts,
            None
        );
        assert_eq!(
            tracker.update(snapshot(&[20_000_000], 2_000_000_000)).watts,
            Some(10.)
        );
        assert_eq!(
            tracker
                .update(snapshot(&[30_000_000, 0], 3_000_000_000))
                .watts,
            None
        );
        let mut failure = snapshot(&[40_000_000, 10_000_000], 4_000_000_000);
        failure.packages[1].counter = None;
        failure.packages[1].error = Some("Permission denied".into());
        assert_eq!(tracker.update(failure).watts, None);
        assert_eq!(tracker.update(snapshot(&[], 5_000_000_000)).watts, None);
    }
    #[test]
    fn duplicate_domains_and_helper_restarts_invalidate_total() {
        let mut tracker = Tracker::default();
        tracker.update(snapshot(&[0], 0));
        let mut duplicated = snapshot(&[10_000_000], 1_000_000_000);
        duplicated.packages.push(duplicated.packages[0].clone());
        assert_eq!(tracker.update(duplicated).watts, None);
        let mut restarted = snapshot(&[20_000_000], 2_000_000_000);
        restarted.clock_id = "new-process".into();
        assert_eq!(tracker.update(restarted).watts, None);
    }
    #[test]
    fn cpu_power_is_not_added_to_saved_benchmark_environment() {
        let mut cpu = crate::environment::Snapshot::read(&[]);
        cpu.package_power = Some(Reading {
            watts: Some(34.5),
            detail: "Test".into(),
        });
        assert!(
            serde_json::to_value(cpu)
                .unwrap()
                .get("package_power")
                .is_none()
        );
    }
}
