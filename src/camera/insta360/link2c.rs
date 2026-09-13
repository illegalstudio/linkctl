//! Link 2C digital crop positioning. Protocol evidence: docs/link2c-framing.md.

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
    if cam.info().model != Model::Link2C {
        return Err(Error::UnsupportedControl("digital framing (Link 2C only)"));
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

pub fn read(cam: &Camera) -> Result<Frame> {
    check_units(cam)?;
    let bytes = cam
        .device()
        .xu_read_reported(AI_MODE_STATUS.unit, AI_MODE_STATUS.selector)?;
    let zoom = cam.get_raw(crate::camera::controls::Control::ZoomAbsolute)?;
    let zoom = u16::try_from(zoom)
        .map_err(|_| Error::Vendor(format!("unexpected frame zoom readback: {zoom}")))?;
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

    #[test]
    fn auto_framing_mode_uses_the_same_coordinate_layout() {
        for mode in [1, 7] {
            let frame = sample(mode);
            assert_eq!(frame.x, 0x1234);
            assert!(frame.updated(Some(0.5), None, None).is_ok());
        }
    }
}
