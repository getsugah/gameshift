//! Aurelia workspace: one summary, recorded data, details on demand.
use crate::aurelia_results::SystemResult;
use adw::prelude::*;

fn label(value: &str, class: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(value));
    l.set_wrap(true);
    l.set_xalign(0.);
    l.set_selectable(true);
    l.add_css_class(class);
    l
}
fn column(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, spacing)
}
fn native_info_icon(kind: &str) -> gtk::DrawingArea {
    let icon = gtk::DrawingArea::new();
    icon.set_content_width(20);
    icon.set_content_height(20);
    icon.set_valign(gtk::Align::Center);
    let kind = kind.to_owned();
    icon.set_draw_func(move |widget, c, w, h| {
        let color = widget.color();
        c.set_source_rgba(
            color.red().into(),
            color.green().into(),
            color.blue().into(),
            color.alpha().into(),
        );
        c.scale(w as f64 / 20., h as f64 / 20.);
        c.set_line_width(1.5);
        c.set_line_join(gtk::cairo::LineJoin::Round);
        match kind.as_str() {
            "calendar" => {
                c.rectangle(3., 4., 14., 13.);
                c.move_to(3., 8.);
                c.line_to(17., 8.);
                for x in [6., 14.] {
                    c.move_to(x, 2.);
                    c.line_to(x, 6.);
                }
                for x in [6., 10., 14.] {
                    c.move_to(x, 10.);
                    c.line_to(x, 15.);
                }
            }
            "clock" => {
                c.arc(10., 10., 7.5, 0., std::f64::consts::TAU);
                c.move_to(10., 5.);
                c.line_to(10., 10.);
                c.line_to(14., 12.);
            }
            "check" => {
                c.move_to(3., 10.);
                c.line_to(8., 15.);
                c.line_to(17., 5.);
                c.set_line_width(2.2);
            }
            "chart" => {
                for (x, y) in [(3., 12.), (8., 8.), (13., 4.)] {
                    c.rectangle(x, y, 3., 16. - y);
                }
            }
            "monitor" => {
                c.rectangle(2., 3., 16., 11.);
                c.move_to(10., 14.);
                c.line_to(10., 18.);
                c.move_to(6., 18.);
                c.line_to(14., 18.);
            }
            "trend" => {
                c.move_to(2., 15.);
                c.line_to(8., 9.);
                c.line_to(12., 11.);
                c.line_to(18., 4.);
                c.move_to(12., 4.);
                c.line_to(18., 4.);
                c.line_to(18., 10.);
            }
            "gauge" => {
                c.arc(10., 11., 7.5, 0., std::f64::consts::TAU);
                c.move_to(10., 11.);
                c.line_to(15., 6.);
                c.move_to(4., 15.);
                c.line_to(16., 15.);
            }
            _ => {
                c.move_to(10., 2.);
                c.line_to(17., 5.);
                c.line_to(16., 13.);
                c.line_to(10., 18.);
                c.line_to(4., 13.);
                c.line_to(3., 5.);
                c.close_path();
                c.move_to(10., 6.);
                c.line_to(10., 12.);
                c.move_to(10., 14.);
                c.line_to(10., 15.);
            }
        }
        let _ = c.stroke();
    });
    icon
}

