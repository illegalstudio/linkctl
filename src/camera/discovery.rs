//! Device discovery through sysfs and `VIDIOC_QUERYCAP`.
//!
//! Strategy (no `lsusb`, `udevadm` or `v4l2-ctl`):
//!
//! 1. Enumerate `/sys/class/video4linux/video*`.
//! 2. For each node, resolve the `device` symlink to the USB *interface*
//!    directory (`.../1-1:1.0`) and its parent USB *device* (`.../1-1`), and
//!    read `idVendor` / `idProduct` from the latter.
//! 3. Only nodes whose USB ids match a known Insta360 model are opened.
//!    Those are classified with `VIDIOC_QUERYCAP`: the node with
//!    `V4L2_CAP_VIDEO_CAPTURE` is both the control and the streaming node;
//!    metadata-only nodes are recorded but never used for control.
//! 4. Nodes are grouped by USB device so a camera with several nodes
//!    (`/dev/video0` + `/dev/video1` on the Link 2) appears once.
//!
//! Non-Insta360 video nodes are never opened.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::model::Model;
use super::v4l2::V4l2Device;
use crate::error::{Error, Result};

/// USB-level identification of a camera, read from sysfs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UsbInfo {
    pub vendor_id: u16,
    pub product_id: u16,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial: Option<String>,
    /// Bus number (`busnum`).
    pub bus: Option<u32>,
    /// Device address on the bus (`devnum`).
    pub address: Option<u32>,
    /// USB port path, e.g. `1-1` or `3-2.4` (the sysfs device name).
    pub port_path: String,
    /// Absolute sysfs path of the USB device.
    pub sysfs_path: PathBuf,
    /// Kernel driver bound to the video interface (normally `uvcvideo`).
    pub driver: Option<String>,
}

/// A single `/dev/videoN` node and its classification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VideoNode {
    pub path: PathBuf,
    pub role: NodeRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeRole {
    /// Video capture node: exposes the UVC controls and the video stream.
    Capture,
    /// UVC metadata node: no controls, no video.
    Metadata,
    /// Something else (or could not be opened to classify).
    Unknown,
}

/// Everything known about a discovered camera before opening it for control.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceInfo {
    pub model: Model,
    pub usb: UsbInfo,
    /// Node used for control ioctls.
    pub control_node: PathBuf,
    /// Node applications open to stream video (same as control on Link 2).
    pub stream_node: PathBuf,
    /// All video nodes belonging to this camera.
    pub video_nodes: Vec<VideoNode>,
    /// Media controller node (`/dev/mediaN`), if any.
    pub media_node: Option<PathBuf>,
    /// `card` string from `VIDIOC_QUERYCAP`.
    pub card: String,
    /// `bus_info` string from `VIDIOC_QUERYCAP`.
    pub bus_info: String,
    /// Kernel/driver version from `VIDIOC_QUERYCAP`.
    pub driver_version: String,
}

impl DeviceInfo {
    pub fn summary(&self) -> DeviceSummary {
        DeviceSummary {
            model: self.model,
            control_node: self.control_node.clone(),
            vendor_id: self.usb.vendor_id,
            product_id: self.usb.product_id,
        }
    }
}

/// Compact description used in listings and errors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceSummary {
    pub model: Model,
    pub control_node: PathBuf,
    pub vendor_id: u16,
    pub product_id: u16,
}

const SYSFS_V4L: &str = "/sys/class/video4linux";

/// Discover all supported cameras.
pub fn discover_all() -> Result<Vec<DeviceInfo>> {
    discover_in(Path::new(SYSFS_V4L), Path::new("/dev"))
}

/// Discover the camera behind an explicit `/dev/videoN` (or any node of it).
pub fn discover_device(dev_path: &Path) -> Result<DeviceInfo> {
    let sysfs_node = sysfs_for_dev(dev_path)?;
    let raw = RawNode::read(&sysfs_node, Path::new("/dev"))
        .ok_or_else(|| Error::UnsupportedDevice(dev_path.to_path_buf()))?;
    let Some(model) = raw.model() else {
        return Err(Error::UnsupportedDevice(dev_path.to_path_buf()));
    };
    // Gather every node of the same USB device so that the info is complete
    // even when the user pointed us at the metadata node.
    let all = discover_all()?;
    if let Some(found) = all
        .into_iter()
        .find(|d| d.usb.sysfs_path == raw.usb.sysfs_path)
    {
        return Ok(found);
    }
    // Fallback: build from this node alone (e.g. sysfs listing unavailable).
    build_device(model, raw.usb.clone(), vec![raw])
        .ok_or_else(|| Error::UnsupportedDevice(dev_path.to_path_buf()))
}

