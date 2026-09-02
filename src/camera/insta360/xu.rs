//! UVC Extension Unit access through the kernel `uvcvideo` driver.
//!
//! We use the `UVCIOC_CTRL_QUERY` ioctl on the open video node. This goes
//! through the kernel driver, so it never requires detaching `uvcvideo`,
//! keeps the device usable by other applications, and is serialised by the
//! driver with regular control traffic. This is the mechanism recommended
//! in `docs/safety.md`.
//!
//! Only *known* unit/selector/length triples are ever written; the write
//! path takes a [`XuWrite`] descriptor that must be declared as a constant
//! next to the reverse-engineering reference it comes from.

use std::io;

use super::super::v4l2::{iowr, V4l2Device};
use crate::error::{Error, Result};

/// `struct uvc_xu_control_query` from `<linux/uvcvideo.h>`.
#[repr(C)]
struct UvcXuControlQuery {
    unit: u8,
    selector: u8,
    query: u8,
    size: u16,
    data: *mut u8,
}
const _: () = assert!(std::mem::size_of::<UvcXuControlQuery>() == 2 * std::mem::size_of::<usize>());

/// `UVCIOC_CTRL_QUERY = _IOWR('u', 0x21, struct uvc_xu_control_query)`.
const UVCIOC_CTRL_QUERY: u32 = iowr::<UvcXuControlQuery>(b'u', 0x21);

/// UVC class-specific request codes (`linux/usb/video.h`, UVC 1.5 A.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
#[allow(dead_code)] // full request set kept for reference/debugging
pub enum XuQuery {
    SetCur = 0x01,
    GetCur = 0x81,
    GetMin = 0x82,
    GetMax = 0x83,
    GetRes = 0x84,
    GetLen = 0x85,
    GetInfo = 0x86,
    GetDef = 0x87,
}

/// `GET_INFO` capability bits (UVC 1.5 §4.1.2).
#[allow(dead_code)]
pub const XU_INFO_SUPPORTS_GET: u8 = 0x01;
pub const XU_INFO_SUPPORTS_SET: u8 = 0x02;

/// Static description of a vendor control that may be *written*. Every
/// instance must be a documented constant (see `link2.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XuControl {
    /// Extension unit id from the USB descriptor.
    pub unit: u8,
    /// Control selector within the unit.
    pub selector: u8,
    /// Expected payload length in bytes (validated against `GET_LEN`).
    pub len: u16,
    /// Short human name for diagnostics.
    pub name: &'static str,
}

