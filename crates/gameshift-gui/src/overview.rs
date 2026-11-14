//! Home dashboard. Real read-only samples, one worker, no benchmark or hardware write.
use crate::{diagnostic_context::LastReport, diagnostic_view::View};
use adw::prelude::*;
use gameshift_cpu::environment::{busy_percent, parse_cpu_ticks, Snapshot as CpuSnapshot};
use gameshift_telemetry::{
    device::{Measurement, Metric},
    processes::shared_snapshot,
};
use gtk::glib;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::mpsc,
    time::{Duration, Instant},
};

const TEAL: (f64, f64, f64) = (0.02, 0.89, 0.84);
fn text(value: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(value));
    label.add_css_class(class);
    label.set_xalign(0.);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_hexpand(true);
    label
}
/// Small code-native icons: no dependency on the desktop's optional SVG loaders.
pub fn icon(name: &str, size: i32) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(size);
    area.set_content_height(size);
    area.set_valign(gtk::Align::Center);
    area.set_halign(gtk::Align::Center);
    let name = name.to_owned();
    area.set_draw_func(move |area, cr, width, height| {
        let color = area.color();
        cr.set_source_rgba(
            color.red() as f64,
            color.green() as f64,
            color.blue() as f64,
            color.alpha() as f64,
        );
        cr.scale(width as f64 / 24., height as f64 / 24.);
        cr.set_line_width(1.7);
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        cr.set_line_join(gtk::cairo::LineJoin::Round);
        match name.as_str() {
            "gameshift" => {
                cr.move_to(20., 4.);
                cr.line_to(8., 4.);
                cr.line_to(2., 12.);
                cr.line_to(8., 20.);
                cr.line_to(18., 20.);
                cr.line_to(22., 11.);
                cr.line_to(12., 11.);
                cr.set_line_width(4.);
            }
            "fan-symbolic" => {
                cr.arc(12., 12., 2., 0., std::f64::consts::TAU);
                let _ = cr.stroke();
                for angle in [
                    0.,
                    std::f64::consts::TAU / 3.,
                    2. * std::f64::consts::TAU / 3.,
                ] {
                    let _ = cr.save();
                    cr.translate(12., 12.);
                    cr.rotate(angle);
                    cr.move_to(2., 0.);
                    cr.curve_to(12., -10., 14., 4., 4., 4.);
                    cr.close_path();
                    let _ = cr.stroke();
                    let _ = cr.restore();
                }
            }
            "temperature-symbolic" => {
                cr.move_to(9., 14.);
                cr.line_to(9., 5.);
                cr.curve_to(9., 1., 15., 1., 15., 5.);
                cr.line_to(15., 14.);
                cr.arc(12., 17., 5., -0.9, 4.05);
                cr.move_to(12., 8.);
                cr.line_to(12., 18.);
            }
            "ac-adapter-symbolic" => {
                cr.move_to(14., 2.);
                cr.line_to(5., 14.);
                cr.line_to(11., 14.);
                cr.line_to(9., 22.);
                cr.line_to(20., 9.);
                cr.line_to(13., 9.);
                cr.close_path();
            }
            "applications-science-symbolic" => {
                for (x, y) in [(4., 14.), (11., 8.), (18., 2.)] {
                    cr.rectangle(x, y, 3., 21. - y);
                }
            }
            "computer-symbolic" => {
                cr.rectangle(2., 3., 20., 14.);
                cr.move_to(12., 17.);
                cr.line_to(12., 22.);
                cr.move_to(7., 22.);
                cr.line_to(17., 22.);
            }
            "drive-harddisk-symbolic" => {
                cr.rectangle(3., 3., 18., 18.);
                cr.arc(12., 11., 5., 0., std::f64::consts::TAU);
                cr.move_to(17., 20.);
                cr.line_to(19., 20.);
            }
            "applications-games-symbolic" => {
                cr.move_to(6., 6.);
                cr.curve_to(0., 10., 0., 22., 6., 18.);
                cr.line_to(9., 15.);
                cr.line_to(15., 15.);
                cr.curve_to(23., 24., 25., 15., 18., 6.);
                cr.close_path();
                cr.move_to(5., 10.);
                cr.line_to(9., 10.);
                cr.move_to(7., 8.);
                cr.line_to(7., 12.);
                cr.move_to(17., 9.);
                cr.line_to(17.1, 9.);
                cr.move_to(19., 12.);
                cr.line_to(19.1, 12.);
            }
            "document-open-recent-symbolic" => {
                cr.arc(12., 12., 9., 0., std::f64::consts::TAU);
                cr.move_to(12., 5.);
                cr.line_to(12., 12.);
                cr.line_to(18., 15.);
            }
            "go-next-symbolic" => {
                cr.move_to(8., 4.);
                cr.line_to(16., 12.);
                cr.line_to(8., 20.);
            }
            "view-list-symbolic" => {
                for y in [5., 12., 19.] {
                    cr.rectangle(2., y - 1., 3., 3.);
                    cr.move_to(9., y);
                    cr.line_to(22., y);
                }
            }
            "view-grid-symbolic" => {
                cr.move_to(2., 11.);
                cr.line_to(12., 2.);
                cr.line_to(22., 11.);
                cr.move_to(5., 9.);
                cr.line_to(5., 22.);
                cr.line_to(19., 22.);
                cr.line_to(19., 9.);
                cr.move_to(10., 22.);
                cr.line_to(10., 15.);
                cr.line_to(14., 15.);
                cr.line_to(14., 22.);
            }
            "video-display-symbolic" => {
                cr.rectangle(2., 5., 20., 14.);
                cr.arc(9., 12., 4., 0., std::f64::consts::TAU);
                cr.new_sub_path();
                cr.arc(17., 12., 3., 0., std::f64::consts::TAU);
            }
            _ => {
                cr.rectangle(5., 5., 14., 14.);
                for n in [8., 12., 16.] {
                    cr.move_to(n, 2.);
                    cr.line_to(n, 5.);
                    cr.move_to(n, 19.);
                    cr.line_to(n, 22.);
                    cr.move_to(2., n);
                    cr.line_to(5., n);
                    cr.move_to(19., n);
                    cr.line_to(22., n);
                }
                cr.rectangle(9., 9., 6., 6.);
            }
        }
        let _ = cr.stroke();
    });
    area
}
fn artwork() -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_hexpand(true);
    area.set_draw_func(|_, cr, width, height| {
        let w = width as f64;
        let h = height as f64;
        let gradient = gtk::cairo::LinearGradient::new(0., h, w, 0.);
        gradient.add_color_stop_rgb(0., 0.035, 0.055, 0.1);
        gradient.add_color_stop_rgb(0.6, 0.16, 0.07, 0.27);
        gradient.add_color_stop_rgb(1., 0.025, 0.11, 0.16);
        let _ = cr.set_source(&gradient);
        let _ = cr.paint();
        for (i, alpha) in [(0., 0.12), (1., 0.28), (2., 0.45), (3., 0.2)] {
            let x = w * (0.3 + i * 0.14);
            cr.move_to(x, 0.);
            cr.line_to(x + w * 0.08, 0.);
            cr.line_to(x - h + w * 0.08, h);
            cr.line_to(x - h, h);
            cr.close_path();
            cr.set_source_rgba(0.62, 0.27, 0.95, alpha);
            let _ = cr.fill();
            cr.move_to(x, 0.);
            cr.line_to(x - h, h);
            cr.set_line_width(1.);
            cr.set_source_rgba(0.79, 0.46, 1., alpha);
            let _ = cr.stroke();
        }
    });
    area
}
/// Static home backdrop, cached by GTK between size/layout invalidations.
/// This layer has no input handling, animation, telemetry or permanent redraw timer.
pub(crate) fn backdrop(aurelia: Option<&gtk::Button>) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_can_target(false);
    area.set_hexpand(true);
    area.set_vexpand(true);
    let aurelia = aurelia.map(|button| button.downgrade());
    let cache = RefCell::new(None::<(i32, i32, gtk::cairo::ImageSurface)>);
    area.set_draw_func(move |area, cr, width, height| {
        let mut cache = cache.borrow_mut();
        if cache
            .as_ref()
            .is_none_or(|(w, h, _)| *w != width || *h != height)
        {
            let aurelia_y = aurelia
                .as_ref()
                .and_then(|button| button.upgrade())
                .and_then(|button| button.compute_bounds(area))
                .map(|bounds| {
                    ((bounds.y() + bounds.height() * 0.5) as f64 / height.max(1) as f64)
                        .clamp(0., 1.)
                })
                .unwrap_or(0.75);
            if let Ok(surface) = gtk::cairo::ImageSurface::create(
                gtk::cairo::Format::ARgb32,
                width.max(1),
                height.max(1),
            ) {
                if let Ok(context) = gtk::cairo::Context::new(&surface) {
                    paint_backdrop(
                        &context,
                        width.max(1) as f64,
                        height.max(1) as f64,
                        aurelia_y,
                    );
                    *cache = Some((width, height, surface));
                }
            }
        }
        if let Some((_, _, surface)) = cache.as_ref() {
            let _ = cr.set_source_surface(surface, 0., 0.);
            let _ = cr.paint();
        } else {
            paint_backdrop(cr, width.max(1) as f64, height.max(1) as f64, 0.75);
        }
    });
    area
}
fn paint_backdrop(cr: &gtk::cairo::Context, w: f64, h: f64, aurelia_y: f64) {
    let base = gtk::cairo::LinearGradient::new(0., 0., w * 0.8, h);
    base.add_color_stop_rgb(0., 0.016, 0.035, 0.063);
    base.add_color_stop_rgb(0.5, 0.028, 0.052, 0.082);
    base.add_color_stop_rgb(1., 0.012, 0.025, 0.047);
    let _ = cr.set_source(&base);
    let _ = cr.paint();
    // Soft elliptical gradients, rather than blur filters or decorative artwork.
    for (x, y, rx, ry, r, g, b, alpha) in [
        (0.15, 0.18, 0.82, 0.90, 0.015, 0.34, 0.38, 0.36),
        (0.49, 0.83, 0.78, 0.74, 0.018, 0.18, 0.29, 0.46),
        (
            0.71,
            aurelia_y.max(0.6),
            0.52,
            0.32,
            0.24,
            0.08,
            0.34,
            0.085,
        ),
    ] {
        let _ = cr.save();
        cr.translate(x * w, y * h);
        cr.scale(rx * w, ry * h);
        let halo = gtk::cairo::RadialGradient::new(0., 0., 0., 0., 0., 1.);
        halo.add_color_stop_rgba(0., r, g, b, alpha);
        halo.add_color_stop_rgba(0.25, r, g, b, alpha * 0.90);
        halo.add_color_stop_rgba(0.55, r, g, b, alpha * 0.40);
        halo.add_color_stop_rgba(0.8, r, g, b, alpha * 0.08);
        halo.add_color_stop_rgba(1., r, g, b, 0.);
        let _ = cr.set_source(&halo);
        cr.arc(0., 0., 1., 0., std::f64::consts::TAU);
        let _ = cr.fill();
        let _ = cr.restore();
    }
}

