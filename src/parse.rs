//! Language-specific parsers for extracting package metadata.
//! Only compiled when the corresponding registry feature is enabled.
#![allow(unsafe_code)]

#[cfg(feature = "gems")]
pub mod ruby;

#[cfg(feature = "crates")]
pub mod rust;

#[cfg(feature = "npm")]
pub mod typescript;
