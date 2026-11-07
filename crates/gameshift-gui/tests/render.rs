//! Opt-in real Vulkan tests. Fixtures never stand in for rendered hardware results.
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn config() -> serde_json::Value {
    let icd = std::env::var("GAMESHIFT_TEST_VULKAN_ICD")
        .expect("set GAMESHIFT_TEST_VULKAN_ICD to a software Vulkan ICD manifest");
    let probe = Command::new(env!("CARGO_BIN_EXE_gameshift-gui"))
        .args(["--vulkan-json", "--require-validation"])
        .env("VK_DRIVER_FILES", &icd)
        .output()
        .unwrap();
    assert!(
        probe.status.success(),
        "{}",
        String::from_utf8_lossy(&probe.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
    let gpu = report["gpus"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["kind"] == "Rendu logiciel (CPU)")
        .expect("software ICD must expose a software GPU");
    serde_json::json!({"gpu_uuid":gpu["device_uuid"],"width":320,"height":240,"grid_side":8,"warmup_seconds":1,"measure_seconds":2,"repetitions":1,"require_validation":true})
}

fn child(flag: &str) -> std::process::Child {
    Command::new(env!("CARGO_BIN_EXE_gameshift-gui"))
        .arg(flag)
        .env(
            "VK_DRIVER_FILES",
            std::env::var("GAMESHIFT_TEST_VULKAN_ICD").unwrap(),
        )
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

#[test]
#[ignore = "requires a software Vulkan ICD and installed validation layers"]
fn renders_distinct_actual_pixels_with_no_validation_error() {
    let request = config();
    let mut child = child("--render-smoke");
    writeln!(child.stdin.as_mut().unwrap(), "{request}").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["validation_errors"], 0);
    assert!(result["distinct_colors"].as_u64().unwrap() > 10);
    assert!(result["performance_score"].is_null());
}

#[test]
#[ignore = "requires a software Vulkan ICD and installed validation layers"]
fn worker_cancels_on_control_pipe_eof_without_returning_partial_results() {
    let request = config();
    let mut child = child("--bench-worker-headless");
    writeln!(child.stdin.as_mut().unwrap(), "{request}").unwrap();
    // wait_with_output closes stdin, exactly as parent process termination would.
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Test annulé"));
}

#[test]
#[ignore = "requires a software Vulkan ICD and installed validation layers"]
fn timed_run_excludes_preparation_and_reports_real_progress() {
    let mut request = config();
    request["measure_seconds"] = 1.into();
    request["repetitions"] = 2.into();
    request["duration_profile"] = "custom".into();
    let mut child = child("--bench-worker-headless");
    let mut control = child.stdin.take().unwrap();
    writeln!(control, "{request}").unwrap();
    let output = child.wait_with_output().unwrap();
    drop(control);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["schema_version"],
        gameshift_bench::RESULT_SCHEMA_VERSION
    );
    assert_eq!(report["duration"]["camera_measurement_start_seconds"], 3.);
    for pass in report["duration"]["passes"].as_array().unwrap() {
        assert!(pass["preparation_ms"].as_f64().unwrap() >= 1000.);
        assert!(pass["measurement_wall_ms"].as_f64().unwrap() >= 1000.);
    }
    for run in report["repetitions"].as_array().unwrap() {
        let samples = run["samples"].as_array().unwrap();
        assert_eq!(
            samples.len(),
            run["statistics"]["frames"].as_u64().unwrap() as usize
        );
        let measured = run["statistics"]["measured_ms"].as_f64().unwrap();
        assert!((1000.0..4000.0).contains(&measured));
    }
    let typed: gameshift_bench::RunResult = serde_json::from_value(report.clone()).unwrap();
    let csv = typed.csv();
    let mut lines = csv.lines();
    let header = lines.next().unwrap();
    assert!(header.starts_with("repetition,frame,frame_interval_ms,cpu_work_ms,gpu_execution_ms,"));
    assert!(header.contains("duration_protocol"));
    assert_eq!(
        header.split(',').count(),
        lines.next().unwrap().split(',').count()
    );
    let mut old = report.clone();
    old.as_object_mut().unwrap().remove("duration");
    old["config"]
        .as_object_mut()
        .unwrap()
        .remove("duration_profile");
    old["schema_version"] = 2.into();
    let legacy: gameshift_bench::RunResult = serde_json::from_value(old).unwrap();
    assert!(legacy.duration.is_none());
    assert_eq!(
        legacy.csv().lines().next().unwrap(),
        "repetition,frame,frame_interval_ms,cpu_work_ms,gpu_execution_ms"
    );
    let logs = String::from_utf8_lossy(&output.stderr);
    assert!(logs.contains("GAMESHIFT_PROGRESS"));
    assert!(logs.contains("analyse"));
}
