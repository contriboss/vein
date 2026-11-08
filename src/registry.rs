//! The registries this build serves: the `gems`, `crates` and `npm`
//! features. A registry built out has no routes, search, docs, catalog,
//! release delay or MCP resources, and a config naming it is refused.

use vein_adapter::AssetKind;

#[cfg(not(any(feature = "gems", feature = "crates", feature = "npm")))]
compile_error!("vein serves at least one registry: enable `gems`, `crates` or `npm`");

/// Whether this build serves registry `kind`.
#[must_use]
pub const fn built(kind: AssetKind) -> bool {
    match kind {
        AssetKind::Gem | AssetKind::Spec => cfg!(feature = "gems"),
        AssetKind::Crate => cfg!(feature = "crates"),
        AssetKind::NpmPackage => cfg!(feature = "npm"),
    }
}

/// The Cargo feature that builds registry `kind` in.
#[must_use]
pub const fn feature(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Gem | AssetKind::Spec => "gems",
        AssetKind::Crate => "crates",
        AssetKind::NpmPackage => "npm",
    }
}

/// The registry a command means when it names none: the first this build
/// serves.
#[must_use]
pub const fn primary() -> AssetKind {
    if cfg!(feature = "gems") {
        AssetKind::Gem
    } else if cfg!(feature = "crates") {
        AssetKind::Crate
    } else {
        AssetKind::NpmPackage
    }
}

/// `kind`, if this build serves it.
pub const fn require(kind: AssetKind) -> Result<AssetKind, NotBuilt> {
    if built(kind) {
        Ok(kind)
    } else {
        Err(NotBuilt(kind))
    }
}

/// `RubyGems`, `crates.io`, `npm`: the registry, as people name it.
#[must_use]
pub const fn name(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Gem | AssetKind::Spec => "RubyGems",
        AssetKind::Crate => "crates.io",
        AssetKind::NpmPackage => "npm",
    }
}

/// `gem`, `crate`, `npm package`.
#[must_use]
pub const fn noun(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Gem | AssetKind::Spec => "gem",
        AssetKind::Crate => "crate",
        AssetKind::NpmPackage => "npm package",
    }
}

/// Asked of a registry this build left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotBuilt(pub AssetKind);

impl std::fmt::Display for NotBuilt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "this vein serves no {}s: it was built without the `{}` feature",
            noun(self.0),
            feature(self.0)
        )
    }
}

impl std::error::Error for NotBuilt {}

/// The registries this build serves, as `[catalog] registries` names them.
#[must_use]
pub fn served() -> Vec<AssetKind> {
    [AssetKind::Gem, AssetKind::Crate, AssetKind::NpmPackage]
        .into_iter()
        .filter(|&kind| built(kind))
        .collect()
}

#[cfg(test)]
mod tests;

/// That `name` could be a package of registry `kind`, built into vein: it
/// goes into proxy paths and storage keys, so nothing that could leave one.
pub fn check_name(kind: AssetKind, name: &str) -> anyhow::Result<()> {
    if !built(kind) {
        return Err(NotBuilt(kind).into());
    }
    let segment = |part: &str| {
        !part.is_empty()
            && !part.starts_with('.')
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    };
    let valid = name.len() <= 214
        && match (kind, name.strip_prefix('@')) {
            (AssetKind::NpmPackage, Some(scoped)) => scoped
                .split_once('/')
                .is_some_and(|(scope, rest)| segment(scope) && segment(rest)),
            _ => segment(name),
        };
    if valid {
        Ok(())
    } else {
        anyhow::bail!("`{name}` is not a {} name", noun(kind))
    }
}