fn column(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, spacing)
}
fn card(title: &str, icon: &str) -> gtk::Box {
    let panel = column(12);
    panel.add_css_class("gs-home-card");
    panel.set_hexpand(true);
    let heading = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let image = self::icon(icon, 22);
    image.add_css_class("gs-home-icon");
    heading.append(&image);
    heading.append(&text(title, "gs-home-heading"));
    panel.append(&heading);
    panel
}
fn action(
    stack: &gtk::Stack,
    title: &str,
    subtitle: &str,
    icon: &str,
    target: &str,
    purple: bool,
) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("gs-home-action");
    if purple {
        button.add_css_class("gs-home-aurelia");
    }
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let image = self::icon(icon, 24);
    content.append(&image);
    let labels = column(3);
    labels.append(&text(title, "gs-home-button-title"));
    labels.append(&text(subtitle, "gs-home-caption"));
    content.append(&labels);
    button.set_child(Some(&content));
    button.set_tooltip_text(Some(subtitle));
    let weak = stack.downgrade();
    let target = target.to_owned();
    button.connect_clicked(move |_| {
        if let Some(stack) = weak.upgrade() {
            stack.set_visible_child_name(&target);
        }
    });
    button
}
fn controls(stack: &gtk::Stack) -> gtk::Button {
    let button = action(
        stack,
        "Contrôle GPU",
        "Ventilation · Puissance · Fréquences",
        "preferences-system-symbolic",
        "Carte graphique",
        false,
    );
    button.connect_clicked(|_| crate::gpu_page::show_controls());
    button
}
fn adaptive_row(children: &[&gtk::Box]) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    row.set_homogeneous(true);
    for child in children {
        row.append(*child);
    }
    row
}
fn compact(bin: &adw::BreakpointBin, widgets: &[&gtk::Box], width: f64) -> adw::Breakpoint {
    let condition = adw::BreakpointCondition::new_length(
        adw::BreakpointConditionLengthType::MaxWidth,
        width,
        adw::LengthUnit::Px,
    );
    let breakpoint = adw::Breakpoint::new(condition);
    for widget in widgets {
        breakpoint.add_setter(
            *widget,
            "orientation",
            Some(&gtk::Orientation::Vertical.to_value()),
        );
    }
    bin.add_breakpoint(breakpoint.clone());
    breakpoint
}
fn align_ring_heading(heading: &gtk::Box) {
    heading.set_halign(gtk::Align::Center);
    heading.set_height_request(26);
    if let Some(caption) = heading.last_child().and_downcast::<gtk::Label>() {
        caption.set_tooltip_text(Some(&caption.text()));
        caption.set_wrap(false);
        caption.set_ellipsize(gtk::pango::EllipsizeMode::End);
        caption.set_max_width_chars(11);
    }
}
#[derive(Clone)]
struct Value {
    widget: gtk::Box,
    label: gtk::Label,
    bar: gtk::ProgressBar,
    detail: gtk::Label,
    icon: gtk::DrawingArea,
    ring: Option<(gtk::DrawingArea, Rc<Cell<Option<f64>>>)>,
}
impl Value {
    fn new(title: &str, icon: &str, progress: bool) -> Self {
        let widget = column(5);
        widget.set_hexpand(true);
        let heading = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let image = self::icon(icon, 17);
        image.add_css_class("gs-home-icon");
        heading.append(&image);
        heading.append(&text(title, "gs-home-caption"));
        widget.append(&heading);
        let label = text("—", "gs-home-number");
        widget.append(&label);
        let detail = text("", "gs-home-memory-detail");
        detail.set_visible(false);
        widget.append(&detail);
        let bar = gtk::ProgressBar::new();
        bar.add_css_class("gs-home-bar");
        bar.set_visible(progress);
        widget.append(&bar);
        Self {
            widget,
            label,
            bar,
            detail,
            icon: image,
            ring: None,
        }
    }
    fn circular(&mut self) {
        self.widget.set_valign(gtk::Align::Start);
        if let Some(heading) = self.widget.first_child().and_downcast::<gtk::Box>() {
            align_ring_heading(&heading);
        }
        // Reserve the same footer height even when only VRAM has a detail line.
        self.detail.set_visible(true);
        self.detail.set_height_request(16);
        let area = gtk::DrawingArea::new();
        area.set_content_width(108);
        area.set_content_height(108);
        let fraction = Rc::new(Cell::new(None::<f64>));
        let data = fraction.clone();
        area.set_draw_func(move |area, cr, w, h| {
            let (x, y) = (w as f64 / 2., h as f64 / 2.);
            let radius = x.min(y) - 10.;
            cr.set_line_width(9.);
            cr.set_source_rgba(0.28, 0.40, 0.45, 0.55);
            cr.arc(x, y, radius.max(1.), 0., std::f64::consts::TAU);
            let _ = cr.stroke();
            if let Some(value) = data.get() {
                let color = area.color();
                cr.set_source_rgba(
                    color.red() as f64,
                    color.green() as f64,
                    color.blue() as f64,
                    1.,
                );
                cr.set_line_cap(gtk::cairo::LineCap::Round);
                let start = -std::f64::consts::FRAC_PI_2;
                if value > 0. {
                    cr.arc(
                        x,
                        y,
                        radius.max(1.),
                        start,
                        start + std::f64::consts::TAU * value.clamp(0., 1.),
                    );
                    let _ = cr.stroke();
                }
            }
        });
        self.widget.remove(&self.label);
        let overlay = gtk::Overlay::new();
        overlay.set_halign(gtk::Align::Center);
        overlay.set_valign(gtk::Align::Start);
        overlay.set_child(Some(&area));
        self.label.set_halign(gtk::Align::Center);
        self.label.set_valign(gtk::Align::Center);
        self.label.add_css_class("gs-home-ring-number");
        overlay.add_overlay(&self.label);
        self.widget
            .insert_child_after(&overlay, self.widget.first_child().as_ref());
        self.bar.set_visible(false);
        self.ring = Some((area, fraction));
    }
    fn set(&self, value: Option<f64>, suffix: &str) {
        if let Some((area, fraction)) = &self.ring {
            let maximum = match suffix {
                "°C" => 100.,
                "GHz" => 6.,
                "W" => 500.,
                _ => 100.,
            };
            fraction.set(
                value
                    .filter(|v| v.is_finite())
                    .map(|v| (v / maximum).clamp(0., 1.)),
            );
            area.set_tooltip_text(Some(&format!("Échelle visuelle : 0–{maximum:.0} {suffix}. La valeur mesurée reste exacte au-delà ; cette échelle n’indique pas une limite matérielle.")));
            area.queue_draw();
        }
        if value.is_some_and(|v| v.is_finite()) {
            self.widget.remove_css_class("gs-home-unavailable");
        } else {
            self.widget.add_css_class("gs-home-unavailable");
        }
        self.label.set_text(
            &value
                .filter(|v| v.is_finite())
                .map(|v| format!("{v:.1} {suffix}"))
                .unwrap_or_else(|| "—".into()),
        );
    }
    fn fraction(&self, fraction: Option<f64>) {
        self.bar.set_fraction(fraction.unwrap_or(0.).clamp(0., 1.));
        self.bar
            .set_visible(fraction.is_some() && self.ring.is_none());
        if let Some((area, value)) = &self.ring {
            value.set(fraction);
            area.queue_draw();
        }
    }
    fn memory(&self, pair: Option<(f64, f64)>) {
        if pair.is_some() {
            self.widget.remove_css_class("gs-home-unavailable");
        } else {
            self.widget.add_css_class("gs-home-unavailable");
        }
        self.label.set_text(
            &pair
                .and_then(|(used, total)| ratio(used, total).map(|r| format!("{:.0} %", r * 100.)))
                .unwrap_or_else(|| "—".into()),
        );
        self.detail
            .set_visible(pair.is_some() || self.ring.is_some());
        self.detail.set_text(
            &pair
                .and_then(|(used, total)| {
                    ratio(used, total).map(|_| {
                        format!("{:.1} / {:.0} Gio", used / 1073741824., total / 1073741824.)
                    })
                })
                .unwrap_or_default(),
        );
        self.label.set_tooltip_text(
            pair.map(|(used, total)| {
                format!("{:.1} / {:.1} Gio", used / 1073741824., total / 1073741824.)
            })
            .as_deref(),
        );
        self.fraction(pair.and_then(|(used, total)| ratio(used, total)));
    }
}
fn temperature_fraction(value: Option<f64>) -> Option<f64> {
    value
        .filter(|v| v.is_finite() && (-40. ..=150.).contains(v))
        .map(|v| (v / 100.).clamp(0., 1.))
}
fn temperature_row(title: &str, cpu: bool) -> Value {
    let value = Value::new(
        title,
        if cpu {
            "temperature-symbolic"
        } else {
            "video-display-symbolic"
        },
        true,
    );
    value.widget.set_orientation(gtk::Orientation::Horizontal);
    value.widget.add_css_class("gs-home-temperature-row");
    if cpu {
        value.widget.add_css_class("gs-home-temperature-cpu");
    }
    value.icon.set_content_width(14);
    value.icon.set_content_height(14);
    value.label.set_xalign(1.);
    value.label.set_wrap(false);
    value.label.set_hexpand(false);
    value.label.set_width_chars(5);
    value.bar.set_hexpand(true);
    value.bar.set_width_request(48);
    if let Some(heading) = value.widget.first_child() {
        heading.set_hexpand(false);
        if let Some(label) = heading.last_child().and_downcast::<gtk::Label>() {
            label.set_wrap(false);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            label.set_max_width_chars(15);
            label.set_width_chars(13);
            label.set_hexpand(false);
        }
    }
    value
}
fn update_temperature(value: &Value, reading: Option<f64>) {
    let reading = reading.filter(|v| v.is_finite() && (-40. ..=150.).contains(v));
    value.set(reading, "°C");
    if let Some(reading) = reading {
        value.label.set_text(&format!("{reading:.0} °C"));
        value
            .label
            .set_tooltip_text(Some(&format!("{reading:.1} °C · valeur mesurée")));
    }
    value
        .bar
        .set_fraction(temperature_fraction(reading).unwrap_or(0.));
    value.bar.set_visible(true);
    value.bar.set_tooltip_text(Some("Échelle visuelle 0–100 °C, sans seuil critique. Au-delà de 100 °C la jauge est pleine ; la valeur reste exacte. Une mesure absente est indiquée par un tiret et une jauge grisée."));
}
type GaugeTransition = (Option<f64>, Option<f64>, Instant);
#[derive(Clone)]
struct Gauge {
    widget: gtk::Overlay,
    area: gtk::DrawingArea,
    label: gtk::Label,
    caption: gtk::Label,
    value: Rc<RefCell<Option<f64>>>,
    transition: Rc<RefCell<GaugeTransition>>,
    animating: Rc<Cell<bool>>,
}
impl Gauge {
    fn new(title: &str) -> Self {
        let area = gtk::DrawingArea::new();
        area.set_content_width(108);
        area.set_content_height(108);
        let widget = gtk::Overlay::new();
        widget.set_child(Some(&area));
        widget.set_halign(gtk::Align::Center);
        widget.set_valign(gtk::Align::Start);
        let center = column(2);
        center.set_halign(gtk::Align::Center);
        center.set_valign(gtk::Align::Center);
        let label = text("—", "gs-home-gauge-value");
        label.set_xalign(0.5);
        center.append(&label);
        let caption = text("Utilisation", "gs-home-caption");
        caption.set_xalign(0.5);
        center.append(&caption);
        widget.add_overlay(&center);
        let value: Rc<RefCell<Option<f64>>> = Rc::new(RefCell::new(None));
        let data = value.clone();
        area.set_draw_func(move |_, cr, width, height| {
            let x = width as f64 / 2.;
            let y = height as f64 / 2.;
            let thickness = 9.;
            let radius = (x.min(y) - 10.).max(1.);
            cr.set_line_width(thickness);
            let track =
                gtk::cairo::LinearGradient::new(x - radius, y - radius, x + radius, y + radius);
            track.add_color_stop_rgb(0., 0.29, 0.40, 0.45);
            track.add_color_stop_rgb(0.5, 0.14, 0.24, 0.29);
            track.add_color_stop_rgb(1., 0.2, 0.29, 0.34);
            let _ = cr.set_source(&track);
            cr.arc(x, y, radius, 0., std::f64::consts::TAU);
            let _ = cr.stroke();
            if let Some(percent) = *data.borrow() {
                cr.set_line_cap(gtk::cairo::LineCap::Round);
                let glow =
                    gtk::cairo::LinearGradient::new(x - radius, y - radius, x + radius, y + radius);
                glow.add_color_stop_rgb(0., TEAL.0, TEAL.1, TEAL.2);
                glow.add_color_stop_rgb(0.5, 0., 0.61, 0.69);
                glow.add_color_stop_rgb(1., 0.16, 1., 0.88);
                let _ = cr.set_source(&glow);
                let start = -std::f64::consts::FRAC_PI_2;
                if percent > 0. {
                    cr.arc(
                        x,
                        y,
                        radius,
                        start,
                        start + std::f64::consts::TAU * percent / 100.,
                    );
                    let _ = cr.stroke();
                }
            }
        });
        widget.set_tooltip_text(Some(title));
        widget.update_property(&[gtk::accessible::Property::Label(title)]);
        Self {
            widget,
            area,
            label,
            caption,
            value,
            transition: Rc::new(RefCell::new((None, None, Instant::now()))),
            animating: Rc::new(Cell::new(false)),
        }
    }
    fn set(&self, value: Option<f64>) {
        let value = percent(value);
        let previous = *self.value.borrow();
        if previous == value && !self.animating.get() {
            return;
        }
        let animate = gtk::Settings::default().is_some_and(|s| s.is_gtk_enable_animations());
        if animate && previous.is_some() && value.is_some() && previous != value {
            *self.transition.borrow_mut() = (previous, value, Instant::now());
            if !self.animating.replace(true) {
                let state = self.transition.clone();
                let displayed = self.value.clone();
                let running = self.animating.clone();
                self.area.add_tick_callback(move |area, _| {
                    let (from, to, since) = *state.borrow();
                    let t = (since.elapsed().as_secs_f64() / 0.28).min(1.);
                    if !area.is_mapped()
                        || !gtk::Settings::default().is_some_and(|s| s.is_gtk_enable_animations())
                        || t >= 1.
                        || from.is_none()
                        || to.is_none()
                    {
                        *displayed.borrow_mut() = to;
                        running.set(false);
                        area.queue_draw();
                        return glib::ControlFlow::Break;
                    }
                    *displayed.borrow_mut() = from
                        .zip(to)
                        .map(|(a, b)| tween(a, b, since.elapsed().as_secs_f64()));
                    area.queue_draw();
                    glib::ControlFlow::Continue
                });
            }
        } else {
            *self.value.borrow_mut() = value;
            *self.transition.borrow_mut() = (value, value, Instant::now());
        }
        if value.is_none() {
            self.widget.add_css_class("gs-home-unavailable");
        } else {
            self.widget.remove_css_class("gs-home-unavailable");
        }
        self.label.set_text(
            &value
                .map(|v| format!("{v:.0} %"))
                .unwrap_or_else(|| "—".into()),
        );
        self.widget
            .update_property(&[gtk::accessible::Property::Description(
                &value
                    .map(|v| format!("Utilisation mesurée : {v:.1} %"))
                    .unwrap_or_else(|| "Mesure indisponible".into()),
            )]);
        self.area.queue_draw();
    }
}
fn percent(value: Option<f64>) -> Option<f64> {
    value.filter(|v| v.is_finite() && (0. ..=100.).contains(v))
}
fn tween(from: f64, to: f64, elapsed: f64) -> f64 {
    let t = (elapsed / 0.28).clamp(0., 1.);
    from + (to - from) * (1. - (1. - t).powi(3))
}
fn ratio(used: f64, total: f64) -> Option<f64> {
    (used.is_finite() && total.is_finite() && used >= 0. && total > 0. && used <= total)
        .then_some(used / total)
}
fn metric(values: &[Measurement], kind: Metric) -> Option<f64> {
    values
        .iter()
        .find(|m| m.metric == kind && m.value.is_some())
        .and_then(|m| m.value)
}
fn temperature(values: &[Measurement], names: &[&str]) -> Option<f64> {
    values
        .iter()
        .find(|m| {
            m.metric == Metric::Temperature
                && names
                    .iter()
                    .any(|name| m.label.to_lowercase().contains(name))
        })
        .and_then(|m| m.value)
}
#[cfg(test)]
fn memory(text: &str) -> Option<(f64, f64)> {
    let field = |name: &str| {
        text.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key == name)
                .then(|| value.split_whitespace().next()?.parse::<f64>().ok())
                .flatten()
        })
    };
    let total = field("MemTotal")? * 1024.;
    let available = field("MemAvailable")? * 1024.;
    ratio(total - available, total).map(|_| (total - available, total))
}
fn engine_value_label(
    value: Option<f64>,
    percent: bool,
    state: gameshift_telemetry::engines::State,
) -> String {
    use gameshift_telemetry::engines::State;
    if state == State::Impossible
        || value.is_some_and(|v| !v.is_finite() || v < 0. || (percent && v > 100.))
    {
        return "Mesure impossible".into();
    }
    if state == State::Unavailable {
        return "Non disponible".into();
    }
    match value {
        Some(value) => format!(
            "{} {}",
            format!("{value:.1}").replace('.', ","),
            if percent { "%" } else { "ms/s" }
        ),
        None => "Non disponible".into(),
    }
}
#[derive(Clone)]
struct EnginePanel {
    panel: gtk::Box,
    grid: gtk::FlowBox,
    selector: gtk::DropDown,
    devices: Rc<RefCell<Vec<String>>>,
    status: gtk::Label,
    last_samples: Rc<RefCell<Vec<Option<Instant>>>>,
    tiles: Vec<(gtk::Box, gtk::Label, crate::live_chart::PercentChart)>,
}
impl EnginePanel {
    fn new() -> Self {
        let panel = card("Activité des moteurs GPU", "video-display-symbolic");
        let selector = gtk::DropDown::from_strings(&[]);
        selector.set_tooltip_text(Some(
            "GPU suivi par les graphiques et les capteurs de l’accueil",
        ));
        selector.set_visible(false);
        panel.append(&selector);
        let status = text(
            "Non disponible · aucun compteur GPU accessible",
            "gs-home-caption",
        );
        panel.append(&status);
        panel.append(&text(
            "VCN : % matériel vidéo. GFX / Compute / Encode : ms/s observées, conversion fiable en % indisponible.",
            "gs-home-caption",
        ));
        let grid = gtk::FlowBox::new();
        grid.set_selection_mode(gtk::SelectionMode::None);
        grid.set_min_children_per_line(1);
        grid.set_max_children_per_line(2);
        grid.set_homogeneous(true);
        grid.set_row_spacing(8);
        grid.set_column_spacing(8);
        let mut tiles = Vec::new();
        for (index, title) in ["3D / GFX", "Compute", "Encodage vidéo", "Vidéo / VCN"]
            .iter()
            .enumerate()
        {
            let tile = gtk::Box::new(gtk::Orientation::Vertical, 4);
            tile.add_css_class("gs-home-metric");
            if index == 1 {
                tile.add_css_class("gs-engine-compute");
            }
            let heading = gtk::Box::new(gtk::Orientation::Vertical, 2);
            let name = text(title, "gs-home-heading");
            name.set_hexpand(true);
            name.add_css_class("gs-engine-title");
            let value = text("Non disponible", "gs-home-caption");
            heading.append(&name);
            heading.append(&value);
            tile.append(&heading);
            let chart = if index == 3 {
                crate::live_chart::PercentChart::home(title)
            } else {
                crate::live_chart::PercentChart::rate(title)
            };
            chart.widget.set_content_height(125);
            tile.append(&chart.widget);
            grid.insert(&tile, -1);
            tiles.push((tile, value, chart));
        }
        panel.append(&grid);
        let result = Self {
            panel,
            grid,
            selector,
            devices: Rc::new(RefCell::new(Vec::new())),
            status,
            last_samples: Rc::new(RefCell::new(vec![None; 4])),
            tiles,
        };
        result.update(&[], &[]);
        result
    }
    fn selected(&self) -> Option<String> {
        self.devices
            .borrow()
            .get(self.selector.selected() as usize)
            .cloned()
    }
    fn clear(&self) {
        self.last_samples.borrow_mut().fill(None);
        for (_, _, chart) in &self.tiles {
            chart.clear();
        }
    }
    fn suspend(&self) {
        for (_, _, chart) in &self.tiles {
            chart.push(None);
        }
    }
    fn adapt(&self, height: i32) {
        let (columns, chart_height) = engine_layout(self.panel.width(), height);
        let columns = columns.min(
            self.tiles
                .iter()
                .filter(|(tile, _, _)| tile.is_visible())
                .count()
                .max(1) as u32,
        );
        if self.grid.min_children_per_line() != columns {
            self.grid.set_min_children_per_line(columns);
        }
        if self.grid.max_children_per_line() != columns {
            self.grid.set_max_children_per_line(columns);
        }
        for (_, _, chart) in &self.tiles {
            if chart.widget.content_height() != chart_height {
                chart.widget.set_content_height(chart_height);
            }
        }
    }
    fn update(&self, devices: &[String], readings: &[gameshift_telemetry::engines::Reading]) {
        if self.devices.borrow().as_slice() != devices {
            let selected = self.selected();
            let strings = devices.iter().map(String::as_str).collect::<Vec<_>>();
            self.selector
                .set_model(Some(&gtk::StringList::new(&strings)));
            *self.devices.borrow_mut() = devices.to_vec();
            self.selector.set_selected(
                selected
                    .and_then(|s| devices.iter().position(|p| p == &s))
                    .unwrap_or(0) as u32,
            );
        }
        self.selector.set_visible(devices.len() > 1);
        let mut missing = Vec::new();
        let mut count = 0;
        for (index, (tile, label, chart)) in self.tiles.iter().enumerate() {
            let reading = readings.get(index);
            let detected = reading.is_some_and(|r| r.detected);
            // Hide the FlowBoxChild too: no empty grid cells on partial hardware.
            if let Some(parent) = tile.parent() {
                parent.set_visible(detected);
            }
            tile.set_visible(detected);
            if let Some(reading) = reading {
                if reading.sampled_at != self.last_samples.borrow()[index] {
                    chart.push(reading.value);
                    self.last_samples.borrow_mut()[index] = reading.sampled_at;
                }
            }
            if let Some(reading) = reading {
                label.set_text(&engine_value_label(
                    reading.value,
                    reading.percent,
                    reading.state,
                ));
                let tooltip = format!("{}\n{}", reading.source, reading.detail);
                tile.set_tooltip_text(Some(&tooltip));
                chart.widget.set_tooltip_text(Some(&tooltip));
                chart.set_gap_label(
                    if reading.state == gameshift_telemetry::engines::State::Impossible {
                        "Mesure impossible"
                    } else {
                        "Non disponible"
                    },
                );
                if detected {
                    count += 1;
                } else {
                    missing.push(reading.title);
                }
            }
        }
        self.status.set_tooltip_text(Some(
            &readings
                .iter()
                .map(|r| format!("{} : {}\n{}", r.title, r.source, r.detail))
                .collect::<Vec<_>>()
                .join("\n\n"),
        ));
        self.grid.set_visible(count > 0);
        self.status.set_visible(count == 0 || !missing.is_empty());
        self.status.set_text(&if count == 0 {
            "Non disponible · aucun compteur moteur accessible pour ce GPU".into()
        } else if missing.is_empty() {
            "Historique 60 s · sources et couverture dans les infobulles".into()
        } else {
            format!(
                "Non disponible : {} · sources dans les infobulles",
                missing.join(", ")
            )
        });
    }
}

