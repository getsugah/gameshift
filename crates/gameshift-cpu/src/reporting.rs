//! Human-readable terminology and explanations; no change to measurement or serialization.
use crate::{
    runner::{Case, Config, Report},
    workload::Pattern,
};

pub fn pattern_name(pattern: Pattern) -> &'static str {
    match pattern {
        Pattern::Sequential => "Traitement de données",
        Pattern::Random => "Accès mémoire pseudo-aléatoires",
        Pattern::Dependent => "Accès mémoire dépendants",
        Pattern::Entities => "Simulation d’objets",
        Pattern::Spatial => "Simulation spatiale",
        Pattern::Mixed => "Simulation combinée",
    }
}
/// Explain the existing decision flag; these checks do not decide or change validity.
fn own_reasons(case: &Case, config: &Config) -> Vec<String> {
    let mut reasons = Vec::new();
    if config.repetitions < 3 {
        reasons.push(
            "Le nombre de répétitions est insuffisant : effectuez au moins trois répétitions."
                .into(),
        );
    }
    if case.repetition_cv > 0.05 {
        reasons.push("Les mesures ont trop varié entre les répétitions. Relancez le test dans des conditions plus stables.".into());
    }
    if case.repetitions.iter().any(|samples| {
        let mean_ms = samples
            .iter()
            .map(|s| s.critical_wall_ns as f64 / 1e6)
            .sum::<f64>()
            / samples.len() as f64;
        mean_ms
            * (case
                .calibration
                .as_ref()
                .map_or(config.measured_steps, |c| c.measured_steps) as f64)
            < 100.
    }) {
        reasons.push("Le test était trop court : au moins une répétition a duré moins de 100 ms de mesure. Augmentez le nombre d’étapes de simulation mesurées.".into());
    }
    reasons
}
pub fn confirmation_reasons(case: &Case, report: &Report) -> Vec<String> {
    if !case.inconclusive {
        return Vec::new();
    }
    let mut reasons = own_reasons(case, &report.config);
    if case.workers != 1 {
        if let Some(baseline) = report
            .cases
            .iter()
            .find(|b| b.workers == 1 && b.size_mib == case.size_mib && b.inconclusive)
        {
            for reason in own_reasons(baseline, &report.config) {
                reasons.push(format!("Référence à un thread CPU : {reason}"));
            }
        }
    }
    if reasons.is_empty() {
        reasons.push("La référence ou les mesures ne permettent pas encore de conclure. Recommencez la comparaison ; ce statut ne signale pas une panne matérielle.".into());
    }
    reasons
}
fn readable_warning(warning: &str) -> &str {
    match warning {
        "CV > 5 %, moins de trois répétitions ou une répétition mesurée < 100 ms : résultat non concluant ; aucun test statistique de gain implémenté." => "Résultat à confirmer si la variation dépasse 5 %, s’il manque des répétitions ou si une répétition dure moins de 100 ms. Un résultat stable ne démontre pas, à lui seul, le bénéfice d’une optimisation.",
        "Empreinte des buffers comptabilisée ; allocator, Rayon et échantillons conservés ajoutent de la mémoire hors de cette empreinte." => "La taille des données comptabilise les zones de travail. La gestion de la mémoire, les threads CPU et les mesures enregistrées utilisent aussi de la mémoire.",
        "Compteurs cache génériques : ne représentent pas des événements L3 spécifiques. Instrumentation incluse dans le temps critique." => "Les compteurs de cache génériques ne mesurent pas spécifiquement le cache L3. Le coût de la collecte des mesures est inclus dans le temps observé.",
        _ => warning,
    }
}
/// UI explanation only; backend errors and identifiers are preserved.
pub fn error_message(error: &str) -> String {
    match error {
        "Configuration hors limites (workers autorisés, 1–256 MiB, 1–1000 pas, 1–20 répétitions)"=>"Paramètres hors limites : choisissez un nombre de threads CPU autorisé, 1–256 Mio de données, 1–1000 étapes mesurées et 1–20 répétitions.",
        "Échantillons bruts trop volumineux (budget prudent 512 Mio) : réduire les pas, tailles ou paliers"=>"Les résultats détaillés dépasseraient le budget mémoire de 512 Mio. Réduisez le nombre d’étapes mesurées, la taille des données ou le nombre de configurations comparées.",
        "Résultats déterministes divergents entre nombres de workers"=>"Le calcul n’a pas produit les mêmes résultats avec les différents nombres de threads CPU. La comparaison a été arrêtée ; ces mesures ne sont pas comparables.",
        _=>error,
    }.to_owned()
}
pub fn format(report: &Report) -> String {
    let self_report = report;
    let preparation = if let Some(d) = &report.config.duration {
        format!("Profil {} · cible {:.2} s de calcul par répétition · préparation cible {:.2} s. Les nombres d’étapes calibrés figurent par configuration.",d.profile.label(),d.target_ms as f64/1000.,d.preparation_ms as f64/1000.)
    } else {
        format!(
            "Mode historique : préparation {} étapes ; mesure {} étapes par répétition",
            report.config.warmup_steps, report.config.measured_steps
        )
    };
    let mut text=format!("GameShift — Architecture du processeur\nMoteur : {} · protocole {} · schéma {}\nType de test : {} · graine : {} · durée simulée par étape : {} ns\n{}\n{} répétitions · compteurs matériels CPU : {}\n\nTemps moyen par étape de simulation\n",if report.engine_version.is_empty(){"Indisponible"}else{&report.engine_version},report.protocol,report.schema_version,pattern_name(report.config.pattern),report.config.seed,report.config.fixed_step_ns,preparation,report.config.repetitions,if report.config.hardware_counters{"activés"}else{"désactivés"});
    for c in &self_report.cases {
        if let Some(cal) = &c.calibration {
            text.push_str(&format!("Calibration exclue : {} étapes de mesure, {} étapes de préparation par répétition. Sonde {} étapes / {:.2} ms.\n",cal.measured_steps,cal.preparation_steps,cal.probe_steps,cal.probe_compute_ms));
            for (index, samples) in c.repetitions.iter().enumerate() {
                let compute = samples
                    .iter()
                    .map(|s| s.critical_wall_ns as f64 / 1e6)
                    .sum::<f64>();
                text.push_str(&format!("Passage {} : calcul mesuré {:.3} s · durée murale {:.3} s · préparation {:.3} s\n",index+1,compute/1000.,cal.measurement_wall_ms[index]/1000.,cal.preparation_actual_ms[index]/1000.));
            }
        }
        if !c.inconclusive {
            text.push_str("Mesure stable selon les critères existants ; cela ne démontre pas à lui seul un gain d’optimisation.\n");
        }
        let reasons = confirmation_reasons(c, self_report);
        for reason in &reasons {
            text.push_str(&format!("Résultat à confirmer — {reason}\n"));
        }
        text.push_str(&format!("{} Mio cibles · {} thread(s) CPU utilisés · {:.3} ms · débit de calcul : {:.0} objets traités/s · variation entre les mesures : {:.1} %{}{}\nDonnées de travail et structures comptabilisées : {} octets ; gestion mémoire, threads et résultats non compris.\n",
                c.size_mib, c.workers, c.mean_step_ms, c.entities_per_second, c.repetition_cv * 100.,
                c.speedup.map(|s| format!(" · accélération par rapport à 1 thread : ×{s:.2} · efficacité du parallélisme {:.1} %", c.parallel_efficiency.unwrap_or(0.) * 100.)).unwrap_or_default(),
                if c.inconclusive { " · Résultat à confirmer" } else { "" }, c.footprint.persistent_allocated_bytes));
        text.push_str(&format!(
                "Échantillons conservés : {} octets · Mémoire physique du processus après mesure (RSS) : {}\n",
                c.retained_sample_allocation_bytes,
                c.process_rss_after_bytes
                    .map(|n| format!("{n} octets"))
                    .unwrap_or_else(|| "Indisponible".into())
            ));
        if let Some(load) = c.environment.system_busy_outside_process_percent {
            text.push_str(&format!("Activité CPU système hors processus (estimation sur le cas complet) : {load:.1} %{}\n",
                    if load > 10. { " · charge concurrente possible" } else { "" }));
        }
        if c.environment.after.temperatures.is_empty() {
            text.push_str("Température CPU : Indisponible\n");
        }
        for temperature in &c.environment.after.temperatures {
            text.push_str(&format!(
                "Température en fin de cas · {} : {:.1} °C\n",
                temperature.source, temperature.celsius
            ));
        }
        if self_report.config.hardware_counters {
            let mut unavailable = std::collections::BTreeSet::new();
            for event in c
                .repetitions
                .iter()
                .flatten()
                .flat_map(|s| &s.jobs)
                .flat_map(|j| &j.counters)
            {
                if let Some(reason) = &event.unavailable_reason {
                    unavailable.insert(format!("{} : {reason}", event.event));
                }
            }
            for reason in unavailable {
                text.push_str(&format!("Compteur indisponible — {reason}\n"));
            }
        }
    }
    text.push_str("La graine est le nombre qui rend la génération des données reproductible. Mio signifie mébioctets ; RSS désigne la mémoire physique résidente du processus.\n");
    text.push_str("Ce test ne désactive jamais le 3D V-Cache. Les durées de calcul des threads ne s’additionnent pas pour obtenir le temps total observé.\n");
    text.push_str(
        &self_report
            .warnings
            .iter()
            .map(|w| readable_warning(w))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Report {
        // Real engine creates valid structures; timings below are explicit test fixtures, not scores.
        let mut report = crate::runner::run(Config {
            workers: vec![1],
            sizes_mib: vec![1],
            warmup_steps: 0,
            measured_steps: 2,
            repetitions: 3,
            ..Config::default()
        })
        .unwrap();
        let case = &mut report.cases[0];
        for samples in &mut case.repetitions {
            for sample in samples {
                sample.critical_wall_ns = 60_000_000;
            }
        }
        case.repetition_cv = 0.;
        case.inconclusive = false;
        report
    }
    #[test]
    fn explanations_follow_actual_causes_without_changing_flags_or_json() {
        let mut report = fixture();
        assert!(confirmation_reasons(&report.cases[0], &report).is_empty());
        report.cases[0].inconclusive = true;
        report.cases[0].repetition_cv = 0.06;
        let before = serde_json::to_value(&report).unwrap();
        let reasons = confirmation_reasons(&report.cases[0], &report);
        assert_eq!(reasons.len(), 1);
        assert!(reasons[0].contains("trop varié"));
        assert!(!format(&report).contains("NON CONCLUANT"));
        assert!(format(&report).contains("Résultat à confirmer"));
        assert_eq!(serde_json::to_value(&report).unwrap(), before);
        report.cases[0].repetition_cv = 0.;
        report.cases[0].repetitions[0][0].critical_wall_ns = 1;
        assert!(confirmation_reasons(&report.cases[0], &report)[0].contains("trop court"));
        report.config.repetitions = 2;
        assert!(confirmation_reasons(&report.cases[0], &report)
            .iter()
            .any(|r| r.contains("nombre de répétitions")));
    }
    #[test]
    fn uncertainty_of_single_thread_reference_is_explained() {
        let mut report = fixture();
        report.cases[0].inconclusive = true;
        report.cases[0].repetition_cv = 0.08;
        let mut parallel = crate::runner::run(report.config.clone())
            .unwrap()
            .cases
            .remove(0);
        parallel.workers = 4;
        parallel.repetition_cv = 0.;
        parallel.inconclusive = true;
        for samples in &mut parallel.repetitions {
            for sample in samples {
                sample.critical_wall_ns = 60_000_000;
            }
        }
        report.cases.push(parallel);
        let reasons = confirmation_reasons(&report.cases[1], &report);
        assert_eq!(reasons.len(), 1);
        assert!(reasons[0].starts_with("Référence à un thread CPU"));
    }
    #[test]
    fn existing_boundary_values_are_not_described_as_excessive() {
        let mut report = fixture();
        report.cases[0].repetition_cv = 0.05;
        for samples in &mut report.cases[0].repetitions {
            for sample in samples {
                sample.critical_wall_ns = 50_000_000;
            }
        }
        assert!(own_reasons(&report.cases[0], &report.config).is_empty());
    }
}
