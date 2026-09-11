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
use std::time::{Duration, Instant};

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
    /// Set when the scan gave up before inspecting every process because it
    /// exceeded [`SCAN_BUDGET`]. A partial scan can only prove that the
    /// camera *is* in use, never that it is idle.
    pub partial: bool,
}

impl Activity {
    pub fn is_active(&self) -> bool {
        !self.holders.is_empty()
    }
}

/// Upper bound on how long a `/proc` scan may run.
///
/// The scan is inherently O(file descriptors open on the whole machine): it
/// must `stat` every fd of every process, because proving that *nobody* holds
/// the camera is a universal negative. On a healthy machine that is a few tens
/// of thousands of fds and takes milliseconds. A process leaking descriptors
/// elsewhere on the system can push it into the millions — measured at ~2.2 us
/// per fd regardless of whether `stat`, `lstat` or `readlink` is used, so there
/// is no constant factor to optimise away. The budget keeps one pathological
/// process from turning every `linkctl` invocation into a multi-second,
/// CPU-bound stall.
pub const SCAN_BUDGET: Duration = Duration::from_millis(750);

/// What the caller needs to learn from a scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanGoal {
    /// Collect every holder. Always inspects all processes.
    AllHolders,
    /// Only answer "is anyone holding it?". Stops at the first hit, which
    /// makes the in-use case cheap; the idle case still costs a full sweep.
    AnyHolder,
}

/// Scan `/proc` for every process (excluding `self_pid`) holding any of the
/// given device ids open, with no deadline.
///
/// Used only where the user explicitly asked for the holder list and wants a
/// complete answer rather than a fast one. Everything on a polled path should
/// go through [`scan_with`] with a budget instead.
pub fn scan_exhaustive(device_ids: &[DeviceId], self_pid: u32) -> Activity {
    scan_with(
        Path::new("/proc"),
        device_ids,
        self_pid,
        ScanGoal::AllHolders,
        None,
    )
}

/// Convenience wrapper for tests: a complete scan of an explicit proc root.
#[cfg(test)]
fn scan_proc(proc_root: &Path, device_ids: &[DeviceId], self_pid: u32) -> Activity {
    scan_with(proc_root, device_ids, self_pid, ScanGoal::AllHolders, None)
}

