//! Video format (resolution / pixel format / frame rate) ioctls.
//!
//! In V4L2 the "resolution" is not a persistent camera property: every
//! streaming application negotiates its own format with `VIDIOC_S_FMT`.
//! The driver does however keep a *current* format, which applications
//! that do not negotiate (ffplay, mpv, most simple tools) will use. That is
//! what `linkctl resolution` reads and sets.
//!
//! Wrapped ioctls (`<linux/videodev2.h>`):
//!
//! * `VIDIOC_ENUM_FMT`, `VIDIOC_ENUM_FRAMESIZES`, `VIDIOC_ENUM_FRAMEINTERVALS`
//!   — enumerate what the camera offers (read-only).
//! * `VIDIOC_G_FMT` / `VIDIOC_S_FMT` — current pixel format and size.
//! * `VIDIOC_G_PARM` / `VIDIOC_S_PARM` — current frame interval.
//!
//! Setting the format sends a UVC probe to the streaming interface but does
//! **not** start streaming, so the gimbal stays parked. While another
//! process streams, the driver answers `EBUSY`; that is surfaced as
//! [`Error::DeviceBusy`].

use std::fmt;

use serde::Serialize;

use super::v4l2::{iowr, V4l2Device};
use crate::error::{Error, Result};

const V4L2_BUF_TYPE_VIDEO_CAPTURE: u32 = 1;
const V4L2_FRMSIZE_TYPE_DISCRETE: u32 = 1;
const V4L2_FRMIVAL_TYPE_DISCRETE: u32 = 1;
const V4L2_CAP_TIMEPERFRAME: u32 = 0x1000;

/// `struct v4l2_fmtdesc`.
#[repr(C)]
struct V4l2FmtDesc {
    index: u32,
    type_: u32,
    flags: u32,
    description: [u8; 32],
    pixelformat: u32,
    mbus_code: u32,
    reserved: [u32; 3],
}
const _: () = assert!(std::mem::size_of::<V4l2FmtDesc>() == 64);

/// `struct v4l2_frmsizeenum` (the stepwise union member is only kept for
/// size; `linkctl` uses discrete sizes, which is what UVC reports).
#[repr(C)]
struct V4l2FrmSizeEnum {
    index: u32,
    pixel_format: u32,
    type_: u32,
    width: u32,
    height: u32,
    stepwise_rest: [u32; 4],
    reserved: [u32; 2],
}
const _: () = assert!(std::mem::size_of::<V4l2FrmSizeEnum>() == 44);

/// `struct v4l2_frmivalenum`.
#[repr(C)]
struct V4l2FrmIvalEnum {
    index: u32,
    pixel_format: u32,
    width: u32,
    height: u32,
    type_: u32,
    numerator: u32,
    denominator: u32,
    stepwise_rest: [u32; 4],
    reserved: [u32; 2],
}
const _: () = assert!(std::mem::size_of::<V4l2FrmIvalEnum>() == 52);

/// `struct v4l2_pix_format`.
#[repr(C)]
#[derive(Clone, Copy)]
struct V4l2PixFormat {
    width: u32,
    height: u32,
    pixelformat: u32,
    field: u32,
    bytesperline: u32,
    sizeimage: u32,
    colorspace: u32,
    priv_: u32,
    flags: u32,
    ycbcr_enc: u32,
    quantization: u32,
    xfer_func: u32,
}
const _: () = assert!(std::mem::size_of::<V4l2PixFormat>() == 48);

/// The 200-byte `fmt` union of `struct v4l2_format`. Its alignment is that
/// of a pointer (the `v4l2_window` member holds one), reproduced here with
/// `usize` so the struct layout matches on both 32- and 64-bit targets.
#[repr(C)]
#[derive(Clone, Copy)]
union FormatUnion {
    pix: V4l2PixFormat,
    raw: [usize; 200 / std::mem::size_of::<usize>()],
}

/// `struct v4l2_format`.
#[repr(C)]
#[derive(Clone, Copy)]
struct V4l2Format {
    type_: u32,
    fmt: FormatUnion,
}
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<V4l2Format>() == 208);
#[cfg(target_pointer_width = "32")]
const _: () = assert!(std::mem::size_of::<V4l2Format>() == 204);

