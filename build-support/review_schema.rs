//! Deterministic Draft 7 schema and route export used by Cargo and the developer tool.
//! Schema generation only sees the same pure DTO source that the HTTP handlers use.

use crate::review_contract::*;

#[path = "review_fixtures.rs"]
pub mod fixtures;

#[cfg(test)]
#[path = "review_schema_tests.rs"]
mod tests;
use schemars::{JsonSchema, generate::SchemaSettings};
use serde::Serialize;
use std::{error::Error, fs, path::Path};

/// Request schema catalog; property names are stable generator entry points, not wire envelopes.
#[derive(JsonSchema)]
pub struct RequestContracts {
    pub review: ReviewUpdateRequest,
    pub ui: ReviewUiUpdateRequest,
    pub burst: ReviewBurstExpansionRequest,
    pub publish: PublishRequest,
    pub sampler_create: ReviewSamplerStartRequest,
    pub sampler_priority: ReviewSamplerPriorityRequest,
    pub sampler_select: ReviewSamplerSelectionRequest,
    pub diffusion_create: ReviewDiffusionJobRequest,
    pub diffusion_apply: ReviewDiffusionSettingsRequest,
    pub diffusion_reset: ReviewDiffusionSettingsResetRequest,
    pub panorama_create: ReviewPanoramaCreateRequest,
    pub panorama_update: ReviewPanoramaUpdateRequest,
    pub panorama_previews: ReviewPanoramaPreviewRequest,
    pub panorama_render: ReviewPanoramaRenderRequest,
}

/// Response schema catalog keeps complete output properties distinct from defaulted input properties.
#[derive(JsonSchema)]
pub struct ResponseContracts {
    pub state: ReviewStateSnapshot,
    pub patch: ReviewStatePatch,
    pub message: ReviewStateMessage,
    pub publish_created: ReviewPublishCreated,
    pub panorama_created: ReviewPanoramaCreated,
    pub sampler_job: ReviewSamplerJobSnapshot,
    pub diffusion_job: ReviewDiffusionJob,
    pub error: ReviewError,
    pub keepalive: ReviewKeepalive,
}

/// An operation selects its request and response catalog entries without caller-supplied type assertions.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Operation {
    pub name: &'static str,
    pub method: &'static str,
    pub path: &'static str,
    pub request: Option<&'static str>,
    pub response: &'static str,
    pub allow_empty_request: bool,
    pub transport: &'static str,
    pub parameters: &'static [OperationParameter],
}

/// Route identities keep numeric server identifiers distinct from opaque, URL-encoded keys.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ParameterKind {
    Number,
    String,
}

/// Export the server parser's parameter type instead of guessing from a placeholder in JavaScript.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct OperationParameter {
    pub name: &'static str,
    pub kind: ParameterKind,
}

impl Operation {
    /// Attach parameter metadata beside its route so new placeholders require an explicit type.
    const fn parameters(mut self, parameters: &'static [OperationParameter]) -> Self {
        self.parameters = parameters;
        self
    }
}

const JOB_ID: OperationParameter = OperationParameter {
    name: "job_id",
    kind: ParameterKind::Number,
};
const PROJECT_ID: OperationParameter = OperationParameter {
    name: "project_id",
    kind: ParameterKind::Number,
};

/// Describe a JSON HTTP operation, preserving the existing relative routing convention.
const fn http(
    name: &'static str,
    method: &'static str,
    path: &'static str,
    request: Option<&'static str>,
    response: &'static str,
    allow_empty_request: bool,
) -> Operation {
    Operation {
        name,
        method,
        path,
        request,
        response,
        allow_empty_request,
        transport: "http",
        parameters: &[],
    }
}

