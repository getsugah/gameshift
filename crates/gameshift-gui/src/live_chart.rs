//! Fixed-scale occupancy chart. Home opts into fill and finite scrolling; no extra telemetry reads.
use adw::prelude::*;
use gameshift_telemetry::history::{PercentSeries, WINDOW};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Instant,
};
#[derive(Clone)]
pub struct PercentChart {
    pub widget: gtk::DrawingArea,
    series: Rc<RefCell<PercentSeries>>,
    gap_label: Rc<RefCell<String>>,
    started: Instant,
    motion: Rc<Cell<f64>>,
    filled: bool,
    rate: bool,
    generation: Rc<Cell<u64>>,
}
impl PercentChart {
    pub fn new(title: &str) -> Self {
        Self::build(title, false, false)
    }
    pub fn home(title: &str) -> Self {
        Self::build(title, true, false)
    }
    pub fn rate(title: &str) -> Self {
        Self::build(title, true, true)
    }
    fn build(title: &str, filled: bool, rate: bool) -> Self {
        let series = Rc::new(RefCell::new(PercentSeries::default()));
        let data = series.clone();
        let gap_label = Rc::new(RefCell::new("Mesure indisponible".to_owned()));
        let missing_label = gap_label.clone();
        let motion = Rc::new(Cell::new(0.));
        let position = motion.clone();
        let widget = gtk::DrawingArea::new();
        widget.add_css_class("gs-live-chart");
        widget.set_content_height(120);
        widget.set_hexpand(true);
        widget.set_tooltip_text(Some(&format!("{title} · 60 dernières secondes, échelle fixe de 0 à 100 %. Les interruptions indiquent une mesure indisponible ou une collecte suspendue.")));
        if rate {
            widget.set_tooltip_text(Some("Clients DRM observables · ms/s cumulées · échelle automatique non plafonnée · historique 60 s. Ne représente pas un pourcentage matériel."));
        }
        widget.update_property(&[gtk::accessible::Property::Label(title)]);
        widget.set_draw_func(move |area, cr, width, height| {
            let color = area.color();
            let font = area
                .pango_context()
                .font_description()
                .map(|font| font.size() as f64 / gtk::pango::SCALE as f64)
                .unwrap_or(10.)
                .clamp(10., 16.);
            let unit = font / 10.;
            let scale = if rate {
                data.borrow()
                    .points()
                    .filter_map(|p| p.percent)
                    .fold(1000_f64, f64::max)
                    * 1.1
            } else {
                100.
            };
            let left = if filled { 8. * unit } else { 42. * unit };
            let top = 12. * unit;
            let w = (width as f64
                - left
                - if filled {
                    if rate {
                        ((format!("{scale:.0}").len() as f64 * font * 0.65) + 10.).max(34. * unit)
                    } else {
                        34. * unit
                    }
                } else {
                    12. * unit
                })
            .max(1.);
            let h = (height as f64 - top - 24. * unit).max(1.);
            cr.set_source_rgba(
                color.red() as f64,
                color.green() as f64,
                color.blue() as f64,
                0.25,
            );
            cr.set_line_width(1.);
            for percent in if filled {
                &[0., 25., 50., 75., 100.][..]
            } else {
                &[0., 50., 100.][..]
            } {
                let y = top + h * (1. - percent / 100.);
                cr.move_to(left, y);
                cr.line_to(left + w, y);
            }
            if filled {
                for step in 0..=6 {
                    let x = left + w * step as f64 / 6.;
                    cr.move_to(x, top);
                    cr.line_to(x, top + h);
                }
            }
            let _ = cr.stroke();
            cr.set_source_rgba(
                color.red() as f64,
                color.green() as f64,
                color.blue() as f64,
                0.8,
            );
            cr.set_font_size(font);
            if filled {
                for percent in [0., 25., 50., 75., 100.] {
                    cr.move_to(left + w + 4., top + h * (1. - percent / 100.) + 3.);
                    let _ = cr.show_text(&if rate {
                        format!("{:.0}", percent * scale / 100.)
                    } else {
                        format!("{percent:.0} %")
                    });
                }
            } else {
                cr.move_to(0., top + 4.);
                let _ = cr.show_text("100 %");
                cr.move_to(8., top + h);
                let _ = cr.show_text("0 %");
            }
            cr.move_to(left, height as f64 - 3.);
            let _ = cr.show_text("−60 s");
            let short = width < 240;
            cr.move_to(
                (left + w - if short { 20. } else { 80. }).max(left),
                height as f64 - 3.,
            );
            let _ = cr.show_text(if short { "0 s" } else { "Dernier relevé" });
            let series = data.borrow();
            let measured_end = series
                .points()
                .last()
                .map(|p| p.time.as_secs_f64())
                .unwrap_or(0.);
            let end = if filled { position.get() } else { measured_end };
            if series.points().last().is_none_or(|p| p.percent.is_none()) {
                cr.move_to(left + 8., top + 18.);
                let _ = cr.show_text(&missing_label.borrow());
            }
            let _ = cr.save();
            cr.rectangle(left, top - 2., w, h + 4.);
            cr.clip();
            if filled {
                // Fill each contiguous measured segment separately: missing values stay gaps.
                let mut segment = Vec::new();
                let fill = |points: &mut Vec<(f64, f64)>| {
                    if points.len() > 1 {
                        cr.move_to(points[0].0, top + h);
                        for &(x, y) in points.iter() {
                            cr.line_to(x, y);
                        }
                        cr.line_to(points.last().unwrap().0, top + h);
                        cr.close_path();
                        let gradient = gtk::cairo::LinearGradient::new(0., top, 0., top + h);
                        gradient.add_color_stop_rgba(
                            0.,
                            color.red() as f64,
                            color.green() as f64,
                            color.blue() as f64,
                            0.32,
                        );
                        gradient.add_color_stop_rgba(
                            1.,
                            color.red() as f64,
                            color.green() as f64,
                            color.blue() as f64,
                            0.04,
                        );
                        let _ = cr.set_source(&gradient);
                        let _ = cr.fill();
                    }
                    points.clear();
                };
                for point in series.points() {
                    if let Some(value) = point.percent {
                        segment.push((
                            left + w
                                * (1. - (end - point.time.as_secs_f64()) / WINDOW.as_secs_f64()),
                            top + h * (1. - value / scale),
                        ));
                    } else {
                        fill(&mut segment);
                    }
                }
                fill(&mut segment);
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
                let y = top + h * (1. - value / scale);
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
                    let y = top + h * (1. - value / scale);
                    cr.arc(x, y, 1.5, 0., std::f64::consts::TAU);
                    let _ = cr.fill();
                }
            }
            let _ = cr.restore();
        });
        Self {
            widget,
            series,
            gap_label,
            started: Instant::now(),
            motion,
            filled,
            rate,
            generation: Rc::new(Cell::new(0)),
        }
    }
    pub fn set_gap_label(&self, label: &str) {
        if self.gap_label.borrow().as_str() != label {
            *self.gap_label.borrow_mut() = label.to_owned();
            self.widget.queue_draw();
        }
    }
    pub fn clear(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
        *self.series.borrow_mut() = PercentSeries::default();
        self.widget.queue_draw();
    }
    pub fn push(&self, value: Option<f64>) {
        let generation = self.generation.get().wrapping_add(1);
        self.generation.set(generation);
        let elapsed = self.started.elapsed();
        if self.rate {
            self.series.borrow_mut().push_rate(elapsed, value);
        } else {
            self.series.borrow_mut().push(elapsed, value);
        }
        let end = elapsed.as_secs_f64();
        let from = self.motion.get();
        let animate = self.filled
            && value.is_some()
            && self.widget.is_mapped()
            && gtk::Settings::default().is_some_and(|s| s.is_gtk_enable_animations());
        if animate {
            let current_generation = self.generation.clone();
            let motion = self.motion.clone();
            let since = Instant::now();
            self.widget.add_tick_callback(move |area, _| {
                if current_generation.get() != generation {
                    return gtk::glib::ControlFlow::Break;
                }
                let t = (since.elapsed().as_secs_f64() / 0.28).min(1.);
                let finished = t >= 1.
                    || !area.is_mapped()
                    || !gtk::Settings::default().is_some_and(|s| s.is_gtk_enable_animations());
                motion.set(if finished {
                    end
                } else {
                    from + (end - from) * (1. - (1. - t).powi(3))
                });
                area.queue_draw();
                if finished {
                    gtk::glib::ControlFlow::Break
                } else {
                    gtk::glib::ControlFlow::Continue
                }
            });
        } else {
            self.motion.set(end);
        }
        self.widget.queue_draw();
    }
}
