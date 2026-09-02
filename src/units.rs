//! Pure conversions between human-friendly units and raw V4L2 values, plus
//! range validation and relative-movement arithmetic.
//!
//! Nothing in this module touches hardware, so it is fully unit-tested.

/// Arc seconds per degree, as used by `V4L2_CID_PAN_ABSOLUTE` and
/// `V4L2_CID_TILT_ABSOLUTE` (Linux V4L2 spec: "in arc seconds").
pub const ARCSEC_PER_DEGREE: i64 = 3600;

/// Scale factor of `V4L2_CID_ZOOM_ABSOLUTE` on the Link 2: `100` == `1.0x`.
/// Confirmed against the device (`zoom_absolute min=100 max=400`).
pub const ZOOM_RAW_PER_X: f64 = 100.0;

/// Convert degrees to arc seconds, rounding to the nearest unit.
pub fn degrees_to_arcsec(degrees: f64) -> i64 {
    (degrees * ARCSEC_PER_DEGREE as f64).round() as i64
}

/// Convert arc seconds to degrees.
pub fn arcsec_to_degrees(arcsec: i64) -> f64 {
    arcsec as f64 / ARCSEC_PER_DEGREE as f64
}

/// Convert a zoom multiplier (`1.0` .. `4.0`) to the raw control value.
pub fn zoom_to_raw(zoom: f64) -> i64 {
    (zoom * ZOOM_RAW_PER_X).round() as i64
}

/// Convert a raw zoom control value to a multiplier.
pub fn raw_to_zoom(raw: i64) -> f64 {
    raw as f64 / ZOOM_RAW_PER_X
}

/// Inclusive integer range with a step, as reported by `VIDIOC_QUERYCTRL`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub min: i64,
    pub max: i64,
    pub step: i64,
}

impl Range {
    pub const fn new(min: i64, max: i64, step: i64) -> Self {
        Self { min, max, step }
    }

    /// True if `value` is inside `[min, max]` (step is not checked).
    pub fn contains(&self, value: i64) -> bool {
        value >= self.min && value <= self.max
    }

    /// Clamp `value` into the range and snap it onto the step grid anchored
    /// at `min`, rounding to the nearest step. This mirrors what the kernel
    /// control framework does, but doing it ourselves means the value we
    /// report back to the user is the value the device actually received.
    pub fn clamp_and_snap(&self, value: i64) -> i64 {
        let clamped = value.clamp(self.min, self.max);
        if self.step <= 1 {
            return clamped;
        }
        let offset = clamped - self.min;
        // Round to nearest step (half-up for positive offsets).
        let steps = (offset + self.step / 2).div_euclid(self.step);
        let snapped = self.min + steps * self.step;
        snapped.clamp(self.min, self.max)
    }

    /// Validate that `value` lies within the range, returning a message
    /// suitable for the user otherwise. Snapping is applied on success.
    pub fn validate(&self, value: i64, what: &str, unit: &str) -> Result<i64, String> {
        if !self.contains(value) {
            return Err(format!(
                "{what} {value}{unit} is out of range ({}{unit} .. {}{unit}).",
                self.min, self.max
            ));
        }
        Ok(self.clamp_and_snap(value))
    }
}

/// Compute the target of a relative move: `current + delta`, clamped into
/// the range. A move that is already at the limit yields the limit itself.
pub fn relative_target(current: i64, delta: i64, range: Range) -> i64 {
    range.clamp_and_snap(current.saturating_add(delta))
}

/// Format degrees for humans: integral values without decimals, otherwise one
/// decimal place.
pub fn format_degrees(degrees: f64) -> String {
    if (degrees - degrees.round()).abs() < 1e-6 {
        format!("{}°", degrees.round() as i64)
    } else {
        format!("{degrees:.1}°")
    }
}

