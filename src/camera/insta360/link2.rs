//! Insta360 Link 2 vendor extension units: documented constants and the
//! AI-tracking control.
//!
//! Everything here is reverse-engineered knowledge, not an official API.
//! Sources (see `docs/research.md`):
//!
//! * fmontes/insta360-link-cli (MIT) — Link 2 XU GUIDs from the USB
//!   descriptor; XU 11 selector 0x02, 1 byte, `0x01` on / `0x00` off,
//!   verified on a Link 2.
//! * fugisawa/insta360-link-ctl (MIT) — same selector on the original Link
//!   via `UVCIOC_CTRL_QUERY`; notes that vendor writes are silently reverted
//!   by the firmware unless video is streaming.
//! * csmarshall/link-ctl (MIT) — XU 9 selector 0x02 is a 61-byte "AI mode"
//!   payload on the Link 2 with a fragile multi-step write protocol. We only
//!   *read* it (byte 0 = mode) for diagnostics and never write it.
//!
//! Policy: read first; write only [`AI_TRACKING`], only after the unit's
//! GUID has been confirmed from the USB descriptors, only after `GET_LEN`
//! and `GET_INFO` agree, and always followed by a read-back.

use super::xu::XuControl;
use crate::camera::v4l2::V4l2Device;
use crate::error::{Error, Result};

/// UVC extension unit GUIDs from the Link 2 USB descriptor (little-endian
/// byte order as they appear on the wire).
pub mod guid {
    /// Unit 9: device information / mode & status.
    pub const INFO: [u8; 16] = uuid_le(0xFAF1672D, 0xB71B, 0x4793, 0x8C91, 0x7B1C9B7F95F8);
    /// Unit 10: image / auto-exposure parameters.
    pub const IMAGE: [u8; 16] = uuid_le(0xE307E649, 0x4618, 0xA3FF, 0x82FC, 0x2D8B5F216773);
    /// Unit 11: AI features.
    pub const AI: [u8; 16] = uuid_le(0xA8BD5DF2, 0x1A98, 0x474E, 0x8DD0, 0xD92672D194FA);

    /// Encode a textual UUID (`d1-d2-d3-d4-d5`) into the USB/UVC on-wire
    /// layout: `d1`, `d2`, `d3` little-endian, `d4` and `d5` big-endian.
    pub const fn uuid_le(d1: u32, d2: u16, d3: u16, d4: u16, d5: u64) -> [u8; 16] {
        let a = d1.to_le_bytes();
        let b = d2.to_le_bytes();
        let c = d3.to_le_bytes();
        let d = d4.to_be_bytes();
        let e = d5.to_be_bytes();
        [
            a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], d[0], d[1], e[2], e[3], e[4], e[5],
            e[6], e[7],
        ]
    }
}

/// Extension unit ids as enumerated by the Link 2 descriptor. These are the
/// *expected* ids; [`resolve_units`] confirms them against the descriptor.
pub const XU_INFO_UNIT: u8 = 9;
#[allow(dead_code)] // documented for completeness; nothing on unit 10 is used yet
pub const XU_IMAGE_UNIT: u8 = 10;
pub const XU_AI_UNIT: u8 = 11;

/// AI subject tracking on/off. Unit 11 (AI), selector 0x02, 1 byte.
/// Payload: `0x01` = tracking on, `0x00` = off. Readable and writable.
pub const AI_TRACKING: XuControl = XuControl {
    unit: XU_AI_UNIT,
    selector: 0x02,
    len: 1,
    name: "ai_tracking",
};

/// AI mode / status on unit 9, selector 0x02.
/// Byte 0: 0x00 normal, 0x01 tracking, 0x04 whiteboard, 0x05 overhead,
/// 0x06 deskview, 0xFF idle/transition. **Read-only in linkctl.**
///
/// Length: csmarshall/link-ctl documents 61 bytes on the Link 2; the
/// development camera (firmware as of 2026-09) reports **60** via `GET_LEN`.
/// Because the length is firmware-dependent, this control is only ever read
/// with the device-reported length and is never written.
pub const AI_MODE_STATUS: XuControl = XuControl {
    unit: XU_INFO_UNIT,
    selector: 0x02,
    len: 60,
    name: "ai_mode_status",
};

/// Decoded byte 0 of [`AI_MODE_STATUS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AiMode {
    Normal,
    Tracking,
    Whiteboard,
    Overhead,
    DeskView,
    Idle,
    Unknown(u8),
}

