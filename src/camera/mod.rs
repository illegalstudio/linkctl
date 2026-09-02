//! Camera model: device information, standard V4L2 controls and (later)
//! vendor extension units.
//!
//! ```text
//! Camera
//!  ├── DeviceInfo            (discovery.rs — sysfs + QUERYCAP)
//!  ├── standard controls     (controls.rs + v4l2.rs)
//!  └── vendor XU controls    (insta360/)
//! ```

pub mod activity;
pub mod controls;
pub mod discovery;
pub mod insta360;
pub mod model;
pub mod v4l2;

use std::path::Path;

use self::controls::Control;
use self::discovery::DeviceInfo;
use self::v4l2::{ControlInfo, V4l2Device};
use crate::error::{Error, Result};
use crate::units::{self, Range};

/// An Insta360 Link camera with its control node open.
pub struct Camera {
    info: DeviceInfo,
    dev: V4l2Device,
}

impl Camera {
    /// Open the control node of a discovered camera.
    pub fn open(info: DeviceInfo) -> Result<Self> {
        let dev = V4l2Device::open(&info.control_node)?;
        Ok(Self { info, dev })
    }

    pub fn info(&self) -> &DeviceInfo {
        &self.info
    }

    pub fn device(&self) -> &V4l2Device {
        &self.dev
    }

    // --- generic control access -------------------------------------------

    /// Query a control's metadata; `UnsupportedControl` if absent.
    pub fn query(&self, control: Control) -> Result<ControlInfo> {
        self.dev
            .query_control(control.id())?
            .ok_or(Error::UnsupportedControl(control.name()))
    }

    /// Query a control, returning `None` if unsupported.
    pub fn try_query(&self, control: Control) -> Result<Option<ControlInfo>> {
        self.dev.query_control(control.id())
    }

    pub fn supports(&self, control: Control) -> bool {
        matches!(self.try_query(control), Ok(Some(_)))
    }

    /// Read a raw control value.
    pub fn get_raw(&self, control: Control) -> Result<i64> {
        match self.dev.get_control(control.id()) {
            Ok(v) => Ok(v as i64),
            Err(Error::Io(msg)) if msg.contains("EINVAL") || msg.contains("Invalid argument") => {
                Err(Error::UnsupportedControl(control.name()))
            }
            Err(e) => Err(e),
        }
    }

    /// Read a raw value if the control exists, else `None`.
    pub fn try_get_raw(&self, control: Control) -> Result<Option<i64>> {
        match self.try_query(control)? {
            Some(_) => self.get_raw(control).map(Some),
            None => Ok(None),
        }
    }

    /// Validate a raw value for `control` without writing.
    pub fn check_raw(&self, control: Control, value: i64) -> Result<i64> {
        let info = self.query(control)?;
        if info.is_read_only() {
            return Err(Error::InvalidValue(format!(
                "{} is read-only on this camera.",
                control.name()
            )));
        }
        info.range()
            .validate(value, control.name(), "")
            .map_err(Error::InvalidValue)
    }

    /// Write a raw control value after validating it against the control's
    /// range. Returns the value actually written (snapped to the step).
    pub fn set_raw(&self, control: Control, value: i64) -> Result<i64> {
        let value = self.check_raw(control, value)?;
        let v = i32::try_from(value)
            .map_err(|_| Error::InvalidValue(format!("{value} does not fit a 32-bit control")))?;
        self.dev.set_control(control.id(), v)?;
        Ok(value)
    }

    pub fn set_bool(&self, control: Control, on: bool) -> Result<()> {
        self.set_raw(control, i64::from(on)).map(|_| ())
    }

    // --- pan / tilt / zoom in human units ---------------------------------

    pub fn pan_range(&self) -> Result<Range> {
        Ok(self.query(Control::PanAbsolute)?.range())
    }

    pub fn tilt_range(&self) -> Result<Range> {
        Ok(self.query(Control::TiltAbsolute)?.range())
    }

    pub fn zoom_range(&self) -> Result<Range> {
        Ok(self.query(Control::ZoomAbsolute)?.range())
    }

    pub fn pan_degrees(&self) -> Result<f64> {
        Ok(units::arcsec_to_degrees(
            self.get_raw(Control::PanAbsolute)?,
        ))
    }

    pub fn tilt_degrees(&self) -> Result<f64> {
        Ok(units::arcsec_to_degrees(
            self.get_raw(Control::TiltAbsolute)?,
        ))
    }

    pub fn zoom_factor(&self) -> Result<f64> {
        Ok(units::raw_to_zoom(self.get_raw(Control::ZoomAbsolute)?))
    }

    /// Validate a pan angle against the device range without writing.
    /// Returns the raw value that would be written.
    pub fn check_pan_degrees(&self, degrees: f64) -> Result<i64> {
        self.check_angle(Control::PanAbsolute, "Pan", degrees)
    }

    pub fn check_tilt_degrees(&self, degrees: f64) -> Result<i64> {
        self.check_angle(Control::TiltAbsolute, "Tilt", degrees)
    }

