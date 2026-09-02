use serde::Serialize;

use super::Context;
use crate::camera::controls::Control;
use crate::camera::discovery::{DeviceInfo, NodeRole};
use crate::camera::insta360::link2::{self, ExtensionUnit};
use crate::camera::{self, Camera};
use crate::cli::InfoArgs;
use crate::error::Result;
use crate::units::{arcsec_to_degrees, format_degrees, format_zoom, raw_to_zoom};

#[derive(Serialize)]
struct ControlJson {
    id: String,
    name: String,
    #[serde(rename = "type")]
    control_type: crate::camera::v4l2::ControlType,
    min: i64,
    max: i64,
    step: i64,
    default: i64,
    flags: Vec<&'static str>,
    value: Option<i64>,
}

#[derive(Serialize)]
struct UsbJson<'a> {
    vendor_id: String,
    product_id: String,
    manufacturer: Option<&'a str>,
    product: Option<&'a str>,
    serial: Option<&'a str>,
    bus: Option<u32>,
    address: Option<u32>,
    port_path: &'a str,
    sysfs_path: &'a std::path::Path,
    driver: Option<&'a str>,
    driver_version: &'a str,
}

#[derive(Serialize)]
struct InfoJson<'a> {
    model: &'a str,
    model_id: crate::camera::model::Model,
    tested: bool,
    device: &'a std::path::Path,
    state: &'static str,
    usb: UsbJson<'a>,
    control_node: &'a std::path::Path,
    stream_node: &'a std::path::Path,
    video_nodes: &'a [crate::camera::discovery::VideoNode],
    media_node: Option<&'a std::path::Path>,
    card: &'a str,
    bus_info: &'a str,
    extension_units: Vec<ExtensionUnit>,
    pan_range_degrees: Option<(f64, f64)>,
    tilt_range_degrees: Option<(f64, f64)>,
    zoom_range: Option<(f64, f64)>,
    supported: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ai_mode: Option<link2::AiMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ai_mode_payload_len: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    controls: Option<Vec<ControlJson>>,
}

