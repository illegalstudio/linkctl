//! Minimal, documented V4L2 ioctl wrappers.
//!
//! Only the handful of ioctls `linkctl` needs are wrapped here:
//!
//! * `VIDIOC_QUERYCAP`      — classify a video node (capture vs. metadata).
//! * `VIDIOC_QUERYCTRL`     — control metadata (range, step, flags) and
//!   enumeration via `V4L2_CTRL_FLAG_NEXT_CTRL`.
//! * `VIDIOC_G_CTRL` / `VIDIOC_S_CTRL` — read/write a single 32-bit control.
//!
//! Struct layouts are transcribed from `<linux/videodev2.h>`; each has a
//! compile-time size assertion so a layout mistake fails the build rather
//! than corrupting memory. All `unsafe` is confined to this file and each
//! block documents its invariants.
//!
//! The ioctl request numbers are computed with the generic Linux `_IOC`
//! encoding (`asm-generic/ioctl.h`), which is what x86_64 and aarch64 use.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

// --- ioctl number encoding (asm-generic/ioctl.h) ---------------------------

const IOC_NRBITS: u32 = 8;
const IOC_TYPEBITS: u32 = 8;
const IOC_SIZEBITS: u32 = 14;
const IOC_NRSHIFT: u32 = 0;
const IOC_TYPESHIFT: u32 = IOC_NRSHIFT + IOC_NRBITS;
const IOC_SIZESHIFT: u32 = IOC_TYPESHIFT + IOC_TYPEBITS;
const IOC_DIRSHIFT: u32 = IOC_SIZESHIFT + IOC_SIZEBITS;
const IOC_WRITE: u32 = 1;
const IOC_READ: u32 = 2;

const fn ioc(dir: u32, ty: u8, nr: u8, size: usize) -> u32 {
    (dir << IOC_DIRSHIFT)
        | ((ty as u32) << IOC_TYPESHIFT)
        | ((nr as u32) << IOC_NRSHIFT)
        | ((size as u32) << IOC_SIZESHIFT)
}

pub(crate) const fn ior<T>(ty: u8, nr: u8) -> u32 {
    ioc(IOC_READ, ty, nr, std::mem::size_of::<T>())
}

pub(crate) const fn iowr<T>(ty: u8, nr: u8) -> u32 {
    ioc(IOC_READ | IOC_WRITE, ty, nr, std::mem::size_of::<T>())
}

// --- struct v4l2_capability ------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy)]
pub struct V4l2Capability {
    pub driver: [u8; 16],
    pub card: [u8; 32],
    pub bus_info: [u8; 32],
    pub version: u32,
    pub capabilities: u32,
    pub device_caps: u32,
    pub reserved: [u32; 3],
}
const _: () = assert!(std::mem::size_of::<V4l2Capability>() == 104);

pub const V4L2_CAP_VIDEO_CAPTURE: u32 = 0x0000_0001;
pub const V4L2_CAP_META_CAPTURE: u32 = 0x0080_0000;
pub const V4L2_CAP_DEVICE_CAPS: u32 = 0x8000_0000;

// --- struct v4l2_queryctrl -------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy)]
pub struct V4l2QueryCtrl {
    pub id: u32,
    pub type_: u32,
    pub name: [u8; 32],
    pub minimum: i32,
    pub maximum: i32,
    pub step: i32,
    pub default_value: i32,
    pub flags: u32,
    pub reserved: [u32; 2],
}
const _: () = assert!(std::mem::size_of::<V4l2QueryCtrl>() == 68);

pub const V4L2_CTRL_TYPE_INTEGER: u32 = 1;
pub const V4L2_CTRL_TYPE_BOOLEAN: u32 = 2;
pub const V4L2_CTRL_TYPE_MENU: u32 = 3;
pub const V4L2_CTRL_TYPE_BUTTON: u32 = 4;
pub const V4L2_CTRL_TYPE_INTEGER64: u32 = 5;
pub const V4L2_CTRL_TYPE_CTRL_CLASS: u32 = 6;
pub const V4L2_CTRL_TYPE_STRING: u32 = 7;
pub const V4L2_CTRL_TYPE_BITMASK: u32 = 8;
pub const V4L2_CTRL_TYPE_INTEGER_MENU: u32 = 9;

