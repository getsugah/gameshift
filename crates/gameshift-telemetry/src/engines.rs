//! Vendor-neutral engine readings; currently only AMDGPU supplies the hardware counters.
use crate::{gpus::Gpu, processes};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    time::Duration,
};

#[derive(Debug, Clone, Default)]
pub struct Activity {
    /// Cumulative client time per wall-clock second, never hardware occupancy.
    pub milliseconds_per_second: Option<f64>,
    pub clients: usize,
    pub pending_clients: usize,
    pub regressed_clients: usize,
    pub invalid_intervals: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Available,
    Unavailable,
    Impossible,
}
#[derive(Debug, Clone)]
pub struct Reading {
    pub state: State,
    pub sampled_at: Option<std::time::Instant>,
    pub title: &'static str,
    pub percent: bool,
    pub detected: bool,
    pub value: Option<f64>,
    pub source: String,
    pub detail: String,
}

pub fn discover_at(root: &Path) -> Result<Vec<Gpu>, String> {
    let entries = fs::read_dir(root.join("sys/class/drm"))
        .map_err(|e| format!("Découverte DRM indisponible : {e}"))?;
    let mut addresses = BTreeSet::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name
            .strip_prefix("card")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        {
            continue;
        }
        if let Ok(path) = fs::canonicalize(entry.path().join("device")) {
            if let Some(pci) = path
                .file_name()
                .and_then(|s| s.to_str())
                .and_then(|s| gameshift_control::canonical_pci(s).ok())
            {
                addresses.insert(pci);
            }
        }
    }
    Ok(crate::gpus::discover_at(root)?
        .into_iter()
        .filter(|g| addresses.contains(&g.pci) && g.driver.as_deref() == Some("amdgpu"))
        .collect())
}

/// Explicit hardware percent; no clamping and no guessed substitute source.
#[derive(Debug)]
enum PercentError {
    Unavailable(String),
    Impossible(String),
}
impl PercentError {
    fn message(&self) -> &str {
        match self {
            Self::Unavailable(s) | Self::Impossible(s) => s,
        }
    }
    fn state(&self) -> State {
        match self {
            Self::Unavailable(_) => State::Unavailable,
            Self::Impossible(_) => State::Impossible,
        }
    }
}
fn percent_read(root: &Path, pci: &str, file: &str) -> Result<f64, PercentError> {
    let pci = gameshift_control::canonical_pci(pci).map_err(PercentError::Impossible)?;
    if !matches!(
        file,
        "gpu_busy_percent" | "mem_busy_percent" | "vcn_busy_percent"
    ) {
        return Err(PercentError::Impossible(
            "Compteur non pris en charge".into(),
        ));
    }
    let text = fs::read_to_string(root.join("sys/bus/pci/devices").join(pci).join(file))
        .map_err(|e| PercentError::Unavailable(e.to_string()))?;
    text.trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && (0. ..=100.).contains(v))
        .ok_or_else(|| {
            PercentError::Impossible(
                "Pourcentage matériel hors plage ou invalide, valeur non plafonnée".into(),
            )
        })
}
pub fn hardware_percent(root: &Path, pci: &str, file: &str) -> Result<f64, String> {
    percent_read(root, pci, file).map_err(|e| e.message().to_owned())
}

