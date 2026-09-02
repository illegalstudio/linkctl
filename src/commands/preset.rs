use serde::Serialize;

use super::Context;
use crate::cli::PresetAction;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::presets::{self, Preset};
use crate::units::{format_degrees, format_zoom};

#[derive(Serialize)]
struct PresetJson<'a> {
    name: &'a str,
    #[serde(flatten)]
    preset: Preset,
}

fn config_path() -> Result<std::path::PathBuf> {
    Config::path().ok_or_else(|| {
        Error::Config("cannot determine the configuration directory (HOME is unset)".into())
    })
}

fn render(name: &str, p: &Preset) -> String {
    format!(
        "{name}: pan {}, tilt {}, zoom {}",
        format_degrees(p.pan),
        format_degrees(p.tilt),
        format_zoom(p.zoom)
    )
}

pub fn run(ctx: &Context, action: PresetAction) -> Result<()> {
    match action {
        PresetAction::List => {
            let list: Vec<PresetJson> = ctx
                .config
                .presets
                .iter()
                .map(|(name, preset)| PresetJson {
                    name,
                    preset: *preset,
                })
                .collect();
            ctx.out.emit(
                || {
                    if list.is_empty() {
                        "No presets saved.\n\nSave one with:\n  linkctl preset save NAME"
                            .to_string()
                    } else {
                        list.iter()
                            .map(|p| render(p.name, &p.preset))
                            .collect::<Vec<_>>()
                            .join("\n")
                    }
                },
                &list,
            );
            Ok(())
        }
        PresetAction::Save { name } => {
            presets::validate_name(&name)?;
            // Read-only: allowed while inactive. Values may reflect the last
            // commanded position rather than the parked one; see README.
            let cam = ctx.open_camera()?;
            let preset = Preset {
                pan: cam.pan_degrees()?,
                tilt: cam.tilt_degrees()?,
                zoom: cam.zoom_factor()?,
            };
            let path = config_path()?;
            presets::edit_config_file(&path, |text| {
                presets::upsert_in_document(text, &name, &preset)
            })?;
            ctx.out
                .debug(format!("saved preset '{name}' to {}", path.display()));
            ctx.out.emit(
                || format!("Saved {}", render(&name, &preset)),
                &PresetJson {
                    name: &name,
                    preset,
                },
            );
            Ok(())
        }
        PresetAction::Load { name } => {
            let preset = *ctx.config.presets.get(&name).ok_or_else(|| {
                Error::InvalidValue(format!(
                    "No preset named '{name}'. List presets with: linkctl preset list"
                ))
            })?;
            let (cam, _) = ctx.open_validated(|cam| {
                cam.check_pan_degrees(preset.pan)?;
                cam.check_tilt_degrees(preset.tilt)?;
                cam.check_zoom_factor(preset.zoom)
            })?;
            let applied = Preset {
                pan: cam.set_pan_degrees(preset.pan)?,
                tilt: cam.set_tilt_degrees(preset.tilt)?,
                zoom: cam.set_zoom_factor(preset.zoom)?,
            };
            ctx.out.emit(
                || format!("Loaded {}", render(&name, &applied)),
                &PresetJson {
                    name: &name,
                    preset: applied,
                },
            );
            Ok(())
        }
        PresetAction::Delete { name } => {
            presets::validate_name(&name)?;
            let path = config_path()?;
            let mut existed = false;
            presets::edit_config_file(&path, |text| {
                match presets::remove_from_document(text, &name)? {
                    Some(new_text) => {
                        existed = true;
                        Ok(new_text)
                    }
                    None => Ok(text.to_string()),
                }
            })?;
            if !existed {
                return Err(Error::InvalidValue(format!("No preset named '{name}'.")));
            }
            ctx.out.emit(
                || format!("Deleted preset '{name}'"),
                &serde_json::json!({ "deleted": name }),
            );
            Ok(())
        }
    }
}
