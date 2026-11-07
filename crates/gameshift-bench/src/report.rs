//! Readable UTF-8 reports, including legacy JSON with explicitly missing values.
use serde_json::Value;
use std::fmt::Write;

/// Localize explanations for display only; stored warnings remain unchanged.
pub fn display_warning(warning: &str) -> String {
    match warning {
        "Renderer initial avec une frame en vol : débit sérialisé CPU/GPU, pas une estimation des performances de tous les jeux." => "Moteur de rendu avec une seule image en cours de traitement : le CPU et le GPU travaillent de façon sérialisée. Le résultat ne prédit pas les performances de tous les jeux.",
        "Couches de validation indisponibles ou désactivées." => "Vérifications Vulkan indisponibles ou désactivées.",
        "Validation de développement activée : son coût CPU impose de comparer des tests avec le même état de validation." => "Vérifications Vulkan activées : elles ajoutent du travail au CPU. Comparez des tests utilisant le même état des vérifications.",
        "Timestamps GPU absents ou incomplets ; les durées manquantes ne sont pas estimées." => "Horodatages GPU absents ou incomplets : les temps de calcul GPU manquants restent indisponibles, sans estimation.",
        "Moins de 1 000 frames dans une répétition : percentiles extrêmes et 0,1 % low peu représentatifs." => "Résultat à confirmer pour les images les plus lentes : une répétition contient moins de 1 000 images. Les percentiles extrêmes et le débit 0,1 % low sont peu représentatifs ; répétez le test et interprétez-les avec prudence.",
        "Variabilité supérieure à 5 % entre répétitions ; résultats à reproduire avant toute conclusion." => "Résultat à confirmer : les mesures ont trop varié entre les répétitions (plus de 5 %). Relancez le test dans des conditions plus stables ; cela ne signale pas une panne matérielle.",
        _ => return if let Some(rest)=warning.strip_prefix("MSAA ") { format!("Anticrénelage (MSAA) {rest}") } else {warning.to_owned()},
    }.to_owned()
}

fn number(value: &Value, key: &str) -> Option<f64> {
    value.get(key)?.as_f64().filter(|n| n.is_finite())
}
fn decimal(value: Option<f64>, digits: usize, unit: &str) -> String {
    value
        .map(|n| format!("{n:.digits$}{}", unit).replace('.', ","))
        .unwrap_or_else(|| "Indisponible".into())
}
fn string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("Indisponible")
        .into()
}
fn boolean(value: &Value, key: &str) -> &'static str {
    match value.get(key).and_then(Value::as_bool) {
        Some(true) => "Oui",
        Some(false) => "Non",
        None => "Indisponible",
    }
}
fn dimensions(value: &Value, width: &str, height: &str) -> String {
    match (
        value.get(width).and_then(Value::as_u64),
        value.get(height).and_then(Value::as_u64),
    ) {
        (Some(w), Some(h)) => format!("{w} × {h}"),
        _ => "Indisponible".into(),
    }
}
fn line(output: &mut String, label: &str, value: impl std::fmt::Display) {
    let _ = writeln!(output, "{label:<28} {value}");
}