pub fn readings(root: &Path, pci: &str, snapshot: &processes::Snapshot) -> Vec<Reading> {
    let engines = snapshot.engine_activity.get(pci);
    let mut out = Vec::new();
    for (title, prefix) in [
        ("3D / GFX", "gfx"),
        ("Compute", "compute"),
        ("Encodage vidéo", "enc"),
    ] {
        let matching = engines
            .into_iter()
            .flat_map(|e| e.iter())
            .filter(|(name, _)| engine_family(name) == Some(prefix))
            .collect::<Vec<_>>();
        let ready = !matching.is_empty()
            && matching
                .iter()
                .all(|(_, a)| a.milliseconds_per_second.is_some());
        let value = ready.then(|| {
            matching
                .iter()
                .filter_map(|(_, a)| a.milliseconds_per_second)
                .sum::<f64>()
        });
        let invalid = matching
            .iter()
            .any(|(_, a)| a.regressed_clients > 0 || a.invalid_intervals > 0)
            || value.is_some_and(|v| !v.is_finite() || v < 0.);
        out.push(Reading {
            state:if invalid {State::Impossible}else if ready {State::Available}else{State::Unavailable},
            sampled_at:snapshot.engine_sampled_at,title,percent:false,detected:!matching.is_empty(),value:if invalid {None}else{value},
            source:format!("DRM fdinfo · {pci} · {}",matching.iter().map(|(name,_)|format!("drm-engine-{name}")).collect::<Vec<_>>().join(", ")),
            detail:format!("Activité cumulée des clients DRM observables · ms/s = Σ Δtemps client (ns) / Δtemps monotone (s) / 10⁶. Conversion fiable en % indisponible : les clients et instances peuvent se chevaucher ; ni leur union temporelle ni une capacité matérielle globale non ambiguë ne sont conservées dans cet agrégat. Diviser par 10 donnerait un indice cumulé pouvant dépasser 100 %, pas une occupation matérielle. Aucun plafonnement. {} relevé(s) client-moteur, {} en attente, {} compteur(s) régressif(s), {} intervalle(s) invalide(s). {} accès refusé(s), {} association(s) ambiguë(s). {} {}",
                matching.iter().map(|(_,a)|a.clients).sum::<usize>(),matching.iter().map(|(_,a)|a.pending_clients).sum::<usize>(),matching.iter().map(|(_,a)|a.regressed_clients).sum::<usize>(),matching.iter().map(|(_,a)|a.invalid_intervals).sum::<usize>(),snapshot.denied,snapshot.ambiguous,
                if snapshot.truncated {"Parcours partiel."} else {""},snapshot.engine_issues.join(" · ")),
        });
    }
    let path = root
        .join("sys/bus/pci/devices")
        .join(pci)
        .join("vcn_busy_percent");
    let value = percent_read(root, pci, "vcn_busy_percent");
    out.push(Reading {
        state:match &value {Ok(_)=>State::Available,Err(e)=>e.state()},
        sampled_at:Some(std::time::Instant::now()),title:"Vidéo / VCN",percent:true,detected:path.try_exists().unwrap_or(true),value:value.as_ref().ok().copied(),
        source:format!("sysfs · {pci}/vcn_busy_percent"),
        detail:match value {Ok(_)=>"Pourcentage matériel du bloc vidéo global, utilisé directement (aucune conversion DRM). Échelle fixe 0–100 %. VCN peut inclure l’encodage ; ce n’est pas un compteur de décodage indépendant.".into(),Err(e)=>format!("{} : {}. Pourcentage matériel VCN, échelle fixe 0–100 %, sans plafonnement ni remplacement par zéro. VCN inclut éventuellement l’encodage.",if e.state()==State::Impossible {"Mesure impossible"}else{"Non disponible"},e.message())},
    });
    out
}
fn engine_family(name: &str) -> Option<&'static str> {
    // AMDGPU aliases only. Unknown names remain in the source snapshot, never guessed as decode.
    for (family, aliases) in [
        ("gfx", &["gfx", "graphics", "3d"][..]),
        ("compute", &["compute"][..]),
        ("enc", &["enc", "encode"][..]),
    ] {
        if aliases.iter().any(|alias| {
            name == *alias
                || name.strip_prefix(alias).is_some_and(|suffix| {
                    !suffix.trim_start_matches(['-', '_']).is_empty()
                        && suffix
                            .trim_start_matches(['-', '_'])
                            .bytes()
                            .all(|b| b.is_ascii_digit())
                })
        }) {
            return Some(family);
        }
    }
    None
}

