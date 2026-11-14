//! Presentation-only Aurelia summaries. Scores come from the existing comparison engine.
use adw::prelude::*;
use gameshift_system_bench::{
    session::Session,
    system::{reporting, State},
};

#[derive(Clone, Debug)]
pub struct Measure {
    pub name: String,
    pub value: f64,
    pub unit: String,
    pub variation: f64,
    pub uncertain: bool,
    pub reference_index: Option<f64>,
}
#[derive(Clone, Debug)]
pub struct Category {
    pub id: String,
    pub name: String,
    pub state: String,
    pub index: Option<f64>,
    pub uncertain: bool,
    pub measures: Vec<Measure>,
    pub notes: Vec<String>,
}
pub struct SystemResult {
    pub configuration: Vec<(String,String)>,
    pub date: String,
    pub duration_seconds: f64,
    pub protocol: String,
    pub completed: bool,
    pub points: gameshift_system_bench::system::scoring::Scores,
    pub previous_points: std::collections::BTreeMap<String, (f64, f64)>,
    pub hardware: std::collections::BTreeMap<String, String>,
    pub fps: Vec<f64>,
    pub metadata: String,
    pub status: String,
    pub index: Option<f64>,
    pub explanation: String,
    pub reference: Option<String>,
    pub score_version: &'static str,
    pub categories: Vec<Category>,
    pub warnings: Vec<String>,
}
fn category_name(id: &str) -> &str {
    match id {
        "cpu" => "CPU",
        "gpu" => "GPU",
        "ram" => "RAM",
        "storage" => "Stockage",
        "cache" => "Cache CPU",
        "combined" => "Charge combinée",
        "glibc" => "Fonctions glibc",
        "applications" => "Bibliothèques système",
        other => other,
    }
}
fn number(value: f64, digits: usize) -> String {
    if value.is_finite() {
        format!("{value:.digits$}").replace('.', ",")
    } else {
        "—".into()
    }
}
fn index_text(index: Option<f64>) -> String {
    index
        .filter(|v| v.is_finite() && *v > 0.)
        .map(|v| number(v, 1))
        .unwrap_or_else(|| "—".into())
}
pub fn system(
    session: &Session,
    reference: Option<&Session>,
    across_hardware: bool,
) -> SystemResult {
    let mut warnings = Vec::new();
    let comparison = reference.and_then(|a| {
        if session.system.is_none() || a.system.is_none() {
            return None;
        }
        match reporting::compare_with_hardware(a, session, across_hardware) {
            Ok(scores) => Some(scores),
            Err(error) => {
                warnings.push(format!("Indices indisponibles : {error}"));
                None
            }
        }
    });
    let mut categories = Vec::new();
    let ids = [
        "cpu",
        "gpu",
        "ram",
        "storage",
        "cache",
        "combined",
        "glibc",
        "applications",
    ];
    for id in ids {
        let source = session
            .system
            .as_ref()
            .and_then(|s| s.categories.iter().find(|c| c.id == id));
        let measures: Vec<_> = session
            .metrics
            .iter()
            .filter(|m| m.category == id)
            .map(|m| Measure {
                name: m.label.clone(),
                value: m.mean(),
                unit: m.unit.clone(),
                variation: m.cv() * 100.,
                uncertain: !m.confirmation_reasons.is_empty() || m.cv() > 0.05,
                reference_index: reference.filter(|a| comparison.is_some() && source.is_some_and(|c|c.state==State::Executed) && a.system.as_ref().is_some_and(|b|b.categories.iter().any(|c|c.id==id && c.state==State::Executed)) && gameshift_system_bench::system::scoring::compatible(a,session,id,across_hardware)).and_then(|a| {
                    let old=a.metrics.iter().find(|old|old.id==m.id || (across_hardware && m.id.starts_with("cpu/multi_sustained/") && old.id.starts_with("cpu/multi_sustained/")))?;
                    let index=100.*(m.mean()/old.mean());
                    (index.is_finite() && index>0.).then_some(index)
                }),
            })
            .collect();
        let index = comparison
            .as_ref()
            .and_then(|c| c.categories.get(id).copied().flatten());
        let uncertain = measures.iter().any(|m| m.uncertain)
            || comparison
                .as_ref()
                .is_some_and(|c| c.uncertain.contains(id));
        categories.push(Category {
            id: id.into(),
            name: category_name(id).into(),
            state: source
                .map(|c| c.state.label())
                .unwrap_or(if measures.is_empty() {
                    "Non exécuté"
                } else {
                    "Mesures historiques"
                })
                .into(),
            index,
            uncertain,
            measures,
            notes: source.map(|c| c.notes.clone()).unwrap_or_default(),
        });
    }
    let complete = ["cpu", "cache", "ram", "gpu", "combined"].iter().all(|id| {
        session.system.as_ref().is_some_and(|s| {
            s.categories
                .iter()
                .any(|c| c.id == *id && c.state == State::Executed)
        })
    });
    let index = comparison.as_ref().and_then(|c| c.native);
    let explanation = if index.is_some() {
        "Indice relatif au résultat de référence (100). Moyenne géométrique des cinq catégories matérielles ; aucun point absolu.".into()
    } else if session.system.is_none() {
        "Parcours historique partiel : aucun indice matériel complet. Le rapport d’origine et ses comparaisons restent accessibles dans les détails.".into()
    } else if !complete {
        "Résultat matériel partiel : CPU, cache, RAM, GPU et charge combinée sont nécessaires à l’indice complet. Les mesures disponibles restent consultables.".into()
    } else if reference.is_none() {
        "Mesures disponibles. Choisissez une référence compatible dans l’historique pour calculer les indices relatifs ; aucun score absolu n’est attribué.".into()
    } else {
        "Indice matériel non calculé : référence ou épreuves incompatibles. Les catégories compatibles restent présentées séparément.".into()
    };
    if let Some(scores) = &comparison {
        warnings.extend(scores.warnings.iter().cloned());
    }
    warnings.sort();
    warnings.dedup();
    let gpu = session
        .system
        .as_ref()
        .and_then(|s| s.categories.iter().find(|c| c.id == "gpu"))
        .and_then(|c| c.raw["identity"]["name"].as_str())
        .unwrap_or("Non enregistré");
    let ram = session
        .fingerprint
        .ram_kib
        .map(|v| format!("{} Gio", number(v as f64 / 1048576., 1)))
        .unwrap_or_else(|| "Non enregistrée".into());
    let mut metadata = format!(
        "{} · {} s\nCPU : {} · RAM : {ram}\nGPU : {gpu}\nNoyau : {} · GameShift {} · {}",
        session.fingerprint.distribution,
        number(session.elapsed_seconds, 1),
        session
            .fingerprint
            .cpu_model
            .as_deref()
            .unwrap_or("Non enregistré"),
        session
            .fingerprint
            .kernel
            .as_deref()
            .unwrap_or("Non enregistré"),
        session.application_version,
        session.protocol
    );
    if let Some(name) = session
        .system
        .as_ref()
        .and_then(|body| body.parameters["gpu_profile_before"]["active_profile"]["name"].as_str())
    {
        metadata.push_str(&format!("\nProfil GPU observé avant mesure : {name}"));
    }
    let date = gtk::glib::DateTime::from_unix_local(session.completed_unix_seconds as i64)
        .ok()
        .and_then(|d| d.format("%d/%m/%Y à %H:%M").ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "Date non enregistrée".into());
    metadata.push_str(&format!("\n{date}"));
    let status = if categories.iter().any(|c| c.uncertain) {
        "Mesures ou écarts à confirmer"
    } else if complete {
        "Couverture matérielle complète"
    } else {
        "Résultat partiel · couverture matérielle incomplète"
    }
    .to_owned();
    let points = gameshift_system_bench::system::scoring::calculate(session);
    let mut previous_points = std::collections::BTreeMap::new();
    if let Some(a) = reference.filter(|_| comparison.is_some()) {
        let previous = gameshift_system_bench::system::scoring::calculate(a);
        for id in ["cpu", "gpu", "ram", "storage"] {
            if gameshift_system_bench::system::scoring::compatible(a, session, id, across_hardware) {
                if let (Some(old), Some(new)) = (previous.categories[id], points.categories[id]) {
                    previous_points.insert(id.into(), (old, new));
                }
            }
        }
        if previous_points.len() == 4 {
            if let (Some(old), Some(new)) = (previous.global, points.global) { previous_points.insert("global".into(), (old, new)); }
        }
    }
    let storage = session.system.as_ref().and_then(|b| b.categories.iter().find(|c|c.id=="storage")).and_then(|c|c.raw["volume"].as_str()).unwrap_or("Non testé");
    let hardware = [("cpu".into(),session.fingerprint.cpu_model.clone().unwrap_or_else(||"CPU non identifié".into())), ("gpu".into(),gpu.into()), ("ram".into(),ram), ("storage".into(),storage.into())].into_iter().collect();
    let fps = session.system.as_ref().and_then(|b|b.categories.iter().find(|c|c.id=="gpu")).and_then(|c|c.raw["passes"].as_array()).and_then(|p|p.first()).and_then(|p|p["frames"].as_array()).map(|frames|frames.iter().filter_map(|f|f["frame_interval_ms"].as_f64()).filter(|v|v.is_finite() && *v>0.).map(|v|1000./v).collect()).unwrap_or_default();
    SystemResult {
        configuration: vec![("Système".into(),session.fingerprint.distribution.clone()),("Noyau".into(),session.fingerprint.kernel.clone().unwrap_or_else(||"Non enregistré".into())),("Pilote".into(),session.system.as_ref().and_then(|b|b.categories.iter().find(|c|c.id=="gpu")).map(|c|[c.raw["identity"]["driver_name"].as_str(),c.raw["identity"]["driver_info"].as_str()].into_iter().flatten().filter(|s|!s.is_empty()).collect::<Vec<_>>().join(" · ")).filter(|s|!s.is_empty()).unwrap_or_else(||"Non enregistré".into()))],
        date,
        duration_seconds: session.elapsed_seconds,
        protocol: session.protocol.clone(),
        completed: ["cpu","gpu","ram","storage"].iter().all(|id|session.system.as_ref().is_some_and(|b|b.categories.iter().any(|c|c.id==*id && c.state==State::Executed))),
        points, previous_points, hardware, fps,
        metadata,
        status,
        score_version: if session.system.is_some() {
            reporting::SCORE_VERSION
        } else {
            gameshift_system_bench::comparison::SCORE_VERSION
        },
        index,
        explanation,
        reference: reference.map(|s| {
            format!(
                "{} · {}",
                s.fingerprint.distribution, gtk::glib::DateTime::from_unix_local(s.completed_unix_seconds as i64).ok().and_then(|d|d.format("%d/%m/%Y · %H:%M").ok()).map(|d|d.to_string()).unwrap_or_else(||"Date indisponible".into())
            )
        }),
        categories,
        warnings,
    }
}
fn text(value: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(value));
    label.set_wrap(true);
    label.set_xalign(0.);
    label.set_selectable(true);
    label.add_css_class(class);
    label
}
fn column(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, spacing)
}
fn clear(root: &gtk::Box) {
    while let Some(child) = root.first_child() {
        root.remove(&child);
    }
}
pub fn hero(root: &gtk::Box, title: &str, value: &str, unit: &str, explanation: &str) {
    let card = crate::gloss_card::column(6);
    card.add_css_class("gs-aurelia-hero");
    card.append(&text(title, "gs-aurelia-kicker"));
    let value = text(value, "gs-aurelia-result");
    value.set_tooltip_text(Some(explanation));
    card.append(&value);
    card.append(&text(unit, "gs-aurelia-unit"));
    card.append(&text(explanation, "gs-card-detail"));
    root.append(&card);
}
pub fn cards(root: &gtk::Box, entries: &[(String, String, String)]) {
    let grid = gtk::FlowBox::new();
    grid.set_selection_mode(gtk::SelectionMode::None);
    grid.set_min_children_per_line(1);
    grid.set_max_children_per_line(3);
    grid.set_homogeneous(true);
    grid.set_column_spacing(12);
    grid.set_row_spacing(12);
    grid.add_css_class("gs-aurelia-grid");
    for (title, value, detail) in entries {
        let card = crate::gloss_card::column(6);
        card.add_css_class("gs-aurelia-tile");
        card.set_hexpand(true);
        let heading = text(title, "gs-card-label");
        heading.set_max_width_chars(28);
        card.append(&heading);
        card.append(&text(value, "gs-aurelia-subscore"));
        let description = text(detail, "gs-card-detail");
        description.set_max_width_chars(30);
        card.append(&description);
        grid.insert(&card, -1);
    }
    root.append(&grid);
}
pub fn render_system(root: &gtk::Box, result: &SystemResult) {
    crate::aurelia_dashboard::render(root, result);
}
pub fn render_system_details(root: &gtk::Box, result: &SystemResult) {
    clear(root);
    hero(
        root,
        "INDICE MATÉRIEL AURELIA",
        &index_text(result.index),
        "Indice relatif · référence = 100",
        &result.explanation,
    );
    root.append(&text(&result.status, "gs-card-label"));
    root.append(&text(&result.metadata, "gs-card-detail"));
    if let Some(reference) = &result.reference {
        root.append(&text(
            &format!(
                "Référence : {reference}\nMéthode : {}",
                result.score_version
            ),
            "gs-card-detail",
        ));
    }
    let entries = result
        .categories
        .iter()
        .filter(|c| ["cpu", "gpu", "ram"].contains(&c.id.as_str()))
        .map(|c| {
            let value = c
                .index
                .map(|v| format!("{} · {:+.1} %", number(v, 1), v - 100.))
                .unwrap_or_else(|| {
                    if c.measures.is_empty() {
                        "Indisponible".into()
                    } else {
                        "Mesuré".into()
                    }
                });
            let mut detail = c
                .index
                .map(|_| "Indice relatif · A = 100\n".to_owned())
                .unwrap_or_default();
            detail.push_str(&c.state);
            if c.uncertain {
                detail.push_str(" · À confirmer");
            }
            for m in c.measures.iter().take(3) {
                detail.push_str(&format!("\n{} : {} {}", m.name, number(m.value, 3), m.unit));
            }
            if c.measures.len() > 3 {
                detail.push_str(&format!(
                    "\n{} épreuves · détail ci-dessous",
                    c.measures.len()
                ));
            }
            (c.name.clone(), value, detail)
        })
        .collect::<Vec<_>>();
    cards(root, &entries);
    root.append(&text("Détail des performances", "gs-section-title"));
    let maximum = result
        .categories
        .iter()
        .filter_map(|c| c.index)
        .fold(100., f64::max)
        * 1.05;
    if result.categories.iter().any(|c| c.index.is_some()) {
        root.append(&text(&format!("Échelle commune : 0–{} · référence A = 100. Les indices peuvent dépasser 100 ; ce ne sont pas des pourcentages de capacité.", number(maximum, 1)), "gs-card-detail"));
    }
    for category in &result.categories {
        let expander = gtk::Expander::new(Some(&format!(
            "{} · {} · {} épreuve(s){}",
            category.name,
            category.state,
            category.measures.len(),
            if category.uncertain {
                " · À confirmer"
            } else {
                ""
            }
        )));
        let details = column(8);
        for m in &category.measures {
            let row = column(3);
            row.add_css_class("gs-aurelia-measure");
            row.append(&text(&m.name, "heading"));
            row.append(&text(
                &format!(
                    "{} {} · variation {} %{}",
                    number(m.value, 3),
                    m.unit,
                    number(m.variation, 2),
                    if m.uncertain { " · À confirmer" } else { "" }
                ),
                "gs-card-detail",
            ));
            row.set_tooltip_text(Some("Mesure brute, moyenne des répétitions. La variation est le coefficient de variation ; ce n’est pas une preuve de significativité statistique."));
            details.append(&row);
        }
        for note in &category.notes {
            details.append(&text(note, "gs-card-detail"));
        }
        if category.measures.is_empty() {
            details.append(&text(
                "Aucune valeur n’est remplacée par zéro.",
                "gs-card-detail",
            ));
        }
        expander.set_child(Some(&details));
        root.append(&expander);
        if let Some(index) = category.index {
            root.append(&text(
                &format!(
                    "{} : {} · écart {:+.1} %",
                    category.name,
                    number(index, 1),
                    index - 100.
                ),
                "gs-card-detail",
            ));
            for (name, value) in [("Référence A", 100.), ("Résultat B", index)] {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                let label = text(name, "gs-card-detail");
                label.set_width_chars(12);
                row.append(&label);
                let bar = gtk::LevelBar::for_interval(0., maximum);
                bar.set_value(value);
                bar.set_hexpand(true);
                bar.set_tooltip_text(Some(&format!(
                    "{name} : {} · référence = 100",
                    number(value, 1)
                )));
                row.append(&bar);
                root.append(&row);
            }
        }
    }
    if !result.warnings.is_empty() {
        let warnings = gtk::Expander::new(Some(&format!(
            "Interprétation et réserves · {}",
            result.warnings.len()
        )));
        warnings.set_child(Some(&text(&result.warnings.join("\n"), "gs-card-detail")));
        root.append(&warnings);
    }
}
#[derive(Debug)]
pub struct CpuResult {
    pub value: String,
    pub unit: String,
    pub explanation: String,
    pub cards: Vec<(String, String, String)>,
}
pub fn cpu(report: &gameshift_cpu::runner::Report) -> CpuResult {
    let mut cards = Vec::new();
    for case in &report.cases {
        cards.push((
            format!("{} thread(s) · {} Mio", case.workers, case.size_mib),
            format!("{} ms", number(case.mean_step_ms, 3)),
            format!(
                "{} Mobjets/s · variation {} %{}",
                number(case.entities_per_second / 1e6, 3),
                number(case.repetition_cv * 100., 2),
                if case.inconclusive {
                    " · À confirmer"
                } else {
                    ""
                }
            ),
        ));
    }
    let (value, unit) = if report.cases.len() == 1 {
        (
            number(report.cases[0].mean_step_ms, 3),
            "ms par étape de simulation · mesure brute".into(),
        )
    } else {
        (
            report.cases.len().to_string(),
            "Configurations CPU mesurées · résultats séparés".into(),
        )
    };
    CpuResult { value, unit, explanation: format!("{} · moteur {}. Un temps par étape plus faible indique un traitement plus rapide pour ce travail précis. Sans indice global : chaque carte identifie les threads et le volume de données ; le débit d’objets n’est pas un FPS.", report.protocol, report.engine_version), cards }
}
pub fn render_cpu(root: &gtk::Box, result: &CpuResult) {
    clear(root);
    hero(
        root,
        "AURELIA · PROCESSEUR",
        &result.value,
        &result.unit,
        &result.explanation,
    );
    if result.cards.len() > 3 {
        let expander = gtk::Expander::new(Some(
            "Détail des épreuves CPU · temps et débit par configuration",
        ));
        let content = column(8);
        cards(&content, &result.cards);
        expander.set_child(Some(&content));
        root.append(&expander);
    } else {
        cards(root, &result.cards);
    }
    root.set_visible(true);
}
pub fn render_raster(root: &gtk::Box, result: &gameshift_bench::RunResult) {
    let frames = result
        .repetitions
        .iter()
        .map(|r| r.statistics.frames)
        .sum::<usize>();
    let ms = result
        .repetitions
        .iter()
        .map(|r| r.statistics.measured_ms)
        .sum::<f64>();
    let fps = (ms > 0.).then_some(frames as f64 * 1000. / ms);
    hero(root, "AURELIA · RASTERISATION VULKAN", &fps.map(|v| number(v, 1)).unwrap_or_else(|| "—".into()), "FPS moyens · mesure brute",
        "Scène synthétique Aurelia. Ces FPS ne prédisent pas les performances des jeux et ne constituent pas un indice matériel complet.");
    root.append(&text(
        &format!(
            "{} · {} × {} · MSAA {}× · {}\n{} · GameShift {}",
            result.gpu_name,
            result.actual_width,
            result.actual_height,
            result.msaa_samples,
            result.present_mode,
            result.scene_version,
            result.application_version
        ),
        "gs-card-detail",
    ));
    cards(root, &[
        ("Temps moyen par image".into(), if frames > 0 {format!("{} ms", number(ms / frames as f64, 3))} else {"—".into()}, "Intervalle CPU de fin de frame, incluant les attentes ; pas un temps d’affichage physique.".into()),
        ("Répétitions".into(), result.repetitions.len().to_string(), format!("{} images enregistrées · {} s mesurées", frames, number(ms / 1000., 1))),
        ("Variation".into(), result.repetition_cv_percent.map(|v| format!("{} %", number(v, 2))).unwrap_or_else(|| "—".into()), "Variation des FPS moyens entre répétitions. Un écart faible ne prouve pas un gain.".into())]);
    if !result.warnings.is_empty() {
        root.append(&text(
            &format!(
                "{} réserve(s) de mesure : consultez les détails avant de comparer.",
                result.warnings.len()
            ),
            "warning",
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gameshift_system_bench::{
        session::{Fingerprint, SystemKind},
        system::{self, Body, Category as RawCategory},
    };
    fn fixture(rate: f64) -> Session {
        let categories: Vec<_> = [
            "cpu",
            "cache",
            "ram",
            "gpu",
            "combined",
            "glibc",
            "applications",
        ]
        .iter()
        .map(|id| RawCategory {
            id: (*id).into(),
            state: State::Executed,
            elapsed_seconds: 1.,
            metrics: vec![system::metric(
                &format!("{id}/fixture"),
                id,
                "Test fixture",
                "unit/s",
                vec![rate; 3],
                "same-work",
            )],
            raw: serde_json::json!({"identity":{"software":false}}),
            notes: Vec::new(),
        })
        .collect();
        Session {
            schema_version: 2,
            protocol: system::PROTOCOL.into(),
            application_version: "fixture".into(),
            completed_unix_seconds: 1,
            kind: SystemKind::Original,
            fingerprint: Fingerprint {
                distribution: "fixture".into(),
                distribution_id: "fixture".into(),
                kernel: None,
                architecture: "x86_64".into(),
                cpu_model: Some("fixture CPU".into()),
                microcode: None,
                logical_cpus: vec![0, 1],
                physical_cores: Some(1),
                ram_kib: Some(1024),
                pci_gpus: Vec::new(),
                governors: Default::default(),
                energy_preferences: Default::default(),
                engine_sha256: "a".repeat(64),
                helper_sha256: "b".repeat(64),
            },
            elapsed_seconds: 7.,
            metrics: categories.iter().flat_map(|c| c.metrics.clone()).collect(),
            cpu_raw: Vec::new(),
            glibc_raw: serde_json::Value::Null,
            limitations: Vec::new(),
            system: Some(Body {
                protocol: system::PROTOCOL.into(),
                target_seconds: 120.,
                elapsed_seconds: 7.,
                overrun_seconds: 0.,
                categories,
                parameters: serde_json::Value::Null,
            }),
        }
    }
    #[test]
    fn standalone_measurements_never_become_fabricated_scores() {
        let report = fixture(120.);
        report.validate().unwrap();
        let view = system(&report, None, false);
        assert!(view.index.is_none());
        assert!(view
            .categories
            .iter()
            .all(|c| c.index.is_none() && c.measures.iter().all(|m|m.value == 120.)));
        assert!(view.categories.iter().find(|c|c.id=="storage").unwrap().measures.is_empty());
        assert!(view.explanation.contains("référence"));
        assert!(view.metadata.contains("Noyau : Non enregistré"));
    }
    #[test]
    fn displayed_indices_reuse_existing_engine_and_partial_runs_keep_missing_values() {
        let baseline = fixture(100.);
        let mut candidate = fixture(120.);
        let view = system(&candidate, Some(&baseline), false);
        assert!((view.index.unwrap() - 120.).abs() < 1e-8);
        assert_eq!(view.score_version, reporting::SCORE_VERSION);
        let gpu = candidate
            .system
            .as_mut()
            .unwrap()
            .categories
            .iter_mut()
            .find(|c| c.id == "gpu")
            .unwrap();
        gpu.state = State::Unavailable;
        gpu.metrics.clear();
        candidate.metrics.retain(|m| m.category != "gpu");
        candidate.validate().unwrap();
        let partial = system(&candidate, Some(&baseline), false);
        assert!(partial.index.is_none());
        let gpu = partial.categories.iter().find(|c| c.id == "gpu").unwrap();
        assert!(gpu.index.is_none() && gpu.measures.is_empty());
        assert_eq!(gpu.state, "Indisponible");
        assert!((partial.categories[0].index.unwrap() - 120.).abs() < 1e-8);
    }
    #[test]
    fn incompatible_references_remain_unscored_with_a_reason() {
        let baseline = fixture(100.);
        let mut candidate = fixture(120.);
        candidate.fingerprint.engine_sha256 = "c".repeat(64);
        let view = system(&candidate, Some(&baseline), false);
        assert!(view.index.is_none() && view.categories.iter().all(|c| c.index.is_none()));
        assert!(view
            .warnings
            .iter()
            .any(|w| w.contains("Indices indisponibles")));
    }
    #[test]
    fn all_metrics_are_retained_in_the_detail_model() {
        let mut report = fixture(100.);
        for i in 0..20 {
            let measure = system::metric(
                &format!("cpu/additional-{i}"),
                "cpu",
                "Additional fixture",
                "unit/s",
                vec![100.; 3],
                "same-work",
            );
            report.metrics.push(measure.clone());
            report.system.as_mut().unwrap().categories[0]
                .metrics
                .push(measure);
        }
        report.validate().unwrap();
        let view = system(&report, None, false);
        assert_eq!(
            view.categories
                .iter()
                .find(|c| c.id == "cpu")
                .unwrap()
                .measures
                .len(),
            21
        );
    }
    #[test]
    fn reference_indices_are_per_measure_and_never_filled_without_compatibility(){
        let reference=fixture(100.);let mut current=fixture(120.);
        let view=system(&current,Some(&reference),false);
        assert!(view.categories.iter().find(|c|c.id=="cpu").unwrap().measures.iter().all(|m|(m.reference_index.unwrap()-120.).abs()<1e-10));
        current.system.as_mut().unwrap().categories.iter_mut().find(|c|c.id=="gpu").unwrap().state=State::Unavailable;
        let view=system(&current,Some(&reference),false);
        assert!(view.categories.iter().find(|c|c.id=="gpu").unwrap().measures.iter().all(|m|m.reference_index.is_none()));
        let view=system(&current,None,false);assert!(view.categories.iter().flat_map(|c|&c.measures).all(|m|m.reference_index.is_none()));
    }
    #[test]
    fn legacy_partial_reports_remain_readable_without_full_material_scores() {
        let mut legacy = fixture(100.);
        legacy.schema_version = 1;
        legacy.protocol = gameshift_system_bench::session::SESSION_PROTOCOL.into();
        legacy.system = None;
        legacy
            .metrics
            .retain(|m| ["cpu", "cache", "glibc"].contains(&m.category.as_str()));
        legacy.validate().unwrap();
        let view = system(&legacy, None, false);
        assert!(view.index.is_none());
        assert_eq!(
            view.score_version,
            gameshift_system_bench::comparison::SCORE_VERSION
        );
        assert!(view.explanation.contains("historique partiel"));
        assert_eq!(
            view.categories
                .iter()
                .map(|c| c.measures.len())
                .sum::<usize>(),
            3
        );
        assert!(view
            .categories
            .iter()
            .find(|c| c.id == "gpu")
            .unwrap()
            .measures
            .is_empty());
    }
}
