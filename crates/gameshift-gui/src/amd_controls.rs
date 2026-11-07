//! GPU editors shared by persistent profiles. The user agent owns hardware sessions.
use adw::prelude::*;
use gameshift_control::*;
use gameshift_profiles::{Mode, Profile};
use gtk::glib;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::mpsc,
    time::Duration,
};
thread_local! {static EDIT_PROFILE:RefCell<Option<Profile>>=const{RefCell::new(None)};}
thread_local! {static NEW_PROFILE:Cell<bool>=const{Cell::new(false)};}
pub fn new_profile() {
    EDIT_PROFILE.with_borrow_mut(|p| *p = None);
    NEW_PROFILE.with(|v| v.set(true));
}
pub fn edit_profile(profile: Profile) {
    EDIT_PROFILE.with_borrow_mut(|p| *p = Some(profile));
}
pub fn awaiting_confirmation() -> bool {
    crate::profiles_page::pending_apply()
}
pub fn any_active() -> bool {
    false
}
fn label(value: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(value));
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::Word);
    label.set_xalign(0.);
    label
}
struct ScalarEditor {
    include: gtk::CheckButton,
    target: ScalarTarget,
    spin: gtk::SpinButton,
    factor: f64,
}
struct CurveEditor {
    include: gtk::CheckButton,
    points: Vec<(gtk::SpinButton, gtk::SpinButton)>,
}
struct ClockEditor {
    include: gtk::CheckButton,
    domain: ClockDomain,
    minimum: Option<(gtk::CheckButton, gtk::SpinButton)>,
    maximum: Option<(gtk::CheckButton, gtk::SpinButton)>,
}