    /// Set pan in degrees; out-of-range values are rejected. Returns the
    /// degrees actually applied after snapping to the control step.
    pub fn set_pan_degrees(&self, degrees: f64) -> Result<f64> {
        let raw = self.check_pan_degrees(degrees)?;
        self.dev
            .set_control(Control::PanAbsolute.id(), raw as i32)?;
        Ok(units::arcsec_to_degrees(raw))
    }

    pub fn set_tilt_degrees(&self, degrees: f64) -> Result<f64> {
        let raw = self.check_tilt_degrees(degrees)?;
        self.dev
            .set_control(Control::TiltAbsolute.id(), raw as i32)?;
        Ok(units::arcsec_to_degrees(raw))
    }

    fn check_angle(&self, control: Control, what: &str, degrees: f64) -> Result<i64> {
        let info = self.query(control)?;
        let raw = units::degrees_to_arcsec(degrees);
        let range = info.range();
        if !range.contains(raw) {
            return Err(Error::InvalidValue(format!(
                "{what} {} is out of range ({} .. {}).",
                units::format_degrees(degrees),
                units::format_degrees(units::arcsec_to_degrees(range.min)),
                units::format_degrees(units::arcsec_to_degrees(range.max)),
            )));
        }
        Ok(range.clamp_and_snap(raw))
    }

    /// Move pan by `delta` degrees relative to the current position,
    /// clamping at the limits. Returns the new absolute pan in degrees.
    pub fn pan_relative(&self, delta_degrees: f64) -> Result<f64> {
        self.move_relative(Control::PanAbsolute, delta_degrees)
    }

    pub fn tilt_relative(&self, delta_degrees: f64) -> Result<f64> {
        self.move_relative(Control::TiltAbsolute, delta_degrees)
    }

    fn move_relative(&self, control: Control, delta_degrees: f64) -> Result<f64> {
        let info = self.query(control)?;
        let current = self.get_raw(control)?;
        let target = units::relative_target(
            current,
            units::degrees_to_arcsec(delta_degrees),
            info.range(),
        );
        self.dev.set_control(control.id(), target as i32)?;
        Ok(units::arcsec_to_degrees(target))
    }

    /// Validate a zoom multiplier without writing; returns the raw value.
    pub fn check_zoom_factor(&self, zoom: f64) -> Result<i64> {
        let info = self.query(Control::ZoomAbsolute)?;
        let raw = units::zoom_to_raw(zoom);
        let range = info.range();
        if !range.contains(raw) {
            return Err(Error::InvalidValue(format!(
                "Zoom {} is out of range ({} .. {}).",
                units::format_zoom(zoom),
                units::format_zoom(units::raw_to_zoom(range.min)),
                units::format_zoom(units::raw_to_zoom(range.max)),
            )));
        }
        Ok(range.clamp_and_snap(raw))
    }

    /// Set zoom as a multiplier (`1.0` .. `4.0` on the Link 2).
    pub fn set_zoom_factor(&self, zoom: f64) -> Result<f64> {
        let raw = self.check_zoom_factor(zoom)?;
        self.dev
            .set_control(Control::ZoomAbsolute.id(), raw as i32)?;
        Ok(units::raw_to_zoom(raw))
    }

    // --- focus / white balance --------------------------------------------

    pub fn set_focus_auto(&self) -> Result<()> {
        self.set_bool(Control::FocusAuto, true)
    }

    /// Manual focus: disable continuous autofocus first, then set the value.
    pub fn set_focus_manual(&self, value: i64) -> Result<i64> {
        if self.supports(Control::FocusAuto) {
            self.set_bool(Control::FocusAuto, false)?;
        }
        self.set_raw(Control::FocusAbsolute, value)
    }

    pub fn set_white_balance_auto(&self) -> Result<()> {
        self.set_bool(Control::WhiteBalanceAuto, true)
    }

    /// Manual white balance: disable auto first, then set the temperature.
    pub fn set_white_balance_temperature(&self, kelvin: i64) -> Result<i64> {
        if self.supports(Control::WhiteBalanceAuto) {
            self.set_bool(Control::WhiteBalanceAuto, false)?;
        }
        self.set_raw(Control::WhiteBalanceTemperature, kelvin)
    }
}

/// Device ids (`st_rdev`) of all nodes whose being open means "active".
/// Includes every video node of the camera, so a metadata-only consumer is
/// also treated as activity (it keeps the camera powered anyway).
pub fn activity_ids(info: &DeviceInfo) -> Vec<activity::DeviceId> {
    let mut ids = Vec::new();
    for node in &info.video_nodes {
        if let Ok(id) = activity::DeviceId::of_path(&node.path) {
            ids.push(id);
        }
    }
    if ids.is_empty() {
        if let Ok(id) = activity::DeviceId::of_path(Path::new(&info.control_node)) {
            ids.push(id);
        }
    }
    ids
}

/// Scan for activity on the given camera (without opening it).
pub fn check_activity(info: &DeviceInfo) -> activity::Activity {
    activity::scan(&activity_ids(info), std::process::id())
}
