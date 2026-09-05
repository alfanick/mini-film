//! Incremental review messages preserve absent fields separately from explicit null.
//! Typed snapshot comparison makes additions to the server contract visible to the compiler.

use super::*;
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Serialize, Serializer};
use std::{borrow::Cow, collections::HashMap};

/// An omitted patch member or an explicitly supplied replacement value.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum PatchField<T> {
    #[default]
    Missing,
    Set(T),
}

impl<T> PatchField<T> {
    /// Tell serde to omit unchanged members, without treating explicit null as absent.
    pub fn is_missing(&self) -> bool {
        matches!(self, Self::Missing)
    }
}

impl<T: Serialize> Serialize for PatchField<T> {
    /// Serialize a present member exactly as its value, never with an enum wrapper.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Set(value) => value.serialize(serializer),
            Self::Missing => Err(serde::ser::Error::custom(
                "missing patch members must be omitted",
            )),
        }
    }
}

impl<T: JsonSchema> JsonSchema for PatchField<T> {
    /// Delegate the value schema; serde's skip rule supplies property optionality.
    fn schema_name() -> Cow<'static, str> {
        format!("PatchField_{}", T::schema_name()).into()
    }
    /// Keep generic identities distinct when nested definitions share display names.
    fn schema_id() -> Cow<'static, str> {
        format!("PatchField<{}>", T::schema_id()).into()
    }
    /// Missing is not a JSON value and must not widen the member's nullable type.
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        generator.subschema_for::<T>()
    }
}

/// The existing incremental-state tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewPatchType {
    Patch,
}

/// A server patch has complete changed images and optional top-level replacements.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct ReviewStatePatch {
    #[serde(rename = "type")]
    pub kind: ReviewPatchType,
    pub version: String,
    #[serde(skip_serializing_if = "PatchField::is_missing")]
    pub invocation: PatchField<Option<String>>,
    #[serde(skip_serializing_if = "PatchField::is_missing")]
    pub profiles: PatchField<Vec<ReviewProfile>>,
    #[serde(skip_serializing_if = "PatchField::is_missing")]
    pub client_count: PatchField<usize>,
    #[serde(skip_serializing_if = "PatchField::is_missing")]
    pub codex: PatchField<ReviewCodexSummary>,
    #[serde(skip_serializing_if = "PatchField::is_missing")]
    pub publish_defaults: PatchField<ReviewPublishDefaults>,
    #[serde(skip_serializing_if = "PatchField::is_missing")]
    pub diffusion_default: PatchField<DiffusionSettings>,
    #[serde(skip_serializing_if = "PatchField::is_missing")]
    pub profile_diffusion_settings: PatchField<Vec<ReviewProfileDiffusionSetting>>,
    #[serde(skip_serializing_if = "PatchField::is_missing")]
    pub publish_jobs: PatchField<Vec<ReviewPublishJob>>,
    #[serde(skip_serializing_if = "PatchField::is_missing")]
    pub capabilities: PatchField<ReviewCapabilities>,
    #[serde(skip_serializing_if = "PatchField::is_missing")]
    pub panorama: PatchField<ReviewPanoramaState>,
    #[serde(skip_serializing_if = "PatchField::is_missing")]
    pub ui: PatchField<ReviewUiState>,
    #[serde(skip_serializing_if = "PatchField::is_missing")]
    pub bursts: PatchField<Vec<ReviewBurst>>,
    #[serde(skip_serializing_if = "PatchField::is_missing")]
    pub images: PatchField<Vec<ReviewImage>>,
    #[serde(skip_serializing_if = "PatchField::is_missing")]
    pub publish_root: PatchField<String>,
    #[serde(skip_serializing_if = "PatchField::is_missing")]
    pub image_ids: PatchField<Vec<u64>>,
    #[serde(skip_serializing_if = "PatchField::is_missing")]
    pub removed_image_ids: PatchField<Vec<u64>>,
}

/// Ordinary SSE data preserves the historical untagged snapshot/tagged patch union.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ReviewStateMessage {
    Snapshot(ReviewStateSnapshot),
    Patch(ReviewStatePatch),
}

/// Clone a replacement only when the corresponding snapshot value changed.
fn changed<T: Clone + PartialEq>(previous: &T, current: &T) -> PatchField<T> {
    if previous == current {
        PatchField::Missing
    } else {
        PatchField::Set(current.clone())
    }
}

impl ReviewStatePatch {
    /// Compare complete snapshots using the original changed-image/order/removal protocol.
    pub fn between(previous: &ReviewStateSnapshot, current: &ReviewStateSnapshot) -> Self {
        // Do not use `..`: a new snapshot member must explicitly enter the patch protocol.
        let ReviewStateSnapshot {
            version,
            invocation,
            profiles,
            client_count,
            codex,
            publish_defaults,
            diffusion_default,
            profile_diffusion_settings,
            publish_jobs,
            capabilities,
            panorama,
            ui,
            bursts,
            images,
            publish_root,
        } = current;
        let previous_images: HashMap<u64, &ReviewImage> = previous
            .images
            .iter()
            .map(|image| (image.id, image))
            .collect();
        let current_images: HashMap<u64, &ReviewImage> =
            images.iter().map(|image| (image.id, image)).collect();
        let changed_images: Vec<ReviewImage> = images
            .iter()
            .filter(|image| {
                previous_images
                    .get(&image.id)
                    .is_none_or(|old| **old != **image)
            })
            .cloned()
            .collect();
        let removed: Vec<u64> = previous_images
            .keys()
            .filter(|id| !current_images.contains_key(id))
            .copied()
            .collect();
        let image_ids = if changed_images.is_empty() && removed.is_empty() {
            PatchField::Missing
        } else {
            PatchField::Set(images.iter().map(|image| image.id).collect())
        };
        Self {
            kind: ReviewPatchType::Patch,
            version: version.clone(),
            invocation: changed(&previous.invocation, invocation),
            profiles: changed(&previous.profiles, profiles),
            client_count: changed(&previous.client_count, client_count),
            codex: changed(&previous.codex, codex),
            publish_defaults: changed(&previous.publish_defaults, publish_defaults),
            diffusion_default: changed(&previous.diffusion_default, diffusion_default),
            profile_diffusion_settings: changed(
                &previous.profile_diffusion_settings,
                profile_diffusion_settings,
            ),
            publish_jobs: changed(&previous.publish_jobs, publish_jobs),
            capabilities: changed(&previous.capabilities, capabilities),
            panorama: changed(&previous.panorama, panorama),
            ui: changed(&previous.ui, ui),
            bursts: changed(&previous.bursts, bursts),
            images: if changed_images.is_empty() {
                PatchField::Missing
            } else {
                PatchField::Set(changed_images)
            },
            publish_root: changed(&previous.publish_root, publish_root),
            image_ids,
            removed_image_ids: if removed.is_empty() {
                PatchField::Missing
            } else {
                PatchField::Set(removed)
            },
        }
    }
}
