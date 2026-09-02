//! Command-line interface definition (clap derive).

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// Native Linux control for Insta360 Link webcams.
///
/// linkctl never wakes an inactive camera implicitly. Open the camera in an
/// application or run `linkctl preview`, or pass --force to override.
#[derive(Debug, Parser)]
#[command(name = "linkctl", version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    /// Use this video device instead of auto-discovery
    #[arg(long, short = 'd', global = true, value_name = "PATH")]
    pub device: Option<PathBuf>,

    /// Bypass the inactive-camera guard (may physically wake the camera)
    #[arg(long, global = true)]
    pub force: bool,

    /// Machine-readable JSON output
    #[arg(long, global = true, conflicts_with = "quiet")]
    pub json: bool,

    /// No output on success
    #[arg(long, short = 'q', global = true)]
    pub quiet: bool,

    /// Extra diagnostics on stderr
    #[arg(long, short = 'v', global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Show camera state and current framing
    Status,

    /// Show detailed device and control information
    Info(InfoArgs),

    /// List connected Insta360 Link cameras
    Devices,

    /// List the pixel formats, resolutions and frame rates the camera offers
    Formats,

    /// Show or set the camera's current resolution (e.g. 1920x1080@30)
    Resolution(ResolutionArgs),

    /// Recenter the gimbal (pan 0°, tilt 0°)
    Center,

    /// Pan left by DEGREES (default: configured step)
    Left(StepArg),
    /// Pan right by DEGREES (default: configured step)
    Right(StepArg),
    /// Tilt up by DEGREES (default: configured step)
    Up(StepArg),
    /// Tilt down by DEGREES (default: configured step)
    Down(StepArg),

    /// Set absolute pan in degrees (negative = left); omit to read
    Pan(AngleArg),
    /// Set absolute tilt in degrees (negative = down); omit to read
    Tilt(AngleArg),
    /// Set pan and/or tilt absolutely in one command
    Move(MoveArgs),

    /// Set zoom multiplier (e.g. 1, 1.5, 2, 4); omit to read
    Zoom(ZoomArg),
    /// Set focus: `auto` or a manual value; omit to read
    Focus(ModeOrValueArg),
    /// Set white balance: `auto` or a temperature in kelvin; omit to read
    Wb(ModeOrValueArg),

    /// Set brightness; omit to read
    Brightness(ValueArg),
    /// Set contrast; omit to read
    Contrast(ValueArg),
    /// Set saturation; omit to read
    Saturation(ValueArg),
    /// Set sharpness; omit to read
    Sharpness(ValueArg),
    /// Set hue; omit to read
    Hue(ValueArg),

    /// AI subject tracking (experimental, vendor extension unit)
    Tracking(TrackingArgs),

    /// Manage framing presets (pan/tilt/zoom)
    Preset(PresetArgs),

    /// Open a live preview (this intentionally activates the camera)
    Preview(PreviewArgs),
}

#[derive(Debug, Args)]
pub struct InfoArgs {
    /// List every V4L2 control with range, default, flags and value
    #[arg(long)]
    pub controls: bool,
}

#[derive(Debug, Args)]
pub struct ResolutionArgs {
    /// `WIDTHxHEIGHT` or `WIDTHxHEIGHT@FPS`; omit to show the current format
    #[arg(value_name = "WxH[@FPS]")]
    pub spec: Option<String>,
    /// Pixel format: mjpeg or h264 (default: keep current)
    #[arg(long, value_name = "FORMAT")]
    pub format: Option<String>,
}

#[derive(Debug, Args)]
pub struct StepArg {
    /// Degrees to move
    #[arg(value_name = "DEGREES", value_parser = parse_positive_degrees)]
    pub degrees: Option<f64>,
}

#[derive(Debug, Args)]
pub struct AngleArg {
    /// Absolute angle in degrees
    #[arg(value_name = "DEGREES", allow_negative_numbers = true, value_parser = parse_degrees)]
    pub degrees: Option<f64>,
}

#[derive(Debug, Args)]
pub struct MoveArgs {
    /// Absolute pan in degrees
    #[arg(long, allow_negative_numbers = true, value_parser = parse_degrees)]
    pub pan: Option<f64>,
    /// Absolute tilt in degrees
    #[arg(long, allow_negative_numbers = true, value_parser = parse_degrees)]
    pub tilt: Option<f64>,
}

