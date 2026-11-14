//! Directional, tapered specular reflections over native GTK card contents.
use gtk::{glib, prelude::*};

mod imp {
    use super::*;
    use gtk::subclass::prelude::*;

    #[derive(Default)]
    pub struct GlossCard;
    #[glib::object_subclass]
    impl ObjectSubclass for GlossCard {
        const NAME: &'static str = "GameShiftGlossCard";
        type Type = super::GlossCard;
        type ParentType = gtk::Box;
    }
    impl ObjectImpl for GlossCard {}
    impl BoxImpl for GlossCard {}
    impl WidgetImpl for GlossCard {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            self.parent_snapshot(snapshot);
            let card = self.obj();
            let (width, height) = (card.width() as f64, card.height() as f64);
            if width < 30. || height < 20. {
                return;
            }
            // GTK invokes snapshot at the content origin. Query the resolved
            // insets so the reflection follows the actual CSS border, not text.
            // GTK4 currently exposes these insets through StyleContext.
            #[allow(deprecated)]
            let (padding, border) = {
                let style = card.style_context();
                (style.padding(), style.border())
            };
            let left = f64::from(padding.left()) + f64::from(border.left());
            let right = f64::from(padding.right()) + f64::from(border.right());
            let top = f64::from(padding.top()) + f64::from(border.top());
            let bottom = f64::from(padding.bottom()) + f64::from(border.bottom());
            let (width, height) = (width + left + right, height + top + bottom);
            snapshot.save();
            snapshot.translate(&gtk::graphene::Point::new(-left as f32, -top as f32));
            let bounds = gtk::graphene::Rect::new(0., 0., width as f32, height as f32);
            let c = snapshot.append_cairo(&bounds);
            let radius = if card.has_css_class("gs-aurelia-hero") {
                18.
            } else if card.has_css_class("gs-aurelia-panel") {
                15.
            } else {
                14.
            };
            draw_reflections(&c, width, height, radius);
            snapshot.restore();
        }
    }
}

glib::wrapper! {
    pub struct GlossCard(ObjectSubclass<imp::GlossCard>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

pub fn column(spacing: i32) -> gtk::Box {
    glib::Object::builder::<GlossCard>()
        .property("orientation", gtk::Orientation::Vertical)
        .property("spacing", spacing)
        .build()
        .upcast()
}

fn draw_reflections(c: &gtk::cairo::Context, w: f64, h: f64, r: f64) {
    // Filled ribbons, not a constant-width white stroke. Their inner edge
    // tapers from a broad upper-left catchlight into the unlit material.
    let top = gtk::cairo::LinearGradient::new(r, 0., w * 0.88, 0.);
    top.add_color_stop_rgba(0., 1., 1., 1., 0.52);
    top.add_color_stop_rgba(0.17, 1., 1., 1., 0.42);
    top.add_color_stop_rgba(0.50, 1., 1., 1., 0.13);
    top.add_color_stop_rgba(1., 1., 1., 1., 0.);
    let _ = c.set_source(&top);
    c.move_to(r, 0.8);
    c.line_to(w * 0.88, 0.8);
    c.curve_to(w * 0.78, 1.0, w * 0.58, 1.5, w * 0.42, 2.2);
    c.curve_to(w * 0.24, 3.3, r + 12., 3.6, r, 3.1);
    c.close_path();
    let _ = c.fill();

    let fade = (h * 0.55).min(76.);
    let corner = gtk::cairo::LinearGradient::new(0., 0., 0., fade);
    corner.add_color_stop_rgba(0., 1., 1., 1., 0.52);
    corner.add_color_stop_rgba(0.18, 1., 1., 1., 0.59);
    corner.add_color_stop_rgba(0.60, 1., 1., 1., 0.12);
    corner.add_color_stop_rgba(1., 1., 1., 1., 0.);
    let _ = c.set_source(&corner);
    c.move_to(r, 0.8);
    c.curve_to(r * 0.43, 0.8, 0.8, r * 0.43, 0.8, r);
    c.line_to(0.8, fade);
    c.curve_to(1.3, fade * 0.75, 2.7, r + 12., 3.0, r);
    c.curve_to(3.0, r * 0.50, r * 0.52, 3.0, r, 3.1);
    c.close_path();
    let _ = c.fill();

    // A weaker opposite catchlight; the rest of the right edge stays dark.
    let right = gtk::cairo::LinearGradient::new(w, 0., w, (h * 0.4).min(48.));
    right.add_color_stop_rgba(0., 1., 1., 1., 0.18);
    right.add_color_stop_rgba(0.35, 1., 1., 1., 0.08);
    right.add_color_stop_rgba(1., 1., 1., 1., 0.);
    let _ = c.set_source(&right);
    c.move_to(w - r - 18., 1.0);
    c.line_to(w - r, 1.0);
    c.curve_to(w - r * 0.4, 1.0, w - 1.0, r * 0.4, w - 1.0, r);
    c.line_to(w - 1.0, (h * 0.4).min(48.));
    c.curve_to(w - 1.5, r + 12., w - 2.6, r + 2., w - 3.0, r);
    c.curve_to(w - 3.0, r * 0.55, w - r * 0.5, 2.3, w - r, 2.1);
    c.close_path();
    let _ = c.fill();

    // Small reflected light near the lower-right corner, never a white frame.
    let lower = gtk::cairo::LinearGradient::new(w * 0.58, h, w, h);
    lower.add_color_stop_rgba(0., 1., 1., 1., 0.);
    lower.add_color_stop_rgba(0.73, 1., 1., 1., 0.15);
    lower.add_color_stop_rgba(1., 1., 1., 1., 0.02);
    let _ = c.set_source(&lower);
    c.move_to(w * 0.58, h - 1.0);
    c.line_to(w - r, h - 1.0);
    c.curve_to(w - r * 0.40, h - 1.0, w - 1.0, h - r * 0.4, w - 1.0, h - r);
    c.curve_to(w - 2.0, h - r * 0.5, w - r * 0.5, h - 2.4, w - r, h - 2.4);
    c.curve_to(w * 0.77, h - 2.1, w * 0.67, h - 1.1, w * 0.58, h - 1.0);
    c.close_path();
    let _ = c.fill();
}

#[cfg(test)]
mod tests {
    #[test]
    fn reflection_is_localized_instead_of_an_even_white_frame() {
        let mut surface =
            gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, 240, 120).unwrap();
        {
            let c = gtk::cairo::Context::new(&surface).unwrap();
            super::draw_reflections(&c, 240., 120., 15.);
        }
        let stride = surface.stride() as usize;
        let data = surface.data().unwrap();
        let alpha = |x: usize, y: usize| {
            u32::from_ne_bytes(
                data[y * stride + x * 4..y * stride + x * 4 + 4]
                    .try_into()
                    .unwrap(),
            ) >> 24
        };
        let top_left: u32 = (0..20)
            .flat_map(|y| (0..90).map(move |x| (x, y)))
            .map(|(x, y)| alpha(x, y))
            .sum();
        let bottom_left: u32 = (100..120)
            .flat_map(|y| (0..90).map(move |x| (x, y)))
            .map(|(x, y)| alpha(x, y))
            .sum();
        assert!(top_left > 1000);
        assert_eq!(
            bottom_left, 0,
            "The unlit edge must not acquire a white outline"
        );
        assert!(
            alpha(50, 1) > alpha(190, 1),
            "The highlight must fade along its length"
        );
    }
}
