//! Crash-safe persistence: atomic writes and corrupt-file recovery.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Toml,
    Json,
}

impl Format {
    fn decode<T: DeserializeOwned>(self, s: &str) -> Result<T, String> {
        match self {
            Format::Toml => toml::from_str(s).map_err(|e| e.to_string()),
            Format::Json => serde_json::from_str(s).map_err(|e| e.to_string()),
        }
    }

    fn encode<T: Serialize>(self, v: &T) -> Result<String, String> {
        match self {
            Format::Toml => toml::to_string_pretty(v).map_err(|e| e.to_string()),
            Format::Json => serde_json::to_string_pretty(v).map_err(|e| e.to_string()),
        }
    }
}

/// Outcome of a load, so callers can log what happened.
#[derive(Debug, PartialEq, Eq)]
pub enum Loaded {
    Ok,
    Missing,
    /// File was unreadable; it was moved to the given backup path.
    Recovered(PathBuf),
}

/// Load `path`, falling back to `T::default()`. A corrupt file is renamed to
/// `<name>.bak` so the user's data is never silently destroyed.
pub fn load<T: DeserializeOwned + Default>(path: &Path, fmt: Format) -> (T, Loaded) {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return (T::default(), Loaded::Missing),
        Err(_) => return (T::default(), backup(path)),
    };
    match fmt.decode(&text) {
        Ok(v) => (v, Loaded::Ok),
        Err(e) => {
            log::warn!("{}: {e}", path.display());
            (T::default(), backup(path))
        }
    }
}

fn backup(path: &Path) -> Loaded {
    let bak = path.with_extension(format!(
        "{}.bak",
        path.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));
    let _ = fs::rename(path, &bak);
    Loaded::Recovered(bak)
}

/// Serialize and write atomically: write `<path>.tmp`, flush, then rename over.
pub fn save<T: Serialize>(path: &Path, fmt: Format, value: &T) -> io::Result<()> {
    let text = fmt.encode(value).map_err(io::Error::other)?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{Data, Settings};

    fn tmp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ailan-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn roundtrip_toml_and_json() {
        let d = tmp_dir("rt");
        let mut s = Settings::default();
        s.reduce_motion = true;
        s.tab = crate::settings::Tab::Todo;
        save(&d.join("settings.toml"), Format::Toml, &s).unwrap();
        let (back, how): (Settings, _) = load(&d.join("settings.toml"), Format::Toml);
        assert_eq!(how, Loaded::Ok);
        assert_eq!(back, s);

        let mut data = Data::default();
        data.todos.add("x".into(), Some(5));
        save(&d.join("data.json"), Format::Json, &data).unwrap();
        // Overwrite works (rename over existing file).
        save(&d.join("data.json"), Format::Json, &data).unwrap();
        let (back, _): (Data, _) = load(&d.join("data.json"), Format::Json);
        assert_eq!(back, data);
        assert!(!d.join("data.tmp").exists());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_and_corrupt() {
        let d = tmp_dir("bad");
        let (_, how): (Data, _) = load(&d.join("nope.json"), Format::Json);
        assert_eq!(how, Loaded::Missing);

        let p = d.join("data.json");
        fs::write(&p, "{ not json").unwrap();
        let (v, how): (Data, _) = load(&p, Format::Json);
        assert_eq!(v, Data::default());
        assert_eq!(how, Loaded::Recovered(d.join("data.json.bak")));
        assert!(!p.exists());
        assert!(d.join("data.json.bak").exists());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn partial_settings_use_defaults() {
        let d = tmp_dir("partial");
        let p = d.join("settings.toml");
        fs::write(&p, "reduce_motion = true\n").unwrap();
        let (s, how): (Settings, _) = load(&p, Format::Toml);
        assert_eq!(how, Loaded::Ok);
        assert!(s.reduce_motion);
        assert_eq!(s.tab, crate::settings::Tab::Music);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn legacy_island_settings_are_ignored_not_fatal() {
        // Files written by older versions carry island settings that no longer exist.
        let d = tmp_dir("legacy");
        let p = d.join("settings.toml");
        let legacy = r#"lang = "vi"
minimal = true
mode = "always_dynamic"
pin_todo = true

[priority]
order = ["music", "focus", "todo"]
smart_minute_peek = false

[minimal_ui]
hide_when_idle = true

[app_window]
last_page = "island"
"#;
        fs::write(&p, legacy).unwrap();
        let (s, how): (Settings, _) = load(&p, Format::Toml);
        assert_eq!(how, Loaded::Ok, "must not be treated as corrupt");
        assert_eq!(s.lang, Some(crate::i18n::Lang::Vi));
        assert!(!d.join("settings.toml.bak").exists());
        let _ = fs::remove_dir_all(&d);
    }
}
