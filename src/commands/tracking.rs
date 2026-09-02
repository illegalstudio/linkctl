//! AI tracking via the Link 2 AI extension unit. Experimental.

use serde::Serialize;

use super::Context;
use crate::camera::insta360::link2::{self, guid, XU_AI_UNIT};
use crate::camera::model::Model;
use crate::camera::Camera;
use crate::cli::TrackingAction;
use crate::error::{Error, Result};

#[derive(Serialize)]
struct TrackingJson {
    tracking: bool,
    raw: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<String>,
}

fn emit(ctx: &Context, state: link2::TrackingState, warning: Option<String>) {
    let json = TrackingJson {
        tracking: state.enabled,
        raw: state.raw,
        warning: warning.clone(),
    };
    ctx.out.emit(
        || {
            let mut s = format!("Tracking: {}", if state.enabled { "On" } else { "Off" });
            if let Some(w) = &warning {
                s.push_str(&format!("\n\n{w}"));
            }
            s
        },
        &json,
    );
}

/// Make sure the device really is a Link 2 whose unit 11 carries the AI GUID.
fn check_unit(cam: &Camera) -> Result<()> {
    let info = cam.info();
    if info.model != Model::Link2 {
        return Err(Error::Vendor(format!(
            "tracking is only implemented for the Insta360 Link 2 (found {})",
            info.model
        )));
    }
    let desc = link2::read_descriptors(&info.usb.sysfs_path)?;
    let units = link2::parse_extension_units(&desc);
    link2::confirm_unit(&units, XU_AI_UNIT, &guid::AI)
}

pub fn run(ctx: &Context, action: Option<TrackingAction>) -> Result<()> {
    let action = action.unwrap_or(TrackingAction::Status);
    match action {
        TrackingAction::Status => {
            let cam = ctx.open_camera()?;
            check_unit(&cam)?;
            let state = link2::read_tracking(cam.device())?;
            emit(ctx, state, None);
            Ok(())
        }
        TrackingAction::On | TrackingAction::Off | TrackingAction::Toggle => {
            let cam = ctx.open_camera_for_control()?;
            check_unit(&cam)?;
            let current = link2::read_tracking(cam.device())?;
            let target = match action {
                TrackingAction::On => true,
                TrackingAction::Off => false,
                _ => !current.enabled,
            };
            ctx.out.debug(format!(
                "tracking: current raw=0x{:02x}, writing {}",
                current.raw,
                u8::from(target)
            ));
            let after = link2::write_tracking(cam.device(), target)?;
            let warning = if after.enabled != target {
                Some(
                    "The camera did not confirm the change. Vendor settings are only \
                     applied by the firmware while video is streaming."
                        .to_string(),
                )
            } else {
                None
            };
            emit(ctx, after, warning);
            Ok(())
        }
    }
}