#[derive(Debug, Args)]
pub struct ZoomArg {
    /// Zoom multiplier (1.0 = no zoom)
    #[arg(value_name = "FACTOR", value_parser = parse_zoom)]
    pub factor: Option<f64>,
}

/// `auto` or an integer value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModeOrValue {
    Auto,
    Value(i64),
}

impl std::str::FromStr for ModeOrValue {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let t = s.trim();
        if t.eq_ignore_ascii_case("auto") {
            return Ok(ModeOrValue::Auto);
        }
        t.parse::<i64>()
            .map(ModeOrValue::Value)
            .map_err(|_| format!("expected 'auto' or an integer, got '{s}'"))
    }
}

#[derive(Debug, Args)]
pub struct ModeOrValueArg {
    /// `auto` or a numeric value
    #[arg(value_name = "auto|VALUE")]
    pub value: Option<ModeOrValue>,
}

#[derive(Debug, Args)]
pub struct ValueArg {
    /// Integer value within the control's range
    #[arg(value_name = "VALUE", allow_negative_numbers = true)]
    pub value: Option<i64>,
}

#[derive(Debug, Args)]
pub struct TrackingArgs {
    #[command(subcommand)]
    pub action: Option<TrackingAction>,
}

#[derive(Debug, Subcommand, Clone, Copy, PartialEq, Eq)]
pub enum TrackingAction {
    /// Show whether tracking is enabled
    Status,
    /// Enable AI tracking
    On,
    /// Disable AI tracking
    Off,
    /// Toggle AI tracking
    Toggle,
}

#[derive(Debug, Args)]
pub struct PresetArgs {
    #[command(subcommand)]
    pub action: PresetAction,
}

#[derive(Debug, Subcommand)]
pub enum PresetAction {
    /// Save the current pan/tilt/zoom under NAME
    Save {
        #[arg(value_name = "NAME")]
        name: String,
    },
    /// Apply the preset NAME
    Load {
        #[arg(value_name = "NAME")]
        name: String,
    },
    /// List saved presets
    List,
    /// Delete the preset NAME
    Delete {
        #[arg(value_name = "NAME")]
        name: String,
    },
}

#[derive(Debug, Args)]
pub struct PreviewArgs {
    /// Player to use (overrides config)
    #[arg(long, value_enum)]
    pub player: Option<Player>,
    /// Resolution to request, e.g. 1280x720 (overrides config)
    #[arg(long, value_name = "WxH")]
    pub resolution: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Player {
    Ffplay,
    Mpv,
}

fn parse_f64(s: &str, what: &str) -> Result<f64, String> {
    let v: f64 = s
        .trim()
        .parse()
        .map_err(|_| format!("expected a number for {what}, got '{s}'"))?;
    if !v.is_finite() {
        return Err(format!("{what} must be a finite number"));
    }
    Ok(v)
}

fn parse_degrees(s: &str) -> Result<f64, String> {
    let s = s.trim().trim_end_matches('°');
    parse_f64(s, "degrees")
}

fn parse_positive_degrees(s: &str) -> Result<f64, String> {
    let v = parse_degrees(s)?;
    if v <= 0.0 {
        return Err(format!(
            "step must be a positive number of degrees, got {v}"
        ));
    }
    Ok(v)
}

fn parse_zoom(s: &str) -> Result<f64, String> {
    let s = s.trim().trim_end_matches(['x', 'X']);
    let v = parse_f64(s, "zoom")?;
    if v <= 0.0 {
        return Err(format!("zoom must be positive, got {v}"));
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("linkctl").chain(args.iter().copied()))
    }

    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn global_flags_anywhere() {
        let c = parse(&["--force", "center"]).unwrap();
        assert!(c.force);
        assert!(matches!(c.command, Command::Center));
        let c = parse(&["center", "--force"]).unwrap();
        assert!(c.force);
        let c = parse(&["--quiet", "left"]).unwrap();
        assert!(c.quiet);
        let c = parse(&["--device", "/dev/video4", "status"]).unwrap();
        assert_eq!(
            c.device.as_deref(),
            Some(std::path::Path::new("/dev/video4"))
        );
        assert!(parse(&["--json", "--quiet", "status"]).is_err());
    }

