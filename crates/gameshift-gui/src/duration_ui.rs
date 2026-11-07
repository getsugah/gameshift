//! Presets with inspectable/customizable timings. Values never change workload settings.
use adw::prelude::*;
use gameshift_core::duration::{BenchmarkProgress, DurationProfile};
#[derive(Clone)]
pub struct Controls {
    pub root: gtk::Box,
    pub profile: gtk::DropDown,
    pub preparation: gtk::SpinButton,
    pub measurement: gtk::SpinButton,
    pub repetitions: gtk::SpinButton,
    cpu: bool,
}
impl Controls {
    pub fn new(cpu: bool) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 10);
        root.append(&crate::context_help::heading("Durée du test","Rapide donne un premier aperçu. Standard est le choix par défaut. Approfondi observe le comportement plus longtemps ; il ne garantit pas à lui seul une meilleure précision. Les calculs et la scène restent identiques."));
        let profile = gtk::DropDown::from_strings(&["Rapide", "Standard", "Approfondi"]);
        profile.set_selected(1);
        root.append(&profile);
        let explanation = gtk::Label::new(None);
        explanation.set_xalign(0.);
        explanation.set_wrap(true);
        explanation.add_css_class("gs-card-detail");
        root.append(&explanation);
        let preparation = gtk::SpinButton::with_range(
            if cpu { 0. } else { 1. },
            if cpu { 10. } else { 300. },
            if cpu { 0.1 } else { 1. },
        );
        preparation.set_digits(if cpu { 2 } else { 0 });
        let measurement = gtk::SpinButton::with_range(
            if cpu { 0.1 } else { 1. },
            if cpu { 60. } else { 600. },
            if cpu { 0.1 } else { 1. },
        );
        measurement.set_digits(if cpu { 2 } else { 0 });
        let repetitions = gtk::SpinButton::with_range(if cpu { 2. } else { 1. }, 20., 1.);
        let advanced = gtk::Expander::new(Some("Paramètres de durée détaillés"));
        let fields = gtk::Box::new(gtk::Orientation::Vertical, 8);
        fields.append(&crate::context_help::heading("Préparation par répétition (s)","Travail réellement exécuté avant chaque répétition, exclu des mesures. La même préparation est appliquée aux passages comparables, sans attente artificielle. Le CPU fixe un nombre d’étapes commun par taille après calibration."));
        fields.append(&preparation);
        fields.append(&crate::context_help::heading(if cpu{"Cible de mesure par répétition (s)"}else{"Mesure par répétition (s)"},if cpu{"La calibration estime le nombre d’étapes puis le fixe pour toutes les répétitions et tous les nombres de threads d’une même taille. Les configurations plus lentes prennent davantage de temps. La durée cible est approximative ; les durées réelles sont enregistrées."}else{"Durée de rendu après la préparation. Les images de préparation sont exclues. Une image en cours peut dépasser légèrement la durée demandée."}));
        fields.append(&measurement);
        fields.append(&crate::context_help::heading("Répétitions","Chaque répétition recommence la même simulation ou trajectoire de caméra. Plusieurs répétitions permettent d’observer la variation. Le profil GPU Rapide ne comporte que deux répétitions : il reste exploratoire."));
        fields.append(&repetitions);
        advanced.set_child(Some(&fields));
        root.append(&advanced);
        let c = Self {
            root,
            profile,
            preparation,
            measurement,
            repetitions,
            cpu,
        };
        let weak_root = c.root.downgrade();
        let weak_profile = c.profile.downgrade();
        let weak_preparation = c.preparation.downgrade();
        let weak_measurement = c.measurement.downgrade();
        let weak_repetitions = c.repetitions.downgrade();
        let rebuild: std::rc::Rc<dyn Fn() -> Option<Controls>> = std::rc::Rc::new(move || {
            Some(Controls {
                root: weak_root.upgrade()?,
                profile: weak_profile.upgrade()?,
                preparation: weak_preparation.upgrade()?,
                measurement: weak_measurement.upgrade()?,
                repetitions: weak_repetitions.upgrade()?,
                cpu,
            })
        });
        let explanation = explanation.downgrade();
        let update: std::rc::Rc<dyn Fn()> = std::rc::Rc::new({
            let rebuild = rebuild.clone();
            move || {
                let (Some(c), Some(explanation)) = (rebuild(), explanation.upgrade()) else {
                    return;
                };
                let profile = c.effective_profile();
                let prep = c.preparation.value();
                let measure = c.measurement.value();
                let reps = c.repetitions.value_as_int();
                let estimate = (prep + measure) * reps as f64;
                explanation.set_text(&if cpu{format!("{} · cible {:.2} s de calcul par répétition, {} répétitions. Environ {:.1} s par configuration, hors calibration ; davantage pour les nombres de threads plus lents. Aucune pause pour allonger la mesure.",profile.label(),measure,reps,estimate)}else{format!("{} · {} s de préparation puis {} s de mesure × {} passages. Environ {:.0} s, hors initialisation et analyse. Préparation active identique avant chaque passage.",profile.label(),prep,measure,reps,estimate)});
            }
        });
        for spin in [&c.preparation, &c.measurement, &c.repetitions] {
            let update = update.clone();
            spin.connect_value_changed(move |_| update());
        }
        let updated = update.clone();
        c.profile.connect_selected_notify(move |_| {
            if let Some(values) = rebuild() {
                values.apply_preset();
                updated();
            }
        });
        c.apply_preset();
        update();
        c
    }
    pub fn selected_profile(&self) -> DurationProfile {
        match self.profile.selected() {
            0 => DurationProfile::Quick,
            2 => DurationProfile::Thorough,
            _ => DurationProfile::Standard,
        }
    }
    pub fn effective_profile(&self) -> DurationProfile {
        let p = self.selected_profile();
        let (a, b, c) = if self.cpu {
            let (m, w, r) = p.cpu();
            (w as f64 / 1000., m as f64 / 1000., r as i32)
        } else {
            let (w, m, r) = p.gpu();
            (w as f64, m as f64, r as i32)
        };
        if (self.preparation.value() - a).abs() < 0.001
            && (self.measurement.value() - b).abs() < 0.001
            && self.repetitions.value_as_int() == c
        {
            p
        } else {
            DurationProfile::Custom
        }
    }
    fn apply_preset(&self) {
        if self.cpu {
            let (m, w, r) = self.selected_profile().cpu();
            self.preparation.set_value(w as f64 / 1000.);
            self.measurement.set_value(m as f64 / 1000.);
            self.repetitions.set_value(r as f64);
        } else {
            let (w, m, r) = self.selected_profile().gpu();
            self.preparation.set_value(w as f64);
            self.measurement.set_value(m as f64);
            self.repetitions.set_value(r as f64);
        }
    }
}
pub fn show_progress(bar: &gtk::ProgressBar, status: &gtk::Label, p: &BenchmarkProgress) {
    bar.set_fraction(p.fraction.clamp(0., 1.));
    bar.set_show_text(true);
    let caption = if p.repetition == 0 {
        format!("{} · configuration {}/{}", p.phase, p.case_index, p.cases)
    } else {
        format!(
            "{} · passage {}/{} · configuration {}/{}",
            p.phase, p.repetition, p.repetitions, p.case_index, p.cases
        )
    };
    bar.set_text(Some(&caption));
    let fps = p
        .fps
        .map(|n| format!(" · {n:.1} FPS récents"))
        .unwrap_or_default();
    let cpu = p
        .cpu_ms
        .map(|n| {
            if p.fps.is_some() {
                format!(" · préparation/soumission CPU {n:.2} ms")
            } else {
                format!(" · temps d’étape CPU récent {n:.2} ms")
            }
        })
        .unwrap_or_default();
    let gpu = p
        .gpu_ms
        .map(|n| format!(" · calcul GPU {n:.2} ms"))
        .unwrap_or_default();
    status.set_text(&format!(
        "{} · {:.0} s écoulées{fps}{cpu}{gpu}",
        p.phase, p.elapsed_seconds
    ));
}