pub const V4L2_CTRL_FLAG_DISABLED: u32 = 0x0001;
pub const V4L2_CTRL_FLAG_GRABBED: u32 = 0x0002;
pub const V4L2_CTRL_FLAG_READ_ONLY: u32 = 0x0004;
pub const V4L2_CTRL_FLAG_INACTIVE: u32 = 0x0010;
pub const V4L2_CTRL_FLAG_WRITE_ONLY: u32 = 0x0040;
pub const V4L2_CTRL_FLAG_VOLATILE: u32 = 0x0080;
pub const V4L2_CTRL_FLAG_NEXT_CTRL: u32 = 0x8000_0000;

// --- struct v4l2_control ---------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy)]
pub struct V4l2Control {
    pub id: u32,
    pub value: i32,
}
const _: () = assert!(std::mem::size_of::<V4l2Control>() == 8);

// --- request numbers -------------------------------------------------------

const VIDIOC_QUERYCAP: u32 = ior::<V4l2Capability>(b'V', 0);
const VIDIOC_G_CTRL: u32 = iowr::<V4l2Control>(b'V', 27);
const VIDIOC_S_CTRL: u32 = iowr::<V4l2Control>(b'V', 28);
const VIDIOC_QUERYCTRL: u32 = iowr::<V4l2QueryCtrl>(b'V', 36);

// --- device handle ---------------------------------------------------------

/// An open V4L2 device node. Opening a UVC node does **not** start streaming;
/// it only lets us issue control ioctls.
pub struct V4l2Device {
    file: File,
    path: PathBuf,
}

impl V4l2Device {
    /// Open `path` read/write (required for `VIDIOC_S_CTRL`) and non-blocking
    /// (so a wedged device cannot hang the CLI on open).
    pub fn open(path: &Path) -> Result<Self> {
        use std::os::unix::fs::OpenOptionsExt;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(path)
            .map_err(|e| Error::from_io(e, path, "open"))?;
        Ok(Self {
            file,
            path: path.to_path_buf(),
        })
    }

