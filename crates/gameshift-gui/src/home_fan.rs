//! Home fan card: local drafts, explicit apply, existing validated profile transport.
use adw::prelude::*;
use gameshift_control::{
    check_request, Catalog, Curve, FanRecovery, FanSetting, Limits, Operation, Point, Request,
    ScalarTarget,
};
use gameshift_profiles::{Config, Mode, Profile};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::mpsc,
    time::Duration,
};
fn label(text: &str, class: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class(class);
    l.set_xalign(0.);
    l.set_wrap(true);
    l
}
fn equal(a: &Curve, b: &Curve) -> bool {
    a.points == b.points && a.temperature == b.temperature && a.pwm_percent == b.pwm_percent
}
fn coordinates(point: &Point, t: Limits, w: f64, h: f64) -> (f64, f64) {
    (
        38. + (w - 50.).max(1.) * (point.temperature - t.minimum) as f64
            / (t.maximum - t.minimum).max(1) as f64,
        12. + (h - 40.).max(1.) * (1. - point.pwm_percent as f64 / 100.),
    )
}
fn moved(
    points: &[Point],
    index: usize,
    position: (f64, f64),
    t: Limits,
    p: Limits,
    size: (f64, f64),
) -> Option<Point> {
    let (x, y) = position;
    let (w, h) = size;
    if index >= points.len() || !x.is_finite() || !y.is_finite() {
        return None;
    }
    let temperature = (t.minimum as f64
        + ((x - 38.) / (w - 50.).max(1.)).clamp(0., 1.) * (t.maximum - t.minimum) as f64)
        .round() as i64;
    let pwm = (100. * (1. - ((y - 12.) / (h - 40.).max(1.)).clamp(0., 1.))).round() as i64;
    Some(Point {
        temperature: temperature.clamp(
            if index == 0 {
                t.minimum
            } else {
                points[index - 1].temperature
            },
            points
                .get(index + 1)
                .map(|q| q.temperature)
                .unwrap_or(t.maximum),
        ),
        pwm_percent: pwm.clamp(
            if index == 0 {
                p.minimum
            } else {
                points[index - 1].pwm_percent
            },
            points
                .get(index + 1)
                .map(|q| q.pwm_percent)
                .unwrap_or(p.maximum),
        ),
    })
}
fn draft(curve: &Curve) -> Option<Vec<Point>> {
    if curve.validate(&curve.points).is_ok() {
        return Some(curve.points.clone());
    }
    let (t, p) = (curve.temperature?, curve.pwm_percent?);
    if curve.points.is_empty() || t.minimum > t.maximum || p.minimum > p.maximum {
        return None;
    }
    let last = curve.points.len().saturating_sub(1).max(1) as i64;
    Some(
        (0..curve.points.len())
            .map(|i| Point {
                temperature: t.minimum + (t.maximum - t.minimum) * i as i64 / last,
                pwm_percent: p.minimum + (p.maximum - p.minimum) * i as i64 / last,
            })
            .collect(),
    )
}
fn fan_operation(op: &Operation) -> bool {
    matches!(
        op,
        Operation::FanCurve { .. }
            | Operation::FanAutomatic {}
            | Operation::Scalar {
                target: ScalarTarget::Fan {
                    setting: FanSetting::AcousticLimit
                        | FanSetting::AcousticTarget
                        | FanSetting::TargetTemperature
                        | FanSetting::MinimumPwm
                },
                ..
            }
    )
}
fn merge_request(
    catalog: &Catalog,
    prior: Option<&Request>,
    automatic: bool,
    points: Vec<Point>,
) -> Result<Request, String> {
    let mut request = prior.cloned().unwrap_or_else(|| Request {
        schema_version: 1,
        advanced_confirmed: false,
        identity: catalog.identity.clone(),
        fan_recovery: FanRecovery::Automatic,
        operations: Vec::new(),
    });
    if request.identity != catalog.identity {
        return Err("L’identité du GPU a changé".into());
    }
    request.operations.retain(|op| !fan_operation(op));
    request.operations.push(if automatic {
        Operation::FanAutomatic {}
    } else {
        Operation::FanCurve { points }
    });
    check_request(catalog, &request)?;
    Ok(request)
}
fn apply_draft(
    pci: String,
    baseline: Curve,
    automatic: bool,
    points: Vec<Point>,
) -> Result<String, String> {
    gameshift_profiles::component()?;
    gameshift_profiles::start()?;
    if gameshift_control::benchmark::benchmark_active() {
        return Err("Attendre la fin du benchmark".into());
    }
    let catalog = Catalog::discover(&pci)?;
    if !catalog.curve.as_ref().is_some_and(|c| equal(c, &baseline)) {
        return Err(
            "La courbe ou ses limites ont changé : annuler et relire avant d’appliquer".into(),
        );
    }
    let state = gameshift_profiles::call(serde_json::json!({"command":"get"}))?;
    if state["pending"] == true || !state["error"].is_null() {
        return Err("L’agent est occupé ou en erreur ; vérifier Profils avant d’appliquer".into());
    }
    let mut config: Config =
        serde_json::from_value(state["config"].clone()).map_err(|e| e.to_string())?;
    let active = state["active"]
        .as_str()
        .and_then(|id| config.profiles.iter().find(|p| p.id == id));
    if active.is_some_and(|p| p.request.identity.pci != pci) {
        return Err("Un profil d’un autre GPU est actif ; utiliser l’onglet GPU/Profils".into());
    }
    if state["active"].as_str().is_some() && active.is_none() {
        return Err("Le profil actif a changé ; relire avant d’appliquer".into());
    }
    let request = merge_request(&catalog, active.map(|p| &p.request), automatic, points)?;
    let id = format!("home-fan-{}", pci.replace([':', '.'], "_"));
    let name = "Accueil · ventilation";
    if config.profiles.iter().any(|p| p.id == id && p.name != name) {
        return Err(
            "Identifiant de profil déjà utilisé ; les données existantes sont conservées".into(),
        );
    }
    let profile = Profile {
        id: id.clone(),
        name: name.into(),
        request,
    };
    if let Some(old) = config.profiles.iter_mut().find(|p| p.id == id) {
        *old = profile;
    } else {
        config.profiles.push(profile);
    }
    config.mode = Mode::Manual {
        profile: id.clone(),
    };
    config.validate()?;
    gameshift_profiles::call(
        serde_json::json!({"command":"configure","config":config,"revision":state["revision"]}),
    )?;
    gameshift_profiles::wait_transition()?;
    let after = gameshift_profiles::call(serde_json::json!({"command":"get"}))?;
    if after["active"].as_str() != Some(&id) || !after["error"].is_null() {
        return Err("Application non confirmée par l’agent : vérifier Profils".into());
    }
    Ok("Ventilation appliquée et relue · mode manuel du profil actif".into())
}
fn load_profiles(inner: &Rc<Inner>) {
    let (tx, rx) = mpsc::channel();
    let loading = Cell::new(false);
    let weak = Rc::downgrade(inner);
    gtk::glib::timeout_add_local(Duration::from_secs(2), move || {
        let Some(inner) = weak.upgrade() else {
            return gtk::glib::ControlFlow::Break;
        };
        if let Ok(result) = rx.try_recv() {
            loading.set(false);
            if let Ok(config) = result {
                let config: Config = config;
                *inner.profiles.borrow_mut() = config.profiles;
            }
        }
        if !inner.state.borrow().dirty && !inner.state.borrow().busy {
            let profiles: Vec<Profile> = inner
                .profiles
                .borrow()
                .iter()
                .filter(|p| Some(&p.request.identity.pci) == inner.state.borrow().pci.as_ref())
                .cloned()
                .collect();
            let mut labels = vec!["Non déterminé", "Automatique", "Personnalisée"];
            labels.extend(profiles.iter().map(|p| p.name.as_str()));
            let old: Vec<String> = (0..inner.names.n_items())
                .filter_map(|i| inner.names.string(i).map(|s| s.to_string()))
                .collect();
            if old.iter().map(String::as_str).collect::<Vec<_>>() != labels
                || inner
                    .entries
                    .borrow()
                    .iter()
                    .map(|p| &p.id)
                    .collect::<Vec<_>>()
                    != profiles.iter().map(|p| &p.id).collect::<Vec<_>>()
            {
                let previous = inner.mode.selected();
                let selected_id = previous
                    .checked_sub(3)
                    .and_then(|i| inner.entries.borrow().get(i as usize).map(|p| p.id.clone()));
                let selection = if previous < 3 {
                    previous
                } else {
                    selected_id
                        .and_then(|id| profiles.iter().position(|p| p.id == id))
                        .map(|i| i as u32 + 3)
                        .unwrap_or(0)
                };
                inner.guard.set(true);
                inner.names.splice(0, inner.names.n_items(), &labels);
                inner.mode.set_selected(selection);
                inner.state.borrow_mut().confirmed_mode = selection;
                *inner.entries.borrow_mut() = profiles;
                inner.guard.set(false);
            }
        }
        if !loading.replace(true) {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(Config::load());
            });
        }
        inner.refresh();
        gtk::glib::ControlFlow::Continue
    });
}
#[derive(Default)]
struct State {
    pci: Option<String>,
    baseline: Option<Curve>,
    dirty: bool,
    conflict: bool,
    busy: bool,
    confirmed_mode: u32,
    signature: Option<String>,
}
struct Inner {
    area: gtk::DrawingArea,
    automatic: gtk::Box,
    automatic_speed: gtk::Label,
    names: gtk::StringList,
    profiles: RefCell<Vec<Profile>>,
    entries: RefCell<Vec<Profile>>,
    mode: gtk::DropDown,
    actions: gtk::Box,
    apply: gtk::Button,
    cancel: gtk::Button,
    status: gtk::Label,
    speed: gtk::Label,
    data: Rc<RefCell<Vec<Point>>>,
    state: RefCell<State>,
    guard: Cell<bool>,
    compact: Cell<bool>,
    crosshair: Cell<bool>,
    selected: Box<dyn Fn() -> Option<String>>,
}
#[derive(Clone)]
pub struct Panel {
    pub widget: gtk::Box,
    inner: Rc<Inner>,
}
impl Inner {
    fn refresh(&self) {
        let state = self.state.borrow();
        let editable = state.baseline.as_ref().is_some_and(|c| {
            c.temperature.is_some() && c.pwm_percent.is_some() && !c.points.is_empty()
        });
        self.mode.set_sensitive(
            (editable || !self.entries.borrow().is_empty())
                && !state.busy
                && !state.conflict
                && !crate::profiles_page::pending_apply(),
        );
        self.actions.set_visible(state.dirty || state.busy);
        self.apply.set_sensitive(
            state.dirty
                && !state.busy
                && !state.conflict
                && !crate::profiles_page::pending_apply()
                && !crate::live_settings::benchmark_running(),
        );
        self.cancel.set_sensitive(!state.busy);
        let crosshair = editable && self.mode.selected() == 2 && !state.busy && !state.conflict;
        if self.crosshair.replace(crosshair) != crosshair {
            self.area
                .set_cursor_from_name(Some(if crosshair { "crosshair" } else { "default" }));
        }
        let show_curve = editable && self.mode.selected() == 2;
        self.area.set_visible(show_curve);
        self.automatic.set_visible(!show_curve);
        let height = if editable {
            if self.compact.get() {
                110
            } else {
                150
            }
        } else {
            64
        };
        if self.area.content_height() != height {
            self.area.set_content_height(height);
        }
    }
    fn render(&self) {
        let state = self.state.borrow();
        if let Some((t, _p)) = state
            .baseline
            .as_ref()
            .and_then(|c| Some((c.temperature?, c.pwm_percent?)))
            .filter(|_| !self.data.borrow().is_empty())
        {
            crate::amd_controls::draw_curve(
                &self.area,
                self.data.clone(),
                t,
                Limits {
                    minimum: 0,
                    maximum: 100,
                },
            );
        } else {
            self.area.set_draw_func(|_, cr, _, h| {
                cr.set_source_rgb(0.55, 0.68, 0.71);
                cr.set_font_size(11.);
                cr.move_to(12., h as f64 / 2.);
                let _ = cr.show_text("Courbe non disponible");
            });
        }
        self.area.queue_draw();
    }
    fn reset(&self) {
        let mut state = self.state.borrow_mut();
        state.dirty = false;
        state.conflict = false;
        *self.data.borrow_mut() = state
            .baseline
            .as_ref()
            .filter(|c| c.validate(&c.points).is_ok())
            .map(|c| c.points.clone())
            .unwrap_or_default();
        self.guard.set(true);
        self.mode.set_selected(state.confirmed_mode);
        self.guard.set(false);
        drop(state);
        self.status.set_text("");
        self.status.set_visible(false);
        self.render();
        self.refresh();
    }
}
impl Panel {
    pub fn new(selected: impl Fn() -> Option<String> + 'static) -> Self {
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 6);
        widget.add_css_class("gs-home-card");
        let heading = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        heading.append(&crate::overview::icon("fan-symbolic", 24));
        heading.append(&label("Courbe de ventilation (GPU)", "gs-home-heading"));
        widget.append(&heading);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let area = gtk::DrawingArea::new();
        area.set_hexpand(true);
        area.set_content_height(132);
        area.add_css_class("gs-live-chart");
        row.append(&area);
        let automatic = gtk::Box::new(gtk::Orientation::Vertical, 8);
        automatic.set_hexpand(true);
        automatic.set_halign(gtk::Align::Fill);
        automatic.set_valign(gtk::Align::Center);
        let fan = crate::overview::icon("fan-symbolic", 52);
        fan.set_halign(gtk::Align::Center);
        automatic.append(&fan);
        let automatic_speed = label("— tr/min", "gs-home-fan-speed");
        automatic_speed.set_halign(gtk::Align::Center);
        automatic.append(&automatic_speed);
        let explanation = label(
            "Vitesse mesurée en temps réel\nLa courbe de réglage s’affiche en mode personnalisé.",
            "gs-home-caption",
        );
        explanation.set_justify(gtk::Justification::Center);
        explanation.set_halign(gtk::Align::Center);
        automatic.append(&explanation);
        row.append(&automatic);
        let controls = gtk::Box::new(gtk::Orientation::Vertical, 8);
        controls.set_width_request(150);
        controls.append(&label("Ventilation", "gs-home-caption"));
        let names = gtk::StringList::new(&["Non déterminé", "Automatique", "Personnalisée"]);
        let mode = gtk::DropDown::new(Some(names.clone()), None::<gtk::Expression>);
        mode.add_css_class("gs-home-fan-mode");
        mode.set_tooltip_text(Some("Le mode du firmware n’est pas déduit de la courbe. Choisir un mode prépare un brouillon ; Appliquer utilise l’agent et ses validations."));
        controls.append(&mode);
        let speed_row = gtk::Box::new(gtk::Orientation::Horizontal, 7);
        speed_row.append(&crate::overview::icon("fan-symbolic", 30));
        let speed_box = gtk::Box::new(gtk::Orientation::Vertical, 3);
        speed_box.append(&label("Vitesse actuelle", "gs-home-caption"));
        let speed = label("— tr/min", "gs-home-fan-speed");
        speed_box.append(&speed);
        speed_row.append(&speed_box);
        controls.append(&speed_row);
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let apply = gtk::Button::with_label("Appliquer");
        apply.add_css_class("gs-home-detail-action");
        apply.set_tooltip_text(Some("Appliquer utilise un profil manuel de ventilation, conserve les autres réglages du profil actif et suspend la sélection automatique des profils jusqu’au retour au mode automatique dans Profils."));
        let cancel = gtk::Button::with_label("Annuler");
        cancel.add_css_class("gs-home-detail-action");
        actions.append(&apply);
        actions.append(&cancel);
        controls.append(&actions);
        row.append(&controls);
        widget.append(&row);
        let status = label("", "gs-home-caption");
        status.set_visible(false);
        widget.append(&status);
        let inner = Rc::new(Inner {
            area,
            automatic,
            automatic_speed,
            names,
            profiles: RefCell::new(Vec::new()),
            entries: RefCell::new(Vec::new()),
            mode,
            actions,
            apply,
            cancel,
            status,
            speed,
            data: Rc::new(RefCell::new(Vec::new())),
            state: RefCell::new(State::default()),
            guard: Cell::new(false),
            compact: Cell::new(false),
            crosshair: Cell::new(false),
            selected: Box::new(selected),
        });
        let weak = Rc::downgrade(&inner);
        inner.mode.connect_selected_notify(move |mode| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            if inner.guard.get() {
                return;
            }
            if mode.selected() == 0 {
                inner.reset();
                return;
            }
            let mut state = inner.state.borrow_mut();
            if mode.selected() >= 3 {
                state.dirty = true;
                drop(state);
                inner
                    .status
                    .set_text("Profil sélectionné · Appliquer pour activer ses réglages GPU");
                inner.status.set_visible(true);
                inner.refresh();
                return;
            }
            let Some(curve) = &state.baseline else {
                return;
            };
            if mode.selected() == 2 {
                let Some(points) = draft(curve) else {
                    return;
                };
                *inner.data.borrow_mut() = points;
            } else {
                *inner.data.borrow_mut() = if curve.validate(&curve.points).is_ok() {
                    curve.points.clone()
                } else {
                    Vec::new()
                };
            }
            state.dirty = true;
            drop(state);
            inner
                .status
                .set_text("Brouillon non appliqué · déplacer les points puis Appliquer");
            inner.status.set_visible(true);
            inner.render();
            inner.refresh();
        });
        let weak = Rc::downgrade(&inner);
        inner.cancel.connect_clicked(move |_| {
            if let Some(inner) = weak.upgrade() {
                inner.reset();
            }
        });
        let drag = gtk::GestureDrag::new();
        let index = Rc::new(Cell::new(None));
        let origin = Rc::new(Cell::new((0., 0.)));
        let weak = Rc::downgrade(&inner);
        let chosen = index.clone();
        let start = origin.clone();
        drag.connect_drag_begin(move |_, x, y| {
            chosen.set(None);
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let state = inner.state.borrow();
            if state.busy || state.conflict || inner.mode.selected() != 2 {
                return;
            }
            let Some((t, _p)) = state
                .baseline
                .as_ref()
                .and_then(|c| Some((c.temperature?, c.pwm_percent?)))
            else {
                return;
            };
            let best = inner
                .data
                .borrow()
                .iter()
                .enumerate()
                .map(|(i, q)| {
                    let (px, py) =
                        coordinates(q, t, inner.area.width() as f64, inner.area.height() as f64);
                    (i, (px - x).powi(2) + (py - y).powi(2))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1));
            chosen.set(best.filter(|(_, d)| *d <= 400.).map(|(i, _)| i));
            start.set((x, y));
        });
        let weak = Rc::downgrade(&inner);
        drag.connect_drag_update(move |_, dx, dy| {
            let (Some(inner), Some(i)) = (weak.upgrade(), index.get()) else {
                return;
            };
            let state = inner.state.borrow();
            if state.busy
                || state.conflict
                || inner.mode.selected() != 2
                || state.pci.as_deref() != (inner.selected)().as_deref()
            {
                return;
            }
            let Some((t, p)) = state
                .baseline
                .as_ref()
                .and_then(|c| Some((c.temperature?, c.pwm_percent?)))
            else {
                return;
            };
            let (x, y) = origin.get();
            let point = moved(
                &inner.data.borrow(),
                i,
                (x + dx, y + dy),
                t,
                p,
                (inner.area.width() as f64, inner.area.height() as f64),
            );
            drop(state);
            if let Some(point) = point {
                inner.data.borrow_mut()[i] = point;
                inner.state.borrow_mut().dirty = true;
                inner.status.set_text("Brouillon non appliqué");
                inner.status.set_visible(true);
                inner.area.queue_draw();
                inner.refresh();
            }
        });
        inner.area.add_controller(drag);
        let (tx, rx) = mpsc::channel();
        let weak = Rc::downgrade(&inner);
        inner.apply.connect_clicked(move |_| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            if crate::live_settings::benchmark_running() || crate::profiles_page::pending_apply() {
                return;
            }
            let mut state = inner.state.borrow_mut();
            if !state.dirty || state.busy || state.conflict {
                return;
            }
            if inner.mode.selected() >= 3 {
                let selected = inner.mode.selected();
                let Some(profile) = inner.entries.borrow().get((selected - 3) as usize).cloned()
                else {
                    return;
                };
                if (inner.selected)().as_deref() != Some(profile.request.identity.pci.as_str()) {
                    return;
                }
                state.busy = true;
                drop(state);
                inner.status.set_visible(true);
                inner.refresh();
                let tx = tx.clone();
                crate::profiles_page::profile_task(&inner.status, move || {
                    let result = (|| {
                        gameshift_profiles::component()?;
                        gameshift_profiles::start()?;
                        crate::profiles_page::update(move |config| {
                            config.mode = Mode::Manual {
                                profile: profile.id,
                            };
                            Ok(())
                        })?;
                        gameshift_profiles::wait_transition()?;
                        Ok("Profil appliqué".into())
                    })();
                    let _ = tx.send((selected, result.clone()));
                    result
                });
                return;
            }
            let (Some(pci), Some(curve)) = (state.pci.clone(), state.baseline.clone()) else {
                return;
            };
            if (inner.selected)().as_deref() != Some(pci.as_str()) {
                inner
                    .status
                    .set_text("La sélection GPU a changé ; annuler le brouillon");
                inner.status.set_visible(true);
                return;
            }
            let automatic = inner.mode.selected() == 1;
            let points = inner.data.borrow().clone();
            if !automatic {
                if let Err(e) = curve.validate(&points) {
                    inner.status.set_text(&e);
                    inner.status.set_visible(true);
                    return;
                }
            }
            state.busy = true;
            drop(state);
            inner.refresh();
            let tx = tx.clone();
            crate::profiles_page::profile_task(&inner.status, move || {
                let result = apply_draft(pci, curve, automatic, points);
                let _ = tx.send((if automatic { 1 } else { 2 }, result.clone()));
                result
            });
        });
        let weak = Rc::downgrade(&inner);
        gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
            let Some(inner) = weak.upgrade() else {
                return gtk::glib::ControlFlow::Break;
            };
            let result = rx.try_recv();
            if let Ok((selected, result)) = result {
                let mut state = inner.state.borrow_mut();
                state.busy = false;
                if result.is_ok() {
                    state.dirty = false;
                    state.conflict = false;
                    state.confirmed_mode = selected;
                    state.signature = None;
                }
                drop(state);
                inner.status.set_text(&result.unwrap_or_else(|e| e));
                inner.status.set_visible(true);
            }
            if inner.state.borrow().busy && !crate::profiles_page::pending_apply() {
                inner.state.borrow_mut().busy = false;
                inner
                    .status
                    .set_text("Application interrompue · vérifier Profils avant de réessayer");
                inner.status.set_visible(true);
            }
            inner.refresh();
            gtk::glib::ControlFlow::Continue
        });
        load_profiles(&inner);
        inner.render();
        inner.refresh();
        Self { widget, inner }
    }
    pub fn adapt(&self, compact: bool) {
        if self.inner.compact.replace(compact) != compact {
            self.inner.refresh();
        }
    }
    pub fn update(
        &self,
        pci: Option<&String>,
        curve: Option<Curve>,
        rpm: Option<f64>,
        pwm: Option<f64>,
    ) {
        let inner = &self.inner;
        let speed = rpm
            .filter(|v| v.is_finite() && *v >= 0.)
            .map(|v| format!("{v:.0} tr/min"))
            .unwrap_or_else(|| "— tr/min".into());
        inner.speed.set_text(
            &pwm.filter(|v| v.is_finite() && (0. ..=100.).contains(v))
                .map(|v| format!("{speed} ({v:.0} % PWM)"))
                .unwrap_or(speed),
        );
        let mut state = inner.state.borrow_mut();
        let changed = state.pci.as_ref() != pci;
        inner.automatic_speed.set_text(&inner.speed.text());
        if changed {
            inner.status.set_text("");
            inner.status.set_visible(false);
            state.pci = pci.cloned();
            state.dirty = false;
            state.conflict = false;
            state.confirmed_mode = 0;
            inner.guard.set(true);
            inner.mode.set_selected(0);
            inner.guard.set(false);
        }
        let signature = curve.as_ref().and_then(|c| serde_json::to_string(c).ok());
        if changed || signature != state.signature {
            if state.signature.is_some() && signature != state.signature {
                state.confirmed_mode = 0;
                if !state.dirty {
                    inner.guard.set(true);
                    inner.mode.set_selected(0);
                    inner.guard.set(false);
                }
            }
            if state.dirty && !state.busy {
                state.conflict = true;
                inner
                    .status
                    .set_text("Courbe modifiée ailleurs · Annuler pour relire");
                inner.status.set_visible(true);
            }
            state.signature = signature;
            state.baseline = curve;
            if !state.dirty {
                *inner.data.borrow_mut() = state
                    .baseline
                    .as_ref()
                    .filter(|c| c.validate(&c.points).is_ok())
                    .map(|c| c.points.clone())
                    .unwrap_or_default();
            }
            drop(state);
            inner.render();
        } else {
            drop(state);
        }
        inner.refresh();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn curve() -> Curve {
        Curve {
            points: vec![
                Point {
                    temperature: 30,
                    pwm_percent: 10,
                },
                Point {
                    temperature: 60,
                    pwm_percent: 40,
                },
                Point {
                    temperature: 100,
                    pwm_percent: 90,
                },
            ],
            temperature: Some(Limits {
                minimum: 30,
                maximum: 100,
            }),
            pwm_percent: Some(Limits {
                minimum: 0,
                maximum: 100,
            }),
        }
    }
    #[test]
    fn dragging_preserves_driver_ranges_and_neighbour_order() {
        let c = curve();
        let t = c.temperature.unwrap();
        let p = c.pwm_percent.unwrap();
        let q = moved(&c.points, 1, (-100., 1000.), t, p, (400., 160.)).unwrap();
        assert_eq!(
            q,
            Point {
                temperature: 30,
                pwm_percent: 10
            }
        );
        let q = moved(&c.points, 1, (10000., -1000.), t, p, (400., 160.)).unwrap();
        assert_eq!(
            q,
            Point {
                temperature: 100,
                pwm_percent: 90
            }
        );
        for (index, point) in c.points.iter().enumerate() {
            let (x, y) = coordinates(point, t, 400., 160.);
            assert_eq!(
                moved(&c.points, index, (x, y), t, p, (400., 160.)).unwrap(),
                *point
            );
        }
        assert!(moved(&c.points, 0, (f64::NAN, 0.), t, p, (400., 160.)).is_none());
    }
    #[test]
    fn editing_a_draft_never_changes_the_read_baseline() {
        let original = curve();
        let mut points = draft(&original).unwrap();
        points[1].temperature = 65;
        assert_eq!(original.points[1].temperature, 60);
        let mut automatic = original.clone();
        automatic.points.iter_mut().for_each(|p| {
            p.temperature = 0;
            p.pwm_percent = 0;
        });
        let points = draft(&automatic).unwrap();
        assert!(automatic.validate(&points).is_ok());
        assert_eq!(automatic.points[0].temperature, 0);
        automatic.temperature = None;
        assert!(draft(&automatic).is_none());
    }
    #[test]
    fn inline_fan_requests_preserve_non_fan_settings_and_do_not_mutate_the_source() {
        use gameshift_control::{Identity, Scalar};
        let identity = Identity {
            pci: "0000:05:00.0".into(),
            vendor: "0x1002".into(),
            device: "0x747e".into(),
            revision: "0x01".into(),
            subsystem_vendor: "0x1002".into(),
            subsystem_device: "0x0001".into(),
            unique_id: None,
        };
        let catalog = Catalog {
            identity: identity.clone(),
            scalars: vec![Scalar {
                target: ScalarTarget::PowerCap { channel: 1 },
                label: "Puissance".into(),
                unit: "µW".into(),
                source: "hwmon/power1_cap".into(),
                current: 100_000_000,
                limits: Some(Limits {
                    minimum: 50_000_000,
                    maximum: 200_000_000,
                }),
            }],
            curve: Some(curve()),
            performance: Some("auto".into()),
            workloads: vec![],
            clocks: vec![],
            unavailable: vec![],
        };
        let prior = Request {
            schema_version: 1,
            identity,
            advanced_confirmed: false,
            fan_recovery: FanRecovery::Automatic,
            operations: vec![
                Operation::Performance {
                    mode: "auto".into(),
                },
                Operation::Scalar {
                    target: ScalarTarget::PowerCap { channel: 1 },
                    value: 100_000_000,
                },
                Operation::FanAutomatic {},
            ],
        };
        let request = merge_request(&catalog, Some(&prior), false, curve().points).unwrap();
        assert!(request
            .operations
            .iter()
            .any(|op| matches!(op,Operation::Performance{mode} if mode=="auto")));
        assert!(request.operations.iter().any(|op| matches!(
            op,
            Operation::Scalar {
                target: ScalarTarget::PowerCap { .. },
                value: 100_000_000
            }
        )));
        assert!(request
            .operations
            .iter()
            .any(|op| matches!(op, Operation::FanCurve { .. })));
        assert!(!request
            .operations
            .iter()
            .any(|op| matches!(op, Operation::FanAutomatic {})));
        assert!(matches!(
            prior.operations.last().unwrap(),
            Operation::FanAutomatic {}
        ));
        let mut wrong = prior.clone();
        wrong.identity.pci = "0000:08:00.0".into();
        assert!(merge_request(&catalog, Some(&wrong), false, curve().points).is_err());
    }
}
