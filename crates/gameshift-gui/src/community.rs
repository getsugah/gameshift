//! Explicit diagnostic export with an allowlist. Never exports the raw Collector snapshot.
use adw::prelude::*;
use gtk::{gio, glib};
use serde_json::{json, Value};
use std::{fs, io::Read, path::Path, sync::mpsc, time::Duration};
fn read(root: &Path, path: &str) -> Option<String> {
    let mut text = String::new();
    fs::File::open(root.join(path))
        .ok()?
        .take(65536)
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}
fn os_fields(text: &str) -> Value {
    let mut map = serde_json::Map::new();
    for line in text.lines() {
        if let Some((key, value)) = line.split_once('=') {
            if ["ID", "VERSION_ID", "PRETTY_NAME"].contains(&key) {
                map.insert(key.into(), json!(value.trim_matches('"')));
            }
        }
    }
    Value::Object(map)
}
fn safe_error(text: &str) -> String {
    text.split_whitespace()
        .map(|word| {
            if ["/home/", "/run/user/", "/tmp/", "/root/"]
                .iter()
                .any(|p| word.contains(p))
            {
                "<chemin privé>"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
pub fn build_at(root: &Path) -> Value {
    let cpu = read(root, "proc/cpuinfo").and_then(|s| {
        s.lines().find_map(|l| {
            l.split_once(':')
                .filter(|(k, _)| k.trim() == "model name")
                .map(|(_, v)| v.trim().to_owned())
        })
    });
    let memory = read(root, "proc/meminfo").and_then(|s| {
        s.lines()
            .find_map(|l| l.strip_prefix("MemTotal:").map(|v| v.trim().to_owned()))
    });
    let (gpus, errors) = match gameshift_telemetry::gpus::discover_at(root) {
        Ok(g) => (g, vec![]),
        Err(e) => (vec![], vec![safe_error(&e)]),
    };
    json!({"schema_version":1,"application":{"name":"GameShift","version":env!("CARGO_PKG_VERSION"),"aurelia_raster":gameshift_bench::SCENE_VERSION,"aurelia_system":gameshift_system_bench::system::PROTOCOL},
        "system":{"distribution":os_fields(&read(root,"etc/os-release").unwrap_or_default()),"kernel":read(root,"proc/sys/kernel/osrelease").map(|s|s.trim().to_owned()),"cpu_model":cpu,"memory_total":memory},
        "gpus":gpus,"errors":errors,"features":{"monitoring_without_helper":true,"benchmarks_without_helper":true,"gpu_processes":"DRM fdinfo: client-id, engine ns, capacity and resident memory; depends on driver and permissions","cycles_only_accounting":"not implemented","video_capabilities":"optional explicit VA-API query using vainfo","nvidia_private_backend":"not implemented","privileged_controls":"optional AMDGPU only; declarative installation required on Nix"},
        "privacy":{"automatic_upload":false,"raw_process_list":false,"hostname_or_serial_numbers":false,"environment_or_command_line":false},"benchmarks":[]})
}
fn benchmark_summary(value: &Value) -> Value {
    let metrics=value["metrics"].as_array().map(|metrics|metrics.iter().take(256).map(|m|json!({"id":m["id"],"label":m["label"],"unit":m["unit"],"values":m["values"]})).collect::<Vec<_>>()).unwrap_or_default();
    json!({"protocol":value["protocol"],"application_version":value["application_version"],"duration_seconds":value["elapsed_seconds"],"metrics":metrics})
}
pub fn build() -> Value {
    let mut report = build_at(Path::new("/"));
    match gameshift_bench::probe(gameshift_bench::ValidationMode::Disabled) {
        Ok(api) => {
            report["graphics_api"] = json!({"name":"Vulkan","devices":api.gpus.iter().map(|g|json!({"name":g.name,"kind":g.kind,"api_version":g.api_version,"driver_name":g.driver_name,"vendor_id":g.vendor_id,"device_id":g.device_id,"graphics_timestamps":g.graphics_timestamps()})).collect::<Vec<_>>() })
        }
        Err(error) => {
            if let Some(errors) = report["errors"].as_array_mut() {
                errors.push(json!(safe_error(&error.to_string())));
            }
        }
    }
    if !crate::live_settings::benchmark_running() {
        let sample = gameshift_telemetry::processes::Tracker::default().sample();
        report["process_accounting_observation"] = json!({"clients_visible":sample.rows.iter().map(|r|r.clients).sum::<usize>(),"denied_reads":sample.denied,"partial":sample.truncated,"meaning":"Une absence de clients ne prouve pas une absence de prise en charge ; aucun PID ou nom d’application collecté dans ce rapport."});
    }
    report
}
fn label(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_wrap(true);
    label.set_xalign(0.);
    label
}
pub fn panel() -> gtk::Box {
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 10);
    let title = label("Diagnostic local · alpha");
    title.add_css_class("heading");
    panel.append(&title);
    panel.append(&label("Préparez un diagnostic local, choisissez les résultats à joindre, puis relisez le contenu. Aucun envoi automatique, aucune collecte du nom d’hôte, des numéros de série ou de la liste des processus."));
    let open = gtk::Button::with_label("Exporter un rapport de diagnostic…");
    panel.append(&open);
    open.connect_clicked(|button|{
        let window=gtk::Window::builder().title("Diagnostic · aperçu avant export").default_width(880).default_height(720).build();if let Some(parent)=button.root().and_then(|w|w.downcast::<gtk::Window>().ok()){window.set_transient_for(Some(&parent));}
        let content=gtk::Box::new(gtk::Orientation::Vertical,12);content.set_margin_top(18);content.set_margin_bottom(18);content.set_margin_start(18);content.set_margin_end(18);
        let status=label("Préparation du diagnostic et de l’historique, sans privilèges…");content.append(&status);
        let selections=gtk::Box::new(gtk::Orientation::Vertical,6);let attach=gtk::Expander::new(Some("Résultats système à joindre (facultatif, au maximum 8)"));attach.set_child(Some(&selections));content.append(&attach);
        let view=gtk::TextView::new();view.set_editable(false);view.set_monospace(true);view.set_wrap_mode(gtk::WrapMode::WordChar);let scroll=gtk::ScrolledWindow::builder().vexpand(true).child(&view).build();content.append(&scroll);
        let refresh=gtk::Button::with_label("Actualiser l’aperçu avec cette sélection");refresh.set_sensitive(false);content.append(&refresh);
        let export=gtk::Button::with_label("Exporter la copie affichée…");export.set_sensitive(false);export.add_css_class("suggested-action");content.append(&export);window.set_child(Some(&content));window.present();
        let base=std::rc::Rc::new(std::cell::RefCell::new(None::<Value>));let choices=std::rc::Rc::new(std::cell::RefCell::new(Vec::<(gtk::CheckButton,String)>::new()));
        let (tx,rx)=mpsc::sync_channel(1);std::thread::spawn(move||{let report=build();let entries=gameshift_system_bench::store::list(&gameshift_system_bench::store::root()).unwrap_or_default();let _=tx.send((report,entries));});
        let weak=window.downgrade();let base_=base.clone();let choices_=choices.clone();let view_=view.clone();let refresh_=refresh.clone();let export_=export.clone();let status_=status.clone();
        glib::timeout_add_local(Duration::from_millis(100),move||{if weak.upgrade().is_none(){return glib::ControlFlow::Break;}match rx.try_recv(){Ok((report,mut entries))=>{view_.buffer().set_text(&serde_json::to_string_pretty(&report).unwrap_or_default());*base_.borrow_mut()=Some(report);entries.sort_by_key(|e|std::cmp::Reverse(e.completed_unix_seconds));for entry in entries.into_iter().take(32){let check=gtk::CheckButton::with_label(&format!("{} · {}",entry.protocol,&entry.id[..12]));selections.append(&check);choices_.borrow_mut().push((check,entry.id));}refresh_.set_sensitive(true);export_.set_sensitive(true);status_.set_text("Diagnostic limité aux informations techniques listées. Consultez l’aperçu avant export. Aucun fichier n’a encore été créé.");glib::ControlFlow::Break},Err(mpsc::TryRecvError::Disconnected)=>{status_.set_text("Préparation interrompue");glib::ControlFlow::Break},_=>glib::ControlFlow::Continue}});
        refresh.connect_clicked({let base=base.clone();let view=view.clone();let status=status.clone();move|button|{
            let Some(mut report)=base.borrow().clone()else{return};let ids=choices.borrow().iter().filter(|(c,_)|c.is_active()).map(|(_,id)|id.clone()).collect::<Vec<_>>();if ids.len()>8{status.set_text("Sélectionner au maximum 8 résultats");return;}
            button.set_sensitive(false);let weak=button.downgrade();let (tx,rx)=mpsc::sync_channel(1);std::thread::spawn(move||{let mut summaries=vec![];for id in ids{match gameshift_system_bench::store::load(&gameshift_system_bench::store::root(),&id){Ok(session)=>summaries.push(benchmark_summary(&serde_json::to_value(session).unwrap_or_default())),Err(e)=>{if let Some(errors)=report["errors"].as_array_mut(){errors.push(json!(safe_error(&e)));}}}}report["benchmarks"]=json!(summaries);let _=tx.send(serde_json::to_string_pretty(&report).unwrap_or_default());});let view=view.clone();glib::timeout_add_local(Duration::from_millis(100),move||{let Some(button)=weak.upgrade()else{return glib::ControlFlow::Break;};if let Ok(text)=rx.try_recv(){view.buffer().set_text(&text);button.set_sensitive(true);glib::ControlFlow::Break}else{glib::ControlFlow::Continue}});
        }});
        export.connect_clicked({let view=view.clone();let status=status.clone();move|button|{let buffer=view.buffer();let text=buffer.text(&buffer.start_iter(),&buffer.end_iter(),false).to_string();let status=status.clone();let dialog=gtk::FileDialog::builder().title("Exporter le diagnostic affiché").initial_name("GameShift-diagnostic.json").build();let parent=button.root().and_then(|w|w.downcast::<gtk::Window>().ok());dialog.save(parent.as_ref(),None::<&gio::Cancellable>,move|result|{if let Ok(file)=result{if let Some(path)=file.path(){crate::profiles_page::task(&status,move||{fs::write(path,text).map_err(|e|e.to_string())?;Ok("Diagnostic exporté localement. Aucun envoi effectué.".into())});}}});}});
    });
    panel
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn benchmark_diagnostic_is_an_allowlist() {
        let raw = json!({"protocol":"test","elapsed_seconds":2.,"hostname":"private","fingerprint":{"serial":"secret"},"glibc_raw":{"path":"/home/private/lib.so"},"metrics":[{"id":"cpu","label":"CPU","unit":"ops/s","values":[1,2,3],"work_description":"/home/private"}]});
        let summary = benchmark_summary(&raw).to_string();
        assert!(!summary.contains("private"));
        assert!(!summary.contains("serial"));
        assert!(summary.contains("ops/s"));
    }
    #[test]
    fn os_fields_and_errors_never_copy_unrelated_identity() {
        let os = os_fields(
            "ID=nixos\nPRETTY_NAME=\"NixOS\"\nHOSTNAME=secret\nHOME_URL=https://example.test\n",
        )
        .to_string();
        assert!(!os.contains("secret"));
        assert!(!os.contains("HOME_URL"));
        assert!(
            !safe_error("failed /home/alice/private/lib.so /run/user/1000/path").contains("alice")
        );
    }
}
