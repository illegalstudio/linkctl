//! Gimbal and zoom commands. Every write goes through
//! `Context::open_camera_for_control`, i.e. the inactivity guard.

use serde::Serialize;

use super::Context;
use crate::error::Result;
use crate::units::{format_degrees, format_zoom};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl Direction {
    /// Signed delta applied to pan (left/right) or tilt (up/down).
    pub fn signed(self, degrees: f64) -> f64 {
        match self {
            Direction::Left | Direction::Down => -degrees,
            Direction::Right | Direction::Up => degrees,
        }
    }

    pub fn is_pan(self) -> bool {
        matches!(self, Direction::Left | Direction::Right)
    }
}

#[derive(Serialize)]
struct PanTiltJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    pan: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tilt: Option<f64>,
}

#[derive(Serialize)]
struct ZoomJson {
    zoom: f64,
}

fn emit_pan_tilt(ctx: &Context, pan: Option<f64>, tilt: Option<f64>) {
    ctx.out.emit(
        || {
            let mut parts = Vec::new();
            if let Some(p) = pan {
                parts.push(format!("Pan: {}", format_degrees(p)));
            }
            if let Some(t) = tilt {
                parts.push(format!("Tilt: {}", format_degrees(t)));
            }
            parts.join("\n")
        },
        &PanTiltJson { pan, tilt },
    );
}

pub fn center(ctx: &Context) -> Result<()> {
    let cam = ctx.open_camera_for_control()?;
    let pan = cam.set_pan_degrees(0.0)?;
    let tilt = cam.set_tilt_degrees(0.0)?;
    emit_pan_tilt(ctx, Some(pan), Some(tilt));
    Ok(())
}

pub fn relative(ctx: &Context, dir: Direction, degrees: Option<f64>) -> Result<()> {
    let step = degrees.unwrap_or(ctx.config.default_step);
    let delta = dir.signed(step);
    let cam = ctx.open_camera_for_control()?;
    if dir.is_pan() {
        let pan = cam.pan_relative(delta)?;
        emit_pan_tilt(ctx, Some(pan), None);
    } else {
        let tilt = cam.tilt_relative(delta)?;
        emit_pan_tilt(ctx, None, Some(tilt));
    }
    Ok(())
}

pub fn pan(ctx: &Context, degrees: Option<f64>) -> Result<()> {
    match degrees {
        None => {
            let cam = ctx.open_camera()?;
            let pan = cam.pan_degrees()?;
            emit_pan_tilt(ctx, Some(pan), None);
        }
        Some(d) => {
            let cam = ctx.open_camera_for_control()?;
            let pan = cam.set_pan_degrees(d)?;
            emit_pan_tilt(ctx, Some(pan), None);
        }
    }
    Ok(())
}

pub fn tilt(ctx: &Context, degrees: Option<f64>) -> Result<()> {
    match degrees {
        None => {
            let cam = ctx.open_camera()?;
            let tilt = cam.tilt_degrees()?;
            emit_pan_tilt(ctx, None, Some(tilt));
        }
        Some(d) => {
            let cam = ctx.open_camera_for_control()?;
            let tilt = cam.set_tilt_degrees(d)?;
            emit_pan_tilt(ctx, None, Some(tilt));
        }
    }
    Ok(())
}

pub fn move_to(ctx: &Context, pan: Option<f64>, tilt: Option<f64>) -> Result<()> {
    if pan.is_none() && tilt.is_none() {
        return Err(crate::error::Error::InvalidValue(
            "move requires --pan and/or --tilt".into(),
        ));
    }
    let cam = ctx.open_camera_for_control()?;
    let new_pan = match pan {
        Some(p) => Some(cam.set_pan_degrees(p)?),
        None => None,
    };
    let new_tilt = match tilt {
        Some(t) => Some(cam.set_tilt_degrees(t)?),
        None => None,
    };
    emit_pan_tilt(ctx, new_pan, new_tilt);
    Ok(())
}

pub fn zoom(ctx: &Context, factor: Option<f64>) -> Result<()> {
    let zoom = match factor {
        None => ctx.open_camera()?.zoom_factor()?,
        Some(f) => ctx.open_camera_for_control()?.set_zoom_factor(f)?,
    };
    ctx.out.emit(
        || format!("Zoom: {}", format_zoom(zoom)),
        &ZoomJson { zoom },
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_signs() {
        assert_eq!(Direction::Left.signed(5.0), -5.0);
        assert_eq!(Direction::Right.signed(5.0), 5.0);
        assert_eq!(Direction::Up.signed(5.0), 5.0);
        assert_eq!(Direction::Down.signed(5.0), -5.0);
        assert!(Direction::Left.is_pan());
        assert!(!Direction::Up.is_pan());
    }
}
