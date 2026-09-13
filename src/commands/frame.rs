//! Digital framing is deliberately separate from degree-based gimbal controls.

use serde::Serialize;

use super::Context;
use crate::camera::insta360::link2c::{self, Frame};
use crate::cli::FrameArgs;
use crate::error::Result;

#[derive(Serialize)]
struct Coordinates {
    x: f64,
    y: f64,
    zoom: f64,
}

impl From<Frame> for Coordinates {
    fn from(frame: Frame) -> Self {
        Self {
            x: frame.normalized_x(),
            y: frame.normalized_y(),
            zoom: frame.zoom_factor(),
        }
    }
}

#[derive(Serialize)]
struct FrameJson {
    #[serde(flatten)]
    current: Coordinates,
    #[serde(skip_serializing_if = "Option::is_none")]
    requested: Option<Coordinates>,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<&'static str>,
}

pub fn run(ctx: &Context, args: &FrameArgs) -> Result<()> {
    let x = if args.center { Some(0.5) } else { args.x };
    let y = if args.center { Some(0.5) } else { args.y };
    let (current, requested, warning) = if x.is_none() && y.is_none() && args.zoom.is_none() {
        (link2c::read(&ctx.open_camera()?)?, None, None)
    } else {
        let (cam, target) = ctx.open_validated(|cam| {
            let current = link2c::read(cam)?;
            let target = current.updated(x, y, args.zoom)?;
            cam.check_zoom_factor(target.zoom_factor())?;
            Ok(target)
        })?;
        link2c::write(&cam, target)?;
        // Report observed coordinates, never present the request as confirmation.
        let current = link2c::read(&cam)?;
        let warning = if (current.x, current.y, current.zoom) != (target.x, target.y, target.zoom) {
            Some("The camera has not confirmed the requested frame yet. It may still be moving, clamping the crop, or applying Auto Framing; check the live preview and run 'linkctl frame' again.")
        } else {
            None
        };
        (current, Some(Coordinates::from(target)), warning)
    };
    let json = FrameJson {
        current: current.into(),
        requested,
        warning,
    };
    ctx.out.emit(
        || {
            let mut text = format!(
                "Frame: x {:.4}, y {:.4}, zoom {:.2}x",
                json.current.x, json.current.y, json.current.zoom
            );
            if let Some(warning) = warning {
                text.push_str(&format!("\n\n{warning}"));
            }
            text
        },
        &json,
    );
    Ok(())
}