/// `struct v4l2_captureparm`.
#[repr(C)]
#[derive(Clone, Copy)]
struct V4l2CaptureParm {
    capability: u32,
    capturemode: u32,
    numerator: u32,
    denominator: u32,
    extendedmode: u32,
    readbuffers: u32,
    reserved: [u32; 4],
}
const _: () = assert!(std::mem::size_of::<V4l2CaptureParm>() == 40);

/// `struct v4l2_streamparm` (capture member + padding to the 200-byte union).
#[repr(C)]
#[derive(Clone, Copy)]
struct V4l2StreamParm {
    type_: u32,
    capture: V4l2CaptureParm,
    rest: [u8; 160],
}
const _: () = assert!(std::mem::size_of::<V4l2StreamParm>() == 204);

const VIDIOC_ENUM_FMT: u32 = iowr::<V4l2FmtDesc>(b'V', 2);
const VIDIOC_G_FMT: u32 = iowr::<V4l2Format>(b'V', 4);
const VIDIOC_S_FMT: u32 = iowr::<V4l2Format>(b'V', 5);
const VIDIOC_G_PARM: u32 = iowr::<V4l2StreamParm>(b'V', 21);
const VIDIOC_S_PARM: u32 = iowr::<V4l2StreamParm>(b'V', 22);
const VIDIOC_ENUM_FRAMESIZES: u32 = iowr::<V4l2FrmSizeEnum>(b'V', 74);
const VIDIOC_ENUM_FRAMEINTERVALS: u32 = iowr::<V4l2FrmIvalEnum>(b'V', 75);

/// A V4L2 pixel format code (FourCC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FourCc(pub u32);

impl FourCc {
    pub const MJPEG: FourCc = FourCc::from_bytes(*b"MJPG");
    pub const H264: FourCc = FourCc::from_bytes(*b"H264");

    pub const fn from_bytes(b: [u8; 4]) -> Self {
        FourCc(u32::from_le_bytes(b))
    }

    /// Parse a user-supplied name: a FourCC (`MJPG`) or a friendly alias
    /// (`mjpeg`, `h264`).
    pub fn parse(s: &str) -> Option<Self> {
        let t = s.trim();
        match t.to_ascii_lowercase().as_str() {
            "mjpg" | "mjpeg" | "jpeg" => return Some(Self::MJPEG),
            "h264" | "h.264" | "avc" => return Some(Self::H264),
            _ => {}
        }
        let b = t.as_bytes();
        if b.len() == 4 && b.iter().all(|c| c.is_ascii_graphic()) {
            let mut arr = [0u8; 4];
            arr.copy_from_slice(b);
            return Some(Self::from_bytes(arr));
        }
        None
    }

    /// Name usable with FFmpeg's `-input_format`, if known.
    pub fn ffmpeg_name(&self) -> Option<&'static str> {
        match *self {
            FourCc::MJPEG => Some("mjpeg"),
            FourCc::H264 => Some("h264"),
            FourCc(x) if x == u32::from_le_bytes(*b"YUYV") => Some("yuyv422"),
            FourCc(x) if x == u32::from_le_bytes(*b"NV12") => Some("nv12"),
            _ => None,
        }
    }
}

impl fmt::Display for FourCc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = self.0.to_le_bytes();
        for c in b {
            if c.is_ascii_graphic() || c == b' ' {
                write!(f, "{}", c as char)?;
            } else {
                write!(f, "?")?;
            }
        }
        Ok(())
    }
}

impl Serialize for FourCc {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(self.to_string().trim())
    }
}

/// One discrete frame size with its supported frame rates.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FrameSize {
    pub width: u32,
    pub height: u32,
    /// Frames per second, descending.
    pub fps: Vec<f64>,
}

/// One pixel format with its frame sizes.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FormatDesc {
    pub fourcc: FourCc,
    pub description: String,
    pub sizes: Vec<FrameSize>,
}