pub fn run(ctx: &Context, args: &InfoArgs) -> Result<()> {
    let info = ctx.device_info()?;
    let activity = camera::check_activity(&info);
    let state = if activity.is_active() {
        "active"
    } else {
        "inactive"
    };
    let cam = Camera::open(info)?;
    let info = cam.info();

    let extension_units = link2::read_descriptors(&info.usb.sysfs_path)
        .map(|d| link2::parse_extension_units(&d))
        .unwrap_or_default();

    let pan = cam.pan_range().ok();
    let tilt = cam.tilt_range().ok();
    let zoom = cam.zoom_range().ok();
    let pan_deg = pan.map(|r| (arcsec_to_degrees(r.min), arcsec_to_degrees(r.max)));
    let tilt_deg = tilt.map(|r| (arcsec_to_degrees(r.min), arcsec_to_degrees(r.max)));
    let zoom_x = zoom.map(|r| (raw_to_zoom(r.min), raw_to_zoom(r.max)));

    let probe = [
        (Control::FocusAbsolute, "focus"),
        (Control::FocusAuto, "focus_auto"),
        (Control::WhiteBalanceAuto, "white_balance_auto"),
        (
            Control::WhiteBalanceTemperature,
            "white_balance_temperature",
        ),
        (Control::Brightness, "brightness"),
        (Control::Contrast, "contrast"),
        (Control::Saturation, "saturation"),
        (Control::Hue, "hue"),
        (Control::Sharpness, "sharpness"),
        (Control::PowerLineFrequency, "power_line_frequency"),
        (Control::ExposureAbsolute, "exposure"),
        (Control::Gain, "gain"),
        (Control::BacklightCompensation, "backlight_compensation"),
    ];
    let supported: Vec<&'static str> = probe
        .iter()
        .filter(|(c, _)| cam.supports(*c))
        .map(|(_, n)| *n)
        .collect();

    let (ai_mode, ai_len) = match link2::read_ai_mode(cam.device()) {
        Ok((m, len)) => (Some(m), Some(len)),
        Err(e) => {
            ctx.out.debug(format!("AI mode status unavailable: {e}"));
            (None, None)
        }
    };

    let controls = if args.controls {
        let list = cam.device().enumerate_controls()?;
        Some(
            list.iter()
                .map(|c| ControlJson {
                    id: format!("0x{:08x}", c.id),
                    name: c.name.clone(),
                    control_type: c.control_type,
                    min: c.minimum,
                    max: c.maximum,
                    step: c.step,
                    default: c.default_value,
                    flags: c.flag_names(),
                    value: if c.is_write_only() {
                        None
                    } else {
                        cam.device().get_control(c.id).ok().map(i64::from)
                    },
                })
                .collect::<Vec<_>>(),
        )
    } else {
        None
    };

    let json = InfoJson {
        model: info.model.name(),
        model_id: info.model,
        tested: info.model.is_tested(),
        device: &info.control_node,
        state,
        usb: UsbJson {
            vendor_id: format!("{:04x}", info.usb.vendor_id),
            product_id: format!("{:04x}", info.usb.product_id),
            manufacturer: info.usb.manufacturer.as_deref(),
            product: info.usb.product.as_deref(),
            serial: info.usb.serial.as_deref(),
            bus: info.usb.bus,
            address: info.usb.address,
            port_path: &info.usb.port_path,
            sysfs_path: &info.usb.sysfs_path,
            driver: info.usb.driver.as_deref(),
            driver_version: &info.driver_version,
        },
        control_node: &info.control_node,
        stream_node: &info.stream_node,
        video_nodes: &info.video_nodes,
        media_node: info.media_node.as_deref(),
        card: &info.card,
        bus_info: &info.bus_info,
        extension_units: extension_units.clone(),
        pan_range_degrees: pan_deg,
        tilt_range_degrees: tilt_deg,
        zoom_range: zoom_x,
        supported: supported.clone(),
        ai_mode,
        ai_mode_payload_len: ai_len,
        controls,
    };

    ctx.out.emit(
        || {
            render(
                info,
                state,
                &extension_units,
                pan_deg,
                tilt_deg,
                zoom_x,
                &supported,
                ai_mode.map(|m| (m, ai_len.unwrap_or(0))),
                json.controls.as_deref(),
                &activity,
            )
        },
        &json,
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn render(
    info: &DeviceInfo,
    state: &str,
    units: &[ExtensionUnit],
    pan: Option<(f64, f64)>,
    tilt: Option<(f64, f64)>,
    zoom: Option<(f64, f64)>,
    supported: &[&str],
    ai_mode: Option<(link2::AiMode, usize)>,
    controls: Option<&[ControlJson]>,
    activity: &crate::camera::activity::Activity,
) -> String {
    let mut o = String::new();
    let p = |o: &mut String, s: String| {
        o.push_str(&s);
        o.push('\n');
    };
    p(&mut o, info.model.name().to_string());
    if !info.model.is_tested() {
        p(&mut o, "  (recognised, not validated with linkctl)".into());
    }
    p(&mut o, String::new());
    p(&mut o, "USB".into());
    p(&mut o, format!("  VID:     {:04x}", info.usb.vendor_id));
    p(&mut o, format!("  PID:     {:04x}", info.usb.product_id));
    if let Some(m) = &info.usb.manufacturer {
        p(&mut o, format!("  Vendor:  {m}"));
    }
    if let Some(pr) = &info.usb.product {
        p(&mut o, format!("  Product: {pr}"));
    }
    if let Some(s) = &info.usb.serial {
        p(&mut o, format!("  Serial:  {s}"));
    }
    p(&mut o, format!("  Port:    {}", info.usb.port_path));
    if let (Some(b), Some(a)) = (info.usb.bus, info.usb.address) {
        p(&mut o, format!("  Address: bus {b} device {a}"));
    }
    p(
        &mut o,
        format!(
            "  Driver:  {} (v{})",
            info.usb.driver.as_deref().unwrap_or("?"),
            info.driver_version
        ),
    );
    p(
        &mut o,
        format!("  Sysfs:   {}", info.usb.sysfs_path.display()),
    );
    p(&mut o, String::new());
    p(&mut o, "Devices".into());
    p(
        &mut o,
        format!("  Control: {}", info.control_node.display()),
    );
    p(&mut o, format!("  Stream:  {}", info.stream_node.display()));
    for n in &info.video_nodes {
        let role = match n.role {
            NodeRole::Capture => "video capture",
            NodeRole::Metadata => "metadata",
            NodeRole::Unknown => "unknown",
        };
        p(&mut o, format!("  Node:    {}  ({role})", n.path.display()));
    }
    if let Some(m) = &info.media_node {
        p(&mut o, format!("  Media:   {}", m.display()));
    }
    p(&mut o, format!("  Card:    {}", info.card));
    p(&mut o, format!("  Bus:     {}", info.bus_info));
    p(&mut o, String::new());
    p(&mut o, "State".into());
    p(&mut o, format!("  {state}"));
    for h in &activity.holders {
        p(&mut o, format!("  used by {} (pid {})", h.comm, h.pid));
    }
    p(&mut o, String::new());
    p(&mut o, "Controls".into());
    match pan {
        Some((a, b)) => p(
            &mut o,
            format!("  Pan:   {} .. {}", format_degrees(a), format_degrees(b)),
        ),
        None => p(&mut o, "  Pan:   not supported".into()),
    }
    match tilt {
        Some((a, b)) => p(
            &mut o,
            format!("  Tilt:  {} .. {}", format_degrees(a), format_degrees(b)),
        ),
        None => p(&mut o, "  Tilt:  not supported".into()),
    }
    match zoom {
        Some((a, b)) => p(
            &mut o,
            format!("  Zoom:  {} .. {}", format_zoom(a), format_zoom(b)),
        ),
        None => p(&mut o, "  Zoom:  not supported".into()),
    }
    p(
        &mut o,
        format!(
            "  Other: {}",
            if supported.is_empty() {
                "none".to_string()
            } else {
                supported.join(", ")
            }
        ),
    );
    p(&mut o, String::new());
    p(&mut o, "Extension units (vendor, experimental)".into());
    if units.is_empty() {
        p(&mut o, "  none found in USB descriptor".into());
    }
    for u in units {
        p(
            &mut o,
            format!(
                "  Unit {:>2}: {}  {} controls{}",
                u.id,
                u.guid,
                u.num_controls,
                u.role.map(|r| format!("  [{r}]")).unwrap_or_default()
            ),
        );
    }
    if let Some((m, len)) = ai_mode {
        p(
            &mut o,
            format!(
                "  AI mode: {} (unit 9 selector 0x02, {len}-byte payload)",
                m.label()
            ),
        );
    }
    if let Some(list) = controls {
        p(&mut o, String::new());
        p(&mut o, "V4L2 controls".into());
        for c in list {
            let flags = if c.flags.is_empty() {
                String::new()
            } else {
                format!("  flags={}", c.flags.join(","))
            };
            let value = c.value.map(|v| v.to_string()).unwrap_or_else(|| "-".into());
            p(
                &mut o,
                format!(
                    "  {:<28} {} ({:<4}) min={} max={} step={} default={} value={}{flags}",
                    c.name,
                    c.id,
                    c.control_type.as_str(),
                    c.min,
                    c.max,
                    c.step,
                    c.default,
                    value
                ),
            );
        }
    }
    o.trim_end().to_string()
}
