//! `/api/prompt-templates` — the registered prompt templates.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use delta_wire::rest::{
    WireCreatePromptTemplateRequest, WirePromptTemplate, WirePromptTemplatesResponse,
    WireUpdatePromptTemplateRequest,
};

use crate::state::AppState;

use super::ApiError;

/// `GET /api/prompt-templates` — the registered prompt templates.
///
/// Returns the `(label, text)` records the user registered as reusable composer
/// instructions, oldest first, for the settings screen to manage and the
/// composer to insert from. Unlike launch options the list is global: the text
/// is provider-independent prose, so nothing is filtered per provider.
pub(crate) async fn list_prompt_templates(
    State(state): State<AppState>,
) -> Result<Json<WirePromptTemplatesResponse>, ApiError> {
    let templates = state.interactor().list_prompt_templates().await?;
    Ok(Json(WirePromptTemplatesResponse {
        prompt_templates: templates
            .into_iter()
            .map(WirePromptTemplate::from)
            .collect(),
    }))
}

/// `POST /api/prompt-templates` — register a new prompt template.
///
/// Both `label` and `text` are required and must be non-blank; a blank one is a
/// `400` from the use case, which names the offending field. Both are stored
/// verbatim — the emptiness check trims, the storage does not — so a template
/// that ends with a newline keeps it. Returns the created record so the client
/// can render it without a refetch.
pub(crate) async fn create_prompt_template(
    State(state): State<AppState>,
    Json(req): Json<WireCreatePromptTemplateRequest>,
) -> Result<(StatusCode, Json<WirePromptTemplate>), ApiError> {
    let template = state
        .interactor()
        .create_prompt_template(&req.label, &req.text)
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(WirePromptTemplate::from(template)),
    ))
}

/// `PATCH /api/prompt-templates/{id}` — replace a template's content in place.
///
/// Updating in place preserves the template's id and `created_at` (a
/// delete+recreate would churn both and move the row to the end of the list) and
/// re-stamps `updated_at`. Both fields are required, and held to the same
/// non-blank rule as the create (`400`). Returns the updated record, or `404`
/// when no template has that id.
pub(crate) async fn update_prompt_template(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<WireUpdatePromptTemplateRequest>,
) -> Result<Json<WirePromptTemplate>, ApiError> {
    let template = state
        .interactor()
        .update_prompt_template(id, &req.label, &req.text)
        .await?;
    match template {
        Some(template) => Ok(Json(WirePromptTemplate::from(template))),
        None => Err(ApiError::NotFound(format!(
            "no prompt template with id {id}"
        ))),
    }
}

/// `DELETE /api/prompt-templates/{id}` — remove a registered prompt template.
///
/// Deleting an unknown id is a no-op, so this is idempotent and always replies
/// `204`.
pub(crate) async fn delete_prompt_template(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    state.interactor().delete_prompt_template(id).await?;
    Ok(StatusCode::NO_CONTENT)
}
