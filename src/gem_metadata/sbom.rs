//! CycloneDX SBOM generation with recursive dependency resolution.

use anyhow::{anyhow, Context, Result};
use cyclonedx_bom::{
    models::{
        component::{Classification, Component, Components},
        dependency::{Dependencies, Dependency},
        hash::{Hash, HashAlgorithm, HashValue, Hashes},
        license::{LicenseChoice, Licenses},
        metadata::Metadata as BomMetadata,
        property::{Properties, Property},
        tool::{Tool, Tools},
    },
    prelude::{Bom, NormalizedString, Purl, SpecVersion, Validate},
};
use rama::telemetry::tracing::{info, warn};
use state_machines::state_machine;
use std::collections::HashSet;
use vein_adapter::{CacheBackend, CacheBackendTrait, DependencyKind, GemMetadata};

use super::version_req::find_latest_matching;

state_machine! {
    name: SbomFlow,
    initial: Pending,
    states: [Pending, Ready],
    events {
        reuse {
            transition: { from: Pending, to: Ready }
        }
        compute {
            transition: { from: Pending, to: Ready }
        }
    }
}

#[derive(Debug)]
struct SbomContext {
    existing: Option<serde_json::Value>,
    result: Option<serde_json::Value>,
    reused: bool,
}

impl SbomContext {
    fn new(existing: Option<serde_json::Value>) -> Self {
        Self {
            existing,
            result: None,
            reused: false,
        }
    }

    fn has_existing(&self) -> bool {
        self.existing.is_some()
    }

    fn reuse_existing(&mut self) -> Result<()> {
        let sbom = self
            .existing
            .take()
            .context("attempted to reuse SBOM but none was available")?;
        self.result = Some(sbom);
        self.reused = true;
        Ok(())
    }

    fn set_computed(&mut self, sbom: Option<serde_json::Value>) {
        self.result = sbom;
        self.reused = false;
    }

    fn into_result(self) -> (Option<serde_json::Value>, bool) {
        (self.result, self.reused)
    }
}

impl<S> SbomFlow<SbomContext, S> {
    fn ctx(&self) -> &SbomContext {
        &self.ctx
    }

    fn ctx_mut(&mut self) -> &mut SbomContext {
        &mut self.ctx
    }

    fn into_ctx(self) -> SbomContext {
        self.ctx
    }
}

impl SbomFlow<SbomContext, Pending> {
    fn mark_reused(mut self) -> Result<SbomFlow<SbomContext, Ready>> {
        self.ctx_mut().reuse_existing()?;
        self.reuse()
            .map_err(|err| anyhow!("failed to transition SBOM flow to ready (reuse): {err:?}"))
    }

    fn mark_computed(
        mut self,
        sbom: Option<serde_json::Value>,
    ) -> Result<SbomFlow<SbomContext, Ready>> {
        self.ctx_mut().set_computed(sbom);
        self.compute()
            .map_err(|err| anyhow!("failed to transition SBOM flow to ready (compute): {err:?}"))
    }
}

/// Default maximum depth for recursive dependency resolution.
const DEFAULT_MAX_DEPTH: usize = 10;

/// Generate a CycloneDX SBOM for a gem (non-recursive, for backward compatibility).
pub fn generate_cyclonedx_sbom(
    metadata: &GemMetadata,
    existing_sbom: Option<serde_json::Value>,
) -> Result<Option<serde_json::Value>> {
    let flow = SbomFlow::new(SbomContext::new(existing_sbom));
    if flow.ctx().has_existing() {
        let flow = flow.mark_reused()?;
        let (result, reused) = flow.into_ctx().into_result();
        if reused {
            info!(
                event = "sbom.reuse",
                gem = %metadata.name,
                version = %metadata.version,
                platform = %metadata.platform,
                "reused cached CycloneDX SBOM"
            );
        }
        return Ok(result);
    }

    let computed = compute_cyclonedx_sbom(metadata, &[], &[])?;
    let flow = flow.mark_computed(computed)?;
    let (result, reused) = flow.into_ctx().into_result();
    if !reused {
        if result.is_some() {
            info!(
                event = "sbom.compute",
                gem = %metadata.name,
                version = %metadata.version,
                platform = %metadata.platform,
                "generated CycloneDX SBOM"
            );
        } else {
            info!(
                event = "sbom.absent",
                gem = %metadata.name,
                version = %metadata.version,
                platform = %metadata.platform,
                "gem provided no SBOM-compatible metadata payload"
            );
        }
    }
    Ok(result)
}

