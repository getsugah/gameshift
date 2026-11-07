//! Optional read-only enrichment, always run off the GTK main thread.
use gameshift_bench::{GpuCapabilities, ValidationMode};
use gameshift_core::{Reading, Snapshot};
use std::path::PathBuf;

/// A pairing is accepted only if both PCI and Vulkan identities are unambiguous.
fn unique_gpu(
    vendor: u32,
    device: u32,
    pci_matches: usize,
    gpus: &[GpuCapabilities],
) -> Option<&GpuCapabilities> {
    let candidates = gpus
        .iter()
        .filter(|g| {
            g.vendor_id == vendor && g.device_id == device && !g.kind.to_lowercase().contains("cpu")
        })
        .collect::<Vec<_>>();
    (pci_matches == 1 && candidates.len() == 1).then(|| candidates[0])
}
pub fn enrich(snapshot: &mut Snapshot) {
    let report = match gameshift_bench::probe(ValidationMode::Disabled) {
        Ok(report) => report,
        Err(error) => {
            if let Some(system) = snapshot.sections.iter_mut().find(|s| s.title == "Système") {
                system.readings.push(Reading::unavailable(
                    "Chargeur Vulkan",
                    "Vulkan",
                    error.to_string(),
                ));
            }
            return;
        }
    };
    if let Some(system) = snapshot.sections.iter_mut().find(|s| s.title == "Système") {
        system.readings.push(Reading::available(
            "Chargeur Vulkan",
            "vkEnumerateInstanceVersion",
            report.loader_api_version,
        ));
    }
    let Some(section) = snapshot
        .sections
        .iter_mut()
        .find(|s| s.title == "Carte graphique")
    else {
        return;
    };
    section.readings.retain(|r| {
        !matches!(
            r.label.as_str(),
            "Version Mesa" | "Pilote et extensions Vulkan"
        )
    });
    let ids = section
        .readings
        .iter()
        .filter(|r| r.label.ends_with(" — Périphérique PCI"))
        .filter_map(|r| {
            let id = r.label.split_once(" — ")?.0.to_owned();
            let device =
                u32::from_str_radix(r.value.as_deref()?.trim_start_matches("0x"), 16).ok()?;
            let vendor = section
                .readings
                .iter()
                .find(|r| r.label == format!("{id} — Constructeur PCI"))?
                .value
                .as_deref()?;
            Some((
                id,
                u32::from_str_radix(vendor.trim_start_matches("0x"), 16).ok()?,
                device,
            ))
        })
        .collect::<Vec<_>>();
    for (id, vendor, device) in &ids {
        let matched = unique_gpu(
            *vendor,
            *device,
            ids.iter()
                .filter(|(_, v, d)| v == vendor && d == device)
                .count(),
            &report.gpus,
        );
        let fields = match matched {
            Some(gpu) => vec![
                ("Pilote Vulkan", gpu.driver_name.clone()),
                ("Version pilote Vulkan", gpu.driver_info.clone()),
                ("API Vulkan", Some(gpu.api_version.clone())),
                (
                    "Mesa",
                    gpu.driver_info.as_ref().and_then(|info| {
                        info.split_once("Mesa ")
                            .and_then(|(_, tail)| tail.split_whitespace().next())
                            .map(str::to_owned)
                    }),
                ),
                (
                    "Timestamps Vulkan",
                    Some(
                        if gpu.graphics_timestamps() {
                            "Disponibles sur file graphique"
                        } else {
                            "Non pris en charge sur file graphique"
                        }
                        .into(),
                    ),
                ),
            ],
            None => vec![
                ("Pilote Vulkan", None),
                ("Version pilote Vulkan", None),
                ("API Vulkan", None),
                ("Mesa", None),
                ("Timestamps Vulkan", None),
            ],
        };
        for (field, value) in fields {
            let label = format!("{id} — {field}");
            section.readings.push(match value {
                Some(value) => Reading::available(
                    &label,
                    "Vulkan : identité constructeur/périphérique unique",
                    value,
                ),
                None => Reading::unavailable(
                    &label,
                    "Vulkan",
                    if matched.is_some() {
                        "Information non fournie par cet ICD ; Mesa n'est pas supposé"
                    } else {
                        "Association PCI/Vulkan absente ou ambiguë ; aucune version attribuée"
                    },
                ),
            });
        }
    }
}
#[derive(Debug, Clone)]
pub struct LastReport {
    pub path: PathBuf,
    pub description: String,
}
/// Read a bounded set of completed exports; there is no database or invented score.
pub fn latest_report() -> Option<LastReport> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    latest_in(&base.join("gameshift/runs"))
}
fn latest_in(directory: &std::path::Path) -> Option<LastReport> {
    let mut candidates = std::fs::read_dir(directory)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            (e.path().extension()?.to_str()? == "json"
                && meta.is_file()
                && meta.len() <= 32 * 1024 * 1024)
                .then(|| (meta.modified().ok(), e.path()))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    let mut reports = Vec::new();
    for (_, path) in candidates.into_iter().take(32) {
        let data = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
        let Some(data) = data else {
            continue;
        };
        let Some(scene) = data["scene_version"].as_str() else {
            continue;
        };
        let Some(repetitions) = data["repetitions"].as_array() else {
            continue;
        };
        if repetitions.is_empty() {
            continue;
        }
        // Compute from recorded samples, using the engine's existing statistics unchanged.
        let mut frames = 0usize;
        let mut duration = 0.;
        let mut valid = true;
        for repetition in repetitions {
            let samples = serde_json::from_value::<Vec<gameshift_bench::FrameSample>>(
                repetition["samples"].clone(),
            );
            match samples
                .ok()
                .and_then(|s| gameshift_bench::summarize(&s).ok())
            {
                Some(stats) => {
                    frames += stats.frames;
                    duration += stats.measured_ms;
                }
                None => {
                    valid = false;
                    break;
                }
            }
        }
        if !valid || frames == 0 || !duration.is_finite() || duration <= 0. {
            continue;
        }
        let Some(completed) = data["completed_unix_seconds"].as_u64() else {
            continue;
        };
        let gpu = data["gpu_name"].as_str().unwrap_or("GPU non renseigné");
        let date = gtk::glib::DateTime::from_unix_local(completed.min(i64::MAX as u64) as i64)
            .ok()
            .and_then(|date| date.format("%d/%m/%Y à %H:%M").ok())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "date indisponible".into());
        let width = data["actual_width"]
            .as_u64()
            .map(|n| n.to_string())
            .unwrap_or_else(|| "Indisponible".into());
        let height = data["actual_height"]
            .as_u64()
            .map(|n| n.to_string())
            .unwrap_or_else(|| "Indisponible".into());
        let software = if data["software_device"].as_bool() == Some(true) {
            " · rendu logiciel"
        } else {
            ""
        };
        let timing = if data["duration"].is_object() {
            format!(
                "Profil {} · préparation {} s / mesure {} s × {} passages",
                match data["duration"]["profile"].as_str() {
                    Some("quick") => "Rapide",
                    Some("standard") => "Standard",
                    Some("thorough") => "Approfondi",
                    Some("custom") => "Personnalisé",
                    _ => "non renseigné",
                },
                data["config"]["warmup_seconds"],
                data["config"]["measure_seconds"],
                data["config"]["repetitions"]
            )
        } else {
            "Profil historique non enregistré".into()
        };
        let description=format!("{:.1} FPS moyens · {scene}\n{gpu}{software} · {width} × {height} · terminé le {date}\n{timing}\nUne durée différente est à signaler dans la comparaison, sans modifier à elle seule la charge de rendu.", frames as f64*1000./duration);
        reports.push((completed, LastReport { path, description }));
    }
    reports
        .into_iter()
        .max_by_key(|(date, _)| *date)
        .map(|(_, report)| report)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn gpu() -> GpuCapabilities {
        GpuCapabilities {
            enumeration_index: 0,
            name: "Fixture GPU".into(),
            kind: "Discrete GPU".into(),
            vendor_id: 0x1002,
            device_id: 0x747e,
            device_uuid: None,
            api_version: "1.3".into(),
            driver_version_raw: 0,
            driver_name: Some("fixture".into()),
            driver_info: None,
            device_local_heap_bytes: 0,
            max_image_dimension_2d: 4096,
            timestamp_period_ns: 1.,
            queues: vec![],
            extensions: vec![],
            swapchain_extension: false,
        }
    }
    #[test]
    fn identical_adapters_or_multiple_icds_are_never_guessed() {
        assert!(unique_gpu(0x1002, 0x747e, 2, &[gpu()]).is_none());
        assert!(unique_gpu(0x1002, 0x747e, 1, &[gpu(), gpu()]).is_none());
        assert!(unique_gpu(0x8086, 0x747e, 1, &[gpu()]).is_none());
        let devices = [gpu()];
        assert!(unique_gpu(0x1002, 0x747e, 1, &devices).is_some());
    }
    #[test]
    fn missing_and_incomplete_history_does_not_become_a_score() {
        let root = std::env::temp_dir().join(format!(
            "gameshift-history-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        assert!(latest_in(&root).is_none());
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("invalid.json"), "{broken").unwrap();
        std::fs::write(
            root.join("empty.json"),
            r#"{"scene_version":"fixture","repetitions":[]}"#,
        )
        .unwrap();
        std::fs::write(root.join("partial.json.partial"), "{}").unwrap();
        assert!(latest_in(&root).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
