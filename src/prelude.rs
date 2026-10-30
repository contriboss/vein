//! Vein prelude: commonly used imports and re-exports.

pub use rama::bytes;
pub use rama::futures;
pub use rama::net::address::ip::ipnet;
pub use rama::telemetry::tracing::{debug, error, info, warn};

pub use anyhow::{Context, Result, anyhow, bail};
pub use serde::{Deserialize, Serialize};
