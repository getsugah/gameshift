//! Explicit indirect calls to host glibc. No GTK, Vulkan or bundled libraries.
mod applications;
use serde::Serialize;
use std::{
    ffi::{CStr, CString},
    hint::black_box,
    time::Instant,
};

type CopyFn =
    unsafe extern "C" fn(*mut libc::c_void, *const libc::c_void, usize) -> *mut libc::c_void;
type SetFn = unsafe extern "C" fn(*mut libc::c_void, libc::c_int, usize) -> *mut libc::c_void;
type CmpFn = unsafe extern "C" fn(*const libc::c_void, *const libc::c_void, usize) -> libc::c_int;
type AllocFn = unsafe extern "C" fn(usize) -> *mut libc::c_void;
type FreeFn = unsafe extern "C" fn(*mut libc::c_void);
type ReallocFn = unsafe extern "C" fn(*mut libc::c_void, usize) -> *mut libc::c_void;

#[derive(Serialize)]
struct Provider {
    symbol: String,
    path: String,
    sha256: Option<String>,
}
#[derive(Serialize)]
struct Sample {
    operation: String,
    block_bytes: usize,
    iterations: u64,
    elapsed_ns: u128,
    calls_per_second: f64,
    verified: bool,
    repetition: usize,
    calibration_ns: u128,
    calibration_calls: u64,
}
#[derive(Serialize)]
struct Report {
    schema_version: u32,
    protocol: &'static str,
    engine_version: &'static str,
    executable_sha256: Option<String>,
    glibc_version: String,
    providers: Vec<Provider>,
    target_ms: u64,
    repetitions: usize,
    samples: Vec<Sample>,
    summaries: Vec<Summary>,
    environment_overrides: std::collections::BTreeMap<String, String>,
    notes: Vec<&'static str>,
}
fn sha256(path: &std::path::Path) -> Option<String> {
    let output = std::process::Command::new("sha256sum")
        .arg("--")
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let digest = std::str::from_utf8(&output.stdout)
        .ok()?
        .split_whitespace()
        .next()?;
    (digest.len() == 64 && digest.bytes().all(|v| v.is_ascii_hexdigit())).then(|| digest.to_owned())
}
#[derive(Serialize)]
struct Summary {
    operation: String,
    block_bytes: usize,
    mean_calls_per_second: f64,
    coefficient_of_variation: f64,
    confirmation_reasons: Vec<&'static str>,
}
fn summarize(samples: &[Sample]) -> Vec<Summary> {
    let mut summaries = Vec::new();
    for first in samples.iter().filter(|s| s.repetition == 1) {
        let group: Vec<_> = samples
            .iter()
            .filter(|s| s.operation == first.operation && s.block_bytes == first.block_bytes)
            .collect();
        let mean = group.iter().map(|s| s.calls_per_second).sum::<f64>() / group.len() as f64;
        let cv = if group.len() > 1 {
            (group
                .iter()
                .map(|s| (s.calls_per_second - mean).powi(2))
                .sum::<f64>()
                / (group.len() - 1) as f64)
                .sqrt()
                / mean
        } else {
            0.
        };
        let mut reasons = Vec::new();
        if group.len() < 3 {
            reasons.push("Moins de trois répétitions : résultat à confirmer.");
        }
        if group.iter().any(|s| s.elapsed_ns < 100_000_000) {
            reasons.push("Mesure inférieure à 100 ms : résultat à confirmer.");
        }
        if cv > 0.05 {
            reasons
                .push("Variation supérieure à 5 % entre les répétitions : résultat à confirmer.");
        }
        summaries.push(Summary {
            operation: first.operation.clone(),
            block_bytes: first.block_bytes,
            mean_calls_per_second: mean,
            coefficient_of_variation: cv,
            confirmation_reasons: reasons,
        });
    }
    summaries
}

