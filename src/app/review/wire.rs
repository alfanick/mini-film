//! Convert application models to public wire DTOs without sharing private runtime fields.
//! Exhaustive enums and explicit field construction make Rust contract changes compiler-visible.

use super::{model, sampler};
use crate::review_contract as wire;
use crate::{
    app::{retouch, timestamps},
    cli,
};

impl From<retouch::BwFilter> for wire::BwFilter {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: retouch::BwFilter) -> Self {
        match value {
            retouch::BwFilter::None => Self::None,
            retouch::BwFilter::Yellow => Self::Yellow,
            retouch::BwFilter::Orange => Self::Orange,
            retouch::BwFilter::Red => Self::Red,
            retouch::BwFilter::Green => Self::Green,
        }
    }
}

impl From<wire::BwFilter> for retouch::BwFilter {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: wire::BwFilter) -> Self {
        match value {
            wire::BwFilter::None => Self::None,
            wire::BwFilter::Yellow => Self::Yellow,
            wire::BwFilter::Orange => Self::Orange,
            wire::BwFilter::Red => Self::Red,
            wire::BwFilter::Green => Self::Green,
        }
    }
}

impl From<mini_film::DiffusionMethod> for wire::DiffusionMethod {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: mini_film::DiffusionMethod) -> Self {
        match value {
            mini_film::DiffusionMethod::MultiScaleMist => Self::MultiScaleMist,
            mini_film::DiffusionMethod::EdgeAwareGlow => Self::EdgeAwareGlow,
        }
    }
}

impl From<wire::DiffusionMethod> for mini_film::DiffusionMethod {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: wire::DiffusionMethod) -> Self {
        match value {
            wire::DiffusionMethod::MultiScaleMist => Self::MultiScaleMist,
            wire::DiffusionMethod::EdgeAwareGlow => Self::EdgeAwareGlow,
        }
    }
}

impl From<cli::PanoramaMatchingMode> for wire::PanoramaMatchingMode {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: cli::PanoramaMatchingMode) -> Self {
        match value {
            cli::PanoramaMatchingMode::Automatic => Self::Automatic,
            cli::PanoramaMatchingMode::Sequential => Self::Sequential,
            cli::PanoramaMatchingMode::MultiRow => Self::MultiRow,
            cli::PanoramaMatchingMode::FlatMosaic => Self::FlatMosaic,
        }
    }
}

impl From<wire::PanoramaMatchingMode> for cli::PanoramaMatchingMode {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: wire::PanoramaMatchingMode) -> Self {
        match value {
            wire::PanoramaMatchingMode::Automatic => Self::Automatic,
            wire::PanoramaMatchingMode::Sequential => Self::Sequential,
            wire::PanoramaMatchingMode::MultiRow => Self::MultiRow,
            wire::PanoramaMatchingMode::FlatMosaic => Self::FlatMosaic,
        }
    }
}

impl From<cli::PanoramaProjection> for wire::PanoramaProjection {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: cli::PanoramaProjection) -> Self {
        match value {
            cli::PanoramaProjection::Rectilinear => Self::Rectilinear,
            cli::PanoramaProjection::Cylindrical => Self::Cylindrical,
            cli::PanoramaProjection::Equirectangular => Self::Equirectangular,
            cli::PanoramaProjection::Panini => Self::Panini,
        }
    }
}

impl From<wire::PanoramaProjection> for cli::PanoramaProjection {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: wire::PanoramaProjection) -> Self {
        match value {
            wire::PanoramaProjection::Rectilinear => Self::Rectilinear,
            wire::PanoramaProjection::Cylindrical => Self::Cylindrical,
            wire::PanoramaProjection::Equirectangular => Self::Equirectangular,
            wire::PanoramaProjection::Panini => Self::Panini,
        }
    }
}

impl From<model::ReviewLabel> for wire::ReviewLabel {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: model::ReviewLabel) -> Self {
        match value {
            model::ReviewLabel::None => Self::None,
            model::ReviewLabel::Red => Self::Red,
            model::ReviewLabel::Yellow => Self::Yellow,
            model::ReviewLabel::Green => Self::Green,
            model::ReviewLabel::Blue => Self::Blue,
            model::ReviewLabel::Purple => Self::Purple,
        }
    }
}

impl From<wire::ReviewLabel> for model::ReviewLabel {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: wire::ReviewLabel) -> Self {
        match value {
            wire::ReviewLabel::None => Self::None,
            wire::ReviewLabel::Red => Self::Red,
            wire::ReviewLabel::Yellow => Self::Yellow,
            wire::ReviewLabel::Green => Self::Green,
            wire::ReviewLabel::Blue => Self::Blue,
            wire::ReviewLabel::Purple => Self::Purple,
        }
    }
}

impl From<model::ReviewMetadataSource> for wire::ReviewMetadataSource {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: model::ReviewMetadataSource) -> Self {
        match value {
            model::ReviewMetadataSource::Default => Self::Default,
            model::ReviewMetadataSource::Camera => Self::Camera,
            model::ReviewMetadataSource::Codex => Self::Codex,
            model::ReviewMetadataSource::Manual => Self::Manual,
        }
    }
}

impl From<wire::ReviewMetadataSource> for model::ReviewMetadataSource {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: wire::ReviewMetadataSource) -> Self {
        match value {
            wire::ReviewMetadataSource::Default => Self::Default,
            wire::ReviewMetadataSource::Camera => Self::Camera,
            wire::ReviewMetadataSource::Codex => Self::Codex,
            wire::ReviewMetadataSource::Manual => Self::Manual,
        }
    }
}

