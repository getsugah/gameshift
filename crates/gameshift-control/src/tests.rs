use super::*;
use std::collections::BTreeMap;
#[derive(Clone)]
struct Fake {
    id: Identity,
    files: BTreeMap<String, String>,
    pending: BTreeMap<String, String>,
    writes: Vec<(String, String)>,
    fail_commit: bool,
}
fn identity() -> Identity {
    Identity {
        pci: "0000:09:00.0".into(),
        vendor: "0x1002".into(),
        device: "0x747e".into(),
        revision: "0xc8".into(),
        subsystem_vendor: "0x1002".into(),
        subsystem_device: "0x0001".into(),
        unique_id: None,
    }
}
fn curve() -> String {
    "OD_FAN_CURVE:\n0: 30C 20%\n1: 60C 50%\n2: 95C 100%\nOD_RANGE:\nFAN_CURVE(hotspot temp): 25C 100C\nFAN_CURVE(fan speed): 20% 100%".into()
}
impl Fake {
    fn new() -> Self {
        let mut files = BTreeMap::new();
        files.insert("gpu_od/fan_ctrl/fan_curve".into(), curve());
        files.insert("power_dpm_force_performance_level".into(), "auto".into());
        files.insert(
            "pp_power_profile_mode".into(),
            "0 BOOTUP_DEFAULT*\n1 3D_FULL_SCREEN\n2 POWER_SAVING".into(),
        );
        for setting in FanSetting::ALL {
            let (_, header, key, _, _) = setting.definition();
            let (value, min, max) = match setting {
                FanSetting::ZeroRpm => (0, 0, 1),
                FanSetting::ZeroRpmStopTemperature => (45, 25, 70),
                FanSetting::TargetTemperature => (80, 60, 95),
                FanSetting::MinimumPwm => (20, 10, 100),
                _ => (2000, 500, 3000),
            };
            files.insert(
                setting.path(),
                format!("{header}:\n{value}\nOD_RANGE:\n{key}: {min} {max}"),
            );
        }
        files.insert("hwmon/hwmon73/power1_cap".into(), "250000000".into());
        files.insert("hwmon/hwmon73/power1_cap_min".into(), "100000000".into());
        files.insert("hwmon/hwmon73/power1_cap_max".into(), "300000000".into());
        files.insert("pp_od_clk_voltage".into(),"OD_SCLK:\n0: 500Mhz\n1: 2500Mhz\nOD_MCLK:\n1: 1200Mhz\nOD_VDDGFX_OFFSET:\n0mV\nOD_RANGE:\nSCLK: 500Mhz 3000Mhz\nMCLK: 500Mhz 1500Mhz\nVDDGFX_OFFSET: -200mv 0mv".into());
        Self {
            id: identity(),
            files,
            pending: BTreeMap::new(),
            writes: vec![],
            fail_commit: false,
        }
    }
    fn request(&self, operations: Vec<Operation>) -> Request {
        Request {
            schema_version: 1,
            advanced_confirmed: false,
            identity: self.id.clone(),
            fan_recovery: FanRecovery::Automatic,
            operations,
        }
    }
}
impl DeviceIo for Fake {
    fn identity(&self) -> Result<Identity> {
        Ok(self.id.clone())
    }
    fn read(&self, path: &str) -> Result<String> {
        self.files.get(path).cloned().ok_or("Absent".into())
    }
    fn power_paths(&self) -> Result<Vec<(u32, String)>> {
        Ok(vec![(1, "hwmon/hwmon73/power1_cap".into())])
    }
    fn write(&mut self, path: &str, command: &str) -> Result<()> {
        self.writes.push((path.into(), command.into()));
        let command = command.trim();
        if command == "c" {
            if self.fail_commit {
                self.fail_commit = false;
                return Err("Refus simulé du pilote".into());
            }
            if let Some(value) = self.pending.remove(path) {
                self.files.insert(path.into(), value);
            }
            return Ok(());
        }
        if path.starts_with("gpu_od/fan_ctrl/") {
            if path.ends_with("fan_curve") {
                if command == "r" {
                    self.files.insert(path.into(), curve());
                    return Ok(());
                }
                let values = command
                    .split_whitespace()
                    .map(|s| s.parse::<i64>().unwrap())
                    .collect::<Vec<_>>();
                let mut current = parse_curve(
                    self.pending
                        .get(path)
                        .unwrap_or(self.files.get(path).unwrap()),
                )?;
                current.points[values[0] as usize] = Point {
                    temperature: values[1],
                    pwm_percent: values[2],
                };
                let points = current
                    .points
                    .iter()
                    .enumerate()
                    .map(|(i, p)| format!("{i}: {}C {}%", p.temperature, p.pwm_percent))
                    .collect::<Vec<_>>()
                    .join("\n");
                self.pending.insert(path.into(),format!("OD_FAN_CURVE:\n{points}\nOD_RANGE:\nFAN_CURVE(hotspot temp): 25C 100C\nFAN_CURVE(fan speed): 20% 100%"));
            } else {
                let old = self.files.get(path).unwrap();
                let mut lines = old.lines().map(str::to_owned).collect::<Vec<_>>();
                lines[1] = command.into();
                self.pending.insert(path.into(), lines.join("\n"));
            }
            return Ok(());
        }
        if path == "pp_power_profile_mode" {
            let index = command.parse::<u32>().unwrap();
            let updated = parse_workloads(self.files.get(path).unwrap())
                .iter()
                .map(|p| {
                    format!(
                        "{} {}{}",
                        p.index,
                        p.name,
                        if p.index == index { "*" } else { "" }
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            self.files.insert(path.into(), updated);
            return Ok(());
        }
        if path == "pp_od_clk_voltage" {
            let old = self
                .pending
                .get(path)
                .unwrap_or(self.files.get(path).unwrap())
                .clone();
            let (mut clocks, mut voltage) = parse_overdrive(&old)?;
            let parts = command.split_whitespace().collect::<Vec<_>>();
            match parts.as_slice() {
                ["vo", value] => voltage.as_mut().unwrap().0 = value.parse().unwrap(),
                [domain, index, value] => {
                    let domain = if *domain == "s" {
                        ClockDomain::Gpu
                    } else {
                        ClockDomain::Memory
                    };
                    let target = clocks.iter_mut().find(|c| c.domain == domain).unwrap();
                    if *index == "0" {
                        target.minimum = Some(value.parse().unwrap());
                    } else {
                        target.maximum = Some(value.parse().unwrap());
                    }
                }
                _ => return Err("Commande OverDrive simulée inconnue".into()),
            }
            let mut raw = String::new();
            for clock in clocks {
                raw.push_str(if clock.domain == ClockDomain::Gpu {
                    "OD_SCLK:\n"
                } else {
                    "OD_MCLK:\n"
                });
                if let Some(v) = clock.minimum {
                    raw.push_str(&format!("0: {v}Mhz\n"));
                }
                if let Some(v) = clock.maximum {
                    raw.push_str(&format!("1: {v}Mhz\n"));
                }
            }
            if let Some((v, _)) = voltage {
                raw.push_str(&format!("OD_VDDGFX_OFFSET:\n{v}mV\n"));
            }
            raw.push_str("OD_RANGE:");
            raw.push_str(old.split_once("OD_RANGE:").unwrap().1);
            self.pending.insert(path.into(), raw);
            return Ok(());
        }
        self.files.insert(path.into(), command.into());
        Ok(())
    }
}
#[test]
fn parses_driver_units_without_inventing_ranges() {
    let c = parse_curve(&curve()).unwrap();
    assert_eq!(c.pwm_percent.unwrap().maximum, 100);
    assert_eq!(c.points.len(), 3);
    let raw = "OD_ACOUSTIC_LIMIT:\n2200";
    assert!(parse_fan(raw, FanSetting::AcousticLimit)
        .unwrap()
        .1
        .is_none());
    assert!(parse_curve(&curve().replace("20%", "51rpm")).is_err());
    let io = Fake::new();
    let c = Catalog::read(&io).unwrap();
    assert_eq!(
        c.scalar(ScalarTarget::VoltageOffset {})
            .unwrap()
            .limits
            .unwrap()
            .minimum,
        -200
    );
}
#[test]
fn validates_every_operation_before_any_write_and_rejects_paths() {
    let io = Fake::new();
    let req = io.request(vec![
        Operation::Scalar {
            target: ScalarTarget::Fan {
                setting: FanSetting::ZeroRpm,
            },
            value: 1,
        },
        Operation::Scalar {
            target: ScalarTarget::Fan {
                setting: FanSetting::AcousticLimit,
            },
            value: 99999,
        },
    ]);
    assert!(prepare(&io, req).is_err());
    assert!(io.writes.is_empty());
    assert!(serde_json::from_str::<Operation>(
        r#"{"operation":"write","path":"/sys/kernel/debug/dri/0/amdgpu_gpu_recover","value":"1"}"#
    )
    .is_err());
    assert!(Sysfs::open("../../etc").is_err());
}
#[test]
fn applies_commits_reads_back_and_restores_pmfw_values() {
    let mut io = Fake::new();
    let req = io.request(vec![
        Operation::Scalar {
            target: ScalarTarget::Fan {
                setting: FanSetting::AcousticLimit,
            },
            value: 2400,
        },
        Operation::Scalar {
            target: ScalarTarget::Fan {
                setting: FanSetting::ZeroRpm,
            },
            value: 1,
        },
    ]);
    let prepared = prepare(&io, req).unwrap();
    let after = execute(&mut io, &prepared).unwrap();
    assert_eq!(
        after
            .scalar(ScalarTarget::Fan {
                setting: FanSetting::ZeroRpm
            })
            .unwrap()
            .current,
        1
    );
    assert!(io.writes.iter().any(|(_, value)| value == "c\n"));
    assert!(restore(&mut io, &prepared).is_empty());
    assert_eq!(
        Catalog::read(&io)
            .unwrap()
            .scalar(ScalarTarget::Fan {
                setting: FanSetting::AcousticLimit
            })
            .unwrap()
            .current,
        2000
    );
}
#[test]
fn restores_partial_failures_and_uses_manual_for_workload_selection() {
    let mut io = Fake::new();
    let prepared = prepare(
        &io,
        io.request(vec![Operation::Scalar {
            target: ScalarTarget::Fan {
                setting: FanSetting::ZeroRpm,
            },
            value: 1,
        }]),
    )
    .unwrap();
    io.fail_commit = true;
    assert!(execute(&mut io, &prepared).is_err());
    assert!(restore(&mut io, &prepared).is_empty());
    let prepared = prepare(&io, io.request(vec![Operation::Workload { index: 1 }])).unwrap();
    execute(&mut io, &prepared).unwrap();
    assert_eq!(io.files["power_dpm_force_performance_level"], "manual");
    assert!(restore(&mut io, &prepared).is_empty());
    assert_eq!(io.files["power_dpm_force_performance_level"], "auto");
}
#[test]
fn avoids_competing_fan_modes_and_enforces_monotonic_curves() {
    let io = Fake::new();
    let mut points = parse_curve(&curve()).unwrap().points;
    points[1].pwm_percent = 10;
    assert!(prepare(&io, io.request(vec![Operation::FanCurve { points }])).is_err());
    let points = parse_curve(&curve()).unwrap().points;
    assert!(prepare(
        &io,
        io.request(vec![
            Operation::FanCurve { points },
            Operation::Scalar {
                target: ScalarTarget::Fan {
                    setting: FanSetting::MinimumPwm
                },
                value: 30
            }
        ])
    )
    .is_err());
}
#[test]
fn changed_identity_or_control_state_is_not_applied() {
    let mut io = Fake::new();
    let prepared = prepare(
        &io,
        io.request(vec![Operation::Performance { mode: "low".into() }]),
    )
    .unwrap();
    io.files
        .insert("power_dpm_force_performance_level".into(), "high".into());
    assert!(execute(&mut io, &prepared).is_err());
    assert!(io.writes.is_empty());
    io.id.device = "0xffff".into();
    assert!(!restore(&mut io, &prepared).is_empty());
    assert!(io.writes.is_empty());
}

#[test]
fn advanced_clock_voltage_and_power_transactions_have_real_readback_and_rollback() {
    let mut io = Fake::new();
    let mut request = io.request(vec![
        Operation::Clocks {
            domain: ClockDomain::Gpu,
            minimum: Some(2600),
            maximum: Some(2800),
        },
        Operation::Scalar {
            target: ScalarTarget::VoltageOffset {},
            value: -50,
        },
        Operation::Scalar {
            target: ScalarTarget::PowerCap { channel: 1 },
            value: 240000000,
        },
    ]);
    assert!(prepare(&io, request.clone()).is_err());
    request.advanced_confirmed = true;
    let prepared = prepare(&io, request).unwrap();
    let after = execute(&mut io, &prepared).unwrap();
    assert_eq!(
        after
            .scalar(ScalarTarget::VoltageOffset {})
            .unwrap()
            .current,
        -50
    );
    let max = io
        .writes
        .iter()
        .position(|(p, v)| p == "pp_od_clk_voltage" && v == "s 1 2800\n")
        .unwrap();
    let min = io
        .writes
        .iter()
        .position(|(p, v)| p == "pp_od_clk_voltage" && v == "s 0 2600\n")
        .unwrap();
    assert!(max < min);
    assert!(restore(&mut io, &prepared).is_empty());
    let original = Catalog::read(&io).unwrap();
    assert_eq!(
        original
            .scalar(ScalarTarget::VoltageOffset {})
            .unwrap()
            .current,
        0
    );
    assert_eq!(original.clocks[0].maximum, Some(2500));
    assert_eq!(original.performance.as_deref(), Some("auto"));
}
#[test]
fn rejects_unknown_units_and_zero_rpm_outside_boolean_domain() {
    assert!(number("500banana").is_err());
    assert!(number("1rpm").is_err());
    let mut io = Fake::new();
    let path = FanSetting::ZeroRpm.path();
    let value = io.files[&path].replace("0 1", "0 3");
    io.files.insert(path, value);
    assert!(prepare(
        &io,
        io.request(vec![Operation::Scalar {
            target: ScalarTarget::Fan {
                setting: FanSetting::ZeroRpm
            },
            value: 2
        }])
    )
    .is_err());
    assert!(io.writes.is_empty());
}

#[test]
fn unit_operations_reject_unknown_fields_instead_of_ignoring_them() {
    assert!(serde_json::from_str::<Operation>(
        r#"{"operation":"fan_automatic","path":"/sys/kernel/debug/dri/0/amdgpu_gpu_recover"}"#
    )
    .is_err());
    assert!(serde_json::from_str::<ScalarTarget>(
        r#"{"kind":"voltage_offset","path":"/etc/passwd"}"#
    )
    .is_err());
}
#[test]
fn sparse_clock_profiles_do_not_restore_unconfigured_bounds() {
    let mut io = Fake::new();
    let mut request = io.request(vec![Operation::Clocks {
        domain: ClockDomain::Gpu,
        minimum: Some(600),
        maximum: None,
    }]);
    request.advanced_confirmed = true;
    let prepared = prepare(&io, request).unwrap();
    execute(&mut io, &prepared).unwrap();
    io.writes.clear();
    assert!(restore(&mut io, &prepared).is_empty());
    assert!(io.writes.iter().any(|(_, v)| v == "s 0 500\n"));
    assert!(!io.writes.iter().any(|(_, v)| v.starts_with("s 1 ")));
}
