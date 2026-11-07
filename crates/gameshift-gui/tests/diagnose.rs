//! Exercise the real executable, including its no-display JSON entry point.
use std::{fs, process::Command};

#[test]
fn json_diagnostic_runs_without_graphical_session() {
    let output = Command::new(env!("CARGO_BIN_EXE_gameshift-gui"))
        .arg("--diagnose-json")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("XDG_RUNTIME_DIR")
        .output()
        .expect("diagnostic executable must start");
    assert!(
        output.status.success(),
        "diagnostic failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let snapshot: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must contain only JSON");
    assert_eq!(snapshot["schema_version"], 1);
    let sections = snapshot["sections"].as_array().expect("sections array");
    assert_eq!(sections.len(), 5);
    let readings: Vec<_> = sections
        .iter()
        .flat_map(|s| s["readings"].as_array().expect("readings array"))
        .collect();
    for reading in &readings {
        assert_eq!(reading["value"].is_null(), !reading["error"].is_null());
        assert!(!reading["source"]
            .as_str()
            .expect("source string")
            .is_empty());
    }
    let kernel = readings
        .iter()
        .find(|r| r["label"] == "Kernel")
        .expect("kernel observation");
    match fs::read_to_string("/proc/sys/kernel/osrelease") {
        Ok(expected) => assert_eq!(kernel["value"], expected.trim()),
        Err(_) => assert!(kernel["value"].is_null()),
    }
}

#[test]
fn missing_vulkan_driver_is_an_explicit_failure_not_a_score() {
    let output = Command::new(env!("CARGO_BIN_EXE_gameshift-gui"))
        .arg("--vulkan-json")
        .env("VK_DRIVER_FILES", "/gameshift-test/nonexistent-icd.json")
        .env_remove("VK_ADD_DRIVER_FILES")
        .env_remove("VK_ICD_FILENAMES")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .expect("Vulkan probe process must start");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let message = String::from_utf8_lossy(&output.stderr);
    assert!(message.contains("Vulkan"), "{message}");
    assert!(!message.contains("panicked"), "{message}");
}

#[test]
fn driver_inventory_is_headless_and_rejects_arbitrary_paths() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_gameshift-gui"))
        .arg("--driver-lab-json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["mode"], "read_only");
    assert!(report["inventories"].is_array());
    let bad = std::process::Command::new(env!("CARGO_BIN_EXE_gameshift-gui"))
        .args(["--driver-lab-json", "../../etc/passwd"])
        .output()
        .unwrap();
    assert!(!bad.status.success());
}
