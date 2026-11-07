//! Opt-in GTK rendering validation; no benchmark is run or modified.
use adw::prelude::*;
use gtk::glib;
use std::{cell::Cell, rc::Rc, time::Duration};
pub fn run(directory: std::path::PathBuf) -> Result<(), String> {
    adw::init().map_err(|e| e.to_string())?;
    let provider = gtk::CssProvider::new();
    provider.load_from_data(include_str!("style.css"));
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let snapshot = gameshift_hardware::Collector::default().collect();
    let stack = gtk::Stack::new();
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);
    stack.set_hexpand(true);
    stack.set_vexpand(true);
    crate::presentation::render(&stack, &snapshot, None);
    stack.add_titled(&crate::benchmark::page(), Some("laboratory"), "Laboratoire");
    let sidebar = gtk::StackSidebar::new();
    sidebar.set_width_request(180);
    sidebar.set_stack(&stack);
    let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    root.append(&sidebar);
    root.append(&stack);
    let window = gtk::Window::builder()
        .title("GameShift — contrôle GTK")
        .child(&root)
        .build();
    let pages = [
        "overview",
        "Système",
        "Processeur",
        "Carte graphique",
        "Mémoire",
        "Stockage",
        "laboratory",
    ];
    let cases = [(640, 600), (960, 720), (1320, 840)]
        .into_iter()
        .flat_map(|size| {
            [false, true]
                .into_iter()
                .flat_map(move |dark| pages.into_iter().map(move |page| (size, dark, page)))
        })
        .collect::<Vec<_>>();
    let index = Rc::new(Cell::new(0usize));
    let failures = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let main_loop = glib::MainLoop::new(None, false);
    window.set_default_size(640, 600);
    window.present();
    let loop_clone = main_loop.clone();
    let errors = failures.clone();
    glib::timeout_add_local(Duration::from_millis(250), move || {
        let i = index.get();
        if i > 0 {
            let ((width, _), dark, page) = cases[i - 1];
            if let Some(visible) = stack.visible_child() {
                if let Ok(scroll) = visible.downcast::<gtk::ScrolledWindow>() {
                    let adj = scroll.hadjustment();
                    if adj.upper() > adj.page_size() + 2. {
                        errors.borrow_mut().push(format!(
                            "Débordement horizontal {page} largeur {width}: {} > {}",
                            adj.upper(),
                            adj.page_size()
                        ));
                    }
                }
            }
            let paintable = gtk::WidgetPaintable::new(Some(&window));
            let snapshot = gtk::Snapshot::new();
            paintable.snapshot(&snapshot, window.width() as f64, window.height() as f64);
            match (snapshot.to_node(), window.renderer()) {
                (Some(node), Some(renderer)) => {
                    let texture = renderer.render_texture(&node, None);
                    if let Err(e) = texture.save_to_png(directory.join(format!(
                        "{width}-{}-{page}.png",
                        if dark { "dark" } else { "light" }
                    ))) {
                        errors.borrow_mut().push(e.to_string());
                    }
                }
                _ => errors.borrow_mut().push(format!("Rendu absent : {page}")),
            }
        }
        if i == cases.len() {
            window.close();
            loop_clone.quit();
            return glib::ControlFlow::Break;
        }
        let ((width, height), dark, page) = cases[i];
        adw::StyleManager::default().set_color_scheme(if dark {
            adw::ColorScheme::ForceDark
        } else {
            adw::ColorScheme::ForceLight
        });
        window.set_default_size(width, height);
        stack.set_visible_child_name(page);
        index.set(i + 1);
        glib::ControlFlow::Continue
    });
    main_loop.run();
    if !failures.borrow().is_empty() {
        return Err(failures.borrow().join("\n"));
    }
    println!("42 rendus GTK : sept pages, trois tailles, deux thèmes ; aucun débordement horizontal détecté.");
    Ok(())
}