/// The driver's current capture format.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct CurrentFormat {
    pub fourcc: FourCc,
    pub width: u32,
    pub height: u32,
    /// `None` if the driver does not report a frame interval.
    pub fps: Option<f64>,
}

impl CurrentFormat {
    pub fn resolution(&self) -> String {
        format!("{}x{}", self.width, self.height)
    }
}

/// A requested format change. Unspecified parts keep their current value.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FormatRequest {
    pub fourcc: Option<FourCc>,
    pub width: u32,
    pub height: u32,
    pub fps: Option<f64>,
}

impl V4l2Device {
    /// Enumerate every pixel format, frame size and frame rate offered by
    /// the capture node. Read-only.
    pub fn enumerate_formats(&self) -> Result<Vec<FormatDesc>> {
        let mut formats = Vec::new();
        for index in 0u32.. {
            // SAFETY: plain-data struct; zero is a valid bit pattern.
            let mut d: V4l2FmtDesc = unsafe { std::mem::zeroed() };
            d.index = index;
            d.type_ = V4L2_BUF_TYPE_VIDEO_CAPTURE;
            // SAFETY: VIDIOC_ENUM_FMT takes a `struct v4l2_fmtdesc *`.
            match unsafe { self.ioctl(VIDIOC_ENUM_FMT, &mut d) } {
                Ok(()) => {}
                Err(e) if e.raw_os_error() == Some(libc::EINVAL) => break,
                Err(e) => return Err(self.io_err(e, "VIDIOC_ENUM_FMT")),
            }
            let fourcc = FourCc(d.pixelformat);
            let sizes = self.enumerate_sizes(fourcc)?;
            formats.push(FormatDesc {
                fourcc,
                description: cstr(&d.description),
                sizes,
            });
        }
        Ok(formats)
    }

    fn enumerate_sizes(&self, fourcc: FourCc) -> Result<Vec<FrameSize>> {
        let mut sizes = Vec::new();
        for index in 0u32.. {
            // SAFETY: plain-data struct; zero is a valid bit pattern.
            let mut s: V4l2FrmSizeEnum = unsafe { std::mem::zeroed() };
            s.index = index;
            s.pixel_format = fourcc.0;
            // SAFETY: VIDIOC_ENUM_FRAMESIZES takes a `struct v4l2_frmsizeenum *`.
            match unsafe { self.ioctl(VIDIOC_ENUM_FRAMESIZES, &mut s) } {
                Ok(()) => {}
                Err(e) if e.raw_os_error() == Some(libc::EINVAL) => break,
                Err(e) => return Err(self.io_err(e, "VIDIOC_ENUM_FRAMESIZES")),
            }
            if s.type_ != V4L2_FRMSIZE_TYPE_DISCRETE {
                // Stepwise/continuous ranges are not something UVC cameras
                // report; skip rather than guess.
                continue;
            }
            let fps = self.enumerate_intervals(fourcc, s.width, s.height)?;
            sizes.push(FrameSize {
                width: s.width,
                height: s.height,
                fps,
            });
        }
        Ok(sizes)
    }

    fn enumerate_intervals(&self, fourcc: FourCc, width: u32, height: u32) -> Result<Vec<f64>> {
        let mut out = Vec::new();
        for index in 0u32.. {
            // SAFETY: plain-data struct; zero is a valid bit pattern.
            let mut i: V4l2FrmIvalEnum = unsafe { std::mem::zeroed() };
            i.index = index;
            i.pixel_format = fourcc.0;
            i.width = width;
            i.height = height;
            // SAFETY: VIDIOC_ENUM_FRAMEINTERVALS takes a `struct v4l2_frmivalenum *`.
            match unsafe { self.ioctl(VIDIOC_ENUM_FRAMEINTERVALS, &mut i) } {
                Ok(()) => {}
                Err(e) if e.raw_os_error() == Some(libc::EINVAL) => break,
                Err(e) => return Err(self.io_err(e, "VIDIOC_ENUM_FRAMEINTERVALS")),
            }
            if i.type_ == V4L2_FRMIVAL_TYPE_DISCRETE && i.numerator != 0 {
                out.push(fps_from_interval(i.numerator, i.denominator));
            }
        }
        out.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        Ok(out)
    }