fn panel(title: &str) -> gtk::Box {
    let b = crate::gloss_card::column(10);
    b.add_css_class("gs-aurelia-panel");
    b.set_hexpand(true);
    b.set_valign(gtk::Align::Start);
    let heading = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    heading.add_css_class("gs-aurelia-panel-heading");
    heading.append(&native_info_icon(if title.contains("Dernier") {
        "calendar"
    } else if title.contains("Comparaison") {
        "trend"
    } else if title.contains("Configuration") {
        "monitor"
    } else if title.contains("Résultats") {
        "gauge"
    } else {
        "chart"
    }));
    heading.append(&label(title, "gs-aurelia-panel-title"));
    b.append(&heading);
    b
}
fn color(id: &str, hardware: &str) -> &'static str {
    match id {
        "ram" => "gs-memory",
        "storage" => "gs-storage",
        _ => crate::hardware_colors::class(hardware),
    }
}
fn grouped(value: &str) -> String {
    let (integer, decimals) = value.split_once('.').unwrap_or((value, ""));
    let mut out = String::new();
    for (i, c) in integer.chars().enumerate() {
        if i > 0 && (integer.len() - i).is_multiple_of(3) {
            out.push('\u{202f}');
        }
        out.push(c);
    }
    if !decimals.is_empty() {
        out.push(',');
        out.push_str(decimals);
    }
    out
}
fn points(value: Option<f64>) -> String {
    value
        .filter(|v| v.is_finite() && *v > 0.)
        .map(|v| grouped(&format!("{v:.2}")))
        .unwrap_or_else(|| "—".into())
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Layout {
    Scores,
    Workspace,
    ResultPair,
    Header,
    Footer,
}
fn positions(layout: Layout, width: i32) -> Vec<(i32, i32, i32)> {
    match layout {
        Layout::Scores if width >= 1150 => {
            vec![(0, 0, 3), (3, 0, 2), (5, 0, 2), (7, 0, 2), (9, 0, 2)]
        }
        Layout::Scores if width >= 1040 => {
            vec![(0, 0, 4), (0, 1, 1), (1, 1, 1), (2, 1, 1), (3, 1, 1)]
        }
        Layout::Scores => vec![(0, 0, 2), (0, 1, 1), (1, 1, 1), (0, 2, 1), (1, 2, 1)],
        Layout::Workspace if width >= 1200 => vec![(0, 0, 3), (3, 0, 1)],
        Layout::Workspace => vec![(0, 0, 1), (0, 1, 1)],
        Layout::ResultPair if width >= 850 => vec![(0, 0, 3), (3, 0, 2)],
        Layout::ResultPair => vec![(0, 0, 1), (0, 1, 1)],
        Layout::Footer if width >= 1250 => vec![(0, 0, 7), (7, 0, 5)],
        Layout::Footer => vec![(0, 0, 1), (0, 1, 1)],
        Layout::Header if width >= 1250 => vec![(0, 0, 1), (1, 0, 1)],
        Layout::Header => vec![(0, 0, 1), (0, 1, 1)],
    }
}
// Reparent only at breakpoints, never during ordinary monitoring updates.
fn responsive(children: Vec<gtk::Widget>, layout: Layout) -> gtk::Grid {
    let grid = gtk::Grid::new();
    grid.set_column_homogeneous(true);
    grid.set_column_spacing(18);
    grid.set_row_spacing(18);
    grid.set_hexpand(true);
    for (child, (x, y, span)) in children.iter().zip(positions(layout, 1600)) {
        grid.attach(child, x, y, span, 1);
    }
    let previous = std::cell::RefCell::new(positions(layout, 1600));
    grid.add_tick_callback(move |grid, _| {
        // The grid can temporarily exceed its viewport while still arranged in
        // wide columns. Use the real window boundary to break that feedback.
        let available = grid
            .root()
            .and_then(|root| root.downcast::<gtk::Window>().ok())
            .and_then(|window| {
                grid.compute_bounds(&window)
                    .map(|bounds| window.width() - bounds.x().round() as i32 - 38)
            })
            .unwrap_or_else(|| grid.width());
        let next = positions(layout, available);
        if grid.width() > 0 && next != *previous.borrow() {
            for child in &children {
                grid.remove(child);
            }
            for (child, (x, y, span)) in children.iter().zip(&next) {
                grid.attach(child, *x, *y, *span, 1);
            }
            *previous.borrow_mut() = next;
        }
        gtk::glib::ControlFlow::Continue
    });
    grid
}
pub fn header(title: &gtk::Box, actions: &gtk::FlowBox) -> gtk::Grid {
    responsive(
        vec![title.clone().upcast(), actions.clone().upcast()],
        Layout::Header,
    )
}
fn icon(id: &str) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_valign(gtk::Align::Center);
    area.set_halign(gtk::Align::Center);
    area.set_content_width(32);
    area.set_content_height(32);
    let id = id.to_owned();
    area.set_draw_func(move |widget, c, w, h| {
        let color = widget.color();
        c.set_source_rgba(
            color.red().into(),
            color.green().into(),
            color.blue().into(),
            color.alpha().into(),
        );
        c.scale(w as f64 / 32., h as f64 / 32.);
        c.set_line_width(1.8);
        match id.as_str() {
            "gpu" => {
                c.rectangle(2., 6., 28., 20.);
                c.set_source_rgba(
                    color.red().into(),
                    color.green().into(),
                    color.blue().into(),
                    0.2,
                );
                let _ = c.fill_preserve();
                c.set_source_rgba(
                    color.red().into(),
                    color.green().into(),
                    color.blue().into(),
                    1.,
                );
                c.set_line_width(2.3);
                let _ = c.stroke();
                c.select_font_face(
                    "Sans",
                    gtk::cairo::FontSlant::Normal,
                    gtk::cairo::FontWeight::Bold,
                );
                c.set_font_size(8.);
                c.move_to(5., 19.);
                let _ = c.show_text("GPU");
            }
            "cpu" => {
                c.rectangle(7., 7., 18., 18.);
                c.set_source_rgba(
                    color.red().into(),
                    color.green().into(),
                    color.blue().into(),
                    0.2,
                );
                let _ = c.fill_preserve();
                c.set_source_rgba(
                    color.red().into(),
                    color.green().into(),
                    color.blue().into(),
                    1.,
                );
                let _ = c.stroke();
                c.rectangle(11., 11., 10., 10.);
                let _ = c.stroke();
                for p in [10., 16., 22.] {
                    c.move_to(p, 3.);
                    c.line_to(p, 7.);
                    c.move_to(p, 25.);
                    c.line_to(p, 29.);
                    c.move_to(3., p);
                    c.line_to(7., p);
                    c.move_to(25., p);
                    c.line_to(29., p);
                }
                let _ = c.stroke();
            }
            "ram" => {
                c.rectangle(3., 8., 26., 16.);
                let _ = c.stroke();
                for p in [7., 13., 19.] {
                    c.rectangle(p, 12., 4., 7.);
                }
                let _ = c.stroke();
                for p in [6., 11., 16., 21., 26.] {
                    c.move_to(p, 24.);
                    c.line_to(p, 28.);
                }
                let _ = c.stroke();
            }
            "storage" => {
                c.rectangle(5., 4., 22., 24.);
                let _ = c.stroke();
                c.move_to(5., 21.);
                c.line_to(27., 21.);
                let _ = c.stroke();
                c.arc(22., 25., 1., 0., std::f64::consts::TAU);
                let _ = c.fill();
            }
            _ => {
                c.set_line_width(1.6);
                for side in [-1., 1.] {
                    c.move_to(16., 28.);
                    c.curve_to(
                        16. + side * 14.,
                        24.,
                        16. + side * 14.,
                        12.,
                        16. + side * 7.,
                        4.,
                    );
                    let _ = c.stroke();
                    for (x, y, angle) in [
                        (6., 23., 0.9),
                        (4., 18., 0.3),
                        (5., 13., -0.2),
                        (7., 8., -0.5),
                        (11., 5., -0.8),
                    ] {
                        let _ = c.save();
                        c.translate(if side < 0. { x } else { 32. - x }, y);
                        c.rotate(angle * side);
                        c.move_to(0., -3.5);
                        c.curve_to(3., -1., 3., 1.5, 0., 3.5);
                        c.curve_to(-2., 1., -2., -1., 0., -3.5);
                        let _ = c.fill();
                        let _ = c.restore();
                    }
                }
            }
        }
    });
    area
}
fn valid_index(current: f64, reference: f64) -> Option<f64> {
    let value = 100. * (current / reference);
    (current > 0. && reference > 0. && value.is_finite() && value > 0.).then_some(value)
}
fn reference_scale(r: Option<&SystemResult>) -> f64 {
    let maximum = r
        .map(|r| {
            r.categories
                .iter()
                .filter(|c| ["cpu", "gpu", "ram", "storage"].contains(&c.id.as_str()))
                .flat_map(|c| c.measures.iter().filter_map(|m| m.reference_index))
                .chain(
                    r.previous_points
                        .values()
                        .filter_map(|(a, b)| valid_index(*b, *a)),
                )
                .fold(125., f64::max)
        })
        .unwrap_or(125.);
    ((maximum / 25.).ceil() * 25.).min(f64::MAX)
}
fn rounded(c: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64) {
    let r = (h / 2.).min(w / 2.).max(0.);
    c.new_sub_path();
    c.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.);
    c.arc(x + w - r, y + h - r, r, 0., std::f64::consts::FRAC_PI_2);
    c.arc(
        x + r,
        y + h - r,
        r,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    c.arc(
        x + r,
        y + r,
        r,
        std::f64::consts::PI,
        3. * std::f64::consts::FRAC_PI_2,
    );
    c.close_path();
}
fn reference_bar(index: Option<f64>, maximum: f64, uncertain: bool) -> gtk::DrawingArea {
    let bar = gtk::DrawingArea::new();
    bar.add_css_class("gs-aurelia-reference-bar");
    bar.set_content_height(14);
    bar.set_width_request(84);
    bar.set_hexpand(true);
    bar.set_valign(gtk::Align::Center);
    bar.set_tooltip_text(Some(
        &index
            .map(|v| {
                format!(
                    "Indice {v:.2} · référence A = 100 · échelle 0–{maximum:.0}{}",
                    if uncertain { " · À confirmer" } else { "" }
                )
            })
            .unwrap_or_else(|| {
                "Aucune référence compatible ; barre indisponible, pas un score zéro".into()
            }),
    ));
    bar.set_draw_func(move |_, c, w, h| {
        let w = w as f64;
        let h = (h as f64).min(14.);
        let track = gtk::cairo::LinearGradient::new(0., 0., 0., h);
        track.add_color_stop_rgb(0., 0.19, 0.22, 0.25);
        track.add_color_stop_rgb(0.5, 0.35, 0.39, 0.42);
        track.add_color_stop_rgb(1., 0.18, 0.21, 0.23);
        let _ = c.set_source(&track);
        rounded(c, 0., 0., w, h);
        let _ = c.fill();
        if let Some(value) = index {
            let gradient = gtk::cairo::LinearGradient::new(0., 0., 0., h);
            if value < 100. || uncertain {
                gradient.add_color_stop_rgb(0., 1., 0.91, 0.34);
                gradient.add_color_stop_rgb(0.45, 1., 0.84, 0.03);
                gradient.add_color_stop_rgb(1., 0.84, 0.56, 0.01);
            } else {
                gradient.add_color_stop_rgb(0., 0.25, 1., 0.39);
                gradient.add_color_stop_rgb(0.45, 0.05, 0.94, 0.2);
                gradient.add_color_stop_rgb(1., 0., 0.62, 0.12);
            }
            let _ = c.set_source(&gradient);
            rounded(c, 0., 0., w * (value / maximum).clamp(0., 1.), h);
            let _ = c.fill();
            let x = w * 100. / maximum;
            c.set_source_rgba(1., 1., 1., 0.92);
            c.set_line_width(1.5);
            c.move_to(x, 0.);
            c.line_to(x, h);
            let _ = c.stroke();
        } else {
            c.set_source_rgba(0.7, 0.75, 0.8, 0.25);
            c.move_to(w / 2. - 3., h / 2.);
            c.line_to(w / 2. + 3., h / 2.);
            let _ = c.stroke();
        }
    });
    bar
}

