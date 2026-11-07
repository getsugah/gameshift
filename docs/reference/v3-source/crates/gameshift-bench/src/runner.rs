//! Window lifecycle, repeatable camera timeline and wall-clock frame collection.
use crate::{
    protocol::{variability, Repetition, RunConfig, RunResult, SCENE_VERSION},
    renderer::Renderer,
    FrameSample,
};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
    window::{Window, WindowId},
};

type Result<T> = std::result::Result<T, String>;

struct Recording {
    started: Instant,
    last_complete: Option<Instant>,
    measure_started: Option<Instant>,
    samples: Vec<FrameSample>,
    runs: Vec<Repetition>,
    telemetry_phase: Arc<AtomicU32>,
}
impl Recording {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            last_complete: None,
            measure_started: None,
            samples: vec![],
            runs: vec![],
            telemetry_phase: Arc::new(AtomicU32::new(0)),
        }
    }
    fn step(&mut self, r: &mut Renderer, config: &RunConfig) -> Result<bool> {
        if self.measure_started.is_none()
            && self.started.elapsed().as_secs_f64() >= f64::from(config.warmup_seconds)
        {
            let now = Instant::now();
            self.measure_started = Some(now);
            self.telemetry_phase
                .store(self.runs.len() as u32 + 1, Ordering::Relaxed);
            self.last_complete = Some(now);
        }
        let measured_elapsed = self
            .measure_started
            .map(|start| start.elapsed().as_secs_f64());
        if measured_elapsed.is_some_and(|elapsed| elapsed >= f64::from(config.measure_seconds)) {
            self.telemetry_phase.store(0, Ordering::Relaxed);
            self.runs
                .push(Repetition::from_samples(std::mem::take(&mut self.samples))?);
            if self.runs.len() == config.repetitions as usize {
                return Ok(true);
            }
            self.started = Instant::now();
            self.last_complete = None;
            self.measure_started = None;
            return Ok(false);
        }
        let measuring = self.measure_started.is_some();
        // Every repetition starts the measured camera path at exactly the same phase.
        let camera_seconds = measured_elapsed
            .map(|elapsed| f64::from(config.warmup_seconds) + elapsed)
            .unwrap_or_else(|| self.started.elapsed().as_secs_f64());
        let mut sample = r.draw(config, camera_seconds as f32)?;
        let completed = Instant::now();
        if measuring {
            if self.samples.len() >= 200_000 {
                return Err("Trop de frames pour ce protocole ; réduire sa durée".into());
            }
            sample.frame_interval_ms = completed
                .duration_since(self.last_complete.unwrap())
                .as_secs_f64()
                * 1000.0;
            self.samples.push(sample);
            self.last_complete = Some(completed);
        }
        Ok(false)
    }
}