impl From<model::ReviewRenderStatus> for wire::ReviewRenderStatus {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: model::ReviewRenderStatus) -> Self {
        match value {
            model::ReviewRenderStatus::Missing => Self::Missing,
            model::ReviewRenderStatus::Queued => Self::Queued,
            model::ReviewRenderStatus::Processing => Self::Processing,
            model::ReviewRenderStatus::Done => Self::Done,
            model::ReviewRenderStatus::Failed => Self::Failed,
        }
    }
}

impl From<wire::ReviewRenderStatus> for model::ReviewRenderStatus {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: wire::ReviewRenderStatus) -> Self {
        match value {
            wire::ReviewRenderStatus::Missing => Self::Missing,
            wire::ReviewRenderStatus::Queued => Self::Queued,
            wire::ReviewRenderStatus::Processing => Self::Processing,
            wire::ReviewRenderStatus::Done => Self::Done,
            wire::ReviewRenderStatus::Failed => Self::Failed,
        }
    }
}

impl From<model::ReviewCodexStatus> for wire::ReviewCodexStatus {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: model::ReviewCodexStatus) -> Self {
        match value {
            model::ReviewCodexStatus::Missing => Self::Missing,
            model::ReviewCodexStatus::Queued => Self::Queued,
            model::ReviewCodexStatus::Processing => Self::Processing,
            model::ReviewCodexStatus::Done => Self::Done,
            model::ReviewCodexStatus::Failed => Self::Failed,
            model::ReviewCodexStatus::Skipped => Self::Skipped,
        }
    }
}

impl From<wire::ReviewCodexStatus> for model::ReviewCodexStatus {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: wire::ReviewCodexStatus) -> Self {
        match value {
            wire::ReviewCodexStatus::Missing => Self::Missing,
            wire::ReviewCodexStatus::Queued => Self::Queued,
            wire::ReviewCodexStatus::Processing => Self::Processing,
            wire::ReviewCodexStatus::Done => Self::Done,
            wire::ReviewCodexStatus::Failed => Self::Failed,
            wire::ReviewCodexStatus::Skipped => Self::Skipped,
        }
    }
}

impl From<model::ReviewDiffusionSettingSource> for wire::ReviewDiffusionSettingSource {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: model::ReviewDiffusionSettingSource) -> Self {
        match value {
            model::ReviewDiffusionSettingSource::Current => Self::Current,
            model::ReviewDiffusionSettingSource::All => Self::All,
            model::ReviewDiffusionSettingSource::Daemon => Self::Daemon,
        }
    }
}

impl From<wire::ReviewDiffusionSettingSource> for model::ReviewDiffusionSettingSource {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: wire::ReviewDiffusionSettingSource) -> Self {
        match value {
            wire::ReviewDiffusionSettingSource::Current => Self::Current,
            wire::ReviewDiffusionSettingSource::All => Self::All,
            wire::ReviewDiffusionSettingSource::Daemon => Self::Daemon,
        }
    }
}

impl From<model::ReviewDiffusionScope> for wire::ReviewDiffusionScope {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: model::ReviewDiffusionScope) -> Self {
        match value {
            model::ReviewDiffusionScope::Current => Self::Current,
            model::ReviewDiffusionScope::All => Self::All,
        }
    }
}

impl From<wire::ReviewDiffusionScope> for model::ReviewDiffusionScope {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: wire::ReviewDiffusionScope) -> Self {
        match value {
            wire::ReviewDiffusionScope::Current => Self::Current,
            wire::ReviewDiffusionScope::All => Self::All,
        }
    }
}

impl From<model::ReviewDiffusionJobStatus> for wire::ReviewDiffusionJobStatus {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: model::ReviewDiffusionJobStatus) -> Self {
        match value {
            model::ReviewDiffusionJobStatus::Queued => Self::Queued,
            model::ReviewDiffusionJobStatus::Processing => Self::Processing,
            model::ReviewDiffusionJobStatus::Done => Self::Done,
            model::ReviewDiffusionJobStatus::Failed => Self::Failed,
            model::ReviewDiffusionJobStatus::Cancelled => Self::Cancelled,
        }
    }
}

impl From<wire::ReviewDiffusionJobStatus> for model::ReviewDiffusionJobStatus {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: wire::ReviewDiffusionJobStatus) -> Self {
        match value {
            wire::ReviewDiffusionJobStatus::Queued => Self::Queued,
            wire::ReviewDiffusionJobStatus::Processing => Self::Processing,
            wire::ReviewDiffusionJobStatus::Done => Self::Done,
            wire::ReviewDiffusionJobStatus::Failed => Self::Failed,
            wire::ReviewDiffusionJobStatus::Cancelled => Self::Cancelled,
        }
    }
}

impl From<model::ReviewDiffusionFocusSource> for wire::ReviewDiffusionFocusSource {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: model::ReviewDiffusionFocusSource) -> Self {
        match value {
            model::ReviewDiffusionFocusSource::CameraFocus => Self::CameraFocus,
            model::ReviewDiffusionFocusSource::CenterFallback => Self::CenterFallback,
        }
    }
}

impl From<wire::ReviewDiffusionFocusSource> for model::ReviewDiffusionFocusSource {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: wire::ReviewDiffusionFocusSource) -> Self {
        match value {
            wire::ReviewDiffusionFocusSource::CameraFocus => Self::CameraFocus,
            wire::ReviewDiffusionFocusSource::CenterFallback => Self::CenterFallback,
        }
    }
}

