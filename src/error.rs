//! Structured error type and the stable exit-code contract.
//!
//! Every failure that can reach the user is one of these variants. The exit
//! codes are part of the public interface (see `docs/exit-codes` in the
//! README) and must stay stable so that scripts and desktop integrations can
//! rely on them.

use std::path::PathBuf;

use crate::camera::discovery::DeviceSummary;

/// All errors surfaced by `linkctl`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("No supported Insta360 Link camera found.")]
    CameraNotFound,

    #[error("Multiple Insta360 Link cameras found.")]
    MultipleCameras(Vec<DeviceSummary>),

    #[error("{0} is not a supported Insta360 Link camera.")]
    UnsupportedDevice(PathBuf),

    #[error("Permission denied opening {0}.")]
    PermissionDenied(PathBuf),

    #[error("Camera is inactive.")]
    CameraInactive,

    #[error("Control '{0}' is not supported by this camera.")]
    UnsupportedControl(&'static str),

    #[error("{0}")]
    InvalidValue(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("{0}")]
    Preview(String),

    #[error("Device I/O error: {0}")]
    Io(String),

    #[error("Vendor control error: {0}")]
    Vendor(String),

    #[error("Device disappeared: {0}")]
    DeviceGone(PathBuf),
}

/// Stable exit codes. Keep this list in sync with the README.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ExitCode {
    InvalidArguments = 2,
    CameraNotFound = 3,
    MultipleCameras = 4,
    CameraInactive = 5,
    PermissionDenied = 6,
    UnsupportedControl = 7,
    DeviceIo = 8,
    Config = 9,
    Preview = 10,
    InvalidValue = 11,
    VendorControl = 12,
}

impl Error {
    /// Map an error to its documented exit code.
    pub fn exit_code(&self) -> ExitCode {
        match self {
            Error::CameraNotFound => ExitCode::CameraNotFound,
            Error::MultipleCameras(_) => ExitCode::MultipleCameras,
            Error::UnsupportedDevice(_) => ExitCode::CameraNotFound,
            Error::PermissionDenied(_) => ExitCode::PermissionDenied,
            Error::CameraInactive => ExitCode::CameraInactive,
            Error::UnsupportedControl(_) => ExitCode::UnsupportedControl,
            Error::InvalidValue(_) => ExitCode::InvalidValue,
            Error::Config(_) => ExitCode::Config,
            Error::Preview(_) => ExitCode::Preview,
            Error::Io(_) => ExitCode::DeviceIo,
            Error::Vendor(_) => ExitCode::VendorControl,
            Error::DeviceGone(_) => ExitCode::DeviceIo,
        }
    }

    /// Short machine-readable identifier used in `--json` error output.
    pub fn kind(&self) -> &'static str {
        match self {
            Error::CameraNotFound => "camera_not_found",
            Error::MultipleCameras(_) => "multiple_cameras",
            Error::UnsupportedDevice(_) => "unsupported_device",
            Error::PermissionDenied(_) => "permission_denied",
            Error::CameraInactive => "camera_inactive",
            Error::UnsupportedControl(_) => "unsupported_control",
            Error::InvalidValue(_) => "invalid_value",
            Error::Config(_) => "config",
            Error::Preview(_) => "preview",
            Error::Io(_) => "io",
            Error::Vendor(_) => "vendor_control",
            Error::DeviceGone(_) => "device_gone",
        }
    }

    /// Optional multi-line hint printed after the main message.
    pub fn hint(&self) -> Option<String> {
        match self {
            Error::CameraInactive => Some("Start a preview with:\n  linkctl preview".into()),
            Error::PermissionDenied(_) => {
                Some("Check your device permissions or group membership.".into())
            }
            Error::MultipleCameras(devices) => {
                let mut s = String::new();
                for (i, d) in devices.iter().enumerate() {
                    s.push_str(&format!(
                        "{}. {} — {}\n",
                        i + 1,
                        d.model,
                        d.control_node.display()
                    ));
                }
                s.push_str("\nUse --device to select one.");
                Some(s)
            }
            Error::CameraNotFound => {
                Some("Connect an Insta360 Link 2 or pass --device /dev/videoN.".into())
            }
            _ => None,
        }
    }

    /// Build an I/O error from an `std::io::Error` and a context string,
    /// mapping `EACCES`/`EPERM` and `ENODEV`/`ENXIO` to their dedicated variants.
    pub fn from_io(err: std::io::Error, path: &std::path::Path, context: &str) -> Self {
        use std::io::ErrorKind;
        match err.kind() {
            ErrorKind::PermissionDenied => Error::PermissionDenied(path.to_path_buf()),
            ErrorKind::NotFound => Error::DeviceGone(path.to_path_buf()),
            _ => {
                if matches!(err.raw_os_error(), Some(libc::ENODEV) | Some(libc::ENXIO)) {
                    Error::DeviceGone(path.to_path_buf())
                } else {
                    Error::Io(format!("{context}: {err}"))
                }
            }
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
