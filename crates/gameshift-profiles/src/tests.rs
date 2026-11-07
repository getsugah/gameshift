use crate::process::*;
use crate::*;
use gameshift_control::{FanRecovery, Identity, Operation};
fn profile(id: &str) -> Profile {
    Profile {
        id: id.into(),
        name: id.into(),
        request: gameshift_control::Request {
            schema_version: 1,
            advanced_confirmed: false,
            identity: Identity {
                pci: "0000:09:00.0".into(),
                vendor: "0x1002".into(),
                device: "0x747e".into(),
                revision: "0xc8".into(),
                subsystem_vendor: "0x1eae".into(),
                subsystem_device: "0x7801".into(),
                unique_id: None,
            },
            fan_recovery: FanRecovery::Automatic,
            operations: vec![Operation::Performance {
                mode: "auto".into(),
            }],
        },
    }
}
fn rule(id: &str, profile: &str, priority: i32, path: &str) -> Rule {
    Rule {
        id: id.into(),
        profile: profile.into(),
        matcher: Matcher::Native { path: path.into() },
        priority,
        enabled: true,
    }
}
fn sample() -> Config {
    Config {
        profiles: vec![profile("silent"), profile("gaming"), profile("compute")],
        default_profile: Some("silent".into()),
        rules: vec![
            rule("a", "gaming", 10, "/opt/game"),
            rule("b", "compute", 20, "/opt/ai"),
        ],
        ..Config::default()
    }
}
#[test]
fn rules_are_deterministic_with_priorities_and_return_to_default() {
    let c = sample();
    c.validate().unwrap();
    let mut processes = vec![Process {
        pid: 42,
        executable: "/opt/game".into(),
        ..Process::default()
    }];
    assert_eq!(select(&c, &processes).profile.as_deref(), Some("gaming"));
    processes.push(Process {
        pid: 71,
        executable: "/opt/ai".into(),
        ..Process::default()
    });
    assert_eq!(select(&c, &processes).profile.as_deref(), Some("compute"));
    processes.pop();
    assert_eq!(select(&c, &processes).profile.as_deref(), Some("gaming"));
    processes.clear();
    assert_eq!(select(&c, &processes).profile.as_deref(), Some("silent"));
}
#[test]
fn manual_mode_cannot_be_overridden_by_a_running_application() {
    let mut c = sample();
    c.mode = Mode::Manual {
        profile: "silent".into(),
    };
    let p = Process {
        executable: "/opt/ai".into(),
        ..Process::default()
    };
    assert_eq!(select(&c, &[p]).profile.as_deref(), Some("silent"));
}
#[test]
fn tie_breaking_does_not_depend_on_process_or_rule_order() {
    let mut c = sample();
    c.rules[0].priority = 20;
    c.rules.reverse();
    let a = Process {
        executable: "/opt/ai".into(),
        pid: 5,
        ..Process::default()
    };
    let b = Process {
        executable: "/opt/game".into(),
        pid: 100,
        ..Process::default()
    };
    assert_eq!(
        select(&c, &[a.clone(), b.clone()]).profile.as_deref(),
        Some("gaming")
    );
    assert_eq!(select(&c, &[b, a]).profile.as_deref(), Some("gaming"));
}
#[test]
fn steam_and_launchers_alone_never_match_a_proton_game() {
    let matcher = Matcher::WindowsExecutable {
        basename: "Game.exe".into(),
        steam_app_id: Some(123),
    };
    let mut p = Process {
        executable: "/usr/bin/steam".into(),
        arguments: vec!["Z:\\Games\\Game.exe".into()],
        steam_app_id: Some(123),
        ..Process::default()
    };
    assert!(!matcher.matches(&p));
    p.executable = "/usr/bin/python3".into();
    assert!(!matcher.matches(&p));
    p.executable = "/opt/proton/wine64-preloader".into();
    assert!(matcher.matches(&p));
    p.steam_app_id = Some(124);
    assert!(!matcher.matches(&p));
    p.steam_app_id = Some(123);
    p.arguments = vec!["OtherGame.exe".into()];
    assert!(!matcher.matches(&p));
}
#[test]
fn wine_without_steam_is_supported_and_native_paths_are_exact() {
    let windows = Matcher::WindowsExecutable {
        basename: "Game.exe".into(),
        steam_app_id: None,
    };
    let p = Process {
        executable: "/usr/bin/wine64".into(),
        arguments: vec!["/games/game.exe".into()],
        ..Process::default()
    };
    assert!(windows.matches(&p));
    assert!(!Matcher::Native {
        path: "/bin/wine64".into()
    }
    .matches(&p));
}
#[test]
fn debounce_limits_automatic_flapping_but_manual_is_immediate() {
    let mut d = Debounce::default();
    let a = Some("a".into());
    let b = Some("b".into());
    assert!(!d.ready(&a, 0, false));
    assert!(!d.ready(&a, 2, false));
    assert!(d.ready(&a, 3, false));
    d.changed(3);
    assert!(!d.ready(&b, 4, false));
    assert!(!d.ready(&b, 7, false));
    assert!(d.ready(&b, 8, false));
    assert!(d.ready(&a, 8, true));
}
#[test]
fn imports_are_versioned_validated_and_never_import_activation_rules() {
    let p = profile("gaming");
    let data = export_profile(&p).unwrap();
    let imported = import_profile(&data).unwrap();
    assert_ne!(imported.id, p.id);
    assert_eq!(imported.name, p.name);
    assert_eq!(imported.request.operations.len(), 1);
    assert!(import_profile(br#"{"schema_version":2,"profile":{}}"#).is_err());
    assert!(import_profile(&vec![b'x'; 65537]).is_err());
    let mut value: Value = serde_json::from_slice(&data).unwrap();
    value["rules"] = json!([]);
    assert!(import_profile(&serde_json::to_vec(&value).unwrap()).is_err());
}
#[test]
fn invalid_configs_cannot_enable_unknown_profiles_or_arbitrary_paths() {
    let mut c = sample();
    c.rules[0].profile = "absent".into();
    assert!(c.validate().is_err());
    c = sample();
    c.profiles.push(profile("gaming"));
    assert!(c.validate().is_err());
    c = sample();
    c.rules[0].matcher = Matcher::Native {
        path: "steam".into(),
    };
    assert!(c.validate().is_err());
    c = sample();
    c.rules[0].matcher = Matcher::WindowsExecutable {
        basename: "/Game.exe".into(),
        steam_app_id: None,
    };
    assert!(c.validate().is_err());
}
#[test]
fn persistence_roundtrip_keeps_sparse_settings_and_rejects_corruption() {
    let root = std::env::temp_dir().join(format!(
        "gameshift-profiles-{}-{}",
        std::process::id(),
        new_id().unwrap()
    ));
    fs::create_dir(&root).unwrap();
    let path = root.join("profiles.json");
    let c = sample();
    save_at(&path, &c).unwrap();
    assert_eq!(
        load_at(&path).unwrap().profiles[0].request.operations.len(),
        1
    );
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    fs::write(&path, b"corrupt").unwrap();
    assert!(load_at(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"corrupt");
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn ipc_exchange_handles_errors_and_bounds_without_hardware() {
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let worker = std::thread::spawn(move || {
        let mut line = String::new();
        std::io::BufReader::new(&mut server)
            .read_line(&mut line)
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&line).unwrap()["command"],
            "ping"
        );
        writeln!(
            server,
            "{}",
            json!({"ok":false,"error":"permission refused"})
        )
        .unwrap();
    });
    assert_eq!(
        exchange(&mut client, &json!({"command":"ping"})).unwrap_err(),
        "permission refused"
    );
    worker.join().unwrap();
}
#[test]
fn manual_driver_mode_preserves_defaults_and_application_rules() {
    let mut c = sample();
    c.mode = Mode::Driver {};
    c.validate().unwrap();
    let running = Process {
        pid: 9,
        executable: "/opt/ai".into(),
        ..Process::default()
    };
    assert_eq!(select(&c, std::slice::from_ref(&running)).profile, None);
    assert_eq!(c.rules.len(), 2);
    assert_eq!(c.default_profile.as_deref(), Some("silent"));
    c.mode = Mode::Automatic {};
    assert_eq!(select(&c, &[running]).profile.as_deref(), Some("compute"));
}
#[test]
fn steam_launcher_is_rejected_as_an_automatic_rule() {
    let mut c = sample();
    c.rules[0].matcher = Matcher::Native {
        path: "/usr/bin/steam".into(),
    };
    assert!(c.validate().unwrap_err().contains("Steam seul"));
}
#[test]
fn appimage_bootstrap_is_staged_privately_and_only_its_own_copies_are_removed() {
    let root = std::env::temp_dir().join(format!("gameshift-stage-test-{}", new_id().unwrap()));
    let source = root.join("source");
    fs::create_dir_all(&source).unwrap();
    for name in ["gameshift-control", "gameshift-profiles"] {
        fs::write(source.join(name), b"test payload").unwrap();
    }
    let stage = stage_bundle_at(&source, &root).unwrap();
    let path = stage.directory.clone();
    assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o700);
    assert_eq!(
        fs::read(path.join("gameshift-control")).unwrap(),
        b"test payload"
    );
    assert_eq!(
        fs::metadata(path.join("gameshift-profiles"))
            .unwrap()
            .mode()
            & 0o777,
        0o755
    );
    drop(stage);
    assert!(!path.exists());
    assert!(source.join("gameshift-control").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn benchmarks_hold_automatic_selection_but_allow_explicit_manual_actions() {
    let mut config = sample();
    let processes = vec![Process {
        pid: 7,
        executable: "/opt/ai".into(),
        ..Process::default()
    }];
    assert_eq!(
        select_for_benchmark(&config, &processes, Some("silent".into()), true)
            .profile
            .as_deref(),
        Some("silent")
    );
    assert_eq!(
        select_for_benchmark(&config, &processes, None, false)
            .profile
            .as_deref(),
        Some("compute")
    );
    config.mode = Mode::Manual {
        profile: "gaming".into(),
    };
    assert_eq!(
        select_for_benchmark(&config, &processes, Some("silent".into()), true)
            .profile
            .as_deref(),
        Some("gaming")
    );
}