impl From<model::ReviewDiffusionDetailAreaKind> for wire::ReviewDiffusionDetailAreaKind {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: model::ReviewDiffusionDetailAreaKind) -> Self {
        match value {
            model::ReviewDiffusionDetailAreaKind::Focus => Self::Focus,
            model::ReviewDiffusionDetailAreaKind::HighContrastHighlight => {
                Self::HighContrastHighlight
            }
            model::ReviewDiffusionDetailAreaKind::BroadHighlight => Self::BroadHighlight,
        }
    }
}

impl From<wire::ReviewDiffusionDetailAreaKind> for model::ReviewDiffusionDetailAreaKind {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: wire::ReviewDiffusionDetailAreaKind) -> Self {
        match value {
            wire::ReviewDiffusionDetailAreaKind::Focus => Self::Focus,
            wire::ReviewDiffusionDetailAreaKind::HighContrastHighlight => {
                Self::HighContrastHighlight
            }
            wire::ReviewDiffusionDetailAreaKind::BroadHighlight => Self::BroadHighlight,
        }
    }
}

impl From<model::ReviewPanoramaStatus> for wire::ReviewPanoramaStatus {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: model::ReviewPanoramaStatus) -> Self {
        match value {
            model::ReviewPanoramaStatus::Draft => Self::Draft,
            model::ReviewPanoramaStatus::Previewing => Self::Previewing,
            model::ReviewPanoramaStatus::Ready => Self::Ready,
            model::ReviewPanoramaStatus::Rendering => Self::Rendering,
            model::ReviewPanoramaStatus::Complete => Self::Complete,
            model::ReviewPanoramaStatus::Failed => Self::Failed,
            model::ReviewPanoramaStatus::Interrupted => Self::Interrupted,
            model::ReviewPanoramaStatus::Cancelled => Self::Cancelled,
        }
    }
}

impl From<wire::ReviewPanoramaStatus> for model::ReviewPanoramaStatus {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: wire::ReviewPanoramaStatus) -> Self {
        match value {
            wire::ReviewPanoramaStatus::Draft => Self::Draft,
            wire::ReviewPanoramaStatus::Previewing => Self::Previewing,
            wire::ReviewPanoramaStatus::Ready => Self::Ready,
            wire::ReviewPanoramaStatus::Rendering => Self::Rendering,
            wire::ReviewPanoramaStatus::Complete => Self::Complete,
            wire::ReviewPanoramaStatus::Failed => Self::Failed,
            wire::ReviewPanoramaStatus::Interrupted => Self::Interrupted,
            wire::ReviewPanoramaStatus::Cancelled => Self::Cancelled,
        }
    }
}

impl From<model::ReviewPanoramaPreviewStatus> for wire::ReviewPanoramaPreviewStatus {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: model::ReviewPanoramaPreviewStatus) -> Self {
        match value {
            model::ReviewPanoramaPreviewStatus::Queued => Self::Queued,
            model::ReviewPanoramaPreviewStatus::Processing => Self::Processing,
            model::ReviewPanoramaPreviewStatus::Done => Self::Done,
            model::ReviewPanoramaPreviewStatus::Failed => Self::Failed,
            model::ReviewPanoramaPreviewStatus::Cancelled => Self::Cancelled,
        }
    }
}

impl From<wire::ReviewPanoramaPreviewStatus> for model::ReviewPanoramaPreviewStatus {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: wire::ReviewPanoramaPreviewStatus) -> Self {
        match value {
            wire::ReviewPanoramaPreviewStatus::Queued => Self::Queued,
            wire::ReviewPanoramaPreviewStatus::Processing => Self::Processing,
            wire::ReviewPanoramaPreviewStatus::Done => Self::Done,
            wire::ReviewPanoramaPreviewStatus::Failed => Self::Failed,
            wire::ReviewPanoramaPreviewStatus::Cancelled => Self::Cancelled,
        }
    }
}

impl From<model::ReviewPublishJobStatus> for wire::ReviewPublishJobStatus {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: model::ReviewPublishJobStatus) -> Self {
        match value {
            model::ReviewPublishJobStatus::Running => Self::Running,
            model::ReviewPublishJobStatus::Done => Self::Done,
            model::ReviewPublishJobStatus::Failed => Self::Failed,
        }
    }
}

impl From<wire::ReviewPublishJobStatus> for model::ReviewPublishJobStatus {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: wire::ReviewPublishJobStatus) -> Self {
        match value {
            wire::ReviewPublishJobStatus::Running => Self::Running,
            wire::ReviewPublishJobStatus::Done => Self::Done,
            wire::ReviewPublishJobStatus::Failed => Self::Failed,
        }
    }
}

impl From<sampler::ReviewSamplerJobStatus> for wire::ReviewSamplerJobStatus {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: sampler::ReviewSamplerJobStatus) -> Self {
        match value {
            sampler::ReviewSamplerJobStatus::Preparing => Self::Preparing,
            sampler::ReviewSamplerJobStatus::Rendering => Self::Rendering,
            sampler::ReviewSamplerJobStatus::Done => Self::Done,
            sampler::ReviewSamplerJobStatus::Failed => Self::Failed,
        }
    }
}

