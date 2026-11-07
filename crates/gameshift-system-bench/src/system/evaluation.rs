//! Full coverage orchestration: partial failures stay in the individual report.
use super::{cpu, gpu, metric, ram, Body, Category, State, PROTOCOL};
use crate::session::{Session, SystemKind};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
fn timed(run: impl FnOnce() -> Category) -> Category {
    let start = Instant::now();
    let mut c = run();
    c.elapsed_seconds = start.elapsed().as_secs_f64();
    c
}
fn cache() -> Category {
    let started = Instant::now();
    let result = (|| {
        let mut metrics = Vec::new();
        let mut raw = Vec::new();
        for mib in [4, 32, 128] {
            let bytes = mib * 1048576;
            let mut buffers = ram::Buffers::new(bytes)?;
            buffers.sample("read", Duration::from_millis(100), started)?;
            let mut samples = Vec::new();
            for _ in 0..3 {
                samples.push(buffers.sample("read", Duration::from_millis(300), started)?);
            }
            metrics.push(metric(
                &format!("cache/sequential/{bytes}"),
                "cache",
                &format!("Parcours séquentiel · {mib} Mio"),
                "Go/s logiques",
                samples.iter().map(|s| s.gb_per_second).collect(),
                &format!("native-stream-v2/{bytes}/300ms"),
            ));
            drop(buffers);
            let latency = ram::latency(bytes, Duration::from_millis(300))?;
            metrics.push(metric(
                &format!("cache/dependent/{bytes}"),
                "cache",
                &format!("Accès chaînés · {mib} Mio"),
                "accès/s",
                latency.iter().map(|s| 1e9 / s.ns_per_access).collect(),
                &format!("dependent-permutation-v2/{bytes}/300ms"),
            ));
            raw.push(
                serde_json::json!({"requested_bytes":bytes,"stream":samples,"latency":latency}),
            );
        }
        Ok::<_, String>((metrics, serde_json::json!(raw)))
    })();
    match result{Ok((metrics,raw))=>Category{id:"cache".into(),state:State::Executed,elapsed_seconds:started.elapsed().as_secs_f64(),metrics,raw,notes:vec!["Sensibilité à la taille des données et aux accès. Les ns/accès incluent TLB, instructions et caches résiduels ; ils ne mesurent pas directement un niveau L1/L2/L3.".into()]},Err(e)=>Category::missing("cache",State::Failed,e)}
}
fn helper(path: &std::path::Path, args: &[&str]) -> Result<serde_json::Value, String> {
    let result = crate::host_command(path)
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if !result.status.success() {
        return Err(format!(
            "Auxiliaire terminé avec {} : {}",
            result.status,
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    serde_json::from_slice(&result.stdout).map_err(|e| e.to_string())
}
fn glibc(value: &serde_json::Value) -> Result<Category, String> {
    let mut metrics = Vec::new();
    for summary in value["summaries"]
        .as_array()
        .ok_or("Résumés glibc absents")?
    {
        let op = summary["operation"]
            .as_str()
            .ok_or("Opération glibc absente")?;
        let size = summary["block_bytes"]
            .as_u64()
            .ok_or("Taille glibc absente")?;
        let samples: Vec<_> = value["samples"]
            .as_array()
            .ok_or("Échantillons glibc absents")?
            .iter()
            .filter(|s| {
                s["operation"].as_str() == Some(op) && s["block_bytes"].as_u64() == Some(size)
            })
            .collect();
        if samples.iter().any(|s| s["verified"] != true) {
            return Err("Résultat glibc invalide".into());
        }
        let mut item = metric(
            &format!("glibc/{op}/{size}"),
            "glibc",
            &format!("{op} · {size} octets"),
            "appels/s",
            samples
                .iter()
                .filter_map(|s| s["calls_per_second"].as_f64())
                .collect(),
            &format!("host-glibc-v1/{op}/{size}/250ms"),
        );
        item.confirmation_reasons = summary["confirmation_reasons"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        metrics.push(item);
    }
    Ok(Category {
        id: "glibc".into(),
        state: State::Executed,
        elapsed_seconds: 0.,
        metrics,
        raw: value.clone(),
        notes: vec![
            "Appels FFI glibc hôte réels ; allocations touchant un octet, coût noyau non isolé."
                .into(),
        ],
    })
}
fn applications(value: serde_json::Value) -> Result<Category, String> {
    let mut state = State::Executed;
    let mut metrics = Vec::new();
    for lib in value["libraries"]
        .as_array()
        .ok_or("Bibliothèques applicatives absentes")?
    {
        match lib["state"].as_str() {
            Some("failed") => state = State::Failed,
            Some("unavailable") if state != State::Failed => state = State::Unavailable,
            Some("executed") => (),
            Some("unavailable") => (),
            _ => state = State::Failed,
        }
        let name = lib["library"]
            .as_str()
            .ok_or("Nom de bibliothèque absent")?;
        let samples = lib["samples"]
            .as_array()
            .ok_or("Échantillons applicatifs absents")?;
        let mut operations = std::collections::BTreeSet::new();
        for sample in samples {
            let op = sample["operation"].as_str().ok_or("Opération absente")?;
            if sample["verified"] != true {
                return Err("Validation applicative échouée".into());
            }
            operations.insert(op);
        }
        for op in operations {
            let values = samples
                .iter()
                .filter(|s| s["operation"].as_str() == Some(op))
                .filter_map(|s| s["rate"].as_f64())
                .collect();
            metrics.push(metric(
                &format!("applications/{name}/{op}"),
                "applications",
                op,
                if op.ends_with("pixels") {
                    "pixels/s"
                } else {
                    "octets/s"
                },
                values,
                &format!("host-app-v1/{name}/{op}/550ms"),
            ));
        }
    }
    Ok(Category{id:"applications".into(),state,elapsed_seconds:0.,metrics,raw:value,notes:vec!["Bibliothèques hôte : compression/décompression zstd, image PNG, SHA-256 OpenSSL. Une absence ne devient pas un zéro.".into()]})
}
pub fn run(kind: SystemKind, mut progress: impl FnMut(&str, f64)) -> Result<Session, String> {
    let _profile_guard = gameshift_control::benchmark::BenchmarkGuard::begin()?;
    let profile_before = gameshift_control::benchmark::profile_snapshot();
    let started = Instant::now();
    let mut executable = std::env::current_exe().map_err(|e| e.to_string())?;
    executable.set_file_name("aurelia-host-bench");
    let fingerprint = crate::session::fingerprint(&executable)?;
    let cpus = fingerprint.logical_cpus.clone();
    let controls_before = gameshift_control::snapshot_all();
    let before = gameshift_cpu::environment::Snapshot::read(&cpus);
    let mut categories = Vec::new();
    progress("Processeur : monothread et multithread soutenu", 0.);
    categories.push(timed(cpu::run));
    progress("Cache CPU : parcours séquentiels et chaînés", 1. / 7.);
    categories.push(timed(cache));
    progress("RAM native : lecture, écriture, copie et latence", 2. / 7.);
    categories.push(timed(ram::run));
    progress("Aurelia Vulkan : rendu GPU réel hors écran", 3. / 7.);
    let config = gpu::select(false);
    categories.push(match &config {
        Ok(config) => gpu::run(config, false),
        Err(e) => Category::missing("gpu", State::Unavailable, e),
    });
    progress("Charge simultanée CPU / GPU / RAM", 4. / 7.);
    let mut combined = if categories
        .iter()
        .all(|c| !["cpu", "ram", "gpu"].contains(&c.id.as_str()) || c.state == State::Executed)
    {
        gpu::run(config.as_ref().map_err(|e| e.to_string())?, true)
    } else {
        Category::missing(
            "combined",
            State::Unavailable,
            "Pré-requis isolés CPU/RAM/GPU non exécutés ; aucune charge de remplacement",
        )
    };
    if combined.state == State::Executed {
        let mut degradation = Vec::new();
        for (id, base_category, prefix) in [
            ("combined/cpu", "cpu", "cpu/multi_sustained"),
            ("combined/ram", "ram", "ram/copy"),
            ("combined/raster", "gpu", "gpu/raster"),
        ] {
            let baseline = categories
                .iter()
                .find(|c| c.id == base_category)
                .and_then(|c| c.metrics.iter().find(|m| m.id.starts_with(prefix)))
                .ok_or("Référence isolée absente")?;
            let simultaneous = combined
                .metrics
                .iter()
                .find(|m| m.id == id)
                .ok_or("Mesure simultanée absente")?;
            if baseline.work_description != simultaneous.work_description
                || baseline.unit != simultaneous.unit
            {
                combined.state = State::Failed;
                combined.notes.push(format!(
                    "Paramètres isolés et combinés différents pour {base_category}"
                ));
                break;
            }
            degradation.push(serde_json::json!({"component":base_category,"isolated_mean":baseline.mean(),"simultaneous_mean":simultaneous.mean(),"unit":baseline.unit,"degradation_percent":100.*(1.-simultaneous.mean()/baseline.mean()),"per_repetition_percent":baseline.values.iter().zip(&simultaneous.values).map(|(a,b)|100.*(1.-b/a)).collect::<Vec<_>>(),"isolated_parameters":baseline.work_description,"combined_parameters":simultaneous.work_description}));
        }
        combined.raw["degradation"] = serde_json::json!(degradation);
    }
    categories.push(combined);
    progress("Fonctions glibc de la distribution hôte", 5. / 7.);
    let time = Instant::now();
    let glibc_value = helper(&executable, &[]);
    let glibc_raw = glibc_value
        .as_ref()
        .ok()
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let mut glibc_category = match glibc_value {
        Ok(value) => {
            if value["executable_sha256"].as_str() != Some(fingerprint.helper_sha256.as_str()) {
                Category::missing("glibc", State::Failed, "Empreinte auxiliaire différente")
            } else {
                glibc(&value).unwrap_or_else(|e| Category::missing("glibc", State::Failed, e))
            }
        }
        Err(e) => {
            let state = if e.contains("glibc indisponible") {
                State::Unavailable
            } else {
                State::Failed
            };
            Category::missing("glibc", state, e)
        }
    };
    glibc_category.elapsed_seconds = time.elapsed().as_secs_f64();
    categories.push(glibc_category);
    progress("Bibliothèques système : zstd, PNG et SHA-256", 6. / 7.);
    let time = Instant::now();
    let mut apps = match helper(&executable, &["--applications"]) {
        Ok(value) => {
            if value["engine_sha256"].as_str() != Some(fingerprint.helper_sha256.as_str()) {
                Category::missing(
                    "applications",
                    State::Failed,
                    "Empreinte auxiliaire différente",
                )
            } else {
                applications(value)
                    .unwrap_or_else(|e| Category::missing("applications", State::Failed, e))
            }
        }
        Err(e) => Category::missing("applications", State::Failed, e),
    };
    apps.elapsed_seconds = time.elapsed().as_secs_f64();
    categories.push(apps);
    let after = gameshift_cpu::environment::Snapshot::read(&cpus);
    let controls_after = gameshift_control::snapshot_all();
    let elapsed = started.elapsed().as_secs_f64();
    let metrics = categories.iter().flat_map(|c| c.metrics.clone()).collect();
    let system = Body {
        protocol: PROTOCOL.into(),
        target_seconds: 120.,
        elapsed_seconds: elapsed,
        overrun_seconds: (elapsed - 120.).max(0.),
        categories,
        parameters: serde_json::json!({"plan":crate::SuitePlan::default(),"scene":gameshift_bench::SCENE_VERSION,"cpu_workers":cpu::workers(),"canonical_cpu_epoch_steps":64,"gpu_render":"offscreen","render_config":config.ok(),"ram_kernel":"explicit-SSE2-on-x86_64-volatile-fallback","gpu_profile_before":profile_before,"gpu_profile_after":gameshift_control::benchmark::profile_snapshot(),"driver_controls_before":controls_before,"driver_controls_after":controls_after,"driver_controls_changed":controls_before!=controls_after,"cpu_environment_before":before,"cpu_environment_after":after,"score_version":"aurelia-system-relative-v2"}),
    };
    let report=Session{schema_version:2,protocol:PROTOCOL.into(),application_version:env!("CARGO_PKG_VERSION").into(),completed_unix_seconds:SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e|e.to_string())?.as_secs(),kind,fingerprint,elapsed_seconds:elapsed,metrics,cpu_raw:vec![],glibc_raw,limitations:vec!["Durée cible indicative ; résultats manquants explicites, aucun remplacement ni score zéro.".into(),"Le rendu GPU système est hors écran ; les profils historiques fenêtrés restent indépendants.".into()],system:Some(system)};
    report.validate()?;
    progress("Rapport individuel disponible", 1.);
    Ok(report)
}
