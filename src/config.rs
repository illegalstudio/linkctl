//! XDG configuration: `$XDG_CONFIG_HOME/linkctl/config.toml`
//! (fallback `~/.config/linkctl/config.toml`).
//!
//! Configuration is optional; every field has a default. A malformed file is
//! a hard error so that typos never silently change behaviour.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::presets::Preset;

/// Default relative movement step in degrees.
pub const DEFAULT_STEP_DEGREES: f64 = 5.0;
/// Default preview player.
pub const DEFAULT_PREVIEW_PLAYER: &str = "ffplay";

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Degrees moved by `left`/`right`/`up`/`down` without an argument.
    #[serde(default = "default_step")]
    pub default_step: f64,
    /// Player used by `linkctl preview` (`ffplay` or `mpv`).
    #[serde(default = "default_player")]
    pub preview_player: String,
    /// Resolution requested by the preview (`WIDTHxHEIGHT`). When unset the
    /// camera's current format (see `linkctl resolution`) is used.
    #[serde(default)]
    pub preview_resolution: Option<String>,
    /// Named framing presets.
    #[serde(default)]
    pub presets: BTreeMap<String, Preset>,
}

fn default_step() -> f64 {
    DEFAULT_STEP_DEGREES
}
fn default_player() -> String {
    DEFAULT_PREVIEW_PLAYER.to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_step: default_step(),
            preview_player: default_player(),
            preview_resolution: None,
            presets: BTreeMap::new(),
        }
    }
}

impl Config {
    /// Resolve the configuration file path from the environment.
    pub fn path() -> Option<PathBuf> {
        config_dir().map(|d| d.join("config.toml"))
    }

    /// Load the configuration, returning defaults when the file is absent.
    pub fn load() -> Result<Self> {
        match Self::path() {
            Some(p) => Self::load_from(&p),
            None => Ok(Self::default()),
        }
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                Self::parse(&text).map_err(|e| Error::Config(format!("{}: {e}", path.display())))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::Config(format!("{}: {e}", path.display()))),
        }
    }

    pub fn parse(text: &str) -> std::result::Result<Self, String> {
        let cfg: Config = toml::from_str(text).map_err(|e| e.to_string())?;
        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> std::result::Result<(), String> {
        if !(self.default_step.is_finite() && self.default_step > 0.0) {
            return Err(format!(
                "default_step must be a positive number of degrees (got {})",
                self.default_step
            ));
        }
        if self.preview_player.trim().is_empty() {
            return Err("preview_player must not be empty".into());
        }
        if let Some(r) = &self.preview_resolution {
            if parse_resolution(r).is_none() {
                return Err(format!(
                    "preview_resolution must look like WIDTHxHEIGHT (got {r:?})"
                ));
            }
        }
        for (name, preset) in &self.presets {
            preset
                .validate()
                .map_err(|e| format!("preset '{name}': {e}"))?;
        }
        Ok(())
    }
}

/// `$XDG_CONFIG_HOME/linkctl` or `~/.config/linkctl`.
pub fn config_dir() -> Option<PathBuf> {
    config_dir_from(
        std::env::var_os("XDG_CONFIG_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
}

fn config_dir_from(
    xdg_config_home: Option<&std::ffi::OsStr>,
    home: Option<&std::ffi::OsStr>,
) -> Option<PathBuf> {
    if let Some(x) = xdg_config_home.filter(|x| !x.is_empty()) {
        let p = PathBuf::from(x);
        if p.is_absolute() {
            return Some(p.join("linkctl"));
        }
    }
    home.filter(|h| !h.is_empty())
        .map(|h| PathBuf::from(h).join(".config").join("linkctl"))
}

/// Parse `WIDTHxHEIGHT`.
pub fn parse_resolution(s: &str) -> Option<(u32, u32)> {
    let (w, h) = s.trim().split_once('x')?;
    let w: u32 = w.parse().ok()?;
    let h: u32 = h.parse().ok()?;
    (w > 0 && h > 0).then_some((w, h))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn defaults_when_empty() {
        let cfg = Config::parse("").unwrap();
        assert_eq!(cfg, Config::default());
        assert_eq!(cfg.default_step, 5.0);
        assert_eq!(cfg.preview_player, "ffplay");
        assert!(cfg.presets.is_empty());
    }

    #[test]
    fn parses_full_config() {
        let cfg = Config::parse(
            r#"
default_step = 2.5
preview_player = "mpv"

[presets.desk]
pan = -15.0
tilt = 4.0
zoom = 1.1

[presets.whiteboard]
pan = 45
tilt = 12
zoom = 1.4
"#,
        )
        .unwrap();
        assert_eq!(cfg.default_step, 2.5);
        assert_eq!(cfg.preview_player, "mpv");
        assert_eq!(cfg.presets.len(), 2);
        let desk = &cfg.presets["desk"];
        assert_eq!(desk.pan, -15.0);
        assert_eq!(desk.tilt, 4.0);
        assert_eq!(desk.zoom, 1.1);
        assert_eq!(cfg.presets["whiteboard"].pan, 45.0);
    }

    #[test]
    fn rejects_malformed_and_unknown() {
        assert!(Config::parse("default_step = ").is_err());
        assert!(Config::parse("default_step = -1").is_err());
        assert!(Config::parse("default_step = 0").is_err());
        assert!(Config::parse("unknown_key = 1").is_err());
        assert!(Config::parse("preview_resolution = \"big\"").is_err());
        assert!(Config::parse("[presets.x]\npan = 1.0\ntilt = 2.0\nzoom = 0.0").is_err());
        assert!(Config::parse("[presets.x]\npan = 1.0").is_err());
    }

    #[test]
    fn xdg_resolution() {
        assert_eq!(
            config_dir_from(Some(OsStr::new("/xdg")), Some(OsStr::new("/home/u"))),
            Some(PathBuf::from("/xdg/linkctl"))
        );
        // Relative XDG_CONFIG_HOME must be ignored per the spec.
        assert_eq!(
            config_dir_from(Some(OsStr::new("rel")), Some(OsStr::new("/home/u"))),
            Some(PathBuf::from("/home/u/.config/linkctl"))
        );
        assert_eq!(
            config_dir_from(None, Some(OsStr::new("/home/u"))),
            Some(PathBuf::from("/home/u/.config/linkctl"))
        );
        assert_eq!(config_dir_from(None, None), None);
    }

    #[test]
    fn resolution_parsing() {
        assert_eq!(parse_resolution("1280x720"), Some((1280, 720)));
        assert_eq!(parse_resolution("1920x1080\n"), Some((1920, 1080)));
        assert_eq!(parse_resolution("0x720"), None);
        assert_eq!(parse_resolution("720p"), None);
    }

    #[test]
    fn missing_file_yields_defaults() {
        let cfg = Config::load_from(Path::new("/nonexistent/linkctl/config.toml")).unwrap();
        assert_eq!(cfg, Config::default());
    }
}