/// Exactly-one semantics used by every command that needs a camera.
pub fn select(explicit: Option<&Path>) -> Result<DeviceInfo> {
    if let Some(p) = explicit {
        return discover_device(p);
    }
    let mut all = discover_all()?;
    match all.len() {
        0 => Err(Error::CameraNotFound),
        1 => Ok(all.remove(0)),
        _ => Err(Error::MultipleCameras(
            all.iter().map(DeviceInfo::summary).collect(),
        )),
    }
}

fn discover_in(sysfs_v4l: &Path, dev_root: &Path) -> Result<Vec<DeviceInfo>> {
    let mut by_usb: BTreeMap<PathBuf, (UsbInfo, Vec<RawNode>)> = BTreeMap::new();
    let entries = match fs::read_dir(sysfs_v4l) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error::Io(format!("reading {}: {e}", sysfs_v4l.display()))),
    };
    for entry in entries.flatten() {
        let Some(raw) = RawNode::read(&entry.path(), dev_root) else {
            continue;
        };
        if raw.model().is_none() {
            continue;
        }
        by_usb
            .entry(raw.usb.sysfs_path.clone())
            .or_insert_with(|| (raw.usb.clone(), Vec::new()))
            .1
            .push(raw);
    }
    let mut devices = Vec::new();
    for (_, (usb, nodes)) in by_usb {
        let model = Model::from_usb_ids(usb.vendor_id, usb.product_id).expect("filtered above");
        if let Some(d) = build_device(model, usb, nodes) {
            devices.push(d);
        }
    }
    devices.sort_by(|a, b| a.control_node.cmp(&b.control_node));
    Ok(devices)
}

/// Open each node, classify it, and pick the control node.
fn build_device(model: Model, usb: UsbInfo, mut nodes: Vec<RawNode>) -> Option<DeviceInfo> {
    nodes.sort_by(|a, b| a.dev_path.cmp(&b.dev_path));
    let mut video_nodes = Vec::new();
    let mut control: Option<(PathBuf, super::v4l2::Capabilities)> = None;
    for n in &nodes {
        let (role, caps) = classify(&n.dev_path);
        if role == NodeRole::Capture && control.is_none() {
            control = Some((n.dev_path.clone(), caps.expect("capture implies caps")));
        }
        video_nodes.push(VideoNode {
            path: n.dev_path.clone(),
            role,
        });
    }
    let (control_node, caps) = control?;
    let media_node = nodes.iter().find_map(|n| n.media_node.clone());
    let (maj, min, patch) = caps.version_tuple();
    Some(DeviceInfo {
        model,
        usb,
        stream_node: control_node.clone(),
        control_node,
        video_nodes,
        media_node,
        card: caps.card,
        bus_info: caps.bus_info,
        driver_version: format!("{maj}.{min}.{patch}"),
    })
}

fn classify(path: &Path) -> (NodeRole, Option<super::v4l2::Capabilities>) {
    match V4l2Device::open_read_only(path).and_then(|d| d.query_capabilities()) {
        Ok(caps) if caps.is_video_capture() => (NodeRole::Capture, Some(caps)),
        Ok(caps) if caps.is_metadata() => (NodeRole::Metadata, Some(caps)),
        Ok(caps) => (NodeRole::Unknown, Some(caps)),
        Err(_) => (NodeRole::Unknown, None),
    }
}

/// A video4linux sysfs entry with its resolved USB parent.
#[derive(Debug, Clone)]
struct RawNode {
    dev_path: PathBuf,
    media_node: Option<PathBuf>,
    usb: UsbInfo,
}

impl RawNode {
    fn model(&self) -> Option<Model> {
        Model::from_usb_ids(self.usb.vendor_id, self.usb.product_id)
    }

    /// Read `/sys/class/video4linux/videoN`. Returns `None` for nodes that
    /// are not USB-backed or whose sysfs attributes are unreadable.
    fn read(sysfs_node: &Path, dev_root: &Path) -> Option<Self> {
        let name = sysfs_node.file_name()?.to_str()?.to_string();
        if !name.starts_with("video") {
            return None;
        }
        let dev_path = dev_root.join(&name);
        // `device` -> USB interface dir; its parent is the USB device.
        let iface = fs::canonicalize(sysfs_node.join("device")).ok()?;
        let usb = usb_info_for_interface(&iface)?;
        let media_node = fs::read_dir(&iface).ok().and_then(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .find(|n| n.starts_with("media"))
                .map(|n| dev_root.join(n))
        });
        Some(Self {
            dev_path,
            media_node,
            usb,
        })
    }
}

