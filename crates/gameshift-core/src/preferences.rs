//! Small versioned user preferences; atomic writes and explicit errors.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    System,
    Light,
    Dark,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Preferences {
    pub schema_version: u32,
    pub interval_millis: u64,
    pub theme: Theme,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            schema_version: 1,
            interval_millis: 1000,
            theme: Theme::Dark,
        }
    }
}
impl Preferences {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 || ![500, 1000, 2000, 5000].contains(&self.interval_millis) {
            return Err("Version ou cadence des préférences non prise en charge".into());
        }
        Ok(())
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.to_string()),
        };
        let mut bytes = Vec::new();
        file.take(65537)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 65536 {
            return Err("Préférences trop volumineuses".into());
        }
        let value: Self = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        value.validate()?;
        Ok(value)
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        // Preserve an invalid or future user file instead of replacing it silently.
        Self::load(path)?;
        let parent = path.parent().ok_or("Chemin de configuration invalide")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let temporary = parent.join(format!(
            ".preferences-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let outcome = (|| {
            let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|e| e.to_string())?;
            file.write_all(&bytes)
                .and_then(|()| file.sync_all())
                .map_err(|e| e.to_string())?;
            fs::rename(&temporary, path).map_err(|e| e.to_string())?;
            fs::File::open(parent)
                .and_then(|f| f.sync_all())
                .map_err(|e| e.to_string())
        })();
        if temporary.exists() {
            let _ = fs::remove_file(temporary);
        }
        outcome
    }
}
pub fn path() -> Result<PathBuf, String> {
    if let Some(root) = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
    {
        if root.is_absolute() {
            return Ok(root.join("gameshift/preferences.json"));
        }
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .map(|p| p.join(".config/gameshift/preferences.json"))
        .ok_or("Répertoire personnel indisponible".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_and_preserve_future_configuration() {
        let root =
            std::env::temp_dir().join(format!("gameshift-preferences-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("preferences.json");
        let preferences = Preferences {
            interval_millis: 5000,
            ..Default::default()
        };
        preferences.save(&path).unwrap();
        assert_eq!(Preferences::load(&path).unwrap(), preferences);
        fs::write(
            &path,
            b"{\"schema_version\":99,\"interval_millis\":1000,\"theme\":\"dark\"}",
        )
        .unwrap();
        let before = fs::read(&path).unwrap();
        assert!(preferences.save(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        fs::remove_dir_all(root).unwrap();
    }
}
