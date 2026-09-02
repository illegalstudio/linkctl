//! Opt-in hardware tests against a physically connected Insta360 Link 2.
//!
//! These are never run by default:
//!
//! ```bash
//! # read-only checks (safe while the camera is inactive)
//! cargo test --features hardware-tests -- --ignored readonly
//!
//! # movement checks: start `linkctl preview` first, then
//! cargo test --features hardware-tests -- --ignored movement
//! ```
//!
//! Movement tests refuse to run while the camera is inactive (exit code 5),
//! exactly like the CLI, so they can never wake a parked camera.
#![cfg(feature = "hardware-tests")]

use std::process::Command;

fn linkctl(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_linkctl"))
        .args(args)
        .output()
        .expect("run linkctl");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn assert_ok(args: &[&str]) -> String {
    let (code, stdout, stderr) = linkctl(args);
    assert_eq!(code, 0, "linkctl {args:?} failed: {stderr}");
    stdout
}

#[test]
#[ignore]
fn readonly_devices_lists_link2() {
    let out = assert_ok(&["devices"]);
    assert!(out.contains("Insta360 Link"), "{out}");
    assert!(out.contains("2e1a:"), "{out}");
}

#[test]
#[ignore]
fn readonly_status_and_info() {
    let out = assert_ok(&["status", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(matches!(
        v["state"].as_str(),
        Some("active") | Some("inactive")
    ));
    let out = assert_ok(&["info", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["usb"]["vendor_id"], "2e1a");
    assert!(v["pan_range_degrees"].is_array());
    assert!(
        v["extension_units"]
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0)
            >= 3
    );
}

#[test]
#[ignore]
fn readonly_reads_do_not_require_activity() {
    for cmd in [
        "pan",
        "tilt",
        "zoom",
        "focus",
        "wb",
        "brightness",
        "tracking",
    ] {
        assert_ok(&[cmd]);
    }
}

#[test]
#[ignore]
fn readonly_out_of_range_is_rejected_before_guard() {
    let (code, _, err) = linkctl(&["pan", "999"]);
    assert_eq!(code, 11, "{err}");
    let (code, _, err) = linkctl(&["zoom", "9"]);
    assert_eq!(code, 11, "{err}");
}

/// Requires an active camera (e.g. `linkctl preview` in another terminal).
#[test]
#[ignore]
fn movement_relative_and_center() {
    let (code, _, err) = linkctl(&["status", "--json"]);
    assert_eq!(code, 0, "{err}");
    assert_ok(&["right", "5"]);
    assert_ok(&["left", "5"]);
    assert_ok(&["up", "5"]);
    assert_ok(&["down", "5"]);
    assert_ok(&["pan", "10"]);
    assert_ok(&["tilt", "5"]);
    assert_ok(&["zoom", "1.5"]);
    assert_ok(&["zoom", "1"]);
    let out = assert_ok(&["center", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["pan"], 0.0);
    assert_eq!(v["tilt"], 0.0);
}
