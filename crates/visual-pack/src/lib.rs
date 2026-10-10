//! Strict, capability-neutral parser for Luminous Ambient Scene Pack v1.
//!
//! This crate validates manifest structure and semantics without touching the
//! filesystem, display server, GPU, network, or OS lifecycle. Asset paths are
//! declarations only. A trusted host supplies file resolution and SHA-256
//! computation through AssetHashProvider; no path is opened by this crate.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Map as JsonMap, Number, Value};
use sovereign_visual_core::color::Rgba;
use sovereign_visual_core::mycelium::MycelialNetwork;
use sovereign_visual_core::settings::{ScenePalette, SceneSettings, SceneSettingsError};

/// Maximum accepted manifest size. The schema caps collection sizes too, but
/// bounding input before parsing limits unnecessary allocation from hostile input.
pub const MAX_MANIFEST_BYTES: usize = 1_048_576;
/// Engine build currently implemented by the shared Scene Pack reference core.
pub const SUPPORTED_ENGINE_VERSION: &str = "1.0.0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScenePackError {
    ManifestTooLarge,
    InvalidJson(String),
    SchemaViolation(String),
    InvalidValue { field: String, reason: String },
    AssetVerificationFailed { path: String, reason: String },
    AssetDigestMismatch { path: String, expected: String, actual: String },
    InvalidAssetDigest { path: String },
    CoreSettings(SceneSettingsError),
}

impl fmt::Display for ScenePackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ManifestTooLarge => write!(f, "Scene Pack manifest exceeds 1 MiB"),
            Self::InvalidJson(message) => write!(f, "invalid Scene Pack JSON: {message}"),
            Self::SchemaViolation(message) => {
                write!(f, "Scene Pack v1 schema violation: {message}")
            }
            Self::InvalidValue { field, reason } => {
                write!(f, "invalid Scene Pack field {field}: {reason}")
            }
            Self::AssetVerificationFailed { path, reason } => {
                write!(f, "asset verification failed for {path}: {reason}")
            }
            Self::AssetDigestMismatch { path, expected, actual } => write!(
                f,
                "asset SHA-256 mismatch for {path}: expected {expected}, received {actual}"
            ),
            Self::InvalidAssetDigest { path } => {
                write!(f, "asset verifier returned an invalid SHA-256 digest for {path}")
            }
            Self::CoreSettings(error) => write!(f, "core rejected Scene Pack settings: {error}"),
        }
    }
}

impl Error for ScenePackError {}

