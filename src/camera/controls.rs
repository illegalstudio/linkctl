//! Typed names for the standard V4L2 controls `linkctl` uses.
//!
//! Control ids come from `<linux/v4l2-controls.h>`; the values below are the
//! upstream constants, not guesses. Adding a control means adding a variant
//! here and (optionally) a CLI subcommand — nothing else.

const V4L2_CTRL_CLASS_USER: u32 = 0x0098_0000;
const V4L2_CTRL_CLASS_CAMERA: u32 = 0x009a_0000;
const V4L2_CID_BASE: u32 = V4L2_CTRL_CLASS_USER | 0x900;
const V4L2_CID_CAMERA_CLASS_BASE: u32 = V4L2_CTRL_CLASS_CAMERA | 0x900;

/// Standard V4L2 controls known to `linkctl`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Control {
    // User class
    Brightness,
    Contrast,
    Saturation,
    Hue,
    WhiteBalanceAuto,
    Gain,
    PowerLineFrequency,
    WhiteBalanceTemperature,
    Sharpness,
    BacklightCompensation,
    // Camera class
    ExposureAbsolute,
    PanAbsolute,
    TiltAbsolute,
    FocusAbsolute,
    FocusAuto,
    ZoomAbsolute,
}

impl Control {
    /// The `V4L2_CID_*` id.
    pub const fn id(self) -> u32 {
        match self {
            Control::Brightness => V4L2_CID_BASE,
            Control::Contrast => V4L2_CID_BASE + 1,
            Control::Saturation => V4L2_CID_BASE + 2,
            Control::Hue => V4L2_CID_BASE + 3,
            Control::WhiteBalanceAuto => V4L2_CID_BASE + 12,
            Control::Gain => V4L2_CID_BASE + 19,
            Control::PowerLineFrequency => V4L2_CID_BASE + 24,
            Control::WhiteBalanceTemperature => V4L2_CID_BASE + 26,
            Control::Sharpness => V4L2_CID_BASE + 27,
            Control::BacklightCompensation => V4L2_CID_BASE + 28,
            Control::ExposureAbsolute => V4L2_CID_CAMERA_CLASS_BASE + 2,
            Control::PanAbsolute => V4L2_CID_CAMERA_CLASS_BASE + 8,
            Control::TiltAbsolute => V4L2_CID_CAMERA_CLASS_BASE + 9,
            Control::FocusAbsolute => V4L2_CID_CAMERA_CLASS_BASE + 10,
            Control::FocusAuto => V4L2_CID_CAMERA_CLASS_BASE + 12,
            Control::ZoomAbsolute => V4L2_CID_CAMERA_CLASS_BASE + 13,
        }
    }

    /// Stable, user-facing name (matches the kernel's `v4l2-ctl` names).
    pub const fn name(self) -> &'static str {
        match self {
            Control::Brightness => "brightness",
            Control::Contrast => "contrast",
            Control::Saturation => "saturation",
            Control::Hue => "hue",
            Control::WhiteBalanceAuto => "white_balance_automatic",
            Control::Gain => "gain",
            Control::PowerLineFrequency => "power_line_frequency",
            Control::WhiteBalanceTemperature => "white_balance_temperature",
            Control::Sharpness => "sharpness",
            Control::BacklightCompensation => "backlight_compensation",
            Control::ExposureAbsolute => "exposure_time_absolute",
            Control::PanAbsolute => "pan_absolute",
            Control::TiltAbsolute => "tilt_absolute",
            Control::FocusAbsolute => "focus_absolute",
            Control::FocusAuto => "focus_automatic_continuous",
            Control::ZoomAbsolute => "zoom_absolute",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_match_kernel_headers() {
        // Values printed by `v4l2-ctl --list-ctrls` on the Link 2.
        assert_eq!(Control::Brightness.id(), 0x0098_0900);
        assert_eq!(Control::Contrast.id(), 0x0098_0901);
        assert_eq!(Control::Saturation.id(), 0x0098_0902);
        assert_eq!(Control::Hue.id(), 0x0098_0903);
        assert_eq!(Control::WhiteBalanceAuto.id(), 0x0098_090c);
        assert_eq!(Control::PowerLineFrequency.id(), 0x0098_0918);
        assert_eq!(Control::WhiteBalanceTemperature.id(), 0x0098_091a);
        assert_eq!(Control::Sharpness.id(), 0x0098_091b);
        assert_eq!(Control::PanAbsolute.id(), 0x009a_0908);
        assert_eq!(Control::TiltAbsolute.id(), 0x009a_0909);
        assert_eq!(Control::FocusAbsolute.id(), 0x009a_090a);
        assert_eq!(Control::FocusAuto.id(), 0x009a_090c);
        assert_eq!(Control::ZoomAbsolute.id(), 0x009a_090d);
    }
}