#[derive(Clone)]
struct HomeTemperatures {
    automatic: [Value; 4],
    names: gtk::SizeGroup,
    numbers: gtk::SizeGroup,
    custom: gtk::Box,
    rows: Rc<RefCell<Vec<(String, Value)>>>,
}
impl HomeTemperatures {
    fn align(&self, value: &Value) {
        if let Some(heading) = value.widget.first_child() {
            self.names.add_widget(&heading);
        }
        self.numbers.add_widget(&value.label);
    }
    fn update(&self, sensors: &[crate::home_sensors::Sensor], automatic: [Option<f64>; 4]) {
        let selected = crate::home_sensors::selected();
        for (value, reading) in self.automatic.iter().zip(automatic) {
            value.widget.set_visible(selected.is_none());
            update_temperature(value, reading);
        }
        self.custom.set_visible(selected.is_some());
        let Some(ids) = selected else {
            return;
        };
        if self.rows.borrow().iter().map(|(id, _)| id).ne(ids.iter()) {
            while let Some(child) = self.custom.first_child() {
                self.custom.remove(&child);
            }
            let mut rows = self.rows.borrow_mut();
            for (_, value) in rows.iter() {
                if let Some(heading) = value.widget.first_child() {
                    self.names.remove_widget(&heading);
                }
                self.numbers.remove_widget(&value.label);
            }
            rows.clear();
            if ids.is_empty() {
                self.custom.append(&text(
                    "Aucune sonde sélectionnée · Paramètres → Sondes de température",
                    "gs-home-caption",
                ));
            }
            for id in ids {
                let title = sensors
                    .iter()
                    .find(|s| s.id == id)
                    .map(|s| s.label.as_str())
                    .unwrap_or("Sonde indisponible");
                let value = temperature_row(title, title.starts_with("CPU"));
                self.align(&value);
                self.custom.append(&value.widget);
                rows.push((id, value));
            }
        }
        for (id, value) in self.rows.borrow().iter() {
            let sensor = sensors.iter().find(|s| &s.id == id);
            update_temperature(value, sensor.and_then(|s| s.value));
            value.widget.set_tooltip_text(Some(
                &sensor
                    .map(|s| s.source.clone())
                    .unwrap_or_else(|| format!("Sonde indisponible : {id}")),
            ));
        }
    }
}
#[derive(Clone)]
struct HomeProcesses {
    note: gtk::Label,
    rows: Vec<(gtk::Box, gtk::Image, gtk::DrawingArea, gtk::Label)>,
}
struct HomeProcessReading {
    note: String,
    rows: Vec<(String, Option<String>)>,
}
impl HomeProcesses {
    fn update(&self, reading: HomeProcessReading) {
        self.note.set_text(&reading.note);
        self.note.set_visible(!reading.note.is_empty());
        for (index, (row, image, fallback, label)) in self.rows.iter().enumerate() {
            row.set_visible(index < reading.rows.len());
            if let Some((value, icon)) = reading.rows.get(index) {
                label.set_text(value);
                let icon = icon
                    .as_ref()
                    .and_then(|s| gtk::gio::Icon::for_string(s).ok());
                image.set_visible(icon.is_some());
                fallback.set_visible(icon.is_none());
                if let Some(icon) = icon {
                    image.set_from_gicon(&icon);
                } else {
                    image.clear();
                }
            }
        }
    }
}
// AppInfo::executable() assumes a non-null C path. Desktop entries activated
// through D-Bus may not have an Exec command; commandline() models absence safely.
fn icon_executable(command: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
    let arguments = glib::shell_parse_argv(command?.to_str()?).ok()?;
    let executable = arguments.first().filter(|s| !s.is_empty())?;
    Some(std::path::PathBuf::from(executable.as_os_str()))
}
fn process_icon(executable: Option<&str>, name: &str) -> Option<String> {
    use std::collections::BTreeMap;
    static ICONS: std::sync::OnceLock<BTreeMap<String, Option<String>>> =
        std::sync::OnceLock::new();
    let icons = ICONS.get_or_init(|| {
        let mut icons = BTreeMap::new();
        for app in gtk::gio::AppInfo::all() {
            let Some(executable) = icon_executable(app.commandline().as_deref()) else {
                continue;
            };
            let Some(icon) = app
                .icon()
                .and_then(|i| i.to_string())
                .map(|s| s.to_string())
            else {
                continue;
            };
            for key in [
                executable.to_string_lossy().into_owned(),
                executable
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            ] {
                icons
                    .entry(key)
                    .and_modify(|old: &mut Option<String>| {
                        if old.as_ref() != Some(&icon) {
                            *old = None;
                        }
                    })
                    .or_insert_with(|| Some(icon.clone()));
            }
        }
        icons
    });
    executable
        .and_then(|s| icons.get(s))
        .and_then(Clone::clone)
        .or_else(|| {
            executable
                .and_then(|s| std::path::Path::new(s).file_name())
                .and_then(|s| s.to_str())
                .or(Some(name))
                .and_then(|s| icons.get(s))
                .and_then(Clone::clone)
        })
}

