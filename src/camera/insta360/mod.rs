//! Insta360-specific (vendor) functionality.
//!
//! Standard PTZ and image controls live in the generic V4L2 layer; this
//! module only holds what genuinely needs the proprietary UVC Extension
//! Units. See `docs/research.md` and `docs/safety.md`.

pub mod link2;
pub mod xu;
