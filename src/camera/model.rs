//! Camera model identification from USB vendor/product ids.

use std::fmt;

/// USB vendor id used by Insta360 (Arashi Vision).
pub const INSTA360_VID: u16 = 0x2e1a;
/// USB product id of the Insta360 Link 2 (validated on real hardware).
pub const LINK2_PID: u16 = 0x4c04;
/// USB product id of the Insta360 Link 2C.
pub const LINK2C_PID: u16 = 0x4c03;
/// USB product id of the original Insta360 Link (recognised, **not** tested).
pub const LINK_PID: u16 = 0x4c01;

/// Supported (or at least recognised) camera models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Model {
    /// Insta360 Link 2 — the primary, hardware-validated target.
    Link2,
    /// Insta360 Link 2C - shares the Link 2 command protocol.
    Link2C,
    /// Original Insta360 Link — recognised by VID/PID only; untested.
    Link,
}

impl Model {
    /// Identify a model from a USB VID/PID pair.
    pub fn from_usb_ids(vid: u16, pid: u16) -> Option<Self> {
        if vid != INSTA360_VID {
            return None;
        }
        match pid {
            LINK2_PID => Some(Model::Link2),
            LINK2C_PID => Some(Model::Link2C),
            LINK_PID => Some(Model::Link),
            _ => None,
        }
    }

    /// Marketing name.
    pub fn name(&self) -> &'static str {
        match self {
            Model::Link2 => "Insta360 Link 2",
            Model::Link2C => "Insta360 Link 2C",
            Model::Link => "Insta360 Link",
        }
    }

    /// Whether pan/tilt controls represent a physical gimbal.
    pub fn has_gimbal(&self) -> bool {
        !matches!(self, Model::Link2C)
    }

    /// Whether this model uses the Link 2 vendor command protocol.
    pub fn supports_link2_protocol(&self) -> bool {
        matches!(self, Model::Link2 | Model::Link2C)
    }

    /// Whether this model has been validated against physical hardware.
    pub fn is_tested(&self) -> bool {
        matches!(self, Model::Link2)
    }
}

impl fmt::Display for Model {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_known_ids() {
        assert_eq!(Model::from_usb_ids(0x2e1a, 0x4c04), Some(Model::Link2));
        assert_eq!(Model::from_usb_ids(0x2e1a, 0x4c03), Some(Model::Link2C));
        assert_eq!(Model::from_usb_ids(0x2e1a, 0x4c01), Some(Model::Link));
        assert_eq!(Model::from_usb_ids(0x046d, 0x4c03), None);
        assert_eq!(Model::from_usb_ids(0x2e1a, 0x0000), None);
        assert_eq!(Model::from_usb_ids(0x046d, 0x4c04), None);
    }

    #[test]
    fn link2c_shares_link2_protocol() {
        assert!(Model::Link2.supports_link2_protocol());
        assert!(Model::Link2C.supports_link2_protocol());
        assert!(!Model::Link.supports_link2_protocol());
        assert_eq!(Model::Link2C.name(), "Insta360 Link 2C");
        assert_eq!(Model::Link2C.to_string(), "Insta360 Link 2C");
        assert!(!Model::Link2C.is_tested());
        assert!(!Model::Link2C.has_gimbal());
        assert!(Model::Link2.has_gimbal());
        assert!(Model::Link.has_gimbal());
    }

    #[test]
    fn only_link2_is_tested() {
        assert!(Model::Link2.is_tested());
        assert!(!Model::Link.is_tested());
        assert_eq!(Model::Link2.name(), "Insta360 Link 2");
    }
}