impl V4l2Device {
    /// Issue one `UVCIOC_CTRL_QUERY`. `buf` must be exactly the size the
    /// device expects for `query` (2 bytes for `GET_LEN`, 1 for `GET_INFO`,
    /// the control length otherwise).
    fn xu_query(&self, unit: u8, selector: u8, query: XuQuery, buf: &mut [u8]) -> io::Result<()> {
        let size = u16::try_from(buf.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "XU payload too large"))?;
        let mut q = UvcXuControlQuery {
            unit,
            selector,
            query: query as u8,
            size,
            data: buf.as_mut_ptr(),
        };
        // SAFETY: `q.data` points to `buf`, which lives for the whole call
        // and has exactly `q.size` bytes; the kernel copies at most `size`
        // bytes in either direction. Request/arg types match by construction.
        unsafe { self.ioctl(UVCIOC_CTRL_QUERY, &mut q) }
    }

    /// `GET_LEN`: payload length of a vendor control, as the device reports.
    pub fn xu_get_len(&self, unit: u8, selector: u8) -> Result<u16> {
        let mut buf = [0u8; 2];
        self.xu_query(unit, selector, XuQuery::GetLen, &mut buf)
            .map_err(|e| self.xu_err(e, unit, selector, "GET_LEN"))?;
        Ok(u16::from_le_bytes(buf))
    }

    /// `GET_INFO`: capability bits (`XU_INFO_SUPPORTS_GET` / `_SET`).
    pub fn xu_get_info(&self, unit: u8, selector: u8) -> Result<u8> {
        let mut buf = [0u8; 1];
        self.xu_query(unit, selector, XuQuery::GetInfo, &mut buf)
            .map_err(|e| self.xu_err(e, unit, selector, "GET_INFO"))?;
        Ok(buf[0])
    }

    /// `GET_CUR` with an explicit length. Read-only and therefore safe to
    /// call on any documented control.
    pub fn xu_get_cur(&self, unit: u8, selector: u8, len: u16) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; usize::from(len)];
        self.xu_query(unit, selector, XuQuery::GetCur, &mut buf)
            .map_err(|e| self.xu_err(e, unit, selector, "GET_CUR"))?;
        Ok(buf)
    }

    /// Read the current payload of a documented control, verifying the
    /// device-reported length matches the documented one first.
    pub fn xu_read(&self, ctrl: &XuControl) -> Result<Vec<u8>> {
        let len = self.xu_get_len(ctrl.unit, ctrl.selector)?;
        if len != ctrl.len {
            return Err(Error::Vendor(format!(
                "{}: device reports a {len}-byte payload, expected {} (unit {}, selector 0x{:02x}); refusing to touch it",
                ctrl.name, ctrl.len, ctrl.unit, ctrl.selector
            )));
        }
        self.xu_get_cur(ctrl.unit, ctrl.selector, len)
    }

    /// Read a control using the length the *device* reports (`GET_LEN`),
    /// for read-only diagnostics where the exact length is not critical.
    /// Never used on the write path.
    pub fn xu_read_reported(&self, unit: u8, selector: u8) -> Result<Vec<u8>> {
        let len = self.xu_get_len(unit, selector)?;
        if len == 0 {
            return Err(Error::Vendor(format!(
                "unit {unit} selector 0x{selector:02x} reports a zero-length payload"
            )));
        }
        self.xu_get_cur(unit, selector, len)
    }

    /// `SET_CUR` of a documented control. The payload length is validated
    /// against both the constant and the device's `GET_LEN`, and `GET_INFO`
    /// must advertise SET support. Callers are expected to have produced
    /// `payload` by read-modify-write of [`V4l2Device::xu_read`].
    pub fn xu_write(&self, ctrl: &XuControl, payload: &[u8]) -> Result<()> {
        if payload.len() != usize::from(ctrl.len) {
            return Err(Error::Vendor(format!(
                "{}: payload is {} bytes, expected {}",
                ctrl.name,
                payload.len(),
                ctrl.len
            )));
        }
        let len = self.xu_get_len(ctrl.unit, ctrl.selector)?;
        if len != ctrl.len {
            return Err(Error::Vendor(format!(
                "{}: device reports a {len}-byte payload, expected {}; refusing to write",
                ctrl.name, ctrl.len
            )));
        }
        let info = self.xu_get_info(ctrl.unit, ctrl.selector)?;
        if info & XU_INFO_SUPPORTS_SET == 0 {
            return Err(Error::Vendor(format!(
                "{}: device does not advertise SET support (GET_INFO=0x{info:02x})",
                ctrl.name
            )));
        }
        let mut buf = payload.to_vec();
        self.xu_query(ctrl.unit, ctrl.selector, XuQuery::SetCur, &mut buf)
            .map_err(|e| self.xu_err(e, ctrl.unit, ctrl.selector, "SET_CUR"))
    }

    fn xu_err(&self, e: io::Error, unit: u8, selector: u8, what: &str) -> Error {
        match e.kind() {
            io::ErrorKind::PermissionDenied => Error::PermissionDenied(self.path().to_path_buf()),
            _ => Error::Vendor(format!(
                "{what} on unit {unit} selector 0x{selector:02x} failed: {e}"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioctl_number_matches_kernel_header() {
        // `_IOWR('u', 0x21, struct uvc_xu_control_query)`; the struct is 16
        // bytes on 64-bit and 12 on 32-bit targets.
        #[cfg(target_pointer_width = "64")]
        assert_eq!(UVCIOC_CTRL_QUERY, 0xc010_7521);
        #[cfg(target_pointer_width = "32")]
        assert_eq!(UVCIOC_CTRL_QUERY, 0xc00c_7521);
    }

    #[test]
    fn query_codes() {
        assert_eq!(XuQuery::SetCur as u8, 0x01);
        assert_eq!(XuQuery::GetCur as u8, 0x81);
        assert_eq!(XuQuery::GetLen as u8, 0x85);
        assert_eq!(XuQuery::GetInfo as u8, 0x86);
    }
}