impl AiMode {
    pub fn from_byte(b: u8) -> Self {
        match b {
            0x00 => AiMode::Normal,
            0x01 => AiMode::Tracking,
            0x04 => AiMode::Whiteboard,
            0x05 => AiMode::Overhead,
            0x06 => AiMode::DeskView,
            0xFF => AiMode::Idle,
            other => AiMode::Unknown(other),
        }
    }

    pub fn label(&self) -> String {
        match self {
            AiMode::Normal => "normal".into(),
            AiMode::Tracking => "tracking".into(),
            AiMode::Whiteboard => "whiteboard".into(),
            AiMode::Overhead => "overhead".into(),
            AiMode::DeskView => "deskview".into(),
            AiMode::Idle => "idle".into(),
            AiMode::Unknown(b) => format!("unknown (0x{b:02x})"),
        }
    }
}

/// An extension unit found in the USB descriptor.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ExtensionUnit {
    pub id: u8,
    pub guid: String,
    pub num_controls: u8,
    pub role: Option<&'static str>,
}

/// Parse the USB configuration descriptors (`<usb device>/descriptors` in
/// sysfs) and list the UVC extension units. Read-only.
///
/// Descriptor layout (UVC 1.5 §3.7.2.7): `bLength, bDescriptorType=0x24
/// (CS_INTERFACE), bDescriptorSubtype=0x06 (VC_EXTENSION_UNIT), bUnitID,
/// guidExtensionCode[16], bNumControls, ...`.
pub fn parse_extension_units(descriptors: &[u8]) -> Vec<ExtensionUnit> {
    const CS_INTERFACE: u8 = 0x24;
    const VC_EXTENSION_UNIT: u8 = 0x06;
    let mut units = Vec::new();
    let mut i = 0usize;
    while i + 2 <= descriptors.len() {
        let len = usize::from(descriptors[i]);
        if len < 2 || i + len > descriptors.len() {
            break;
        }
        let d = &descriptors[i..i + len];
        if d[1] == CS_INTERFACE && d[2] == VC_EXTENSION_UNIT && len >= 21 {
            let mut guid = [0u8; 16];
            guid.copy_from_slice(&d[4..20]);
            units.push(ExtensionUnit {
                id: d[3],
                guid: format_guid(&guid),
                num_controls: d[20],
                role: role_for_guid(&guid),
            });
        }
        i += len;
    }
    units
}

fn role_for_guid(g: &[u8; 16]) -> Option<&'static str> {
    if *g == guid::INFO {
        Some("info/mode")
    } else if *g == guid::IMAGE {
        Some("image")
    } else if *g == guid::AI {
        Some("ai")
    } else {
        None
    }
}

/// Format an on-wire GUID in the usual `XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX`
/// textual form.
pub fn format_guid(g: &[u8; 16]) -> String {
    format!(
        "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        g[3], g[2], g[1], g[0], g[5], g[4], g[7], g[6], g[8], g[9], g[10], g[11], g[12], g[13], g[14], g[15]
    )
}

/// Read the descriptor blob for a USB device from sysfs.
pub fn read_descriptors(usb_sysfs_path: &std::path::Path) -> Result<Vec<u8>> {
    let p = usb_sysfs_path.join("descriptors");
    std::fs::read(&p).map_err(|e| Error::Io(format!("reading {}: {e}", p.display())))
}

/// Confirm that the unit id we intend to talk to carries the expected GUID.
/// Refuses (rather than guessing) when the descriptor disagrees.
pub fn confirm_unit(
    units: &[ExtensionUnit],
    expected_id: u8,
    expected_guid: &[u8; 16],
) -> Result<()> {
    let want = format_guid(expected_guid);
    match units.iter().find(|u| u.id == expected_id) {
        Some(u) if u.guid == want => Ok(()),
        Some(u) => Err(Error::Vendor(format!(
            "extension unit {expected_id} has GUID {} but {want} was expected; refusing to use it",
            u.guid
        ))),
        None => Err(Error::Vendor(format!(
            "extension unit {expected_id} ({want}) not present in the USB descriptor"
        ))),
    }
}

/// AI tracking state as read from the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackingState {
    pub enabled: bool,
    /// Raw byte, in case the firmware reports something other than 0/1.
    pub raw: u8,
}

/// Read AI tracking state (XU 11 / 0x02). Read-only; safe while inactive.
pub fn read_tracking(dev: &V4l2Device) -> Result<TrackingState> {
    let payload = dev.xu_read(&AI_TRACKING)?;
    let raw = payload[0];
    Ok(TrackingState {
        enabled: raw == 0x01,
        raw,
    })
}