/// Resolved dependency with its metadata (if available).
struct ResolvedDep {
    name: String,
    version: String,
    bom_ref: String,
    /// Full metadata if available, None if only basic info
    metadata: Option<GemMetadata>,
}

/// Generate a CycloneDX SBOM with recursive dependency resolution.
///
/// Resolves dependencies to their latest matching versions and includes them
/// as proper CycloneDX components.
pub async fn generate_cyclonedx_sbom_recursive(
    metadata: &GemMetadata,
    existing_sbom: Option<serde_json::Value>,
    cache: &CacheBackend,
    max_depth: Option<usize>,
) -> Result<Option<serde_json::Value>> {
    let flow = SbomFlow::new(SbomContext::new(existing_sbom));
    if flow.ctx().has_existing() {
        let flow = flow.mark_reused()?;
        let (result, reused) = flow.into_ctx().into_result();
        if reused {
            info!(
                event = "sbom.reuse",
                gem = %metadata.name,
                version = %metadata.version,
                platform = %metadata.platform,
                "reused cached CycloneDX SBOM"
            );
        }
        return Ok(result);
    }

    // Resolve all dependencies recursively
    let mut visited = HashSet::new();
    let root_ref = format!("pkg:gem/{}@{}", metadata.name, metadata.version);
    visited.insert(root_ref.clone());

    let resolved = resolve_dependencies_recursive(
        &metadata.dependencies,
        cache,
        &mut visited,
        0,
        max_depth.unwrap_or(DEFAULT_MAX_DEPTH),
    )
    .await;

    // Build components and dependencies lists
    let (components, dep_graph) = build_components_and_deps(metadata, &resolved);

    let computed = compute_cyclonedx_sbom(metadata, &components, &dep_graph)?;
    let flow = flow.mark_computed(computed)?;
    let (result, reused) = flow.into_ctx().into_result();

    if !reused {
        if result.is_some() {
            info!(
                event = "sbom.compute.recursive",
                gem = %metadata.name,
                version = %metadata.version,
                platform = %metadata.platform,
                dep_count = resolved.len(),
                "generated recursive CycloneDX SBOM"
            );
        } else {
            info!(
                event = "sbom.absent",
                gem = %metadata.name,
                version = %metadata.version,
                platform = %metadata.platform,
                "gem provided no SBOM-compatible metadata payload"
            );
        }
    }
    Ok(result)
}

/// Recursively resolve dependencies to their latest matching versions.
async fn resolve_dependencies_recursive(
    deps: &[vein_adapter::GemDependency],
    cache: &CacheBackend,
    visited: &mut HashSet<String>,
    depth: usize,
    max_depth: usize,
) -> Vec<ResolvedDep> {
    if depth >= max_depth {
        return Vec::new();
    }

    let mut resolved = Vec::new();

    for dep in deps {
        // Only resolve runtime dependencies for the SBOM
        if dep.kind != DependencyKind::Runtime {
            continue;
        }

        // Get available versions for this gem
        let versions = cache
            .get_gem_versions_for_index(&dep.name)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|v| v.version)
            .collect::<Vec<_>>();

        // Find latest matching version, or use requirement as version placeholder
        let resolved_version = find_latest_matching(&versions, &dep.requirement)
            .unwrap_or_else(|| dep.requirement.clone());

        // Create bom-ref for cycle detection
        let bom_ref = format!("pkg:gem/{}@{}", dep.name, resolved_version);
        if visited.contains(&bom_ref) {
            continue; // Already processed
        }
        visited.insert(bom_ref.clone());

        // Try to fetch metadata
        let dep_metadata = cache
            .gem_metadata(&dep.name, &resolved_version, Some("ruby"))
            .await
            .ok()
            .flatten();

        // Recursively resolve this dependency's dependencies (if we have metadata)
        let transitive = if let Some(ref meta) = dep_metadata {
            Box::pin(resolve_dependencies_recursive(
                &meta.dependencies,
                cache,
                visited,
                depth + 1,
                max_depth,
            ))
            .await
        } else {
            Vec::new()
        };

        resolved.push(ResolvedDep {
            name: dep.name.clone(),
            version: resolved_version,
            bom_ref,
            metadata: dep_metadata,
        });

        resolved.extend(transitive);
    }

    resolved
}