    /// Read the driver's current capture format and frame rate.
    pub fn current_format(&self) -> Result<CurrentFormat> {
        // SAFETY: plain-data struct; zero is a valid bit pattern.
        let mut f: V4l2Format = unsafe { std::mem::zeroed() };
        f.type_ = V4L2_BUF_TYPE_VIDEO_CAPTURE;
        // SAFETY: VIDIOC_G_FMT takes a `struct v4l2_format *`.
        unsafe { self.ioctl(VIDIOC_G_FMT, &mut f) }.map_err(|e| self.io_err(e, "VIDIOC_G_FMT"))?;
        // SAFETY: for VIDEO_CAPTURE the driver fills the `pix` member.
        let pix = unsafe { f.fmt.pix };
        let fps = self.current_fps()?;
        Ok(CurrentFormat {
            fourcc: FourCc(pix.pixelformat),
            width: pix.width,
            height: pix.height,
            fps,
        })
    }

    fn current_fps(&self) -> Result<Option<f64>> {
        // SAFETY: plain-data struct; zero is a valid bit pattern.
        let mut p: V4l2StreamParm = unsafe { std::mem::zeroed() };
        p.type_ = V4L2_BUF_TYPE_VIDEO_CAPTURE;
        // SAFETY: VIDIOC_G_PARM takes a `struct v4l2_streamparm *`.
        match unsafe { self.ioctl(VIDIOC_G_PARM, &mut p) } {
            Ok(()) => {}
            Err(e) if e.raw_os_error() == Some(libc::ENOTTY) => return Ok(None),
            Err(e) => return Err(self.io_err(e, "VIDIOC_G_PARM")),
        }
        let c = p.capture;
        if c.capability & V4L2_CAP_TIMEPERFRAME == 0 || c.numerator == 0 {
            return Ok(None);
        }
        Ok(Some(fps_from_interval(c.numerator, c.denominator)))
    }

    /// Set the current capture format (and optionally the frame rate).
    /// Returns what the driver actually applied.
    pub fn set_format(&self, req: FormatRequest) -> Result<CurrentFormat> {
        let current = self.current_format()?;
        let fourcc = req.fourcc.unwrap_or(current.fourcc);
        // SAFETY: plain-data struct; zero is a valid bit pattern.
        let mut f: V4l2Format = unsafe { std::mem::zeroed() };
        f.type_ = V4L2_BUF_TYPE_VIDEO_CAPTURE;
        // Writing union fields is safe; `pix` is the member the driver reads
        // for VIDEO_CAPTURE.
        f.fmt.pix.width = req.width;
        f.fmt.pix.height = req.height;
        f.fmt.pix.pixelformat = fourcc.0;
        // SAFETY: VIDIOC_S_FMT takes a `struct v4l2_format *`.
        unsafe { self.ioctl(VIDIOC_S_FMT, &mut f) }
            .map_err(|e| self.busy_or_io(e, "VIDIOC_S_FMT"))?;

        if let Some(fps) = req.fps {
            let (num, den) = interval_from_fps(fps);
            // SAFETY: plain-data struct; zero is a valid bit pattern.
            let mut p: V4l2StreamParm = unsafe { std::mem::zeroed() };
            p.type_ = V4L2_BUF_TYPE_VIDEO_CAPTURE;
            p.capture.numerator = num;
            p.capture.denominator = den;
            // SAFETY: VIDIOC_S_PARM takes a `struct v4l2_streamparm *`.
            unsafe { self.ioctl(VIDIOC_S_PARM, &mut p) }
                .map_err(|e| self.busy_or_io(e, "VIDIOC_S_PARM"))?;
        }
        self.current_format()
    }

    fn busy_or_io(&self, e: std::io::Error, what: &str) -> Error {
        if e.raw_os_error() == Some(libc::EBUSY) {
            Error::DeviceBusy(
                "the format can only be changed while no application is streaming".into(),
            )
        } else {
            self.io_err(e, what)
        }
    }
}

