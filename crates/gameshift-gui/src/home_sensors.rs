//! Optional home temperature selection. Sensor keys never contain unstable hwmon indices.
use adw::prelude::*;
use std::{
    cell::RefCell,
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};
#[derive(Clone, Debug)]
pub struct Sensor {
    pub id: String,
    pub label: String,
    pub source: String,
    pub value: Option<f64>,
}
fn friendly_label(chip: &str, label: &str) -> String {
    let label = label.trim();
    let lower = label.to_ascii_lowercase();
    match chip {
        "amdgpu" => match lower.as_str() {
            "edge" => "Edge".into(),
            "mem" | "memory" => "Mem".into(),
            "junction" | "hotspot" => "Hotspot".into(),
            _ => format!("GPU · {label}"),
        },
        "k10temp" | "coretemp" | "zenpower" | "cpu_thermal" => {
            if let Some(ccd) = lower
                .strip_prefix("tccd")
                .or_else(|| lower.strip_prefix("ccd"))
            {
                format!("CPU CCD{}", ccd.trim())
            } else {
                format!("CPU {}", label.replace("Package id ", "Package ").trim())
            }
        }
        "nvme" => {
            if lower == "composite" {
                "SSD NVMe".into()
            } else {
                format!("SSD · {label}")
            }
        }
        _ => label.to_owned(),
    }
}
fn move_selected(ids: &mut [String], id: &str, offset: isize) -> bool {
    let Some(index) = ids.iter().position(|s| s == id) else {
        return false;
    };
    let Some(target) = index.checked_add_signed(offset).filter(|i| *i < ids.len()) else {
        return false;
    };
    ids.swap(index, target);
    true
}
pub fn discover(root: &Path) -> Vec<Sensor> {
    let mut out = Vec::new();
    let Ok(chips) = fs::read_dir(root.join("sys/class/hwmon")) else {
        return out;
    };
    for chip in chips.flatten().take(256) {
        let path = chip.path();
        let name = fs::read_to_string(path.join("name"))
            .unwrap_or_default()
            .trim()
            .to_owned();
        let Ok(real) = fs::canonicalize(&path) else {
            continue;
        };
        let device = fs::canonicalize(path.join("device"))
            .ok()
            .or_else(|| real.parent()?.parent().map(Path::to_path_buf));
        let Some(device) = device else {
            continue;
        };
        let stable = device
            .strip_prefix(root)
            .unwrap_or(&device)
            .to_string_lossy();
        let Ok(inputs) = fs::read_dir(&path) else {
            continue;
        };
        for input in inputs.flatten().take(512) {
            let file = input.file_name().to_string_lossy().into_owned();
            let Some(channel) = file
                .strip_prefix("temp")
                .and_then(|s| s.strip_suffix("_input"))
                .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            else {
                continue;
            };
            let label = fs::read_to_string(path.join(format!("temp{channel}_label")))
                .unwrap_or_else(|_| format!("Température {channel}"));
            let value = fs::read_to_string(input.path())
                .ok()
                .and_then(|s| s.trim().parse::<f64>().ok())
                .filter(|v| v.is_finite() && (-40_000. ..=150_000.).contains(v))
                .map(|v| v / 1000.);
            out.push(Sensor {
                id: format!("{stable}/{name}/temp{channel}"),
                label: friendly_label(&name, label.trim()),
                source: input.path().display().to_string(),
                value,
            });
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out.dedup_by(|a, b| a.id == b.id);
    // Distinguish identical names without exposing bus addresses.
    let original = out.iter().map(|s| s.label.clone()).collect::<Vec<_>>();
    for (index, sensor) in out.iter_mut().enumerate() {
        if original
            .iter()
            .filter(|name| *name == &sensor.label)
            .count()
            > 1
        {
            let number = original[..=index]
                .iter()
                .filter(|name| *name == &sensor.label)
                .count();
            sensor.label = format!("{} · {number}", sensor.label);
        }
    }
    out
}
#[derive(Default)]
pub struct Sampler {
    cache: gameshift_core::discovery::Cache<Vec<Sensor>>,
}
impl Sampler {
    pub fn sample_at(&mut self, root: &Path, now: std::time::Instant) -> Vec<Sensor> {
        let sensors = self.cache.get(root, now, |root| {
            let sensors = discover(root);
            let mut watched = vec![root.join("sys/class/hwmon")];
            watched.extend(
                sensors
                    .iter()
                    .filter_map(|s| Path::new(&s.source).parent().map(Path::to_path_buf)),
            );
            (sensors, watched)
        });
        let mut failed = false;
        let out = sensors
            .iter()
            .map(|sensor| {
                let mut sensor = sensor.clone();
                sensor.value = fs::read_to_string(&sensor.source)
                    .ok()
                    .and_then(|s| s.trim().parse::<f64>().ok())
                    .filter(|v| v.is_finite() && (-40_000. ..=150_000.).contains(v))
                    .map(|v| v / 1000.);
                failed |= sensor.value.is_none();
                sensor
            })
            .collect();
        self.cache.failed(failed);
        out
    }
}
fn path() -> Result<PathBuf, String> {
    Ok(gameshift_core::preferences::path()?.with_file_name("home-temperatures.json"))
}
fn load(path: &Path) -> Result<Option<Vec<String>>, String> {
    use std::io::Read;
    let bytes = match fs::File::open(path).and_then(|f| {
        let mut bytes = Vec::new();
        f.take(65537).read_to_end(&mut bytes)?;
        Ok(bytes)
    }) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    if bytes.len() > 65536 {
        return Err("Configuration des sondes trop volumineuse".into());
    }
    let json: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if json["schema_version"] != 1 {
        return Err("Version de configuration des sondes non prise en charge".into());
    }
    if json.get("selected").is_some_and(|v| v.is_null()) {
        return Ok(None);
    }
    let list = json["selected"]
        .as_array()
        .ok_or("Liste des sondes invalide")?;
    if list.len() > 128 {
        return Err("Trop de sondes sélectionnées".into());
    }
    let mut ids = Vec::new();
    for id in list {
        ids.push(
            id.as_str()
                .filter(|s| !s.is_empty() && s.len() < 4096)
                .ok_or("Identifiant de sonde invalide")?
                .to_owned(),
        );
    }
    if ids.iter().collect::<BTreeSet<_>>().len() != ids.len() {
        return Err("Sondes dupliquées".into());
    }
    Ok(Some(ids))
}
fn save(path: &Path, selected: &Option<Vec<String>>) -> Result<(), String> {
    use std::io::Write;
    load(path)?; // Preserve invalid or future configuration.
    if selected.as_ref().is_some_and(|ids| {
        ids.len() > 128
            || ids.iter().any(|id| id.is_empty() || id.len() >= 4096)
            || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
    }) {
        return Err("Sélection des sondes invalide (128 maximum)".into());
    }
    let parent = path.parent().ok_or("Chemin invalide")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = parent.join(format!(".home-temperatures-{}.tmp", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| e.to_string())?;
    let result = (|| {
        file.write_all(
            &serde_json::to_vec_pretty(
                &serde_json::json!({"schema_version":1,"selected":selected}),
            )
            .map_err(|e| e.to_string())?,
        )
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
        fs::rename(&temporary, path).map_err(|e| e.to_string())
    })();
    if temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
thread_local! { static SELECTED:RefCell<Option<Vec<String>>>=RefCell::new(path().and_then(|p|load(&p)).unwrap_or_default()); }
pub fn selected() -> Option<Vec<String>> {
    SELECTED.with_borrow(Clone::clone)
}
fn store(selected: Option<Vec<String>>, status: &adw::ActionRow) {
    match path().and_then(|p| save(&p, &selected)) {
        Ok(()) => {
            SELECTED.with_borrow_mut(|s| *s = selected);
            status.set_subtitle("Choix enregistré · appliqué à l’accueil au prochain relevé");
        }
        Err(error) => status.set_subtitle(&format!("Choix non enregistré : {error}")),
    }
}
pub fn preferences() -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Sondes de température de l’accueil");
    group.set_description(Some("Cocher les sondes à afficher, puis utiliser Monter / Descendre pour choisir leur ordre dans le panneau Températures."));
    let status = adw::ActionRow::new();
    status.set_title("Détection des sondes hwmon");
    if let Err(error) = path().and_then(|p| load(&p)) {
        status.set_subtitle(&format!("Configuration conservée : {error}"));
    }
    group.add(&status);
    let automatic = gtk::CheckButton::new();
    automatic.set_active(selected().is_none());
    let automatic_row = adw::ActionRow::new();
    automatic_row.set_title("Sélection automatique");
    automatic_row.add_suffix(&automatic);
    automatic_row.set_activatable_widget(Some(&automatic));
    group.add(&automatic_row);
    let refresh = gtk::Button::with_label("Détecter les sondes");
    status.add_suffix(&refresh);
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("boxed-list");
    group.add(&list);
    let (tx, rx) = mpsc::channel();
    let loading = std::rc::Rc::new(std::cell::Cell::new(false));
    refresh.connect_clicked({
        let loading = loading.clone();
        move |_| {
            if loading.replace(true) {
                return;
            }
            let tx = tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(discover(Path::new("/")));
            });
        }
    });
    automatic.connect_toggled({
        let status = status.clone();
        let list = list.clone();
        move |button| {
            list.set_sensitive(!button.is_active());
            store(
                if button.is_active() {
                    None
                } else {
                    Some(Vec::new())
                },
                &status,
            );
        }
    });
    list.set_sensitive(!automatic.is_active());
    let weak = group.downgrade();
    let refresh_weak = refresh.downgrade();
    let mut cached = Vec::<Sensor>::new();
    let mut previous_selection = selected();
    gtk::glib::timeout_add_local(Duration::from_millis(150), move || {
        if weak.upgrade().is_none() {
            return gtk::glib::ControlFlow::Break;
        }
        let mut changed = false;
        if let Ok(sensors) = rx.try_recv() {
            loading.set(false);
            cached = sensors;
            changed = true;
        }
        let selection = selected();
        if changed || selection != previous_selection {
            previous_selection = selection.clone();
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }
            let ids = selection.unwrap_or_default();
            let present = cached.iter().map(|s| s.id.clone()).collect::<BTreeSet<_>>();
            if changed {
                status.set_subtitle(&format!(
                    "{} sonde(s) détectée(s) · les flèches règlent l’ordre d’affichage",
                    cached.len()
                ));
            }
            let mut sensors = cached.clone();
            sensors.extend(
                ids.iter()
                    .filter(|id| !present.contains(*id))
                    .map(|id| Sensor {
                        id: id.clone(),
                        label: "Sonde indisponible".into(),
                        source: id.clone(),
                        value: None,
                    }),
            );
            sensors.sort_by(|a, b| {
                ids.iter()
                    .position(|id| id == &a.id)
                    .unwrap_or(usize::MAX)
                    .cmp(&ids.iter().position(|id| id == &b.id).unwrap_or(usize::MAX))
                    .then_with(|| a.label.cmp(&b.label))
            });
            for sensor in sensors {
                let row = adw::ActionRow::new();
                row.set_title(&sensor.label);
                row.set_subtitle(
                    &sensor
                        .value
                        .map(|v| format!("{v:.1} °C"))
                        .unwrap_or_else(|| "Non disponible".into()),
                );
                row.set_tooltip_text(Some(&sensor.source));
                let check = gtk::CheckButton::new();
                check.set_active(ids.contains(&sensor.id));
                row.add_suffix(&check);
                row.set_activatable_widget(Some(&check));
                for (symbol, title, offset) in
                    [("↑", "Monter", -1isize), ("↓", "Descendre", 1isize)]
                {
                    let button = gtk::Button::with_label(symbol);
                    button.set_tooltip_text(Some(title));
                    button.set_valign(gtk::Align::Center);
                    let position = ids.iter().position(|id| id == &sensor.id);
                    button.set_visible(position.is_some());
                    button.set_sensitive(position.is_some_and(|i| {
                        i.checked_add_signed(offset).is_some_and(|j| j < ids.len())
                    }));
                    button.connect_clicked({
                        let id = sensor.id.clone();
                        let status = status.clone();
                        move |_| {
                            let mut ids = selected().unwrap_or_default();
                            if move_selected(&mut ids, &id, offset) {
                                store(Some(ids), &status);
                            }
                        }
                    });
                    row.add_suffix(&button);
                }
                check.connect_toggled({
                    let status = status.clone();
                    move |check| {
                        let mut ids = selected().unwrap_or_default();
                        ids.retain(|id| id != &sensor.id);
                        if check.is_active() {
                            ids.push(sensor.id.clone());
                        }
                        store(Some(ids), &status);
                    }
                });
                list.append(&row);
            }
        }
        if let Some(refresh) = refresh_weak.upgrade() {
            refresh.set_sensitive(!loading.get());
        }
        gtk::glib::ControlFlow::Continue
    });
    refresh.emit_clicked();
    group
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn labels_are_human_readable_without_bus_addresses() {
        for (chip, label, expected) in [
            ("amdgpu", "edge", "Edge"),
            ("amdgpu", "mem", "Mem"),
            ("amdgpu", "junction", "Hotspot"),
            ("k10temp", "Tccd1", "CPU CCD1"),
            ("k10temp", "Tccd2", "CPU CCD2"),
            ("k10temp", "Tctl", "CPU Tctl"),
            ("nvme", "Composite", "SSD NVMe"),
        ] {
            assert_eq!(friendly_label(chip, label), expected);
        }
    }
    #[test]
    fn ordering_moves_only_selected_sensors_and_respects_boundaries() {
        let mut ids = vec!["CPU".into(), "Edge".into(), "Mem".into()];
        assert!(move_selected(&mut ids, "Mem", -1));
        assert_eq!(ids, ["CPU", "Mem", "Edge"]);
        assert!(move_selected(&mut ids, "CPU", 1));
        assert_eq!(ids, ["Mem", "CPU", "Edge"]);
        assert!(!move_selected(&mut ids, "Mem", -1));
        assert!(!move_selected(&mut ids, "Edge", 1));
        assert!(!move_selected(&mut ids, "absent", 1));
    }
    #[test]
    fn selection_roundtrips_and_invalid_configuration_is_preserved() {
        let dir =
            std::env::temp_dir().join(format!("gameshift-home-sensors-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("selection.json");
        assert_eq!(load(&file).unwrap(), None);
        save(&file, &Some(vec!["chip/temp2".into(), "chip/temp1".into()])).unwrap();
        assert_eq!(
            load(&file).unwrap(),
            Some(vec!["chip/temp2".into(), "chip/temp1".into()])
        );
        save(&file, &Some(vec![])).unwrap();
        assert_eq!(load(&file).unwrap(), Some(vec![]));
        save(&file, &None).unwrap();
        assert_eq!(load(&file).unwrap(), None);
        fs::write(&file, "{\"schema_version\":99}").unwrap();
        let before = fs::read(&file).unwrap();
        assert!(save(&file, &None).is_err());
        assert_eq!(fs::read(&file).unwrap(), before);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn hwmon_indices_change_without_changing_sensor_identity() {
        let dir = std::env::temp_dir().join(format!("gameshift-home-hwmon-{}", std::process::id()));
        let chip = dir.join("sys/devices/pci0000:00/0000:05:00.0/hwmon/hwmon9");
        fs::create_dir_all(&chip).unwrap();
        fs::write(chip.join("name"), "amdgpu").unwrap();
        fs::write(chip.join("temp1_input"), "42000").unwrap();
        fs::write(chip.join("temp1_label"), "edge").unwrap();
        let class = dir.join("sys/class/hwmon");
        fs::create_dir_all(&class).unwrap();
        std::os::unix::fs::symlink(&chip, class.join("hwmon9")).unwrap();
        let first = discover(&dir);
        assert_eq!(first[0].value, Some(42.));
        fs::rename(class.join("hwmon9"), class.join("hwmon73")).unwrap();
        let second = discover(&dir);
        assert_eq!(first[0].id, second[0].id);
        assert!(!first[0].id.contains("hwmon9"));
        fs::write(chip.join("temp1_input"), "NaN").unwrap();
        assert_eq!(discover(&dir)[0].value, None);
        fs::remove_dir_all(dir).unwrap();
    }
}