fn result(
    config: RunConfig,
    r: &Renderer,
    runs: Vec<Repetition>,
    headless: bool,
    focus_lost: bool,
    gpu_utilization: gameshift_telemetry::GpuUtilization,
) -> RunResult {
    let mut warnings=vec!["Renderer initial avec une frame en vol : débit sérialisé CPU/GPU, pas une estimation des performances de tous les jeux.".into(),"Températures, fréquences et limiteurs FPS externes non surveillés ; aucune conclusion d’optimisation automatique.".into()];
    if r.software {
        warnings.push("Rendu logiciel CPU : ces chiffres ne mesurent pas les performances d’une carte graphique.".into());
    }
    if !r.validation_enabled {
        warnings.push("Couches de validation indisponibles ou désactivées.".into());
    } else {
        warnings.push("Validation de développement activée : son coût CPU impose de comparer des tests avec le même état de validation.".into());
    }
    if !headless {
        warnings.push("Le compositeur peut limiter le débit ; la date réelle d’affichage des images n’est pas mesurée.".into());
    }
    if r.present_mode == "FIFO" {
        warnings.push("Présentation FIFO synchronisée à l’écran : le FPS peut être plafonné par le rafraîchissement.".into());
    }
    if r.present_mode == "MAILBOX" {
        warnings.push("Présentation MAILBOX : certaines images rendues peuvent être remplacées avant affichage.".into());
    }
    if focus_lost {
        warnings.push("La fenêtre a perdu le focus pendant le test.".into());
    }
    let (width, height) = r.dimensions();
    if (width, height) != (config.width, config.height) {
        warnings.push("Le compositeur a imposé une résolution différente de celle demandée ; utiliser la résolution réelle pour comparer.".into());
    }
    if runs
        .iter()
        .any(|r| r.statistics.gpu_samples < r.statistics.frames)
    {
        warnings.push(
            "Timestamps GPU absents ou incomplets ; les durées manquantes ne sont pas estimées."
                .into(),
        );
    }
    if runs.iter().any(|run| run.samples.len() < 1000) {
        warnings.push("Moins de 1 000 frames dans une répétition : percentiles extrêmes et 0,1 % low peu représentatifs.".into());
    }
    if r.sample_count.as_raw() != config.requested_msaa {
        warnings.push(format!("MSAA {}× demandé, {}× pris en charge pour ces formats ; niveau réellement utilisé enregistré.",config.requested_msaa,r.sample_count.as_raw()));
    }
    match gpu_utilization.average_percent {
        Some(percent)if percent<90.0=>warnings.push(format!("Occupation globale moyenne {percent:.1} % : saturation du GPU non démontrée pour ce protocole.")),
        Some(_)=>warnings.push("Occupation globale élevée : ce capteur inclut les autres applications et ne prouve pas la charge du seul benchmark.".into()),
        None=>warnings.push("Occupation GPU indisponible ; aucune saturation ne peut être confirmée par un capteur.".into()),
    }
    let repetition_cv_percent = variability(&runs);
    if repetition_cv_percent.is_some_and(|cv| cv > 5.0) {
        warnings.push("Variabilité supérieure à 5 % entre répétitions ; résultats à reproduire avant toute conclusion.".into());
    }
    RunResult {
        schema_version: crate::protocol::RESULT_SCHEMA_VERSION,
        scene_version: SCENE_VERSION.into(),
        application_version: env!("CARGO_PKG_VERSION").into(),
        frames_in_flight: 1,
        msaa_samples: r.sample_count.as_raw(),
        gpu_utilization,
        gpu_timestamp_period_ns: r.timestamp_metadata().0,
        gpu_timestamp_valid_bits: r.timestamp_metadata().1,
        config,
        gpu_name: r.gpu_name.clone(),
        driver_name: r.driver_name.clone(),
        driver_info: r.driver_info.clone(),
        software_device: r.software,
        headless,
        present_mode: r.present_mode.clone(),
        actual_width: width,
        actual_height: height,
        presentation_width: r.presentation_dimensions().0,
        presentation_height: r.presentation_dimensions().1,
        validation_enabled: r.validation_enabled,
        validation_errors: 0,
        environment: crate::protocol::RunEnvironment::capture(),
        completed_unix_seconds: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        repetitions: runs,
        repetition_cv_percent,
        warnings,
    }
}