fn fps_from_interval(numerator: u32, denominator: u32) -> f64 {
    // Round to 3 decimals so 30000/1001 shows as 29.97, not 29.970029...
    let v = f64::from(denominator) / f64::from(numerator);
    (v * 1000.0).round() / 1000.0
}

/// Convert a frame rate to a `timeperframe` fraction. Integral rates map to
/// `1/N`; others to `1000/(fps*1000)` which is exact for values like 29.97.
pub fn interval_from_fps(fps: f64) -> (u32, u32) {
    if (fps - fps.round()).abs() < 1e-9 {
        (1, fps.round() as u32)
    } else {
        (1000, (fps * 1000.0).round() as u32)
    }
}

/// Parse `WIDTHxHEIGHT` or `WIDTHxHEIGHT@FPS`.
pub fn parse_resolution_spec(s: &str) -> Option<(u32, u32, Option<f64>)> {
    let t = s.trim();
    let (size, fps) = match t.split_once('@') {
        Some((a, b)) => (a, Some(b)),
        None => (t, None),
    };
    let (w, h) = crate::config::parse_resolution(size)?;
    let fps = match fps {
        Some(f) => {
            let f = f.trim().trim_end_matches("fps").trim();
            let v: f64 = f.parse().ok()?;
            if !(v.is_finite() && v > 0.0) {
                return None;
            }
            Some(v)
        }
        None => None,
    };
    Some((w, h, fps))
}

/// Check a request against the enumerated formats. Returns an error message
/// listing the valid choices when the size (or rate) is not offered.
pub fn validate_request(
    formats: &[FormatDesc],
    current: &CurrentFormat,
    req: &FormatRequest,
) -> std::result::Result<(), String> {
    let fourcc = req.fourcc.unwrap_or(current.fourcc);
    let Some(fmt) = formats.iter().find(|f| f.fourcc == fourcc) else {
        let names: Vec<String> = formats.iter().map(|f| f.fourcc.to_string()).collect();
        return Err(format!(
            "Format {fourcc} is not offered by this camera (available: {}).",
            names.join(", ")
        ));
    };
    let Some(size) = fmt
        .sizes
        .iter()
        .find(|s| s.width == req.width && s.height == req.height)
    else {
        let sizes: Vec<String> = fmt
            .sizes
            .iter()
            .map(|s| format!("{}x{}", s.width, s.height))
            .collect();
        return Err(format!(
            "{}x{} is not offered for {fourcc} (available: {}).",
            req.width,
            req.height,
            sizes.join(", ")
        ));
    };
    if let Some(fps) = req.fps {
        if !size.fps.iter().any(|f| (f - fps).abs() < 0.01) {
            let rates: Vec<String> = size.fps.iter().map(|f| format_fps(*f)).collect();
            return Err(format!(
                "{} fps is not offered at {}x{} (available: {}).",
                format_fps(fps),
                req.width,
                req.height,
                rates.join(", ")
            ));
        }
    }
    Ok(())
}

pub fn format_fps(fps: f64) -> String {
    if (fps - fps.round()).abs() < 1e-6 {
        format!("{}", fps.round() as i64)
    } else {
        format!("{fps:.2}")
    }
}

