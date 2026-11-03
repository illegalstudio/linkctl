//! Link 2C and 2C Pro digital crop positioning. Protocol evidence: docs/link2c-framing.md.

use super::link2::{self, guid, AI_MODE_STATUS, XU_IMAGE_UNIT, XU_INFO_UNIT};
use super::xu::XuControl;
use crate::camera::model::Model;
use crate::camera::Camera;
use crate::error::{Error, Result};

/// Official SDK SetHostPTZ: three LE uint16 values, followed by two step bytes.
pub const HOST_PTZ: XuControl = XuControl {
    unit: XU_IMAGE_UNIT,
    selector: 0x13,
    len: 8,
    name: "link2c_host_ptz",
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    pub x: u16,
    pub y: u16,
    pub zoom: u16,
    mode: u8,
}

impl Frame {
    pub fn normalized_x(self) -> f64 {
        f64::from(self.x) / f64::from(u16::MAX)
    }

    pub fn normalized_y(self) -> f64 {
        f64::from(self.y) / f64::from(u16::MAX)
    }

    pub fn zoom_factor(self) -> f64 {
        f64::from(self.zoom) / 100.0
    }

    /// Preserve exact raw coordinates when only one axis or zoom is changed.
    pub fn updated(self, x: Option<f64>, y: Option<f64>, zoom: Option<f64>) -> Result<Self> {
        let next = Self {
            x: x.map(|v| coordinate(v, "x")).transpose()?.unwrap_or(self.x),
            y: y.map(|v| coordinate(v, "y")).transpose()?.unwrap_or(self.y),
            zoom: zoom.map(zoom_value).transpose()?.unwrap_or(self.zoom),
            ..self
        };
        Ok(next)
    }

    fn payload(self) -> [u8; 8] {
        let mut bytes = [0; 8];
        bytes[0..2].copy_from_slice(&self.zoom.to_le_bytes());
        bytes[2..4].copy_from_slice(&self.x.to_le_bytes());
        bytes[4..6].copy_from_slice(&self.y.to_le_bytes());
        bytes[6] = 20;
        bytes[7] = 20;
        bytes
    }
}

pub fn coordinate(value: f64, axis: &str) -> Result<u16> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(Error::InvalidValue(format!(
            "frame {axis} must be between 0 and 1"
        )));
    }
    Ok((value * f64::from(u16::MAX)).round() as u16)
}

pub fn zoom_value(value: f64) -> Result<u16> {
    if !value.is_finite() || !(1.0..=4.0).contains(&value) {
        return Err(Error::InvalidValue(
            "frame zoom must be between 1 and 4".into(),
        ));
    }
    Ok((value * 100.0).round() as u16)
}

fn check_units(cam: &Camera) -> Result<()> {
    if !matches!(cam.info().model, Model::Link2C | Model::Link2CPro) {
        return Err(Error::UnsupportedControl(
            "digital framing (Link 2C and 2C Pro only)",
        ));
    }
    let descriptors = link2::read_descriptors(&cam.info().usb.sysfs_path)?;
    let units = link2::parse_extension_units(&descriptors);
    link2::confirm_unit(&units, XU_INFO_UNIT, &guid::INFO)?;
    link2::confirm_unit(&units, HOST_PTZ.unit, &guid::IMAGE)
}

fn decode_mode(bytes: &[u8], zoom: u16) -> Result<Frame> {
    // The SDK decodes these offsets only in the extended mode-state layout.
    // Special modes and the idle/transition sentinel must not become coordinates.
    if bytes.len() < 52 || !matches!(bytes[0], 0 | 1 | 7) {
        return Err(Error::Vendor(
            "digital framing state unavailable; use normal video mode with a live stream".into(),
        ));
    }
    if !(100..=400).contains(&zoom) {
        return Err(Error::Vendor(format!(
            "unexpected frame zoom readback: {zoom}"
        )));
    }
    Ok(Frame {
        x: u16::from_le_bytes([bytes[38], bytes[39]]),
        y: u16::from_le_bytes([bytes[40], bytes[41]]),
        zoom,
        mode: bytes[0],
    })
}

/// Zoom x100 from the Link 2C Pro mode payload (bytes 50-51, LE). On the
/// 2C Pro, V4L2 `zoom_absolute` readback misses zoom set through Host PTZ.
/// Verified in normal mode (0x00) only; `None` means use `zoom_absolute`.
fn payload_zoom(model: Model, bytes: &[u8]) -> Option<u16> {
    if model != Model::Link2CPro || bytes.len() < 52 || bytes[0] != 0 {
        return None;
    }
    Some(u16::from_le_bytes([bytes[50], bytes[51]]))
}

pub fn read(cam: &Camera) -> Result<Frame> {
    check_units(cam)?;
    let bytes = cam
        .device()
        .xu_read_reported(AI_MODE_STATUS.unit, AI_MODE_STATUS.selector)?;
    let zoom = match payload_zoom(cam.info().model, &bytes) {
        Some(zoom) => zoom,
        // Not `Camera::zoom_factor()`: it calls `read()` on the 2C Pro.
        None => {
            let zoom = cam.get_raw(crate::camera::controls::Control::ZoomAbsolute)?;
            u16::try_from(zoom)
                .map_err(|_| Error::Vendor(format!("unexpected frame zoom readback: {zoom}")))?
        }
    };
    decode_mode(&bytes, zoom)
}

