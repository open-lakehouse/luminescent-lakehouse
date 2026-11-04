//! The app's own settings, apart from trestle's shared state (which the CLI shares):
//! a small JSON file in the app's data directory. A missing or unreadable file is
//! the defaults; a write replaces the file whole (write, then rename).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// The file's name in the app's data directory.
pub const SETTINGS_FILE: &str = "settings.json";

/// What quitting the app does to the instances it started.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum QuitPolicy {
    /// They keep running: instances outlive the app, as the CLI's do.
    #[default]
    Keep,
    /// The ones started from the app in this session are stopped (data volumes kept).
    Stop,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub quit: QuitPolicy,
    /// Workspaces whose Start stops their other instances first (side by side
    /// otherwise), by id.
    pub one_at_a_time: BTreeSet<String>,
    /// The instance that was the current environment, restored at launch if it
    /// still runs.
    pub last_instance: Option<String>,
}

/// The settings, kept in memory and written through to `path`.
pub struct SettingsFile {
    path: PathBuf,
    current: Mutex<Settings>,
}

impl SettingsFile {
    /// The settings in `dir` (created on the first write), the defaults if there are
    /// none or they don't parse.
    pub fn load(dir: &Path) -> Self {
        let path = dir.join(SETTINGS_FILE);
        let current = std::fs::read(&path)
            .ok()
            .and_then(|bytes| match serde_json::from_slice(&bytes) {
                Ok(settings) => Some(settings),
                Err(e) => {
                    eprintln!("ignoring {}: {e}", path.display());
                    None
                }
            })
            .unwrap_or_default();
        SettingsFile {
            path,
            current: Mutex::new(current),
        }
    }

    pub fn get(&self) -> Settings {
        self.current.lock().expect("settings lock").clone()
    }

    /// Change the settings and write them; returns what was written.
    pub fn update(&self, change: impl FnOnce(&mut Settings)) -> Result<Settings, String> {
        let mut current = self.current.lock().expect("settings lock");
        let mut next = current.clone();
        change(&mut next);
        if next != *current {
            write(&self.path, &next)
                .map_err(|e| format!("writing {}: {e}", self.path.display()))?;
            *current = next.clone();
        }
        Ok(next)
    }
}

fn write(path: &Path, settings: &Settings) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_vec_pretty(settings).expect("settings serialize");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_without_a_file_and_kept_across_loads() {
        let dir = tempfile::tempdir().unwrap();
        let file = SettingsFile::load(dir.path());
        assert_eq!(file.get(), Settings::default());
        assert_eq!(file.get().quit, QuitPolicy::Keep);

        file.update(|s| {
            s.quit = QuitPolicy::Stop;
            s.one_at_a_time.insert("ws-1".into());
            s.last_instance = Some("inst-1".into());
        })
        .unwrap();
        let again = SettingsFile::load(dir.path()).get();
        assert_eq!(again.quit, QuitPolicy::Stop);
        assert!(again.one_at_a_time.contains("ws-1"));
        assert_eq!(again.last_instance.as_deref(), Some("inst-1"));
    }

    #[test]
    fn an_unreadable_file_is_the_defaults_and_unknown_fields_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(SETTINGS_FILE), "not json").unwrap();
        assert_eq!(SettingsFile::load(dir.path()).get(), Settings::default());

        std::fs::write(
            dir.path().join(SETTINGS_FILE),
            r#"{"quit":"stop","later":true}"#,
        )
        .unwrap();
        assert_eq!(SettingsFile::load(dir.path()).get().quit, QuitPolicy::Stop);
    }
}