// Counter arithmetic is kept with the existing fdinfo tracker, before its legacy per-process formatting.
pub(crate) type Identity = (String, String, u64);
pub(crate) type Owners = BTreeSet<(u32, u64)>;
pub(crate) type Previous = BTreeMap<Identity, (processes::Client, Owners)>;
pub(crate) fn deltas(
    observed: Vec<(u32, u64, processes::Client)>,
    previous: &mut Previous,
    dt: Option<Duration>,
) -> BTreeMap<String, BTreeMap<String, Activity>> {
    let mut unique = Previous::new();
    for (pid, start, client) in observed {
        let (Some(pci), Some(id)) = (client.pci.clone(), client.id) else {
            continue;
        };
        let entry = unique
            .entry((pci, client.driver.clone(), id))
            .or_insert_with(|| (client.clone(), BTreeSet::new()));
        entry.1.insert((pid, start));
        for (name, value) in client.engines_ns {
            let old = entry.0.engines_ns.entry(name).or_default();
            *old = (*old).max(value);
        }
    }
    let mut out = BTreeMap::<String, BTreeMap<String, Activity>>::new();
    for (key, (client, owners)) in &mut unique {
        let old = previous
            .get(key)
            .filter(|(_, old_owners)| !owners.is_disjoint(old_owners));
        for (name, ns) in &mut client.engines_ns {
            let activity = out
                .entry(key.0.clone())
                .or_default()
                .entry(name.clone())
                .or_default();
            activity.clients += 1;
            if dt.is_some_and(|d| d.is_zero()) || (old.is_some() && dt.is_none()) {
                activity.invalid_intervals += 1;
            }
            let before = old.and_then(|(c, _)| c.engines_ns.get(name));
            let delta = before.and_then(|before| {
                if *ns < *before {
                    *ns = *before;
                    activity.regressed_clients += 1;
                    None
                } else {
                    Some(*ns - *before)
                }
            });
            let rate = delta
                .zip(dt.filter(|d| !d.is_zero()))
                .map(|(delta, dt)| delta as f64 / dt.as_secs_f64() / 1e6);
            if let Some(rate) = rate {
                activity.milliseconds_per_second =
                    Some(activity.milliseconds_per_second.unwrap_or(0.) + rate);
            } else {
                activity.pending_clients += 1;
            }
        }
    }
    for engines in out.values_mut() {
        for activity in engines.values_mut() {
            if activity.pending_clients > 0 {
                activity.milliseconds_per_second = None;
            }
        }
    }
    *previous = unique;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn client(pci: &str, id: u64, ns: u64) -> processes::Client {
        processes::parse_fdinfo(&format!(
            "drm-driver: amdgpu\ndrm-pdev: {pci}\ndrm-client-id: {id}\ndrm-engine-gfx: {ns} ns\n"
        ))
        .unwrap()
    }
    const A: &str = "0000:05:00.0";
    const B: &str = "0000:08:00.0";
    #[test]
    fn deduplicates_shared_clients_separates_cards_and_never_caps_rates() {
        let mut previous = Previous::new();
        let observations = |ns| {
            vec![
                (1, 10, client(A, 7, ns)),
                (2, 20, client(A, 7, ns)),
                (1, 10, client(B, 7, ns / 2)),
            ]
        };
        assert_eq!(
            deltas(observations(0), &mut previous, None)[A]["gfx"].milliseconds_per_second,
            None
        );
        let out = deltas(
            observations(2_000_000_000),
            &mut previous,
            Some(Duration::from_secs(1)),
        );
        assert_eq!(out[A]["gfx"].milliseconds_per_second, Some(2000.));
        assert_eq!(out[A]["gfx"].clients, 1);
        assert_eq!(out[B]["gfx"].milliseconds_per_second, Some(1000.));
        let out = deltas(
            observations(2_000_000_000),
            &mut previous,
            Some(Duration::from_secs(1)),
        );
        assert_eq!(out[A]["gfx"].milliseconds_per_second, Some(0.));
    }
    #[test]
    fn regression_holds_high_water_and_new_owners_require_baseline() {
        let mut previous = Previous::new();
        let mut read = |pid, start, ns| {
            deltas(
                vec![(pid, start, client(A, 7, ns))],
                &mut previous,
                Some(Duration::from_secs(1)),
            )[A]["gfx"]
                .clone()
        };
        assert_eq!(read(1, 10, 1_000_000_000).milliseconds_per_second, None);
        assert_eq!(read(1, 10, 10).regressed_clients, 1);
        assert_eq!(
            read(1, 10, 1_100_000_000).milliseconds_per_second,
            Some(100.)
        );
        assert_eq!(read(1, 11, 10).milliseconds_per_second, None);
        assert_eq!(read(2, 11, 20).milliseconds_per_second, None);
        assert_eq!(read(2, 11, 20).milliseconds_per_second, Some(0.));
    }
    #[test]
    fn new_and_vanished_clients_and_zero_intervals_are_gaps() {
        let mut previous = Previous::new();
        deltas(vec![(1, 10, client(A, 1, 0))], &mut previous, None);
        let out = deltas(
            vec![(1, 10, client(A, 1, 100)), (2, 20, client(A, 2, 100))],
            &mut previous,
            Some(Duration::from_secs(1)),
        );
        assert_eq!(out[A]["gfx"].milliseconds_per_second, None);
        let out = deltas(
            vec![(1, 10, client(A, 1, 100))],
            &mut previous,
            Some(Duration::from_secs(1)),
        );
        assert_eq!(out[A]["gfx"].milliseconds_per_second, Some(0.));
        assert_eq!(
            deltas(
                vec![(1, 10, client(A, 1, 200))],
                &mut previous,
                Some(Duration::ZERO)
            )[A]["gfx"]
                .milliseconds_per_second,
            None
        );
        assert!(deltas(vec![], &mut previous, Some(Duration::from_secs(1))).is_empty());
    }
    #[test]
    fn aliases_never_guess_decode_or_unknown_names() {
        assert_eq!(engine_family("compute_1"), Some("compute"));
        assert_eq!(engine_family("gfx0"), Some("gfx"));
        for name in ["dec", "video", "compute_", "gfxunknown"] {
            assert_eq!(engine_family(name), None);
        }
    }
}