/// Format recorded values only. `date_label` is a localized representation of the
/// recorded timestamp, supplied by the UI without introducing a GTK dependency here.
/// Missing legacy metadata stays unavailable; the source JSON is never changed.
pub fn format_text_report(report: &Value, date_label: &str) -> Result<String, String> {
    let runs = report
        .get("repetitions")
        .and_then(Value::as_array)
        .filter(|r| !r.is_empty())
        .ok_or("Export sans répétitions de benchmark")?;
    let config = &report["config"];
    let mut out = String::from(
        "GAMESHIFT — RAPPORT DE TEST DE PERFORMANCES\n================================\n\n",
    );
    line(&mut out, "Date du test :", date_label);
    line(
        &mut out,
        "Version de scène :",
        string(report, "scene_version"),
    );
    line(
        &mut out,
        "Schéma de l’export JSON :",
        report["schema_version"]
            .as_u64()
            .map(|n| n.to_string())
            .unwrap_or_else(|| "Indisponible".into()),
    );
    out.push_str("\nCONFIGURATION\n-------------\n");
    line(&mut out, "Carte graphique :", string(report, "gpu_name"));
    line(&mut out, "Pilote Vulkan :", string(report, "driver_name"));
    line(
        &mut out,
        "Version du pilote :",
        string(report, "driver_info"),
    );
    line(
        &mut out,
        "Résolution de calcul :",
        dimensions(report, "actual_width", "actual_height"),
    );
    line(
        &mut out,
        "Fenêtre de présentation :",
        dimensions(report, "presentation_width", "presentation_height"),
    );
    line(&mut out, "Présentation :", string(report, "present_mode"));
    line(
        &mut out,
        "Anticrénelage (MSAA) demandé :",
        decimal(number(config, "requested_msaa"), 0, "×"),
    );
    line(
        &mut out,
        "Anticrénelage (MSAA) utilisé :",
        decimal(number(report, "msaa_samples"), 0, "×"),
    );
    match config["grid_side"].as_u64() {
        Some(n) => line(&mut out, "Grille de bâtiments :", format!("{n} × {n}")),
        None => line(&mut out, "Grille de bâtiments :", "Indisponible"),
    }
    line(
        &mut out,
        "Préparation par répétition :",
        decimal(number(config, "warmup_seconds"), 0, " s"),
    );
    line(
        &mut out,
        "Mesure par répétition :",
        decimal(number(config, "measure_seconds"), 0, " s"),
    );
    line(&mut out, "Répétitions enregistrées :", runs.len());
    line(
        &mut out,
        "Rendu logiciel :",
        boolean(report, "software_device"),
    );
    line(&mut out, "Rendu hors écran :", boolean(report, "headless"));
    line(
        &mut out,
        "Vérifications Vulkan activées :",
        boolean(report, "validation_enabled"),
    );
    line(
        &mut out,
        "Erreurs des vérifications Vulkan :",
        decimal(number(report, "validation_errors"), 0, ""),
    );
    if report["duration"].is_object() {
        let timing = &report["duration"];
        let profile = match timing["profile"].as_str() {
            Some("quick") => "Rapide",
            Some("standard") => "Standard",
            Some("thorough") => "Approfondi",
            _ => "Personnalisé / historique",
        };
        line(&mut out, "Profil de durée :", profile);
        line(&mut out, "Moteur :", string(timing, "engine_version"));
        line(
            &mut out,
            "Protocole de durée :",
            string(timing, "protocol_id"),
        );
        line(
            &mut out,
            "FPS minimum (fenêtres ≥ 1 s) :",
            decimal(number(timing, "minimum_window_fps"), 2, " FPS"),
        );
        line(
            &mut out,
            "FPS maximum (fenêtres ≥ 1 s) :",
            decimal(number(timing, "maximum_window_fps"), 2, " FPS"),
        );
        line(
            &mut out,
            "Méthode des extrema :",
            string(timing, "fps_extrema_method"),
        );
        if let Some(passes) = timing["passes"].as_array() {
            for pass in passes {
                line(
                    &mut out,
                    &format!("Passage {} :", pass["repetition"]),
                    format!(
                        "préparation réelle {} ; mesure réelle {}",
                        decimal(number(pass, "preparation_ms"), 1, " ms"),
                        decimal(number(pass, "measurement_wall_ms"), 1, " ms")
                    ),
                );
            }
        }
        let sensors = &timing["gpu_sensors"];
        let last = sensors["samples"].as_array().and_then(|samples| {
            samples
                .iter()
                .rev()
                .find(|s| s["repetition"].as_u64().unwrap_or(0) > 0)
        });
        line(
            &mut out,
            "Température GPU (dernière mesure) :",
            decimal(
                last.and_then(|s| number(s, "temperature_celsius")),
                1,
                " °C",
            ),
        );
        line(
            &mut out,
            "Fréquence GPU (dernière mesure) :",
            decimal(last.and_then(|s| number(s, "core_clock_mhz")), 0, " MHz"),
        );
        out.push_str("Chronologie GPU et capteurs : échantillons dans le JSON. Attente de présentation séparée : Indisponible.\n");
    }
    out.push_str("\nSYNTHÈSE\n--------\n");
    let totals = runs.iter().try_fold((0.0, 0.0), |(frames, ms), run| {
        let n = number(&run["statistics"], "frames")?;
        let duration = number(&run["statistics"], "measured_ms")?;
        let total_frames = frames + n;
        let total_ms = ms + duration;
        (n > 0.0 && duration > 0.0 && total_frames.is_finite() && total_ms.is_finite())
            .then_some((total_frames, total_ms))
    });
    let fps = totals.and_then(|(n, ms)| {
        let fps = n * 1000.0 / ms;
        fps.is_finite().then_some(fps)
    });
    line(
        &mut out,
        "Images/s (FPS), moyenne pondérée :",
        decimal(fps, 2, " FPS"),
    );
    line(
        &mut out,
        "Images calculées au total :",
        decimal(totals.map(|(n, _)| n), 0, ""),
    );
    line(
        &mut out,
        "Variation entre les mesures :",
        decimal(number(report, "repetition_cv_percent"), 2, " %"),
    );
    let utilization = &report["gpu_utilization"];
    line(
        &mut out,
        "Occupation GPU moyenne :",
        decimal(number(utilization, "average_percent"), 1, " %"),
    );
    line(
        &mut out,
        "Occupation GPU maximale :",
        decimal(number(utilization, "maximum_percent"), 0, " %"),
    );
    if utilization["average_percent"].is_null() {
        if let Some(reason) = utilization["unavailable_reason"].as_str() {
            line(&mut out, "Motif d’indisponibilité :", reason);
        }
    }
    if let Some(source) = utilization["source"].as_str() {
        line(&mut out, "Source d’occupation GPU :", source);
    }
    out.push_str("L’occupation GPU est globale : elle inclut les autres applications.\n");
    out.push_str("\nRÉSULTATS PAR RÉPÉTITION\n-----------------------\n");
    for (index, run) in runs.iter().enumerate() {
        let s = &run["statistics"];
        let _ = writeln!(out, "\nRépétition {}", index + 1);
        for (label, key, digits, unit) in [
            ("  FPS moyens :", "average_fps", 2, " FPS"),
            (
                "  Temps moyen par image :",
                "average_frametime_ms",
                3,
                " ms",
            ),
            ("  P95 :", "p95_ms", 3, " ms"),
            ("  P99 :", "p99_ms", 3, " ms"),
            ("  P99,9 :", "p999_ms", 3, " ms"),
            (
                "  Images lentes (1 % low) :",
                "one_percent_low_fps",
                2,
                " FPS",
            ),
            (
                "  Images lentes (0,1 % low) :",
                "point_one_percent_low_fps",
                2,
                " FPS",
            ),
            (
                "  Préparation et soumission CPU :",
                "average_cpu_work_ms",
                3,
                " ms",
            ),
            (
                "  Temps de calcul GPU :",
                "average_gpu_execution_ms",
                3,
                " ms",
            ),
            ("  Images calculées :", "frames", 0, ""),
            ("  Durée réellement mesurée :", "measured_ms", 3, " ms"),
        ] {
            line(&mut out, label, decimal(number(s, key), digits, unit));
        }
        let maximum = run["samples"].as_array().and_then(|samples| {
            samples
                .iter()
                .filter_map(|sample| number(sample, "frame_interval_ms"))
                .reduce(f64::max)
        });
        line(
            &mut out,
            "  Image la plus longue :",
            decimal(maximum, 3, " ms"),
        );
    }
    out.push_str("\nCONDITIONS ET LIMITES\n--------------------\n");
    if let Some(warnings) = report["warnings"].as_array() {
        for warning in warnings.iter().filter_map(Value::as_str) {
            let _ = writeln!(out, "- {}", display_warning(warning));
        }
    }
    out.push_str("\nLe travail CPU correspond à la préparation/enregistrement/soumission ;\nil ne représente pas toute l’activité CPU. Le temps GPU vient des horodatages\nVulkan lorsqu’ils sont disponibles. Le temps par image ne mesure pas le balayage écran.\nLes préparations du test sont exclues des statistiques. Comparer des protocoles, scènes,\nrésolutions et niveaux de qualité identiques, sans charge parallèle pour une référence.\n");
    out.push_str("\nLECTURE DES INDICATEURS\n----------------------\nFPS (images par seconde) : une valeur élevée signifie davantage d’images calculées par seconde pour ce test.\nTemps par image et percentiles (en ms) : plus faibles indiquent des images calculées plus rapidement.\nP99 : 99 % des intervalles mesurés sont inférieurs ou égaux à cette durée\n(méthode du rang supérieur). Les indicateurs 1 % low et 0,1 % low sont l’inverse du temps moyen par image\ndes 1 % / 0,1 % des images les plus lentes, arrondis au nombre d’images supérieur.\nLes valeurs extrêmes reposent sur peu d’échantillons dans un test court.\nLa variation entre les mesures concerne les FPS moyens des répétitions, pas tous les à-coups.\n\nLe JSON conserve les métadonnées et échantillons ; le CSV conserve chaque image calculée.\nLes modes d’affichage Vulkan restent nommés FIFO (synchronisé à l’écran),\nMAILBOX (images remplaçables avant affichage) ou IMMEDIATE (sans attente\nde synchronisation). Les comparer à mode identique.\nCe rapport ne permet pas d’attribuer un défaut visuel à un manque de puissance GPU.\n");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_metadata_is_not_replaced_by_invented_msaa_or_utilization() {
        let fixture = serde_json::json!({"schema_version":1,"scene_version":"test-fixture","gpu_name":"Test fixture, not hardware","repetitions":[{"statistics":{"frames":2,"measured_ms":20.0,"average_fps":100.0}}]});
        let text = format_text_report(&fixture, "Date de test").unwrap();
        assert!(text.contains("100,00 FPS"));
        let msaa = text
            .lines()
            .find(|l| l.starts_with("Anticrénelage (MSAA) utilisé"))
            .unwrap();
        assert!(msaa.contains("Indisponible"));
        let occupancy = text
            .lines()
            .find(|l| l.starts_with("Occupation GPU moyenne"))
            .unwrap();
        assert!(occupancy.contains("Indisponible"));
    }
    #[test]
    fn capability_reports_are_not_formatted_as_performance_results() {
        assert!(format_text_report(&serde_json::json!({"gpus":[]}), "Date").is_err());
    }
}

#[cfg(test)]
mod wording_tests {
    use super::*;
    #[test]
    fn display_explanation_keeps_raw_values_and_unknown_driver_errors() {
        let warning="Variabilité supérieure à 5 % entre répétitions ; résultats à reproduire avant toute conclusion.";
        let display = display_warning(warning);
        assert!(display.contains("Résultat à confirmer"));
        assert!(display.contains("5 %"));
        assert!(display.contains("conditions plus stables"));
        assert_eq!(
            display_warning("MSAA 16× demandé, 8× pris en charge"),
            "Anticrénelage (MSAA) 16× demandé, 8× pris en charge"
        );
        assert_eq!(
            display_warning("VK_ERROR_DEVICE_LOST"),
            "VK_ERROR_DEVICE_LOST"
        );
    }
}