    #[test]
    fn relative_moves() {
        let c = parse(&["left"]).unwrap();
        assert!(matches!(
            c.command,
            Command::Left(StepArg { degrees: None })
        ));
        let c = parse(&["right", "20"]).unwrap();
        assert!(matches!(c.command, Command::Right(StepArg { degrees: Some(d) }) if d == 20.0));
        assert!(parse(&["up", "-5"]).is_err());
        assert!(parse(&["up", "0"]).is_err());
    }

    #[test]
    fn absolute_moves_accept_negatives() {
        let c = parse(&["pan", "-30"]).unwrap();
        assert!(matches!(c.command, Command::Pan(AngleArg { degrees: Some(d) }) if d == -30.0));
        let c = parse(&["tilt", "15°"]).unwrap();
        assert!(matches!(c.command, Command::Tilt(AngleArg { degrees: Some(d) }) if d == 15.0));
        let c = parse(&["move", "--pan", "30", "--tilt", "-10"]).unwrap();
        match c.command {
            Command::Move(m) => {
                assert_eq!(m.pan, Some(30.0));
                assert_eq!(m.tilt, Some(-10.0));
            }
            _ => panic!(),
        }
        let c = parse(&["pan"]).unwrap();
        assert!(matches!(
            c.command,
            Command::Pan(AngleArg { degrees: None })
        ));
    }

    #[test]
    fn zoom_and_modes() {
        let c = parse(&["zoom", "1.5x"]).unwrap();
        assert!(matches!(c.command, Command::Zoom(ZoomArg { factor: Some(f) }) if f == 1.5));
        assert!(parse(&["zoom", "0"]).is_err());
        let c = parse(&["focus", "auto"]).unwrap();
        assert!(matches!(
            c.command,
            Command::Focus(ModeOrValueArg {
                value: Some(ModeOrValue::Auto)
            })
        ));
        let c = parse(&["focus", "80"]).unwrap();
        assert!(matches!(
            c.command,
            Command::Focus(ModeOrValueArg {
                value: Some(ModeOrValue::Value(80))
            })
        ));
        let c = parse(&["wb", "3650"]).unwrap();
        assert!(matches!(
            c.command,
            Command::Wb(ModeOrValueArg {
                value: Some(ModeOrValue::Value(3650))
            })
        ));
        assert!(parse(&["wb", "warm"]).is_err());
        let c = parse(&["hue", "-5"]).unwrap();
        assert!(matches!(
            c.command,
            Command::Hue(ValueArg { value: Some(-5) })
        ));
    }

    #[test]
    fn presets_tracking_preview() {
        let c = parse(&["preset", "save", "desk"]).unwrap();
        assert!(
            matches!(c.command, Command::Preset(PresetArgs { action: PresetAction::Save { ref name } }) if name == "desk")
        );
        let c = parse(&["preset", "list"]).unwrap();
        assert!(matches!(
            c.command,
            Command::Preset(PresetArgs {
                action: PresetAction::List
            })
        ));
        let c = parse(&["tracking", "toggle"]).unwrap();
        assert!(matches!(
            c.command,
            Command::Tracking(TrackingArgs {
                action: Some(TrackingAction::Toggle)
            })
        ));
        let c = parse(&["tracking"]).unwrap();
        assert!(matches!(
            c.command,
            Command::Tracking(TrackingArgs { action: None })
        ));
        let c = parse(&["preview", "--player", "mpv"]).unwrap();
        assert!(matches!(
            c.command,
            Command::Preview(PreviewArgs {
                player: Some(Player::Mpv),
                ..
            })
        ));
        let c = parse(&["info", "--controls"]).unwrap();
        assert!(matches!(
            c.command,
            Command::Info(InfoArgs { controls: true })
        ));
    }

    #[test]
    fn formats_and_resolution() {
        let c = parse(&["formats"]).unwrap();
        assert!(matches!(c.command, Command::Formats));
        let c = parse(&["resolution"]).unwrap();
        assert!(matches!(
            c.command,
            Command::Resolution(ResolutionArgs {
                spec: None,
                format: None
            })
        ));
        let c = parse(&["resolution", "1920x1080@30", "--format", "mjpeg"]).unwrap();
        match c.command {
            Command::Resolution(r) => {
                assert_eq!(r.spec.as_deref(), Some("1920x1080@30"));
                assert_eq!(r.format.as_deref(), Some("mjpeg"));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn missing_subcommand_is_usage_error() {
        let err = parse(&[]).unwrap_err();
        assert!(matches!(
            err.kind(),
            ErrorKind::MissingSubcommand | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        ));
    }
}