/// Build CycloneDX components and dependency graph from resolved deps.
fn build_components_and_deps(
    root: &GemMetadata,
    resolved: &[ResolvedDep],
) -> (Vec<Component>, Vec<(String, Vec<String>)>) {
    let mut components = Vec::new();
    let mut dep_graph = Vec::new();

    // Root component's dependencies
    let root_ref = format!("pkg:gem/{}@{}", root.name, root.version);
    let root_deps: Vec<String> = root
        .dependencies
        .iter()
        .filter(|d| d.kind == DependencyKind::Runtime)
        .filter_map(|d| {
            // Find the resolved version for this dep
            resolved
                .iter()
                .find(|r| r.name == d.name)
                .map(|r| r.bom_ref.clone())
        })
        .collect();
    if !root_deps.is_empty() {
        dep_graph.push((root_ref, root_deps));
    }

    // Build component for each resolved dependency
    for resolved_dep in resolved {
        let mut component = Component::new(
            Classification::Library,
            &resolved_dep.name,
            &resolved_dep.version,
            Some(resolved_dep.bom_ref.clone()),
        );

        // Set PURL (always available from name/version)
        if let Ok(purl) = Purl::new("gem", &resolved_dep.name, &resolved_dep.version) {
            component.purl = Some(purl);
        }

        // Add rich metadata if available
        if let Some(ref meta) = resolved_dep.metadata {
            if let Some(desc) = meta.description.as_deref().or(meta.summary.as_deref()) {
                component.description = Some(NormalizedString::new(desc));
            }

            component.group = Some(NormalizedString::new(&meta.platform));

            if !meta.sha256.is_empty() {
                component.hashes = Some(Hashes(vec![Hash {
                    alg: HashAlgorithm::SHA_256,
                    content: HashValue(meta.sha256.clone()),
                }]));
            }

            // Add licenses
            let license_choices: Vec<LicenseChoice> = meta
                .licenses
                .iter()
                .filter_map(|license| {
                    let trimmed = license.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(LicenseChoice::license(trimmed))
                    }
                })
                .collect();
            if !license_choices.is_empty() {
                component.licenses = Some(Licenses(license_choices));
            }

            // Add this component's dependencies to the graph
            let comp_deps: Vec<String> = meta
                .dependencies
                .iter()
                .filter(|d| d.kind == DependencyKind::Runtime)
                .filter_map(|d| {
                    resolved
                        .iter()
                        .find(|r| r.name == d.name)
                        .map(|r| r.bom_ref.clone())
                })
                .collect();
            if !comp_deps.is_empty() {
                dep_graph.push((resolved_dep.bom_ref.clone(), comp_deps));
            }
        }

        components.push(component);
    }

    (components, dep_graph)
}