// Resolution is restricted to the libc handle, and every resolved provider is checked.
unsafe fn resolve(
    handle: *mut libc::c_void,
    name: &str,
    providers: &mut Vec<Provider>,
) -> Result<*mut libc::c_void, String> {
    let name_c = CString::new(name).map_err(|e| e.to_string())?;
    let ptr = libc::dlsym(handle, name_c.as_ptr());
    if ptr.is_null() {
        return Err(format!("Symbole absent : {name}"));
    }
    let mut info: libc::Dl_info = std::mem::zeroed();
    if libc::dladdr(ptr, &mut info) == 0 || info.dli_fname.is_null() {
        return Err(format!("Provenance inconnue : {name}"));
    }
    let path = std::fs::canonicalize(CStr::from_ptr(info.dli_fname).to_string_lossy().as_ref())
        .map_err(|e| e.to_string())?;
    if let Some(appdir) = std::env::var_os("APPDIR") {
        if path.starts_with(std::fs::canonicalize(appdir).map_err(|e| e.to_string())?) {
            return Err("Bibliothèque embarquée refusée".into());
        }
    }
    providers.push(Provider {
        symbol: name.into(),
        sha256: sha256(&path),
        path: path.to_string_lossy().into_owned(),
    });
    Ok(ptr)
}
struct Api {
    copy: CopyFn,
    mov: CopyFn,
    set: SetFn,
    cmp: CmpFn,
    alloc: AllocFn,
    free: FreeFn,
    realloc: ReallocFn,
}
impl Api {
    // Non-uniform fixtures catch wrong offsets, ineffective stores and broken overlap.
    unsafe fn verify(&self) -> Result<(), String> {
        let source: Vec<u8> = (0..257).map(|i| (i * 37) as u8).collect();
        let mut destination = vec![0; source.len()];
        (self.copy)(
            destination.as_mut_ptr().cast(),
            source.as_ptr().cast(),
            source.len(),
        );
        if destination != source {
            return Err("memcpy : contenu incorrect".into());
        }
        if (self.cmp)(
            source.as_ptr().cast(),
            destination.as_ptr().cast(),
            source.len(),
        ) != 0
        {
            return Err("memcmp : égalité incorrecte".into());
        }
        destination[256] ^= 1;
        let expected = source.cmp(&destination);
        let actual = (self.cmp)(
            source.as_ptr().cast(),
            destination.as_ptr().cast(),
            source.len(),
        )
        .cmp(&0);
        if expected != actual {
            return Err("memcmp : ordre incorrect".into());
        }
        destination.clone_from(&source);
        (self.mov)(
            destination.as_mut_ptr().add(1).cast(),
            destination.as_ptr().cast(),
            256,
        );
        if destination[1..] != source[..256] {
            return Err("memmove : chevauchement incorrect".into());
        }
        (self.set)(destination.as_mut_ptr().cast(), 0xa7, destination.len());
        if destination.iter().any(|v| *v != 0xa7) {
            return Err("memset : contenu incorrect".into());
        }
        Ok(())
    }
    unsafe fn batch(
        &self,
        op: &str,
        n: u64,
        a: &mut [u8],
        b: &mut [u8],
        size: usize,
    ) -> Result<(), String> {
        for _ in 0..n {
            let src = black_box(a.as_ptr()).cast();
            let dst = black_box(b.as_mut_ptr()).cast();
            match op {
                "memcpy" => {
                    black_box((self.copy)(dst, src, black_box(size)));
                }
                "memmove" => {
                    black_box((self.mov)(
                        a.as_mut_ptr().add(1).cast(),
                        src,
                        black_box(size),
                    ));
                }
                "memset" => {
                    black_box((self.set)(dst, 0x5a, black_box(size)));
                }
                "memcmp" => {
                    if black_box((self.cmp)(src, b.as_ptr().cast(), black_box(size))) != 0 {
                        return Err("memcmp incorrect".into());
                    }
                }
                "malloc_free" | "realloc" => {
                    let mut p = (self.alloc)(black_box(size));
                    if p.is_null() {
                        return Err("Allocation impossible".into());
                    }
                    // Touch one byte: this is an allocator test, not a whole-buffer page-fault test.
                    p.cast::<u8>().write(0x5a);
                    if op == "realloc" {
                        let q = (self.realloc)(p, black_box(size * 2));
                        if q.is_null() {
                            (self.free)(p);
                            return Err("Réallocation impossible".into());
                        }
                        p = q;
                    }
                    let valid = black_box(p.cast::<u8>().read()) == 0x5a;
                    (self.free)(p);
                    if !valid {
                        return Err("Contenu alloué incorrect".into());
                    }
                }
                _ => return Err("Opération inconnue".into()),
            }
        }
        Ok(())
    }
}
fn run(target_ms: u64, repetitions: usize) -> Result<Report, String> {
    for key in ["LD_LIBRARY_PATH", "LD_PRELOAD", "LD_AUDIT"] {
        if std::env::var_os(key).is_some_and(|v| !v.is_empty()) {
            return Err(format!(
                "Environnement non isolé : {key}. Utilisez le lanceur hôte."
            ));
        }
    }
    unsafe {
        let handle = libc::dlopen(c"libc.so.6".as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
        if handle.is_null() {
            return Err("glibc indisponible".into());
        }
        // Keep the handle open until process exit: all function pointers depend on it.
        let mut providers = Vec::new();
        macro_rules! load {
            ($n:literal,$t:ty) => {
                std::mem::transmute::<*mut libc::c_void, $t>(resolve(handle, $n, &mut providers)?)
            };
        }
        let version = load!(
            "gnu_get_libc_version",
            unsafe extern "C" fn() -> *const libc::c_char
        );
        let glibc_version = CStr::from_ptr(version()).to_string_lossy().into_owned();
        let api = Api {
            copy: load!("memcpy", CopyFn),
            mov: load!("memmove", CopyFn),
            set: load!("memset", SetFn),
            cmp: load!("memcmp", CmpFn),
            alloc: load!("malloc", AllocFn),
            free: load!("free", FreeFn),
            realloc: load!("realloc", ReallocFn),
        };
        if providers.iter().any(|p| p.path != providers[0].path) {
            return Err("Les symboles ne proviennent pas tous de la même glibc".into());
        }
        api.verify()?;
        let mut samples = Vec::new();
        for size in [64, 4096, 128 * 1024 * 1024] {
            let mut a = vec![0x5a; size + 1];
            let mut b = vec![0x5a; size + 1];
            for op in [
                "memcpy",
                "memmove",
                "memset",
                "memcmp",
                "malloc_free",
                "realloc",
            ] {
                // Preparation and calibration are excluded from samples.
                let start = Instant::now();
                let mut probes = 0u64;
                let batch = if size < 1024 * 1024 { 256 } else { 1 };
                while start.elapsed().as_millis() < 50 {
                    api.batch(op, batch, &mut a, &mut b, size)?;
                    probes += batch;
                }
                let calibration_ns = start.elapsed().as_nanos();
                let iterations = ((target_ms as f64 * 1e6 * 1.1
                    / (calibration_ns as f64 / probes as f64))
                    .ceil() as u64)
                    .max(1);
                for repetition in 1..=repetitions {
                    a.fill(0x5a);
                    b.fill(0x5a);
                    let start = Instant::now();
                    api.batch(op, iterations, &mut a, &mut b, size)?;
                    let elapsed_ns = start.elapsed().as_nanos();
                    let verified = a.iter().all(|v| *v == 0x5a) && b.iter().all(|v| *v == 0x5a);
                    if !verified {
                        return Err(format!("Validation échouée : {op}"));
                    }
                    samples.push(Sample {
                        operation: op.into(),
                        block_bytes: size,
                        iterations,
                        elapsed_ns,
                        calls_per_second: iterations as f64 * 1e9 / elapsed_ns as f64,
                        verified,
                        repetition,
                        calibration_ns,
                        calibration_calls: probes,
                    });
                }
            }
        }
        let summaries = summarize(&samples);
        let environment_overrides = [
            "GLIBC_TUNABLES",
            "MALLOC_ARENA_MAX",
            "MALLOC_PERTURB_",
            "MALLOC_MMAP_THRESHOLD_",
            "MALLOC_TRIM_THRESHOLD_",
        ]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| (key.into(), value)))
        .collect();
        let executable_sha256 = std::env::current_exe().ok().and_then(|p| sha256(&p));
        Ok(Report {schema_version:1,protocol:"aurelia-host-glibc-v1-experimental",engine_version:env!("CARGO_PKG_VERSION"),executable_sha256,glibc_version,providers,target_ms,repetitions,samples,summaries,environment_overrides,notes:vec!["Mesures mono-thread ; jeu de blocs réutilisé, aucune attribution exclusive à glibc.","128 Mio ne garantit pas un dépassement des caches de toutes les machines.","malloc/free et realloc : un octet touché, appels système éventuels non isolés.","memmove : chevauchement d’un octet ; memcmp : blocs égaux, parcours complet.","Calibration par cas ; nombre d’appels fixé pour toutes ses répétitions."]})
    }
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--applications"] {
        match applications::run() {
            Ok(report) => println!("{}", report),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return;
    }
    let result = if args.is_empty() {
        run(250, 3)
    } else if args == ["--smoke"] {
        run(5, 2)
    } else {
        Err("Usage : aurelia-host-bench [--smoke]".into())
    };
    match result {
        Ok(report) => println!(
            "{}",
            serde_json::to_string(&report).expect("serializable report")
        ),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
