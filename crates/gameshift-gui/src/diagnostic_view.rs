//! One presentation model per snapshot; no filesystem reads or hardware probes here.
use gameshift_core::{Reading, Section, Snapshot};
use std::collections::BTreeSet;

pub const MISSING: &str = "Indisponible";
#[derive(Debug)]
pub struct Row {
    pub title: String,
    pub value: String,
    pub help: String,
    pub sources: String,
}
#[derive(Debug)]
pub struct Group {
    pub title: String,
    pub explanation: String,
    pub rows: Vec<Row>,
}
#[derive(Debug)]
pub struct Tile {
    pub title: String,
    pub value: String,
    pub detail: String,
    pub target: String,
    pub icon: &'static str,
}
#[derive(Debug)]
pub struct Detail {
    pub name: String,
    pub groups: Vec<Group>,
    pub technical: Vec<Reading>,
}
#[derive(Debug)]
pub struct View {
    pub tiles: Vec<Tile>,
    pub details: Vec<Detail>,
    pub captured: u64,
}
struct Fields<'a> {
    section: &'a Section,
    used: BTreeSet<String>,
}
fn raw<'a>(section: &'a Section, key: &str) -> Option<&'a str> {
    section
        .readings
        .iter()
        .find(|r| r.label == key)?
        .value
        .as_deref()
}
fn number(value: &str) -> Option<f64> {
    value
        .split_whitespace()
        .next()?
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite() && *n >= 0.)
}
pub fn gib(bytes: f64) -> String {
    format!("{:.1} Gio", bytes / 1_073_741_824.)
}
fn convert(value: &str, unit: &str) -> String {
    match unit {
        "kB" => number(value).map(|n| gib(n * 1024.)),
        "bytes" => number(value).map(gib),
        "sectors" => number(value).map(|n| gib(n * 512.)),
        "temperature" => number(value)
            .filter(|n| *n <= 150_000.)
            .map(|n| format!("{:.1} °C", n / 1000.)),
        "cache" => value
            .strip_suffix('K')
            .and_then(number)
            .map(|n| {
                if n >= 1024. {
                    format!("{:.0} Mio", n / 1024.)
                } else {
                    format!("{n:.0} Kio")
                }
            })
            .or_else(|| {
                value
                    .strip_suffix('M')
                    .and_then(number)
                    .map(|n| format!("{n:.0} Mio"))
            }),
        "khz" => number(value).map(|n| format!("{:.2} GHz", n / 1_000_000.)),
        _ => Some(value.to_owned()),
    }
    .unwrap_or_else(|| MISSING.into())
}
impl<'a> Fields<'a> {
    fn row(&mut self, title: &str, key: &str, unit: &str, help: &str) -> Row {
        self.used.insert(key.into());
        let reading = self.section.readings.iter().find(|r| r.label == key);
        Row {
            title: title.into(),
            value: reading
                .and_then(|r| r.value.as_deref())
                .map(|s| convert(s, unit))
                .unwrap_or_else(|| MISSING.into()),
            help: match reading.and_then(|r| r.error.as_deref()) {
                Some(error) => format!("{help}\n{error}"),
                None => help.into(),
            },
            sources: reading.map(|r| r.source.clone()).unwrap_or_default(),
        }
    }
    fn aggregate(&mut self, title: &str, suffix: &str, unit: &str, help: &str) -> Row {
        let readings = self
            .section
            .readings
            .iter()
            .filter(|r| r.label.ends_with(suffix))
            .collect::<Vec<_>>();
        let mut values = BTreeSet::new();
        let mut sources = Vec::new();
        for reading in readings {
            self.used.insert(reading.label.clone());
            if let Some(value) = &reading.value {
                values.insert(convert(value, unit));
            }
            sources.push(format!(
                "{} : {} = {}{}",
                reading.label,
                reading.source,
                reading.value.as_deref().unwrap_or(MISSING),
                reading
                    .error
                    .as_ref()
                    .map(|s| format!(" — {s}"))
                    .unwrap_or_default()
            ));
        }
        Row {
            title: title.into(),
            value: if values.is_empty() {
                MISSING.into()
            } else {
                values.into_iter().collect::<Vec<_>>().join(" · ")
            },
            help: help.into(),
            sources: sources.join("\n"),
        }
    }
    fn finish(self, groups: Vec<Group>) -> Detail {
        Detail {
            name: self.section.title.clone(),
            groups,
            technical: self
                .section
                .readings
                .iter()
                .filter(|r| !self.used.contains(&r.label))
                .cloned()
                .collect(),
        }
    }
}
fn group(title: &str, explanation: &str, rows: Vec<Row>) -> Group {
    Group {
        title: title.into(),
        explanation: explanation.into(),
        rows,
    }
}
fn vendor(id: &str) -> &str {
    match id {
        "0x1002" => "AMD",
        "0x10de" => "NVIDIA",
        "0x8086" => "Intel",
        _ => id,
    }
}
fn devices(section: &Section) -> Vec<String> {
    section
        .readings
        .iter()
        .filter(|r| r.label.ends_with(" — Périphérique PCI"))
        .filter_map(|r| r.label.split_once(" — ").map(|(id, _)| id.to_owned()))
        .collect()
}
impl View {
    pub fn build(snapshot: &Snapshot) -> Self {
        let mut tiles = Vec::new();
        let mut details = Vec::new();
        let system_arch = snapshot
            .sections
            .iter()
            .find(|s| s.title == "Système")
            .and_then(|s| raw(s, "Architecture système"))
            .unwrap_or(MISSING);
        for section in &snapshot.sections {
            let mut f = Fields {
                section,
                used: BTreeSet::new(),
            };
            let mut tile = |title: &str, value: String, detail: String, icon: &'static str| {
                tiles.push(Tile {
                    title: title.into(),
                    value,
                    detail,
                    target: section.title.clone(),
                    icon,
                })
            };
            let groups = match section.title.as_str() {
                "Système" => {
                    tile(
                        "Linux",
                        raw(section, "Distribution").unwrap_or(MISSING).into(),
                        format!("Noyau {}", raw(section, "Kernel").unwrap_or(MISSING)),
                        "computer-symbolic",
                    );
                    vec![group("Distribution et session", "", vec![
                        f.row("Distribution", "Distribution", "", "Distribution et version déclarées par os-release."),
                        f.row("Noyau actif", "Kernel", "", "Noyau actuellement démarré."),
                        f.row("Architecture système", "Architecture système", "", "Architecture annoncée par le noyau, distincte de celle du binaire."),
                        f.row("Bureau", "Environnement de bureau", "", "Bureau déclaré par la session courante."),
                        f.row("Session graphique", "Session graphique", "", "Wayland ou X11, lorsque la session le déclare."),
                    ]), group("Composants graphiques", "Les versions des pilotes propres à chaque GPU figurent sur sa fiche.", vec![
                        f.row("Chargeur Vulkan", "Chargeur Vulkan", "", "Version API prise en charge par le chargeur, pas version du pilote graphique."),
                    ])]
                }
                "Processeur" => {
                    tile(
                        "Processeur",
                        raw(section, "Modèle").unwrap_or(MISSING).into(),
                        format!(
                            "{} cœurs · {} threads",
                            raw(section, "Cœurs physiques exposés").unwrap_or(MISSING),
                            raw(section, "Threads exposés par procfs").unwrap_or(MISSING)
                        ),
                        "preferences-system-symbolic",
                    );
                    let mut identity = vec![f.row("Modèle", "Modèle", "", "Nom exposé par Linux."),
                        Row { title: "Architecture".into(), value: system_arch.into(), help: "Architecture annoncée par le noyau.".into(), sources: "uname -m".into() },
                        f.row("Cœurs physiques", "Cœurs physiques exposés", "", "Topologie exposée à Linux ; peut être limitée dans une machine virtuelle."),
                        f.row("Threads logiques", "Threads exposés par procfs", "", "Unités logiques exposées, incluant le SMT lorsqu'il est actif.")];
                    let indices = section
                        .readings
                        .iter()
                        .filter(|r| r.label.ends_with(" — Cache Niveau"))
                        .filter_map(|r| r.label.split_once(" — ").map(|(s, _)| s.to_owned()))
                        .collect::<Vec<_>>();
                    for index in indices {
                        let level = raw(section, &format!("{index} — Cache Niveau")).unwrap_or("?");
                        let kind =
                            raw(section, &format!("{index} — Cache Type")).unwrap_or(MISSING);
                        identity.push(f.row(&format!("Cache L{level} · {kind}"), &format!("{index} — Cache Capacité"), "cache", "Description du domaine de cache de CPU0 ; les caches partagés ne sont pas additionnés."));
                        f.used.insert(format!("{index} — Cache Niveau"));
                        f.used.insert(format!("{index} — Cache Type"));
                    }
                    vec![group("Identité et caches", "", identity), group("Fréquences et énergie", "Le gouverneur est une politique, pas une fréquence imposée. Avec amd-pstate-epp, powersave peut autoriser le boost ; l’EPP oriente le compromis réactivité/consommation.", vec![
                        f.aggregate("Pilote de fréquences", " — Pilote", "", "Les valeurs distinctes des politiques CPU sont affichées."),
                        f.aggregate("Gouverneur", " — Gouverneur", "", "Ne permet pas à lui seul de conclure que le CPU est bridé."),
                        f.aggregate("Préférence énergétique (EPP)", " — EPP", "", "Préférence du pilote entre performance et économie, lorsqu'il la prend en charge."),
                        f.aggregate("Fréquence observée", " — Fréquence courante (kHz)", "khz", "Instantané des politiques CPU ; pas une mesure du boost maximal ni de charge."),
                        f.aggregate("Minimum matériel annoncé", " — Fréquence minimale (kHz)", "khz", "Bornes cpuinfo du pilote, pas limites de fréquence configurées."),
                        f.aggregate("Maximum matériel annoncé", " — Fréquence maximale (kHz)", "khz", "Borne déclarée par le pilote, pas fréquence garantie en charge."),
                        f.aggregate("Température observée", " — Température (millidegrés Celsius)", "temperature", "Sondes CPU hwmon uniquement, au moment du diagnostic ; plusieurs valeurs peuvent correspondre à plusieurs capteurs."),
                    ])]
                }
                "Carte graphique" => {
                    let ids = devices(section);
                    let mut groups = Vec::new();
                    for (index, id) in ids.iter().enumerate() {
                        let model = raw(section, &format!("{id} — Modèle graphique"))
                            .unwrap_or("Modèle non identifié");
                        let vram = raw(section, &format!("{id} — Mémoire vidéo (octets)"))
                            .or_else(|| raw(section, &format!("{id} — VRAM (octets)")))
                            .map(|s| convert(s, "bytes"))
                            .unwrap_or_else(|| MISSING.into());
                        let maker = raw(section, &format!("{id} — Constructeur PCI"))
                            .map(vendor)
                            .unwrap_or(MISSING);
                        tile(
                            &if ids.len() > 1 {
                                format!("Carte graphique {}", index + 1)
                            } else {
                                "Carte graphique".into()
                            },
                            model.into(),
                            format!("{maker} · VRAM {vram}"),
                            "video-display-symbolic",
                        );
                        let mut rows = vec![f.row("Modèle", &format!("{id} — Modèle graphique"), "", "Nom résolu depuis les bases matérielles locales."),
                            Row {title: "Fabricant du GPU".into(),value:maker.into(),help:"Constructeur du processeur graphique, distinct du fabricant de la carte.".into(),sources:format!("{id} — Constructeur PCI")},
                            f.row("Fabricant de la carte", &format!("{id} — Fabricant de la carte"), "", "Identification du sous-système PCI si disponible."),
                            f.row("Mémoire vidéo dédiée", &format!("{id} — Mémoire vidéo (octets)"), "bytes", "VRAM dédiée exposée par le pilote. Un GPU intégré peut ne pas exposer de VRAM dédiée."),
                            f.row("Pilote noyau", &format!("{id} — pilote PCI"), "", "Module Linux assurant l'accès au matériel ; différent du pilote Vulkan."),
                            f.row("Pilote Vulkan annoncé", &format!("{id} — Pilote Vulkan"), "", "Pilote de l'ICD énuméré. L'association nécessite une correspondance matérielle unique ; un jeu peut choisir un autre ICD."),
                            f.row("Informations du pilote Vulkan", &format!("{id} — Version pilote Vulkan"), "", "Chaîne de version fournie par le pilote ; aucune interprétation du numéro brut constructeur."),
                            f.row("API Vulkan", &format!("{id} — API Vulkan"), "", "Version API annoncée pour ce périphérique."),
                            f.row("Mesa attribué à ce GPU", &format!("{id} — Mesa"), "", "Affiché seulement si la chaîne de l'ICD associé mentionne explicitement Mesa."),
                            f.row("Temps GPU pour le benchmark", &format!("{id} — Timestamps Vulkan"), "", "Support déclaré sur une file graphique ; les mesures nécessitent un test réel."),
                        ];
                        if raw(section, &format!("{id} — pilote PCI")).is_none() {
                            rows[4] = f.row(
                                "Pilote noyau",
                                &format!("{id} — pilote kernel"),
                                "",
                                "Module Linux exposé par DRM.",
                            );
                        }
                        if raw(section, &format!("{id} — Mémoire vidéo (octets)")).is_none()
                            && raw(section, &format!("{id} — VRAM (octets)")).is_some()
                        {
                            rows[3] = f.row(
                                "Mémoire vidéo dédiée",
                                &format!("{id} — VRAM (octets)"),
                                "bytes",
                                "VRAM dédiée exposée par DRM.",
                            );
                        }
                        groups.push(group(&format!("Carte graphique {}", index+1), &format!("Identifiant matériel : {id}. Les données de cette fiche restent propres à ce périphérique."), rows));
                    }
                    if groups.is_empty() {
                        tile(
                            "Carte graphique",
                            MISSING.into(),
                            "Aucun GPU accessible".into(),
                            "video-display-symbolic",
                        );
                        groups.push(group("Périphériques graphiques", "Aucun GPU accessible. Les causes détaillées restent disponibles ci-dessous.", vec![]));
                    }
                    groups
                }
                "Mémoire" => {
                    tile(
                        "Mémoire vive",
                        raw(section, "RAM totale")
                            .map(|s| convert(s, "kB"))
                            .unwrap_or_else(|| MISSING.into()),
                        "Capacité utilisable déclarée par Linux".into(),
                        "media-flash-symbolic",
                    );
                    vec![group("Capacités", "", vec![f.row("RAM totale", "RAM totale", "kB", "Mémoire utilisable par Linux, distincte de la capacité physique des barrettes."), f.row("Swap configurée", "Swap totale", "kB", "Espace d'échange total, pouvant inclure de la ZRAM.")]),
                        group("État au moment du diagnostic", "Ces valeurs ne sont actualisées qu'avec le bouton Actualiser.", vec![f.row("RAM disponible", "RAM disponible", "kB", "MemAvailable inclut les caches récupérables ; ce n'est pas la RAM strictement libre."),f.row("Swap libre", "Swap libre", "kB", "Espace d'échange encore disponible.")]),
                        group("Gestion de la mémoire", "", vec![f.row("Propension à utiliser la swap", "Swappiness", "", "Swappiness est une préférence de récupération mémoire, pas un pourcentage d'utilisation de la RAM."),f.row("Pages mémoire de grande taille", "Transparent Huge Pages", "", "Le mode actif figure entre crochets. Aucun réglage n'est modifié.")])]
                }
                "Stockage" => {
                    let root = raw(section, "Disque principal");
                    if let Some(id) = root {
                        tile(
                            "Stockage principal",
                            raw(section, &format!("{id} — Modèle")).unwrap_or(id).into(),
                            raw(
                                section,
                                &format!("{id} — Capacité (secteurs de 512 octets)"),
                            )
                            .map(|s| convert(s, "sectors"))
                            .unwrap_or_else(|| MISSING.into()),
                            "drive-harddisk-symbolic",
                        );
                    } else {
                        tile(
                            "Volume système",
                            raw(section, "Volume racine").unwrap_or(MISSING).into(),
                            "Disque physique principal non déterminé".into(),
                            "drive-harddisk-symbolic",
                        );
                    }
                    let mut groups = vec![group("Volume système", "Le disque principal n'est désigné que si la chaîne vers un disque physique unique est vérifiable. Les volumes Btrfs, chiffrés, RAID et conteneurs restent prudents.", vec![f.row("Source du montage racine", "Volume racine", "", "Source du montage / dans l'espace de noms du processus."),f.row("Système de fichiers", "Système de fichiers racine", "", "Format du volume monté sur /."),f.row("Disque principal identifié", "Disque principal", "", "Indisponible lorsque l'association physique est ambiguë.")])];
                    let ids = section
                        .readings
                        .iter()
                        .filter(|r| r.label.ends_with(" — Capacité (secteurs de 512 octets)"))
                        .filter_map(|r| r.label.split_once(" — ").map(|(n, _)| n))
                        .filter(|n| !["loop", "ram", "zram"].iter().any(|p| n.starts_with(p)))
                        .collect::<Vec<_>>();
                    for id in ids {
                        let kind = if id.starts_with("nvme") {
                            "NVMe"
                        } else {
                            match raw(section, &format!("{id} — Rotatif (0=non, 1=oui)")) {
                                Some("1") => "Disque mécanique",
                                Some("0") => "Non rotatif",
                                _ => "Type indisponible",
                            }
                        };
                        groups.push(group(&format!("Périphérique {id}"),kind,vec![f.row("Modèle",&format!("{id} — Modèle"),"","Modèle exposé par sysfs."),f.row("Capacité",&format!("{id} — Capacité (secteurs de 512 octets)"),"sectors","Capacité du périphérique bloc, pas espace libre du système de fichiers."),f.row("Ordonnanceur d'entrées/sorties",&format!("{id} — Scheduler I/O"),"","Mode actif entre crochets ; valeurs restantes disponibles."),f.row("Discard / TRIM maximal",&format!("{id} — Discard maximal (octets)"),"bytes","Capacité de discard annoncée par la pile bloc ; ne prouve pas qu'un TRIM périodique est actif.")]));
                    }
                    groups
                }
                _ => vec![],
            };
            details.push(f.finish(groups));
        }
        Self {
            tiles,
            details,
            captured: snapshot.captured_unix_seconds,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn section(title: &str, values: &[(&str, &str)]) -> Section {
        Section {
            title: title.into(),
            readings: values
                .iter()
                .map(|(k, v)| Reading::available(k, "fixture", *v))
                .collect(),
        }
    }
    #[test]
    fn kernel_cache_units_are_readable_and_invalid_values_stay_unavailable() {
        assert_eq!(convert("98304K", "cache"), "96 Mio");
        assert_eq!(convert("32K", "cache"), "32 Kio");
        assert_eq!(convert("invalid", "cache"), MISSING);
    }
    #[test]
    fn multiple_gpus_keep_their_own_memory_and_driver() {
        let snapshot = Snapshot {
            schema_version: 1,
            captured_unix_seconds: 1,
            sections: vec![section(
                "Carte graphique",
                &[
                    ("0000:03:00.0 — Périphérique PCI", "0x747e"),
                    ("0000:03:00.0 — Modèle graphique", "AMD GPU"),
                    ("0000:03:00.0 — Constructeur PCI", "0x1002"),
                    ("0000:03:00.0 — Mémoire vidéo (octets)", "17179869184"),
                    ("0000:03:00.0 — pilote PCI", "amdgpu"),
                    ("0000:09:00.0 — Périphérique PCI", "0x1234"),
                    ("0000:09:00.0 — Modèle graphique", "Intel GPU"),
                    ("0000:09:00.0 — Constructeur PCI", "0x8086"),
                    ("0000:09:00.0 — pilote PCI", "i915"),
                ],
            )],
        };
        let view = View::build(&snapshot);
        assert_eq!(view.tiles.len(), 2);
        assert!(view.tiles[0].detail.contains("16.0 Gio"));
        assert!(view.tiles[1].detail.contains(MISSING));
        let groups = &view.details[0].groups;
        assert_eq!(
            groups[0]
                .rows
                .iter()
                .find(|r| r.title == "Pilote noyau")
                .unwrap()
                .value,
            "amdgpu"
        );
        assert_eq!(
            groups[1]
                .rows
                .iter()
                .find(|r| r.title == "Pilote noyau")
                .unwrap()
                .value,
            "i915"
        );
        assert_eq!(
            groups[1]
                .rows
                .iter()
                .find(|r| r.title == "Mémoire vidéo dédiée")
                .unwrap()
                .value,
            MISSING
        );
        assert_eq!(
            groups[0]
                .rows
                .iter()
                .find(|r| r.title == "Mesa attribué à ce GPU")
                .unwrap()
                .value,
            MISSING
        );
        assert!(!view.details[0]
            .technical
            .iter()
            .any(|r| r.label.ends_with(" — Modèle graphique")));
    }
    #[test]
    fn unavailable_ram_is_not_zero_and_cpu_energy_is_explained() {
        let snapshot = Snapshot {
            schema_version: 1,
            captured_unix_seconds: 1,
            sections: vec![
                section("Mémoire", &[("RAM totale", "invalid")]),
                section(
                    "Processeur",
                    &[
                        ("policy0 — Gouverneur", "powersave"),
                        ("policy0 — Pilote", "amd-pstate-epp"),
                        ("policy0 — EPP", "balance_performance"),
                    ],
                ),
            ],
        };
        let view = View::build(&snapshot);
        assert_eq!(view.tiles[0].value, MISSING);
        assert!(view.details[1].groups[1].explanation.contains("boost"));
        assert!(view.details[1].groups[1]
            .rows
            .iter()
            .any(|r| r.value == "balance_performance"));
    }
    #[test]
    fn no_arbitrary_disk_is_designated_as_primary() {
        let snapshot = Snapshot {
            schema_version: 1,
            captured_unix_seconds: 1,
            sections: vec![section(
                "Stockage",
                &[
                    ("sda — Capacité (secteurs de 512 octets)", "1000"),
                    ("sdb — Capacité (secteurs de 512 octets)", "2000"),
                ],
            )],
        };
        let view = View::build(&snapshot);
        assert_eq!(view.tiles[0].title, "Volume système");
        assert_eq!(view.tiles[0].value, MISSING);
        assert_eq!(view.details[0].groups.len(), 3);
    }
}