fn compute_cyclonedx_sbom(
    metadata: &GemMetadata,
    dep_components: &[Component],
    dep_graph: &[(String, Vec<String>)],
) -> Result<Option<serde_json::Value>> {
    let root_ref = format!("pkg:gem/{}@{}", metadata.name, metadata.version);

    let mut component = Component::new(
        Classification::Library,
        &metadata.name,
        &metadata.version,
        Some(root_ref.clone()),
    );

    if let Some(desc) = metadata
        .description
        .as_deref()
        .or(metadata.summary.as_deref())
    {
        component.description = Some(NormalizedString::new(desc));
    }

    component.group = Some(NormalizedString::new(&metadata.platform));

    let author_list: Vec<_> = metadata
        .authors
        .iter()
        .map(|author| author.trim())
        .filter(|author| !author.is_empty())
        .collect();
    if !author_list.is_empty() {
        component.author = Some(NormalizedString::new(&author_list.join(", ")));
    }

    let license_choices: Vec<LicenseChoice> = metadata
        .licenses
        .iter()
        .filter_map(|license| {
            let trimmed = license.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(LicenseChoice::license(trimmed))
            }
        })
        .collect();
    if !license_choices.is_empty() {
        component.licenses = Some(Licenses(license_choices));
    }

    component.hashes = Some(Hashes(vec![Hash {
        alg: HashAlgorithm::SHA_256,
        content: HashValue(metadata.sha256.clone()),
    }]));

    if let Ok(purl) = Purl::new("gem", &metadata.name, &metadata.version) {
        component.purl = Some(purl);
    }

    let mut properties = Vec::new();

    properties.push(Property::new("vein:platform", &metadata.platform));

    properties.push(Property::new(
        "vein:has-native-extensions",
        if metadata.has_native_extensions {
            "true"
        } else {
            "false"
        },
    ));

    properties.push(Property::new(
        "vein:has-embedded-binaries",
        if metadata.has_embedded_binaries {
            "true"
        } else {
            "false"
        },
    ));

    properties.push(Property::new(
        "vein:size-bytes",
        &metadata.size_bytes.to_string(),
    ));

    if let Some(built_at) = metadata.built_at.as_deref() {
        properties.push(Property::new("vein:built-at", built_at));
    }

    if let Some(required_ruby) = metadata.required_ruby_version.as_deref() {
        properties.push(Property::new("vein:required-ruby-version", required_ruby));
    }

    if let Some(required_rubygems) = metadata.required_rubygems_version.as_deref() {
        properties.push(Property::new(
            "vein:required-rubygems-version",
            required_rubygems,
        ));
    }

    if let Some(rubygems_version) = metadata.rubygems_version.as_deref() {
        properties.push(Property::new("vein:rubygems-version", rubygems_version));
    }

    if let Some(spec_version) = metadata.specification_version {
        properties.push(Property::new(
            "vein:specification-version",
            &spec_version.to_string(),
        ));
    }

    if !metadata.executables.is_empty() {
        properties.push(Property::new(
            "vein:executables",
            &metadata.executables.join(", "),
        ));
    }

    if !metadata.extensions.is_empty() {
        properties.push(Property::new(
            "vein:extensions",
            &metadata.extensions.join(", "),
        ));
    }

    if !metadata.emails.is_empty() {
        properties.push(Property::new("vein:emails", &metadata.emails.join(", ")));
    }

    if let Some(homepage) = metadata.homepage.as_deref() {
        properties.push(Property::new("vein:homepage", homepage));
    }

    if let Some(documentation) = metadata.documentation_url.as_deref() {
        properties.push(Property::new("vein:documentation-url", documentation));
    }

    if let Some(changelog) = metadata.changelog_url.as_deref() {
        properties.push(Property::new("vein:changelog-url", changelog));
    }

    if let Some(source_url) = metadata.source_code_url.as_deref() {
        properties.push(Property::new("vein:source-url", source_url));
    }

    if let Some(bugs) = metadata.bug_tracker_url.as_deref() {
        properties.push(Property::new("vein:bug-tracker-url", bugs));
    }

    if let Some(wiki) = metadata.wiki_url.as_deref() {
        properties.push(Property::new("vein:wiki-url", wiki));
    }

    if let Some(funding) = metadata.funding_url.as_deref() {
        properties.push(Property::new("vein:funding-url", funding));
    }

    if !properties.is_empty() {
        component.properties = Some(Properties(properties));
    }

    let mut bom_metadata = BomMetadata::new().unwrap_or_default();
    bom_metadata.component = Some(component.clone());
    bom_metadata.tools = Some(Tools::List(vec![Tool::new(
        "Ore Ecosystem",
        "Vein",
        env!("CARGO_PKG_VERSION"),
    )]));

    if let Some(component_licenses) = component.licenses.clone() {
        bom_metadata.licenses = Some(component_licenses);
    }

    // Build the BOM with components and dependencies
    let mut bom = Bom {
        spec_version: SpecVersion::V1_5,
        metadata: Some(bom_metadata),
        ..Bom::default()
    };

    // Add resolved components
    if !dep_components.is_empty() {
        bom.components = Some(Components(dep_components.to_vec()));
    }

    // Add dependency graph
    if !dep_graph.is_empty() {
        let deps: Vec<Dependency> = dep_graph
            .iter()
            .map(|(ref_id, child_deps)| Dependency {
                dependency_ref: ref_id.clone(),
                dependencies: child_deps.clone(),
            })
            .collect();
        bom.dependencies = Some(Dependencies(deps));
    }

    let validation = bom.validate_version(SpecVersion::V1_5);
    if !validation.passed() {
        warn!(?validation, "generated CycloneDX SBOM failed validation");
        return Ok(None);
    }

    let mut output = Vec::new();
    bom.output_as_json_v1_5(&mut output)
        .context("serializing CycloneDX SBOM")?;

    let sbom = serde_json::from_slice(&output).context("parsing CycloneDX SBOM json")?;
    Ok(Some(sbom))
}
