//! Opt-in real D-Bus/Unix IPC test; never installs components or submits a GPU operation.
use gameshift_profiles::*;
use serde_json::json;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
struct Running(Child, std::path::PathBuf);
impl Drop for Running {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            unsafe {
                libc::kill(self.0.id() as i32, libc::SIGTERM);
            }
        }
        let deadline = Instant::now() + Duration::from_secs(8);
        while self.0.try_wait().ok().flatten().is_none() {
            assert!(Instant::now() < deadline, "agent failed to stop");
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = fs::remove_dir_all(&self.1);
    }
}
#[test]
#[ignore = "requires isolated dbus-run-session; no GTK or hardware writes"]
fn agent_ipc_persistence_singleton_and_sni_without_a_graphical_interface() {
    assert_ne!(unsafe { libc::geteuid() }, 0);
    let root = std::env::temp_dir().join(format!("gameshift-agent-test-{}", new_id().unwrap()));
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let config_root = root.join("config");
    fs::create_dir(&config_root).unwrap();
    std::env::set_var("XDG_RUNTIME_DIR", &root);
    std::env::set_var("XDG_CONFIG_HOME", &config_root);
    let child = Command::new(env!("CARGO_BIN_EXE_gameshift-profiles"))
        .arg("daemon")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    let mut guard = Running(child, root.clone());
    let deadline = Instant::now() + Duration::from_secs(5);
    let initial = loop {
        if let Ok(value) = call(json!({"command":"get"})) {
            break value;
        }
        assert!(Instant::now() < deadline, "agent socket missing");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(initial["config"]["profiles"].as_array().unwrap().is_empty());
    {
        let guard = gameshift_control::benchmark::BenchmarkGuard::begin().unwrap();
        assert!(gameshift_control::benchmark::benchmark_active());
        drop(guard);
        assert!(!gameshift_control::benchmark::benchmark_active());
    }

    assert!(call(json!({"command":"get","path":"/etc/shadow"})).is_err());
    let second = Command::new(env!("CARGO_BIN_EXE_gameshift-profiles"))
        .arg("daemon")
        .output()
        .unwrap();
    assert!(!second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("déjà actif"));
    let mut config: Config = serde_json::from_value(initial["config"].clone()).unwrap();
    config.preferences.keep_tray_on_close = true;
    call(json!({"command":"configure","revision":initial["revision"],"config":config})).unwrap();
    assert!(call(
        json!({"command":"configure","revision":initial["revision"],"config":Config::default()})
    )
    .unwrap_err()
    .contains("changé"));
    assert!(Config::load().unwrap().preferences.keep_tray_on_close);
    let name = format!("org.kde.StatusNotifierItem-{pid}-1");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let result = Command::new("gdbus")
            .args([
                "call",
                "--session",
                "--dest",
                &name,
                "--object-path",
                "/MenuBar",
                "--method",
                "com.canonical.dbusmenu.GetLayout",
                "--",
                "0",
                "-1",
                "[]",
            ])
            .output()
            .unwrap();
        if result.status.success() {
            let text = String::from_utf8_lossy(&result.stdout);
            assert!(text.contains("Automatique"));
            assert!(text.contains("Ouvrir GameShift"));
            assert!(text.contains("Quitter GameShift"));
            break;
        }
        assert!(
            Instant::now() < deadline,
            "SNI D-Bus menu unavailable: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let cli = Command::new(env!("CARGO_BIN_EXE_gameshift-profiles"))
        .arg("status")
        .output()
        .unwrap();
    assert!(cli.status.success());
    assert!(
        serde_json::from_slice::<serde_json::Value>(&cli.stdout).unwrap()["config"]["preferences"]
            ["keep_tray_on_close"]
            == true
    );
    unsafe {
        libc::kill(guard.0.id() as i32, libc::SIGTERM);
    }
    assert!(guard.0.wait().unwrap().success());
    let child = Command::new(env!("CARGO_BIN_EXE_gameshift-profiles"))
        .arg("daemon")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    guard.0 = child;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(value) = call(json!({"command":"get"})) {
            assert_eq!(value["config"]["preferences"]["keep_tray_on_close"], true);
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(50));
    }
    // Quit through the real tray menu even when no GTK interface is running.
    let name = format!("org.kde.StatusNotifierItem-{}-1", guard.0.id());
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let result = Command::new("gdbus")
            .args([
                "call",
                "--session",
                "--dest",
                &name,
                "--object-path",
                "/MenuBar",
                "--method",
                "com.canonical.dbusmenu.Event",
                "--",
                "5",
                "clicked",
                "<0>",
                "0",
            ])
            .output()
            .unwrap();
        if result.status.success() {
            break;
        }
        assert!(Instant::now() < deadline, "tray quit action unavailable");
        std::thread::sleep(Duration::from_millis(100));
    }
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(status) = guard.0.try_wait().unwrap() {
            assert!(status.success(), "tray quit must exit cleanly");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "tray quit left the agent running"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!runtime().unwrap().join("profiles.sock").exists());
    assert!(Config::load().unwrap().preferences.keep_tray_on_close);
}