impl From<wire::ReviewSamplerJobStatus> for sampler::ReviewSamplerJobStatus {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: wire::ReviewSamplerJobStatus) -> Self {
        match value {
            wire::ReviewSamplerJobStatus::Preparing => Self::Preparing,
            wire::ReviewSamplerJobStatus::Rendering => Self::Rendering,
            wire::ReviewSamplerJobStatus::Done => Self::Done,
            wire::ReviewSamplerJobStatus::Failed => Self::Failed,
        }
    }
}

impl From<sampler::ReviewSamplerEntryStatus> for wire::ReviewSamplerEntryStatus {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: sampler::ReviewSamplerEntryStatus) -> Self {
        match value {
            sampler::ReviewSamplerEntryStatus::Queued => Self::Queued,
            sampler::ReviewSamplerEntryStatus::Rendering => Self::Rendering,
            sampler::ReviewSamplerEntryStatus::Done => Self::Done,
            sampler::ReviewSamplerEntryStatus::Failed => Self::Failed,
        }
    }
}

impl From<wire::ReviewSamplerEntryStatus> for sampler::ReviewSamplerEntryStatus {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: wire::ReviewSamplerEntryStatus) -> Self {
        match value {
            wire::ReviewSamplerEntryStatus::Queued => Self::Queued,
            wire::ReviewSamplerEntryStatus::Rendering => Self::Rendering,
            wire::ReviewSamplerEntryStatus::Done => Self::Done,
            wire::ReviewSamplerEntryStatus::Failed => Self::Failed,
        }
    }
}

impl From<sampler::ReviewSamplerScope> for wire::ReviewSamplerScope {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: sampler::ReviewSamplerScope) -> Self {
        match value {
            sampler::ReviewSamplerScope::Current => Self::Current,
            sampler::ReviewSamplerScope::All => Self::All,
        }
    }
}

impl From<wire::ReviewSamplerScope> for sampler::ReviewSamplerScope {
    /// Preserve the enum's existing wire spelling through exhaustive variant mapping.
    fn from(value: wire::ReviewSamplerScope) -> Self {
        match value {
            wire::ReviewSamplerScope::Current => Self::Current,
            wire::ReviewSamplerScope::All => Self::All,
        }
    }
}

impl From<&retouch::BasicRetouchAdjustments> for wire::BasicRetouchAdjustments {
    /// Project shared values without depending on serialization round trips.
    fn from(value: &retouch::BasicRetouchAdjustments) -> Self {
        Self {
            exposure: value.exposure,
            contrast: value.contrast,
            highlights: value.highlights,
            shadows: value.shadows,
            whites: value.whites,
            blacks: value.blacks,
            temperature: value.temperature,
            offset: value.offset,
            clarity: value.clarity,
        }
    }
}

impl From<wire::BasicRetouchAdjustments> for retouch::BasicRetouchAdjustments {
    /// Adapt decoded settings while leaving normalization to the existing application path.
    fn from(value: wire::BasicRetouchAdjustments) -> Self {
        Self {
            exposure: value.exposure,
            contrast: value.contrast,
            highlights: value.highlights,
            shadows: value.shadows,
            whites: value.whites,
            blacks: value.blacks,
            temperature: value.temperature,
            offset: value.offset,
            clarity: value.clarity,
        }
    }
}

impl From<&retouch::RetouchCrop> for wire::RetouchCrop {
    /// Project shared values without depending on serialization round trips.
    fn from(value: &retouch::RetouchCrop) -> Self {
        Self {
            x: value.x,
            y: value.y,
            width: value.width,
            height: value.height,
        }
    }
}

impl From<wire::RetouchCrop> for retouch::RetouchCrop {
    /// Adapt decoded settings while leaving normalization to the existing application path.
    fn from(value: wire::RetouchCrop) -> Self {
        Self {
            x: value.x,
            y: value.y,
            width: value.width,
            height: value.height,
        }
    }
}

impl From<&retouch::RetouchSettings> for wire::RetouchSettings {
    /// Project shared values without depending on serialization round trips.
    fn from(value: &retouch::RetouchSettings) -> Self {
        Self {
            adjustments: wire::BasicRetouchAdjustments::from(&value.adjustments),
            crop: value.crop.as_ref().map(Into::into),
            rotation_degrees: value.rotation_degrees,
        }
    }
}

impl From<wire::RetouchSettings> for retouch::RetouchSettings {
    /// Adapt decoded settings while leaving normalization to the existing application path.
    fn from(value: wire::RetouchSettings) -> Self {
        Self {
            adjustments: value.adjustments.into(),
            crop: value.crop.map(Into::into),
            rotation_degrees: value.rotation_degrees,
        }
    }
}

impl From<&mini_film::DiffusionSettings> for wire::DiffusionSettings {
    /// Project shared values without depending on serialization round trips.
    fn from(value: &mini_film::DiffusionSettings) -> Self {
        Self {
            method: value.method.into(),
            softness: value.softness,
            highlight_glow: value.highlight_glow,
            softness_radius_percent: value.softness_radius_percent,
            glow_radius_percent: value.glow_radius_percent,
            intensity_percent: value.intensity_percent,
            highlight_reach: value.highlight_reach,
        }
    }
}

impl From<wire::DiffusionSettings> for mini_film::DiffusionSettings {
    /// Adapt decoded settings while leaving normalization to the existing application path.
    fn from(value: wire::DiffusionSettings) -> Self {
        Self {
            method: value.method.into(),
            softness: value.softness,
            highlight_glow: value.highlight_glow,
            softness_radius_percent: value.softness_radius_percent,
            glow_radius_percent: value.glow_radius_percent,
            intensity_percent: value.intensity_percent,
            highlight_reach: value.highlight_reach,
        }
    }
}

