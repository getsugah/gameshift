//! Versioned system-suite plan. Existing CPU/GPU protocols remain independent.
use serde::{Deserialize, Serialize};
pub mod comparison;
pub mod session;
pub mod store;
pub mod system;

pub const PROTOCOL: &str = "aurelia-system-v2";
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Domain {
    Cpu,
    Cache,
    Ram,
    Gpu,
    Combined,
    Glibc,
    Applications,
    Finalization,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase {
    pub domain: Domain,
    pub budget_seconds: u32,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct SuitePlan {
    pub protocol: String,
    pub phases: Vec<Phase>,
}
impl Default for SuitePlan {
    fn default() -> Self {
        Self {
            protocol: PROTOCOL.into(),
            phases: [
                (Domain::Cpu, 21),
                (Domain::Cache, 10),
                (Domain::Ram, 25),
                (Domain::Gpu, 16),
                (Domain::Combined, 16),
                (Domain::Glibc, 17),
                (Domain::Applications, 9),
                (Domain::Finalization, 6),
            ]
            .into_iter()
            .map(|(domain, budget_seconds)| Phase {
                domain,
                budget_seconds,
            })
            .collect(),
        }
    }
}
impl SuitePlan {
    pub fn target_seconds(&self) -> u32 {
        self.phases.iter().map(|p| p.budget_seconds).sum()
    }
}
/// Builds an isolated child command. APPDIR is retained to reject bundled providers.
/// The caller supplies the trusted sibling executable; reports must capture its hash.
pub fn host_command(executable: &std::path::Path) -> std::process::Command {
    use std::os::unix::process::CommandExt;
    let mut command = std::process::Command::new(executable);
    for key in ["LD_LIBRARY_PATH", "LD_PRELOAD", "LD_AUDIT"] {
        command.env_remove(key);
    }
    let parent = std::process::id() as libc::pid_t;
    // Async-signal-safe lifecycle setup only. Also covers abrupt parent exit.
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                return Err(std::io::Error::from_raw_os_error(libc::ECHILD));
            }
            Ok(())
        });
    }
    command
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn budget_is_two_minutes_and_versioned() {
        let plan = SuitePlan::default();
        assert_eq!(plan.target_seconds(), 120);
        let decoded: SuitePlan =
            serde_json::from_str(&serde_json::to_string(&plan).unwrap()).unwrap();
        assert_eq!(decoded.protocol, PROTOCOL);
        assert_eq!(decoded.phases.len(), 8);
    }
    #[test]
    fn child_cannot_inherit_bundle_loader_overrides() {
        let command = host_command(std::path::Path::new("aurelia-host-bench"));
        for key in ["LD_LIBRARY_PATH", "LD_PRELOAD", "LD_AUDIT"] {
            assert!(command
                .get_envs()
                .any(|(name, value)| name == key && value.is_none()));
        }
    }
}