fn score_card(id: &str, name: &str, r: Option<&SystemResult>) -> gtk::Box {
    let global = id == "global";
    let card = crate::gloss_card::column(10);
    card.add_css_class("gs-aurelia-score-card");
    card.set_hexpand(true);
    card.add_css_class(if global {
        "gs-aurelia-global"
    } else {
        "gs-aurelia-category"
    });
    let hardware = r
        .and_then(|r| r.hardware.get(id))
        .map(String::as_str)
        .unwrap_or("Matériel non enregistré");
    if !global {
        card.add_css_class(color(id, hardware));
    }
    let value = r.and_then(|r| {
        if global {
            r.points.global
        } else {
            r.points.categories[id]
        }
    });
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let symbol = icon(id);
    symbol.set_content_width(if global { 68 } else { 40 });
    symbol.set_content_height(if global { 68 } else { 40 });
    symbol.add_css_class("gs-hardware-accent");
    row.append(&symbol);
    let content = column(4);
    content.set_hexpand(true);
    let title = label(name, "gs-aurelia-score-heading");
    if global {
        title.set_xalign(0.5);
    }
    content.append(&title);
    if !global {
        let text = label(
            hardware.strip_prefix("AMD ").unwrap_or(hardware),
            "gs-aurelia-card-hardware",
        );
        text.set_wrap(false);
        text.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text.set_tooltip_text(Some(hardware));
        content.append(&text);
    }
    let score = label(
        &points(value),
        if global {
            "gs-aurelia-global-number"
        } else {
            "gs-aurelia-category-number"
        },
    );
    score.set_wrap(false);
    score.set_ellipsize(gtk::pango::EllipsizeMode::End);
    score.set_tooltip_text(Some(
        &value
            .map(|v| format!("{v:.6} points Aurelia"))
            .unwrap_or_else(|| "Score indisponible".into()),
    ));
    if global {
        score.set_xalign(0.5);
    }
    content.append(&score);
    if global {
        let unit = label("Points Aurelia", "gs-aurelia-unit");
        unit.set_xalign(0.5);
        content.append(&unit);
    }
    row.append(&content);
    card.append(&row);
    if global {
        if value.is_none() {
            let message = label(
                if r.is_some() {
                    "CPU, GPU, mémoire et stockage requis"
                } else {
                    "En attente du premier résultat"
                },
                "gs-aurelia-muted",
            );
            message.set_xalign(0.5);
            card.append(&message);
        }
    } else {
        let index = r
            .and_then(|r| r.previous_points.get(id))
            .and_then(|(a, b)| valid_index(*b, *a));
        card.append(&reference_bar(index, reference_scale(r), false));
        let caption = if let Some((a, b)) = r.and_then(|r| r.previous_points.get(id)) {
            format!("{:+.1} % · {:+.2} pts", 100. * (b / a - 1.), b - a)
        } else if value.is_some() {
            "Référence compatible requise".into()
        } else {
            r.and_then(|r| r.categories.iter().find(|c| c.id == id))
                .map(|c| c.state.clone())
                .unwrap_or_else(|| "Non testé".into())
        };
        card.set_tooltip_text(Some(&format!("{hardware}\n{caption}")));
    }
    card
}
pub fn empty(root: &gtk::Box) {
    compose(root, None);
}
pub fn render(root: &gtk::Box, r: &SystemResult) {
    compose(root, Some(r));
}
fn compose(root: &gtk::Box, r: Option<&SystemResult>) {
    while let Some(child) = root.first_child() {
        root.remove(&child);
    }
    root.set_spacing(12);
    let cards = [
        ("global", "Score global"),
        ("cpu", "CPU"),
        ("gpu", "GPU"),
        ("ram", "Mémoire"),
        ("storage", "Stockage"),
    ]
    .into_iter()
    .map(|(id, name)| score_card(id, name, r).upcast())
    .collect();
    root.append(&responsive(cards, Layout::Scores));
    let scale = label(
        &if r.is_some_and(|r| !r.previous_points.is_empty()) {
            format!("Réf. A = 100 · échelle 0–{:.0}", reference_scale(r))
        } else {
            "Barres : référence compatible requise".into()
        },
        "gs-aurelia-scale",
    );
    scale.set_xalign(0.5);
    root.append(&scale);
    let tests = tests(r);
    let graphs = column(16);
    graphs.set_valign(gtk::Align::Start);
    graphs.append(&radar(r));
    graphs.append(&fps(r.map(|r| r.fps.as_slice()).unwrap_or(&[])));
    let side = column(16);
    side.set_valign(gtk::Align::Start);
    side.append(&latest(r));
    side.append(&comparison(r));
    let tabs = gtk::Stack::new();
    tabs.set_vhomogeneous(false);
    tabs.set_hhomogeneous(false);
    let switcher = gtk::StackSwitcher::new();
    switcher.set_stack(Some(&tabs));
    switcher.add_css_class("gs-aurelia-view-tabs");
    switcher.set_halign(gtk::Align::Start);
    let views = column(12);
    views.append(&switcher);
    views.append(&tabs);
    tabs.add_titled(
        &responsive(vec![tests.upcast(), graphs.upcast()], Layout::ResultPair),
        Some("summary"),
        "Résumé",
    );
    let details = column(12);
    if let Some(r) = r {
        crate::aurelia_results::render_system_details(&details, r);
        details.append(&label(&r.points.method, "gs-aurelia-caption"));
    } else {
        details.append(&label(
            "Les détails apparaîtront après un benchmark.",
            "gs-aurelia-muted",
        ));
    }
    tabs.add_titled(&details, Some("details"), "Détails");
    let plots = responsive(
        vec![
            radar_sized(r, 260).upcast(),
            fps_sized(r.map(|r| r.fps.as_slice()).unwrap_or(&[]), 300).upcast(),
        ],
        Layout::Header,
    );
    tabs.add_titled(&plots, Some("graphs"), "Graphes");
    let compare = column(16);
    compare.append(&comparison(r));
    let history = gtk::Button::with_label("Choisir les résultats à comparer…");
    history.set_halign(gtk::Align::Start);
    history.connect_clicked(|button| {
        if let Some(root) = button
            .root()
            .and_then(|root| root.downcast::<gtk::Window>().ok())
        {
            crate::benchmark::navigate(root.upcast_ref(), "history");
        }
    });
    compare.append(&history);
    tabs.add_titled(&compare, Some("comparison"), "Comparaison");
    root.append(&responsive(
        vec![views.upcast(), side.upcast()],
        Layout::Workspace,
    ));
    let config = panel("Configuration testée");
    if let Some(r) = r {
        let hardware = column(5);
        for (id, name) in [("cpu", "CPU"), ("gpu", "GPU"), ("ram", "RAM")] {
            hardware.append(&label(
                &format!("{name} : {}", r.hardware[id]),
                "gs-aurelia-caption",
            ));
        }
        let software = column(5);
        for (name, value) in &r.configuration {
            let text = label(&format!("{name} : {value}"), "gs-aurelia-caption");
            text.set_wrap(false);
            text.set_ellipsize(gtk::pango::EllipsizeMode::End);
            text.set_tooltip_text(Some(value));
            software.append(&text);
        }
        config.set_tooltip_text(Some(&r.metadata));
        config.append(&responsive(
            vec![hardware.upcast(), software.upcast()],
            Layout::Header,
        ));
    } else {
        config.append(&label(
            "Le matériel et les conditions du résultat apparaîtront ici.",
            "gs-aurelia-muted",
        ));
    }
    let actions = gtk::FlowBox::new();
    actions.set_selection_mode(gtk::SelectionMode::None);
    actions.set_max_children_per_line(3);
    actions.set_column_spacing(12);
    actions.set_row_spacing(8);
    actions.set_valign(gtk::Align::Center);
    let saved = gtk::Button::with_label(if r.is_some() {
        "✓  Enregistré"
    } else {
        "Enregistrement auto"
    });
    saved.set_sensitive(false);
    saved.add_css_class("gs-aurelia-saved");
    saved.set_tooltip_text(Some(
        "Les résultats sont conservés automatiquement, indépendamment de l’export.",
    ));
    actions.insert(&saved, -1);
    let export = gtk::Button::with_label("Exporter…");
    export.set_sensitive(r.is_some());
    export.connect_clicked(|button| {
        if let Some(window) = button.root().and_then(|r| r.downcast::<gtk::Window>().ok()) {
            if let Some(export) =
                crate::benchmark::named_widget(window.upcast_ref(), "aurelia-export-result")
                    .and_then(|w| w.downcast::<gtk::Button>().ok())
            {
                export.emit_clicked();
            }
        }
    });
    actions.insert(&export, -1);
    actions.insert(&history_button("Voir l’historique"), -1);
    root.append(&responsive(
        vec![config.upcast(), actions.upcast()],
        Layout::Footer,
    ));
}
fn tests(r: Option<&SystemResult>) -> gtk::Box {
    let p = panel("Résultats par test");
    p.set_vexpand(false);
    p.append(&label(
        &if r.is_some_and(|r| {
            r.categories
                .iter()
                .any(|c| c.measures.iter().any(|m| m.reference_index.is_some()))
        }) {
            format!(
                "Indice vs référence A = 100 · échelle 0–{:.0} · plus haut = mieux",
                reference_scale(r)
            )
        } else {
            "Mesures brutes · référence requise pour les barres".into()
        },
        "gs-aurelia-scale",
    ));
    if let Some(r) = r {
        for category in r
            .categories
            .iter()
            .filter(|c| ["cpu", "gpu", "ram", "storage"].contains(&c.id.as_str()))
        {
            if category.measures.is_empty() {
                p.append(&test_row(
                    &category.id,
                    &category.name,
                    &category.state,
                    &r.hardware[&category.id],
                    None,
                    reference_scale(Some(r)),
                    false,
                ));
            }
            for measure in &category.measures {
                let row = test_row(
                    &category.id,
                    &summary_test_name(&category.id, &measure.name),
                    &measure_text(measure.value, &measure.unit),
                    &r.hardware[&category.id],
                    measure.reference_index,
                    reference_scale(Some(r)),
                    measure.uncertain,
                );
                row.set_tooltip_text(Some(&format!(
                    "{} · CV des mesures brutes {:.2} %{} · unité originale : {}",
                    if measure.unit == "étapes/s" {
                        "Temps équivalent par cycle : inverse du débit moyen"
                    } else {
                        "Débit moyen des répétitions"
                    },
                    measure.variation,
                    if measure.uncertain {
                        " · À confirmer"
                    } else {
                        ""
                    },
                    measure.unit
                )));
                p.append(&row);
            }
        }
    } else {
        for (id, name) in [
            ("cpu", "CPU monothread"),
            ("cpu", "CPU multithread"),
            ("gpu", "GPU rasterisation"),
            ("ram", "Mémoire · lecture"),
            ("ram", "Mémoire · écriture"),
            ("ram", "Mémoire · copie"),
            ("storage", "Stockage · lecture"),
            ("storage", "Stockage · écriture"),
        ] {
            p.append(&test_row(
                id,
                name,
                "—",
                "",
                None,
                reference_scale(None),
                false,
            ));
        }
    }
    p
}
fn summary_test_name(id: &str, name: &str) -> String {
    match id {
        "cpu" if name.to_lowercase().contains("monothread") => "CPU monothread".into(),
        "cpu" => "CPU multithread".into(),
        "gpu" => "GPU rasterisation".into(),
        "ram" if name.to_lowercase().contains("lecture") => "Mémoire lecture".into(),
        "ram" if name.to_lowercase().contains("écriture") => "Mémoire écriture".into(),
        "ram" => "Mémoire copie".into(),
        "storage" if name.to_lowercase().contains("lecture") => "Stockage lecture".into(),
        "storage" => "Stockage écriture".into(),
        _ => name.into(),
    }
}
fn measure_text(value: f64, unit: &str) -> String {
    if unit == "étapes/s" && value.is_finite() && value > 0. {
        format!("{} ms/cycle", grouped(&format!("{:.3}", 1000. / value)))
    } else {
        format!(
            "{} {}",
            grouped(&format!("{value:.2}")),
            match unit {
                "images/s" => "FPS",
                "Go/s logiques" => "Go/s",
                other => other,
            }
        )
    }
}
fn test_row(
    id: &str,
    name: &str,
    value: &str,
    hardware: &str,
    index: Option<f64>,
    maximum: f64,
    uncertain: bool,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("gs-aurelia-test-row");
    row.add_css_class(color(id, hardware));
    let symbol = icon(id);
    symbol.set_content_width(24);
    symbol.set_content_height(24);
    symbol.add_css_class("gs-hardware-accent");
    row.append(&symbol);
    let title = label(name, "gs-aurelia-caption");
    title.set_width_chars(4);
    title.set_wrap(false);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.set_hexpand(true);
    title.set_tooltip_text(Some(name));
    row.append(&title);
    let value = label(value, "gs-aurelia-measure-value");
    value.set_wrap(false);
    value.set_width_chars(13);
    value.set_xalign(1.);
    row.append(&value);
    row.append(&reference_bar(index, maximum, uncertain));
    let number = label(
        &index
            .map(|v| format!("{v:.0}"))
            .unwrap_or_else(|| "—".into()),
        "gs-aurelia-measure-value",
    );
    number.set_width_chars(4);
    number.set_xalign(1.);
    row.append(&number);
    row
}
fn history_button(title: &str) -> gtk::Button {
    let button = gtk::Button::with_label(title);
    button.connect_clicked(|button| {
        if let Some(window) = button.root().and_then(|r| r.downcast::<gtk::Window>().ok()) {
            crate::benchmark::navigate(window.upcast_ref(), "history");
        }
    });
    button
}
fn info_row(kind: &str, text: &str) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.append(&native_info_icon(kind));
    row.append(&label(text, "gs-aurelia-caption"));
    row
}
fn duration(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0. {
        return "—".into();
    }
    if seconds < 60. {
        return format!("{seconds:.1} s").replace('.', ",");
    }
    let seconds = seconds.round() as u64;
    format!("{} min {:02} s", seconds / 60, seconds % 60)
}
fn latest(r: Option<&SystemResult>) -> gtk::Box {
    let p = panel("Dernier résultat");
    p.add_css_class("gs-aurelia-latest");
    p.set_spacing(8);
    p.append(&info_row(
        "calendar",
        r.map(|r| r.date.as_str())
            .unwrap_or("Aucune session enregistrée"),
    ));
    let badge = info_row(
        if r.is_some_and(|r| r.completed) {
            "check"
        } else {
            "shield"
        },
        r.map(|r| {
            if r.completed {
                "Test terminé"
            } else {
                "Résultat partiel"
            }
        })
        .unwrap_or("Non testé"),
    );
    badge.add_css_class("gs-result-badge");
    badge.add_css_class(if r.is_some_and(|r| r.completed) {
        "success"
    } else if r.is_some() {
        "warning"
    } else {
        "unavailable"
    });
    badge.set_halign(gtk::Align::Start);
    p.append(&badge);
    p.append(&info_row(
        "clock",
        &format!(
            "Durée : {}",
            r.map(|r| duration(r.duration_seconds))
                .unwrap_or_else(|| "—".into())
        ),
    ));
    let protocol = r
        .map(|r| match r.protocol.as_str() {
            "aurelia-system-v3-storage" => "système v3 (stockage)",
            "aurelia-system-v2" => "système v2",
            other => other,
        })
        .unwrap_or("—");
    let row = info_row("shield", &format!("Protocole : {protocol}"));
    row.set_tooltip_text(r.map(|r| r.protocol.as_str()));
    p.append(&row);
    if let Some(r) = r {
        p.set_tooltip_text(Some(&r.status));
    }
    p
}
fn comparison(r: Option<&SystemResult>) -> gtk::Box {
    let p = panel("Comparaison rapide");
    p.set_spacing(6);
    let reference = history_button(
        &r.and_then(|r| r.reference.as_ref())
            .map(|name| format!("vs. {name}  ⌄"))
            .unwrap_or_else(|| "Choisir une référence  ⌄".into()),
    );
    reference.set_tooltip_text(Some("Choisir les résultats A et B dans l’historique"));
    if let Some(label) = reference
        .child()
        .and_then(|w| w.downcast::<gtk::Label>().ok())
    {
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(28);
    }
    p.append(&reference);
    if let Some((a, b)) = r.and_then(|r| r.previous_points.get("global")) {
        let gain = label(
            &format!(
                "{}{:+.1} %",
                if r.is_some_and(|r| r.categories.iter().any(|c| c.uncertain)) {
                    "≈ "
                } else {
                    ""
                },
                100. * (b / a - 1.)
            )
            .replace('.', ","),
            "gs-aurelia-comparison-number",
        );
        gain.add_css_class(if b >= a { "gs-positive" } else { "gs-negative" });
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        row.append(&gain);
        let delta = label(
            &format!("{:+.2} pts", b - a).replace('.', ","),
            "gs-aurelia-caption",
        );
        delta.set_hexpand(true);
        delta.set_xalign(1.);
        delta.set_valign(gtk::Align::End);
        row.append(&delta);
        p.append(&row);
    } else {
        p.append(&label("—", "gs-aurelia-comparison-number"));
    }
    for (id, name) in [
        ("cpu", "CPU"),
        ("gpu", "GPU"),
        ("ram", "Mémoire"),
        ("storage", "Stockage"),
    ] {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        row.add_css_class("gs-aurelia-comparison-row");
        let title = label(name, "gs-aurelia-caption");
        title.set_hexpand(true);
        row.append(&title);
        let pair = r.and_then(|r| r.previous_points.get(id));
        let value = label(
            &pair
                .map(|(a, b)| format!("{:+.1} %", 100. * (b / a - 1.)).replace('.', ","))
                .unwrap_or_else(|| "—".into()),
            "gs-aurelia-measure-value",
        );
        if let Some((a, b)) = pair {
            value.add_css_class(if b >= a { "gs-positive" } else { "gs-negative" });
            row.set_tooltip_text(Some(&format!(
                "{:+.2} points · référence {a:.2} → {b:.2}",
                b - a
            )));
        }
        row.append(&value);
        p.append(&row);
    }
    if r.is_some_and(|r| r.categories.iter().any(|c| c.uncertain)) {
        p.set_tooltip_text(Some("Mesures ou écarts à confirmer ; les réserves de variabilité et la méthode restent disponibles dans Détails."));
    }
    p
}
fn unavailable(p: &gtk::Box, message: &str, height: i32) {
    let empty = label(message, "gs-aurelia-muted");
    empty.set_justify(gtk::Justification::Center);
    empty.set_xalign(0.5);
    empty.set_valign(gtk::Align::Center);
    empty.set_height_request(height);
    p.append(&empty);
}
fn radar(r: Option<&SystemResult>) -> gtk::Box {
    radar_sized(r, 150)
}
fn radar_sized(r: Option<&SystemResult>, height: i32) -> gtk::Box {
    let p = panel("Aperçu des performances");
    let values = r.and_then(|r| {
        ["cpu", "gpu", "ram", "storage"]
            .iter()
            .map(|id| r.previous_points.get(*id).map(|(a, b)| 100. * b / a))
            .collect::<Option<Vec<f64>>>()
    });
    let Some(values) = values.filter(|v| v.iter().all(|v| v.is_finite() && *v > 0.)) else {
        unavailable(
            &p,
            "Radar disponible avec quatre\ncatégories comparables à une référence",
            height - 30,
        );
        return p;
    };
    let maximum = values.iter().copied().fold(100., f64::max) * 1.15;
    let area = gtk::DrawingArea::new();
    area.set_content_height(height);
    area.set_hexpand(true);
    area.set_draw_func(move |_, c, w, h| {
        c.set_source_rgb(0.76, 0.8, 0.84);
        c.set_font_size(10.);
        c.move_to(2., 11.);
        let _ = c.show_text(&format!("Réf. A = 100 · échelle 0–{maximum:.0}"));
        let cx = w as f64 / 2.;
        let cy = h as f64 / 2.;
        let radius = (w.min(h) as f64 / 2. - 18.).max(1.);
        let xy = |i: usize, v: f64| {
            let a = i as f64 * std::f64::consts::FRAC_PI_2 - std::f64::consts::FRAC_PI_2;
            (
                cx + radius * v / maximum * a.cos(),
                cy + radius * v / maximum * a.sin(),
            )
        };
        for ring in 1..=4 {
            for i in 0..4 {
                let (x, y) = xy(i, maximum * ring as f64 / 4.);
                if i == 0 {
                    c.move_to(x, y);
                } else {
                    c.line_to(x, y);
                }
            }
            c.close_path();
            c.set_source_rgba(0.7, 0.75, 0.8, 0.22);
            let _ = c.stroke();
        }
        for (i, value) in values.iter().enumerate() {
            let (x, y) = xy(i, *value);
            if i == 0 {
                c.move_to(x, y);
            } else {
                c.line_to(x, y);
            }
        }
        c.close_path();
        c.set_source_rgba(0.96, 0.62, 0.04, 0.23);
        let _ = c.fill_preserve();
        c.set_source_rgb(0.96, 0.62, 0.04);
        c.set_line_width(2.);
        let _ = c.stroke();
        c.set_source_rgb(0.85, 0.87, 0.9);
        c.set_font_size(12.);
        for (i, name) in ["CPU", "GPU", "Mémoire", "Stockage"].iter().enumerate() {
            let (x, y) = xy(i, maximum);
            let ext = c.text_extents(name).ok();
            let tw = ext.map(|e| e.width()).unwrap_or(0.);
            c.move_to(
                (x - tw / 2.).clamp(2., w as f64 - tw - 2.),
                y.clamp(14., h as f64 - 2.),
            );
            let _ = c.show_text(name);
        }
    });
    p.append(&area);
    p.set_tooltip_text(Some(&format!(
        "Référence A = 100 · échelle commune 0–{maximum:.0}"
    )));
    p
}
#[derive(Debug)]
struct FpsSummary {
    minimum: f64,
    maximum: f64,
    average: f64,
    upper: f64,
}
fn fps_summary(samples: &[f64]) -> Option<FpsSummary> {
    if samples.is_empty() || samples.iter().any(|v| !v.is_finite() || *v <= 0.) {
        return None;
    }
    let minimum = samples.iter().copied().fold(f64::INFINITY, f64::min);
    let maximum = samples.iter().copied().fold(0., f64::max);
    // Equal frame work, unequal intervals: total frames / total seconds.
    let average = samples.len() as f64 / samples.iter().map(|v| 1. / v).sum::<f64>();
    let step = 10_f64.powf(maximum.log10().floor()) / 5.;
    let upper = (maximum * 1.05 / step).ceil() * step;
    if !upper.is_finite() || !average.is_finite() || average <= 0. {
        return None;
    }
    Some(FpsSummary {
        minimum,
        maximum,
        average,
        upper,
    })
}
fn fps(samples: &[f64]) -> gtk::Box {
    fps_sized(samples, 120)
}
fn fps_sized(samples: &[f64], height: i32) -> gtk::Box {
    let p = panel("Évolution des FPS · GPU");
    p.set_spacing(6);
    p.add_css_class("gs-aurelia-fps");
    let Some(summary) = fps_summary(samples) else {
        unavailable(&p, "Aucun échantillon GPU enregistré", height - 10);
        p.append(&label(
            "Rendu hors écran · pas la fréquence d’affichage",
            "gs-aurelia-muted",
        ));
        return p;
    };
    let statistics = format!(
        "Min {:.1} · Moy {:.1} · Max {:.1} FPS",
        summary.minimum, summary.average, summary.maximum
    )
    .replace('.', ",");
    let samples = samples.to_vec();
    let times = samples
        .iter()
        .scan(0., |elapsed, value| {
            *elapsed += 1. / value;
            Some(*elapsed)
        })
        .collect::<Vec<f64>>();
    let seconds = *times.last().unwrap_or(&1.);
    let area = gtk::DrawingArea::new();
    area.set_content_height(height);
    area.set_hexpand(true);
    area.set_draw_func(move |_, c, w, h| {
        let left = 42.;
        let top = 15.;
        let width = (w as f64 - left - 10.).max(1.);
        let height = (h as f64 - top - 27.).max(1.);
        c.set_font_size(10.);
        c.set_line_width(1.);
        for i in 0..=4 {
            let value = summary.upper * i as f64 / 4.;
            let y = top + height * (1. - value / summary.upper);
            c.set_source_rgba(0.65, 0.7, 0.75, 0.18);
            c.move_to(left, y);
            c.line_to(left + width, y);
            let _ = c.stroke();
            c.set_source_rgb(0.75, 0.79, 0.83);
            c.move_to(2., y + 4.);
            let _ = c.show_text(&format!("{value:.0}"));
        }
        c.move_to(2., 10.);
        let _ = c.show_text("FPS");
        for i in 0..=4 {
            let fraction = i as f64 / 4.;
            let x = left + width * fraction;
            c.move_to(x - 9., top + height + 16.);
            let _ = c.show_text(
                &format!(
                    "{:.*}",
                    if seconds < 1. { 2 } else { 1 },
                    seconds * fraction
                )
                .replace('.', ","),
            );
        }
        c.move_to(left + width - 46., h as f64 - 1.);
        let _ = c.show_text("Temps (s)");
        let chunk = samples.len().div_ceil(width as usize).max(1);
        let path = |c: &gtk::cairo::Context| {
            for (i, part) in samples.chunks(chunk).enumerate() {
                let value = part.len() as f64 / part.iter().map(|v| 1. / v).sum::<f64>();
                let end = (i * chunk + part.len() - 1).min(times.len() - 1);
                let x = left + times[end] / seconds * width;
                let y = top + height * (1. - value / summary.upper);
                if i == 0 {
                    c.move_to(x, y);
                } else {
                    c.line_to(x, y);
                }
            }
        };
        c.new_path();
        path(c);
        c.line_to(left + width, top + height);
        c.line_to(left, top + height);
        c.close_path();
        let fill = gtk::cairo::LinearGradient::new(0., top, 0., top + height);
        fill.add_color_stop_rgba(0., 1., 0.53, 0.01, 0.45);
        fill.add_color_stop_rgba(1., 1., 0.43, 0., 0.035);
        let _ = c.set_source(&fill);
        let _ = c.fill();
        c.new_path();
        path(c);
        c.set_source_rgb(1., 0.64, 0.07);
        c.set_line_width(2.);
        let _ = c.stroke();
        c.set_line_width(1.);
        c.set_source_rgba(1., 0.62, 0.04, 0.65);
        for (i, part) in samples.chunks(chunk).enumerate() {
            let lo = part.iter().copied().fold(f64::INFINITY, f64::min);
            let hi = part.iter().copied().fold(0., f64::max);
            let end = (i * chunk + part.len() - 1).min(times.len() - 1);
            let x = left + times[end] / seconds * width;
            c.move_to(x, top + height * (1. - hi / summary.upper));
            c.line_to(x, top + height * (1. - lo / summary.upper));
        }
        let _ = c.stroke();
    });
    p.append(&area);
    p.append(&label(&statistics, "gs-aurelia-scale"));
    p.set_tooltip_text(Some("Rendu synthétique hors écran. Temps cumulé des intervalles CPU entre images ; aucun horodatage d’affichage physique. Pics min/max conservés."));
    p
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reference_scale_is_explicit_and_has_no_artificial_ceiling() {
        assert_eq!(reference_scale(None), 125.);
        assert_eq!(valid_index(300., 100.), Some(300.));
        assert_eq!(valid_index(1., 0.), None);
        let mut s = crate::aurelia_results::system(
            &crate::aurelia_visual_tests::fixture_for_scale_test(),
            None,
            false,
        );
        s.previous_points.insert("cpu".into(), (100., 350.));
        assert_eq!(reference_scale(Some(&s)), 350.);
        assert_eq!(duration(132.), "2 min 12 s");
        assert_eq!(duration(f64::NAN), "—");
    }
    #[test]
    fn cpu_summary_time_is_reciprocal_of_recorded_rate_without_relabelling_other_units() {
        assert_eq!(measure_text(250., "étapes/s"), "4,000 ms/cycle");
        assert_eq!(measure_text(60., "images/s"), "60,00 FPS");
    }
    #[test]
    fn fps_average_uses_total_frame_time_and_scale_never_crops_peaks() {
        let s = fps_summary(&[50., 100.]).unwrap();
        assert!((s.average - 200. / 3.).abs() < 1e-9);
        assert_eq!(s.minimum, 50.);
        assert_eq!(s.maximum, 100.);
        assert!(s.upper >= s.maximum);
        assert!(fps_summary(&[]).is_none());
        assert!(fps_summary(&[f64::NAN]).is_none());
    }
    #[test]
    fn responsive_breakpoints_keep_every_card_and_use_one_column_when_narrow() {
        for width in [480, 800, 1100, 1600, 2200] {
            assert_eq!(positions(Layout::Scores, width).len(), 5);
            assert_eq!(positions(Layout::Workspace, width).len(), 2);
            assert_eq!(positions(Layout::ResultPair, width).len(), 2);
        }
        assert!(positions(Layout::Workspace, 600)
            .iter()
            .all(|(x, _, _)| *x == 0));
        assert_eq!(positions(Layout::Scores, 1600)[0].2, 3);
    }
}