    /// Open read-only. Sufficient for `VIDIOC_QUERYCAP` and reads.
    pub fn open_read_only(path: &Path) -> Result<Self> {
        use std::os::unix::fs::OpenOptionsExt;
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(path)
            .map_err(|e| Error::from_io(e, path, "open"))?;
        Ok(Self {
            file,
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Issue an ioctl whose argument is a pointer to `arg`.
    ///
    /// # Safety
    /// The caller must pass a `request` whose kernel-side argument type is
    /// exactly `T` (size and layout), which is guaranteed by the private
    /// constants above: each request number embeds `size_of::<T>()`.
    pub(crate) unsafe fn ioctl<T>(&self, request: u32, arg: &mut T) -> io::Result<()> {
        // SAFETY: `arg` is a valid, exclusively borrowed, properly sized
        // buffer for the duration of the call; the fd is owned by `self`.
        let rc = libc::ioctl(
            self.file.as_raw_fd(),
            request as libc::c_ulong as _,
            arg as *mut T,
        );
        if rc < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    /// Query device capabilities.
    pub fn query_capabilities(&self) -> Result<Capabilities> {
        // SAFETY: zero is a valid bit pattern for this plain-data struct.
        let mut cap: V4l2Capability = unsafe { std::mem::zeroed() };
        // SAFETY: VIDIOC_QUERYCAP takes a `struct v4l2_capability *`.
        unsafe { self.ioctl(VIDIOC_QUERYCAP, &mut cap) }
            .map_err(|e| self.io_err(e, "VIDIOC_QUERYCAP"))?;
        let device_caps = if cap.capabilities & V4L2_CAP_DEVICE_CAPS != 0 {
            cap.device_caps
        } else {
            cap.capabilities
        };
        Ok(Capabilities {
            card: cstr(&cap.card),
            bus_info: cstr(&cap.bus_info),
            version: cap.version,
            device_caps,
        })
    }

    /// Query metadata for a single control id. Returns `Ok(None)` when the
    /// driver reports the control does not exist (`EINVAL`).
    pub fn query_control(&self, id: u32) -> Result<Option<ControlInfo>> {
        // SAFETY: zero is a valid bit pattern for this plain-data struct.
        let mut q: V4l2QueryCtrl = unsafe { std::mem::zeroed() };
        q.id = id;
        // SAFETY: VIDIOC_QUERYCTRL takes a `struct v4l2_queryctrl *`.
        match unsafe { self.ioctl(VIDIOC_QUERYCTRL, &mut q) } {
            Ok(()) => Ok(Some(ControlInfo::from_raw(&q))),
            Err(e) if e.raw_os_error() == Some(libc::EINVAL) => Ok(None),
            Err(e) => Err(self.io_err(e, "VIDIOC_QUERYCTRL")),
        }
    }

    /// Enumerate every control the driver exposes, using the
    /// `V4L2_CTRL_FLAG_NEXT_CTRL` iteration protocol. Control-class
    /// placeholders are skipped.
    pub fn enumerate_controls(&self) -> Result<Vec<ControlInfo>> {
        let mut out = Vec::new();
        let mut id = V4L2_CTRL_FLAG_NEXT_CTRL;
        loop {
            // SAFETY: zero is a valid bit pattern for this plain-data struct.
            let mut q: V4l2QueryCtrl = unsafe { std::mem::zeroed() };
            q.id = id;
            // SAFETY: VIDIOC_QUERYCTRL takes a `struct v4l2_queryctrl *`.
            match unsafe { self.ioctl(VIDIOC_QUERYCTRL, &mut q) } {
                Ok(()) => {
                    if q.type_ != V4L2_CTRL_TYPE_CTRL_CLASS {
                        out.push(ControlInfo::from_raw(&q));
                    }
                    id = q.id | V4L2_CTRL_FLAG_NEXT_CTRL;
                }
                Err(e) if e.raw_os_error() == Some(libc::EINVAL) => break,
                Err(e) => return Err(self.io_err(e, "VIDIOC_QUERYCTRL")),
            }
        }
        Ok(out)
    }

    /// Read the current value of a 32-bit control.
    pub fn get_control(&self, id: u32) -> Result<i32> {
        let mut c = V4l2Control { id, value: 0 };
        // SAFETY: VIDIOC_G_CTRL takes a `struct v4l2_control *`.
        unsafe { self.ioctl(VIDIOC_G_CTRL, &mut c) }
            .map_err(|e| self.io_err(e, "VIDIOC_G_CTRL"))?;
        Ok(c.value)
    }

    /// Write a 32-bit control value. The value must already be validated.
    pub fn set_control(&self, id: u32, value: i32) -> Result<()> {
        let mut c = V4l2Control { id, value };
        // SAFETY: VIDIOC_S_CTRL takes a `struct v4l2_control *`.
        unsafe { self.ioctl(VIDIOC_S_CTRL, &mut c) }.map_err(|e| self.io_err(e, "VIDIOC_S_CTRL"))
    }

    pub(crate) fn io_err(&self, e: io::Error, what: &str) -> Error {
        Error::from_io(e, &self.path, what)
    }
}

/// Decoded `VIDIOC_QUERYCAP` result.
#[derive(Debug, Clone)]
pub struct Capabilities {
    pub card: String,
    pub bus_info: String,
    pub version: u32,
    /// Per-node capabilities (falls back to `capabilities` on old kernels).
    pub device_caps: u32,
}

impl Capabilities {
    pub fn is_video_capture(&self) -> bool {
        self.device_caps & V4L2_CAP_VIDEO_CAPTURE != 0
    }
    pub fn is_metadata(&self) -> bool {
        self.device_caps & V4L2_CAP_META_CAPTURE != 0
    }
    /// Kernel version encoded as `(major, minor, patch)`.
    pub fn version_tuple(&self) -> (u32, u32, u32) {
        (
            (self.version >> 16) & 0xff,
            (self.version >> 8) & 0xff,
            self.version & 0xff,
        )
    }
}

/// Decoded `VIDIOC_QUERYCTRL` result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlInfo {
    pub id: u32,
    pub name: String,
    pub control_type: ControlType,
    pub minimum: i64,
    pub maximum: i64,
    pub step: i64,
    pub default_value: i64,
    pub flags: u32,
}

impl ControlInfo {
    fn from_raw(q: &V4l2QueryCtrl) -> Self {
        Self {
            id: q.id,
            name: cstr(&q.name),
            control_type: ControlType::from_raw(q.type_),
            minimum: q.minimum as i64,
            maximum: q.maximum as i64,
            step: q.step as i64,
            default_value: q.default_value as i64,
            flags: q.flags,
        }
    }

    pub fn range(&self) -> crate::units::Range {
        crate::units::Range::new(self.minimum, self.maximum, self.step.max(1))
    }

    pub fn is_disabled(&self) -> bool {
        self.flags & V4L2_CTRL_FLAG_DISABLED != 0
    }
    pub fn is_read_only(&self) -> bool {
        self.flags & V4L2_CTRL_FLAG_READ_ONLY != 0
    }
    pub fn is_inactive(&self) -> bool {
        self.flags & V4L2_CTRL_FLAG_INACTIVE != 0
    }
    pub fn is_write_only(&self) -> bool {
        self.flags & V4L2_CTRL_FLAG_WRITE_ONLY != 0
    }
    pub fn is_grabbed(&self) -> bool {
        self.flags & V4L2_CTRL_FLAG_GRABBED != 0
    }
    pub fn is_volatile(&self) -> bool {
        self.flags & V4L2_CTRL_FLAG_VOLATILE != 0
    }

    /// Human-readable flag list (e.g. `inactive, read-only`).
    pub fn flag_names(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.is_disabled() {
            v.push("disabled");
        }
        if self.is_grabbed() {
            v.push("grabbed");
        }
        if self.is_read_only() {
            v.push("read-only");
        }
        if self.is_inactive() {
            v.push("inactive");
        }
        if self.is_write_only() {
            v.push("write-only");
        }
        if self.is_volatile() {
            v.push("volatile");
        }
        v
    }
}

/// V4L2 control types we care about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlType {
    Integer,
    Boolean,
    Menu,
    Button,
    Integer64,
    String,
    Bitmask,
    IntegerMenu,
    Other,
}

impl ControlType {
    fn from_raw(raw: u32) -> Self {
        match raw {
            V4L2_CTRL_TYPE_INTEGER => Self::Integer,
            V4L2_CTRL_TYPE_BOOLEAN => Self::Boolean,
            V4L2_CTRL_TYPE_MENU => Self::Menu,
            V4L2_CTRL_TYPE_BUTTON => Self::Button,
            V4L2_CTRL_TYPE_INTEGER64 => Self::Integer64,
            V4L2_CTRL_TYPE_STRING => Self::String,
            V4L2_CTRL_TYPE_BITMASK => Self::Bitmask,
            V4L2_CTRL_TYPE_INTEGER_MENU => Self::IntegerMenu,
            _ => Self::Other,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Integer => "int",
            Self::Boolean => "bool",
            Self::Menu => "menu",
            Self::Button => "button",
            Self::Integer64 => "int64",
            Self::String => "string",
            Self::Bitmask => "bitmask",
            Self::IntegerMenu => "intmenu",
            Self::Other => "other",
        }
    }
}

/// Convert a NUL-terminated fixed-size byte array to a `String`.
fn cstr(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioctl_numbers_match_kernel_headers() {
        // Values taken from `<linux/videodev2.h>` on x86_64/aarch64.
        assert_eq!(VIDIOC_QUERYCAP, 0x8068_5600);
        assert_eq!(VIDIOC_G_CTRL, 0xc008_561b);
        assert_eq!(VIDIOC_S_CTRL, 0xc008_561c);
        assert_eq!(VIDIOC_QUERYCTRL, 0xc044_5624);
    }

    #[test]
    fn cstr_trims_at_nul() {
        let mut buf = [0u8; 16];
        buf[..8].copy_from_slice(b"uvcvideo");
        assert_eq!(cstr(&buf), "uvcvideo");
        assert_eq!(cstr(b"abc"), "abc");
        assert_eq!(
            cstr(b"Insta360 Link 2: Insta360 Link \0"),
            "Insta360 Link 2: Insta360 Link"
        );
    }

    #[test]
    fn control_info_flags() {
        let info = ControlInfo {
            id: 1,
            name: "x".into(),
            control_type: ControlType::Integer,
            minimum: 0,
            maximum: 10,
            step: 2,
            default_value: 0,
            flags: V4L2_CTRL_FLAG_INACTIVE | V4L2_CTRL_FLAG_READ_ONLY,
        };
        assert!(info.is_inactive());
        assert!(info.is_read_only());
        assert!(!info.is_disabled());
        assert_eq!(info.flag_names(), vec!["read-only", "inactive"]);
        assert_eq!(info.range(), crate::units::Range::new(0, 10, 2));
    }
}
