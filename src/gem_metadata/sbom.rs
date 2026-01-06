use anyhow::{anyhow, Context, Result};
use cyclonedx_bom::{
    models::{
        component::{Classification, Component},
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
use vein_adapter::{DependencyKind, GemMetadata};

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

    let computed = compute_cyclonedx_sbom(metadata)?;
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

fn compute_cyclonedx_sbom(metadata: &GemMetadata) -> Result<Option<serde_json::Value>> {
    let mut component = Component::new(
        Classification::Library,
        &metadata.name,
        &metadata.version,
        None,
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

    if !metadata.dependencies.is_empty() {
        let deps_summary = metadata
            .dependencies
            .iter()
            .map(|dep| {
                let kind = match dep.kind {
                    DependencyKind::Runtime => "runtime",
                    DependencyKind::Development => "development",
                    DependencyKind::Optional => "optional",
                    DependencyKind::Unknown => "unknown",
                };
                format!("{} {} [{}]", dep.name, dep.requirement, kind)
            })
            .collect::<Vec<_>>()
            .join("; ");

        properties.push(Property::new("vein:dependencies", &deps_summary));
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

    let bom = Bom {
        spec_version: SpecVersion::V1_5,
        metadata: Some(bom_metadata),
        ..Bom::default()
    };

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