fn engine_layout(width: i32, height: i32) -> (u32, i32) {
    let columns = if width >= 640 || (height < 950 && width >= 480) {
        4
    } else if width >= 300 {
        2
    } else {
        1
    };
    let chart_height = if height < 850 {
        80
    } else if height < 1100 {
        100
    } else {
        140
    };
    (columns, chart_height)
}
struct Request {
    generation: u64,
    reset: bool,
    processes: bool,
    selected: Option<String>,
    cadence: Duration,
}
struct Reading {
    generation: u64,
    cpu: Option<f64>,
    cpu_temperature: Option<f64>,
    cpu_power: Option<gameshift_cpu::power::Reading>,
    frequency: Option<f64>,
    temperatures: Vec<crate::home_sensors::Sensor>,
    fan_curve: Option<gameshift_control::Curve>,
    fan_pwm: Option<f64>,
    gpu_id: Option<String>,
    gpu: Vec<Measurement>,
    engines: Vec<gameshift_telemetry::engines::Reading>,
    devices: Vec<String>,
    processes: Option<HomeProcessReading>,
    profile: Option<String>,
    sources: String,
}
fn fan_pwm(readings: &[Measurement]) -> Option<f64> {
    let source = readings.iter().find(|r| r.metric == Metric::FanSpeed)?;
    let path = std::path::Path::new(&source.source);
    let channel = path
        .file_name()?
        .to_str()?
        .strip_prefix("fan")?
        .strip_suffix("_input")?;
    if channel.is_empty() || !channel.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    pwm_percent(&std::fs::read_to_string(path.with_file_name(format!("pwm{channel}"))).ok()?)
}
fn pwm_percent(raw: &str) -> Option<f64> {
    raw.trim()
        .parse::<u16>()
        .ok()
        .filter(|v| *v <= 255)
        .map(|v| v as f64 * 100. / 255.)
}
fn profile() -> String {
    let result = (|| {
        let path = gameshift_profiles::runtime()?.join("profiles.sock");
        let mut stream =
            std::os::unix::net::UnixStream::connect(path).map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_millis(250)))
            .map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_millis(250)))
            .map_err(|e| e.to_string())?;
        gameshift_profiles::exchange(&mut stream, &serde_json::json!({"command":"get"}))
    })();
    match result {
        Ok(status) => status["active"]
            .as_str()
            .map(|id| {
                status["config"]["profiles"]
                    .as_array()
                    .and_then(|profiles| profiles.iter().find(|p| p["id"].as_str() == Some(id)))
                    .and_then(|p| p["name"].as_str())
                    .unwrap_or("Profil actif non enregistré")
                    .to_owned()
            })
            .unwrap_or_else(|| "Aucun profil actif".into()),
        Err(_) => "Agent indisponible".into(),
    }
}
fn collect(
    request: Request,
    previous: &mut Option<[u64; 2]>,
    power: &mut gameshift_cpu::power::Sampler,
    temperatures: &mut crate::home_sensors::Sampler,
) -> Reading {
    if request.reset {
        *previous = None;
        power.reset();
    }
    let ticks = parse_cpu_ticks(&std::fs::read_to_string("/proc/stat").unwrap_or_default());
    let ids = ticks
        .keys()
        .filter_map(|k| k.strip_prefix("cpu")?.parse().ok())
        .collect::<Vec<usize>>();
    let mut cpu = CpuSnapshot::read(&ids);
    cpu.package_power = Some(power.sample());
    let now = cpu.cpu_ticks.get("cpu").copied();
    let load = previous.zip(now).and_then(|(a, b)| busy_percent(a, b));
    *previous = now;
    let mut sources = String::from("CPU : /proc/stat, cpufreq, hwmon CPU.\n");
    if let Some(power) = &cpu.package_power {
        sources.push_str(&format!("{}\n", power.detail));
    }
    let mut gpu = Vec::new();
    let inventory = gameshift_telemetry::inventory::current();
    let devices: Vec<String> = inventory
        .gpus
        .iter()
        .filter(|g| {
            g.driver.as_deref() == Some("amdgpu")
                && g.nodes.iter().any(|n| n.starts_with("/dev/dri/card"))
        })
        .map(|g| g.pci.clone())
        .collect();
    for error in &inventory.errors {
        sources.push_str(&format!("{error}\n"));
    }
    let selected = request
        .selected
        .as_ref()
        .filter(|pci| devices.contains(pci))
        .or(devices.first());
    let gpu_id = selected.cloned();
    if let Some(pci) = selected {
        gpu = inventory.read_device(pci);
        sources.push_str(&format!("GPU sélectionné : {pci}\n"));
        gameshift_telemetry::inventory::failed(
            !inventory.errors.is_empty() || gpu.iter().any(|m| m.error.is_some()),
        );
    }
    if gpu_id.is_none() {
        sources.push_str("Aucun GPU amdgpu avec capteurs découvert ; les autres pilotes ne sont pas assimilés à une Radeon.\n");
    }
    for reading in &gpu {
        sources.push_str(&format!(
            "{} : {} · Unix {} ms{}\n",
            reading.label,
            reading.source,
            reading.captured_unix_millis,
            reading
                .error
                .as_ref()
                .map(|e| format!(" · {e}"))
                .unwrap_or_default()
        ));
    }
    let shared = gpu_id.as_ref().map(|_| shared_snapshot(request.cadence));
    let mut engines = shared
        .as_ref()
        .zip(gpu_id.as_ref())
        .map(|(snapshot, pci)| {
            gameshift_telemetry::engines::readings(std::path::Path::new("/"), pci, snapshot)
        })
        .unwrap_or_default();
    if request.reset {
        for engine in &mut engines {
            if !engine.percent {
                engine.value = None;
            }
        }
    }
    let processes = if request.processes {
        let mut snapshot = shared.as_deref().cloned().unwrap_or_default();
        snapshot
            .rows
            .retain(|row| Some(&row.pci) == gpu_id.as_ref());
        snapshot.rows.sort_by(|a, b| {
            b.activity()
                .unwrap_or(-1.)
                .total_cmp(&a.activity().unwrap_or(-1.))
                .then_with(|| a.pid.cmp(&b.pid))
        });
        let lines = snapshot
            .rows
            .iter()
            .take(3)
            .map(|row| {
                let activity = row
                    .activity()
                    .map(|v| format!("{v:.1} %"))
                    .unwrap_or_else(|| "Premier relevé".into());
                let memory = row
                    .memory_bytes
                    .get("vram")
                    .or_else(|| row.memory_bytes.get("local"))
                    .map(|v| format!("{:.1} Mio", *v as f64 / 1_048_576.))
                    .unwrap_or_else(|| "VRAM indisponible".into());
                (
                    format!("{} · {memory} · {activity}", row.name),
                    process_icon(row.executable.as_deref(), &row.name),
                )
            })
            .collect::<Vec<_>>();
        Some(HomeProcessReading {
            note: if lines.is_empty() {
                format!(
                    "Aucun processus DRM lisible\n{} accès refusé(s) · processus sans compteur non affichés.",
                    snapshot.denied
                )
            } else if snapshot.truncated {
                "Collecte partielle".into()
            } else {
                String::new()
            },
            rows: lines,
        })
    } else {
        None
    };
    Reading {
        temperatures: temperatures.sample_at(std::path::Path::new("/"), Instant::now()),
        fan_pwm: fan_pwm(&gpu),
        fan_curve: gpu_id.as_ref().and_then(|pci| inventory.fan_curve(pci)),
        generation: request.generation,
        cpu: load,
        cpu_temperature: cpu.temperatures.iter().map(|t| t.celsius).reduce(f64::max),
        cpu_power: cpu.package_power,
        frequency: cpu
            .frequency_khz
            .values()
            .filter_map(|v| *v)
            .max()
            .map(|v| v as f64 / 1_000_000.),
        gpu_id,
        gpu,
        engines,
        devices,
        processes,
        profile: request.processes.then(profile),
        sources,
    }
}
#[derive(Clone)]
struct HardwareCard {
    panel: gtk::Box,
    gauge: Gauge,
    chart: crate::live_chart::PercentChart,
    values: Vec<Value>,
    identity: gtk::Label,
}
fn hardware(
    stack: &gtk::Stack,
    title: &str,
    identity: &str,
    gpu: bool,
    icon: &str,
) -> HardwareCard {
    let panel = card(title, icon);
    panel.set_spacing(6);
    panel.add_css_class("gs-home-hardware");
    if gpu {
        panel.add_css_class("gs-home-gpu");
    }
    let name = text(identity, "gs-home-caption");
    name.set_wrap(false);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    name.set_max_width_chars(28);
    name.set_tooltip_text(Some(identity));
    let heading = panel
        .first_child()
        .and_downcast::<gtk::Box>()
        .expect("card header");
    if let Some(title) = heading.last_child() {
        title.set_hexpand(false);
    }
    heading.append(&name);
    let gauge = Gauge::new(if gpu {
        "Utilisation du GPU AMD sélectionné"
    } else {
        "Utilisation globale du CPU"
    });
    gauge.caption.set_visible(false);
    let flow = gtk::FlowBox::new();
    flow.set_valign(gtk::Align::Start);
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_min_children_per_line(1);
    flow.set_max_children_per_line(if gpu { 5 } else { 4 });
    flow.set_homogeneous(true);
    flow.set_row_spacing(4);
    flow.set_column_spacing(6);
    flow.set_hexpand(true);
    let utilization = column(5);
    utilization.set_valign(gtk::Align::Start);
    let heading = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let image = self::icon(icon, 17);
    image.add_css_class("gs-home-icon");
    heading.append(&image);
    heading.append(&text("Utilisation", "gs-home-caption"));
    align_ring_heading(&heading);
    utilization.append(&heading);
    utilization.append(&gauge.widget);
    let footer = text("", "gs-home-memory-detail");
    footer.set_height_request(16);
    utilization.append(&footer);
    flow.insert(&utilization, -1);
    let mut values = Vec::new();
    for (name, icon) in if gpu {
        vec![
            ("Température", "temperature-symbolic"),
            ("Jonction", "temperature-symbolic"),
            ("Puissance", "ac-adapter-symbolic"),
            ("VRAM", "media-flash-symbolic"),
        ]
    } else {
        vec![
            ("Temp. CPU", "temperature-symbolic"),
            ("Puissance", "ac-adapter-symbolic"),
            ("Fréq. observée", "preferences-system-symbolic"),
        ]
    } {
        let mut value = Value::new(name, icon, false);
        value.circular();
        if name.contains("emp") || name == "Jonction" {
            value.widget.add_css_class("gs-home-temperature");
        }
        flow.insert(&value.widget, -1);
        values.push(value);
    }
    panel.append(&flow);
    let chart = crate::live_chart::PercentChart::home(if gpu {
        "Charge GPU · 60 secondes"
    } else {
        "Charge CPU · 60 secondes"
    });
    chart.widget.add_css_class("gs-home-chart");
    chart.widget.set_content_height(84);
    panel.append(&chart.widget);
    let link = gtk::Button::with_label(if gpu {
        "Détails GPU →"
    } else {
        "Détails CPU →"
    });
    link.add_css_class("gs-home-detail-action");
    link.set_halign(gtk::Align::End);
    let weak = stack.downgrade();
    link.connect_clicked(move |_| {
        if let Some(s) = weak.upgrade() {
            s.set_visible_child_name(if gpu { "Carte graphique" } else { "Processeur" });
        }
    });
    let header = panel
        .first_child()
        .and_downcast::<gtk::Box>()
        .expect("card header");
    link.set_label("↗");
    link.set_tooltip_text(Some(if gpu {
        "Ouvrir les détails et contrôles GPU"
    } else {
        "Ouvrir les détails CPU"
    }));
    header.append(&link);
    HardwareCard {
        panel,
        gauge,
        chart,
        values,
        identity: name,
    }
}
fn tile(view: &View, target: &str) -> String {
    view.tiles
        .iter()
        .find(|t| t.target == target)
        .map(|t| t.value.clone())
        .unwrap_or_else(|| "—".into())
}

