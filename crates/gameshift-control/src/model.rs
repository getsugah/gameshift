//! Independent parsers for documented AMDGPU interfaces, not LACT code.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub type Result<T> = std::result::Result<T, String>;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub pci: String,
    pub vendor: String,
    pub device: String,
    pub revision: String,
    pub subsystem_vendor: String,
    pub subsystem_device: String,
    pub unique_id: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    pub minimum: i64,
    pub maximum: i64,
}
impl Limits {
    pub fn accepts(self, value: i64) -> bool {
        self.minimum <= value && value <= self.maximum
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FanSetting {
    AcousticLimit,
    AcousticTarget,
    TargetTemperature,
    MinimumPwm,
    ZeroRpm,
    ZeroRpmStopTemperature,
}
impl FanSetting {
    pub const ALL: [Self; 6] = [
        Self::AcousticLimit,
        Self::AcousticTarget,
        Self::TargetTemperature,
        Self::MinimumPwm,
        Self::ZeroRpm,
        Self::ZeroRpmStopTemperature,
    ];
    pub fn definition(
        self,
    ) -> (
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
    ) {
        match self {
            Self::AcousticLimit => (
                "acoustic_limit_rpm_threshold",
                "OD_ACOUSTIC_LIMIT",
                "ACOUSTIC_LIMIT",
                "Limite acoustique",
                "tr/min",
            ),
            Self::AcousticTarget => (
                "acoustic_target_rpm_threshold",
                "OD_ACOUSTIC_TARGET",
                "ACOUSTIC_TARGET",
                "Cible acoustique",
                "tr/min",
            ),
            Self::TargetTemperature => (
                "fan_target_temperature",
                "FAN_TARGET_TEMPERATURE",
                "TARGET_TEMPERATURE",
                "Température cible",
                "°C",
            ),
            Self::MinimumPwm => (
                "fan_minimum_pwm",
                "FAN_MINIMUM_PWM",
                "MINIMUM_PWM",
                "PWM minimal",
                "%",
            ),
            Self::ZeroRpm => (
                "fan_zero_rpm_enable",
                "FAN_ZERO_RPM_ENABLE",
                "ZERO_RPM_ENABLE",
                "Zero RPM",
                "0 / 1",
            ),
            Self::ZeroRpmStopTemperature => (
                "fan_zero_rpm_stop_temperature",
                "FAN_ZERO_RPM_STOP_TEMPERATURE",
                "ZERO_RPM_STOP_TEMPERATURE",
                "Seuil d’arrêt Zero RPM",
                "°C",
            ),
        }
    }
    pub fn path(self) -> String {
        format!("gpu_od/fan_ctrl/{}", self.definition().0)
    }
    pub fn changes_mode(self) -> bool {
        !matches!(self, Self::ZeroRpm | Self::ZeroRpmStopTemperature)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScalarTarget {
    Fan { setting: FanSetting },
    PowerCap { channel: u32 },
    VoltageOffset {},
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockDomain {
    Gpu,
    Memory,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FanRecovery {
    Automatic,
    InitialCurve,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub temperature: i64,
    pub pwm_percent: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Curve {
    pub points: Vec<Point>,
    pub temperature: Option<Limits>,
    pub pwm_percent: Option<Limits>,
}
impl Curve {
    pub fn validate(&self, points: &[Point]) -> Result<()> {
        let temperature = self
            .temperature
            .ok_or("Plage de température de courbe absente")?;
        let pwm = self.pwm_percent.ok_or("Plage PWM de courbe absente")?;
        if points.len() != self.points.len() || points.is_empty() {
            return Err("Le nombre de points doit rester identique à celui du pilote".into());
        }
        if points
            .iter()
            .any(|p| !temperature.accepts(p.temperature) || !pwm.accepts(p.pwm_percent))
        {
            return Err("Point de courbe hors des limites du pilote".into());
        }
        if points
            .windows(2)
            .any(|p| p[0].temperature > p[1].temperature || p[0].pwm_percent > p[1].pwm_percent)
        {
            return Err("Températures et PWM doivent être non décroissants".into());
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scalar {
    pub target: ScalarTarget,
    pub label: String,
    pub unit: String,
    pub source: String,
    pub current: i64,
    pub limits: Option<Limits>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clock {
    pub domain: ClockDomain,
    pub minimum: Option<i64>,
    pub maximum: Option<i64>,
    pub limits: Option<Limits>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workload {
    pub index: u32,
    pub name: String,
    pub active: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    pub identity: Identity,
    pub scalars: Vec<Scalar>,
    pub curve: Option<Curve>,
    pub performance: Option<String>,
    pub workloads: Vec<Workload>,
    pub clocks: Vec<Clock>,
    pub unavailable: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Scalar {
        target: ScalarTarget,
        value: i64,
    },
    FanCurve {
        points: Vec<Point>,
    },
    FanAutomatic {},
    Performance {
        mode: String,
    },
    Workload {
        index: u32,
    },
    Clocks {
        domain: ClockDomain,
        minimum: Option<i64>,
        maximum: Option<i64>,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub advanced_confirmed: bool,
    pub identity: Identity,
    pub fan_recovery: FanRecovery,
    pub operations: Vec<Operation>,
}
pub const PERFORMANCE_MODES: [&str; 8] = [
    "auto",
    "low",
    "high",
    "manual",
    "profile_standard",
    "profile_min_sclk",
    "profile_min_mclk",
    "profile_peak",
];
pub fn number(text: &str) -> Result<i64> {
    let value = text.trim().to_ascii_lowercase();
    let value = ["mhz", "mv", "c", "%"]
        .into_iter()
        .find_map(|unit| value.strip_suffix(unit))
        .unwrap_or(&value);
    value
        .parse()
        .map_err(|_| format!("Valeur ou unité non reconnue : {text}"))
}

fn sections(raw: &str) -> Result<(Vec<&str>, BTreeMap<String, Limits>)> {
    let (content, ranges) = raw.split_once("OD_RANGE:").unwrap_or((raw, ""));
    let mut result = BTreeMap::new();
    for line in ranges.lines().filter(|l| !l.trim().is_empty()) {
        let Some((key, value)) = line.split_once(':') else {
            return Err("Ligne OD_RANGE invalide".into());
        };
        let values = value.split_whitespace().collect::<Vec<_>>();
        if values.len() != 2 {
            return Err("Plage OD_RANGE non reconnue".into());
        }
        let limits = Limits {
            minimum: number(values[0])?,
            maximum: number(values[1])?,
        };
        if limits.minimum > limits.maximum {
            return Err("Plage OD_RANGE inversée".into());
        }
        result.insert(key.trim().into(), limits);
    }
    Ok((
        content
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(str::trim)
            .collect(),
        result,
    ))
}
pub fn parse_fan(raw: &str, setting: FanSetting) -> Result<(i64, Option<Limits>)> {
    let (_, section, key, _, _) = setting.definition();
    let (lines, ranges) = sections(raw)?;
    if lines.len() != 2 || lines[0] != format!("{section}:") {
        return Err(format!("Format PMFW inattendu pour {section}"));
    }
    let value = number(lines[1])?;
    if value < 0 {
        return Err("Valeur PMFW négative".into());
    }
    if setting == FanSetting::ZeroRpm && ![0, 1].contains(&value) {
        return Err("État Zero RPM non reconnu".into());
    }
    Ok((value, ranges.get(key).copied()))
}
pub fn parse_curve(raw: &str) -> Result<Curve> {
    let (lines, ranges) = sections(raw)?;
    if lines.first() != Some(&"OD_FAN_CURVE:") {
        return Err("Courbe PMFW non reconnue".into());
    }
    let mut points = Vec::new();
    for line in &lines[1..] {
        let (index, values) = line.split_once(':').ok_or("Index de courbe absent")?;
        if index.parse::<usize>().ok() != Some(points.len()) {
            return Err("Indices de courbe non contigus".into());
        }
        let pair = values.split_whitespace().collect::<Vec<_>>();
        if pair.len() != 2 || !pair[0].ends_with('C') || !pair[1].ends_with('%') {
            return Err("Unités de courbe attendues : °C et % PWM".into());
        }
        points.push(Point {
            temperature: number(pair[0])?,
            pwm_percent: number(pair[1])?,
        });
    }
    if points.is_empty() || points.len() > 32 {
        return Err("Nombre de points de courbe non pris en charge".into());
    }
    Ok(Curve {
        points,
        temperature: ranges.get("FAN_CURVE(hotspot temp)").copied(),
        pwm_percent: ranges.get("FAN_CURVE(fan speed)").copied(),
    })
}
pub type OverdriveTable = (Vec<Clock>, Option<(i64, Limits)>);
pub fn parse_overdrive(raw: &str) -> Result<OverdriveTable> {
    let (lines, ranges) = sections(raw)?;
    let mut section = "";
    let mut gpu = BTreeMap::new();
    let mut memory = BTreeMap::new();
    let mut voltage = None;
    for line in lines {
        if line.ends_with(':') {
            section = line.trim_end_matches(':');
            continue;
        }
        match section {
            "OD_SCLK" | "OD_MCLK" => {
                let (index, value) = line
                    .split_once(':')
                    .ok_or("Horloge OverDrive non reconnue")?;
                let index = index.parse::<u32>().map_err(|e| e.to_string())?;
                if index > 1
                    || value.split_whitespace().count() != 1
                    || !value.trim().to_ascii_lowercase().ends_with("mhz")
                {
                    return Err("Seul le format OverDrive min/max RDNA est pris en charge".into());
                }
                if section == "OD_SCLK" {
                    gpu.insert(index, number(value)?);
                } else {
                    memory.insert(index, number(value)?);
                }
            }
            "OD_VDDGFX_OFFSET" => {
                voltage = Some(number(line)?);
            }
            _ => (),
        }
    }
    let clocks = [
        (ClockDomain::Gpu, gpu, "SCLK"),
        (ClockDomain::Memory, memory, "MCLK"),
    ]
    .into_iter()
    .filter_map(|(domain, values, key)| {
        (!values.is_empty()).then(|| Clock {
            domain,
            minimum: values.get(&0).copied(),
            maximum: values.get(&1).copied(),
            limits: ranges.get(key).copied(),
        })
    })
    .collect();
    Ok((clocks, voltage.zip(ranges.get("VDDGFX_OFFSET").copied())))
}
pub fn parse_workloads(raw: &str) -> Vec<Workload> {
    raw.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let index = words.next()?.parse::<u32>().ok()?;
            let name = words
                .next()?
                .trim_end_matches(':')
                .trim_end_matches('*')
                .to_owned();
            let active = line.contains('*');
            Some(Workload {
                index,
                name,
                active,
            })
        })
        .collect()
}

impl Operation {
    pub fn advanced(&self) -> bool {
        matches!(
            self,
            Self::Clocks { .. }
                | Self::Scalar {
                    target: ScalarTarget::VoltageOffset {},
                    ..
                }
        ) || matches!(self,Self::Performance{mode} if mode.starts_with("profile_"))
    }
}

impl Identity {
    pub fn matches_observation(&self, actual: &Self) -> bool {
        self.pci == actual.pci
            && self.vendor == actual.vendor
            && self.device == actual.device
            && self.revision == actual.revision
            && self.subsystem_vendor == actual.subsystem_vendor
            && self.subsystem_device == actual.subsystem_device
            && self
                .unique_id
                .as_ref()
                .is_none_or(|id| actual.unique_id.as_ref() == Some(id))
    }
}