impl From<&cli::CodexAnalysisFlags> for wire::CodexAnalysisFlags {
    /// Project shared values without depending on serialization round trips.
    fn from(value: &cli::CodexAnalysisFlags) -> Self {
        Self {
            tags: value.tags,
            note: value.note,
            rating: value.rating,
        }
    }
}

impl From<wire::CodexAnalysisFlags> for cli::CodexAnalysisFlags {
    /// Adapt decoded settings while leaving normalization to the existing application path.
    fn from(value: wire::CodexAnalysisFlags) -> Self {
        Self {
            tags: value.tags,
            note: value.note,
            rating: value.rating,
        }
    }
}

impl From<&model::ReviewProfileBwFilter> for wire::ReviewProfileBwFilter {
    /// Project shared values without depending on serialization round trips.
    fn from(value: &model::ReviewProfileBwFilter) -> Self {
        Self {
            profile_index: value.profile_index,
            filter: value.filter.into(),
        }
    }
}

impl From<wire::ReviewProfileBwFilter> for model::ReviewProfileBwFilter {
    /// Adapt decoded settings while leaving normalization to the existing application path.
    fn from(value: wire::ReviewProfileBwFilter) -> Self {
        Self {
            profile_index: value.profile_index,
            filter: value.filter.into(),
        }
    }
}

impl From<&model::ReviewProfileDiffusionSetting> for wire::ReviewProfileDiffusionSetting {
    /// Project shared values without depending on serialization round trips.
    fn from(value: &model::ReviewProfileDiffusionSetting) -> Self {
        Self {
            profile_index: value.profile_index,
            settings: wire::DiffusionSettings::from(&value.settings),
        }
    }
}

impl From<wire::ReviewProfileDiffusionSetting> for model::ReviewProfileDiffusionSetting {
    /// Adapt decoded settings while leaving normalization to the existing application path.
    fn from(value: wire::ReviewProfileDiffusionSetting) -> Self {
        Self {
            profile_index: value.profile_index,
            settings: value.settings.into(),
        }
    }
}

impl From<&model::ReviewImageProfileDiffusionSetting> for wire::ReviewImageProfileDiffusionSetting {
    /// Project shared values without depending on serialization round trips.
    fn from(value: &model::ReviewImageProfileDiffusionSetting) -> Self {
        Self {
            image_id: value.image_id,
            profile_index: value.profile_index,
            settings: wire::DiffusionSettings::from(&value.settings),
        }
    }
}

impl From<wire::ReviewImageProfileDiffusionSetting> for model::ReviewImageProfileDiffusionSetting {
    /// Adapt decoded settings while leaving normalization to the existing application path.
    fn from(value: wire::ReviewImageProfileDiffusionSetting) -> Self {
        Self {
            image_id: value.image_id,
            profile_index: value.profile_index,
            settings: value.settings.into(),
        }
    }
}

impl From<&model::ReviewProfile> for wire::ReviewProfile {
    /// Copy only explicitly public fields into the response contract.
    fn from(value: &model::ReviewProfile) -> Self {
        Self {
            index: value.index,
            identity: value.identity.clone(),
            selector: value.selector.clone(),
            stem: value.stem.clone(),
            sampler_added: value.sampler_added,
            enabled_by_default: value.enabled_by_default,
            configured_from_cli: value.configured_from_cli,
            retouch_base: wire::BasicRetouchAdjustments::from(&value.retouch_base),
            metadata: value.metadata.as_ref().map(Into::into),
        }
    }
}

impl From<&model::ReviewProfileMetadata> for wire::ReviewProfileMetadata {
    /// Copy only explicitly public fields into the response contract.
    fn from(value: &model::ReviewProfileMetadata) -> Self {
        Self {
            profile_name: value.profile_name.clone(),
            profile_uuid: value.profile_uuid.clone(),
            look_name: value.look_name.clone(),
            look_uuid: value.look_uuid.clone(),
            source_profile_name: value.source_profile_name.clone(),
            source_profile_uuid: value.source_profile_uuid.clone(),
            source_adjustments: wire::ReviewProfileAdjustments::from(&value.source_adjustments),
            source_sharpening: wire::ReviewProfileSharpening::from(&value.source_sharpening),
            emulation_adjustments: wire::ReviewProfileAdjustments::from(
                &value.emulation_adjustments,
            ),
            emulation_sharpening: wire::ReviewProfileSharpening::from(&value.emulation_sharpening),
            has_camera_raw_settings: value.has_camera_raw_settings,
            grain: value.grain.as_ref().map(Into::into),
            has_hald: value.has_hald,
            has_pp3: value.has_pp3,
            pp3_name: value.pp3_name.clone(),
            pp3_adjustments: value.pp3_adjustments.iter().map(Into::into).collect(),
        }
    }
}

impl From<&model::ReviewProfilePp3Section> for wire::ReviewProfilePp3Section {
    /// Copy only explicitly public fields into the response contract.
    fn from(value: &model::ReviewProfilePp3Section) -> Self {
        Self {
            source: value.source.clone(),
            section: value.section.clone(),
            entries: value.entries.iter().map(Into::into).collect(),
        }
    }
}