struct App {
    renderer: Option<Renderer>, // Drop the Vulkan surface before dropping its window.
    window: Option<Window>,
    config: RunConfig,
    cancel: Arc<AtomicBool>,
    recording: Option<Recording>,
    result: Option<RunResult>,
    failure: Option<String>,
    focus_lost: bool,
    title_updated: Instant,
    sampler: Option<gameshift_telemetry::GpuSampler>,
}
impl App {
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: String) {
        self.result = None;
        self.failure = Some(error);
        event_loop.exit();
    }
}
impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let scale = (1280.0 / self.config.width as f64)
            .min(720.0 / self.config.height as f64)
            .min(1.0);
        let window_size = PhysicalSize::new(
            (self.config.width as f64 * scale).round() as u32,
            (self.config.height as f64 * scale).round() as u32,
        );
        let attributes = Window::default_attributes()
            .with_title("GameShift · Initialisation Vulkan")
            .with_resizable(false)
            .with_inner_size(window_size);
        match event_loop.create_window(attributes) {
            Ok(window) => match Renderer::new(&self.config, Some(&window)) {
                Ok(renderer) => {
                    let recording = Recording::new();
                    self.sampler = Some(gameshift_telemetry::GpuSampler::start(
                        renderer.pci_address.as_deref(),
                        recording.telemetry_phase.clone(),
                    ));
                    self.renderer = Some(renderer);
                    self.window = Some(window);
                    self.recording = Some(recording);
                }
                Err(e) => self.fail(event_loop, e),
            },
            Err(e) => self.fail(event_loop, format!("Fenêtre de benchmark : {e}")),
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && event.logical_key == Key::Named(NamedKey::Escape) =>
            {
                self.fail(event_loop, "Test annulé avec Échap".into());
            }
            WindowEvent::CloseRequested => {
                self.fail(event_loop, "Test annulé par fermeture de la fenêtre".into())
            }
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &self.renderer {
                    if (size.width, size.height) != renderer.presentation_dimensions() {
                        self.fail(event_loop,"La taille de la fenêtre a changé ; test interrompu pour conserver une résolution comparable".into());
                    }
                }
            }
            WindowEvent::Occluded(true) => self.fail(
                event_loop,
                "Fenêtre masquée ou minimisée : test interrompu".into(),
            ),
            WindowEvent::Focused(false) => self.focus_lost = true,
            _ => {}
        }
    }
    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        self.fail(
            event_loop,
            "Session graphique suspendue : test interrompu".into(),
        );
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.failure.is_some() || self.result.is_some() {
            event_loop.exit();
            return;
        }
        if self.cancel.load(Ordering::Relaxed) {
            self.fail(event_loop, "Test annulé".into());
            return;
        }
        let (Some(renderer), Some(recording)) = (&mut self.renderer, &mut self.recording) else {
            return;
        };
        match recording.step(renderer, &self.config) {
            Ok(true) => {
                self.result = Some(result(
                    self.config.clone(),
                    renderer,
                    std::mem::take(&mut recording.runs),
                    false,
                    self.focus_lost,
                    self.sampler
                        .take()
                        .expect("sampler created with renderer")
                        .finish(),
                ));
                event_loop.exit();
            }
            Ok(false) => {
                if self.title_updated.elapsed().as_millis() > 500 {
                    let warming = recording.measure_started.is_none();
                    if let Some(window) = &self.window {
                        window.set_title(&format!(
                            "GameShift · {} · répétition {}/{}",
                            if warming { "Échauffement" } else { "Mesure" },
                            recording.runs.len() + 1,
                            self.config.repetitions
                        ));
                    }
                    self.title_updated = Instant::now();
                }
            }
            Err(e) => self.fail(event_loop, e),
        }
    }
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        let errors = self.renderer.as_ref().map(|r| r.validation_errors.clone());
        drop(self.sampler.take());
        drop(self.renderer.take());
        drop(self.window.take());
        if let Some(errors) = errors {
            let errors = errors.load(Ordering::Relaxed);
            if errors > 0 {
                self.result = None;
                self.failure = Some(format!(
                    "{errors} erreurs de validation Vulkan ; résultats refusés"
                ));
            }
        }
    }
}

/// Runs on the child process main thread. Never call on the GTK main thread.
pub fn run(config: RunConfig, cancel: Arc<AtomicBool>, headless: bool) -> Result<RunResult> {
    config.validate()?;
    if headless {
        let mut renderer = Renderer::new(&config, None)?;
        let errors = renderer.validation_errors.clone();
        let mut recording = Recording::new();
        let sampler = gameshift_telemetry::GpuSampler::start(
            renderer.pci_address.as_deref(),
            recording.telemetry_phase.clone(),
        );
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("Test annulé".into());
            }
            if recording.step(&mut renderer, &config)? {
                break;
            }
        }
        let result = result(
            config,
            &renderer,
            recording.runs,
            true,
            false,
            sampler.finish(),
        );
        drop(renderer);
        if errors.load(Ordering::Relaxed) > 0 {
            return Err("Validation Vulkan en échec : résultats refusés".into());
        }
        return Ok(result);
    }
    let event_loop =
        EventLoop::new().map_err(|e| format!("Session graphique inaccessible : {e}"))?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        renderer: None,
        window: None,
        config,
        cancel,
        recording: None,
        result: None,
        failure: None,
        focus_lost: false,
        title_updated: Instant::now(),
        sampler: None,
    };
    event_loop
        .run_app(&mut app)
        .map_err(|e| format!("Boucle du benchmark : {e}"))?;
    app.result.ok_or_else(|| {
        app.failure
            .unwrap_or_else(|| "Test interrompu sans résultat".into())
    })
}

/// Render and read back actual Vulkan pixels for development, without publishing a score.
pub fn smoke_offscreen(config: RunConfig) -> Result<(String, Vec<u8>, usize)> {
    let mut renderer = Renderer::new(&config, None)?;
    let name = renderer.gpu_name.clone();
    let errors = renderer.validation_errors.clone();
    renderer.draw(&config, 5.0)?;
    let pixels = renderer.readback()?;
    drop(renderer);
    let count = errors.load(Ordering::Relaxed);
    if count > 0 {
        return Err(format!(
            "{count} erreurs de validation pendant le smoke test"
        ));
    }
    Ok((name, pixels, count))
}