/// Caller must pass the activity guard. The XU layer validates length and SET support.
pub fn write(cam: &Camera, frame: Frame) -> Result<()> {
    check_units(cam)?;
    cam.device().xu_write(&HOST_PTZ, &frame.payload())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(mode: u8) -> Frame {
        let mut bytes = [0; 56];
        bytes[0] = mode;
        bytes[38..42].copy_from_slice(&[0x34, 0x12, 0xcd, 0xab]);
        decode_mode(&bytes, 200).unwrap()
    }

    #[test]
    fn command_and_payload_match_sdk() {
        assert_eq!(
            (HOST_PTZ.unit, HOST_PTZ.selector, HOST_PTZ.len),
            (10, 0x13, 8)
        );
        assert_eq!(
            sample(0).payload(),
            [0xc8, 0, 0x34, 0x12, 0xcd, 0xab, 20, 20]
        );
    }

    #[test]
    fn reads_coordinates_from_mode_not_write_selector() {
        let frame = sample(0);
        assert_eq!((frame.x, frame.y, frame.zoom), (0x1234, 0xabcd, 200));
        assert_eq!(coordinate(frame.normalized_x(), "x").unwrap(), frame.x);
        assert_eq!(coordinate(frame.normalized_y(), "y").unwrap(), frame.y);
        assert_eq!(frame.zoom_factor(), 2.0);
    }

    #[test]
    fn partial_updates_preserve_other_raw_fields() {
        let current = sample(0);
        let next = current.updated(Some(0.75), None, None).unwrap();
        assert_eq!(next.x, 49151);
        assert_eq!((next.y, next.zoom), (current.y, current.zoom));
        let next = current.updated(None, None, Some(1.5)).unwrap();
        assert_eq!((next.x, next.y, next.zoom), (current.x, current.y, 150));
        let next = current.updated(Some(0.5), Some(0.5), None).unwrap();
        assert_eq!((next.x, next.y, next.zoom), (32768, 32768, 200));
    }

    #[test]
    fn rejects_bad_coordinates_and_zoom() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.01, 1.01] {
            assert!(sample(0).updated(Some(bad), None, None).is_err());
            assert!(sample(0).updated(None, Some(bad), None).is_err());
        }
        for bad in [f64::NAN, f64::INFINITY, -1.0, 0.99, 4.01] {
            assert!(sample(0).updated(None, None, Some(bad)).is_err());
        }
        assert_eq!(coordinate(0.0, "x").unwrap(), 0);
        assert_eq!(coordinate(1.0, "x").unwrap(), 65535);
        assert_eq!(zoom_value(1.0).unwrap(), 100);
        assert_eq!(zoom_value(4.0).unwrap(), 400);
    }

    #[test]
    fn rejects_short_special_and_idle_mode_payloads() {
        for len in 0..52 {
            assert!(decode_mode(&vec![0; len], 200).is_err());
        }
        for mode in [4, 5, 6, 0xff, 0xfe] {
            let mut bytes = [0; 56];
            bytes[0] = mode;
            assert!(decode_mode(&bytes, 200).is_err());
        }
        assert!(decode_mode(&[0; 56], 0).is_err());
        assert!(decode_mode(&[0; 56], 401).is_err());
    }

    /// Link 2C Pro mode payload as captured (docs/link2c-framing.md), with
    /// the X/Y bytes (38-41) and zoom bytes (50-51) under test.
    fn captured(xy: [u8; 4], zoom: [u8; 2]) -> [u8; 61] {
        let mut bytes = [0; 61];
        bytes[38..42].copy_from_slice(&xy);
        bytes[50..52].copy_from_slice(&zoom);
        bytes[52..56].copy_from_slice(&[0x3e, 0xfe, 0x22, 0x88]);
        bytes[60] = 1;
        bytes
    }

    fn decode_2c_pro(bytes: &[u8]) -> Result<Frame> {
        decode_mode(bytes, payload_zoom(Model::Link2CPro, bytes).unwrap())
    }

    #[test]
    fn decodes_captured_2c_pro_payloads() {
        let frame = decode_2c_pro(&captured([0xff, 0x7f, 0xff, 0x7f], [0x64, 0])).unwrap();
        assert_eq!((frame.x, frame.y, frame.zoom), (0x7fff, 0x7fff, 100));
        let frame = decode_2c_pro(&captured([0x8f, 0x42, 0x00, 0x80], [0xc8, 0])).unwrap();
        assert_eq!((frame.x, frame.y, frame.zoom), (0x428f, 0x8000, 200));
        let frame = decode_2c_pro(&captured([0x00, 0x80, 0x00, 0x80], [0x2c, 0x01])).unwrap();
        assert_eq!((frame.x, frame.y, frame.zoom), (0x8000, 0x8000, 300));
    }

    #[test]
    fn payload_zoom_only_for_2c_pro_in_normal_mode() {
        let baseline = captured([0xff, 0x7f, 0xff, 0x7f], [0x64, 0]);
        for len in 0..52 {
            assert_eq!(payload_zoom(Model::Link2CPro, &baseline[..len]), None);
        }
        assert_eq!(payload_zoom(Model::Link2CPro, &baseline[..52]), Some(100));
        for mode in [1, 7, 0xff] {
            let mut bytes = baseline;
            bytes[0] = mode;
            assert_eq!(payload_zoom(Model::Link2CPro, &bytes), None);
        }
        assert_eq!(payload_zoom(Model::Link2C, &baseline), None);
    }

    #[test]
    fn payload_zoom_range_is_enforced() {
        for (zoom, ok) in [(99u16, false), (100, true), (400, true), (401, false)] {
            let bytes = captured([0xff, 0x7f, 0xff, 0x7f], zoom.to_le_bytes());
            assert_eq!(decode_2c_pro(&bytes).is_ok(), ok, "zoom {zoom}");
        }
    }

    #[test]
    fn auto_framing_mode_uses_the_same_coordinate_layout() {
        for mode in [1, 7] {
            let frame = sample(mode);
            assert_eq!(frame.x, 0x1234);
            assert!(frame.updated(Some(0.5), None, None).is_ok());
        }
    }
}