pub fn page(stack: &gtk::Stack, view: &View, last: Option<&LastReport>) -> adw::BreakpointBin {
    let root = column(8);
    root.set_valign(gtk::Align::Start);
    root.add_css_class("gs-home");
    root.set_margin_start(12);
    root.set_margin_end(12);
    root.set_margin_top(0);
    root.set_margin_bottom(8);
    root.append(&text("Accueil", "gs-home-title"));
    root.append(&text(
        "Vue d’ensemble de votre machine · Surveillance en temps réel · Accès rapide",
        "gs-home-subtitle",
    ));
    let bin = adw::BreakpointBin::new();
    let identity = action(
        stack,
        &format!(
            "{} · {}",
            tile(view, "Processeur"),
            tile(view, "Carte graphique")
        ),
        &format!(
            "{} · {} de RAM",
            tile(view, "Système"),
            tile(view, "Mémoire")
        ),
        "computer-symbolic",
        "Système",
        false,
    );
    identity.add_css_class("gs-home-identity");
    if let Some(label) = identity
        .child()
        .and_then(|w| w.last_child())
        .and_then(|w| w.first_child())
        .and_downcast::<gtk::Label>()
    {
        label.set_wrap(false);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(43);
    }
    identity.set_tooltip_text(Some(
        &view
            .tiles
            .iter()
            .map(|t| format!("{} : {} · {}", t.title, t.value, t.detail))
            .collect::<Vec<_>>()
            .join("\n"),
    ));
    let summary = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    summary.add_css_class("gs-home-summary");
    summary.append(&identity);
    let profile_label = text("En attente…", "gs-home-profile");
    let profile_box = column(5);
    profile_box.append(&text("Profil actif", "gs-home-caption"));
    profile_box.append(&profile_label);
    let profile_button = gtk::Button::new();
    profile_button.set_child(Some(&profile_box));
    profile_button.add_css_class("gs-home-chip");
    let weak = stack.downgrade();
    profile_button.connect_clicked(move |_| {
        if let Some(s) = weak.upgrade() {
            s.set_visible_child_name("profiles");
        }
    });
    summary.append(&profile_button);
    let previous = action(
        stack,
        "Aurelia",
        last.and_then(|r| r.description.lines().next())
            .unwrap_or("Aucun résultat"),
        "applications-science-symbolic",
        "laboratory",
        false,
    );
    previous.add_css_class("gs-home-chip");
    previous.remove_css_class("gs-home-action");
    summary.append(&previous);
    let live_state = text("● En attente", "gs-home-live-state");
    let state = column(3);
    state.append(&text("Collecte · lecture seule", "gs-home-caption"));
    state.append(&live_state);
    summary.append(&state);
    root.append(&summary);
    // The bin owns responsive setters; all other application pages retain their styles.
    let columns = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    columns.set_hexpand(true);
    let primary = column(10);
    primary.set_hexpand(true);
    let rail = column(10);
    rail.set_width_request(210);
    rail.set_hexpand(false);
    let cpu_icon = view
        .tiles
        .iter()
        .find(|t| t.target == "Processeur")
        .map(|t| t.icon)
        .unwrap_or("preferences-system-symbolic");
    let gpu_icon = view
        .tiles
        .iter()
        .find(|t| t.target == "Carte graphique")
        .map(|t| t.icon)
        .unwrap_or("video-display-symbolic");
    let cpu = hardware(stack, "CPU", &tile(view, "Processeur"), false, cpu_icon);
    let gpu = hardware(stack, "GPU", &tile(view, "Carte graphique"), true, gpu_icon);
    let hardware_row = adaptive_row(&[&cpu.panel, &gpu.panel]);
    primary.append(&hardware_row);
    let engines = EnginePanel::new();
    let metrics = engines.panel.clone();
    let metrics_grid = engines.grid.clone();
    let temperatures = card("Températures", "temperature-symbolic");
    temperatures.add_css_class("gs-home-temperature-card");
    let temp_cpu = temperature_row("CPU · max.", true);
    let temp_edge = temperature_row("GPU (Edge)", false);
    let temp_junction = temperature_row("GPU (Junction)", false);
    let temp_memory = temperature_row("Mem", false);
    for item in [&temp_cpu, &temp_edge, &temp_junction, &temp_memory] {
        update_temperature(item, None);
        temperatures.append(&item.widget);
    }
    let custom_temperatures = column(6);
    custom_temperatures.set_visible(false);
    temperatures.append(&custom_temperatures);
    let temperature_widgets = HomeTemperatures {
        automatic: [temp_cpu, temp_edge, temp_junction, temp_memory],
        names: gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal),
        numbers: gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal),
        custom: custom_temperatures,
        rows: Rc::new(RefCell::new(Vec::new())),
    };
    for value in &temperature_widgets.automatic {
        temperature_widgets.align(value);
    }
    let middle = adaptive_row(&[&metrics, &temperatures]);
    middle.set_homogeneous(false);
    temperatures.set_hexpand(false);
    temperatures.set_width_request(230);
    primary.append(&middle);
    let selected_engine = engines.clone();
    let fan_widgets = crate::home_fan::Panel::new(move || selected_engine.selected());
    let fan = fan_widgets.widget.clone();
    let processes = card("Processus GPU · top 3", "view-list-symbolic");
    let process_label = text("Première collecte en attente…", "gs-home-processes");
    processes.append(&process_label);
    let mut process_rows = Vec::new();
    for _ in 0..3 {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.set_visible(false);
        let image = gtk::Image::new();
        image.set_pixel_size(30);
        image.set_valign(gtk::Align::Center);
        let fallback = icon("applications-games-symbolic", 30);
        fallback.add_css_class("gs-home-icon");
        let label = text("", "gs-home-processes");
        label.set_selectable(true);
        row.append(&image);
        row.append(&fallback);
        row.append(&label);
        processes.append(&row);
        process_rows.push((row, image, fallback, label));
    }
    let process_widgets = HomeProcesses {
        note: process_label,
        rows: process_rows,
    };
    let open_processes = gtk::Button::with_label("Tous les processus GPU →");
    open_processes.add_css_class("gs-home-detail-action");
    let weak = stack.downgrade();
    open_processes.connect_clicked(move |_| {
        if let Some(s) = weak.upgrade() {
            s.set_visible_child_name("Carte graphique");
        }
    });
    processes.append(&open_processes);
    let lower = adaptive_row(&[&fan, &processes]);
    primary.append(&lower);
    let quick = card("Actions rapides", "system-run-symbolic");
    quick.append(&action(
        stack,
        "Lancer Aurelia",
        "Choisir une évaluation",
        "applications-science-symbolic",
        "laboratory",
        true,
    ));
    quick.append(&controls(stack));
    quick.append(&action(
        stack,
        "Profils",
        "Profils et applications",
        "applications-games-symbolic",
        "profiles",
        false,
    ));
    quick.append(&action(
        stack,
        "Historique",
        "Résultats et comparaisons",
        "document-open-recent-symbolic",
        "laboratory",
        false,
    ));
    rail.append(&quick);
    let benchmark = card("Dernier benchmark", "applications-science-symbolic");
    benchmark.add_css_class("gs-home-benchmark");
    let scene = artwork();
    scene.set_content_height(92);
    scene.add_css_class("gs-home-aurelia-art");
    benchmark.append(&scene);
    if let Some(report) = last {
        let result = text(
            report
                .description
                .lines()
                .next()
                .unwrap_or("Résultat disponible"),
            "gs-home-result",
        );
        result.set_max_width_chars(22);
        benchmark.append(&result);
        let detail = text(&report.description, "gs-home-caption");
        detail.set_max_width_chars(28);
        detail.set_selectable(true);
        benchmark.append(&detail);
        let export = gtk::Button::with_label("Ouvrir le résultat JSON");
        export.add_css_class("gs-home-action");
        let file = gtk::gio::File::for_path(&report.path);
        export.connect_clicked(move |_| {
            gtk::FileLauncher::new(Some(&file)).launch(
                None::<&gtk::Window>,
                None::<&gtk::gio::Cancellable>,
                |result| {
                    if let Err(error) = result {
                        tracing::warn!(%error, "Opening benchmark export failed");
                    }
                },
            )
        });
        benchmark.append(&export);
    } else {
        benchmark.append(&text("Aucun résultat enregistré", "gs-home-heading"));
        benchmark.append(&text("Lancez une évaluation Aurelia pour mesurer votre configuration. Aucun score n’est estimé.", "gs-home-caption"));
    }
    rail.append(&benchmark);
    columns.append(&primary);
    columns.append(&rail);
    root.append(&columns);
    compact(&bin, &[&columns], 850.);
    let narrow = compact(
        &bin,
        &[&columns, &hardware_row, &middle, &lower, &summary],
        620.,
    );
    narrow.add_setter(
        &metrics_grid,
        "min-children-per-line",
        Some(&1u32.to_value()),
    );
    narrow.add_setter(
        &metrics_grid,
        "max-children-per-line",
        Some(&2u32.to_value()),
    );
    let wide = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
        adw::BreakpointConditionLengthType::MinWidth,
        1100.,
        adw::LengthUnit::Px,
    ));
    for chart in [&cpu.chart.widget, &gpu.chart.widget] {
        wide.add_setter(chart, "content-height", Some(&100i32.to_value()));
    }
    wide.add_setter(&temperatures, "width-request", Some(&320i32.to_value()));
    bin.add_breakpoint(wide);
    let spacious = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
        adw::BreakpointConditionLengthType::MinWidth,
        1650.,
        adw::LengthUnit::Px,
    ));
    for chart in [&cpu.chart.widget, &gpu.chart.widget] {
        spacious.add_setter(chart, "content-height", Some(&160i32.to_value()));
    }
    spacious.add_setter(&temperatures, "width-request", Some(&410i32.to_value()));
    spacious.add_setter(&rail, "width-request", Some(&280i32.to_value()));
    bin.add_breakpoint(spacious);

    let banner = action(
        stack,
        "Aurelia",
        "Suite de benchmarks et analyses de performances",
        "applications-science-symbolic",
        "laboratory",
        true,
    );
    banner.add_css_class("gs-home-banner");
    if let Some(content) = banner.child().and_downcast::<gtk::Box>() {
        let launch_label = text("Lancer une évaluation  ›", "gs-home-banner-command");
        launch_label.set_hexpand(false);
        launch_label.set_valign(gtk::Align::Center);
        content.append(&launch_label);
    }
    root.append(&banner);
    let sources = text(
        "Mesures en attente · collecte suspendue hors Accueil et pendant les benchmarks.",
        "gs-home-sources",
    );
    let source_details = gtk::Expander::new(Some("Sources et disponibilité des capteurs"));
    source_details.set_child(Some(&sources));
    root.append(&source_details);
    let note = text(
        &format!(
            "Inventaire au démarrage : Unix {} · Actualiser pour renouveler la configuration matérielle.",
            view.captured
        ),
        "gs-home-caption",
    );
    source_details.set_tooltip_text(Some(note.text().as_str()));
    start_live(
        stack,
        &root,
        cpu,
        gpu,
        engines,
        temperature_widgets,
        fan_widgets,
        process_widgets,
        profile_label,
        sources,
        live_state,
        hardware_row,
    );
    root.set_hexpand(true);
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&root)
        .build();
    scroll.add_css_class("gs-home-scroll");
    bin.set_size_request(320, 280);
    bin.set_hexpand(true);
    bin.set_vexpand(true);
    let atmosphere = gtk::Overlay::new();
    atmosphere.set_child(Some(&backdrop(Some(&banner))));
    atmosphere.add_overlay(&scroll);
    atmosphere.set_measure_overlay(&scroll, true);
    bin.set_child(Some(&atmosphere));
    bin
}
#[allow(clippy::too_many_arguments)]
fn start_live(
    stack: &gtk::Stack,
    root: &gtk::Box,
    cpu: HardwareCard,
    gpu: HardwareCard,
    engines: EnginePanel,
    temps: HomeTemperatures,
    fan_widgets: crate::home_fan::Panel,
    processes: HomeProcesses,
    profile_label: gtk::Label,
    sources: gtk::Label,
    live_state: gtk::Label,
    hardware_row: gtk::Box,
) {
    let (request_tx, request_rx) = mpsc::sync_channel::<Request>(1);
    let (reading_tx, reading_rx) = mpsc::sync_channel(1);
    if let Err(error) = std::thread::Builder::new()
        .name("gameshift-home-sensors".into())
        .spawn(move || {
            let mut previous = None;
            let mut power = gameshift_cpu::power::Sampler::default();
            let mut temperatures = crate::home_sensors::Sampler::default();
            while let Ok(request) = request_rx.recv() {
                if reading_tx
                    .send(collect(
                        request,
                        &mut previous,
                        &mut power,
                        &mut temperatures,
                    ))
                    .is_err()
                {
                    break;
                }
            }
        })
    {
        sources.set_text(&format!("Collecte indisponible : {error}"));
        return;
    }
    let weak_root = root.downgrade();
    let weak_stack = stack.downgrade();
    let mut pending = false;
    let mut visible = false;
    let mut generation = 0;
    let mut reset = true;
    let mut last_request = Instant::now() - Duration::from_secs(10);
    let mut last_process = last_request;
    let mut gpu_id = None;
    glib::timeout_add_local(Duration::from_millis(100), move || {
        let (Some(_root), Some(stack)) = (weak_root.upgrade(), weak_stack.upgrade()) else {
            return glib::ControlFlow::Break;
        };
        if _root.is_mapped() {
            let viewport_height = _root
                .ancestor(gtk::ScrolledWindow::static_type())
                .map(|w| w.height())
                .unwrap_or(752);
            engines.adapt(viewport_height);
            let compact = viewport_height < 900;
            if compact {
                _root.add_css_class("gs-home-compact-height");
            } else {
                _root.remove_css_class("gs-home-compact-height");
            }
            fan_widgets.adapt(compact);
            let chart_height = if compact {
                80
            } else if _root.width() >= 1650 {
                160
            } else if _root.width() >= 1100 {
                100
            } else {
                84
            };
            for chart in [&cpu.chart.widget, &gpu.chart.widget] {
                if chart.content_height() != chart_height {
                    chart.set_content_height(chart_height);
                }
            }
        }
        let active = stack.visible_child_name().as_deref() == Some("overview")
            && !crate::live_settings::benchmark_running()
            && !crate::amd_controls::any_active()
            && !crate::profiles_page::pending_apply();
        if !active && visible {
            generation += 1;
            reset = true;
            cpu.chart.push(None);
            gpu.chart.push(None);
            engines.suspend();
            cpu.gauge.set(None);
            gpu.gauge.set(None);
        }
        if active != visible {
            live_state.set_text(if active {
                "● Active"
            } else {
                "Ⅱ Suspendue"
            });
        }
        visible = active;
        match reading_rx.try_recv() {
            Ok(reading) => {
                pending = false;
                if !active || reading.generation != generation {
                    return glib::ControlFlow::Continue;
                }
                live_state.set_text(if reading.cpu.is_some() || !reading.gpu.is_empty() {
                    "● Active"
                } else {
                    "● Partielle"
                });
                cpu.gauge.set(reading.cpu);
                cpu.chart.push(reading.cpu);
                cpu.values[0].set(reading.cpu_temperature, "°C");
                let power = reading.cpu_power.as_ref();
                cpu.values[1].set(power.and_then(|p| p.watts), "W");
                if power.and_then(|p| p.watts).is_none() {
                    cpu.values[1].label.set_text("Non disponible");
                }
                cpu.values[1]
                    .label
                    .set_tooltip_text(power.map(|p| p.detail.as_str()));
                cpu.values[2].set(reading.frequency, "GHz");
                if gpu_id != reading.gpu_id {
                    gpu.chart.clear();
                    engines.clear();
                    gpu_id = reading.gpu_id.clone();
                }
                let utilization = metric(&reading.gpu, Metric::Utilization);
                gpu.gauge.set(utilization);
                gpu.chart.push(utilization);
                let gpu_available = !reading.gpu.is_empty();
                hardware_row.set_homogeneous(true);
                if gpu_available {
                    gpu.panel.remove_css_class("gs-home-no-sensors");
                } else {
                    gpu.panel.add_css_class("gs-home-no-sensors");
                }
                let edge = temperature(&reading.gpu, &["edge"])
                    .or_else(|| metric(&reading.gpu, Metric::Temperature));
                let junction = temperature(&reading.gpu, &["junction", "hotspot"]);
                let power = metric(&reading.gpu, Metric::Power);
                let vram = metric(&reading.gpu, Metric::VramUsed)
                    .zip(metric(&reading.gpu, Metric::VramTotal));
                gpu.values[0].set(edge, "°C");
                gpu.values[1].set(junction, "°C");
                gpu.values[2].set(power, "W");
                gpu.values[3].memory(vram);
                gpu.identity.set_text(
                    &reading
                        .gpu_id
                        .as_ref()
                        .map(|id| format!("Capteurs AMDGPU · {id}"))
                        .unwrap_or_else(|| "Capteurs AMDGPU indisponibles".into()),
                );
                engines.update(&reading.devices, &reading.engines);
                temps.update(
                    &reading.temperatures,
                    [
                        reading.cpu_temperature,
                        temperature(&reading.gpu, &["edge"]),
                        junction,
                        temperature(&reading.gpu, &["mem", "vram"]),
                    ],
                );
                fan_widgets.update(
                    reading.gpu_id.as_ref(),
                    reading.fan_curve,
                    metric(&reading.gpu, Metric::FanSpeed),
                    reading.fan_pwm,
                );
                if let Some(value) = reading.processes {
                    processes.update(value);
                }
                if let Some(value) = reading.profile {
                    if value == "Agent indisponible" {
                        profile_label.add_css_class("gs-home-profile-unavailable");
                    } else {
                        profile_label.remove_css_class("gs-home-profile-unavailable");
                    }
                    profile_label.set_text(&value);
                }
                sources.set_text(&reading.sources);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                live_state.set_text("● Collecte indisponible");
                sources.set_text("Collecte interrompue. Actualiser pour relancer les mesures.");
                return glib::ControlFlow::Break;
            }
            Err(mpsc::TryRecvError::Empty) => (),
        }
        if active && !pending && last_request.elapsed() >= crate::live_settings::interval() {
            let process_due = last_process.elapsed() >= Duration::from_secs(5) || reset;
            if request_tx
                .try_send(Request {
                    generation,
                    reset,
                    processes: process_due,
                    selected: engines.selected(),
                    cadence: crate::live_settings::interval(),
                })
                .is_ok()
            {
                pending = true;
                reset = false;
                last_request = Instant::now();
                if process_due {
                    last_process = last_request;
                }
            }
        }
        glib::ControlFlow::Continue
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn temperature_gauges_are_not_critical_thresholds_and_missing_is_not_zero() {
        assert_eq!(temperature_fraction(None), None);
        assert_eq!(temperature_fraction(Some(f64::NAN)), None);
        assert_eq!(temperature_fraction(Some(0.)), Some(0.));
        assert_eq!(temperature_fraction(Some(52.)), Some(0.52));
        assert_eq!(temperature_fraction(Some(110.)), Some(1.));
        assert_eq!(temperature_fraction(Some(151.)), None);
        assert_eq!(pwm_percent("0"), Some(0.));
        assert_eq!(pwm_percent("255"), Some(100.));
        assert_eq!(pwm_percent("256"), None);
        assert_eq!(pwm_percent("-1"), None);
        assert_eq!(pwm_percent("NaN"), None);
    }
    #[test]
    fn engine_percentages_use_french_decimal_and_distinguish_invalid_from_missing() {
        use gameshift_telemetry::engines::State;
        assert_eq!(
            engine_value_label(Some(8.2), true, State::Available),
            "8,2 %"
        );
        assert_eq!(
            engine_value_label(Some(0.), true, State::Available),
            "0,0 %"
        );
        assert_eq!(
            engine_value_label(None, true, State::Unavailable),
            "Non disponible"
        );
        for value in [Some(100.1), Some(-1.), Some(f64::NAN)] {
            assert_eq!(
                engine_value_label(value, true, State::Available),
                "Mesure impossible"
            );
        }
        assert_eq!(
            engine_value_label(None, true, State::Impossible),
            "Mesure impossible"
        );
        assert_eq!(
            engine_value_label(Some(2000.), false, State::Available),
            "2000,0 ms/s"
        );
    }
    #[test]
    fn engine_grid_responds_to_both_available_width_and_height() {
        assert_eq!(engine_layout(220, 752), (1, 80));
        assert_eq!(engine_layout(400, 752), (2, 80));
        assert_eq!(engine_layout(520, 752), (4, 80));
        assert_eq!(engine_layout(520, 1440), (2, 140));
        assert_eq!(engine_layout(800, 1080), (4, 100));
        assert_eq!(engine_layout(1200, 1440), (4, 140));
    }
    #[test]
    fn application_icons_ignore_missing_or_invalid_exec_commands() {
        use std::{
            os::unix::ffi::OsStrExt,
            path::{Path, PathBuf},
        };
        assert_eq!(icon_executable(None), None);
        for command in ["", "   ", "\"unterminated"] {
            assert_eq!(icon_executable(Some(Path::new(command))), None);
        }
        let invalid = std::ffi::OsStr::from_bytes(b"/bin/\xff");
        assert_eq!(icon_executable(Some(Path::new(invalid))), None);
        assert_eq!(
            icon_executable(Some(Path::new("/usr/bin/firefox %u"))),
            Some(PathBuf::from("/usr/bin/firefox"))
        );
        assert_eq!(
            icon_executable(Some(Path::new("\"/opt/My App/launcher\" --flag %F"))),
            Some(PathBuf::from("/opt/My App/launcher"))
        );
    }

    #[test]
    fn desktop_launchers_without_exec_do_not_crash_icon_collection() {
        if std::env::var_os("GAMESHIFT_TEST_NO_EXEC_CHILD").is_some() {
            let app = gtk::gio::AppInfo::all()
                .into_iter()
                .find(|app| app.id().as_deref() == Some("io.gameshift.NoExec.desktop"))
                .expect("D-Bus desktop fixture must be discovered");
            assert!(app.commandline().is_none());
            assert_eq!(
                process_icon(None, "gameshift-nonexistent-test-process"),
                None
            );
            return;
        }
        let directory = std::env::temp_dir().join(format!(
            "gameshift-no-exec-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(directory.join("applications")).unwrap();
        std::fs::create_dir_all(directory.join("empty")).unwrap();
        std::fs::write(directory.join("applications/io.gameshift.NoExec.desktop"),"[Desktop Entry]\nType=Application\nName=No Exec test\nDBusActivatable=true\nIcon=applications-system\n").unwrap();
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "overview::tests::desktop_launchers_without_exec_do_not_crash_icon_collection",
                "--nocapture",
            ])
            .env("GAMESHIFT_TEST_NO_EXEC_CHILD", "1")
            .env("XDG_DATA_HOME", &directory)
            .env("XDG_DATA_DIRS", directory.join("empty"))
            .env("XDG_CONFIG_HOME", directory.join("config"))
            .env("XDG_CONFIG_DIRS", directory.join("empty"))
            .env("XDG_CACHE_HOME", directory.join("cache"))
            .output()
            .unwrap();
        std::fs::remove_dir_all(&directory).unwrap();
        assert!(
            result.status.success(),
            "Isolated icon lookup failed: {}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
    #[test]
    fn engine_metadata_does_not_change_serialized_process_snapshots() {
        let snapshot = gameshift_telemetry::processes::Snapshot::default();
        let json = serde_json::to_value(snapshot).unwrap();
        let mut keys = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        keys.sort();
        assert_eq!(
            keys,
            [
                "ambiguous",
                "denied",
                "elapsed_seconds",
                "rows",
                "truncated",
                "vanished"
            ]
        );
    }
    #[test]
    fn measured_zero_is_available_and_invalid_loads_are_missing() {
        assert_eq!(percent(Some(0.)), Some(0.));
        assert_eq!(percent(Some(100.)), Some(100.));
        for value in [
            None,
            Some(-1.),
            Some(101.),
            Some(f64::NAN),
            Some(f64::INFINITY),
        ] {
            assert_eq!(percent(value), None);
        }
    }
    #[test]
    fn transitions_finish_at_measurement_without_overshooting() {
        for (from, to) in [(0., 100.), (90., 0.), (45., 45.)] {
            assert_eq!(tween(from, to, 0.), from);
            assert_eq!(tween(from, to, 0.28), to);
            assert_eq!(tween(from, to, 10.), to);
            for i in 0..28 {
                assert!((from.min(to)..=from.max(to)).contains(&tween(from, to, i as f64 / 100.)));
            }
        }
    }
    #[test]
    fn missing_and_invalid_memory_are_not_reported_as_zero() {
        assert_eq!(
            memory("MemTotal: 1024 kB\nMemAvailable: 512 kB"),
            Some((524_288., 1_048_576.))
        );
        assert_eq!(memory("MemTotal: 0 kB\nMemAvailable: 0 kB"), None);
        assert_eq!(memory("MemTotal: 1 kB\nMemAvailable: 2 kB"), None);
        assert_eq!(memory("MemTotal: 1024 kB"), None);
        assert_eq!(ratio(2., 1.), None);
        assert_eq!(ratio(f64::NAN, 1.), None);
    }
}

/// Explicit developer capture, never used as a telemetry fallback.
pub(crate) fn preview_states() -> gtk::Box {
    let root = column(18);
    root.add_css_class("gs-home");
    root.add_css_class("gs-home-scroll");
    root.set_margin_start(24);
    root.set_margin_end(24);
    root.set_margin_top(24);
    root.set_margin_bottom(24);
    root.append(&text(
        "DONNÉES DE TEST · États des widgets",
        "gs-home-title",
    ));
    root.append(&text("Échantillons synthétiques pour valider le rendu uniquement. Aucune mesure matérielle ni donnée sauvegardée n’est remplacée.", "gs-home-subtitle"));
    for (title, load, temp, ram) in [
        (
            "Mesures disponibles",
            Some(42.),
            Some(52.),
            Some((8. * 1073741824., 16. * 1073741824.)),
        ),
        (
            "Zéro réellement mesuré",
            Some(0.),
            Some(0.),
            Some((0., 16. * 1073741824.)),
        ),
        ("Mesures indisponibles", None, None, None),
    ] {
        let panel = card(title, "preferences-system-symbolic");
        panel.add_css_class("gs-home-hardware");
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 24);
        let gauge = Gauge::new(title);
        gauge.set(load);
        row.append(&gauge.widget);
        let temperature = Value::new(
            "Température · sans seuil d’alerte",
            "temperature-symbolic",
            false,
        );
        temperature.widget.add_css_class("gs-home-temperature");
        temperature.set(temp, "°C");
        row.append(&temperature.widget);
        let memory = Value::new("RAM · occupation mesurée", "media-flash-symbolic", true);
        memory.memory(ram);
        row.append(&memory.widget);
        panel.append(&row);
        root.append(&panel);
    }
    root
}
