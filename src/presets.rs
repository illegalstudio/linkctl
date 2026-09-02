//! Software framing presets stored in the config file under
//! `[presets.<name>]`, in human units (degrees / zoom multiplier).
//!
//! `preset save` and `preset delete` edit `config.toml` in place with
//! `toml_edit`, so comments and unrelated settings are preserved.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// A saved framing.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    /// Pan in degrees (positive = right).
    pub pan: f64,
    /// Tilt in degrees (positive = up).
    pub tilt: f64,
    /// Zoom multiplier (`1.0` = no zoom).
    pub zoom: f64,
}

impl Preset {
    pub fn validate(&self) -> std::result::Result<(), String> {
        for (name, v) in [("pan", self.pan), ("tilt", self.tilt), ("zoom", self.zoom)] {
            if !v.is_finite() {
                return Err(format!("{name} must be a finite number"));
            }
        }
        if self.zoom <= 0.0 {
            return Err(format!("zoom must be positive (got {})", self.zoom));
        }
        Ok(())
    }
}

/// Validate a preset name: non-empty, printable, no path-like characters, so
/// it round-trips as a bare TOML key and looks sane in listings.
pub fn validate_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(Error::InvalidValue(format!(
            "Invalid preset name '{name}': use letters, digits, '-' or '_' (max 64 chars)."
        )))
    }
}

/// Insert or replace `[presets.<name>]` in the TOML document text.
pub fn upsert_in_document(
    text: &str,
    name: &str,
    preset: &Preset,
) -> std::result::Result<String, String> {
    let mut doc = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| e.to_string())?;
    let presets = doc
        .entry("presets")
        .or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
    let Some(presets) = presets.as_table_mut() else {
        return Err("'presets' exists but is not a table".into());
    };
    // Keep `[presets]` itself implicit so only `[presets.name]` headers show.
    presets.set_implicit(true);
    let mut table = toml_edit::Table::new();
    table.insert("pan", toml_edit::value(preset.pan));
    table.insert("tilt", toml_edit::value(preset.tilt));
    table.insert("zoom", toml_edit::value(preset.zoom));
    presets.insert(name, toml_edit::Item::Table(table));
    Ok(doc.to_string())
}

/// Remove `[presets.<name>]`; returns `Ok(None)` if it did not exist.
pub fn remove_from_document(text: &str, name: &str) -> std::result::Result<Option<String>, String> {
    let mut doc = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| e.to_string())?;
    let Some(presets) = doc.get_mut("presets").and_then(|p| p.as_table_mut()) else {
        return Ok(None);
    };
    if presets.remove(name).is_none() {
        return Ok(None);
    }
    Ok(Some(doc.to_string()))
}

/// Read the config file (empty if missing), apply `edit`, write it back.
pub fn edit_config_file<F>(path: &Path, edit: F) -> Result<()>
where
    F: FnOnce(&str) -> std::result::Result<String, String>,
{
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(Error::Config(format!("{}: {e}", path.display()))),
    };
    let new_text = edit(&text).map_err(|e| Error::Config(format!("{}: {e}", path.display())))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| Error::Config(format!("creating {}: {e}", dir.display())))?;
    }
    // Write to a sibling temp file and rename for atomic replacement.
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, new_text)
        .map_err(|e| Error::Config(format!("writing {}: {e}", tmp.display())))?;
    std::fs::rename(&tmp, path)
        .map_err(|e| Error::Config(format!("replacing {}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(validate_name("desk").is_ok());
        assert!(validate_name("white-board_2").is_ok());
        assert!(validate_name("").is_err());
        assert!(validate_name("a b").is_err());
        assert!(validate_name("../x").is_err());
        assert!(validate_name(&"x".repeat(65)).is_err());
    }

    #[test]
    fn preset_validation() {
        assert!(Preset {
            pan: 0.0,
            tilt: 0.0,
            zoom: 1.0
        }
        .validate()
        .is_ok());
        assert!(Preset {
            pan: f64::NAN,
            tilt: 0.0,
            zoom: 1.0
        }
        .validate()
        .is_err());
        assert!(Preset {
            pan: 0.0,
            tilt: 0.0,
            zoom: 0.0
        }
        .validate()
        .is_err());
    }

    #[test]
    fn upsert_preserves_comments_and_other_keys() {
        let text = "# my config\ndefault_step = 2.5 # fine\n\n[presets.old]\npan = 1.0\ntilt = 2.0\nzoom = 1.0\n";
        let out = upsert_in_document(
            text,
            "desk",
            &Preset {
                pan: -15.0,
                tilt: 4.0,
                zoom: 1.1,
            },
        )
        .unwrap();
        assert!(out.contains("# my config"));
        assert!(out.contains("default_step = 2.5 # fine"));
        assert!(out.contains("[presets.old]"));
        assert!(out.contains("[presets.desk]"));
        assert!(out.contains("pan = -15.0"));
        assert!(out.contains("zoom = 1.1"));
        // Round-trips through the strict config parser.
        let cfg = crate::config::Config::parse(&out).unwrap();
        assert_eq!(cfg.presets["desk"].tilt, 4.0);
        assert_eq!(cfg.default_step, 2.5);
    }

    #[test]
    fn upsert_replaces_existing() {
        let text = "[presets.desk]\npan = 1.0\ntilt = 2.0\nzoom = 1.0\n";
        let out = upsert_in_document(
            text,
            "desk",
            &Preset {
                pan: 9.0,
                tilt: 8.0,
                zoom: 2.0,
            },
        )
        .unwrap();
        let cfg = crate::config::Config::parse(&out).unwrap();
        assert_eq!(cfg.presets.len(), 1);
        assert_eq!(cfg.presets["desk"].pan, 9.0);
    }

    #[test]
    fn upsert_into_empty_document() {
        let out = upsert_in_document(
            "",
            "desk",
            &Preset {
                pan: 0.0,
                tilt: 0.0,
                zoom: 1.0,
            },
        )
        .unwrap();
        let cfg = crate::config::Config::parse(&out).unwrap();
        assert_eq!(cfg.presets["desk"].zoom, 1.0);
        assert!(
            !out.contains("[presets]\n"),
            "presets header should be implicit: {out}"
        );
    }

    #[test]
    fn remove() {
        let text = "default_step = 3.0\n[presets.desk]\npan = 1.0\ntilt = 2.0\nzoom = 1.0\n";
        let out = remove_from_document(text, "desk").unwrap().unwrap();
        assert!(!out.contains("desk"));
        assert!(out.contains("default_step = 3.0"));
        assert_eq!(remove_from_document(text, "nope").unwrap(), None);
        assert_eq!(remove_from_document("", "nope").unwrap(), None);
    }

    #[test]
    fn edit_config_file_creates_and_replaces() {
        let dir = std::env::temp_dir().join(format!("linkctl-cfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("sub").join("config.toml");
        edit_config_file(&path, |t| {
            assert_eq!(t, "");
            Ok("a = 1\n".into())
        })
        .unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "a = 1\n");
        edit_config_file(&path, |t| Ok(format!("{t}b = 2\n"))).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "a = 1\nb = 2\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