fn cstr(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioctl_numbers_match_kernel_headers() {
        assert_eq!(VIDIOC_ENUM_FMT, 0xc040_5602);
        assert_eq!(VIDIOC_ENUM_FRAMESIZES, 0xc02c_564a);
        assert_eq!(VIDIOC_ENUM_FRAMEINTERVALS, 0xc034_564b);
        #[cfg(target_pointer_width = "64")]
        {
            assert_eq!(VIDIOC_G_FMT, 0xc0d0_5604);
            assert_eq!(VIDIOC_S_FMT, 0xc0d0_5605);
        }
        assert_eq!(VIDIOC_G_PARM, 0xc0cc_5615);
        assert_eq!(VIDIOC_S_PARM, 0xc0cc_5616);
    }

    #[test]
    fn fourcc_round_trip() {
        assert_eq!(FourCc::MJPEG.0, 0x4750_4a4d);
        assert_eq!(FourCc::H264.0, 0x3436_3248);
        assert_eq!(FourCc::MJPEG.to_string(), "MJPG");
        assert_eq!(FourCc::parse("mjpeg"), Some(FourCc::MJPEG));
        assert_eq!(FourCc::parse("H264"), Some(FourCc::H264));
        assert_eq!(FourCc::parse("YUYV"), Some(FourCc::from_bytes(*b"YUYV")));
        assert_eq!(FourCc::parse("nope!"), None);
        assert_eq!(FourCc::MJPEG.ffmpeg_name(), Some("mjpeg"));
        assert_eq!(serde_json::to_string(&FourCc::H264).unwrap(), "\"H264\"");
    }

    #[test]
    fn fps_conversions() {
        assert_eq!(fps_from_interval(1, 30), 30.0);
        assert_eq!(fps_from_interval(1001, 30000), 29.97);
        assert_eq!(interval_from_fps(30.0), (1, 30));
        assert_eq!(interval_from_fps(29.97), (1000, 29970));
        assert_eq!(format_fps(30.0), "30");
        assert_eq!(format_fps(29.97), "29.97");
    }

    #[test]
    fn resolution_spec_parsing() {
        assert_eq!(parse_resolution_spec("1920x1080"), Some((1920, 1080, None)));
        assert_eq!(
            parse_resolution_spec("1920x1080@30"),
            Some((1920, 1080, Some(30.0)))
        );
        assert_eq!(
            parse_resolution_spec("1280x720@29.97fps"),
            Some((1280, 720, Some(29.97)))
        );
        assert_eq!(parse_resolution_spec("1080p"), None);
        assert_eq!(parse_resolution_spec("1920x1080@0"), None);
    }

    fn link2_formats() -> Vec<FormatDesc> {
        vec![
            FormatDesc {
                fourcc: FourCc::MJPEG,
                description: "Motion-JPEG".into(),
                sizes: vec![
                    FrameSize {
                        width: 1920,
                        height: 1080,
                        fps: vec![30.0, 25.0, 24.0],
                    },
                    FrameSize {
                        width: 1280,
                        height: 720,
                        fps: vec![30.0, 25.0, 24.0],
                    },
                ],
            },
            FormatDesc {
                fourcc: FourCc::H264,
                description: "H.264".into(),
                sizes: vec![FrameSize {
                    width: 3840,
                    height: 2160,
                    fps: vec![30.0],
                }],
            },
        ]
    }

    #[test]
    fn request_validation() {
        let formats = link2_formats();
        let current = CurrentFormat {
            fourcc: FourCc::MJPEG,
            width: 1280,
            height: 720,
            fps: Some(30.0),
        };
        let ok = FormatRequest {
            fourcc: None,
            width: 1920,
            height: 1080,
            fps: Some(25.0),
        };
        assert!(validate_request(&formats, &current, &ok).is_ok());
        let bad_size = FormatRequest {
            width: 640,
            height: 480,
            ..ok
        };
        let err = validate_request(&formats, &current, &bad_size).unwrap_err();
        assert!(err.contains("640x480"), "{err}");
        assert!(err.contains("1920x1080, 1280x720"), "{err}");
        let bad_fps = FormatRequest {
            fps: Some(60.0),
            ..ok
        };
        let err = validate_request(&formats, &current, &bad_fps).unwrap_err();
        assert!(err.contains("60 fps"), "{err}");
        let bad_fmt = FormatRequest {
            fourcc: Some(FourCc::from_bytes(*b"YUYV")),
            ..ok
        };
        let err = validate_request(&formats, &current, &bad_fmt).unwrap_err();
        assert!(err.contains("MJPG, H264"), "{err}");
        // Explicit format switches which size table is consulted.
        let h264 = FormatRequest {
            fourcc: Some(FourCc::H264),
            width: 3840,
            height: 2160,
            fps: None,
        };
        assert!(validate_request(&formats, &current, &h264).is_ok());
    }
}
