use serde::Serialize;

use super::Context;
use crate::camera::activity::Holder;
use crate::camera::controls::Control;
use crate::camera::insta360::link2;
use crate::camera::{self, Camera};
use crate::cli::StatusArgs;
use crate::error::Result;
use crate::units::{format_degrees, format_zoom};

#[derive(Serialize)]
struct FocusJson {
    auto: bool,
    value: Option<i64>,
}

#[derive(Serialize)]
struct WbJson {
    auto: bool,
    temperature: Option<i64>,
}

#[derive(Serialize)]
struct StatusJson<'a> {
    model: &'a str,
    device: &'a std::path::Path,
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pan: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tilt: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    zoom: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    focus: Option<FocusJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    white_balance: Option<WbJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tracking: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    used_by: Vec<Holder>,
    /// How `state` was determined. `device-in-use` covers the whole USB
    /// device (video or microphone); `holders` means a process was found
    /// holding a video node open.
    detection: camera::Detection,
    /// The holder scan hit its time budget, so `inactive` means "no holder
    /// found in time" rather than "proven idle".
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    partial: bool,
}

pub fn run(ctx: &Context, args: &StatusArgs) -> Result<()> {
    let info = ctx.device_info()?;
    // Without `--holders` the cheap USB-level reading is enough, and keeps a
    // polled `status` off the O(all file descriptors) scan path.
    let (check, holders) = if args.holders {
        let scan = camera::check_activity(&info);
        (
            camera::InUse {
                in_use: scan.is_active(),
                detection: camera::Detection::Holders,
                partial: scan.partial,
            },
            scan.holders,
        )
    } else {
        (camera::quick_in_use(&info), Vec::new())
    };
    let active = check.in_use;
    let model = info.model.name();
    let device = info.control_node.clone();

    if !active {
        let json = StatusJson {
            model,
            device: &device,
            state: "inactive",
            pan: None,
            tilt: None,
            zoom: None,
            focus: None,
            white_balance: None,
            tracking: None,
            used_by: Vec::new(),
            detection: check.detection,
            partial: check.partial,
        };
        ctx.out.emit(
            || {
                format!(
                    "{model}\nState: inactive\nDevice: {}\n\nStart it with:\n  linkctl preview",
                    device.display()
                )
            },
            &json,
        );
        return Ok(());
    }

    let used_by_line = if holders.is_empty() {
        None
    } else {
        Some(
            holders
                .iter()
                .map(|h| format!("{} ({})", h.comm, h.pid))
                .collect::<Vec<_>>()
                .join(", "),
        )
    };

    let cam = Camera::open(info)?;
    let s = read_state(&cam, ctx);
    let json = StatusJson {
        model,
        device: &device,
        state: "active",
        pan: s.pan,
        tilt: s.tilt,
        zoom: s.zoom,
        focus: s.focus.map(|(auto, value)| FocusJson { auto, value }),
        white_balance: s.wb.map(|(auto, temperature)| WbJson { auto, temperature }),
        tracking: s.tracking,
        used_by: holders,
        detection: check.detection,
        partial: check.partial,
    };
    ctx.out.emit(
        || {
            let mut lines = vec![
                model.to_string(),
                "State: active".to_string(),
                format!("Device: {}", device.display()),
                String::new(),
            ];
            if let Some(p) = s.pan {
                lines.push(format!("Pan:           {:>6}", format_degrees(p)));
            }
            if let Some(t) = s.tilt {
                lines.push(format!("Tilt:          {:>6}", format_degrees(t)));
            }
            if let Some(z) = s.zoom {
                lines.push(format!("Zoom:          {:>6}", format_zoom(z)));
            }
            if let Some((auto, value)) = s.focus {
                let v = if auto {
                    "Auto".to_string()
                } else {
                    value
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "Manual".into())
                };
                lines.push(format!("Focus:         {v:>6}"));
            }
            if let Some((auto, temp)) = s.wb {
                let v = if auto {
                    "Auto".to_string()
                } else {
                    temp.map(|t| format!("{t}K"))
                        .unwrap_or_else(|| "Manual".into())
                };
                lines.push(format!("White balance: {v:>6}"));
            }
            if let Some(t) = s.tracking {
                lines.push(format!(
                    "Tracking:      {:>6}",
                    if t { "On" } else { "Off" }
                ));
            }
            if let Some(who) = &used_by_line {
                lines.push(format!("Used by:       {who}"));
            }
            lines.join("\n")
        },
        &json,
    );
    Ok(())
}

#[derive(Default)]
struct State {
    pan: Option<f64>,
    tilt: Option<f64>,
    zoom: Option<f64>,
    focus: Option<(bool, Option<i64>)>,
    wb: Option<(bool, Option<i64>)>,
    tracking: Option<bool>,
}

/// Read everything we can; individual unsupported controls are skipped.
fn read_state(cam: &Camera, ctx: &Context) -> State {
    let mut s = State {
        pan: cam.pan_degrees().ok(),
        tilt: cam.tilt_degrees().ok(),
        zoom: cam.zoom_factor().ok(),
        ..State::default()
    };
    if let Ok(Some(auto)) = cam.try_get_raw(Control::FocusAuto) {
        let value = cam.try_get_raw(Control::FocusAbsolute).ok().flatten();
        s.focus = Some((auto != 0, value));
    }
    if let Ok(Some(auto)) = cam.try_get_raw(Control::WhiteBalanceAuto) {
        let temp = cam
            .try_get_raw(Control::WhiteBalanceTemperature)
            .ok()
            .flatten();
        s.wb = Some((auto != 0, temp));
    }
    match link2::read_tracking(cam.device()) {
        Ok(t) => s.tracking = Some(t.enabled),
        Err(e) => ctx.out.debug(format!("tracking state unavailable: {e}")),
    }
    s
}
