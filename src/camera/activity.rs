//! Camera activity detection.
//!
//! Definition used throughout `linkctl`:
//!
//! > The camera is **active** when some *other* process currently holds one
//! > of its video nodes open.
//!
//! Linux does not expose an "open count" for V4L2 devices through sysfs or a
//! V4L2 ioctl, so we scan `/proc/<pid>/fd/*` natively. Each fd entry is a
//! magic symlink; `stat(2)` on it follows the link and yields the target's
//! device number (`st_rdev`). We compare that against the `st_rdev` of the
//! camera's own nodes, which is robust against renamed paths, bind mounts
//! and containers, unlike matching the `/dev/videoN` string.
//!
//! Our own process is skipped so that `linkctl` opening the control node
//! never counts as activity. See `docs/activity-detection.md` for the
//! limitations (mainly: processes of other users are invisible without
//! elevated privileges).

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// Identity of a character device: `st_rdev` of the node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceId(pub u64);

impl DeviceId {
    /// Look up the device id of a device node path.
    pub fn of_path(path: &Path) -> std::io::Result<Self> {
        let meta = fs::metadata(path)?;
        Ok(DeviceId(meta.rdev()))
    }
}

/// A process found holding one of the watched device nodes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Holder {
    pub pid: u32,
    pub comm: String,
}

/// Result of an activity scan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Activity {
    /// Processes (other than ourselves) holding the device open.
    pub holders: Vec<Holder>,
    /// Number of `/proc/<pid>` directories that could not be inspected
    /// (typically other users' processes). Informational only.
    pub skipped: usize,
}

impl Activity {
    pub fn is_active(&self) -> bool {
        !self.holders.is_empty()
    }
}

/// Scan `/proc` for processes (excluding `self_pid`) holding any of the given
/// device ids open.
pub fn scan(device_ids: &[DeviceId], self_pid: u32) -> Activity {
    scan_proc(Path::new("/proc"), device_ids, self_pid)
}

/// Same as [`scan`], with an explicit proc root so the logic can be tested
/// against a synthetic directory tree.
pub fn scan_proc(proc_root: &Path, device_ids: &[DeviceId], self_pid: u32) -> Activity {
    let mut activity = Activity::default();
    let Ok(entries) = fs::read_dir(proc_root) else {
        return activity;
    };
    for entry in entries.flatten() {
        let Some(pid) = parse_pid(&entry.file_name()) else {
            continue;
        };
        if pid == self_pid {
            continue;
        }
        let fd_dir = entry.path().join("fd");
        match holds_device(&fd_dir, device_ids) {
            Ok(true) => activity.holders.push(Holder {
                pid,
                comm: read_comm(&entry.path()),
            }),
            Ok(false) => {}
            Err(_) => activity.skipped += 1,
        }
    }
    activity.holders.sort_by_key(|h| h.pid);
    activity
}

/// Whether any fd in `fd_dir` refers to a character device with one of the
/// given ids. Errors reading the directory (EACCES, process exited) are
/// returned so the caller can count them; errors on individual fds are
/// ignored because fds come and go while we scan.
fn holds_device(fd_dir: &Path, device_ids: &[DeviceId]) -> std::io::Result<bool> {
    for fd in fs::read_dir(fd_dir)? {
        let Ok(fd) = fd else { continue };
        // `metadata` follows the magic link to the open file itself.
        let Ok(meta) = fs::metadata(fd.path()) else {
            continue;
        };
        if !is_char_device(meta.mode()) {
            continue;
        }
        if device_ids.iter().any(|d| d.0 == meta.rdev()) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn is_char_device(mode: u32) -> bool {
    mode & libc::S_IFMT == libc::S_IFCHR
}

/// Parse a `/proc` directory name as a pid; non-numeric entries yield `None`.
pub fn parse_pid(name: &std::ffi::OsStr) -> Option<u32> {
    let s = name.to_str()?;
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

fn read_comm(proc_dir: &Path) -> String {
    fs::read_to_string(proc_dir.join("comm"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "?".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn pid_parsing() {
        assert_eq!(parse_pid(OsStr::new("1")), Some(1));
        assert_eq!(parse_pid(OsStr::new("4242")), Some(4242));
        assert_eq!(parse_pid(OsStr::new("self")), None);
        assert_eq!(parse_pid(OsStr::new("")), None);
        assert_eq!(parse_pid(OsStr::new("12a")), None);
    }

    #[test]
    fn char_device_detection() {
        assert!(is_char_device(libc::S_IFCHR | 0o600));
        assert!(!is_char_device(libc::S_IFREG | 0o600));
        assert!(!is_char_device(libc::S_IFSOCK));
    }

    #[test]
    fn empty_activity_is_inactive() {
        let a = Activity::default();
        assert!(!a.is_active());
        let b = Activity {
            holders: vec![Holder {
                pid: 1,
                comm: "ffplay".into(),
            }],
            skipped: 0,
        };
        assert!(b.is_active());
    }

    #[test]
    fn scan_skips_self_and_tolerates_missing_root() {
        // Non-existent proc root -> empty, no panic.
        let a = scan_proc(Path::new("/nonexistent/proc"), &[DeviceId(1)], 1);
        assert_eq!(a, Activity::default());
    }

    /// Real `/proc`: this process holds `/dev/null` open via the fd we create
    /// here, but `scan` must not report *us*. Any other process holding
    /// `/dev/null` is fine to report, so we only assert about our own pid.
    #[test]
    fn scan_never_reports_own_pid() {
        let null = fs::File::open("/dev/null").unwrap();
        let id = DeviceId::of_path(Path::new("/dev/null")).unwrap();
        let me = std::process::id();
        let a = scan(&[id], me);
        assert!(a.holders.iter().all(|h| h.pid != me));
        drop(null);
    }

    /// Synthetic proc tree: a fake pid with an fd symlink to /dev/null must be
    /// detected; a fake pid with an fd to a regular file must not.
    #[test]
    fn scan_detects_holder_in_synthetic_tree() {
        let tmp = std::env::temp_dir().join(format!("linkctl-proc-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let p1 = tmp.join("100").join("fd");
        let p2 = tmp.join("200").join("fd");
        fs::create_dir_all(&p1).unwrap();
        fs::create_dir_all(&p2).unwrap();
        fs::write(tmp.join("100").join("comm"), "viewer\n").unwrap();
        std::os::unix::fs::symlink("/dev/null", p1.join("3")).unwrap();
        fs::write(tmp.join("regular"), "x").unwrap();
        std::os::unix::fs::symlink(tmp.join("regular"), p2.join("3")).unwrap();
        // "self"-style entry that is not a pid.
        fs::create_dir_all(tmp.join("self")).unwrap();

        let id = DeviceId::of_path(Path::new("/dev/null")).unwrap();
        let a = scan_proc(&tmp, &[id], 999);
        assert_eq!(
            a.holders,
            vec![Holder {
                pid: 100,
                comm: "viewer".into()
            }]
        );
        // Excluding pid 100 as "self" hides it.
        let b = scan_proc(&tmp, &[id], 100);
        assert!(!b.is_active());
        let _ = fs::remove_dir_all(&tmp);
    }
}
