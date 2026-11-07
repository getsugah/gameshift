//! Rasterized Aurelia offscreen plus explicitly synchronized mixed load.
use super::{cpu, metric, ram, Category, State};
use gameshift_bench::{system_raster::SystemRaster, FrameSample, RunConfig};
use serde::Serialize;
use std::{
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Barrier,
    },
    time::{Duration, Instant},
};
#[derive(Debug, Serialize)]
pub struct Pass {
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub elapsed_seconds: f64,
    pub frames: Vec<FrameSample>,
    pub statistics: gameshift_bench::Statistics,
}
fn sample(
    renderer: &mut SystemRaster,
    origin: Instant,
    done: Option<&Barrier>,
) -> Result<Pass, String> {
    let mut frames = Vec::with_capacity(10000);
    let start = Instant::now();
    let rendered = (|| {
        while start.elapsed() < Duration::from_secs(4) {
            let elapsed = start.elapsed().as_secs_f64();
            frames.push(renderer.frame(3. + elapsed as f32)?);
        }
        Ok::<_, String>(Instant::now())
    })();
    if let Some(done) = done {
        done.wait();
    }
    let end = rendered?;
    let statistics = gameshift_bench::summarize(&frames).map_err(|e| e.to_string())?;
    Ok(Pass {
        start_seconds: start.duration_since(origin).as_secs_f64(),
        end_seconds: end.duration_since(origin).as_secs_f64(),
        elapsed_seconds: end.duration_since(start).as_secs_f64(),
        frames,
        statistics,
    })
}
fn warm(renderer: &mut SystemRaster) -> Result<(), String> {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(1) {
        renderer.frame(2. + start.elapsed().as_secs_f32())?;
    }
    Ok(())
}
pub fn select(allow_software: bool) -> Result<RunConfig, String> {
    let report = gameshift_bench::probe(gameshift_bench::ValidationMode::Disabled)
        .map_err(|e| e.to_string())?;
    let gpu = report
        .gpus
        .iter()
        .find(|g| g.kind == "GPU dédié" && g.queues.iter().any(|q| q.graphics && q.queue_count > 0))
        .or_else(|| {
            report.gpus.iter().find(|g| {
                g.kind != "Rendu logiciel (CPU)"
                    && g.queues.iter().any(|q| q.graphics && q.queue_count > 0)
            })
        })
        .or_else(|| {
            if allow_software {
                report.gpus.first()
            } else {
                None
            }
        })
        .ok_or("Aucun GPU Vulkan matériel accessible ; le rendu logiciel ne remplace pas un GPU")?;
    Ok(RunConfig {
        gpu_uuid: gpu.device_uuid.clone().ok_or("Identifiant GPU absent")?,
        width: 1920,
        height: 1080,
        grid_side: 24,
        requested_msaa: 8,
        warmup_seconds: 1,
        measure_seconds: 4,
        repetitions: 3,
        require_validation: false,
        duration_profile: Some(gameshift_core::duration::DurationProfile::Custom),
    })
}
fn gpu_metric(passes: &[Pass], id: &str, description: &str) -> crate::session::Metric {
    let mut result = metric(
        id,
        if id.starts_with("combined/") {
            "combined"
        } else {
            "gpu"
        },
        "Aurelia raster hors écran",
        "images/s",
        passes
            .iter()
            .map(|p| p.frames.len() as f64 / p.elapsed_seconds)
            .collect(),
        description,
    );
    if passes.iter().any(|p| p.frames.len() < 100) {
        result.confirmation_reasons.push("Moins de 100 images dans un passage : la résolution du compte dépasse 1 %, confirmer par un test plus long.".into());
    }
    result
}
pub fn run(config: &RunConfig, combined: bool) -> Category {
    execute(config, combined, false)
}
pub fn diagnostic(config: &RunConfig, combined: bool) -> Category {
    execute(config, combined, true)
}
fn execute(config: &RunConfig, combined: bool, diagnostic: bool) -> Category {
    let category = if combined { "combined" } else { "gpu" };
    let began = Instant::now();
    let result = (|| {
        let mut renderer = SystemRaster::new(config.clone(), diagnostic)?;
        let identity = renderer.identity();
        if identity.software && !diagnostic {
            return Err("GPU logiciel refusé pour le score matériel".into());
        }
        let origin = Instant::now();
        let phase = Arc::new(AtomicU32::new(0));
        let sensors = gameshift_telemetry::sensors::SensorSampler::start(
            identity.pci_address.as_deref(),
            phase.clone(),
        );
        let mut passes = Vec::new();
        let mut cpu_samples = Vec::new();
        let mut ram_samples = Vec::new();
        let mut overlap = Vec::new();
        let worker_count = cpu::workers();
        let mut load = if combined {
            Some(cpu::Load::new(worker_count)?)
        } else {
            None
        };
        let mut buffers = if combined {
            Some(ram::Buffers::new(ram::working_set()?.0)?)
        } else {
            None
        };
        for repetition in 0..3 {
            phase.store(repetition + 1, Ordering::Relaxed);
            if combined {
                let barrier = Barrier::new(3);
                let done = Barrier::new(3);
                let cpu_load = load.as_mut().unwrap();
                let ram_load = buffers.as_mut().unwrap();
                let (gpu_result, cpu_result, ram_result) = std::thread::scope(|scope| {
                    let cpu_thread = scope.spawn(|| {
                        let prep = cpu_load.sample(Duration::from_secs(1), origin);
                        barrier.wait();
                        let result =
                            prep.and_then(|_| cpu_load.sample(Duration::from_secs(4), origin));
                        done.wait();
                        result
                    });
                    let ram_thread = scope.spawn(|| {
                        let prep = ram_load.sample("copy", Duration::from_secs(1), origin);
                        barrier.wait();
                        match prep {
                            Ok(_) => ram_load.sample_synchronized(
                                "copy",
                                Duration::from_secs(4),
                                origin,
                                Some(&done),
                            ),
                            Err(e) => {
                                done.wait();
                                Err(e)
                            }
                        }
                    });
                    let preparation = warm(&mut renderer);
                    barrier.wait();
                    let gpu = match preparation {
                        Ok(()) => sample(&mut renderer, origin, Some(&done)),
                        Err(e) => {
                            done.wait();
                            Err(e)
                        }
                    };
                    (
                        gpu,
                        cpu_thread
                            .join()
                            .map_err(|_| "Worker CPU interrompu".to_string())
                            .and_then(|v| v),
                        ram_thread
                            .join()
                            .map_err(|_| "Worker RAM interrompu".to_string())
                            .and_then(|v| v),
                    )
                });
                let gpu = gpu_result?;
                let cpu = cpu_result?;
                let memory = ram_result?;
                let start = gpu
                    .start_seconds
                    .max(cpu.start_seconds)
                    .max(memory.start_seconds);
                let end = gpu.end_seconds.min(cpu.end_seconds).min(memory.end_seconds);
                let longest = gpu
                    .elapsed_seconds
                    .max(cpu.elapsed_seconds)
                    .max(memory.elapsed_seconds);
                let fraction = (end - start).max(0.) / longest;
                if fraction < 0.95 {
                    return Err(format!(
                        "Chevauchement CPU/GPU/RAM insuffisant : {:.1} %",
                        fraction * 100.
                    ));
                }
                overlap.push(fraction);
                passes.push(gpu);
                cpu_samples.push(cpu);
                ram_samples.push(memory);
            } else {
                warm(&mut renderer)?;
                passes.push(sample(&mut renderer, origin, None)?);
            }
        }
        let selection = identity
            .pci_address
            .as_deref()
            .map(|address| {
                let base = std::path::Path::new("/sys/bus/pci/devices").join(address);
                let vendor = std::fs::read_to_string(base.join("vendor")).unwrap_or_default();
                let device = std::fs::read_to_string(base.join("device")).unwrap_or_default();
                format!("{address}/{}/{}", vendor.trim(), device.trim())
            })
            .unwrap_or_else(|| config.gpu_uuid.clone());
        let description = format!(
            "{}/{}x{}/grid{}/msaa{}/warm1/measure4/reps3/validation{}/device{}",
            gameshift_bench::SCENE_VERSION,
            config.width,
            config.height,
            config.grid_side,
            identity.msaa,
            identity.validation_enabled,
            selection
        );
        let mut metrics = vec![gpu_metric(
            &passes,
            &format!("{category}/raster"),
            &description,
        )];
        if combined {
            metrics.push(metric(
                "combined/cpu",
                "combined",
                "Simulation CPU simultanée",
                "étapes/s",
                cpu_samples.iter().map(|s| s.steps_per_second).collect(),
                &cpu::profile(worker_count, 4),
            ));
            metrics.push(metric(
                "combined/ram",
                "combined",
                "Copie RAM simultanée",
                "Go/s logiques",
                ram_samples.iter().map(|s| s.gb_per_second).collect(),
                &ram::profile("copy", buffers.as_ref().unwrap().bytes(), 4),
            ));
        }
        let sensor_data = sensors.finish();
        let raw = serde_json::json!({"scene_version":gameshift_bench::SCENE_VERSION,"render_mode":"offscreen-no-swapchain-no-vsync-one-frame-in-flight","config":config,"cpu_workers":worker_count,"identity":identity,"environment":SystemRaster::environment(),"diagnostic_only":diagnostic,"selected_device_key":selection,"passes":passes,"cpu":cpu_samples,"ram":ram_samples,"overlap_fractions":overlap,"sensors":sensor_data});
        Ok::<_, String>((metrics, raw))
    })();
    match result{Ok((metrics,raw))=>Category{id:category.into(),state:State::Executed,elapsed_seconds:began.elapsed().as_secs_f64(),metrics,raw,notes:vec!["Timestamps GPU réels si pris en charge ; préparation CPU et exécution GPU distinctes. Rendu hors écran : présentation non applicable.".into()]},Err(e)=>{let mut c=Category::missing(category,State::Failed,e);c.elapsed_seconds=began.elapsed().as_secs_f64();c}}
}
