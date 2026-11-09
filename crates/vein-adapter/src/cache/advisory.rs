//! Security advisories, as vein keeps them: what each says and which
//! versions of which packages it affects. Matching a version against them is
//! the reader's job; the store only answers "every advisory for this
//! package".

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use super::types::AssetKind;

/// One advisory (a GHSA), and the packages vein serves that it affects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advisory {
    /// `GHSA-xxxx-xxxx-xxxx`.
    pub id: String,
    /// Other ids for the same issue: CVEs, `RUSTSEC-`, `GMS-`.
    pub aliases: Vec<String>,
    /// `LOW`, `MODERATE`, `HIGH` or `CRITICAL`, as GitHub rates it.
    pub severity: Option<String>,
    pub summary: Option<String>,
    pub published: Option<Timestamp>,
    pub modified: Timestamp,
    pub packages: Vec<AffectedPackage>,
}

/// The versions of one package an advisory affects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AffectedPackage {
    pub kind: AssetKind,
    pub name: String,
    pub affected: Affected,
}

/// Affected versions: intervals, and versions listed one by one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Affected {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub intervals: Vec<Interval>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub versions: Vec<String>,
}

/// From `introduced` (every version when absent) up to `fixed`
/// (exclusive) or `last_affected` (inclusive); open-ended when neither.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Interval {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_affected: Option<String>,
}

/// One import from the advisory database, applied at once.
#[derive(Debug, Clone, Copy)]
pub struct AdvisoryImport<'a> {
    /// `github`: which database `revision` is a commit of.
    pub source: &'a str,
    pub revision: &'a str,
    /// Every advisory vein holds is replaced by `upserts` (a first import,
    /// or one whose previous revision is gone).
    pub full: bool,
    pub upserts: &'a [Advisory],
    /// Advisories withdrawn, deleted, or no longer about a package vein
    /// serves. Unknown ids are ignored.
    pub removed: &'a [String],
}