impl From<&model::ReviewProfilePp3Entry> for wire::ReviewProfilePp3Entry {
    /// Copy only explicitly public fields into the response contract.
    fn from(value: &model::ReviewProfilePp3Entry) -> Self {
        Self {
            key: value.key.clone(),
            value: value.value.clone(),
        }
    }
}

impl From<&model::ReviewProfileGrain> for wire::ReviewProfileGrain {
    /// Copy only explicitly public fields into the response contract.
    fn from(value: &model::ReviewProfileGrain) -> Self {
        Self {
            amount: value.amount,
            size: value.size,
            frequency: value.frequency,
        }
    }
}

impl From<&model::ReviewProfileAdjustments> for wire::ReviewProfileAdjustments {
    /// Copy only explicitly public fields into the response contract.
    fn from(value: &model::ReviewProfileAdjustments) -> Self {
        Self {
            exposure: value.exposure,
            contrast: value.contrast,
            highlights: value.highlights,
            shadows: value.shadows,
            whites: value.whites,
            blacks: value.blacks,
            saturation: value.saturation,
            vibrance: value.vibrance,
            clarity: value.clarity,
            parametric: wire::ReviewProfileParametricTone::from(&value.parametric),
            hsl: wire::ReviewProfileHslAdjustments::from(&value.hsl),
            calibration: wire::ReviewProfileCalibration::from(&value.calibration),
            tone_curve: wire::ReviewProfileToneCurves::from(&value.tone_curve),
        }
    }
}

impl From<&model::ReviewProfileSharpening> for wire::ReviewProfileSharpening {
    /// Copy only explicitly public fields into the response contract.
    fn from(value: &model::ReviewProfileSharpening) -> Self {
        Self {
            present: value.present,
            amount: value.amount,
            radius: value.radius,
            detail: value.detail,
            masking: value.masking,
        }
    }
}

impl From<&model::ReviewProfileParametricTone> for wire::ReviewProfileParametricTone {
    /// Copy only explicitly public fields into the response contract.
    fn from(value: &model::ReviewProfileParametricTone) -> Self {
        Self {
            shadows: value.shadows,
            darks: value.darks,
            lights: value.lights,
            highlights: value.highlights,
            shadow_split: value.shadow_split,
            midtone_split: value.midtone_split,
            highlight_split: value.highlight_split,
        }
    }
}

impl From<&model::ReviewProfileHslAdjustments> for wire::ReviewProfileHslAdjustments {
    /// Copy only explicitly public fields into the response contract.
    fn from(value: &model::ReviewProfileHslAdjustments) -> Self {
        Self {
            hue: value.hue.clone(),
            saturation: value.saturation.clone(),
            luminance: value.luminance.clone(),
        }
    }
}

impl From<&model::ReviewProfileCalibration> for wire::ReviewProfileCalibration {
    /// Copy only explicitly public fields into the response contract.
    fn from(value: &model::ReviewProfileCalibration) -> Self {
        Self {
            red_hue: value.red_hue,
            red_saturation: value.red_saturation,
            green_hue: value.green_hue,
            green_saturation: value.green_saturation,
            blue_hue: value.blue_hue,
            blue_saturation: value.blue_saturation,
        }
    }
}

impl From<&model::ReviewProfileToneCurves> for wire::ReviewProfileToneCurves {
    /// Copy only explicitly public fields into the response contract.
    fn from(value: &model::ReviewProfileToneCurves) -> Self {
        Self {
            composite: value.composite.clone(),
            red: value.red.clone(),
            green: value.green.clone(),
            blue: value.blue.clone(),
        }
    }
}

impl From<&model::ReviewBurst> for wire::ReviewBurst {
    /// Copy only explicitly public fields into the response contract.
    fn from(value: &model::ReviewBurst) -> Self {
        Self {
            id: value.id.clone(),
            image_ids: value.image_ids.clone(),
            expanded: value.expanded,
        }
    }
}

impl From<&model::ReviewDiffusionDetailArea> for wire::ReviewDiffusionDetailArea {
    /// Copy only explicitly public fields into the response contract.
    fn from(value: &model::ReviewDiffusionDetailArea) -> Self {
        Self {
            kind: value.kind.into(),
            x: value.x,
            y: value.y,
            width: value.width,
            height: value.height,
        }
    }
}

impl From<&model::ReviewDiffusionJob> for wire::ReviewDiffusionJob {
    /// Copy only explicitly public fields into the response contract.
    fn from(value: &model::ReviewDiffusionJob) -> Self {
        Self {
            id: value.id,
            status: value.status.into(),
            image_id: value.image_id,
            profile_index: value.profile_index,
            settings: wire::DiffusionSettings::from(&value.settings),
            before_url: value.before_url.clone(),
            after_url: value.after_url.clone(),
            preview_width: value.preview_width,
            preview_height: value.preview_height,
            focus_source: value.focus_source.map(Into::into),
            detail_areas: value.detail_areas.iter().map(Into::into).collect(),
            error: value.error.clone(),
            source_url: None,
            source_width: None,
            source_height: None,
            preview_url: None,
            result_url: None,
            updated_at: None,
            before_updated_at: None,
            after_updated_at: None,
        }
    }
}

