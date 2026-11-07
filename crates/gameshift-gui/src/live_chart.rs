//! Shared fixed-scale occupancy chart. No animation timer or extra telemetry reads.
use adw::prelude::*;
use gameshift_telemetry::history::{PercentSeries, WINDOW};
use std::{cell::RefCell, rc::Rc, time::Instant};
#[derive(Clone)]
pub struct PercentChart {
    pub widget: gtk::DrawingArea,
    series: Rc<RefCell<PercentSeries>>,
    started: Instant,
}
impl PercentChart {
    pub fn new(title: &str) -> Self {
        let series = Rc::new(RefCell::new(PercentSeries::default()));
        let data = series.clone();
        let widget = gtk::DrawingArea::new();
        widget.set_content_height(120);
        widget.set_hexpand(true);
        widget.set_tooltip_text(Some(&format!("{title} · 60 dernières secondes, échelle fixe de 0 à 100 %. Les interruptions indiquent une mesure indisponible ou une collecte suspendue.")));
        widget.update_property(&[gtk::accessible::Property::Label(title)]);
        widget.set_draw_func(move |area, cr, width, height| {
            let color = area.color();
            let left = 42.;
            let top = 12.;
            let w = (width as f64 - left - 12.).max(1.);
            let h = (height as f64 - top - 24.).max(1.);
            cr.set_source_rgba(
                color.red() as f64,
                color.green() as f64,
                color.blue() as f64,
                0.25,
            );
            cr.set_line_width(1.);
            for percent in [0., 50., 100.] {
                let y = top + h * (1. - percent / 100.);
                cr.move_to(left, y);
                cr.line_to(left + w, y);
            }
            let _ = cr.stroke();
            cr.set_source_rgba(
                color.red() as f64,
                color.green() as f64,
                color.blue() as f64,
                0.8,
            );
            cr.set_font_size(10.);
            cr.move_to(0., top + 4.);
            let _ = cr.show_text("100 %");
            cr.move_to(8., top + h);
            let _ = cr.show_text("0 %");
            cr.move_to(left, height as f64 - 3.);
            let _ = cr.show_text("−60 s");
            cr.move_to((left + w - 80.).max(left), height as f64 - 3.);
            let _ = cr.show_text("Dernier relevé");
            let series = data.borrow();
            let end = series
                .points()
                .last()
                .map(|p| p.time.as_secs_f64())
                .unwrap_or(0.);
            if series.points().last().is_none_or(|p| p.percent.is_none()) {
                cr.move_to(left + 8., top + 18.);
                let _ = cr.show_text("Mesure indisponible");
            }
            cr.set_line_width(2.);
            cr.set_source_rgba(
                color.red() as f64,
                color.green() as f64,
                color.blue() as f64,
                1.,
            );
            let mut connected = false;
            for point in series.points() {
                let Some(value) = point.percent else {
                    connected = false;
                    continue;
                };
                let x = left + w * (1. - (end - point.time.as_secs_f64()) / WINDOW.as_secs_f64());
                let y = top + h * (1. - value / 100.);
                if connected {
                    cr.line_to(x, y);
                } else {
                    cr.move_to(x, y);
                }
                connected = true;
            }
            let _ = cr.stroke();
            // Show isolated samples as dots too; one reading must not imply a line.
            for point in series.points() {
                if let Some(value) = point.percent {
                    let x =
                        left + w * (1. - (end - point.time.as_secs_f64()) / WINDOW.as_secs_f64());
                    let y = top + h * (1. - value / 100.);
                    cr.arc(x, y, 1.5, 0., std::f64::consts::TAU);
                    let _ = cr.fill();
                }
            }
        });
        Self {
            widget,
            series,
            started: Instant::now(),
        }
    }
    pub fn clear(&self) {
        *self.series.borrow_mut() = PercentSeries::default();
        self.widget.queue_draw();
    }
    pub fn push(&self, value: Option<f64>) {
        self.series.borrow_mut().push(self.started.elapsed(), value);
        self.widget.queue_draw();
    }
}