/// Host capability used only for resolving declared assets and obtaining a
/// vetted SHA-256 digest. Implementations must resolve paths beneath the pack
/// root (including symlink checks) and must not trust arbitrary manifest paths.
pub trait AssetHashProvider {
    fn sha256_hex_for_safe_relative_path(&mut self, path: &str) -> Result<String, String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Motion {
    Full,
    Ambient,
    Reduced,
    Minimal,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Composition {
    #[serde(rename = "centered-network")]
    CenteredNetwork,
    #[serde(rename = "edge-biased-network")]
    EdgeBiasedNetwork,
    #[serde(rename = "wide-network")]
    WideNetwork,
    #[serde(rename = "minimal-network")]
    MinimalNetwork,
    #[serde(rename = "gradient-only")]
    GradientOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum PresentationVariant {
    #[serde(rename = "boot")]
    Boot,
    #[serde(rename = "desktop")]
    Desktop,
    #[serde(rename = "idle")]
    Idle,
    #[serde(rename = "lockedBackground")]
    LockedBackground,
    #[serde(rename = "staticFallback")]
    StaticFallback,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SafeRegion {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ScenePresentation {
    pub motion: Motion,
    pub max_fps: u32,
    pub brightness: f32,
    pub composition: Composition,
    pub safe_regions: Vec<SafeRegion>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Presentations {
    pub boot: ScenePresentation,
    pub desktop: ScenePresentation,
    pub idle: ScenePresentation,
    pub locked_background: ScenePresentation,
    pub static_fallback: ScenePresentation,
}

impl Presentations {
    pub fn get(&self, variant: PresentationVariant) -> &ScenePresentation {
        match variant {
            PresentationVariant::Boot => &self.boot,
            PresentationVariant::Desktop => &self.desktop,
            PresentationVariant::Idle => &self.idle,
            PresentationVariant::LockedBackground => &self.locked_background,
            PresentationVariant::StaticFallback => &self.static_fallback,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum AssetMediaType {
    #[serde(rename = "image/svg+xml")]
    Svg,
    #[serde(rename = "image/png")]
    Png,
    #[serde(rename = "image/webp")]
    Webp,
    #[serde(rename = "image/jpeg")]
    Jpeg,
    #[serde(rename = "video/webm")]
    Webm,
    #[serde(rename = "video/mp4")]
    Mp4,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SceneLicense {
    pub spdx_id: String,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    pub source_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SceneAsset {
    pub asset_id: String,
    pub path: String,
    pub sha256: String,
    pub media_type: AssetMediaType,
    pub license: SceneLicense,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    pub source_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    Gpu,
    Dmabuf,
    Vulkan,
    Egl,
    WaylandLayerShell,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SceneCapabilities {
    pub required: Vec<Capability>,
    pub static_fallback_required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputKind {
    LocalTime,
    PointerPosition,
    AudioLevel,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SceneInput {
    pub id: InputKind,
    pub enabled_by_default: bool,
    pub consent_required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LockAction {
    Pause,
    ReducedMotion,
    Static,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum SuspendAction {
    #[serde(rename = "pause")]
    Pause,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum WakeAction {
    #[serde(rename = "reinitialize-from-seed")]
    ReinitializeFromSeed,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SceneLifecycle {
    pub on_lock: LockAction,
    pub on_suspend: SuspendAction,
    pub on_wake: WakeAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReducedMotionVariant {
    Idle,
    LockedBackground,
    StaticFallback,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SceneAccessibility {
    pub reduced_motion_presentation: ReducedMotionVariant,
    pub color_is_sole_signal: bool,
    pub static_fallback_available: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SceneResourceBudget {
    pub max_fps: u32,
    pub max_branches: u32,
    pub max_memory_mib: u32,
    pub pause_when_hidden: bool,
    pub pause_when_display_asleep: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RawScenePack {
    schema_version: u32,
    scene_id: String,
    scene_version: String,
    title: String,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    description: Option<String>,
    license: SceneLicense,
    assets: Vec<SceneAsset>,
    palette: RawPalette,
    simulation: RawSimulation,
    presentations: Presentations,
    resource_budget: SceneResourceBudget,
    capabilities: SceneCapabilities,
    inputs: Vec<SceneInput>,
    lifecycle: SceneLifecycle,
    accessibility: SceneAccessibility,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RawPalette {
    canvas: String,
    substrate: String,
    filament: String,
    node: String,
    lichen: String,
    glow: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RawSimulation {
    engine: String,
    engine_version: String,
    seed: u32,
    fixed_step_hz: u32,
    parameters: RawSimulationParameters,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RawSimulationParameters {
    branch_limit: u32,
    max_depth: u32,
    growth_rate: f32,
    pulse_period_seconds: f32,
    drift_amplitude: f32,
}

/// An immutable, schema-validated Scene Pack v1. It preserves the host-facing
/// presentation, capability, input and lifecycle metadata rather than silently
/// discarding fields that the reference renderer does not itself enforce.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedScenePack {
    scene_id: String,
    scene_version: String,
    engine_version: String,
    title: String,
    description: Option<String>,
    license: SceneLicense,
    assets: Vec<SceneAsset>,
    seed: u32,
    settings: SceneSettings,
    presentations: Presentations,
    resource_budget: SceneResourceBudget,
    capabilities: SceneCapabilities,
    inputs: Vec<SceneInput>,
    lifecycle: SceneLifecycle,
    accessibility: SceneAccessibility,
}

impl ValidatedScenePack {
    pub fn scene_id(&self) -> &str {
        &self.scene_id
    }

    pub fn scene_version(&self) -> &str {
        &self.scene_version
    }

    pub fn engine_version(&self) -> &str {
        &self.engine_version
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn license(&self) -> &SceneLicense {
        &self.license
    }

    pub fn assets(&self) -> &[SceneAsset] {
        &self.assets
    }

    pub fn seed(&self) -> u32 {
        self.seed
    }

    pub fn settings(&self) -> &SceneSettings {
        &self.settings
    }

    pub fn presentations(&self) -> &Presentations {
        &self.presentations
    }

    pub fn resource_budget(&self) -> &SceneResourceBudget {
        &self.resource_budget
    }

    pub fn capabilities(&self) -> &SceneCapabilities {
        &self.capabilities
    }

    pub fn inputs(&self) -> &[SceneInput] {
        &self.inputs
    }

    pub fn lifecycle(&self) -> &SceneLifecycle {
        &self.lifecycle
    }

    pub fn accessibility(&self) -> &SceneAccessibility {
        &self.accessibility
    }

    /// Verify every declared asset using a trusted host resolver and its
    /// vetted SHA-256 implementation. A missing file, out-of-root path or
    /// mismatched digest must be reported by the provider and fails closed.
    pub fn verify_asset_hashes(
        &self,
        provider: &mut impl AssetHashProvider,
    ) -> Result<(), ScenePackError> {
        for asset in &self.assets {
            let actual = provider
                .sha256_hex_for_safe_relative_path(&asset.path)
                .map_err(|reason| ScenePackError::AssetVerificationFailed {
                    path: asset.path.clone(),
                    reason,
                })?;
            if !is_lower_hex_digest(&actual) {
                return Err(ScenePackError::InvalidAssetDigest {
                    path: asset.path.clone(),
                });
            }
            if actual != asset.sha256 {
                return Err(ScenePackError::AssetDigestMismatch {
                    path: asset.path.clone(),
                    expected: asset.sha256.clone(),
                    actual,
                });
            }
        }
        Ok(())
    }

    /// Validate the renderer-owned part of the profile for a concrete output
    /// size. This catches the pixel/memory-budget constraint before allocation.
    pub fn validate_dimensions(
        &self,
        width: u32,
        height: u32,
    ) -> Result<(), ScenePackError> {
        self.settings
            .validate_for_dimensions(width, height)
            .map_err(ScenePackError::CoreSettings)
    }

    /// Construct the shared native/reference renderer from the validated pack.
    /// Host presentation and lifecycle policy remain outside the engine.
    pub fn instantiate(
        &self,
        width: u32,
        height: u32,
    ) -> Result<MycelialNetwork, ScenePackError> {
        MycelialNetwork::with_settings(width, height, self.seed, self.settings)
            .map_err(ScenePackError::CoreSettings)
    }
}

/// Parse and validate a Scene Pack v1 manifest. The parser rejects duplicate
/// JSON object keys (which serde_json::Value by itself would otherwise collapse),
/// unknown fields, null for optional-but-string fields, values outside the v1
/// schema's ranges, invalid safe regions, and contradictory branch budgets.
///
/// Asset bytes are not read. Use verify_asset_hashes with a trusted host
/// provider before an adapter activates any asset-backed pack.
pub fn parse_scene_pack_v1(bytes: &[u8]) -> Result<ValidatedScenePack, ScenePackError> {
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(ScenePackError::ManifestTooLarge);
    }

    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let strict = StrictJsonValue::deserialize(&mut deserializer)
        .map_err(|error| ScenePackError::InvalidJson(error.to_string()))?;
    deserializer
        .end()
        .map_err(|error| ScenePackError::InvalidJson(error.to_string()))?;
    let json = strict.into_value();
    let raw: RawScenePack = serde_json::from_value(json)
        .map_err(|error| ScenePackError::SchemaViolation(error.to_string()))?;

    validate_scene_id(&raw.scene_id)?;
    validate_semver("sceneVersion", &raw.scene_version)?;
    if raw.title.is_empty() || raw.title.chars().count() > 80 {
        return Err(invalid_value("title", "must contain 1..=80 characters"));
    }
    if raw.description.as_ref().is_some_and(|text| text.chars().count() > 500) {
        return Err(invalid_value("description", "must contain at most 500 characters"));
    }
    validate_license("license", &raw.license)?;

    if raw.schema_version != 1 {
        return Err(invalid_value("schemaVersion", "only version 1 is supported"));
    }
    if raw.assets.len() > 256 {
        return Err(invalid_value("assets", "must contain no more than 256 entries"));
    }
    let mut asset_ids = BTreeSet::new();
    for asset in &raw.assets {
        validate_asset(asset)?;
        if !asset_ids.insert(asset.asset_id.as_str()) {
            return Err(invalid_value("assets.assetId", "asset IDs must be unique"));
        }
    }

    let palette = ScenePalette {
        canvas: parse_color("palette.canvas", &raw.palette.canvas)?,
        substrate: parse_color("palette.substrate", &raw.palette.substrate)?,
        filament: parse_color("palette.filament", &raw.palette.filament)?,
        node: parse_color("palette.node", &raw.palette.node)?,
        lichen: parse_color("palette.lichen", &raw.palette.lichen)?,
        glow: parse_color("palette.glow", &raw.palette.glow)?,
    };

    if raw.simulation.engine != "mycelial-network-v1" {
        return Err(invalid_value(
            "simulation.engine",
            "only mycelial-network-v1 is supported by this contract",
        ));
    }
    validate_semver("simulation.engineVersion", &raw.simulation.engine_version)?;
    if raw.simulation.engine_version != SUPPORTED_ENGINE_VERSION {
        return Err(invalid_value(
            "simulation.engineVersion",
            "this renderer only implements engine version 1.0.0",
        ));
    }
    if !(1..=120).contains(&raw.simulation.fixed_step_hz) {
        return Err(invalid_value("simulation.fixedStepHz", "must be in 1..=120"));
    }
    let params = &raw.simulation.parameters;
    if !(1..=8192).contains(&params.branch_limit) {
        return Err(invalid_value(
            "simulation.parameters.branchLimit",
            "must be in 1..=8192",
        ));
    }
    if !(1..=24).contains(&params.max_depth) {
        return Err(invalid_value(
            "simulation.parameters.maxDepth",
            "must be in 1..=24",
        ));
    }
    if !params.growth_rate.is_finite() || !(0.0..=10.0).contains(&params.growth_rate) {
        return Err(invalid_value(
            "simulation.parameters.growthRate",
            "must be finite and in 0..=10",
        ));
    }
    if !params.pulse_period_seconds.is_finite()
        || params.pulse_period_seconds <= 0.0
        || params.pulse_period_seconds > 3600.0
    {
        return Err(invalid_value(
            "simulation.parameters.pulsePeriodSeconds",
            "must be finite and in (0, 3600]",
        ));
    }
    if !params.drift_amplitude.is_finite()
        || !(0.0..=1.0).contains(&params.drift_amplitude)
    {
        return Err(invalid_value(
            "simulation.parameters.driftAmplitude",
            "must be finite and in 0..=1",
        ));
    }

    validate_resource_budget(&raw.resource_budget)?;
    if params.branch_limit > raw.resource_budget.max_branches {
        return Err(invalid_value(
            "simulation.parameters.branchLimit",
            "exceeds resourceBudget.maxBranches",
        ));
    }
    validate_presentations(&raw.presentations, &raw.resource_budget)?;
    validate_capabilities(&raw.capabilities)?;
    validate_inputs(&raw.inputs)?;
    validate_accessibility(&raw.accessibility)?;

    let settings = SceneSettings {
        branch_limit: params.branch_limit,
        resource_max_branches: raw.resource_budget.max_branches,
        max_depth: params.max_depth,
        growth_rate: params.growth_rate,
        fixed_step_hz: raw.simulation.fixed_step_hz,
        pulse_period_seconds: params.pulse_period_seconds,
        drift_amplitude: params.drift_amplitude,
        max_memory_mib: raw.resource_budget.max_memory_mib,
        palette,
    };

    Ok(ValidatedScenePack {
        scene_id: raw.scene_id,
        scene_version: raw.scene_version,
        engine_version: raw.simulation.engine_version,
        title: raw.title,
        description: raw.description,
        license: raw.license,
        assets: raw.assets,
        seed: raw.simulation.seed,
        settings,
        presentations: raw.presentations,
        resource_budget: raw.resource_budget,
        capabilities: raw.capabilities,
        inputs: raw.inputs,
        lifecycle: raw.lifecycle,
        accessibility: raw.accessibility,
    })
}

fn invalid_value(field: &str, reason: &str) -> ScenePackError {
    ScenePackError::InvalidValue {
        field: field.to_owned(),
        reason: reason.to_owned(),
    }
}

fn validate_scene_id(value: &str) -> Result<(), ScenePackError> {
    if !(3..=64).contains(&value.len()) || !valid_segmented_id(value) {
        return Err(invalid_value(
            "sceneId",
            "must match the lower-case dotted/dashed scene ID pattern",
        ));
    }
    Ok(())
}

fn valid_segmented_id(value: &str) -> bool {
    if !value
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_lowercase)
    {
        return false;
    }
    let mut previous_separator = true;
    for (index, byte) in value.bytes().enumerate() {
        if byte.is_ascii_lowercase() || byte.is_ascii_digit() {
            previous_separator = false;
        } else if (byte == b'.' || byte == b'-')
            && index > 0
            && !previous_separator
        {
            previous_separator = true;
        } else {
            return false;
        }
    }
    !previous_separator
}

fn validate_semver(field: &str, value: &str) -> Result<(), ScenePackError> {
    let (core, prerelease) = match value.split_once('-') {
        Some((core, suffix)) => (core, Some(suffix)),
        None => (value, None),
    };
    let mut parts = core.split('.');
    let valid_component = |part: &str| {
        !part.is_empty()
            && part.bytes().all(|byte| byte.is_ascii_digit())
            && (part == "0" || !part.starts_with('0'))
    };
    if ![parts.next(), parts.next(), parts.next()]
        .into_iter()
        .all(|part| part.is_some_and(valid_component))
        || parts.next().is_some()
    {
        return Err(invalid_value(field, "must use semantic major.minor.patch versioning"));
    }
    if let Some(suffix) = prerelease {
        if suffix.is_empty()
            || !suffix
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
        {
            return Err(invalid_value(field, "has an invalid semantic-version suffix"));
        }
    }
    Ok(())
}

fn validate_license(field: &str, license: &SceneLicense) -> Result<(), ScenePackError> {
    if license.spdx_id.is_empty() || license.spdx_id.chars().count() > 100 {
        return Err(invalid_value(
            &format!("{field}.spdxId"),
            "must contain 1..=100 characters",
        ));
    }
    if let Some(uri) = &license.source_url {
        validate_uri(&format!("{field}.sourceUrl"), uri)?;
    }
    Ok(())
}

fn validate_asset(asset: &SceneAsset) -> Result<(), ScenePackError> {
    if !(2..=64).contains(&asset.asset_id.len())
        || !asset
            .asset_id
            .bytes()
            .enumerate()
            .all(|(index, byte)| {
                if index == 0 {
                    byte.is_ascii_lowercase()
                } else {
                    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
                }
            })
    {
        return Err(invalid_value("assets.assetId", "has an invalid asset ID"));
    }
    if asset.path.is_empty()
        || asset.path.len() > 240
        || asset.path.starts_with('/')
        || !asset
            .path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._/-".contains(&byte))
        || asset.path.split('/').any(|segment| segment == "..")
    {
        return Err(invalid_value(
            "assets.path",
            "must be a safe relative path without parent traversal",
        ));
    }
    if !is_lower_hex_digest(&asset.sha256) {
        return Err(invalid_value(
            "assets.sha256",
            "must be 64 lower-case hexadecimal characters",
        ));
    }
    validate_license("assets.license", &asset.license)?;
    if let Some(uri) = &asset.source_url {
        validate_uri("assets.sourceUrl", uri)?;
    }
    Ok(())
}

fn is_lower_hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_uri(field: &str, uri: &str) -> Result<(), ScenePackError> {
    let bytes = uri.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 2048
        || bytes
            .iter()
            .any(|byte| byte.is_ascii_whitespace() || !byte.is_ascii())
    {
        return Err(invalid_value(
            field,
            "must be an ASCII URI no longer than 2048 bytes",
        ));
    }

    let Some((scheme, remainder)) = uri.split_once(':') else {
        return Err(invalid_value(field, "must include a URI scheme"));
    };
    let valid_scheme = !scheme.is_empty()
        && scheme.as_bytes()[0].is_ascii_alphabetic()
        && scheme
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"+.-".contains(&byte));
    if !valid_scheme || remainder.is_empty() {
        return Err(invalid_value(field, "has an invalid URI scheme or empty address"));
    }

    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return Err(invalid_value(field, "contains an invalid percent escape"));
            }
            index += 3;
            continue;
        }
        if !(byte.is_ascii_alphanumeric()
            || b"-._~:/?#[]@!$&'()*+,;=".contains(&byte))
        {
            return Err(invalid_value(
                field,
                "contains a character forbidden in a URI",
            ));
        }
        index += 1;
    }
    Ok(())
}
fn parse_color(field: &str, value: &str) -> Result<Rgba, ScenePackError> {
    if value.len() != 7
        || !value.starts_with('#')
        || !value.as_bytes()[1..]
            .iter()
            .all(u8::is_ascii_hexdigit)
    {
        return Err(invalid_value(field, "must be a #RRGGBB color"));
    }
    let channel = |start: usize| {
        u8::from_str_radix(&value[start..start + 2], 16)
            .map_err(|_| invalid_value(field, "contains a non-hexadecimal color channel"))
    };
    Ok(Rgba(channel(1)?, channel(3)?, channel(5)?, 0xff))
}

fn validate_resource_budget(budget: &SceneResourceBudget) -> Result<(), ScenePackError> {
    if !(1..=60).contains(&budget.max_fps) {
        return Err(invalid_value("resourceBudget.maxFps", "must be in 1..=60"));
    }
    if !(1..=8192).contains(&budget.max_branches) {
        return Err(invalid_value("resourceBudget.maxBranches", "must be in 1..=8192"));
    }
    if !(16..=2048).contains(&budget.max_memory_mib) {
        return Err(invalid_value("resourceBudget.maxMemoryMiB", "must be in 16..=2048"));
    }
    Ok(())
}

fn validate_presentations(
    presentations: &Presentations,
    budget: &SceneResourceBudget,
) -> Result<(), ScenePackError> {
    for (name, presentation) in [
        ("boot", &presentations.boot),
        ("desktop", &presentations.desktop),
        ("idle", &presentations.idle),
        ("lockedBackground", &presentations.locked_background),
        ("staticFallback", &presentations.static_fallback),
    ] {
        if presentation.max_fps > 60 || presentation.max_fps > budget.max_fps {
            return Err(invalid_value(
                &format!("presentations.{name}.maxFps"),
                "must not exceed 60 or resourceBudget.maxFps",
            ));
        }
        if !presentation.brightness.is_finite()
            || !(0.0..=1.0).contains(&presentation.brightness)
        {
            return Err(invalid_value(
                &format!("presentations.{name}.brightness"),
                "must be finite and in 0..=1",
            ));
        }
        if presentation.safe_regions.len() > 16 {
            return Err(invalid_value(
                &format!("presentations.{name}.safeRegions"),
                "must contain no more than 16 entries",
            ));
        }
        let mut ids = BTreeSet::new();
        for region in &presentation.safe_regions {
            if !(2..=48).contains(&region.id.len())
                || !region.id.bytes().enumerate().all(|(index, byte)| {
                    if index == 0 {
                        byte.is_ascii_lowercase()
                    } else {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
                    }
                })
            {
                return Err(invalid_value(
                    &format!("presentations.{name}.safeRegions.id"),
                    "has an invalid safe-region ID",
                ));
            }
            if !ids.insert(region.id.as_str()) {
                return Err(invalid_value(
                    &format!("presentations.{name}.safeRegions"),
                    "safe-region IDs must be unique within a presentation",
                ));
            }
            if !region.x.is_finite()
                || !region.y.is_finite()
                || !region.width.is_finite()
                || !region.height.is_finite()
                || !(0.0..=1.0).contains(&region.x)
                || !(0.0..=1.0).contains(&region.y)
                || !(0.0..=1.0).contains(&region.width)
                || !(0.0..=1.0).contains(&region.height)
                || region.width == 0.0
                || region.height == 0.0
                || region.x + region.width > 1.0
                || region.y + region.height > 1.0
            {
                return Err(invalid_value(
                    &format!("presentations.{name}.safeRegions"),
                    "coordinates and extents must fit within the normalized viewport",
                ));
            }
        }
    }

    let fallback = &presentations.static_fallback;
    if fallback.motion != Motion::None
        || fallback.max_fps != 0
        || fallback.composition != Composition::GradientOnly
    {
        return Err(invalid_value(
            "presentations.staticFallback",
            "must use motion=none, maxFps=0 and composition=gradient-only",
        ));
    }
    Ok(())
}

fn validate_capabilities(capabilities: &SceneCapabilities) -> Result<(), ScenePackError> {
    if !capabilities.static_fallback_required {
        return Err(invalid_value(
            "capabilities.staticFallbackRequired",
            "must be true",
        ));
    }
    let mut seen = BTreeSet::new();
    for capability in &capabilities.required {
        if !seen.insert(*capability as u8) {
            return Err(invalid_value(
                "capabilities.required",
                "capabilities must not contain duplicates",
            ));
        }
    }
    Ok(())
}

fn validate_inputs(inputs: &[SceneInput]) -> Result<(), ScenePackError> {
    if inputs.len() > 3 {
        return Err(invalid_value("inputs", "must contain no more than 3 entries"));
    }
    let mut seen = BTreeSet::new();
    for input in inputs {
        if input.enabled_by_default {
            return Err(invalid_value(
                "inputs.enabledByDefault",
                "must be false; inputs require explicit consent",
            ));
        }
        if !input.consent_required {
            return Err(invalid_value(
                "inputs.consentRequired",
                "must be true; inputs require explicit consent",
            ));
        }
        if !seen.insert(input.id as u8) {
            return Err(invalid_value("inputs", "input IDs must be unique"));
        }
    }
    Ok(())
}

fn validate_accessibility(accessibility: &SceneAccessibility) -> Result<(), ScenePackError> {
    if accessibility.color_is_sole_signal {
        return Err(invalid_value("accessibility.colorIsSoleSignal", "must be false"));
    }
    if !accessibility.static_fallback_available {
        return Err(invalid_value(
            "accessibility.staticFallbackAvailable",
            "must be true",
        ));
    }
    Ok(())
}

fn deserialize_optional_non_null<'de, D, T>(
    deserializer: D,
) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

enum StrictJsonValue {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<StrictJsonValue>),
    Object(BTreeMap<String, StrictJsonValue>),
}

impl StrictJsonValue {
    fn into_value(self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Bool(value) => Value::Bool(value),
            Self::Number(value) => Value::Number(value),
            Self::String(value) => Value::String(value),
            Self::Array(values) => {
                Value::Array(values.into_iter().map(Self::into_value).collect())
            }
            Self::Object(fields) => {
                let object: JsonMap<String, Value> = fields
                    .into_iter()
                    .map(|(key, value)| (key, value.into_value()))
                    .collect();
                Value::Object(object)
            }
        }
    }
}

impl<'de> Deserialize<'de> for StrictJsonValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictJsonVisitor)
    }
}

struct StrictJsonVisitor;

impl<'de> Visitor<'de> for StrictJsonVisitor {
    type Value = StrictJsonValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value with no duplicate object keys")
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(StrictJsonValue::Null)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(StrictJsonValue::Null)
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(StrictJsonValue::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(StrictJsonValue::Number(Number::from(value)))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(StrictJsonValue::Number(Number::from(value)))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Number::from_f64(value)
            .map(StrictJsonValue::Number)
            .ok_or_else(|| E::custom("non-finite number is not valid JSON"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(StrictJsonValue::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(StrictJsonValue::String(value))
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element::<StrictJsonValue>()? {
            values.push(value);
        }
        Ok(StrictJsonValue::Array(values))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut fields = BTreeMap::new();
        while let Some(key) = map.next_key::<String>()? {
            if fields.contains_key(&key) {
                return Err(de::Error::custom(format!(
                    "duplicate JSON object key {key}"
                )));
            }
            let value = map.next_value::<StrictJsonValue>()?;
            fields.insert(key, value);
        }
        Ok(StrictJsonValue::Object(fields))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str =
        include_str!("../../../tests/fixtures/first-germination.scene.json");

    #[test]
    fn parses_pinned_first_germination_profile() {
        let pack = parse_scene_pack_v1(FIXTURE.as_bytes()).unwrap();
        assert_eq!(pack.scene_id(), "luminous.first-germination");
        assert_eq!(pack.scene_version(), "0.1.0");
        assert_eq!(pack.engine_version(), SUPPORTED_ENGINE_VERSION);
        assert_eq!(pack.seed(), 20261010);
        let scene = pack.instantiate(32, 24).unwrap();
        assert_eq!((scene.width(), scene.height()), (32, 24));
        assert!(scene.branch_count() <= pack.resource_budget().max_branches);
        assert_eq!(pack.settings().branch_limit, 2048);
        assert_eq!(pack.settings().resource_max_branches, 2048);
        assert_eq!(pack.settings().fixed_step_hz, 30);
        assert_eq!(pack.resource_budget().max_memory_mib, 128);
        assert_eq!(
            pack.presentations()
                .get(PresentationVariant::StaticFallback)
                .motion,
            Motion::None
        );
        assert_eq!(
            pack.presentations()
                .get(PresentationVariant::StaticFallback)
                .brightness,
            0.58
        );
        assert!(pack.assets().is_empty());
    }

    #[test]
    fn rejects_duplicate_keys_and_unknown_fields() {
        let duplicate = format!(r#"{{"schemaVersion":1,{}"#, &FIXTURE[1..]);
        assert!(matches!(
            parse_scene_pack_v1(duplicate.as_bytes()),
            Err(ScenePackError::InvalidJson(message))
                if message.contains("duplicate JSON object key")
        ));

        let unknown = FIXTURE.replace(
            r#""sceneId": "luminous.first-germination","#,
            r#""sceneId": "luminous.first-germination", "unexpected": true,"#,
        );
        assert!(matches!(
            parse_scene_pack_v1(unknown.as_bytes()),
            Err(ScenePackError::SchemaViolation(_))
        ));
    }

    #[test]
    fn rejects_resource_budget_contradictions_and_null_optionals() {
        let under_budget = FIXTURE.replace(
            r#""resourceBudget": {
    "maxFps": 30,
    "maxBranches": 2048,"#,
            r#""resourceBudget": {
    "maxFps": 30,
    "maxBranches": 1024,"#,
        );
        assert!(matches!(
            parse_scene_pack_v1(under_budget.as_bytes()),
            Err(ScenePackError::InvalidValue { field, .. })
                if field == "simulation.parameters.branchLimit"
        ));

        let original_description = concat!(
            r#""description": "A procedural mycelial world that grows during boot, "#,
            r#"settles into quiet desktop motion, and has reduced-motion and static variants.""#,
        );
        let null_description = FIXTURE.replace(original_description, r#""description": null"#);
        assert!(matches!(
            parse_scene_pack_v1(null_description.as_bytes()),
            Err(ScenePackError::SchemaViolation(_))
        ));
    }

    #[test]
    fn rejects_unsupported_engine_versions() {
        let invalid = FIXTURE.replace(
            r#""engineVersion": "1.0.0""#,
            r#""engineVersion": "1.1.0""#,
        );
        assert!(matches!(
            parse_scene_pack_v1(invalid.as_bytes()),
            Err(ScenePackError::InvalidValue { field, .. })
                if field == "simulation.engineVersion"
        ));
    }

    #[test]
    fn rejects_scene_ids_that_do_not_start_with_a_lowercase_letter() {
        let invalid = FIXTURE.replace(
            r#""sceneId": "luminous.first-germination""#,
            r#""sceneId": "1uminous.first-germination""#,
        );
        assert!(matches!(
            parse_scene_pack_v1(invalid.as_bytes()),
            Err(ScenePackError::InvalidValue { field, .. }) if field == "sceneId"
        ));
    }

    #[test]
    fn rejects_traversal_and_invalid_safe_region_extents() {
        let asset_manifest = FIXTURE.replace(
            r#""assets": []"#,
            r#""assets": [{
                "assetId": "preview-image",
                "path": "../preview.svg",
                "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "mediaType": "image/svg+xml",
                "license": { "spdxId": "CC0-1.0" }
            }]"#,
        );
        assert!(matches!(
            parse_scene_pack_v1(asset_manifest.as_bytes()),
            Err(ScenePackError::InvalidValue { field, .. }) if field == "assets.path"
        ));

        let bad_region = FIXTURE.replace(
            r#""width": 0.2,
          "height": 1""#,
            r#""width": 1.2,
          "height": 1"#,
        );
        assert!(matches!(
            parse_scene_pack_v1(bad_region.as_bytes()),
            Err(ScenePackError::InvalidValue { field, .. })
                if field.contains("safeRegions")
        ));
    }

    #[test]
    fn asset_hash_provider_is_checked_without_filesystem_access() {
        struct WrongHash;
        impl AssetHashProvider for WrongHash {
            fn sha256_hex_for_safe_relative_path(
                &mut self,
                _path: &str,
            ) -> Result<String, String> {
                Ok("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned())
            }
        }

        let asset_manifest = FIXTURE.replace(
            r#""assets": []"#,
            r#""assets": [{
                "assetId": "preview-image",
                "path": "assets/preview.svg",
                "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "mediaType": "image/svg+xml",
                "license": { "spdxId": "CC0-1.0" }
            }]"#,
        );
        let pack = parse_scene_pack_v1(asset_manifest.as_bytes()).unwrap();
        assert!(matches!(
            pack.verify_asset_hashes(&mut WrongHash),
            Err(ScenePackError::AssetDigestMismatch { .. })
        ));

        struct CorrectHash;
        impl AssetHashProvider for CorrectHash {
            fn sha256_hex_for_safe_relative_path(
                &mut self,
                _path: &str,
            ) -> Result<String, String> {
                Ok("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned())
            }
        }
        assert!(pack.verify_asset_hashes(&mut CorrectHash).is_ok());
    }

    #[test]
    fn rejects_shared_scene_pack_v1_negative_fixtures() {
        // This immutable corpus is also evaluated by the independent
        // Draft 2020-12 validator in tools/ambient_validation. The required
        // GPU capability case is schema-valid and belongs at adapter
        // qualification, not generic manifest parsing.
        let base: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        let corpus: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../contracts/tests/scene-pack-v1-negative-fixtures.json"
        ))
        .unwrap();

        fn set_pointer(
            document: &mut serde_json::Value,
            pointer: &str,
            replacement: serde_json::Value,
        ) {
            let tokens: Vec<String> = pointer
                .trim_start_matches('/')
                .split('/')
                .map(|token| token.replace("~1", "/").replace("~0", "~"))
                .collect();
            assert!(!tokens.is_empty(), "fixture JSON Pointer must not be empty");
            let mut target = document;
            for token in &tokens[..tokens.len() - 1] {
                target = match target {
                    serde_json::Value::Object(object) => object
                        .get_mut(token)
                        .unwrap_or_else(|| panic!("fixture parent path missing: {pointer}")),
                    serde_json::Value::Array(array) => array
                        .get_mut(token.parse::<usize>().expect("array index"))
                        .unwrap_or_else(|| panic!("fixture array index missing: {pointer}")),
                    _ => panic!("fixture parent is not an object or array: {pointer}"),
                };
            }
            let last = tokens.last().unwrap();
            match target {
                serde_json::Value::Object(object) => {
                    object.insert(last.clone(), replacement);
                }
                serde_json::Value::Array(array) => {
                    let index = last.parse::<usize>().expect("array index");
                    *array.get_mut(index).expect("fixture array index") = replacement;
                }
                _ => panic!("fixture target is not an object or array: {pointer}"),
            }
        }

        let mut exercised = 0usize;
        for case in corpus {
            let id = case["id"].as_str().expect("fixture ID");
            if id == "unsupported-renderer-capability" {
                continue;
            }
            let path = case["set"]["path"].as_str().expect("fixture JSON Pointer");
            let replacement = case["set"]["value"].clone();
            let mut candidate = base.clone();
            set_pointer(&mut candidate, path, replacement);
            let bytes = serde_json::to_vec(&candidate).expect("serialize mutated manifest");
            assert!(
                parse_scene_pack_v1(&bytes).is_err(),
                "Rust parser unexpectedly accepted shared negative fixture: {id}"
            );
            exercised += 1;
        }
        assert_eq!(exercised, 8, "the complete shared parser-invalid corpus must run");
    }

}
