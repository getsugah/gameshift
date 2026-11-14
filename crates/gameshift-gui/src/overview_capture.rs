//! Opt-in real GTK captures. No clicks, benchmarks or hardware writes.
use adw::prelude::*;
use gtk::glib;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};
#[cfg(test)]
fn visual_fixture_requested()->bool {std::env::var("GAMESHIFT_AURELIA_VISUAL_TEST").as_deref()==Ok("1")}
#[cfg(not(test))]
fn visual_fixture_requested()->bool {false}
pub fn requested() -> bool {
    visual_fixture_requested() || std::env::args().any(|arg| {
        matches!(
            arg.as_str(),
            "--capture-home" | "--capture-home-states" | "--capture-tabs" | "--capture-aurelia"
        )
    })
}
fn dashboard(widget: &impl IsA<gtk::Widget>) -> Option<gtk::Stack> {
    if let Some(stack) = widget.as_ref().downcast_ref::<gtk::Stack>() {
        if stack.widget_name()=="gameshift-workspace" {
            return Some(stack.clone());
        }
    }
    let mut child = widget.as_ref().first_child();
    while let Some(widget) = child {
        if let Some(stack) = dashboard(&widget) {
            return Some(stack);
        }
        child = widget.next_sibling();
    }
    None
}
fn audit_aurelia(widget:&gtk::Widget,window:&gtk::Window,counts:&mut (usize,usize),errors:&mut Vec<String>) {
    if widget.is_mapped() {
        if widget.is::<gtk::StackSwitcher>() {counts.1+=1;}
        if widget.has_css_class("gs-aurelia-score-card") {counts.0+=1;}
        if widget.has_css_class("gs-aurelia-score-card") || widget.has_css_class("gs-aurelia-panel") {
            if let Some(bounds)=widget.compute_bounds(window) {
                if bounds.x()< -1. || bounds.x()+bounds.width()>window.width() as f32+1. {
                    errors.push(format!("Aurelia : débordement horizontal x={} largeur={} fenêtre={}",bounds.x(),bounds.width(),window.width()));
                }
            }
        }
    }
    let mut child=widget.first_child();
    while let Some(widget)=child {audit_aurelia(&widget,window,counts,errors);child=widget.next_sibling();}
}
// Opt-in measurement, restricted to the first 400 ms after a page switch.
// PNG rendering/saving occurs later, so it cannot be mistaken for application lag.
fn probe_switch(window: &gtk::Window, page: &'static str) {
    let started = Instant::now();
    let mut previous = Instant::now();
    let mut gaps = Vec::<f64>::new();
    glib::timeout_add_local(Duration::from_millis(16), move || {
        let now = Instant::now();
        gaps.push(now.duration_since(previous).as_secs_f64() * 1000.);
        previous = now;
        if started.elapsed() >= Duration::from_millis(400) {
            gaps.sort_by(f64::total_cmp);
            eprintln!(
                "GTK loop {page}: max={:.1}ms p95={:.1}ms",
                gaps.last().unwrap_or(&0.),
                gaps[(gaps.len() * 95 / 100).min(gaps.len() - 1)]
            );
            return glib::ControlFlow::Break;
        }
        glib::ControlFlow::Continue
    });
    let last_frame = Cell::new(None::<i64>);
    let frames = RefCell::new(Vec::<f64>::new());
    window.add_tick_callback(move |_, clock| {
        let now = clock.frame_time();
        let mut frames = frames.borrow_mut();
        if let Some(last) = last_frame.replace(Some(now)) {
            frames.push((now - last) as f64 / 1000.);
        }
        if started.elapsed() >= Duration::from_millis(400) {
            frames.sort_by(f64::total_cmp);
            eprintln!(
                "GTK frames {page}: count={} max={:.1}ms",
                frames.len(),
                frames.last().unwrap_or(&0.)
            );
            return glib::ControlFlow::Break;
        }
        glib::ControlFlow::Continue
    });
}
pub fn run(directory: std::path::PathBuf) -> glib::ExitCode {
    if let Err(error) = std::fs::create_dir_all(&directory) {
        eprintln!("Capture : {error}");
        return glib::ExitCode::FAILURE;
    }
    let app = adw::Application::builder()
        .application_id("io.github.gameshift.HomePreview")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let errors = Rc::new(RefCell::new(Vec::<String>::new()));
    let result = errors.clone();
    app.connect_activate(move |app| {
        let fixture = std::env::args().any(|arg| arg == "--capture-home-states");
        if fixture {
            let provider = gtk::CssProvider::new();
            provider.load_from_data(include_str!("style.css"));
            if let Some(display) = gtk::gdk::Display::default() {
                gtk::style_context_add_provider_for_display(
                    &display,
                    &provider,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                );
            }
            let window = adw::ApplicationWindow::builder()
                .application(app)
                .title("GameShift — États visuels de test")
                .content(&crate::overview::preview_states())
                .build();
            window.present();
        } else {
            crate::build_ui(app);
        }
        let Some(window) = app.active_window() else {
            return;
        };
        let aurelia = visual_fixture_requested() || std::env::args().any(|arg| arg == "--capture-aurelia");
        let tabs = aurelia || std::env::args().any(|arg| arg == "--capture-tabs");
        let size = if std::env::args().any(|arg| arg == "--capture-1440p") {
            (2560, 1440)
        } else {
            (1182, 752)
        };
        let cases = if aurelia {
            vec![("laboratory",1920,1080),("laboratory",2560,1440),("laboratory",1100,800)]
        } else if tabs {
            [
                "Carte graphique",
                "Processeur",
                "laboratory",
                "profiles",
                "settings",
                "Système",
                "Mémoire",
                "Stockage",
                "overview",
            ]
            .into_iter()
            .map(|name| (name, size.0, size.1))
            .collect::<Vec<_>>()
        } else {
            vec![
                ("overview", 1182, 752),
                ("overview", 2560, 1440),
                ("overview", 760, 740),
            ]
        };
        let cases = if std::env::args().any(|arg| arg == "--capture-resize") {
            [(1182, 752), (2560, 1440), (1182, 752)]
                .into_iter()
                .flat_map(|(w, h)| {
                    ["Processeur", "overview"]
                        .into_iter()
                        .map(move |page| (page, w, h))
                })
                .collect::<Vec<_>>()
        } else {
            cases
        };
        window.set_size_request(cases[0].1, cases[0].2);
        window.set_default_size(cases[0].1, cases[0].2);
        let measured = std::env::args().any(|arg| arg == "--measure-tabs");
        #[cfg(test)]
        let fixture_injected=Cell::new(false);
        let mut selected_step = None;
        let step = Rc::new(Cell::new(0));
        let since = Rc::new(Cell::new(Instant::now()));
        let app = app.downgrade();
        let errors = result.clone();
        let directory = directory.clone();
        glib::timeout_add_local(Duration::from_millis(200), move || {
            let index = step.get();
            if tabs {
                let Some(stack) = dashboard(&window) else {
                    return glib::ControlFlow::Continue;
                };
                if stack.child_by_name(cases[index].0).is_none(){return glib::ControlFlow::Continue;}
                if selected_step != Some(index) {
                    if measured {
                        probe_switch(&window, cases[index].0);
                    }
                    stack.set_visible_child_name(cases[index].0);
                    selected_step = Some(index);
                }
            }
            #[cfg(test)]
            if visual_fixture_requested() && !fixture_injected.get(){
                if crate::aurelia_visual_tests::inject(window.upcast_ref()){fixture_injected.set(true);since.set(Instant::now());}
                else{return glib::ControlFlow::Continue;}
            }
            if since.get().elapsed() < Duration::from_secs(if index == 0 { 7 } else { 2 }) {
                return glib::ControlFlow::Continue;
            }
            let paintable = gtk::WidgetPaintable::new(Some(&window));
            if aurelia {
                let mut counts=(0,0);
                audit_aurelia(window.upcast_ref(),window.upcast_ref(),&mut counts,&mut errors.borrow_mut());
                if counts!=(5,1) {errors.borrow_mut().push(format!("Aurelia : {} cartes, {} navigations horizontales visibles",counts.0,counts.1));}
                eprintln!("Audit Aurelia {}×{} : {} cartes, {} barres d’onglets, {} erreurs",window.width(),window.height(),counts.0,counts.1,errors.borrow().len());
            }
            let snapshot = gtk::Snapshot::new();
            paintable.snapshot(&snapshot, window.width() as f64, window.height() as f64);
            match (snapshot.to_node(), window.renderer()) {
                (Some(node), Some(renderer)) => {
                    let texture = renderer.render_texture(&node, None);
                    let path = directory.join(format!(
                        "{}-{}x{}-{}.png",
                        if fixture {
                            "etats-test"
                        } else if tabs {
                            cases[index].0
                        } else {
                            "accueil"
                        },
                        window.width(),
                        window.height(),
                        index + 1
                    ));
                    if let Err(error) = texture.save_to_png(&path) {
                        errors.borrow_mut().push(error.to_string());
                    } else {
                        eprintln!(
                            "Capture réelle : {} ({} × {})",
                            path.display(),
                            window.width(),
                            window.height()
                        );
                    }
                }
                _ => errors.borrow_mut().push("Rendu GTK absent".into()),
            }
            let next = index + 1;
            if next >= cases.len() {
                window.close();
                if let Some(app) = app.upgrade() {
                    app.quit();
                }
                return glib::ControlFlow::Break;
            }
            window.set_size_request(cases[next].1, cases[next].2);
            window.set_default_size(cases[next].1, cases[next].2);
            step.set(next);
            since.set(Instant::now());
            glib::ControlFlow::Continue
        });
    });
    let code = app.run_with_args::<&str>(&[]);
    for error in errors.borrow().iter() {
        eprintln!("Capture : {error}");
    }
    if errors.borrow().is_empty() {
        code
    } else {
        glib::ExitCode::FAILURE
    }
}
