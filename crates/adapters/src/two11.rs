//! 211 Counts food-pantry request volume — the hero leading indicator.
//!
//! STUB. 211 data access is a partnership with the local United Way / 211
//! operator under a data-sharing agreement (NNIP publishes a sample DSA to
//! template). This module exists so the source is registered, visibly
//! disabled, and counted against coverage — absence of the hero source is
//! itself surfaced, never hidden.
//!
//! When the agreement lands: nightly pull of pantry-request counts by ZIP,
//! crosswalked to tracts (HUD ZIP-tract crosswalk), emitted as
//! `pantry_capacity`-class demand signals with low-medium gameability.

pub const SOURCE_ID: &str = "two11";
pub const STATUS: &str = "awaiting data-sharing agreement with NYC/Westchester 211 operator";