/// Format a zoom multiplier like `1.0x` / `1.5x` / `2.25x`.
pub fn format_zoom(zoom: f64) -> String {
    let s = format!("{zoom:.2}");
    let s = s.trim_end_matches('0');
    let s = if s.ends_with('.') {
        format!("{s}0")
    } else {
        s.to_string()
    };
    format!("{s}x")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn degrees_round_trip() {
        assert_eq!(degrees_to_arcsec(10.0), 36000);
        assert_eq!(degrees_to_arcsec(20.0), 72000);
        assert_eq!(degrees_to_arcsec(30.0), 108000);
        assert_eq!(degrees_to_arcsec(-30.0), -108000);
        assert_eq!(degrees_to_arcsec(0.5), 1800);
        assert_eq!(arcsec_to_degrees(108000), 30.0);
        assert_eq!(arcsec_to_degrees(-522000), -145.0);
        assert_eq!(arcsec_to_degrees(360000), 100.0);
    }

    #[test]
    fn zoom_round_trip() {
        assert_eq!(zoom_to_raw(1.0), 100);
        assert_eq!(zoom_to_raw(1.5), 150);
        assert_eq!(zoom_to_raw(2.0), 200);
        assert_eq!(zoom_to_raw(4.0), 400);
        assert_eq!(raw_to_zoom(100), 1.0);
        assert_eq!(raw_to_zoom(150), 1.5);
        assert_eq!(raw_to_zoom(400), 4.0);
    }

    #[test]
    fn range_contains_and_clamp() {
        let r = Range::new(-522000, 522000, 3600);
        assert!(r.contains(0));
        assert!(r.contains(-522000));
        assert!(r.contains(522000));
        assert!(!r.contains(522001));
        assert_eq!(r.clamp_and_snap(600000), 522000);
        assert_eq!(r.clamp_and_snap(-600000), -522000);
        assert_eq!(r.clamp_and_snap(36000), 36000);
    }

    #[test]
    fn range_snaps_to_step() {
        let r = Range::new(-522000, 522000, 3600);
        // 10.4° -> 10°, 10.6° -> 11°
        assert_eq!(r.clamp_and_snap(degrees_to_arcsec(10.4)), 36000);
        assert_eq!(r.clamp_and_snap(degrees_to_arcsec(10.6)), 39600);
        assert_eq!(r.clamp_and_snap(degrees_to_arcsec(-10.4)), -36000);
        // Ranges anchored at a non-multiple minimum snap relative to min.
        let odd = Range::new(-100, 100, 30);
        assert_eq!(odd.clamp_and_snap(0), -10);
        assert_eq!(odd.clamp_and_snap(5), 20);
        // Step 1 never changes values.
        let one = Range::new(0, 100, 1);
        assert_eq!(one.clamp_and_snap(37), 37);
        assert_eq!(one.clamp_and_snap(1000), 100);
    }

    #[test]
    fn range_validate_rejects_out_of_range() {
        let r = Range::new(100, 400, 1);
        assert_eq!(r.validate(150, "Zoom", ""), Ok(150));
        let err = r.validate(500, "Zoom", "").unwrap_err();
        assert!(err.contains("out of range"));
        assert!(err.contains("100"));
        assert!(err.contains("400"));
    }

    #[test]
    fn relative_moves_and_clamping() {
        let r = Range::new(-522000, 522000, 3600);
        // 5° default step from 0.
        assert_eq!(relative_target(0, degrees_to_arcsec(5.0), r), 18000);
        assert_eq!(relative_target(0, degrees_to_arcsec(-5.0), r), -18000);
        // From -15° moving right 20° -> 5°.
        assert_eq!(
            relative_target(degrees_to_arcsec(-15.0), degrees_to_arcsec(20.0), r),
            18000
        );
        // Clamped at the limit.
        assert_eq!(
            relative_target(degrees_to_arcsec(140.0), degrees_to_arcsec(20.0), r),
            522000
        );
        assert_eq!(relative_target(522000, 3600, r), 522000);
        // Never overflows.
        assert_eq!(relative_target(i64::MAX, 1, r), 522000);
    }

    #[test]
    fn formatting() {
        assert_eq!(format_degrees(30.0), "30°");
        assert_eq!(format_degrees(-15.0), "-15°");
        assert_eq!(format_degrees(4.5), "4.5°");
        assert_eq!(format_zoom(1.0), "1.0x");
        assert_eq!(format_zoom(1.5), "1.5x");
        assert_eq!(format_zoom(2.25), "2.25x");
        assert_eq!(format_zoom(4.0), "4.0x");
    }
}