/// The scan proper. `budget` of `None` disables the deadline (used by tests
/// that must see a complete result).
pub fn scan_with(
    proc_root: &Path,
    device_ids: &[DeviceId],
    self_pid: u32,
    goal: ScanGoal,
    budget: Option<Duration>,
) -> Activity {
    let started = Instant::now();
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
        // Checked per process rather than per fd: one `Instant::now()` per
        // process is free, one per descriptor would not be.
        if let Some(limit) = budget {
            if started.elapsed() >= limit {
                activity.partial = true;
                break;
            }
        }
        let fd_dir = entry.path().join("fd");
        match holds_device(&fd_dir, device_ids) {
            Ok(true) => {
                activity.holders.push(Holder {
                    pid,
                    comm: read_comm(&entry.path()),
                });
                if goal == ScanGoal::AnyHolder {
                    break;
                }
            }
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

/// Whether the USB device is currently powered up for use, read from the
/// runtime-PM state in sysfs. `None` means "cannot tell, fall back to a scan".
///
/// This answers a *different* question from [`scan`]: it reports whether the
/// **USB device** is in use, not whether a process holds a particular video
/// node open. The two were measured against a Link 2 and differ in both
/// directions:
///
/// | situation                         | `runtime_status` | holders scan |
/// |-----------------------------------|------------------|--------------|
/// | nothing using the camera          | `suspended`      | inactive     |
/// | node open, not streaming          | `suspended`      | **active**   |
/// | video streaming                   | `active`         | active       |
/// | only the built-in microphone open | **`active`**     | inactive     |
///
/// The microphone row is not a defect: the device really is in use, which is
/// what a status indicator wants to convey. The "open but not streaming" row
/// is why this must never be the sole basis for the inactivity guard.
///
/// Two other sysfs signals were evaluated and rejected: `bAlternateSetting`
/// stays `0` even while streaming (the Link 2 uses bulk rather than isochronous
/// UVC transfers), and the per-interface `power/` directories are empty.
///
/// The reading is only trustworthy while runtime PM is actually allowed to
/// suspend the device. With `power/control` set to `on`, autosuspend is
/// disabled and `runtime_status` is pinned at `active` forever, so we decline
/// to answer.
pub fn usb_device_in_use(sysfs_path: &Path) -> Option<bool> {
    let control = fs::read_to_string(sysfs_path.join("power/control")).ok()?;
    if control.trim() != "auto" {
        return None;
    }
    let status = fs::read_to_string(sysfs_path.join("power/runtime_status")).ok()?;
    match status.trim() {
        "active" => Some(true),
        "suspended" => Some(false),
        // "suspending"/"resuming" are transient; treat as unknown rather than
        // guessing which way the transition will settle.
        _ => None,
    }
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

    fn write_power(root: &Path, control: &str, status: &str) {
        let power = root.join("power");
        fs::create_dir_all(&power).unwrap();
        fs::write(power.join("control"), format!("{control}\n")).unwrap();
        fs::write(power.join("runtime_status"), format!("{status}\n")).unwrap();
    }

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("linkctl-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn usb_in_use_reads_runtime_status() {
        let d = tmpdir("pm-auto");
        write_power(&d, "auto", "active");
        assert_eq!(usb_device_in_use(&d), Some(true));
        write_power(&d, "auto", "suspended");
        assert_eq!(usb_device_in_use(&d), Some(false));
        let _ = fs::remove_dir_all(&d);
    }

    /// With autosuspend disabled the device never leaves "active", so the
    /// reading carries no information and we must fall back to a scan.
    #[test]
    fn usb_in_use_declines_when_autosuspend_disabled() {
        let d = tmpdir("pm-on");
        write_power(&d, "on", "active");
        assert_eq!(usb_device_in_use(&d), None);
        let _ = fs::remove_dir_all(&d);
    }

    /// Transient states must not be guessed either way.
    #[test]
    fn usb_in_use_declines_on_transient_and_missing() {
        let d = tmpdir("pm-transient");
        write_power(&d, "auto", "suspending");
        assert_eq!(usb_device_in_use(&d), None);
        let _ = fs::remove_dir_all(&d);
        assert_eq!(usb_device_in_use(Path::new("/nonexistent/usb")), None);
    }

    /// `AnyHolder` stops early, so it must still report the camera as in use
    /// when several processes hold it.
    #[test]
    fn any_holder_stops_at_first_match() {
        let tmp = tmpdir("proc-any");
        for pid in ["100", "200"] {
            let fd = tmp.join(pid).join("fd");
            fs::create_dir_all(&fd).unwrap();
            fs::write(tmp.join(pid).join("comm"), "viewer\n").unwrap();
            std::os::unix::fs::symlink("/dev/null", fd.join("3")).unwrap();
        }
        let id = DeviceId::of_path(Path::new("/dev/null")).unwrap();
        let a = scan_with(&tmp, &[id], 1, ScanGoal::AnyHolder, None);
        assert!(a.is_active());
        assert_eq!(a.holders.len(), 1, "should stop at the first holder");
        let all = scan_with(&tmp, &[id], 1, ScanGoal::AllHolders, None);
        assert_eq!(all.holders.len(), 2);
        let _ = fs::remove_dir_all(&tmp);
    }

    /// A zero budget makes the scan give up immediately and say so, rather
    /// than reporting a confident (and wrong) "idle".
    #[test]
    fn exhausted_budget_marks_result_partial() {
        let tmp = tmpdir("proc-budget");
        let fd = tmp.join("100").join("fd");
        fs::create_dir_all(&fd).unwrap();
        fs::write(tmp.join("100").join("comm"), "viewer\n").unwrap();
        std::os::unix::fs::symlink("/dev/null", fd.join("3")).unwrap();
        let id = DeviceId::of_path(Path::new("/dev/null")).unwrap();
        let a = scan_with(
            &tmp,
            &[id],
            1,
            ScanGoal::AllHolders,
            Some(Duration::from_nanos(0)),
        );
        assert!(a.partial);
        assert!(!a.is_active());
        let _ = fs::remove_dir_all(&tmp);
    }

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
            partial: false,
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
        let a = scan_exhaustive(&[id], me);
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
