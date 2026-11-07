//! Individual reports and independent native/application relative indices.
use super::{State, PROTOCOL};
use crate::session::{Metric, Session};
use std::collections::{BTreeMap, BTreeSet};
const IDS: [&str; 7] = [
    "cpu",
    "cache",
    "ram",
    "gpu",
    "combined",
    "glibc",
    "applications",
];
fn label(id: &str) -> &str {
    match id {
        "cpu" => "Processeur",
        "cache" => "Cache CPU",
        "ram" => "RAM native",
        "gpu" => "GPU Vulkan Aurelia",
        "combined" => "Charge combinée CPU/GPU/RAM",
        "glibc" => "Fonctions glibc",
        "applications" => "Bibliothèques système",
        _ => id,
    }
}
pub fn validate(s: &Session) -> Result<(), String> {
    let body = s.system.as_ref().ok_or("Rapport système absent")?;
    if s.schema_version != 2 || s.protocol != PROTOCOL || body.protocol != PROTOCOL {
        return Err("Version du protocole système incompatible".into());
    }
    if !s.elapsed_seconds.is_finite() || s.elapsed_seconds < 0. || !body.overrun_seconds.is_finite()
    {
        return Err("Durée système invalide".into());
    }
    let ids: BTreeSet<_> = body.categories.iter().map(|c| c.id.as_str()).collect();
    if body.categories.len() != 7 || ids != IDS.into_iter().collect() || s.metrics.len() > 256 {
        return Err("Le rapport doit contenir les sept catégories avec leur état".into());
    }
    for c in &body.categories {
        if c.state == State::NotImplemented && !c.metrics.is_empty() {
            return Err("Mesures présentes dans une catégorie non exécutée".into());
        }
        if c.metrics.len() > 64 {
            return Err("Trop de sous-tests".into());
        }
        if c.state == State::Executed && (c.metrics.is_empty() || c.raw.is_null()) {
            return Err(format!("{} annoncé exécuté sans mesures", c.id));
        }
        if !c.elapsed_seconds.is_finite() || c.elapsed_seconds < 0. {
            return Err("Durée de catégorie invalide".into());
        }
        for m in &c.metrics {
            if m.id.len() > 256
                || m.label.len() > 1024
                || m.category != c.id
                || m.values.len() != 3
                || m.values.iter().any(|v| !v.is_finite() || *v <= 0.)
            {
                return Err(format!("Mesures invalides : {}", m.id));
            }
        }
        if ["gpu", "combined"].contains(&c.id.as_str())
            && c.state == State::Executed
            && (c.raw["identity"]["software"] != false || c.raw["diagnostic_only"] == true)
        {
            return Err("Un diagnostic logiciel ne remplace pas un GPU matériel".into());
        }
    }
    let metrics: BTreeSet<_> = s.metrics.iter().map(|m| m.id.as_str()).collect();
    let nested: BTreeSet<_> = body
        .categories
        .iter()
        .flat_map(|c| c.metrics.iter().map(|m| m.id.as_str()))
        .collect();
    if metrics != nested || metrics.len() != s.metrics.len() {
        return Err("Mesures de catégories incohérentes".into());
    }
    Ok(())
}
fn policies(values: &BTreeMap<String, String>) -> String {
    let values: BTreeSet<_> = values.values().map(String::as_str).collect();
    if values.is_empty() {
        "Indisponible".into()
    } else {
        values.into_iter().collect::<Vec<_>>().join(", ")
    }
}
fn driver_controls_text(snapshot: &serde_json::Value) -> String {
    let mut out = String::new();
    let Some(devices) = snapshot["devices"].as_array() else {
        return "Relevé indisponible dans ce résultat.\n".into();
    };
    if devices.is_empty() {
        out.push_str("Aucun réglage AMDGPU relevé.\n");
    }
    for device in devices {
        out.push_str(&format!(
            "GPU {} · DPM {}\n",
            device["identity"]["pci"]
                .as_str()
                .unwrap_or("non identifié"),
            device["performance"].as_str().unwrap_or("non exposé")
        ));
        if let Some(scalars) = device["scalars"].as_array() {
            for scalar in scalars {
                out.push_str(&format!(
                    "  {} : {} {}\n",
                    scalar["label"].as_str().unwrap_or("Réglage"),
                    scalar["current"],
                    scalar["unit"].as_str().unwrap_or("")
                ));
            }
        }
        if let Some(workloads) = device["workloads"].as_array() {
            for mode in workloads.iter().filter(|v| v["active"] == true) {
                out.push_str(&format!(
                    "  Profil de charge : {} (identifiant {})\n",
                    mode["name"].as_str().unwrap_or("non identifié"),
                    mode["index"]
                ));
            }
        }
        if let Some(clocks) = device["clocks"].as_array() {
            for clock in clocks {
                out.push_str(&format!(
                    "  Fréquences {} : min {} / max {} MHz\n",
                    clock["domain"].as_str().unwrap_or("non identifié"),
                    clock["minimum"]
                        .as_i64()
                        .map(|v| v.to_string())
                        .unwrap_or("non exposée".into()),
                    clock["maximum"]
                        .as_i64()
                        .map(|v| v.to_string())
                        .unwrap_or("non exposée".into())
                ));
            }
        }
        if let Some(points) = device["curve"]["points"].as_array() {
            out.push_str("  Points PMFW lus (mode interne non déduit) : ");
            out.push_str(
                &points
                    .iter()
                    .map(|p| format!("{} °C → {} %", p["temperature"], p["pwm_percent"]))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            out.push('\n');
        }
    }
    out
}
pub fn individual(s: &Session) -> String {
    let mut out=format!("AURELIA — RAPPORT INDIVIDUEL\n{}\nProtocole : {} · GameShift {}\nDurée réelle : {:.2} s\nNoyau : {} · Architecture : {}\nCPU : {}\nMicrocode : {}\nGouverneur : {} · EPP : {}\nMoteur SHA-256 : {}\nAuxiliaire SHA-256 : {}\n",s.fingerprint.distribution,s.protocol,s.application_version,s.elapsed_seconds,s.fingerprint.kernel.as_deref().unwrap_or("Indisponible"),s.fingerprint.architecture,s.fingerprint.cpu_model.as_deref().unwrap_or("Indisponible"),s.fingerprint.microcode.as_deref().unwrap_or("Indisponible"),policies(&s.fingerprint.governors),policies(&s.fingerprint.energy_preferences),s.fingerprint.engine_sha256,s.fingerprint.helper_sha256);
    if let Some(body) = &s.system {
        if body.parameters["driver_controls_changed"] == true {
            out.push_str("\nLes réglages du pilote diffèrent entre le début et la fin du test. Les instantanés sont conservés ; cela ne prouve pas une cause de variation des performances.\n");
        }
        for (key, title) in [
            ("driver_controls_before", "Réglages du pilote avant mesure"),
            ("driver_controls_after", "Réglages du pilote après mesure"),
        ] {
            if let Some(devices) = body.parameters[key]["devices"].as_array() {
                if !devices.is_empty() {
                    out.push_str(&format!("\n{title}\n"));
                }
                out.push_str(&driver_controls_text(&body.parameters[key]));
            }
        }

        out.push_str(&format!("Cible indicative : {:.0} s · dépassement : {:.2} s\n\nSOUS-SCORES\nIndices relatifs non calculés sans référence compatible. Les mesures physiques restent disponibles ci-dessous.\nUn score natif complet exige CPU, cache, RAM, GPU et charge combinée. Une bibliothèque absente n’invalide pas le score natif.\n",body.target_seconds,body.overrun_seconds));
        for category in &body.categories {
            out.push_str(&format!(
                "\n{} — {} · {:.2} s\n",
                label(&category.id),
                category.state.label(),
                category.elapsed_seconds
            ));
            for note in &category.notes {
                out.push_str(&format!("{note}\n"));
            }
            for m in &category.metrics {
                out.push_str(&format!(
                    "{} : {:.3} {} · variation {:.2} %\n  Répétitions : {}\n",
                    m.label,
                    m.mean(),
                    m.unit,
                    m.cv() * 100.,
                    m.values
                        .iter()
                        .map(|v| format!("{v:.3}"))
                        .collect::<Vec<_>>()
                        .join(" / ")
                ));
                if m.cv() > 0.05 || !m.confirmation_reasons.is_empty() {
                    out.push_str(
                        "  Résultat à confirmer : mesures variables ou signalées incertaines.\n",
                    );
                }
                for reason in &m.confirmation_reasons {
                    out.push_str(&format!("  {reason}\n"));
                }
            }
            if category.id == "cache" {
                if let Some(cases) = category.raw.as_array() {
                    for case in cases {
                        if let Some(samples) = case["latency"].as_array() {
                            for sample in samples {
                                if let Some(ns) = sample["ns_per_access"].as_f64() {
                                    out.push_str(&format!(
                                        "Parcours chaîné {} octets : {ns:.2} ns/accès observés\n",
                                        sample["bytes"]
                                    ));
                                }
                            }
                        }
                    }
                }
            }
            if category.id == "ram" {
                if let Some(bytes) = category.raw["buffer_bytes"].as_u64() {
                    out.push_str(&format!(
                        "Jeu de données : {} Mio par buffer ; LLC observé : {:.1} Mio\n",
                        bytes / 1048576,
                        category.raw["observed_llc_bytes"].as_f64().unwrap_or(0.) / 1048576.
                    ));
                }
                if let Some(samples) = category.raw["latency"].as_array() {
                    for sample in samples {
                        out.push_str(&format!(
                            "Latence observée du parcours dépendant : {:.2} ns/accès\n",
                            sample["ns_per_access"].as_f64().unwrap_or(f64::NAN)
                        ));
                    }
                }
            }
            if category.id == "gpu" || category.id == "combined" {
                let identity = &category.raw["identity"];
                if let Some(name) = identity["name"].as_str() {
                    out.push_str(&format!("GPU : {name}\nPilote Vulkan : {} · {}\nRendu hors écran, MSAA {}, validation {}\n",identity["driver_name"].as_str().unwrap_or("Indisponible"),identity["driver_info"].as_str().unwrap_or("Indisponible"),identity["msaa"],identity["validation_enabled"]));
                }
                if let Some(passes) = category.raw["passes"].as_array() {
                    for pass in passes {
                        let st = &pass["statistics"];
                        out.push_str(&format!("Passage : {} images · temps/image {:.3} ms · P95 {:.3} ms · P99 {:.3} ms\n  Calcul GPU : {} · préparation CPU : {}\n",st["frames"],st["average_frametime_ms"].as_f64().unwrap_or(0.),st["p95_ms"].as_f64().unwrap_or(0.),st["p99_ms"].as_f64().unwrap_or(0.),st["average_gpu_execution_ms"].as_f64().map(|v|format!("{v:.3} ms")).unwrap_or("Indisponible".into()),st["average_cpu_work_ms"].as_f64().map(|v|format!("{v:.3} ms")).unwrap_or("Indisponible".into())));
                    }
                }
                if let Some(values) = category.raw["degradation"].as_array() {
                    for v in values {
                        out.push_str(&format!(
                            "Dégradation simultanée {} : {:.2} % (négatif = débit supérieur)\n",
                            v["component"].as_str().unwrap_or(""),
                            v["degradation_percent"].as_f64().unwrap_or(0.)
                        ));
                    }
                }
            }
            if category.id == "glibc" {
                out.push_str(&format!(
                    "glibc : {}\n",
                    s.glibc_raw["glibc_version"]
                        .as_str()
                        .unwrap_or("Indisponible")
                ));
                if let Some(providers) = s.glibc_raw["providers"].as_array() {
                    let paths: BTreeSet<_> = providers
                        .iter()
                        .filter_map(|p| p["path"].as_str())
                        .collect();
                    for path in paths {
                        out.push_str(&format!("Bibliothèque chargée : {path}\n"));
                    }
                }
            }
            if category.id == "applications" {
                if let Some(libs) = category.raw["libraries"].as_array() {
                    for lib in libs {
                        out.push_str(&format!(
                            "{} : {} · version {}\n",
                            lib["library"].as_str().unwrap_or("Bibliothèque"),
                            lib["state"].as_str().unwrap_or("inconnue"),
                            lib["version"].as_str().unwrap_or("Indisponible")
                        ));
                        if let Some(error) = lib["error"].as_str() {
                            out.push_str(&format!("  {error}\n"));
                        }
                        if let Some(providers) = lib["providers"].as_array() {
                            let paths: BTreeSet<_> = providers
                                .iter()
                                .filter_map(|p| p["path"].as_str())
                                .collect();
                            for path in paths {
                                out.push_str(&format!("  Chargée depuis : {path}\n"));
                            }
                        }
                    }
                }
            }
        }
    } else {
        out.push_str("\nAncien parcours partiel CPU/cache/glibc. GPU, RAM native et charge combinée non exécutés dans ce protocole.\n");
        for m in &s.metrics {
            out.push_str(&format!("{} : {:.3} {}\n", m.label, m.mean(), m.unit));
        }
    }
    out.push_str("\nLes échantillons bruts, paramètres, empreintes et mesures disponibles sont conservés dans result.json. Aucun résultat absent n’est remplacé par zéro.\n");
    out
}
pub const SCORE_VERSION: &str = "aurelia-system-relative-v2";

#[derive(Debug)]
pub struct Scores {
    pub native: Option<f64>,
    pub application: Option<f64>,
    pub synthesis: Option<f64>,
    pub categories: BTreeMap<String, Option<f64>>,
    pub warnings: Vec<String>,
    pub uncertain: BTreeSet<String>,
    pub contributions: BTreeMap<String, f64>,
}
fn geo(values: &[f64]) -> Option<f64> {
    if values.is_empty() || values.iter().any(|v| !v.is_finite() || *v <= 0.) {
        None
    } else {
        Some((values.iter().map(|v| v.ln()).sum::<f64>() / values.len() as f64).exp())
    }
}
fn metric_key(m: &Metric, across_hardware: bool) -> String {
    if across_hardware && m.id.starts_with("cpu/multi_sustained/") {
        "cpu/multi_sustained/available".into()
    } else {
        m.id.clone()
    }
}
fn work(m: &Metric, across_hardware: bool) -> String {
    if !across_hardware {
        return m.work_description.clone();
    }
    if (m.id.starts_with("cpu/multi_sustained/") || m.id == "combined/cpu")
        && m.work_description.starts_with("canonical64/")
    {
        let mut parts = m.work_description.split('/').collect::<Vec<_>>();
        if parts.len() == 6 && parts[3].parse::<usize>().is_ok() {
            parts[3] = "available";
            return parts.join("/");
        }
    }
    if m.id == "gpu/raster" || m.id == "combined/raster" {
        return m.work_description.rsplit_once("/device").map_or_else(
            || m.work_description.clone(),
            |(parameters, _)| parameters.to_owned(),
        );
    }
    m.work_description.clone()
}
fn rate_index(a: &Metric, b: &Metric, across_hardware: bool) -> Result<f64, String> {
    if metric_key(a, across_hardware) != metric_key(b, across_hardware)
        || a.unit != b.unit
        || work(a, across_hardware) != work(b, across_hardware)
    {
        return Err(format!("Paramètres incompatibles : {}", a.id));
    }
    let ratio = 100. * (b.mean() / a.mean());
    if !ratio.is_finite() || ratio <= 0. {
        return Err("Indice hors limites".into());
    }
    Ok(ratio)
}
pub fn compare(a: &Session, b: &Session) -> Result<Scores, String> {
    compare_with_hardware(a, b, false)
}
/// Explicit hardware comparison. Workload parameters still match; only device identity
/// and the protocol's adaptive multithread worker count may differ.
pub fn compare_with_hardware(
    a: &Session,
    b: &Session,
    across_hardware: bool,
) -> Result<Scores, String> {
    a.validate()?;
    b.validate()?;
    if a.protocol != PROTOCOL || b.protocol != PROTOCOL {
        return Err("Ne pas comparer le protocole complet à un ancien parcours partiel".into());
    }
    let aa = &a.fingerprint;
    let bb = &b.fingerprint;
    if aa.engine_sha256 != bb.engine_sha256 || aa.helper_sha256 != bb.helper_sha256 {
        return Err(
            "Utilisez exactement la même AppImage et les mêmes binaires sur les deux distributions"
                .into(),
        );
    }
    if !across_hardware {
        if aa.cpu_model.is_none()
            || aa.cpu_model != bb.cpu_model
            || aa.architecture != bb.architecture
            || aa.logical_cpus != bb.logical_cpus
            || aa.physical_cores != bb.physical_cores
            || aa.pci_gpus != bb.pci_gpus
        {
            return Err("Signatures matérielles différentes ou insuffisantes".into());
        }
        match (aa.ram_kib, bb.ram_kib) {
            (Some(x), Some(y)) if x > 0 && x.abs_diff(y) as f64 / x.max(y) as f64 <= 0.01 => (),
            _ => return Err("Capacité RAM différente ou inconnue".into()),
        }
    }
    let mut indices: BTreeMap<String, Option<f64>> = BTreeMap::new();
    let mut warnings = Vec::new();
    if [a, b].iter().any(|s| {
        s.system
            .as_ref()
            .is_some_and(|body| body.parameters["driver_controls_changed"] == true)
    }) {
        warnings.push("Un relevé signale des réglages du pilote différents avant/après mesure ; interpréter les écarts avec cette réserve, sans attribution causale automatique.".into());
    }

    if across_hardware {
        warnings.push("Comparaison entre configurations : matériel, capacité RAM, placement et pilotes peuvent différer. Le multithread utilise les workers disponibles selon le protocole. Aucune cause n’est attribuée automatiquement.".into());
    }
    let mut uncertain = BTreeSet::new();
    for id in IDS {
        let ca = a
            .system
            .as_ref()
            .unwrap()
            .categories
            .iter()
            .find(|c| c.id == id)
            .unwrap();
        let cb = b
            .system
            .as_ref()
            .unwrap()
            .categories
            .iter()
            .find(|c| c.id == id)
            .unwrap();
        if ca.state != State::Executed || cb.state != State::Executed {
            indices.insert(id.into(), None);
            warnings.push(format!(
                "{} : comparaison indisponible (A {}, B {})",
                label(id),
                ca.state.label(),
                cb.state.label()
            ));
            continue;
        }
        let left: BTreeMap<_, _> = ca
            .metrics
            .iter()
            .map(|m| (metric_key(m, across_hardware), m))
            .collect();
        let right: BTreeMap<_, _> = cb
            .metrics
            .iter()
            .map(|m| (metric_key(m, across_hardware), m))
            .collect();
        if left.keys().ne(right.keys()) {
            indices.insert(id.into(), None);
            warnings.push(format!(
                "{} : ensembles de sous-tests différents",
                label(id)
            ));
            continue;
        }
        let mut values = Vec::new();
        let mut libraries: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        let mut compatible = true;
        for (key, ma) in left {
            let mb = right[&key];
            match rate_index(ma, mb, across_hardware) {
                Ok(value) => {
                    values.push(value);
                    if id == "applications" {
                        libraries
                            .entry(key.split('/').nth(1).unwrap_or("inconnu").to_owned())
                            .or_default()
                            .push(value);
                    }
                    if ma.cv() > 0.05
                        || mb.cv() > 0.05
                        || !ma.confirmation_reasons.is_empty()
                        || !mb.confirmation_reasons.is_empty()
                        || (mb.mean() / ma.mean()).ln().abs() <= ma.cv() + mb.cv()
                    {
                        uncertain.insert(id.to_owned());
                        warnings.push(format!("{} : écart à confirmer", ma.label));
                    }
                }
                Err(reason) => {
                    warnings.push(reason);
                    compatible = false;
                }
            }
        }
        let index = if !compatible {
            None
        } else if id == "applications" {
            geo(&libraries
                .values()
                .filter_map(|v| geo(v))
                .collect::<Vec<_>>())
        } else {
            geo(&values)
        };
        indices.insert(id.into(), index);
    }
    let native = IDS[..5]
        .iter()
        .map(|id| indices[*id])
        .collect::<Option<Vec<_>>>()
        .and_then(|v| geo(&v));
    let application = IDS[5..]
        .iter()
        .map(|id| indices[*id])
        .collect::<Option<Vec<_>>>()
        .and_then(|v| geo(&v));
    let synthesis = match (native, application) {
        (Some(n), Some(p)) => geo(&[n, p]),
        _ => None,
    };
    let mut contributions = BTreeMap::new();
    if let Some(total) = synthesis {
        let log = (total / 100.).ln();
        let factor = if log.abs() < 1e-12 {
            100.
        } else {
            (total - 100.) / log
        };
        for (id, index) in &indices {
            if let Some(index) = index {
                let weight = if IDS[..5].contains(&id.as_str()) {
                    0.1
                } else {
                    0.25
                };
                contributions.insert(id.clone(), weight * (index / 100.).ln() * factor);
            }
        }
    }
    Ok(Scores {
        native,
        application,
        synthesis,
        categories: indices,
        warnings,
        uncertain,
        contributions,
    })
}
pub fn compare_text(a: &Session, b: &Session) -> Result<String, String> {
    compare_text_with_hardware(a, b, false)
}
pub fn compare_text_with_hardware(
    a: &Session,
    b: &Session,
    across_hardware: bool,
) -> Result<String, String> {
    if a.system.is_none() && b.system.is_none() {
        return Ok(crate::comparison::compare(a, b)?.text());
    }
    let scores = compare_with_hardware(a, b, across_hardware)?;
    let display = |v: Option<f64>| {
        v.map(|x| format!("{x:.2}")).unwrap_or_else(|| {
            "Non calculé : couverture obligatoire incomplète ou incompatible".into()
        })
    };
    let mut out=format!("{} (A = 100) / {} (B)\nIndice natif complet : {}\nIndice applicatif : {}\nSynthèse complète : {}\n",a.fingerprint.distribution,b.fingerprint.distribution,display(scores.native),display(scores.application),display(scores.synthesis));
    out.push_str(&format!("Version des indices : {SCORE_VERSION}\n"));
    for (id, index) in &scores.categories {
        out.push_str(&format!("{} : {}", label(id), display(*index)));
        if let Some(v) = index {
            if (*v - 100.).abs() < 1e-9 {
                out.push_str(" · Égalité");
            } else {
                let (winner, percent) = if *v > 100. {
                    (&b.fingerprint.distribution, *v - 100.)
                } else {
                    (&a.fingerprint.distribution, (100. / v - 1.) * 100.)
                };
                out.push_str(&format!(" · {winner} en tête de {percent:.2} %"));
            }
        }
        if scores.uncertain.contains(id) {
            out.push_str(" · Écart à confirmer");
        }
        out.push('\n');
    }
    if !scores.contributions.is_empty() {
        out.push_str(
            "\nContribution à l’écart de synthèse (points d’indice, décomposition logarithmique)\n",
        );
        for (id, points) in &scores.contributions {
            out.push_str(&format!("{} : {points:+.3} points\n", label(id)));
        }
    }
    out.push_str("\nFAITS MESURÉS — écarts B − A\n");
    for ma in a
        .system
        .as_ref()
        .unwrap()
        .categories
        .iter()
        .flat_map(|c| &c.metrics)
    {
        if let Some(mb) = b
            .system
            .as_ref()
            .unwrap()
            .categories
            .iter()
            .flat_map(|c| &c.metrics)
            .find(|mb| metric_key(ma, across_hardware) == metric_key(mb, across_hardware))
        {
            if rate_index(ma, mb, across_hardware).is_ok() {
                out.push_str(&format!(
                    "{} : A {:.3}, B {:.3} {} ; écart {:+.3} {} ({:+.2} %)\n",
                    ma.label,
                    ma.mean(),
                    mb.mean(),
                    ma.unit,
                    mb.mean() - ma.mean(),
                    ma.unit,
                    100. * (mb.mean() / ma.mean() - 1.)
                ));
            }
        }
    }
    out.push_str("\nRÉGLAGES OBSERVÉS AVANT MESURE — A\n");
    out.push_str(&driver_controls_text(
        &a.system.as_ref().unwrap().parameters["driver_controls_before"],
    ));
    out.push_str("\nRÉGLAGES OBSERVÉS AVANT MESURE — B\n");
    out.push_str(&driver_controls_text(
        &b.system.as_ref().unwrap().parameters["driver_controls_before"],
    ));
    out.push_str("\nHYPOTHÈSES — aucune attribution causale automatique. Les températures et fréquences sont des observations ; elles ne démontrent pas seules une limitation thermique.\n");
    out.push_str(&format!(
        "\nNoyau : {} → {}\nglibc : {} → {}\n",
        a.fingerprint.kernel.as_deref().unwrap_or("Indisponible"),
        b.fingerprint.kernel.as_deref().unwrap_or("Indisponible"),
        a.glibc_raw["glibc_version"]
            .as_str()
            .unwrap_or("Indisponible"),
        b.glibc_raw["glibc_version"]
            .as_str()
            .unwrap_or("Indisponible")
    ));
    out.push_str("Les changements de version ne prouvent pas la cause d’un gain.\n");
    if !scores.warnings.is_empty() {
        out.push_str("\nRésultats à confirmer / limites\n");
        out.push_str(&scores.warnings.join("\n"));
    }
    Ok(out)
}

#[cfg(test)]
mod controls_tests {
    use super::*;
    #[test]
    fn settings_report_preserves_units_and_never_infers_pmfw_mode() {
        let snapshot = serde_json::json!({"devices":[{"identity":{"pci":"0000:09:00.0"},"performance":"manual","scalars":[{"label":"PWM minimal","current":25,"unit":"%"}],"workloads":[{"name":"COMPUTE","index":5,"active":true}],"clocks":[{"domain":"gpu","minimum":500,"maximum":2500}],"curve":{"points":[{"temperature":60,"pwm_percent":50}]}}]});
        let text = driver_controls_text(&snapshot);
        for expected in [
            "PWM minimal : 25 %",
            "COMPUTE",
            "min 500 / max 2500 MHz",
            "60 °C → 50 %",
            "mode interne non déduit",
        ] {
            assert!(text.contains(expected), "{expected}");
        }
        assert!(driver_controls_text(&serde_json::Value::Null).contains("indisponible"));
    }
}
