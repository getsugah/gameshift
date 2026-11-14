//! Read-only Linux powercap package counters. No caller-supplied paths cross IPC.
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Counter {
    pub energy_uj: u64,
    pub range_uj: u64,
    pub at_ns: u64,
    pub inode: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub id: String,
    pub name: String,
    pub counter: Option<Counter>,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub clock_id: String,
    pub packages: Vec<Package>,
    pub errors: Vec<String>,
    pub permission_denied: bool,
}
fn clock() -> (&'static str, u64) {
    static ORIGIN: OnceLock<(Instant, String)> = OnceLock::new();
    let (origin, id) = ORIGIN.get_or_init(|| {
        (
            Instant::now(),
            format!(
                "{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ),
        )
    });
    (id, origin.elapsed().as_nanos().min(u64::MAX as u128) as u64)
}
fn text(path: &Path) -> std::io::Result<(String, u64)> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_file() {
        return Err(std::io::Error::other("Attribut RAPL non régulier refusé"));
    }
    // sysfs is kernel-owned and its attributes are never followed through a symlink.
    let file = fs::File::open(path)?;
    let mut value = String::new();
    file.take(257).read_to_string(&mut value)?;
    if value.len() > 256 {
        return Err(std::io::Error::other("Attribut RAPL trop volumineux"));
    }
    Ok((value, m.ino()))
}
fn number(value: &str) -> std::io::Result<u64> {
    let value = value.trim();
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(std::io::Error::other("Compteur RAPL non entier"));
    }
    value.parse().map_err(std::io::Error::other)
}
fn children(path: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut entries = fs::read_dir(path)?
        .take(257)
        .collect::<Result<Vec<_>, _>>()?;
    if entries.len() > 256 {
        return Err(std::io::Error::other("Trop de domaines powercap"));
    }
    entries.sort_by_key(|e| e.file_name());
    Ok(entries
        .into_iter()
        .filter(|e| {
            let n = e.file_name();
            let n = n.to_string_lossy();
            n.contains(':') || n.contains("rapl")
        })
        .map(|e| e.path())
        .collect())
}
pub fn read(root: &Path) -> Snapshot {
    let mut out = Snapshot {
        clock_id: clock().0.to_owned(),
        ..Default::default()
    };
    let mut pending = match children(&root.join("sys/class/powercap")) {
        Ok(v) => v.into_iter().map(|p| (p, 0usize)).collect::<Vec<_>>(),
        Err(e) => {
            out.permission_denied = e.kind() == std::io::ErrorKind::PermissionDenied;
            out.errors.push(format!("Powercap indisponible : {e}"));
            return out;
        }
    };
    pending.reverse();
    let anchor = match fs::canonicalize(root.join("sys/devices")) {
        Ok(p) => p,
        Err(e) => {
            out.errors.push(e.to_string());
            return out;
        }
    };
    let mut visited = BTreeSet::new();
    while let Some((path, depth)) = pending.pop() {
        if visited.len() >= 256 || out.packages.len() >= 64 {
            out.errors.push("Découverte RAPL tronquée".into());
            break;
        }
        let real = match fs::canonicalize(&path) {
            Ok(p) if p.starts_with(&anchor) => p,
            Ok(_) => {
                out.errors.push("Domaine hors de sys/devices refusé".into());
                continue;
            }
            Err(e) => {
                out.permission_denied |= e.kind() == std::io::ErrorKind::PermissionDenied;
                out.errors.push(e.to_string());
                continue;
            }
        };
        if !real.is_dir() || !visited.insert(real.clone()) {
            continue;
        }
        match text(&real.join("name")) {
            Ok((name, _))
                if name.trim().strip_prefix("package-").is_some_and(|s| {
                    !s.is_empty()
                        && s.len() < 64
                        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                }) =>
            {
                let name = name.trim().to_owned();
                let counter = (|| -> std::io::Result<Counter> {
                    let range_uj = number(&text(&real.join("max_energy_range_uj"))?.0)?;
                    let (energy, inode) = text(&real.join("energy_uj"))?;
                    let at_ns = clock().1;
                    let energy_uj = number(&energy)?;
                    if range_uj == 0 || energy_uj >= range_uj {
                        return Err(std::io::Error::other("Compteur RAPL hors plage"));
                    }
                    Ok(Counter {
                        energy_uj,
                        range_uj,
                        at_ns,
                        inode,
                    })
                })();
                let (counter, error) = match counter {
                    Ok(c) => (Some(c), None),
                    Err(e) => {
                        out.permission_denied |= e.kind() == std::io::ErrorKind::PermissionDenied;
                        (None, Some(e.to_string()))
                    }
                };
                out.packages.push(Package {
                    id: real
                        .strip_prefix(root)
                        .unwrap_or(&real)
                        .display()
                        .to_string(),
                    name,
                    counter,
                    error,
                });
            }
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => {
                out.permission_denied |= e.kind() == std::io::ErrorKind::PermissionDenied;
                out.errors
                    .push(format!("Nom de domaine inaccessible : {e}"));
            }
        }
        if depth < 4 {
            match children(&real) {
                Ok(entries) => pending.extend(entries.into_iter().rev().map(|p| (p, depth + 1))),
                Err(e) => {
                    out.permission_denied |= e.kind() == std::io::ErrorKind::PermissionDenied;
                    out.errors.push(e.to_string());
                }
            }
        }
    }
    // A nested package is already covered by its parent. Aliases were removed by canonical identity.
    let ids = out
        .packages
        .iter()
        .map(|p| p.id.clone())
        .collect::<Vec<_>>();
    out.packages.retain(|p| {
        !ids.iter()
            .any(|parent| parent != &p.id && Path::new(&p.id).starts_with(parent))
    });
    let mut names = BTreeMap::new();
    for package in &out.packages {
        if names.insert(&package.name, &package.id).is_some() {
            out.errors.push(format!(
                "Sources RAPL redondantes pour {} : total ambigu",
                package.name
            ));
        }
    }
    out.packages.sort_by(|a, b| a.id.cmp(&b.id));
    if out.packages.is_empty() && out.errors.is_empty() {
        out.errors.push("Aucun domaine package RAPL".into());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "gameshift-rapl-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(root.join("sys/class/powercap")).unwrap();
            fs::create_dir_all(root.join("sys/devices/virtual/powercap")).unwrap();
            Self(root)
        }
        fn zone(&self, zone: &str, name: &str, energy: &str, range: &str) -> PathBuf {
            let p = self.0.join("sys/devices/virtual/powercap").join(zone);
            fs::create_dir_all(&p).unwrap();
            fs::write(p.join("name"), name).unwrap();
            fs::write(p.join("energy_uj"), energy).unwrap();
            fs::write(p.join("max_energy_range_uj"), range).unwrap();
            symlink(
                &p,
                self.0
                    .join("sys/class/powercap")
                    .join(zone.replace('/', "-")),
            )
            .unwrap();
            p
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn discovers_packages_dynamically_without_core_or_alias_double_counting() {
        let f = Fixture::new();
        let a = f.zone("amd-rapl:7", "package-0", "100", "2000");
        f.zone("amd-rapl:7/amd-rapl:7:9", "core", "80", "1000");
        f.zone("intel-rapl:11", "package-1", "250", "4000");
        symlink(&a, f.0.join("sys/class/powercap/intel-rapl:99")).unwrap();
        let sample = read(&f.0);
        assert!(sample.errors.is_empty(), "{:?}", sample.errors);
        assert_eq!(sample.packages.len(), 2);
        assert_eq!(sample.packages[0].counter.as_ref().unwrap().range_uj, 2000);
        assert_eq!(sample.packages[1].name, "package-1");
    }
    #[test]
    fn discovers_packages_nested_under_a_control_type() {
        let f = Fixture::new();
        let group = f.0.join("sys/devices/virtual/powercap/intel-rapl");
        fs::create_dir_all(&group).unwrap();
        symlink(&group, f.0.join("sys/class/powercap/intel-rapl")).unwrap();
        let p = group.join("intel-rapl:42");
        fs::create_dir(&p).unwrap();
        for (file, value) in [
            ("name", "package-3"),
            ("energy_uj", "0"),
            ("max_energy_range_uj", "65532610987"),
        ] {
            fs::write(p.join(file), value).unwrap();
        }
        let sample = read(&f.0);
        assert_eq!(sample.packages.len(), 1);
        assert_eq!(sample.packages[0].name, "package-3");
    }
    #[test]
    fn absent_incomplete_and_invalid_counters_never_become_zero() {
        let f = Fixture::new();
        assert!(read(&f.0).packages.is_empty());
        let p = f.zone("intel-rapl:8", "package-0", "0", "100");
        assert_eq!(
            read(&f.0).packages[0].counter.as_ref().unwrap().energy_uj,
            0
        );
        for (energy, range) in [
            ("-1", "100"),
            ("100", "100"),
            ("NaN", "100"),
            ("1", "0"),
            ("1", "18446744073709551616"),
        ] {
            fs::write(p.join("energy_uj"), energy).unwrap();
            fs::write(p.join("max_energy_range_uj"), range).unwrap();
            assert!(read(&f.0).packages[0].counter.is_none());
        }
        fs::remove_file(p.join("max_energy_range_uj")).unwrap();
        assert!(read(&f.0).packages[0].counter.is_none());
    }
    #[test]
    fn insufficient_permissions_are_explicit_and_do_not_change_file_modes() {
        let f = Fixture::new();
        let p = f
            .zone("intel-rapl:8", "package-0", "20", "100")
            .join("energy_uj");
        fs::set_permissions(&p, fs::Permissions::from_mode(0)).unwrap();
        if fs::File::open(&p).is_err() {
            let sample = read(&f.0);
            assert!(sample.permission_denied);
            assert!(sample.packages[0].counter.is_none());
            assert!(sample.packages[0].error.is_some());
        }
        assert_eq!(fs::metadata(&p).unwrap().mode() & 0o777, 0);
    }
    #[test]
    fn rejects_external_directories_and_symlink_attributes_and_redundant_providers() {
        let f = Fixture::new();
        let p = f.zone("intel-rapl:8", "package-0", "20", "100");
        f.zone("intel-rapl-mmio:2", "package-0", "30", "200");
        assert!(!read(&f.0).errors.is_empty());
        fs::remove_file(p.join("energy_uj")).unwrap();
        symlink(p.join("name"), p.join("energy_uj")).unwrap();
        assert!(read(&f.0).packages.iter().any(|p| p.counter.is_none()));
        let outside = f.0.join("outside");
        fs::create_dir(&outside).unwrap();
        symlink(outside, f.0.join("sys/class/powercap/fake-rapl")).unwrap();
        assert!(read(&f.0).errors.iter().any(|e| e.contains("refusé")));
    }
}
