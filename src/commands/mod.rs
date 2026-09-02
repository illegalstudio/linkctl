//! Command implementations. Each command receives a [`Context`] and returns
//! a [`Result`]; all user-facing rendering goes through [`Output`].

mod devices;
mod image;
mod info;
mod motion;
mod preset;
mod status;
mod tracking;

use std::path::PathBuf;

use crate::camera::discovery::{self, DeviceInfo};
use crate::camera::{self, Camera};
use crate::cli::{Cli, Command};
use crate::config::Config;
use crate::error::{Error, Result};
use crate::output::Output;

/// Everything a command needs besides its own arguments.
pub struct Context {
    pub out: Output,
    pub config: Config,
    pub force: bool,
    pub device: Option<PathBuf>,
}

impl Context {
    /// Discover (or resolve the explicit) camera without opening it.
    pub fn device_info(&self) -> Result<DeviceInfo> {
        let info = discovery::select(self.device.as_deref())?;
        self.out.debug(format!(
            "using {} ({}), control node {}",
            info.model,
            info.usb.port_path,
            info.control_node.display()
        ));
        if !info.model.is_tested() {
            self.out.note(format!(
                "Note: {} is recognised but has not been validated with linkctl.",
                info.model
            ));
        }
        Ok(info)
    }

    /// Open the camera for read-only use (status, info, preset save...).
    pub fn open_camera(&self) -> Result<Camera> {
        let info = self.device_info()?;
        Camera::open(info)
    }

    /// Open the camera for a state-changing command. Activity is checked
    /// **before** the control node is opened, and refused unless `--force`.
    pub fn open_camera_for_control(&self) -> Result<Camera> {
        let info = self.device_info()?;
        self.ensure_active(&info)?;
        Camera::open(info)
    }

    /// Open the camera, run `validate` (which may read the device but must
    /// not write), then apply the inactivity guard. Lets bad arguments fail
    /// with a precise message even when the camera is inactive.
    pub fn open_validated<T>(
        &self,
        validate: impl FnOnce(&Camera) -> Result<T>,
    ) -> Result<(Camera, T)> {
        let cam = self.open_camera()?;
        let value = validate(&cam)?;
        self.ensure_active(cam.info())?;
        Ok((cam, value))
    }

    /// The inactivity guard. Our own open handles never count as activity
    /// (the scan skips our pid), so this may run before or after `open`.
    pub fn ensure_active(&self, info: &DeviceInfo) -> Result<()> {
        let activity = camera::check_activity(info);
        self.out.debug(format!(
            "activity: {} holder(s), {} process(es) not inspectable",
            activity.holders.len(),
            activity.skipped
        ));
        if activity.is_active() {
            return Ok(());
        }
        if self.force {
            self.out
                .debug("camera inactive, proceeding because of --force");
            return Ok(());
        }
        Err(Error::CameraInactive)
    }
}

/// Dispatch a parsed command line.
pub fn run(cli: Cli) -> Result<()> {
    let out = Output::new(cli.json, cli.quiet, cli.verbose);
    let config = Config::load()?;
    out.debug(format!(
        "config: {}",
        Config::path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(none)".into())
    ));
    let ctx = Context {
        out,
        config,
        force: cli.force,
        device: cli.device,
    };

    match cli.command {
        Command::Status => status::run(&ctx),
        Command::Info(args) => info::run(&ctx, &args),
        Command::Devices => devices::run(&ctx),

        Command::Center => motion::center(&ctx),
        Command::Left(a) => motion::relative(&ctx, motion::Direction::Left, a.degrees),
        Command::Right(a) => motion::relative(&ctx, motion::Direction::Right, a.degrees),
        Command::Up(a) => motion::relative(&ctx, motion::Direction::Up, a.degrees),
        Command::Down(a) => motion::relative(&ctx, motion::Direction::Down, a.degrees),
        Command::Pan(a) => motion::pan(&ctx, a.degrees),
        Command::Tilt(a) => motion::tilt(&ctx, a.degrees),
        Command::Move(a) => motion::move_to(&ctx, a.pan, a.tilt),
        Command::Zoom(a) => motion::zoom(&ctx, a.factor),

        Command::Focus(a) => image::focus(&ctx, a.value),
        Command::Wb(a) => image::white_balance(&ctx, a.value),
        Command::Brightness(a) => image::simple(&ctx, image::Simple::Brightness, a.value),
        Command::Contrast(a) => image::simple(&ctx, image::Simple::Contrast, a.value),
        Command::Saturation(a) => image::simple(&ctx, image::Simple::Saturation, a.value),
        Command::Sharpness(a) => image::simple(&ctx, image::Simple::Sharpness, a.value),
        Command::Hue(a) => image::simple(&ctx, image::Simple::Hue, a.value),

        Command::Tracking(a) => tracking::run(&ctx, a.action),
        Command::Preset(a) => preset::run(&ctx, a.action),
        Command::Preview(a) => preview(&ctx, a.player, a.resolution),
    }
}

fn preview(
    ctx: &Context,
    player: Option<crate::cli::Player>,
    resolution: Option<String>,
) -> Result<()> {
    use crate::cli::Player;
    let info = ctx.device_info()?;
    let player = match player {
        Some(p) => p,
        None => match ctx.config.preview_player.as_str() {
            "ffplay" => Player::Ffplay,
            "mpv" => Player::Mpv,
            other => {
                return Err(Error::Config(format!(
                    "preview_player = \"{other}\" is not supported (use \"ffplay\" or \"mpv\")"
                )))
            }
        },
    };
    let resolution = resolution.unwrap_or_else(|| ctx.config.preview_resolution.clone());
    if crate::config::parse_resolution(&resolution).is_none() {
        return Err(Error::InvalidValue(format!(
            "resolution must look like WIDTHxHEIGHT (got {resolution:?})"
        )));
    }
    ctx.out.debug(format!(
        "starting {:?} on {} at {resolution}",
        player,
        info.stream_node.display()
    ));
    let code = crate::preview::run(player, &info.stream_node, info.model.name(), &resolution)?;
    if code != 0 {
        return Err(Error::Preview(format!(
            "preview player exited with status {code}"
        )));
    }
    Ok(())
}
