use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prepared {
    pub request: Request,
    pub before: Catalog,
}
fn scalar_check(catalog: &Catalog, target: ScalarTarget, value: i64) -> Result<()> {
    if matches!(
        target,
        ScalarTarget::Fan {
            setting: FanSetting::ZeroRpm
        }
    ) && ![0, 1].contains(&value)
    {
        return Err("Zero RPM attend 0 ou 1".into());
    }
    if !matches!(target, ScalarTarget::VoltageOffset {}) && value < 0 {
        return Err("Valeur négative refusée pour ce réglage".into());
    }
    let scalar = catalog.scalar(target)?;
    let range = scalar
        .limits
        .ok_or("Plage non exposée : réglage non modifiable")?;
    if !range.accepts(scalar.current) {
        return Err(format!(
            "{} : valeur initiale hors plage, restauration non maîtrisée",
            scalar.label
        ));
    }
    if !range.accepts(value) {
        return Err(format!(
            "{} : {value} hors de {}–{} {}",
            scalar.label, range.minimum, range.maximum, scalar.unit
        ));
    }
    Ok(())
}
fn clock_check(
    catalog: &Catalog,
    domain: ClockDomain,
    minimum: Option<i64>,
    maximum: Option<i64>,
) -> Result<()> {
    let clock = catalog
        .clocks
        .iter()
        .find(|c| c.domain == domain)
        .ok_or("Domaine d’horloge non exposé")?;
    let limits = clock.limits.ok_or("Plage d’horloge non exposée")?;
    if minimum.is_none() && maximum.is_none() {
        return Err("Aucune fréquence demandée".into());
    }
    if minimum.is_some() && clock.minimum.is_none() || maximum.is_some() && clock.maximum.is_none()
    {
        return Err("Borne de fréquence non modifiable dans ce format".into());
    }
    if minimum
        .into_iter()
        .chain(maximum)
        .any(|v| !limits.accepts(v))
    {
        return Err("Fréquence hors de la plage du pilote".into());
    }
    if minimum
        .or(clock.minimum)
        .zip(maximum.or(clock.maximum))
        .is_some_and(|(a, b)| a > b)
    {
        return Err("Fréquence minimale supérieure à la maximale".into());
    }
    Ok(())
}
fn needs_manual(operations: &[Operation]) -> bool {
    operations.iter().any(|o| {
        matches!(
            o,
            Operation::Clocks { .. }
                | Operation::Workload { .. }
                | Operation::Scalar {
                    target: ScalarTarget::VoltageOffset {},
                    ..
                }
        )
    })
}
fn fan_mode_changed(operations: &[Operation]) -> bool {
    operations.iter().any(|o| match o {
        Operation::FanCurve { .. } | Operation::FanAutomatic {} => true,
        Operation::Scalar {
            target: ScalarTarget::Fan { setting },
            ..
        } => setting.changes_mode(),
        _ => false,
    })
}
fn validate(catalog: &Catalog, request: &Request) -> Result<()> {
    if request.schema_version != 1
        || !request.identity.matches_observation(&catalog.identity)
        || request.operations.is_empty()
        || request.operations.len() > 32
    {
        return Err("Requête ou identité incompatible".into());
    }
    if request.operations.iter().any(Operation::advanced) && !request.advanced_confirmed {
        return Err("Confirmation des réglages avancés requise".into());
    }
    let manual = needs_manual(&request.operations);
    if manual
        && !catalog
            .performance
            .as_ref()
            .is_some_and(|mode| PERFORMANCE_MODES.contains(&mode.as_str()))
    {
        return Err("Mode DPM initial non reconnu : restauration impossible".into());
    }
    let custom = request
        .operations
        .iter()
        .any(|o| matches!(o, Operation::FanCurve { .. }));
    let automatic=request.operations.iter().any(|o|matches!(o,Operation::FanAutomatic {})|matches!(o,Operation::Scalar{target:ScalarTarget::Fan{setting},..} if setting.changes_mode()));
    if custom && automatic {
        return Err("Courbe personnalisée et réglages acoustiques/thermiques automatiques ne doivent pas être appliqués ensemble".into());
    }
    if fan_mode_changed(&request.operations) {
        let curve = catalog
            .curve
            .as_ref()
            .ok_or("État de ventilation initial non lisible")?;
        if request.fan_recovery == FanRecovery::InitialCurve {
            curve.validate(&curve.points).map_err(|_|"La courbe initiale ne peut pas être réappliquée ; choisir la récupération automatique")?;
        }
    }
    let mut keys = BTreeSet::new();
    for operation in &request.operations {
        let key = match operation {
            Operation::Scalar { target, value } => {
                scalar_check(catalog, *target, *value)?;
                format!("scalar:{target:?}")
            }
            Operation::FanCurve { points } => {
                catalog
                    .curve
                    .as_ref()
                    .ok_or("Courbe indisponible")?
                    .validate(points)?;
                "fan_mode".into()
            }
            Operation::FanAutomatic {} => {
                if catalog.curve.is_none() {
                    return Err("Courbe PMFW indisponible".into());
                }
                "fan_mode".into()
            }
            Operation::Performance { mode } => {
                if !PERFORMANCE_MODES.contains(&mode.as_str())
                    || catalog
                        .performance
                        .as_ref()
                        .is_none_or(|v| !PERFORMANCE_MODES.contains(&v.as_str()))
                {
                    return Err("Mode DPM non reconnu".into());
                }
                if manual && mode != "manual" {
                    return Err(
                        "OverDrive et profil de charge nécessitent le mode DPM manuel".into(),
                    );
                }
                "performance".into()
            }
            Operation::Workload { index } => {
                if !catalog.workloads.iter().any(|p| p.index == *index)
                    || catalog.workloads.iter().filter(|p| p.active).count() != 1
                {
                    return Err("Profil de charge demandé ou initial non reconnu".into());
                }
                "workload".into()
            }
            Operation::Clocks {
                domain,
                minimum,
                maximum,
            } => {
                clock_check(catalog, *domain, *minimum, *maximum)?;
                format!("clock:{domain:?}")
            }
        };
        if !keys.insert(key) {
            return Err("Plusieurs opérations sur le même réglage".into());
        }
    }
    Ok(())
}
pub fn prepare(io: &impl DeviceIo, request: Request) -> Result<Prepared> {
    let before = Catalog::read(io)?;
    validate(&before, &request)?;
    Ok(Prepared { request, before })
}
fn command(io: &mut impl DeviceIo, path: &str, value: &str, commit: bool) -> Result<()> {
    io.write(path, &format!("{value}\n"))?;
    if commit {
        io.write(path, "c\n")?;
    }
    Ok(())
}
fn apply_op(io: &mut impl DeviceIo, operation: &Operation) -> Result<()> {
    let catalog = Catalog::read(io)?;
    match operation {
        Operation::Scalar { target, value } => {
            scalar_check(&catalog, *target, *value)?;
            let scalar = catalog.scalar(*target)?;
            match target {
                ScalarTarget::Fan { .. } => command(io, &scalar.source, &value.to_string(), true),
                ScalarTarget::PowerCap { .. } => {
                    command(io, &scalar.source, &value.to_string(), false)
                }
                ScalarTarget::VoltageOffset {} => {
                    command(io, "pp_od_clk_voltage", &format!("vo {value}"), true)
                }
            }
        }
        Operation::FanCurve { points } => {
            catalog
                .curve
                .as_ref()
                .ok_or("Courbe indisponible")?
                .validate(points)?;
            let path = "gpu_od/fan_ctrl/fan_curve";
            for (index, p) in points.iter().enumerate() {
                command(
                    io,
                    path,
                    &format!("{index} {} {}", p.temperature, p.pwm_percent),
                    false,
                )?;
            }
            command(io, path, "c", false)
        }
        Operation::FanAutomatic {} => command(io, "gpu_od/fan_ctrl/fan_curve", "r", false),
        Operation::Performance { mode } => {
            if !PERFORMANCE_MODES.contains(&mode.as_str()) {
                return Err("Mode DPM refusé".into());
            }
            command(io, "power_dpm_force_performance_level", mode, false)
        }
        Operation::Workload { index } => {
            if !catalog.workloads.iter().any(|p| p.index == *index) {
                return Err("Profil de charge disparu".into());
            }
            command(io, "pp_power_profile_mode", &index.to_string(), false)
        }
        Operation::Clocks {
            domain,
            minimum,
            maximum,
        } => {
            clock_check(&catalog, *domain, *minimum, *maximum)?;
            let previous = catalog.clocks.iter().find(|c| c.domain == *domain).unwrap();
            let symbol = if *domain == ClockDomain::Gpu {
                's'
            } else {
                'm'
            };
            let mut commands = Vec::new();
            let max_first = minimum
                .zip(previous.maximum)
                .is_some_and(|(new, old)| new > old);
            if max_first {
                if let Some(max) = maximum {
                    commands.push(format!("{symbol} 1 {max}"));
                }
            }
            if let Some(min) = minimum {
                commands.push(format!("{symbol} 0 {min}"));
            }
            if !max_first {
                if let Some(max) = maximum {
                    commands.push(format!("{symbol} 1 {max}"));
                }
            }
            for value in commands {
                command(io, "pp_od_clk_voltage", &value, false)?;
            }
            command(io, "pp_od_clk_voltage", "c", false)
        }
    }
}
fn verify(catalog: &Catalog, operation: &Operation) -> Result<()> {
    let accepted = match operation {
        Operation::Scalar { target, value } => catalog.scalar(*target)?.current == *value,
        Operation::FanCurve { points } => {
            catalog.curve.as_ref().is_some_and(|c| c.points == *points)
        }
        Operation::FanAutomatic {} => catalog.curve.is_some(), // Firmware mode has no reliable readback field; receipt states this limitation.
        Operation::Performance { mode } => catalog.performance.as_ref() == Some(mode),
        Operation::Workload { index } => catalog
            .workloads
            .iter()
            .any(|p| p.index == *index && p.active),
        Operation::Clocks {
            domain,
            minimum,
            maximum,
        } => catalog.clocks.iter().any(|c| {
            c.domain == *domain
                && minimum.is_none_or(|v| c.minimum == Some(v))
                && maximum.is_none_or(|v| c.maximum == Some(v))
        }),
    };
    if !accepted {
        return Err(format!(
            "Le pilote n’a pas confirmé le réglage : {operation:?}"
        ));
    }
    Ok(())
}
pub fn execute(io: &mut impl DeviceIo, prepared: &Prepared) -> Result<Catalog> {
    let current = Catalog::read(io)?;
    validate(&current, &prepared.request)?;
    // Compare control state, not clocks/utilisation telemetry. Another writer requires a refresh.
    if serde_json::to_value(&current).map_err(|e| e.to_string())?
        != serde_json::to_value(&prepared.before).map_err(|e| e.to_string())?
    {
        return Err(
            "État du pilote modifié depuis la préparation ; actualiser avant application".into(),
        );
    }
    if needs_manual(&prepared.request.operations) {
        command(io, "power_dpm_force_performance_level", "manual", false)?;
    }
    for operation in &prepared.request.operations {
        apply_op(io, operation)?;
    }
    let after = Catalog::read(io)?;
    for operation in &prepared.request.operations {
        verify(&after, operation)?;
    }
    if needs_manual(&prepared.request.operations) && after.performance.as_deref() != Some("manual")
    {
        return Err("Mode DPM manuel non confirmé".into());
    }
    Ok(after)
}
pub fn restore(io: &mut impl DeviceIo, prepared: &Prepared) -> Vec<String> {
    let before = &prepared.before;
    let mut errors = Vec::new();
    if io.identity().ok().as_ref() != Some(&before.identity) {
        return vec![
            "Identité GPU différente : aucune restauration sur un autre périphérique".into(),
        ];
    }
    let manual = needs_manual(&prepared.request.operations);
    if manual {
        if let Err(error) = command(io, "power_dpm_force_performance_level", "manual", false) {
            errors.push(error);
        }
    }
    let changed_fan = fan_mode_changed(&prepared.request.operations);
    if changed_fan {
        if let Err(error) = command(io, "gpu_od/fan_ctrl/fan_curve", "r", false) {
            errors.push(error);
        }
    }
    let mut operations = Vec::new();
    for operation in prepared.request.operations.iter().rev() {
        match operation {
            Operation::Scalar { target, .. } => match before.scalar(*target) {
                Ok(s) => operations.push(Operation::Scalar {
                    target: *target,
                    value: s.current,
                }),
                Err(e) => errors.push(e),
            },
            Operation::Workload { .. } => {
                if let Some(p) = before.workloads.iter().find(|p| p.active) {
                    operations.push(Operation::Workload { index: p.index });
                }
            }
            Operation::Clocks {
                domain,
                minimum,
                maximum,
            } => {
                if let Some(c) = before.clocks.iter().find(|c| c.domain == *domain) {
                    operations.push(Operation::Clocks {
                        domain: *domain,
                        minimum: minimum.and(c.minimum),
                        maximum: maximum.and(c.maximum),
                    });
                }
            }
            _ => (),
        }
    }
    if changed_fan && prepared.request.fan_recovery == FanRecovery::InitialCurve {
        if let Some(curve) = &before.curve {
            operations.push(Operation::FanCurve {
                points: curve.points.clone(),
            });
        }
    }
    for operation in operations {
        match apply_op(io, &operation)
            .and_then(|()| Catalog::read(io))
            .and_then(|c| verify(&c, &operation))
        {
            Ok(()) => (),
            Err(error) => errors.push(error),
        }
    }
    if manual
        || prepared
            .request
            .operations
            .iter()
            .any(|o| matches!(o, Operation::Performance { .. }))
    {
        if let Some(mode) = &before.performance {
            let operation = Operation::Performance { mode: mode.clone() };
            if let Err(error) = apply_op(io, &operation)
                .and_then(|()| Catalog::read(io))
                .and_then(|c| verify(&c, &operation))
            {
                errors.push(error);
            }
        }
    }
    errors
}

/// Frontend validation for usability; the privileged helper repeats it from fresh sysfs data.
pub fn check_request(catalog: &Catalog, request: &Request) -> Result<()> {
    validate(catalog, request)
}

/// Verify only configured settings, without comparing live telemetry or unspecified controls.
pub fn verify_applied(io: &impl DeviceIo, request: &Request) -> Result<()> {
    let catalog = Catalog::read(io)?;
    if !request.identity.matches_observation(&catalog.identity) {
        return Err("Identité GPU modifiée".into());
    }
    for operation in &request.operations {
        verify(&catalog, operation)?;
    }
    if needs_manual(&request.operations) && catalog.performance.as_deref() != Some("manual") {
        return Err("Mode DPM manuel perdu".into());
    }
    Ok(())
}
