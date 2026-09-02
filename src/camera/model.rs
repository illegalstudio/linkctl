//! Camera model identification from USB vendor/product ids.

use std::fmt;

/// USB vendor id used by Insta360 (Arashi Vision).
pub const INSTA360_VID: u16 = 0x2e1a;
/// USB product id of the Insta360 Link 2 (validated on real hardware).
pub const LINK2_PID: u16 = 0x4c04;
/// USB product id of the original Insta360 Link (recognised, **not** tested).
pub const LINK_PID: u16 = 0x4c01;

/// Supported (or at least recognised) camera models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Model {
    /// Insta360 Link 2 — the primary, hardware-validated target.
    Link2,
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
            LINK_PID => Some(Model::Link),
            _ => None,
        }
    }

    /// Marketing name.
    pub fn name(&self) -> &'static str {
        match self {
            Model::Link2 => "Insta360 Link 2",
            Model::Link => "Insta360 Link",
        }
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
        assert_eq!(Model::from_usb_ids(0x2e1a, 0x4c01), Some(Model::Link));
        assert_eq!(Model::from_usb_ids(0x2e1a, 0x0000), None);
        assert_eq!(Model::from_usb_ids(0x046d, 0x4c04), None);
    }

    #[test]
    fn only_link2_is_tested() {
        assert!(Model::Link2.is_tested());
        assert!(!Model::Link.is_tested());
        assert_eq!(Model::Link2.name(), "Insta360 Link 2");
    }
}
