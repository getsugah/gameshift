//! Short explanations available by keyboard, mouse and touch, without a new dependency.
use adw::prelude::*;
pub fn button(title: &str, explanation: &str) -> gtk::MenuButton {
    let button = gtk::MenuButton::new();
    button.set_icon_name("help-about-symbolic");
    button.add_css_class("flat");
    button.set_valign(gtk::Align::Center);
    let name = format!("Aide : {title}");
    button.set_tooltip_text(Some(explanation));
    button.update_property(&[gtk::accessible::Property::Label(&name)]);
    button.set_popover(Some(&popup(explanation)));
    button
}
pub fn popup(explanation: &str) -> gtk::Popover {
    let text = gtk::Label::new(Some(explanation));
    text.set_wrap(true);
    text.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    text.set_width_chars(36);
    text.set_max_width_chars(48);
    text.set_xalign(0.);
    text.set_selectable(true);
    text.set_margin_start(12);
    text.set_margin_end(12);
    text.set_margin_top(12);
    text.set_margin_bottom(12);
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_width(300)
        .max_content_width(440)
        .propagate_natural_width(true)
        .max_content_height(340)
        .propagate_natural_height(true)
        .child(&text)
        .build();
    let popover = gtk::Popover::new();
    popover.set_child(Some(&scroll));
    popover
}
pub fn heading(title: &str, explanation: &str) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.);
    label.set_wrap(true);
    label.set_hexpand(true);
    label.add_css_class("gs-card-label");
    row.append(&label);
    row.append(&button(title, explanation));
    row
}
pub fn describe(widget: &impl IsA<gtk::Widget>, explanation: &str) {
    widget.set_tooltip_text(Some(explanation));
    widget
        .as_ref()
        .update_property(&[gtk::accessible::Property::Description(explanation)]);
}
pub fn guide(items: &[(&str, &str)]) -> gtk::Expander {
    let expander = gtk::Expander::new(Some("Comprendre les résultats"));
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    for (title, explanation) in items {
        content.append(&heading(title, explanation));
    }
    expander.set_child(Some(&content));
    expander
}
pub const VARIATION:&str="La variation entre les mesures rapporte leur dispersion à leur moyenne (coefficient de variation). Ce n’est pas l’écart maximal entre deux mesures. Une valeur élevée appelle de nouvelles mesures dans des conditions stables.";
pub const FRAME_TIME:&str="Temps observé entre deux images calculées, en millisecondes (ms). Il inclut les attentes CPU/GPU et la présentation ; il ne mesure pas l’instant d’affichage physique à l’écran. Plus il est faible et régulier, plus le rendu est fluide.";
pub const MSAA:&str="L’anticrénelage (MSAA) réduit les contours en escalier en utilisant plusieurs échantillons par pixel. Un niveau plus élevé sollicite davantage le GPU et sa mémoire. Le niveau réellement pris en charge est enregistré ; comparez le même niveau effectif.";