#[cfg(test)]
mod interval_tests {
    use super::*;
    const PCI: &str = "0000:05:00.0";
    fn client(id: u64, ns: u64) -> processes::Client {
        processes::parse_fdinfo(&format!("drm-driver: amdgpu\ndrm-pdev: {PCI}\ndrm-client-id: {id}\ndrm-engine-compute: {ns} ns\ndrm-engine-capacity-compute: 2\n")).unwrap()
    }
    #[test]
    fn variable_intervals_and_multiple_clients_preserve_observed_time_not_a_capped_percent() {
        for (interval, ns, expected) in [
            (Duration::from_millis(500), 100_000_000, 200.),
            (Duration::from_secs(2), 100_000_000, 50.),
            (Duration::from_secs(1), 2_000_000_000, 2000.),
        ] {
            let mut previous = Previous::new();
            deltas(
                vec![(1, 10, client(1, 0)), (2, 20, client(2, 0))],
                &mut previous,
                None,
            );
            let out = deltas(
                vec![(1, 10, client(1, ns)), (2, 20, client(2, ns))],
                &mut previous,
                Some(interval),
            );
            assert_eq!(
                out[PCI]["compute"].milliseconds_per_second,
                Some(expected * 2.)
            );
            let snapshot = processes::Snapshot {
                engine_activity: out,
                ..Default::default()
            };
            let readings = readings(Path::new("/nonexistent-test-root"), PCI, &snapshot);
            assert!(!readings[1].percent);
            assert_eq!(readings[1].value, Some(expected * 2.));
            assert_eq!(readings[1].state, State::Available);
            assert!(
                readings[1]
                    .detail
                    .contains("Conversion fiable en % indisponible")
            );
        }
    }
    #[test]
    fn first_samples_are_unavailable_but_invalid_intervals_and_regressions_are_impossible() {
        let mut previous = Previous::new();
        let first = deltas(vec![(1, 10, client(1, 100))], &mut previous, None);
        let status = |activity| {
            readings(
                Path::new("/nonexistent-test-root"),
                PCI,
                &processes::Snapshot {
                    engine_activity: activity,
                    ..Default::default()
                },
            )[1]
            .state
        };
        assert_eq!(status(first), State::Unavailable);
        assert_eq!(
            status(deltas(
                vec![(1, 10, client(1, 110))],
                &mut previous,
                Some(Duration::ZERO)
            )),
            State::Impossible
        );
        assert_eq!(
            status(deltas(vec![(1, 10, client(1, 120))], &mut previous, None)),
            State::Impossible
        );
        assert_eq!(
            status(deltas(
                vec![(1, 10, client(1, 10))],
                &mut previous,
                Some(Duration::from_secs(1))
            )),
            State::Impossible
        );
        let caught_up = deltas(
            vec![(1, 10, client(1, 120))],
            &mut previous,
            Some(Duration::from_secs(1)),
        );
        assert_eq!(caught_up[PCI]["compute"].milliseconds_per_second, Some(0.));
        assert_eq!(status(caught_up), State::Available);
    }
}
