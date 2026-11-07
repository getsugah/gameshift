//! Profile management; IPC and filesystem operations run outside the GTK thread.
use adw::prelude::*;
use gameshift_profiles::{self as profiles, Config, Matcher, Mode, Profile, Rule};
use gtk::{gio, glib};
use serde_json::json;
use std::{cell::RefCell, rc::Rc, sync::mpsc, time::Duration};
thread_local! {static TRAY_ON_CLOSE:std::cell::Cell<bool>=const{std::cell::Cell::new(false)};}
pub fn keep_tray_on_close() -> bool {
    TRAY_ON_CLOSE.with(|v| v.get())
}
pub fn session_open() {
    std::thread::spawn(|| {
        let launcher = std::env::var_os("APPIMAGE")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::current_exe().ok())
            .map(|p| p.to_string_lossy().into_owned());
        if profiles::component().is_ok() {
            let _ = profiles::start();
        }
        let _ = update(move |config| {
            config.preferences.gui_launcher = launcher;
            Ok(())
        });
        let _ = profiles::call(json!({"command":"tray","visible":true}));
    });
}
fn label(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_wrap(true);
    l.set_wrap_mode(gtk::pango::WrapMode::Word);
    l.set_xalign(0.);
    l
}
thread_local! {static PENDING_APPLY:std::cell::Cell<usize>=const{std::cell::Cell::new(0)};}
pub fn pending_apply() -> bool {
    PENDING_APPLY.with(|v| v.get() > 0)
}
pub fn task(status: &gtk::Label, work: impl FnOnce() -> Result<String, String> + Send + 'static) {
    run_task(status, work, false);
}
pub fn profile_task(
    status: &gtk::Label,
    work: impl FnOnce() -> Result<String, String> + Send + 'static,
) {
    run_task(status, work, true);
}
fn run_task(
    status: &gtk::Label,
    work: impl FnOnce() -> Result<String, String> + Send + 'static,
    pending: bool,
) {
    if pending {
        PENDING_APPLY.with(|v| v.set(v.get() + 1));
    }
    status.set_text("Traitement en cours…");
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = tx.send(work());
    });
    let weak = status.downgrade();
    glib::timeout_add_local(Duration::from_millis(100), move || {
        let Some(status) = weak.upgrade() else {
            if pending {
                PENDING_APPLY.with(|v| v.set(v.get().saturating_sub(1)));
            }
            return glib::ControlFlow::Break;
        };
        let response = rx.try_recv();
        if pending && !matches!(response, Err(mpsc::TryRecvError::Empty)) {
            PENDING_APPLY.with(|v| v.set(v.get().saturating_sub(1)));
        }
        match response {
            Ok(Ok(text)) => {
                status.set_text(&text);
                glib::ControlFlow::Break
            }
            Ok(Err(error)) => {
                status.set_text(&error);
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                status.set_text("Traitement interrompu");
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
        }
    });
}
static CONFIG_UPDATE: std::sync::Mutex<()> = std::sync::Mutex::new(());
pub fn update(edit: impl FnOnce(&mut Config) -> Result<(), String>) -> Result<(), String> {
    let _guard = CONFIG_UPDATE.lock().map_err(|e| e.to_string())?;
    match profiles::call(json!({"command":"get"})) {
        Ok(state) => {
            let mut config: Config =
                serde_json::from_value(state["config"].clone()).map_err(|e| e.to_string())?;
            edit(&mut config)?;
            config.validate()?;
            profiles::call(
                json!({"command":"configure","config":config,"revision":state["revision"]}),
            )?;
            Ok(())
        }
        Err(error) => {
            if profiles::runtime().is_ok_and(|r| r.join("profiles.sock").exists()) {
                return Err(error);
            }
            let mut config = Config::load()?;
            edit(&mut config)?;
            config.save()
        }
    }
}
pub fn activation() -> gtk::Box {
    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let state = label("Contrôles GPU : vérification à la demande");
    let buttons = gtk::FlowBox::new();
    buttons.set_selection_mode(gtk::SelectionMode::None);
    buttons.set_max_children_per_line(3);
    let component = profiles::component().ok();
    let current = component
        .as_ref()
        .is_some_and(|v| v["version"] == env!("CARGO_PKG_VERSION"));
    let install = gtk::Button::with_label(if current {
        "Contrôles GPU activés"
    } else if component.is_some() {
        "Mettre à jour les contrôles GPU"
    } else {
        "Activer les contrôles GPU"
    });
    let nix = std::path::Path::new("/etc/NIXOS").exists();
    if nix {
        state.set_text("Nix : surveillance et benchmarks sans helper ; les composants privilégiés demandent une installation déclarative, non fournie dans cette alpha.");
    }
    install.set_sensitive(!current && !nix);
    install.set_visible(!current && !nix);
    if let Some(version) = component {
        state.set_text(&format!(
            "Contrôleur installé · version {} · profils persistants disponibles",
            version["version"].as_str().unwrap_or("inconnue")
        ));
    }
    install.add_css_class("suggested-action");
    let start = gtk::Button::with_label("Démarrer l’agent");
    let uninstall = gtk::Button::with_label("Désinstaller les contrôles");
    for button in [&start, &uninstall] {
        buttons.insert(button, -1);
    }
    box_.append(&install);
    install.connect_clicked({let state=state.clone();let install=install.clone();move |_|{install.set_sensitive(false);let install=install.clone();let (tx,rx)=mpsc::sync_channel(1);state.set_text("Autorisation Polkit pour activer les composants…");std::thread::spawn(move||{let _=tx.send(profiles::activate());});let state=state.clone();glib::timeout_add_local(Duration::from_millis(100),move||match rx.try_recv(){Ok(result)=>{install.set_sensitive(result.is_err());if result.is_ok(){install.set_label("Contrôles GPU activés");install.set_visible(false);}state.set_text(&result.map(|_|"Contrôles activés ; agent démarré. Aucune autorisation supplémentaire lors des changements de profils.".into()).unwrap_or_else(|e|e));glib::ControlFlow::Break},Err(mpsc::TryRecvError::Empty)=>glib::ControlFlow::Continue,Err(_)=>{install.set_sensitive(true);state.set_text("Activation interrompue");glib::ControlFlow::Break}});}});
    start.connect_clicked({
        let state = state.clone();
        move |_| {
            task(&state, || {
                profiles::start()?;
                Ok("Agent utilisateur démarré".into())
            })
        }
    });
    uninstall.connect_clicked({let state=state.clone();move|button|{let dialog=gtk::AlertDialog::builder().message("Désinstaller les contrôles GPU ?").detail("Les réglages seront restaurés et les composants système retirés. Vos profils personnels seront conservés.").buttons(["Annuler","Désinstaller"]).cancel_button(0).default_button(0).build();let parent=button.root().and_then(|w|w.downcast::<gtk::Window>().ok());let state=state.clone();dialog.choose(parent.as_ref(),None::<&gio::Cancellable>,move|answer|if answer==Ok(1){task(&state,||{profiles::deactivate()?;Ok("Composants désinstallés. Profils personnels conservés.".into())});});}});
    box_.append(&state);
    // Update sibling activation panels after install/update/uninstall without repeated version probes.
    use std::os::unix::fs::MetadataExt;
    let fingerprint = || {
        std::fs::symlink_metadata("/usr/libexec/gameshift-control")
            .ok()
            .map(|m| (m.ino(), m.mtime(), m.mtime_nsec()))
    };
    let mut last = fingerprint();
    let weak = box_.downgrade();
    let install = install.clone();
    glib::timeout_add_local(Duration::from_secs(1), move || {
        if weak.upgrade().is_none() {
            return glib::ControlFlow::Break;
        }
        let now = fingerprint();
        if now != last && !nix {
            last = now;
            let component = profiles::component().ok();
            let current = component
                .as_ref()
                .is_some_and(|v| v["version"] == env!("CARGO_PKG_VERSION"));
            install.set_sensitive(!current);
            install.set_visible(!current);
            install.set_label(if component.is_some() {
                "Mettre à jour les contrôles GPU"
            } else {
                "Activer les contrôles GPU"
            });
        }
        glib::ControlFlow::Continue
    });
    let maintenance = gtk::Expander::new(Some("Composants et diagnostic"));
    maintenance.set_child(Some(&buttons));
    box_.append(&maintenance);
    box_
}
fn parent(widget: &impl IsA<gtk::Widget>) -> Option<gtk::Window> {
    widget.root().and_then(|w| w.downcast::<gtk::Window>().ok())
}
fn prompt(
    widget: &impl IsA<gtk::Widget>,
    title: &str,
    value: &str,
    done: impl Fn(String) + 'static,
) {
    let dialog = gtk::Window::builder()
        .title(title)
        .modal(true)
        .default_width(420)
        .build();
    if let Some(parent) = parent(widget) {
        dialog.set_transient_for(Some(&parent));
    }
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    for side in [0, 1, 2, 3] {
        match side {
            0 => content.set_margin_top(20),
            1 => content.set_margin_bottom(20),
            2 => content.set_margin_start(20),
            _ => content.set_margin_end(20),
        }
    }
    let entry = gtk::Entry::new();
    entry.set_text(value);
    let save = gtk::Button::with_label("Enregistrer");
    save.add_css_class("suggested-action");
    content.append(&entry);
    content.append(&save);
    dialog.set_child(Some(&content));
    let weak = dialog.downgrade();
    save.connect_clicked(move |_| {
        done(entry.text().to_string());
        if let Some(dialog) = weak.upgrade() {
            dialog.close();
        }
    });
    dialog.present();
}
fn edit_gpu(widget: &impl IsA<gtk::Widget>, profile: Profile) {
    crate::amd_controls::edit_profile(profile);
    if let Some(app) = gio::Application::default() {
        app.activate_action("edit-gpu-profile", None);
    }
    let _ = widget;
}
fn profile_row(profile: &Profile, status: &gtk::Label, default: bool) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 8);
    row.add_css_class("gs-gpu-surface");
    let title = label(&format!(
        "{}{} · {} · {} réglages",
        profile.name,
        if default { " · par défaut" } else { "" },
        profile.request.identity.pci,
        profile.request.operations.len()
    ));
    title.add_css_class("heading");
    row.append(&title);
    let bar = gtk::FlowBox::new();
    bar.set_selection_mode(gtk::SelectionMode::None);
    bar.set_max_children_per_line(6);
    for text in [
        "Activer",
        "Modifier le GPU",
        "Renommer",
        "Dupliquer",
        "Par défaut",
        "Exporter",
        "Supprimer",
    ] {
        let button = gtk::Button::with_label(text);
        bar.insert(&button, -1);
        let profile = profile.clone();
        let status = status.clone();
        button.connect_clicked(move |button| {
            let profile = profile.clone();
            match text {
                "Activer" => {
                    let id = profile.id;
                    profile_task(&status, move || {
                        profiles::call(json!({"command":"manual","profile":id}))?;
                        profiles::wait_transition()?;
                        Ok("Mode manuel sélectionné ; voir le profil actif ci-dessus.".into())
                    })
                }
                "Modifier le GPU" => edit_gpu(button, profile),
                "Renommer" | "Dupliquer" => {
                    let status = status.clone();
                    let duplicate = text == "Dupliquer";
                    let initial = if duplicate {
                        format!("{} — copie", profile.name)
                    } else {
                        profile.name.clone()
                    };
                    prompt(button, text, &initial, move |name| {
                        let mut profile = profile.clone();
                        profile.name = name;
                        if duplicate {
                            match profiles::new_id() {
                                Ok(id) => profile.id = id,
                                Err(error) => {
                                    status.set_text(&error);
                                    return;
                                }
                            }
                        }
                        task(&status, move || {
                            update(move |c| {
                                if duplicate {
                                    c.profiles.push(profile);
                                } else if let Some(p) =
                                    c.profiles.iter_mut().find(|p| p.id == profile.id)
                                {
                                    *p = profile;
                                }
                                Ok(())
                            })?;
                            Ok("Profil enregistré".into())
                        });
                    });
                }
                "Par défaut" => task(&status, move || {
                    update(move |c| {
                        c.default_profile = Some(profile.id);
                        Ok(())
                    })?;
                    Ok("Profil par défaut enregistré".into())
                }),
                "Supprimer" => task(&status, move || {
                    update(move |c| {
                        let id = &profile.id;
                        c.profiles.retain(|p| &p.id != id);
                        c.rules.retain(|r| &r.profile != id);
                        if c.default_profile.as_ref() == Some(id) {
                            c.default_profile = None;
                        }
                        if matches!(&c.mode,Mode::Manual{profile} if profile==id) {
                            c.mode = Mode::Automatic {};
                        }
                        Ok(())
                    })?;
                    Ok("Profil supprimé".into())
                }),
                "Exporter" => {
                    let dialog = gtk::FileDialog::builder()
                        .title("Exporter le profil GPU")
                        .initial_name("profil.gsgpuprofile")
                        .build();
                    let status = status.clone();
                    dialog.save(
                        parent(button).as_ref(),
                        None::<&gio::Cancellable>,
                        move |result| {
                            if let Ok(file) = result {
                                if let Some(path) = file.path() {
                                    task(&status, move || {
                                        let data = profiles::export_profile(&profile)?;
                                        std::fs::write(path, data).map_err(|e| e.to_string())?;
                                        Ok("Copie du profil exportée".into())
                                    });
                                }
                            }
                        },
                    );
                }
                _ => (),
            }
        });
    }
    row.append(&bar);
    row
}
pub fn page() -> gtk::ScrolledWindow {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    for side in [0, 1, 2, 3] {
        match side {
            0 => content.set_margin_top(24),
            1 => content.set_margin_bottom(24),
            2 => content.set_margin_start(24),
            _ => content.set_margin_end(24),
        }
    }
    let title = label("Profils GPU");
    title.add_css_class("gs-page-title");
    content.append(&title);
    content.append(&label(
        "Configurez une fois, puis laissez GameShift accompagner vos applications.",
    ));
    content.append(&activation());
    let active = label("Chargement des profils…");
    active.add_css_class("heading");
    content.append(&active);
    let action_status = label("");
    action_status.set_selectable(true);
    content.append(&action_status);
    let toolbar = gtk::FlowBox::new();
    toolbar.set_selection_mode(gtk::SelectionMode::None);
    toolbar.set_max_children_per_line(4);
    for (text, command) in [
        ("Mode automatique", "automatic"),
        ("Réessayer le profil", "retry"),
        ("Pilote automatique", "driver_automatic"),
    ] {
        let button = gtk::Button::with_label(text);
        toolbar.insert(&button, -1);
        button.connect_clicked({
            let status = action_status.clone();
            move |_| {
                task(&status, move || {
                    profiles::call(json!({"command":command}))?;
                    Ok("Demande envoyée à l’agent".into())
                })
            }
        });
    }
    let create = gtk::Button::with_label("Créer depuis les réglages GPU");
    toolbar.insert(&create, -1);
    create.connect_clicked(|_| {
        crate::amd_controls::new_profile();
        if let Some(app) = gio::Application::default() {
            app.activate_action("edit-gpu-profile", None);
        }
    });
    let import = gtk::Button::with_label("Importer un profil");
    toolbar.insert(&import, -1);
    import.connect_clicked({
        let status = action_status.clone();
        move |button| {
            let dialog = gtk::FileDialog::builder()
                .title("Importer un profil GPU")
                .build();
            let status = status.clone();
            dialog.open(
                parent(button).as_ref(),
                None::<&gio::Cancellable>,
                move |result| {
                    if let Ok(file) = result {
                        if let Some(path) = file.path() {
                            task(&status, move || {
                                use std::io::Read;
                                let mut bytes = Vec::new();
                                std::fs::File::open(path)
                                    .and_then(|f| f.take(65537).read_to_end(&mut bytes))
                                    .map_err(|e| e.to_string())?;
                                let profile = profiles::import_profile(&bytes)?;
                                update(move |c| {
                                    c.profiles.push(profile);
                                    Ok(())
                                })?;
                                Ok("Profil importé sans règle d’activation automatique.".into())
                            });
                        }
                    }
                },
            );
        }
    });
    content.append(&toolbar);
    let list = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.append(&list);
    let settings = adw::PreferencesGroup::new();
    settings.set_title("Session et fermeture");
    let autostart = adw::SwitchRow::new();
    autostart.set_title("Démarrer la gestion des profils avec la session");
    autostart.set_subtitle("L’agent léger démarre sans fenêtre GTK.");
    settings.add(&autostart);
    let keep = adw::SwitchRow::new();
    keep.set_title("Conserver l’icône quand la fenêtre est fermée");
    keep.set_subtitle(
        "Sans icône compatible, la fenêtre quitte normalement ; les profils restent gérés.",
    );
    settings.add(&keep);
    let apply_preferences = gtk::Button::with_label("Enregistrer ces préférences");
    content.append(&settings);
    content.append(&apply_preferences);
    apply_preferences.connect_clicked({
        let status = action_status.clone();
        let autostart = autostart.clone();
        let keep = keep.clone();
        move |_| {
            let start = autostart.is_active();
            let keep = keep.is_active();
            task(&status, move || {
                update(move |c| {
                    c.preferences.autostart = start;
                    c.preferences.keep_tray_on_close = keep;
                    Ok(())
                })?;
                Ok("Préférences de session enregistrées".into())
            })
        }
    });
    let rule_group = gtk::Box::new(gtk::Orientation::Vertical, 10);
    rule_group.append(&label("Applications et priorités"));
    let matcher = gtk::DropDown::from_strings(&[
        "Exécutable Linux (chemin exact)",
        "Jeu Wine / Proton (nom .exe exact)",
    ]);
    rule_group.append(&matcher);
    let executable = gtk::Entry::new();
    executable.set_placeholder_text(Some("Chemin de l’exécutable ou nom du jeu .exe"));
    rule_group.append(&executable);
    let choose = gtk::Button::with_label("Choisir un exécutable…");
    rule_group.append(&choose);
    choose.connect_clicked({
        let executable = executable.clone();
        let matcher = matcher.clone();
        move |button| {
            let executable = executable.clone();
            let matcher = matcher.clone();
            gtk::FileDialog::new().open(
                parent(button).as_ref(),
                None::<&gio::Cancellable>,
                move |result| {
                    if let Ok(file) = result {
                        if let Some(path) = file.path() {
                            if path
                                .extension()
                                .is_some_and(|s| s.eq_ignore_ascii_case("exe"))
                            {
                                matcher.set_selected(1);
                                executable.set_text(
                                    &path.file_name().unwrap_or_default().to_string_lossy(),
                                );
                            } else if let Ok(path) = path.canonicalize() {
                                matcher.set_selected(0);
                                executable.set_text(&path.to_string_lossy());
                            }
                        }
                    }
                },
            );
        }
    });
    let steam = gtk::Entry::new();
    steam.set_placeholder_text(Some(
        "Steam App ID facultatif pour différencier deux jeux .exe",
    ));
    rule_group.append(&steam);
    let priority = gtk::SpinButton::with_range(-100000., 100000., 1.);
    priority.set_tooltip_text(Some(
        "Priorité : la plus grande gagne ; égalité départagée par identifiant stable de règle",
    ));
    rule_group.append(&priority);
    let choice = gtk::DropDown::from_strings(&[]);
    rule_group.append(&choice);
    let add = gtk::Button::with_label("Associer l’application au profil");
    rule_group.append(&add);
    content.append(&rule_group);
    let ids = Rc::new(RefCell::new(Vec::<String>::new()));
    add.connect_clicked({let ids=ids.clone();let choice=choice.clone();let status=action_status.clone();move |_|{let Some(profile)=ids.borrow().get(choice.selected() as usize).cloned()else{status.set_text("Créer ou importer un profil d’abord");return;};let exe=executable.text().trim().to_owned();let steam_text=steam.text().trim().to_owned();let steam_id=if steam_text.is_empty(){None}else{match steam_text.parse::<u32>(){Ok(v)=>Some(v),Err(_)=>{status.set_text("Steam App ID invalide");return;}}};let kind=if matcher.selected()==0{Matcher::Native{path:exe}}else{Matcher::WindowsExecutable{basename:exe,steam_app_id:steam_id}};let priority=priority.value_as_int();task(&status,move||{let rule=Rule{id:profiles::new_id()?,profile,matcher:kind,priority,enabled:true};update(move|c|{c.rules.push(rule);Ok(())})?;Ok("Règle enregistrée. Le mode manuel reste prioritaire jusqu’au retour en Automatique.".into())});}});
    let rules_list = gtk::Box::new(gtk::Orientation::Vertical, 10);
    content.append(&rules_list);
    let (tx, rx) = mpsc::channel();
    let busy = Rc::new(std::cell::Cell::new(false));
    let mut last_config = String::new();
    let mut last_fetch = std::time::Instant::now() - Duration::from_secs(3);
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(
            &adw::Clamp::builder()
                .maximum_size(1400)
                .child(&content)
                .build(),
        )
        .build();
    let weak = scroll.downgrade();
    glib::timeout_add_local(Duration::from_millis(200), move || {
        if weak.upgrade().is_none() {
            return glib::ControlFlow::Break;
        }
        if last_fetch.elapsed() >= Duration::from_secs(2) && !busy.get() {
            last_fetch = std::time::Instant::now();
            busy.set(true);
            let tx = tx.clone();
            std::thread::spawn(move || {
                let result = profiles::call(json!({"command":"get"}));
                let _ = tx.send(result);
            });
        }
        while let Ok(result) = rx.try_recv() {
            busy.set(false);
            let (config, summary) = match result {
                Ok(state) => {
                    TRAY_ON_CLOSE.with(|v| {
                        v.set(
                            state["config"]["preferences"]["keep_tray_on_close"] == true
                                && state["tray_available"] == true,
                        )
                    });
                    let config = serde_json::from_value::<Config>(state["config"].clone())
                        .unwrap_or_default();
                    let name = state["active"]
                        .as_str()
                        .and_then(|id| config.profiles.iter().find(|p| p.id == id))
                        .map(|p| p.name.as_str())
                        .unwrap_or("Pilote automatique");
                    let summary = format!(
                        "{} · {}\n{}{}",
                        name,
                        if !matches!(config.mode, Mode::Automatic { .. }) {
                            "Manuel"
                        } else {
                            "Automatique"
                        },
                        state["reason"].as_str().unwrap_or(""),
                        state["error"]
                            .as_str()
                            .map(|e| format!("\nÉchec : {e}"))
                            .unwrap_or_default()
                    );
                    (config, summary)
                }
                Err(error) => match Config::load() {
                    Ok(config) => (config, format!("Agent arrêté · {error}")),
                    Err(config_error) => {
                        active.set_text(&config_error);
                        continue;
                    }
                },
            };
            active.set_text(&summary);
            let signature = serde_json::to_string(&config).unwrap_or_default();
            if signature == last_config {
                continue;
            }
            last_config = signature;
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }
            for profile in &config.profiles {
                list.append(&profile_row(
                    profile,
                    &action_status,
                    config.default_profile.as_ref() == Some(&profile.id),
                ));
            }
            if config.profiles.is_empty() {
                list.append(&label("Aucun profil. Ouvrir les réglages GPU, cocher les paramètres souhaités puis Enregistrer le profil."));
            }
            let names = config
                .profiles
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>();
            let model = gtk::StringList::new(&names);
            let previous = ids.borrow().get(choice.selected() as usize).cloned();
            choice.set_model(Some(&model));
            *ids.borrow_mut() = config.profiles.iter().map(|p| p.id.clone()).collect();
            if let Some(index) =
                previous.and_then(|id| config.profiles.iter().position(|p| p.id == id))
            {
                choice.set_selected(index as u32);
            }
            autostart.set_active(config.preferences.autostart);
            keep.set_active(config.preferences.keep_tray_on_close);
            while let Some(child) = rules_list.first_child() {
                rules_list.remove(&child);
            }
            for rule in &config.rules {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                let name = config
                    .profiles
                    .iter()
                    .find(|p| p.id == rule.profile)
                    .map(|p| p.name.as_str())
                    .unwrap_or("Absent");
                let source = match &rule.matcher {
                    Matcher::Native { path } => path.clone(),
                    Matcher::WindowsExecutable {
                        basename,
                        steam_app_id,
                    } => format!(
                        "{basename}{}",
                        steam_app_id
                            .map(|id| format!(" · Steam {id}"))
                            .unwrap_or_default()
                    ),
                };
                let text = label(&format!("{source} → {name}"));
                text.set_hexpand(true);
                row.append(&text);
                let enabled = gtk::CheckButton::with_label("Active");
                enabled.set_active(rule.enabled);
                let rank = gtk::SpinButton::with_range(-100000., 100000., 1.);
                rank.set_value(rule.priority as f64);
                row.append(&enabled);
                row.append(&rank);
                let save = gtk::Button::with_label("Enregistrer");
                row.append(&save);
                let remove = gtk::Button::with_label("Retirer");
                row.append(&remove);
                let id = rule.id.clone();
                save.connect_clicked({
                    let status = action_status.clone();
                    move |_| {
                        let id = id.clone();
                        let enabled = enabled.is_active();
                        let priority = rank.value_as_int();
                        task(&status, move || {
                            update(move |c| {
                                if let Some(r) = c.rules.iter_mut().find(|r| r.id == id) {
                                    r.enabled = enabled;
                                    r.priority = priority;
                                }
                                Ok(())
                            })?;
                            Ok("Règle mise à jour".into())
                        });
                    }
                });
                let id = rule.id.clone();
                remove.connect_clicked({
                    let status = action_status.clone();
                    move |_| {
                        let id = id.clone();
                        task(&status, move || {
                            update(move |c| {
                                c.rules.retain(|r| r.id != id);
                                Ok(())
                            })?;
                            Ok("Règle supprimée".into())
                        });
                    }
                });
                rules_list.append(&row);
            }
        }
        glib::ControlFlow::Continue
    });
    scroll
}
