//! Focus, white balance and simple image controls.
//!
//! Image controls do not move the gimbal, but they still change camera state
//! and the firmware only honours them reliably while streaming, so they use
//! the same inactivity guard as motion commands. Reads are always allowed.

use serde::Serialize;

use super::Context;
use crate::camera::controls::Control;
use crate::cli::ModeOrValue;
use crate::error::Result;

#[derive(Serialize)]
struct FocusJson {
    auto: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<i64>,
}

#[derive(Serialize)]
struct WbJson {
    auto: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<i64>,
}

pub fn focus(ctx: &Context, arg: Option<ModeOrValue>) -> Result<()> {
    let (auto, value) = match arg {
        None => {
            let cam = ctx.open_camera()?;
            let auto = cam.try_get_raw(Control::FocusAuto)?.unwrap_or(0) != 0;
            let value = cam.try_get_raw(Control::FocusAbsolute)?;
            (auto, value)
        }
        Some(ModeOrValue::Auto) => {
            let cam = ctx.open_camera_for_control()?;
            cam.set_focus_auto()?;
            (true, cam.try_get_raw(Control::FocusAbsolute)?)
        }
        Some(ModeOrValue::Value(v)) => {
            let (cam, _) = ctx.open_validated(|cam| cam.check_raw(Control::FocusAbsolute, v))?;
            let v = cam.set_focus_manual(v)?;
            (false, Some(v))
        }
    };
    ctx.out.emit(
        || {
            if auto {
                "Focus: Auto".to_string()
            } else {
                format!(
                    "Focus: {}",
                    value
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "Manual".into())
                )
            }
        },
        &FocusJson { auto, value },
    );
    Ok(())
}

pub fn white_balance(ctx: &Context, arg: Option<ModeOrValue>) -> Result<()> {
    let (auto, temperature) = match arg {
        None => {
            let cam = ctx.open_camera()?;
            let auto = cam.try_get_raw(Control::WhiteBalanceAuto)?.unwrap_or(0) != 0;
            let t = cam.try_get_raw(Control::WhiteBalanceTemperature)?;
            (auto, t)
        }
        Some(ModeOrValue::Auto) => {
            let cam = ctx.open_camera_for_control()?;
            cam.set_white_balance_auto()?;
            (true, cam.try_get_raw(Control::WhiteBalanceTemperature)?)
        }
        Some(ModeOrValue::Value(k)) => {
            let (cam, _) =
                ctx.open_validated(|cam| cam.check_raw(Control::WhiteBalanceTemperature, k))?;
            let k = cam.set_white_balance_temperature(k)?;
            (false, Some(k))
        }
    };
    ctx.out.emit(
        || {
            if auto {
                "White balance: Auto".to_string()
            } else {
                format!(
                    "White balance: {}",
                    temperature
                        .map(|t| format!("{t}K"))
                        .unwrap_or_else(|| "Manual".into())
                )
            }
        },
        &WbJson { auto, temperature },
    );
    Ok(())
}

/// Integer controls with no mode switch.
#[derive(Debug, Clone, Copy)]
pub enum Simple {
    Brightness,
    Contrast,
    Saturation,
    Sharpness,
    Hue,
}

impl Simple {
    fn control(self) -> Control {
        match self {
            Simple::Brightness => Control::Brightness,
            Simple::Contrast => Control::Contrast,
            Simple::Saturation => Control::Saturation,
            Simple::Sharpness => Control::Sharpness,
            Simple::Hue => Control::Hue,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Simple::Brightness => "Brightness",
            Simple::Contrast => "Contrast",
            Simple::Saturation => "Saturation",
            Simple::Sharpness => "Sharpness",
            Simple::Hue => "Hue",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Simple::Brightness => "brightness",
            Simple::Contrast => "contrast",
            Simple::Saturation => "saturation",
            Simple::Sharpness => "sharpness",
            Simple::Hue => "hue",
        }
    }
}

pub fn simple(ctx: &Context, which: Simple, value: Option<i64>) -> Result<()> {
    let control = which.control();
    let v = match value {
        None => ctx.open_camera()?.get_raw(control)?,
        Some(v) => {
            let (cam, _) = ctx.open_validated(|cam| cam.check_raw(control, v))?;
            cam.set_raw(control, v)?
        }
    };
    let json = serde_json::json!({ which.key(): v });
    ctx.out.emit(|| format!("{}: {v}", which.label()), &json);
    Ok(())
}