/// Every JSON method/path pair used by review, followed by the separate SSE transport.
pub const OPERATIONS: &[Operation] = &[
    http("state", "GET", "api/state", None, "state", false),
    http(
        "review",
        "POST",
        "api/review",
        Some("review"),
        "patch",
        false,
    ),
    http("ui", "POST", "api/ui", Some("ui"), "patch", false),
    http(
        "burst",
        "PATCH",
        "api/bursts/{burst_id}",
        Some("burst"),
        "patch",
        false,
    )
    .parameters(&[OperationParameter {
        name: "burst_id",
        kind: ParameterKind::String,
    }]),
    http(
        "publish",
        "POST",
        "api/publish",
        Some("publish"),
        "publish_created",
        true,
    ),
    http(
        "sampler_create",
        "POST",
        "api/sampler/jobs",
        Some("sampler_create"),
        "sampler_job",
        false,
    ),
    http(
        "sampler_get",
        "GET",
        "api/sampler/jobs/{job_id}",
        None,
        "sampler_job",
        false,
    )
    .parameters(&[JOB_ID]),
    http(
        "sampler_priority",
        "POST",
        "api/sampler/jobs/{job_id}/priority",
        Some("sampler_priority"),
        "sampler_job",
        false,
    )
    .parameters(&[JOB_ID]),
    http(
        "sampler_select",
        "POST",
        "api/sampler/jobs/{job_id}/profiles/{entry_key}",
        Some("sampler_select"),
        "sampler_job",
        false,
    )
    .parameters(&[
        JOB_ID,
        OperationParameter {
            name: "entry_key",
            kind: ParameterKind::String,
        },
    ]),
    http(
        "diffusion_create",
        "POST",
        "api/diffusion/jobs",
        Some("diffusion_create"),
        "diffusion_job",
        false,
    ),
    http(
        "diffusion_get",
        "GET",
        "api/diffusion/jobs/{job_id}",
        None,
        "diffusion_job",
        false,
    )
    .parameters(&[JOB_ID]),
    http(
        "diffusion_apply",
        "POST",
        "api/diffusion/settings",
        Some("diffusion_apply"),
        "patch",
        false,
    ),
    http(
        "diffusion_reset",
        "DELETE",
        "api/diffusion/settings",
        Some("diffusion_reset"),
        "patch",
        false,
    ),
    http(
        "panorama_create",
        "POST",
        "api/panoramas",
        Some("panorama_create"),
        "panorama_created",
        false,
    ),
    http(
        "panorama_update",
        "PATCH",
        "api/panoramas/{project_id}",
        Some("panorama_update"),
        "patch",
        false,
    )
    .parameters(&[PROJECT_ID]),
    http(
        "panorama_previews",
        "POST",
        "api/panoramas/{project_id}/previews",
        Some("panorama_previews"),
        "patch",
        true,
    )
    .parameters(&[PROJECT_ID]),
    http(
        "panorama_render",
        "POST",
        "api/panoramas/{project_id}/render",
        Some("panorama_render"),
        "patch",
        true,
    )
    .parameters(&[PROJECT_ID]),
    Operation {
        name: "events",
        method: "GET",
        path: "api/events",
        request: None,
        response: "message",
        allow_empty_request: false,
        transport: "sse",
        parameters: &[],
    },
];

/// Write stable JSON bytes and preserve timestamps when schema contents are unchanged.
fn write_json(path: &Path, value: &impl Serialize) -> Result<(), Box<dyn Error>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    match fs::read(path) {
        Ok(existing) if existing == bytes => return Ok(()),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    fs::write(path, bytes)?;
    Ok(())
}

/// Export schema catalogs with their appropriate serde direction and stable operation metadata.
pub fn export(output: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(output)?;
    let requests = SchemaSettings::draft07()
        .for_deserialize()
        .into_generator()
        .into_root_schema_for::<RequestContracts>();
    let responses = SchemaSettings::draft07()
        .for_serialize()
        .into_generator()
        .into_root_schema_for::<ResponseContracts>();
    write_json(&output.join("requests.schema.json"), &requests)?;
    write_json(&output.join("responses.schema.json"), &responses)?;
    write_json(&output.join("operations.json"), &OPERATIONS)?;
    write_json(&output.join("fixtures.json"), &fixtures::responses())?;
    Ok(())
}