/// Walk up from an interface directory to the USB device and read its ids.
/// Returns `None` if this is not a USB device (no `idVendor`).
pub(crate) fn usb_info_for_interface(iface: &Path) -> Option<UsbInfo> {
    let usb_dev = iface.parent()?;
    let vendor_id = read_hex_u16(&usb_dev.join("idVendor"))?;
    let product_id = read_hex_u16(&usb_dev.join("idProduct"))?;
    let driver = fs::read_link(iface.join("driver"))
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
    Some(UsbInfo {
        vendor_id,
        product_id,
        manufacturer: read_trimmed(&usb_dev.join("manufacturer")),
        product: read_trimmed(&usb_dev.join("product")),
        serial: read_trimmed(&usb_dev.join("serial")).filter(|s| !s.is_empty()),
        bus: read_trimmed(&usb_dev.join("busnum")).and_then(|s| s.parse().ok()),
        address: read_trimmed(&usb_dev.join("devnum")).and_then(|s| s.parse().ok()),
        port_path: usb_dev
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        sysfs_path: usb_dev.to_path_buf(),
        driver,
    })
}

/// Map `/dev/videoN` to its `/sys/class/video4linux/videoN` directory using
/// the node's major:minor (`/sys/dev/char/MAJ:MIN`), so symlinks and
/// unusual device paths still resolve.
fn sysfs_for_dev(dev_path: &Path) -> Result<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    let meta = fs::metadata(dev_path).map_err(|e| Error::from_io(e, dev_path, "stat"))?;
    if !is_char(meta.mode()) {
        return Err(Error::UnsupportedDevice(dev_path.to_path_buf()));
    }
    let rdev = meta.rdev();
    let (major, minor) = (libc::major(rdev), libc::minor(rdev));
    let link = PathBuf::from(format!("/sys/dev/char/{major}:{minor}"));
    fs::canonicalize(&link).map_err(|e| Error::from_io(e, &link, "resolve sysfs"))
}

fn is_char(mode: u32) -> bool {
    mode & libc::S_IFMT == libc::S_IFCHR
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

pub(crate) fn read_hex_u16(path: &Path) -> Option<u16> {
    let s = read_trimmed(path)?;
    parse_hex_u16(&s)
}

/// Parse sysfs-style hex (`2e1a`, optionally `0x2e1a`).
pub(crate) fn parse_hex_u16(s: &str) -> Option<u16> {
    let s = s.trim().trim_start_matches("0x");
    u16::from_str_radix(s, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parsing() {
        assert_eq!(parse_hex_u16("2e1a"), Some(0x2e1a));
        assert_eq!(parse_hex_u16("0x4c04\n"), Some(0x4c04));
        assert_eq!(parse_hex_u16("zz"), None);
        assert_eq!(parse_hex_u16(""), None);
    }

    /// Build a fake sysfs tree mirroring the real Link 2 layout and check
    /// that `usb_info_for_interface` reads the right attributes.
    #[test]
    fn reads_usb_info_from_synthetic_sysfs() {
        let tmp = std::env::temp_dir().join(format!("linkctl-sysfs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let usb = tmp.join("usb1").join("1-1");
        let iface = usb.join("1-1:1.0");
        fs::create_dir_all(&iface).unwrap();
        fs::write(usb.join("idVendor"), "2e1a\n").unwrap();
        fs::write(usb.join("idProduct"), "4c04\n").unwrap();
        fs::write(usb.join("product"), "Insta360 Link 2\n").unwrap();
        fs::write(usb.join("manufacturer"), "Insta360\n").unwrap();
        fs::write(usb.join("busnum"), "1\n").unwrap();
        fs::write(usb.join("devnum"), "2\n").unwrap();
        fs::create_dir_all(tmp.join("drivers").join("uvcvideo")).unwrap();
        std::os::unix::fs::symlink(tmp.join("drivers").join("uvcvideo"), iface.join("driver"))
            .unwrap();

        let info = usb_info_for_interface(&iface).unwrap();
        assert_eq!(info.vendor_id, 0x2e1a);
        assert_eq!(info.product_id, 0x4c04);
        assert_eq!(info.product.as_deref(), Some("Insta360 Link 2"));
        assert_eq!(info.manufacturer.as_deref(), Some("Insta360"));
        assert_eq!(info.bus, Some(1));
        assert_eq!(info.address, Some(2));
        assert_eq!(info.port_path, "1-1");
        assert_eq!(info.driver.as_deref(), Some("uvcvideo"));
        assert_eq!(info.serial, None);
        assert_eq!(
            Model::from_usb_ids(info.vendor_id, info.product_id),
            Some(Model::Link2)
        );

        // A non-USB parent (no idVendor) yields None.
        let pci = tmp.join("pci").join("0000:00:01.0");
        fs::create_dir_all(&pci).unwrap();
        assert!(usb_info_for_interface(&pci).is_none());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn discover_in_missing_sysfs_is_empty() {
        let devices = discover_in(Path::new("/nonexistent/v4l"), Path::new("/dev")).unwrap();
        assert!(devices.is_empty());
    }
}