/// Write AI tracking state and read it back. Returns the state reported by
/// the device after the write, so callers can detect a firmware that
/// ignored the request (which happens when no video stream is running).
pub fn write_tracking(dev: &V4l2Device, enabled: bool) -> Result<TrackingState> {
    // Read-modify-write: the payload is a single byte, so "modify" is the
    // whole payload, but reading first still validates GET_LEN and that the
    // control is reachable before we attempt a SET.
    let mut payload = dev.xu_read(&AI_TRACKING)?;
    payload[0] = u8::from(enabled);
    dev.xu_write(&AI_TRACKING, &payload)?;
    read_tracking(dev)
}

/// Read byte 0 of the unit-9 AI mode payload, using the device-reported
/// length. Read-only. Returns the decoded mode and the payload length.
pub fn read_ai_mode(dev: &V4l2Device) -> Result<(AiMode, usize)> {
    let payload = dev.xu_read_reported(AI_MODE_STATUS.unit, AI_MODE_STATUS.selector)?;
    Ok((AiMode::from_byte(payload[0]), payload.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guid_encoding_round_trips() {
        assert_eq!(
            format_guid(&guid::INFO),
            "FAF1672D-B71B-4793-8C91-7B1C9B7F95F8"
        );
        assert_eq!(
            format_guid(&guid::IMAGE),
            "E307E649-4618-A3FF-82FC-2D8B5F216773"
        );
        assert_eq!(
            format_guid(&guid::AI),
            "A8BD5DF2-1A98-474E-8DD0-D92672D194FA"
        );
    }

    #[test]
    fn constants_are_documented_values() {
        assert_eq!(AI_TRACKING.unit, 11);
        assert_eq!(AI_TRACKING.selector, 0x02);
        assert_eq!(AI_TRACKING.len, 1);
        assert_eq!(AI_MODE_STATUS.unit, 9);
        assert_eq!(AI_MODE_STATUS.len, 60);
    }

    #[test]
    fn ai_mode_decoding() {
        assert_eq!(AiMode::from_byte(0x00), AiMode::Normal);
        assert_eq!(AiMode::from_byte(0x01), AiMode::Tracking);
        assert_eq!(AiMode::from_byte(0x06), AiMode::DeskView);
        assert_eq!(AiMode::from_byte(0xFF), AiMode::Idle);
        assert_eq!(AiMode::from_byte(0x42), AiMode::Unknown(0x42));
        assert_eq!(AiMode::Unknown(0x42).label(), "unknown (0x42)");
    }

    fn xu_descriptor(id: u8, guid: &[u8; 16], num_controls: u8) -> Vec<u8> {
        let mut d = vec![0u8, 0x24, 0x06, id];
        d.extend_from_slice(guid);
        d.push(num_controls);
        d.extend_from_slice(&[1, 4, 0xff, 0xff, 0xff, 0x3f, 0]); // bNrInPins.. iExtension
        d[0] = d.len() as u8;
        d
    }

    #[test]
    fn parses_extension_units_from_descriptor_blob() {
        let mut blob = vec![9, 0x02, 0, 0, 1, 1, 0, 0x80, 0xfa]; // config descriptor
        blob.extend_from_slice(&[9, 0x04, 0, 0, 1, 0x0e, 0x01, 0x00, 0]); // interface
        blob.extend(xu_descriptor(9, &guid::INFO, 30));
        blob.extend(xu_descriptor(11, &guid::AI, 5));
        blob.extend(xu_descriptor(10, &guid::IMAGE, 6));
        // Trailing garbage with an impossible length must not panic.
        blob.extend_from_slice(&[0xff, 0x24]);
        let units = parse_extension_units(&blob);
        assert_eq!(units.len(), 3);
        assert_eq!(units[0].id, 9);
        assert_eq!(units[0].role, Some("info/mode"));
        assert_eq!(units[0].num_controls, 30);
        assert_eq!(units[1].id, 11);
        assert_eq!(units[1].guid, "A8BD5DF2-1A98-474E-8DD0-D92672D194FA");
        assert_eq!(units[1].role, Some("ai"));
        assert_eq!(units[2].role, Some("image"));

        assert!(confirm_unit(&units, 11, &guid::AI).is_ok());
        assert!(confirm_unit(&units, 11, &guid::INFO).is_err());
        assert!(confirm_unit(&units, 12, &guid::AI).is_err());
    }

    #[test]
    fn empty_or_truncated_blob() {
        assert!(parse_extension_units(&[]).is_empty());
        assert!(parse_extension_units(&[5, 0x24, 0x06]).is_empty());
    }
}
