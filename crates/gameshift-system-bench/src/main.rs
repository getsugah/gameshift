//! Incremental CLI: actual CPU/cache/glibc measurements; no synthetic suite scores.
use gameshift_core::duration::DurationProfile;
use gameshift_cpu::{
    calibrated::CpuDuration,
    runner::{Config, Report},
    topology::Topology,
    workload::Pattern,
};
use gameshift_system_bench::{host_command, SuitePlan};
use serde::Serialize;
use std::{sync::atomic::AtomicBool, time::Instant};

#[derive(Serialize)]
struct ComponentsReport {
    schema_version: u32,
    protocol: &'static str,
    complete_system_evaluation: bool,
    elapsed_seconds: f64,
    cpu: Vec<Report>,
    cache: Vec<Report>,
    glibc: serde_json::Value,
    unavailable_domains: Vec<&'static str>,
    warnings: Vec<&'static str>,
}
fn cpu(pattern: Pattern, workers: usize, size: u64) -> Result<Report, String> {
    let config = Config {
        workers: vec![workers],
        sizes_mib: vec![size],
        pattern,
        duration: Some(CpuDuration::preset(DurationProfile::Quick)),
        ..Config::default()
    };
    gameshift_cpu::runner::run_cancellable(config, &AtomicBool::new(false))
}
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|v| v == "--component") && args.len() == 2 {
        let category = match args[1].as_str() {
            "ram" => gameshift_system_bench::system::ram::run(),
            "cpu" => gameshift_system_bench::system::cpu::run(),
            "gpu-diagnostic" | "combined-diagnostic" => {
                let mut config = gameshift_system_bench::system::gpu::select(true)?;
                config.width = 320;
                config.height = 240;
                config.grid_side = 8;
                config.requested_msaa = 4;
                gameshift_system_bench::system::gpu::diagnostic(
                    &config,
                    args[1] == "combined-diagnostic",
                )
            }
            _ => return Err("Catégorie inconnue".into()),
        };
        println!(
            "{}",
            serde_json::to_string(&category).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    if args.first().is_some_and(|v| v == "--evaluate") {
        let kind = match args.get(1).map(String::as_str).unwrap_or("original") {
            "original" => gameshift_system_bench::session::SystemKind::Original,
            "optimized" => gameshift_system_bench::session::SystemKind::Optimized,
            _ => return Err("État attendu : original ou optimized".into()),
        };
        let report = gameshift_system_bench::system::evaluation::run(kind, |phase, fraction| {
            eprintln!(
                "GS_SYSTEM_PROGRESS {}",
                serde_json::json!({"phase":phase,"fraction":fraction})
            )
        })?;
        println!(
            "{}",
            serde_json::to_string(&report).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    if args.first().is_some_and(|v| v == "--report") && args.len() == 2 {
        let session = gameshift_system_bench::store::read(std::path::Path::new(&args[1]))?;
        println!(
            "{}",
            gameshift_system_bench::system::reporting::individual(&session)
        );
        return Ok(());
    }
    if args.first().is_some_and(|v| v == "--compare") && args.len() == 3 {
        let a = gameshift_system_bench::store::read(std::path::Path::new(&args[1]))?;
        let b = gameshift_system_bench::store::read(std::path::Path::new(&args[2]))?;
        println!(
            "{}",
            gameshift_system_bench::system::reporting::compare_text(&a, &b)?
        );
        return Ok(());
    }
    if args.first().is_some_and(|v| v == "--import") && args.len() == 2 {
        let entry = gameshift_system_bench::store::import(
            &gameshift_system_bench::store::root(),
            std::path::Path::new(&args[1]),
        )?;
        println!(
            "{}",
            serde_json::to_string(&entry).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    if args == ["--history"] {
        println!(
            "{}",
            serde_json::to_string(&gameshift_system_bench::store::list(
                &gameshift_system_bench::store::root()
            )?)
            .map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    if args.first().is_some_and(|v| v == "--reference") && args.len() == 2 {
        gameshift_system_bench::store::reference(&gameshift_system_bench::store::root(), &args[1])?;
        return Ok(());
    }
    if args.first().is_some_and(|v| v == "--export") && args.len() == 3 {
        gameshift_system_bench::store::export(
            &gameshift_system_bench::store::root(),
            &args[1],
            std::path::Path::new(&args[2]),
        )?;
        return Ok(());
    }
    if args == ["--plan"] {
        println!(
            "{}",
            serde_json::to_string_pretty(&SuitePlan::default()).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    if args != ["--cpu-host"] {
        return Err("Usage : gameshift-system-bench --evaluate [original|optimized] | --compare A.json B.json | --import rapport.json | --history | --reference ID | --export ID fichier.json | --plan | --cpu-host".into());
    }
    let started = Instant::now();
    let mut helper = std::env::current_exe().map_err(|e| e.to_string())?;
    helper.set_file_name("aurelia-host-bench");
    if !helper.is_file() {
        return Err("aurelia-host-bench doit être compilé dans le même dossier".into());
    }
    let topology = Topology::detect();
    let mut counts = vec![1, topology.allowed_logical_cpus.len().max(1)];
    counts.dedup();
    let mut cpu_reports = Vec::new();
    for workers in counts {
        eprintln!("CPU : {workers} threads");
        cpu_reports.push(cpu(Pattern::Entities, workers, 4)?);
    }
    let mut cache_reports = Vec::new();
    for pattern in [Pattern::Sequential, Pattern::Dependent] {
        for size in [4, 32, 128] {
            eprintln!("Influence du cache : {pattern:?}, {size} Mio");
            cache_reports.push(cpu(pattern, 1, size)?);
        }
    }
    eprintln!("Bibliothèque glibc de l’hôte");
    let output = host_command(&helper).output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let glibc = serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
    let report = ComponentsReport {schema_version:1,protocol:"aurelia-system-components-v1-experimental",complete_system_evaluation:false,
        elapsed_seconds:started.elapsed().as_secs_f64(),cpu:cpu_reports,cache:cache_reports,glibc,
        unavailable_domains:vec!["RAM : moteur distinct à développer","GPU : orchestration à intégrer","Charge combinée : à développer","Autres bibliothèques applicatives : à développer"],
        warnings:vec!["Parcours partiel : aucun score système, aucune garantie de durée de 120 secondes.","Les groupes CPU sont calibrés séparément : ne pas déduire un speedup à travail total identique.","Les tailles mémoire ne permettent pas d’attribuer directement un effet à un niveau de cache."]};
    println!(
        "{}",
        serde_json::to_string(&report).map_err(|e| e.to_string())?
    );
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