#[derive(Default)]
struct Editors {
    scalars: Vec<ScalarEditor>,
    curve: Option<CurveEditor>,
    automatic: Option<gtk::CheckButton>,
    performance: Option<(gtk::CheckButton, gtk::DropDown)>,
    workload: Option<(gtk::CheckButton, gtk::DropDown, Vec<u32>)>,
    clocks: Vec<ClockEditor>,
}
impl Editors {
    fn operations(&self) -> Vec<Operation> {
        let mut operations = Vec::new();
        for editor in &self.scalars {
            if editor.include.is_active() {
                operations.push(Operation::Scalar {
                    target: editor.target,
                    value: (editor.spin.value() * editor.factor).round() as i64,
                });
            }
        }
        if let Some(editor) = &self.curve {
            if editor.include.is_active() {
                operations.push(Operation::FanCurve {
                    points: editor
                        .points
                        .iter()
                        .map(|(t, p)| Point {
                            temperature: t.value().round() as i64,
                            pwm_percent: p.value().round() as i64,
                        })
                        .collect(),
                });
            }
        }
        if self.automatic.as_ref().is_some_and(|v| v.is_active()) {
            operations.push(Operation::FanAutomatic {});
        }
        if let Some((include, choice)) = &self.performance {
            if include.is_active() {
                if let Some(mode) = PERFORMANCE_MODES.get(choice.selected() as usize) {
                    operations.push(Operation::Performance {
                        mode: (*mode).into(),
                    });
                }
            }
        }
        if let Some((include, choice, indices)) = &self.workload {
            if include.is_active() {
                if let Some(index) = indices.get(choice.selected() as usize) {
                    operations.push(Operation::Workload { index: *index });
                }
            }
        }
        for editor in &self.clocks {
            if editor.include.is_active() {
                operations.push(Operation::Clocks {
                    domain: editor.domain,
                    minimum: editor
                        .minimum
                        .as_ref()
                        .filter(|(include, _)| include.is_active())
                        .map(|(_, s)| s.value().round() as i64),
                    maximum: editor
                        .maximum
                        .as_ref()
                        .filter(|(include, _)| include.is_active())
                        .map(|(_, s)| s.value().round() as i64),
                });
            }
        }
        operations
    }
}
fn group(title: &str, parent: &gtk::Box) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title(title);
    parent.append(&group);
    group
}
fn spin(limits: Limits, current: i64, factor: f64) -> gtk::SpinButton {
    let spin = gtk::SpinButton::with_range(
        limits.minimum as f64 / factor,
        limits.maximum as f64 / factor,
        if factor == 1. { 1. } else { 0.1 },
    );
    spin.set_digits(if factor == 1. { 0 } else { 1 });
    spin.set_value(current as f64 / factor);
    spin.set_valign(gtk::Align::Center);
    spin
}
fn draw_curve(
    area: &gtk::DrawingArea,
    points: Rc<RefCell<Vec<Point>>>,
    limits: Limits,
    pwm: Limits,
) {
    area.set_draw_func(move |widget, cr, w, h| {
        let left = 40.;
        let top = 15.;
        let width = (w as f64 - left - 15.).max(1.);
        let height = (h as f64 - top - 25.).max(1.);
        let color = widget.color();
        cr.set_source_rgba(
            color.red() as f64,
            color.green() as f64,
            color.blue() as f64,
            0.25,
        );
        cr.rectangle(left, top, width, height);
        let _ = cr.stroke();
        cr.set_source_rgba(
            color.red() as f64,
            color.green() as f64,
            color.blue() as f64,
            1.,
        );
        cr.set_font_size(11.);
        cr.move_to(left, h as f64 - 5.);
        let _ = cr.show_text(&format!(
            "{}–{} °C (point chaud) · {}–{} % PWM",
            limits.minimum, limits.maximum, pwm.minimum, pwm.maximum
        ));
        cr.set_source_rgb(0.15, 0.65, 0.57);
        cr.set_line_width(2.);
        let points = points.borrow();
        for (i, p) in points.iter().enumerate() {
            let x = left
                + width * (p.temperature - limits.minimum) as f64
                    / (limits.maximum - limits.minimum).max(1) as f64;
            let y = top
                + height
                    * (1.
                        - (p.pwm_percent - pwm.minimum) as f64
                            / (pwm.maximum - pwm.minimum).max(1) as f64);
            if i == 0 {
                cr.move_to(x, y);
            } else {
                cr.line_to(x, y);
            }
        }
        let _ = cr.stroke();
        for p in points.iter() {
            let x = left
                + width * (p.temperature - limits.minimum) as f64
                    / (limits.maximum - limits.minimum).max(1) as f64;
            let y = top
                + height
                    * (1.
                        - (p.pwm_percent - pwm.minimum) as f64
                            / (pwm.maximum - pwm.minimum).max(1) as f64);
            cr.arc(x, y, 5., 0., std::f64::consts::TAU);
            let _ = cr.fill();
        }
    });
}
fn curve_editor(curve: &Curve, parent: &gtk::Box) -> Option<CurveEditor> {
    let (Some(temperature), Some(pwm)) = (curve.temperature, curve.pwm_percent) else {
        parent.append(&label(
            "Courbe consultable, plages du pilote absentes : modification indisponible.",
        ));
        return None;
    };
    let include =
        gtk::CheckButton::with_label("Inclure la courbe PMFW personnalisée · Junction / Hotspot");
    parent.append(&include);
    parent.append(&label("Les réglages acoustiques/PWM minimal/température cible réactivent le mode automatique : ne pas les cocher avec une courbe personnalisée. Le Zero RPM reste indépendant."));
    let initial = if curve.validate(&curve.points).is_ok() {
        curve.points.clone()
    } else {
        parent.append(&label("Courbe automatique sans points utilisables : brouillon linéaire construit dans les plages lues, non appliqué."));
        let last = curve.points.len().saturating_sub(1).max(1) as i64;
        (0..curve.points.len())
            .map(|i| Point {
                temperature: temperature.minimum
                    + (temperature.maximum - temperature.minimum) * i as i64 / last,
                pwm_percent: pwm.minimum + (pwm.maximum - pwm.minimum) * i as i64 / last,
            })
            .collect()
    };
    let data = Rc::new(RefCell::new(initial));
    let area = gtk::DrawingArea::new();
    area.set_content_height(220);
    area.set_hexpand(true);
    area.add_css_class("gs-gpu-chart");
    area.set_tooltip_text(Some("Déplacer les points ou utiliser les champs numériques. Aucune écriture avant Appliquer et autorisation."));
    draw_curve(&area, data.clone(), temperature, pwm);
    parent.append(&area);
    let rows = gtk::FlowBox::new();
    rows.set_selection_mode(gtk::SelectionMode::None);
    rows.set_min_children_per_line(1);
    rows.set_max_children_per_line(4);
    let mut editors = Vec::new();
    for (index, point) in data.borrow().iter().enumerate() {
        let box_ = gtk::Box::new(gtk::Orientation::Vertical, 5);
        box_.append(&label(&format!("Point {} · °C / % PWM", index + 1)));
        let t = spin(temperature, point.temperature, 1.);
        let p = spin(pwm, point.pwm_percent, 1.);
        box_.append(&t);
        box_.append(&p);
        rows.insert(&box_, -1);
        editors.push((t, p));
    }
    for (index, (t, p)) in editors.iter().enumerate() {
        let data = data.clone();
        let t_weak = t.downgrade();
        let p_weak = p.downgrade();
        let weak = area.downgrade();
        let update = Rc::new(move || {
            if let (Some(t), Some(p)) = (t_weak.upgrade(), p_weak.upgrade()) {
                data.borrow_mut()[index] = Point {
                    temperature: t.value().round() as i64,
                    pwm_percent: p.value().round() as i64,
                };
                if let Some(area) = weak.upgrade() {
                    area.queue_draw();
                }
            }
        });
        let u = update.clone();
        t.connect_value_changed(move |_| u());
        p.connect_value_changed(move |_| update());
    }
    let drag = gtk::GestureDrag::new();
    let drag_index = Rc::new(Cell::new(None::<usize>));
    let start = Rc::new(Cell::new((0., 0.)));
    let weak = area.downgrade();
    let points = data.clone();
    let chosen = drag_index.clone();
    let origin = start.clone();
    drag.connect_drag_begin(move |_, x, y| {
        let Some(area) = weak.upgrade() else { return };
        let w = (area.width() as f64 - 55.).max(1.);
        let h = (area.height() as f64 - 40.).max(1.);
        let best = points
            .borrow()
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let px = 40.
                    + w * (p.temperature - temperature.minimum) as f64
                        / (temperature.maximum - temperature.minimum).max(1) as f64;
                let py = 15.
                    + h * (1.
                        - (p.pwm_percent - pwm.minimum) as f64
                            / (pwm.maximum - pwm.minimum).max(1) as f64);
                (i, (px - x).powi(2) + (py - y).powi(2))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        chosen.set(
            best.filter(|(_, distance)| *distance <= 400.)
                .map(|(i, _)| i),
        );
        origin.set((x, y));
    });
    let weak = area.downgrade();
    let pairs = editors
        .iter()
        .map(|(t, p)| (t.downgrade(), p.downgrade()))
        .collect::<Vec<_>>();
    drag.connect_drag_update(move |_, dx, dy| {
        let (Some(index), Some(area)) = (drag_index.get(), weak.upgrade()) else {
            return;
        };
        let (x, y) = start.get();
        let w = (area.width() as f64 - 55.).max(1.);
        let h = (area.height() as f64 - 40.).max(1.);
        let temp = (temperature.minimum as f64
            + ((x + dx - 40.) / w).clamp(0., 1.)
                * (temperature.maximum - temperature.minimum) as f64)
            .round();
        let speed = (pwm.minimum as f64
            + (1. - ((y + dy - 15.) / h).clamp(0., 1.)) * (pwm.maximum - pwm.minimum) as f64)
            .round();
        if let (Some(t), Some(p)) = (pairs[index].0.upgrade(), pairs[index].1.upgrade()) {
            t.set_value(temp);
            p.set_value(speed);
        }
    });
    area.add_controller(drag);
    parent.append(&rows);
    Some(CurveEditor {
        include,
        points: editors,
    })
}
fn build_editors(catalog: &Catalog, content: &gtk::Box) -> Editors {
    while let Some(child) = content.first_child() {
        content.remove(&child);
    }
    let mut editors = Editors::default();
    let cooling = group("Refroidissement · PMFW", content);
    let energy = group("Énergie et performances", content);
    let clocks_group = group("Fréquences et tensions · avancé", content);
    for scalar in &catalog.scalars {
        let row = adw::ActionRow::new();
        row.set_title(&scalar.label);
        row.set_subtitle_lines(0);
        row.set_title_lines(0);
        row.set_use_markup(false);
        let factor = if matches!(scalar.target, ScalarTarget::PowerCap { .. }) {
            1e6
        } else {
            1.
        };
        let unit = if factor == 1e6 {
            "W"
        } else {
            scalar.unit.as_str()
        };
        row.set_subtitle(&format!(
            "Relevé avant application : {:.1} {unit} · {}",
            scalar.current as f64 / factor,
            scalar
                .limits
                .map(|l| format!(
                    "plage {:.1}–{:.1} {unit}",
                    l.minimum as f64 / factor,
                    l.maximum as f64 / factor
                ))
                .unwrap_or_else(|| "plage absente : lecture seule".into())
        ));
        row.set_tooltip_text(Some(&scalar.source));
        if let Some(limits) = scalar.limits {
            let include = gtk::CheckButton::new();
            include.set_tooltip_text(Some("Inclure ce réglage dans la prochaine application"));
            row.add_prefix(&include);
            let value = spin(limits, scalar.current, factor);
            row.add_suffix(&value);
            editors.scalars.push(ScalarEditor {
                include,
                target: scalar.target,
                spin: value,
                factor,
            });
        }
        match scalar.target {
            ScalarTarget::Fan { .. } => cooling.add(&row),
            ScalarTarget::PowerCap { .. } => energy.add(&row),
            ScalarTarget::VoltageOffset {} => clocks_group.add(&row),
        }
    }
    if let Some(mode) = &catalog.performance {
        let row = adw::ActionRow::new();
        row.set_title("Mode DPM");
        row.set_subtitle(&format!("Actif : {mode}. Modes documentés ; acceptation vérifiée après écriture. Le pilote n’énumère pas toujours les modes disponibles."));
        row.set_subtitle_lines(0);
        row.set_title_lines(0);
        let include = gtk::CheckButton::new();
        include.set_tooltip_text(Some("Inclure le mode DPM"));
        row.add_prefix(&include);
        let choice = gtk::DropDown::from_strings(&PERFORMANCE_MODES);
        choice.set_selected(
            PERFORMANCE_MODES
                .iter()
                .position(|v| *v == mode)
                .unwrap_or(0) as u32,
        );
        row.add_suffix(&choice);
        energy.add(&row);
        editors.performance = Some((include, choice));
    }
    if !catalog.workloads.is_empty() {
        let row = adw::ActionRow::new();
        row.set_title("Profil de charge énergétique");
        row.set_subtitle(
            "Liste réellement lue dans pp_power_profile_mode ; nécessite le mode DPM manuel.",
        );
        row.set_subtitle_lines(0);
        row.set_title_lines(0);
        let include = gtk::CheckButton::new();
        include.set_tooltip_text(Some("Inclure le profil de charge"));
        row.add_prefix(&include);
        let names = catalog
            .workloads
            .iter()
            .map(|p| format!("{} · {}", p.index, p.name))
            .collect::<Vec<_>>();
        let refs = names.iter().map(String::as_str).collect::<Vec<_>>();
        let choice = gtk::DropDown::from_strings(&refs);
        choice.set_selected(catalog.workloads.iter().position(|p| p.active).unwrap_or(0) as u32);
        row.add_suffix(&choice);
        energy.add(&row);
        editors.workload = Some((
            include,
            choice,
            catalog.workloads.iter().map(|p| p.index).collect(),
        ));
    }
    for clock in &catalog.clocks {
        let Some(limits) = clock.limits else { continue };
        let row = adw::ActionRow::new();
        row.set_title(if clock.domain == ClockDomain::Gpu {
            "Horloge GPU min/max"
        } else {
            "Horloge VRAM min/max"
        });
        row.set_subtitle(&format!(
            "{}–{} MHz · OverDrive RDNA · mode manuel requis",
            limits.minimum, limits.maximum
        ));
        let include = gtk::CheckButton::new();
        include.set_tooltip_text(Some("Inclure les fréquences avancées"));
        row.add_prefix(&include);
        let min = clock.minimum.map(|v| {
            let s = spin(limits, v, 1.);
            s.set_tooltip_text(Some("Fréquence minimale en MHz"));
            s
        });
        let max = clock.maximum.map(|v| {
            let s = spin(limits, v, 1.);
            s.set_tooltip_text(Some("Fréquence maximale en MHz"));
            s
        });
        let make_bound = |input: Option<gtk::SpinButton>, title: &str| {
            input.map(|input| {
                let include = gtk::CheckButton::with_label(title);
                include.set_active(true);
                let container = gtk::Box::new(gtk::Orientation::Horizontal, 4);
                container.append(&include);
                container.append(&input);
                row.add_suffix(&container);
                (include, input)
            })
        };
        let minimum = make_bound(min, "Min");
        let maximum = make_bound(max, "Max");
        clocks_group.add(&row);
        editors.clocks.push(ClockEditor {
            include,
            domain: clock.domain,
            minimum,
            maximum,
        });
    }
    if let Some(curve) = &catalog.curve {
        let box_ = gtk::Box::new(gtk::Orientation::Vertical, 10);
        box_.add_css_class("gs-gpu-surface");
        box_.append(&label("Courbe de ventilation · Junction / Hotspot"));
        let auto = gtk::CheckButton::with_label(
            "Rétablir la courbe automatique du firmware (pas un reset GPU)",
        );
        box_.append(&auto);
        editors.automatic = Some(auto);
        editors.curve = curve_editor(curve, &box_);
        content.append(&box_);
    }
    if !catalog.unavailable.is_empty() {
        let expander = gtk::Expander::new(Some("Réglages absents ou formats non pris en charge"));
        let details = label(&catalog.unavailable.join("\n"));
        details.set_selectable(true);
        expander.set_child(Some(&details));
        content.append(&expander);
    }
    editors
}
impl Editors {
    fn load(&mut self, request: &Request) {
        for operation in &request.operations {
            match operation {
                Operation::Scalar { target, value } => {
                    if let Some(e) = self.scalars.iter().find(|e| e.target == *target) {
                        e.include.set_active(true);
                        e.spin.set_value(*value as f64 / e.factor);
                    }
                }
                Operation::FanCurve { points } => {
                    if let Some(e) = &self.curve {
                        e.include.set_active(true);
                        for ((t, p), point) in e.points.iter().zip(points) {
                            t.set_value(point.temperature as f64);
                            p.set_value(point.pwm_percent as f64);
                        }
                    }
                }
                Operation::FanAutomatic {} => {
                    if let Some(e) = &self.automatic {
                        e.set_active(true);
                    }
                }
                Operation::Performance { mode } => {
                    if let Some((include, choice)) = &self.performance {
                        if let Some(index) = PERFORMANCE_MODES.iter().position(|m| m == mode) {
                            include.set_active(true);
                            choice.set_selected(index as u32);
                        }
                    }
                }
                Operation::Workload { index } => {
                    if let Some((include, choice, ids)) = &self.workload {
                        if let Some(i) = ids.iter().position(|id| id == index) {
                            include.set_active(true);
                            choice.set_selected(i as u32);
                        }
                    }
                }
                Operation::Clocks {
                    domain,
                    minimum,
                    maximum,
                } => {
                    if let Some(editor) = self.clocks.iter().find(|e| e.domain == *domain) {
                        editor.include.set_active(true);
                        for (bound, value) in
                            [(&editor.minimum, minimum), (&editor.maximum, maximum)]
                        {
                            if let Some((include, input)) = bound {
                                include.set_active(value.is_some());
                                if let Some(value) = value {
                                    input.set_value(*value as f64);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
pub fn panel(selected: Rc<RefCell<Option<String>>>) -> gtk::Box {
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 12);
    panel.append(&label("Vos réglages restent actifs grâce à l’agent de profils, même après fermeture de la fenêtre. Cocher uniquement les paramètres à configurer."));
    panel.append(&crate::profiles_page::activation());
    let bar = gtk::FlowBox::new();
    bar.set_selection_mode(gtk::SelectionMode::None);
    bar.set_max_children_per_line(4);
    let reload = gtk::Button::with_label("Relire le GPU");
    let apply = gtk::Button::with_label("Appliquer");
    apply.add_css_class("suggested-action");
    let save = gtk::Button::with_label("Enregistrer le profil");
    let automatic = gtk::Button::with_label("Retour au pilote automatique");
    for button in [&reload, &save, &apply, &automatic] {
        bar.insert(button, -1);
    }
    panel.append(&bar);
    let name = gtk::Entry::new();
    name.set_placeholder_text(Some("Nom du profil, par exemple Gaming"));
    panel.append(&name);
    let advanced = gtk::CheckButton::with_label(
        "Inclure les réglages avancés sélectionnés (fréquences / tension)",
    );
    panel.append(&advanced);
    let recovery = gtk::DropDown::from_strings(&[
        "Récupération : ventilation automatique",
        "Récupération : courbe initiale lue",
    ]);
    panel.append(&recovery);
    panel.append(&label("La courbe PMFW utilise Junction / Hotspot. Son capteur est imposé par le firmware. Le mode interne PMFW n’est pas déduit des points ; la récupération reste explicite."));
    let status = label("Lecture des capacités à la demande…");
    status.set_selectable(true);
    panel.append(&status);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 14);
    panel.append(&content);
    let catalog = Rc::new(RefCell::new(None::<Catalog>));
    let editors = Rc::new(RefCell::new(Editors::default()));
    let editing = Rc::new(RefCell::new(None::<Profile>));
    let loading = Rc::new(Cell::new(false));
    let (tx, rx) = mpsc::channel::<(String, Result<Box<Catalog>>)>();
    {
        let selected = selected.clone();
        let loading = loading.clone();
        let tx = tx.clone();
        let status = status.clone();
        reload.connect_clicked(move |_| {
            if loading.get() {
                return;
            }
            let Some(pci) = selected.borrow().clone() else {
                status.set_text("Aucun GPU AMDGPU sélectionné");
                return;
            };
            loading.set(true);
            let tx = tx.clone();
            std::thread::spawn(move || {
                let value = Catalog::discover(&pci).map(Box::new);
                let _ = tx.send((pci, value));
            });
        });
    }
    for (button, activate) in [(save, false), (apply.clone(), true)] {
        let catalog = catalog.clone();
        let editors = editors.clone();
        let editing = editing.clone();
        let name = name.clone();
        let advanced = advanced.clone();
        let recovery = recovery.clone();
        let status = status.clone();
        let selected = selected.clone();
        button.connect_clicked(move |_| {
            if crate::live_settings::benchmark_running() {
                status.set_text("Attendre la fin du benchmark avant de modifier le profil.");
                return;
            }
            let Some(catalog) = catalog.borrow().clone() else {
                status.set_text("Relire les capacités du GPU");
                return;
            };
            if selected.borrow().as_deref() != Some(catalog.identity.pci.as_str()) {
                status.set_text("La sélection GPU a changé ; relire ses réglages");
                return;
            }
            let request = Request {
                schema_version: 1,
                advanced_confirmed: advanced.is_active(),
                identity: catalog.identity.clone(),
                fan_recovery: if recovery.selected() == 0 {
                    FanRecovery::Automatic
                } else {
                    FanRecovery::InitialCurve
                },
                operations: editors.borrow().operations(),
            };
            if let Err(error) = check_request(&catalog, &request) {
                status.set_text(&error);
                return;
            }
            let profile_name = if name.text().trim().is_empty() {
                "Personnalisé".into()
            } else {
                name.text().trim().to_owned()
            };
            let id = editing
                .borrow()
                .as_ref()
                .map(|p| p.id.clone())
                .unwrap_or_else(|| {
                    if activate {
                        format!("manual-{}", catalog.identity.pci.replace([':', '.'], "_"))
                    } else {
                        gameshift_profiles::new_id().unwrap_or_default()
                    }
                });
            let profile = Profile {
                id: id.clone(),
                name: profile_name,
                request,
            };
            let saved = profile.clone();
            *editing.borrow_mut() = Some(profile.clone());
            crate::profiles_page::profile_task(&status, move || {
                if activate {
                    gameshift_profiles::component()?;
                    gameshift_profiles::start()?;
                }
                let saved_id = id.clone();
                crate::profiles_page::update(move |config| {
                    if let Some(old) = config.profiles.iter_mut().find(|p| p.id == profile.id) {
                        *old = profile;
                    } else {
                        config.profiles.push(profile);
                    }
                    if activate {
                        config.mode = Mode::Manual { profile: id };
                    }
                    Ok(())
                })?;
                let active_saved = gameshift_profiles::call(serde_json::json!({"command":"get"}))
                    .is_ok_and(|s| s["active"].as_str() == Some(saved_id.as_str()));
                if activate || active_saved {
                    gameshift_profiles::wait_transition()?;
                }
                Ok(if activate {
                    "Profil appliqué et relu par le contrôleur. Mode manuel actif."
                } else {
                    "Profil enregistré. Associer une application depuis Profils."
                }
                .into())
            });
            name.set_text(&saved.name);
        });
    }
    automatic.connect_clicked({
        let status = status.clone();
        move |_| {
            crate::profiles_page::task(&status, || {
                gameshift_profiles::call(serde_json::json!({"command":"driver_automatic"}))?;
                Ok(
                    "Retour manuel au pilote automatique demandé ; règles et profils conservés."
                        .into(),
                )
            })
        }
    });
    let weak = panel.downgrade();
    let mut last_selected = None::<String>;
    glib::timeout_add_local(Duration::from_millis(150), move || {
        if weak.upgrade().is_none() {
            return glib::ControlFlow::Break;
        }
        let current = selected.borrow().clone();
        if current != last_selected && !loading.get() {
            *catalog.borrow_mut() = None;
            *editing.borrow_mut() = None;
            name.set_text("");
            reload.emit_clicked();
            last_selected = current.clone();
        }
        while let Ok((pci, result)) = rx.try_recv() {
            loading.set(false);
            if current.as_deref() != Some(&pci) {
                continue;
            }
            match result {
                Ok(value) => {
                    *editors.borrow_mut() = build_editors(&value, &content);
                    *catalog.borrow_mut() = Some(*value);
                    status.set_text("Capacités relues. Les valeurs affichées sont le relevé initial ; voir Profils pour l’état de l’agent.");
                }
                Err(error) => status.set_text(&error),
            }
        }
        if NEW_PROFILE.with(|v| v.replace(false)) {
            *editing.borrow_mut() = None;
            name.set_text("");
        }
        if !loading.get() {
            let pending = EDIT_PROFILE.with_borrow_mut(Option::take);
            if let Some(profile) = pending {
                if current.as_deref() != Some(profile.request.identity.pci.as_str()) {
                    status.set_text(&format!(
                        "Sélectionner le GPU {} pour modifier ce profil.",
                        profile.request.identity.pci
                    ));
                    EDIT_PROFILE.with_borrow_mut(|p| *p = Some(profile));
                } else if let Some(catalog) = catalog.borrow().as_ref() {
                    match check_request(catalog, &profile.request) {
                        Ok(()) => {
                            let mut loaded = build_editors(catalog, &content);
                            loaded.load(&profile.request);
                            *editors.borrow_mut() = loaded;
                            name.set_text(&profile.name);
                            advanced.set_active(profile.request.advanced_confirmed);
                            recovery.set_selected(u32::from(
                                profile.request.fan_recovery == FanRecovery::InitialCurve,
                            ));
                            status.set_text("Profil chargé dans les éditeurs. Enregistrer met à jour le profil ; un profil déjà actif sera réappliqué. Appliquer le sélectionne manuellement.");
                            *editing.borrow_mut() = Some(profile);
                        }
                        Err(error) => status.set_text(&format!(
                            "Profil incompatible avec les capacités actuelles : {error}"
                        )),
                    }
                } else {
                    EDIT_PROFILE.with_borrow_mut(|p| *p = Some(profile));
                }
            }
        }
        reload.set_sensitive(!loading.get());
        apply.set_sensitive(!loading.get() && !crate::live_settings::benchmark_running());
        glib::ControlFlow::Continue
    });
    panel
}
