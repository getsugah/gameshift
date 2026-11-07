use std::process::Command;
#[test]
fn real_glibc_symbols_are_resolved_and_all_cases_validate() {
    let output = Command::new(env!("CARGO_BIN_EXE_aurelia-host-bench"))
        .arg("--smoke")
        .env_remove("LD_LIBRARY_PATH")
        .env_remove("LD_PRELOAD")
        .env_remove("LD_AUDIT")
        .env_remove("APPDIR")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!report["glibc_version"].as_str().unwrap().is_empty());
    let providers = report["providers"].as_array().unwrap();
    assert_eq!(providers.len(), 8);
    assert!(providers.iter().all(|p| p["path"] == providers[0]["path"]));
    let samples = report["samples"].as_array().unwrap();
    assert_eq!(samples.len(), 36);
    assert!(samples.iter().all(|s| s["verified"] == true
        && s["iterations"].as_u64().unwrap() > 0
        && s["elapsed_ns"].as_u64().unwrap() > 0));
    for pair in samples.as_chunks::<2>().0 {
        assert_eq!(pair[0]["iterations"], pair[1]["iterations"]);
    }
}
#[test]
fn inherited_library_path_is_refused() {
    let output = Command::new(env!("CARGO_BIN_EXE_aurelia-host-bench"))
        .arg("--smoke")
        .env("LD_LIBRARY_PATH", "/tmp")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Environnement non isolé"));
}

#[test]
fn provider_inside_appimage_directory_is_refused() {
    let output = Command::new(env!("CARGO_BIN_EXE_aurelia-host-bench"))
        .arg("--smoke")
        .env_remove("LD_LIBRARY_PATH")
        .env_remove("LD_PRELOAD")
        .env_remove("LD_AUDIT")
        // Treat the entire filesystem as an AppImage to exercise the rejection.
        .env("APPDIR", "/")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Bibliothèque embarquée refusée"));
}
