//! Compiled into the test executable only. Never exported or stored as a run.
use adw::prelude::*;
use gameshift_system_bench::{
    session::{Fingerprint, Session, SystemKind},
    system::{self, metric, Body, Category, State},
};

fn specimen(reference: bool) -> Session {
    let values = [
        2000.,
        4002.99_f64.powi(2) / 2000.,
        1060.99,
        58.,
        48.,
        52.12_f64.powi(3) / (58. * 48.),
        1.5,
        1.25_f64.powi(2) / 1.5,
    ];
    let indices = [120., 100., 92., 116., 80., 106., 115., 104.];
    let specs = [
        ("cpu/single/1", "cpu", "CPU monothread", "étapes/s"),
        (
            "cpu/multi_sustained/14",
            "cpu",
            "CPU multithread",
            "étapes/s",
        ),
        ("gpu/raster", "gpu", "GPU rasterisation", "images/s"),
        (
            "ram/read/268435456",
            "ram",
            "Mémoire lecture",
            "Go/s logiques",
        ),
        (
            "ram/write/268435456",
            "ram",
            "Mémoire écriture",
            "Go/s logiques",
        ),
        (
            "ram/copy/268435456",
            "ram",
            "Mémoire copie",
            "Go/s logiques",
        ),
        ("storage/read", "storage", "Stockage lecture", "Go/s"),
        ("storage/write", "storage", "Stockage écriture", "Go/s"),
    ];
    let metrics = specs
        .into_iter()
        .enumerate()
        .map(|(i, (id, category, name, unit))| {
            metric(
                id,
                category,
                name,
                unit,
                vec![
                    if reference {
                        values[i] * 100. / indices[i]
                    } else {
                        values[i]
                    };
                    3
                ],
                "SYNTHETIC UI FIXTURE — NOT A HARDWARE MEASUREMENT",
            )
        })
        .collect::<Vec<_>>();
    let frames = (0..4244)
        .map(|i| {
            let fps = 1061. + 35. * (i as f64 * 0.47).sin() + 12. * (i as f64 * 0.17).cos();
            serde_json::json!({"frame_interval_ms":1000./fps})
        })
        .collect::<Vec<_>>();
    let categories=["cpu","cache","ram","gpu","combined","glibc","applications","storage"].into_iter().map(|id|{
        let selected=metrics.iter().filter(|m|m.category==id).cloned().collect::<Vec<_>>();
        let raw=match id{"gpu"=>serde_json::json!({"identity":{"name":"AMD Radeon RX 7800 XT","driver_name":"RADV · test UI","software":false},"passes":[{"frames":frames}],"synthetic_test_only":true}),"storage"=>serde_json::json!({"volume":"NVMe · test UI","synthetic_test_only":true}),_=>serde_json::json!({"synthetic_test_only":true})};
        Category{id:id.into(),state:if selected.is_empty(){State::Unavailable}else{State::Executed},elapsed_seconds:1.,metrics:selected,raw,notes:vec!["Données synthétiques, validation visuelle uniquement".into()]}
    }).collect::<Vec<_>>();
    let metrics = categories.iter().flat_map(|c| c.metrics.clone()).collect();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut session = Session {
        schema_version: 3,
        protocol: system::scoring::PROTOCOL.into(),
        application_version: env!("CARGO_PKG_VERSION").into(),
        completed_unix_seconds: if reference { now - 86400 } else { now },
        kind: SystemKind::Original,
        fingerprint: Fingerprint {
            distribution: "Test UI".into(),
            distribution_id: "synthetic".into(),
            kernel: Some("jeu de données visuel".into()),
            architecture: "x86_64".into(),
            cpu_model: Some("AMD Ryzen 7 7800X3D".into()),
            microcode: None,
            logical_cpus: (0..16).collect(),
            physical_cores: Some(8),
            ram_kib: Some(32 * 1048576),
            pci_gpus: vec!["SYNTHETIC TEST GPU".into()],
            governors: Default::default(),
            energy_preferences: Default::default(),
            engine_sha256: "a".repeat(64),
            helper_sha256: "b".repeat(64),
        },
        elapsed_seconds: 132.,
        metrics,
        cpu_raw: vec![],
        glibc_raw: serde_json::Value::Null,
        limitations: vec!["Données synthétiques — jamais enregistrées ni exportées".into()],
        system: Some(Body {
            protocol: system::scoring::PROTOCOL.into(),
            target_seconds: 120.,
            elapsed_seconds: 132.,
            overrun_seconds: 12.,
            categories,
            parameters: serde_json::json!({"synthetic_test_only":true}),
        }),
    };
    let points = serde_json::to_value(system::scoring::calculate(&session)).unwrap();
    session.system.as_mut().unwrap().parameters["rate_scores"] = points;
    session.system.as_mut().unwrap().parameters["score_version"] = system::scoring::VERSION.into();
    session.validate().unwrap();
    session
}
pub fn fixture_for_scale_test() -> Session {
    specimen(false)
}
pub fn inject(window: &gtk::Window) -> bool {
    let Some(root) = crate::benchmark::named_widget(window.upcast_ref(), "aurelia-result-body")
        .and_then(|w| w.downcast::<gtk::Box>().ok())
    else {
        return false;
    };
    let current = specimen(false);
    let reference = specimen(true);
    let model = crate::aurelia_results::system(&current, Some(&reference), false);
    crate::aurelia_results::render_system(&root, &model);
    let banner = gtk::Label::new(Some(
        "TEST VISUEL · DONNÉES SYNTHÉTIQUES · AUCUN RÉSULTAT MESURÉ",
    ));
    banner.set_wrap(true);
    banner.add_css_class("gs-aurelia-test-banner");
    banner.set_halign(gtk::Align::Start);
    if let Some(title) =
        crate::benchmark::named_widget(window.upcast_ref(), "gameshift-window-title")
            .and_then(|w| w.downcast::<gtk::Label>().ok())
    {
        title.set_text("TEST VISUEL · DONNÉES SYNTHÉTIQUES · AUCUN RÉSULTAT MESURÉ");
        title.add_css_class("gs-aurelia-test-banner");
    } else {
        root.prepend(&banner);
    }
    true
}
#[test]
#[ignore = "Explicit GTK capture only; requires GAMESHIFT_AURELIA_VISUAL_TEST=1 and a display"]
fn approved_proposal_capture() {
    assert_eq!(
        std::env::var("GAMESHIFT_AURELIA_VISUAL_TEST").as_deref(),
        Ok("1")
    );
    let path = std::env::var_os("GAMESHIFT_AURELIA_VISUAL_OUTPUT")
        .expect("Explicit output directory required");
    assert_eq!(
        crate::overview_capture::run(path.into()),
        gtk::glib::ExitCode::SUCCESS
    );
}
#[test]
fn visual_fixture_uses_real_scoring_code_but_is_not_a_recorded_run() {
    let current = specimen(false);
    let reference = specimen(true);
    let model = crate::aurelia_results::system(&current, Some(&reference), false);
    assert!(model.completed);
    assert_eq!(
        model
            .categories
            .iter()
            .flat_map(|c| &c.measures)
            .filter(|m| m.reference_index.is_some())
            .count(),
        8
    );
    assert!((model.points.categories["cpu"].unwrap() - 4002.99).abs() < 1e-6);
    assert!((model.points.categories["ram"].unwrap() - 52.12).abs() < 1e-6);
    assert!(current.limitations[0].contains("synthétiques"));
}