impl From<&model::ReviewPublishDefaults> for wire::ReviewPublishDefaults {
    /// Copy only explicitly public fields into the response contract.
    fn from(value: &model::ReviewPublishDefaults) -> Self {
        Self {
            album: value.album.clone(),
            output_format: value.output_format.clone(),
            jpg_quality: value.jpg_quality,
            resize: value.resize.clone(),
            long_edge: value.long_edge,
            max_width: value.max_width,
            max_height: value.max_height,
            jpeg_subsampling: value.jpeg_subsampling.clone(),
            strip_metadata: value.strip_metadata,
            progressive_jpeg: value.progressive_jpeg,
            gallery: value.gallery.clone(),
            gallery_thumbnail_long_edge: value.gallery_thumbnail_long_edge,
            gallery_columns: value.gallery_columns,
            grain_engine: value.grain_engine.clone(),
            normalize_grain_mpix: value.normalize_grain_mpix,
        }
    }
}

impl From<&model::ReviewPublishJob> for wire::ReviewPublishJob {
    /// Copy only explicitly public fields into the response contract.
    fn from(value: &model::ReviewPublishJob) -> Self {
        Self {
            id: value.id,
            album: value.album.clone(),
            status: value.status.into(),
            started_at: value.started_at.clone(),
            finished_at: value.finished_at.clone(),
            processed: value.processed,
            total: value.total,
            step: value.step.clone(),
            current: value.current.clone(),
            linked: value.linked,
            skipped: value.skipped,
            galleries: value.galleries,
            gallery_urls: value.gallery_urls.clone(),
            error: value.error.clone(),
        }
    }
}

impl From<&sampler::ReviewSamplerJobSnapshot> for wire::ReviewSamplerJobSnapshot {
    /// Copy only explicitly public fields into the response contract.
    fn from(value: &sampler::ReviewSamplerJobSnapshot) -> Self {
        Self {
            id: value.id,
            image_id: value.image_id,
            file_name: value.file_name.clone(),
            status: value.status.into(),
            source_url: value.source_url.clone(),
            source_width: value.source_width,
            source_height: value.source_height,
            completed: value.completed,
            total: value.total,
            failed: value.failed,
            workers: value.workers,
            error: value.error.clone(),
            entries: value.entries.iter().map(Into::into).collect(),
        }
    }
}

impl From<&sampler::ReviewSamplerEntrySnapshot> for wire::ReviewSamplerEntrySnapshot {
    /// Copy only explicitly public fields into the response contract.
    fn from(value: &sampler::ReviewSamplerEntrySnapshot) -> Self {
        Self {
            key: value.key.clone(),
            name: value.name.clone(),
            filename: value.filename.clone(),
            parts: value.parts.clone(),
            status: value.status.into(),
            thumbnail_url: value.thumbnail_url.clone(),
            duration_ms: value.duration_ms,
            error: value.error.clone(),
            current_enabled: value.current_enabled,
            all_enabled: value.all_enabled,
            configured_from_cli: value.configured_from_cli,
            selected: value.selected,
        }
    }
}

impl From<&timestamps::GalleryFocusRegion> for wire::GalleryFocusRegion {
    /// Copy only explicitly public fields into the response contract.
    fn from(value: &timestamps::GalleryFocusRegion) -> Self {
        Self {
            x: value.x,
            y: value.y,
            width: value.width,
            height: value.height,
            primary: value.primary,
        }
    }
}

impl From<&timestamps::GalleryExifData> for wire::GalleryExifData {
    /// Copy only explicitly public fields into the response contract.
    fn from(value: &timestamps::GalleryExifData) -> Self {
        Self {
            capture_timestamp: value.capture_timestamp,
            capture_subsecond: value.capture_subsecond.clone(),
            rating: value.rating,
            file_size_bytes: value.file_size_bytes,
            image_width: value.image_width,
            image_height: value.image_height,
            focus_frame_width: value.focus_frame_width,
            focus_frame_height: value.focus_frame_height,
            focus_regions: value.focus_regions.iter().map(Into::into).collect(),
            focal_length: value.focal_length.clone(),
            aperture: value.aperture.clone(),
            shutter_speed: value.shutter_speed.clone(),
            iso: value.iso.clone(),
            auto_iso: value.auto_iso,
            iso_auto_hi_limit: value.iso_auto_hi_limit.clone(),
            white_balance_mode: value.white_balance_mode.clone(),
            white_balance_temperature: value.white_balance_temperature,
            white_balance_offset: value.white_balance_offset,
            camera_model: value.camera_model.clone(),
            shutter_count: value.shutter_count,
            shutter_mode: value.shutter_mode.clone(),
            silent_photography: value.silent_photography,
            release_mode: value.release_mode.clone(),
            lens_model: value.lens_model.clone(),
            shooting_mode: value.shooting_mode.clone(),
            exposure_compensation: value.exposure_compensation.clone(),
            flash: value.flash.clone(),
            active_d_lighting: value.active_d_lighting.clone(),
            tags: value.tags.clone(),
            note: value.note.clone(),
        }
    }
}

impl From<wire::ReviewUpdateRequest> for model::ReviewUpdateRequest {
    /// Keep existing application update behavior behind the shared deserialization boundary.
    fn from(value: wire::ReviewUpdateRequest) -> Self {
        Self {
            image_id: value.image_id,
            rating: value.rating,
            label: value.label.into(),
            labels: value.labels.into_iter().map(Into::into).collect(),
            tags: value.tags,
            notes: value.notes,
            retouch: value.retouch.map(Into::into),
            selected_profile_index: value.selected_profile_index,
            publish_profile_indexes: value.publish_profile_indexes,
            enabled_profile_indexes: value.enabled_profile_indexes,
            profile_bw_filters: value
                .profile_bw_filters
                .map(|items| items.into_iter().map(Into::into).collect()),
            advance_after_update: value.advance_after_update,
        }
    }
}

