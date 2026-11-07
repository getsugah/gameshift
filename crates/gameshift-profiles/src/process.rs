use crate::*;
#[derive(Debug, Clone, Default)]
pub struct Process {
    pub pid: u32,
    pub executable: String,
    pub arguments: Vec<String>,
    pub steam_app_id: Option<u32>,
}
impl Matcher {
    pub fn matches(&self, process: &Process) -> bool {
        match self {
            Self::Native { path } => path == &process.executable,
            Self::WindowsExecutable {
                basename,
                steam_app_id,
            } => {
                let exe = Path::new(&process.executable)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let wine = exe.contains("wine") || exe.ends_with(".exe");
                wine && steam_app_id.is_none_or(|id| process.steam_app_id == Some(id))
                    && process.arguments.iter().any(|arg| {
                        arg.rsplit(['\\', '/'])
                            .next()
                            .is_some_and(|s| s.eq_ignore_ascii_case(basename))
                    })
            }
        }
    }
}
pub fn scan(config: &Config) -> Vec<Process> {
    let need_windows = config
        .rules
        .iter()
        .any(|r| r.enabled && matches!(r.matcher, Matcher::WindowsExecutable { .. }));
    let need_steam = config.rules.iter().any(|r| {
        r.enabled
            && matches!(
                r.matcher,
                Matcher::WindowsExecutable {
                    steam_app_id: Some(_),
                    ..
                }
            )
    });
    let uid = unsafe { libc::getuid() };
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return out;
    };
    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        if !entry.metadata().is_ok_and(|m| m.uid() == uid) {
            continue;
        }
        let Ok(executable) = fs::read_link(entry.path().join("exe")) else {
            continue;
        };
        let executable = executable
            .to_string_lossy()
            .trim_end_matches(" (deleted)")
            .to_owned();
        let mut process = Process {
            pid,
            executable,
            ..Process::default()
        };
        if need_windows {
            if let Ok(mut file) = fs::File::open(entry.path().join("cmdline")) {
                let mut data = Vec::new();
                let _ = (&mut file).take(65536).read_to_end(&mut data);
                process.arguments = data
                    .split(|b| *b == 0)
                    .filter_map(|s| std::str::from_utf8(s).ok())
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect();
            }
        }
        if need_steam {
            if let Ok(file) = fs::File::open(entry.path().join("environ")) {
                let mut data = Vec::new();
                let _ = file.take(65536).read_to_end(&mut data);
                process.steam_app_id = data.split(|b| *b == 0).find_map(|s| {
                    std::str::from_utf8(s)
                        .ok()?
                        .strip_prefix("SteamAppId=")?
                        .parse::<u32>()
                        .ok()
                });
            }
        }
        out.push(process);
    }
    out
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub profile: Option<String>,
    pub reason: String,
}
pub fn select(config: &Config, processes: &[Process]) -> Selection {
    if let Mode::Manual { profile } = &config.mode {
        return Selection {
            profile: Some(profile.clone()),
            reason: "Sélection manuelle".into(),
        };
    }
    if matches!(config.mode, Mode::Driver {}) {
        return Selection {
            profile: None,
            reason: "Pilote automatique · sélection manuelle".into(),
        };
    }
    let mut candidates = config
        .rules
        .iter()
        .filter(|r| r.enabled)
        .filter_map(|r| {
            processes
                .iter()
                .filter(|p| r.matcher.matches(p))
                .min_by_key(|p| p.pid)
                .map(|p| (r, p))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|(a, _), (b, _)| b.priority.cmp(&a.priority).then_with(|| a.id.cmp(&b.id)));
    if let Some((rule, process)) = candidates.first() {
        return Selection {
            profile: Some(rule.profile.clone()),
            reason: format!(
                "{} · PID {} · priorité {}",
                Path::new(&process.executable)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy(),
                process.pid,
                rule.priority
            ),
        };
    }
    Selection {
        profile: config.default_profile.clone(),
        reason: if config.default_profile.is_some() {
            "Profil par défaut"
        } else {
            "Réglages du pilote · aucune règle active"
        }
        .into(),
    }
}
/// Stable candidate for 3s, minimum 5s between automatic profile changes. Manual is immediate.
#[derive(Default)]
pub struct Debounce {
    candidate: Option<Option<String>>,
    since: u64,
    last_change: Option<u64>,
}
impl Debounce {
    pub fn ready(&mut self, wanted: &Option<String>, now: u64, manual: bool) -> bool {
        if self.candidate.as_ref() != Some(wanted) {
            self.candidate = Some(wanted.clone());
            self.since = now;
        }
        manual
            || (now.saturating_sub(self.since) >= 3
                && self
                    .last_change
                    .is_none_or(|last| now.saturating_sub(last) >= 5))
    }
    pub fn changed(&mut self, now: u64) {
        self.last_change = Some(now);
    }
}

pub fn select_for_benchmark(
    config: &Config,
    processes: &[Process],
    current: Option<String>,
    held: bool,
) -> Selection {
    if held && matches!(config.mode, Mode::Automatic {}) {
        Selection {
            profile: current,
            reason: "Profil maintenu pendant Aurelia".into(),
        }
    } else {
        select(config, processes)
    }
}
