use serde::Serialize;

use super::Context;
use crate::camera::discovery::{self, DeviceInfo};
use crate::error::Result;

#[derive(Serialize)]
struct Row<'a> {
    index: usize,
    model: &'a str,
    model_id: crate::camera::model::Model,
    device: &'a std::path::Path,
    vendor_id: String,
    product_id: String,
    usb_port: &'a str,
    serial: Option<&'a str>,
    tested: bool,
}

pub fn run(ctx: &Context) -> Result<()> {
    let devices: Vec<DeviceInfo> = match ctx.device.as_deref() {
        Some(p) => vec![discovery::discover_device(p)?],
        None => discovery::discover_all()?,
    };
    let rows: Vec<Row> = devices
        .iter()
        .enumerate()
        .map(|(i, d)| Row {
            index: i + 1,
            model: d.model.name(),
            model_id: d.model,
            device: &d.control_node,
            vendor_id: format!("{:04x}", d.usb.vendor_id),
            product_id: format!("{:04x}", d.usb.product_id),
            usb_port: &d.usb.port_path,
            serial: d.usb.serial.as_deref(),
            tested: d.model.is_tested(),
        })
        .collect();
    ctx.out.emit(
        || {
            if rows.is_empty() {
                return "No Insta360 Link cameras found.".to_string();
            }
            rows.iter()
                .map(|r| {
                    format!(
                        "{}  {}  {}  {}:{}",
                        r.index,
                        r.model,
                        r.device.display(),
                        r.vendor_id,
                        r.product_id
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        },
        &rows,
    );
    Ok(())
}