impl From<wire::ReviewUiUpdateRequest> for model::ReviewUiUpdateRequest {
    /// Keep existing application update behavior behind the shared deserialization boundary.
    fn from(value: wire::ReviewUiUpdateRequest) -> Self {
        Self {
            current_image_id: value.current_image_id,
            min_rating: value.min_rating,
            labels: value.labels.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<wire::ReviewBurstExpansionRequest> for model::ReviewBurstExpansionRequest {
    /// Keep existing application update behavior behind the shared deserialization boundary.
    fn from(value: wire::ReviewBurstExpansionRequest) -> Self {
        Self {
            expanded: value.expanded,
        }
    }
}

impl From<wire::PublishRequest> for model::PublishRequest {
    /// Keep existing application update behavior behind the shared deserialization boundary.
    fn from(value: wire::PublishRequest) -> Self {
        Self {
            min_rating: value.min_rating,
            album: value.album,
            labels: value.labels.into_iter().map(Into::into).collect(),
            tags: value.tags,
            main_profile_only: value.main_profile_only,
            output_format: value.output_format,
            gallery: value.gallery,
            jpg_quality: value.jpg_quality,
            size_mode: value.size_mode,
            resize: value.resize,
            long_edge: value.long_edge,
            max_width: value.max_width,
            max_height: value.max_height,
            jpeg_subsampling: value.jpeg_subsampling,
            strip_metadata: value.strip_metadata,
            progressive_jpeg: value.progressive_jpeg,
            gallery_thumbnail_long_edge: value.gallery_thumbnail_long_edge,
            gallery_columns: value.gallery_columns,
            grain_engine: value.grain_engine,
            normalize_grain: value.normalize_grain,
            normalize_grain_mpix: value.normalize_grain_mpix,
        }
    }
}

impl From<wire::ReviewDiffusionJobRequest> for model::ReviewDiffusionJobRequest {
    /// Keep existing application update behavior behind the shared deserialization boundary.
    fn from(value: wire::ReviewDiffusionJobRequest) -> Self {
        Self {
            image_id: value.image_id,
            profile_index: value.profile_index,
            settings: value.settings.into(),
        }
    }
}

impl From<wire::ReviewDiffusionSettingsRequest> for model::ReviewDiffusionSettingsRequest {
    /// Keep existing application update behavior behind the shared deserialization boundary.
    fn from(value: wire::ReviewDiffusionSettingsRequest) -> Self {
        Self {
            scope: value.scope.into(),
            image_id: value.image_id,
            profile_index: value.profile_index,
            settings: value.settings.into(),
        }
    }
}

impl From<wire::ReviewDiffusionSettingsResetRequest>
    for model::ReviewDiffusionSettingsResetRequest
{
    /// Keep existing application update behavior behind the shared deserialization boundary.
    fn from(value: wire::ReviewDiffusionSettingsResetRequest) -> Self {
        Self {
            scope: value.scope.into(),
            image_id: value.image_id,
            profile_index: value.profile_index,
        }
    }
}

impl From<wire::ReviewPanoramaCreateRequest> for model::ReviewPanoramaCreateRequest {
    /// Keep existing application update behavior behind the shared deserialization boundary.
    fn from(value: wire::ReviewPanoramaCreateRequest) -> Self {
        Self {
            image_ids: value.image_ids,
            name: value.name,
            matching_mode: value.matching_mode.into(),
        }
    }
}

impl From<wire::ReviewPanoramaUpdateRequest> for model::ReviewPanoramaUpdateRequest {
    /// Keep existing application update behavior behind the shared deserialization boundary.
    fn from(value: wire::ReviewPanoramaUpdateRequest) -> Self {
        Self {
            image_ids: value.image_ids,
            name: value.name,
            matching_mode: value.matching_mode.map(Into::into),
            selected_projection: value.selected_projection.map(Into::into),
        }
    }
}

impl From<wire::ReviewPanoramaPreviewRequest> for model::ReviewPanoramaPreviewRequest {
    /// Keep existing application update behavior behind the shared deserialization boundary.
    fn from(value: wire::ReviewPanoramaPreviewRequest) -> Self {
        Self {
            image_ids: value.image_ids,
            matching_mode: value.matching_mode.map(Into::into),
        }
    }
}

impl From<wire::ReviewPanoramaRenderRequest> for model::ReviewPanoramaRenderRequest {
    /// Keep existing application update behavior behind the shared deserialization boundary.
    fn from(value: wire::ReviewPanoramaRenderRequest) -> Self {
        Self {
            name: value.name,
            projection: value.projection.map(Into::into),
        }
    }
}

impl From<wire::ReviewSamplerPriorityRequest> for sampler::ReviewSamplerPriorityRequest {
    /// Keep existing application update behavior behind the shared deserialization boundary.
    fn from(value: wire::ReviewSamplerPriorityRequest) -> Self {
        Self {
            visible_keys: value.visible_keys,
            expanded_keys: value.expanded_keys,
        }
    }
}

impl From<wire::ReviewSamplerSelectionRequest> for sampler::ReviewSamplerSelectionRequest {
    /// Keep existing application update behavior behind the shared deserialization boundary.
    fn from(value: wire::ReviewSamplerSelectionRequest) -> Self {
        Self {
            scope: value.scope.into(),
            enabled: value.enabled,
        }
    }
}
