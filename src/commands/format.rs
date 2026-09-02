//! `formats` (list what the camera offers) and `resolution` (get/set the
//! driver's current capture format).

use super::Context;
use crate::camera::format::{self, FormatRequest, FourCc};
use crate::error::{Error, Result};

pub fn list(ctx: &Context) -> Result<()> {
    let cam = ctx.open_camera()?;
    let formats = cam.device().enumerate_formats()?;
    let current = cam.device().current_format().ok();
    ctx.out.emit(
        || {
            let mut lines = Vec::new();
            for f in &formats {
                lines.push(format!("{} ({})", f.fourcc, f.description));
                for s in &f.sizes {
                    let mark = match current {
                        Some(c)
                            if c.fourcc == f.fourcc
                                && c.width == s.width
                                && c.height == s.height =>
                        {
                            "*"
                        }
                        _ => " ",
                    };
                    let fps: Vec<String> = s.fps.iter().map(|v| format::format_fps(*v)).collect();
                    lines.push(format!(
                        "{mark} {:>4}x{:<4}  {} fps",
                        s.width,
                        s.height,
                        fps.join(" ")
                    ));
                }
            }
            if current.is_some() {
                lines.push(String::new());
                lines.push("* = current format (see: linkctl resolution)".into());
            }
            lines.join("\n")
        },
        &formats,
    );
    Ok(())
}

pub fn resolution(ctx: &Context, spec: Option<&str>, pixel_format: Option<&str>) -> Result<()> {
    let cam = ctx.open_camera()?;
    let dev = cam.device();

    let fourcc = match pixel_format {
        Some(name) => Some(FourCc::parse(name).ok_or_else(|| {
            Error::InvalidValue(format!(
                "Unknown pixel format '{name}' (use mjpeg or h264)."
            ))
        })?),
        None => None,
    };

    let applied = match spec {
        None if fourcc.is_none() => dev.current_format()?,
        _ => {
            let current = dev.current_format()?;
            let (width, height, fps) = match spec {
                Some(s) => format::parse_resolution_spec(s).ok_or_else(|| {
                    Error::InvalidValue(format!(
                        "Expected WIDTHxHEIGHT or WIDTHxHEIGHT@FPS, got '{s}'."
                    ))
                })?,
                None => (current.width, current.height, None),
            };
            let req = FormatRequest {
                fourcc,
                width,
                height,
                fps,
            };
            let formats = dev.enumerate_formats()?;
            format::validate_request(&formats, &current, &req).map_err(Error::InvalidValue)?;
            ctx.out.debug(format!(
                "setting format {}x{} {} fps={:?}",
                width,
                height,
                req.fourcc.unwrap_or(current.fourcc),
                fps
            ));
            dev.set_format(req)?
        }
    };

    ctx.out.emit(
        || {
            let fps = applied
                .fps
                .map(|f| format!(" @ {} fps", format::format_fps(f)))
                .unwrap_or_default();
            format!(
                "Resolution: {}{fps} {}",
                applied.resolution(),
                applied.fourcc
            )
        },
        &applied,
    );
    Ok(())
}
