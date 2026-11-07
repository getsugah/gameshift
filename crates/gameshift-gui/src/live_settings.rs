//! Persistent appearance and visible-page cadence, independent of benchmark protocols.
use adw::prelude::*;
use gameshift_core::preferences::{self, Preferences, Theme};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
    time::Duration,
};
struct State {
    value: Preferences,
    error: Option<String>,
}
thread_local! {static SETTINGS: RefCell<State> = RefCell::new(match preferences::path().and_then(|p| Preferences::load(&p)) {
    Ok(value) => State {value, error:None}, Err(error) => State {value:Preferences::default(), error:Some(error)}
});}
pub fn initialize() {
    set_theme(SETTINGS.with_borrow(|s| s.value.theme));
}
fn set_theme(theme: Theme) {
    adw::StyleManager::default().set_color_scheme(match theme {
        Theme::System => adw::ColorScheme::Default,
        Theme::Light => adw::ColorScheme::ForceLight,
        Theme::Dark => adw::ColorScheme::ForceDark,
    });
}
pub fn interval() -> Duration {
    Duration::from_millis(SETTINGS.with_borrow(|s| s.value.interval_millis))
}
fn save(row: &adw::ActionRow) {
    let result = SETTINGS.with_borrow(|s| preferences::path().and_then(|p| s.value.save(&p)));
    match result {
        Ok(()) => row.set_subtitle("Préférence enregistrée"),
        Err(error) => row.set_subtitle(&format!("Réglage de session uniquement : {error}")),
    }
}
fn explain_error(row: &adw::ActionRow) {
    SETTINGS.with_borrow(|s| {
        if let Some(error) = &s.error {
            row.set_subtitle(&format!(
                "Configuration conservée, valeurs par défaut : {error}"
            ));
        }
    });
}
pub fn appearance() -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Apparence");
    let row = adw::ActionRow::new();
    row.set_title("Thème");
    explain_error(&row);
    let choice = gtk::DropDown::from_strings(&["Automatique", "Clair", "Sombre"]);
    choice.set_selected(SETTINGS.with_borrow(|s| match s.value.theme {
        Theme::System => 0,
        Theme::Light => 1,
        Theme::Dark => 2,
    }));
    choice.set_valign(gtk::Align::Center);
    choice.set_tooltip_text(Some("Apparence de GameShift"));
    let weak = row.downgrade();
    choice.connect_selected_notify(move |choice| {
        let theme = match choice.selected() {
            0 => Theme::System,
            1 => Theme::Light,
            _ => Theme::Dark,
        };
        SETTINGS.with_borrow_mut(|s| s.value.theme = theme);
        set_theme(theme);
        if let Some(row) = weak.upgrade() {
            save(&row);
        }
    });
    row.set_activatable_widget(Some(&choice));
    row.add_suffix(&choice);
    group.add(&row);
    group
}
pub fn preferences() -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Surveillance matérielle");
    group.set_description(Some("Les collectes Accueil/CPU/GPU sont suspendues sur les autres pages. Aurelia conserve sa cadence propre."));
    let row = adw::ActionRow::new();
    row.set_title("Intervalle d’actualisation");
    explain_error(&row);
    let choice = gtk::DropDown::from_strings(&["500 ms", "1 s", "2 s", "5 s"]);
    let intervals = [500, 1000, 2000, 5000];
    choice.set_selected(
        intervals
            .iter()
            .position(|v| *v == interval().as_millis() as u64)
            .unwrap_or(1) as u32,
    );
    choice.set_valign(gtk::Align::Center);
    choice.set_tooltip_text(Some("Cadence des relevés CPU et GPU"));
    let weak = row.downgrade();
    choice.connect_selected_notify(move |choice| {
        if let Some(value) = intervals.get(choice.selected() as usize) {
            SETTINGS.with_borrow_mut(|s| s.value.interval_millis = *value);
            if let Some(row) = weak.upgrade() {
                save(&row);
            }
        }
    });
    row.set_activatable_widget(Some(&choice));
    row.add_suffix(&choice);
    group.add(&row);
    group
}

thread_local! {static BENCHMARKS: RefCell<Vec<Weak<Cell<bool>>>> = const { RefCell::new(Vec::new()) };}
pub fn register_benchmark(active: &Rc<Cell<bool>>) {
    BENCHMARKS.with_borrow_mut(|items| {
        items.retain(|item| item.strong_count() > 0);
        items.push(Rc::downgrade(active));
    });
}
pub fn benchmark_running() -> bool {
    BENCHMARKS.with_borrow_mut(|items| {
        items.retain(|item| item.strong_count() > 0);
        items
            .iter()
            .filter_map(Weak::upgrade)
            .any(|item| item.get())
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn benchmark_guard_covers_multiple_windows_without_retaining_closed_pages() {
        let first = Rc::new(Cell::new(false));
        let second = Rc::new(Cell::new(false));
        register_benchmark(&first);
        register_benchmark(&second);
        assert!(!benchmark_running());
        first.set(true);
        assert!(benchmark_running());
        first.set(false);
        second.set(true);
        assert!(benchmark_running());
        drop(second);
        assert!(!benchmark_running());
    }
}
