//! `/api/launch-options` — the registered launch options.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use delta_bootstrap::AppInteractor;
use delta_usecase::{AgentProvider, LaunchOption};
use delta_wire::rest::{
    WireCreateLaunchOptionRequest, WireLaunchOption, WireLaunchOptionsResponse,
    WireUpdateLaunchOptionRequest,
};

use crate::state::AppState;

use super::ApiError;

/// `GET /api/launch-options` — the registered launch options.
///
/// Returns the flat `(label?, name, value?)` records for the settings screen to
/// list and manage: the rows Delta ships first, in declared-catalog order, then
/// the ones the user registered, newest first. `builtin` tells the two apart.
/// Selecting which to apply when starting a session is a separate concern
/// handled elsewhere.
///
/// Every row also carries `dangerous`, resolved here through the provider's own
/// predicate rather than read from storage, so a client can mark the row and
/// never pre-check it — including a row that predates the rule and still says
/// `default_enabled: true`, whose default control is the way to clear that stale
/// flag. Likewise `choice_group`, the exclusive group a single-valued name puts
/// its rows in (see [`wire_launch_option`]).
pub(crate) async fn list_launch_options(
    State(state): State<AppState>,
) -> Result<Json<WireLaunchOptionsResponse>, ApiError> {
    let interactor = state.interactor();
    let options = interactor.list_launch_options().await?;
    Ok(Json(WireLaunchOptionsResponse {
        launch_options: options
            .into_iter()
            .map(|option| wire_launch_option(interactor, option))
            .collect(),
    }))
}

/// Render a registered launch option for the wire, with the verdicts only the
/// provider's vocabulary can give — `dangerous` and `choice_group` — resolved
/// through the interactor's injected vocabulary.
///
/// One function for the list, create and `PATCH` responses so the three cannot
/// disagree about a row.
fn wire_launch_option(interactor: &AppInteractor, option: LaunchOption) -> WireLaunchOption {
    let dangerous = interactor.is_launch_option_dangerous(&option);
    let choice_group = interactor.launch_option_choice_group(&option);
    WireLaunchOption::new(option, dangerous, choice_group)
}

/// `POST /api/launch-options` — register a new custom launch option.
///
/// `name` is required and must be non-blank; `label` and `value` are optional
/// (a valueless option carries no `value`). A blank `name` is a `400`. What
/// `name` means is the provider's business — a CLI flag for Claude, a
/// `thread/start` field for Codex — so the validation here is deliberately only
/// "present and non-blank", and the message stays provider-neutral. Returns the
/// created record so the client can render it without a refetch.
///
/// The one exception to that neutrality is the safety-bypass rule: an option that
/// disables the agent's own safety mechanism can be registered, but not with
/// `default_enabled: true` — that is a `400` `launch_option_rejected` from the
/// use case (see `create_launch_option` there). The same option undefaulted is
/// created normally and stays selectable per session. So is a `default_enabled`
/// create into a choice group whose default another row already holds: the
/// group takes one default, and the server never clears the sibling itself.
pub(crate) async fn create_launch_option(
    State(state): State<AppState>,
    Json(req): Json<WireCreateLaunchOptionRequest>,
) -> Result<(StatusCode, Json<WireLaunchOption>), ApiError> {
    let name = req.name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest(
            "a launch option must have a non-blank `name`".to_owned(),
        ));
    }
    // `label`/`value` are kept verbatim apart from trimming surrounding
    // whitespace; an all-blank optional is treated as absent rather than a
    // stored empty string.
    let label = req
        .label
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let value = req
        .value
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    // A create that omits `provider` is a Claude option, keeping clients that
    // predate per-provider launch options working unchanged.
    let provider = req
        .provider
        .map(AgentProvider::from)
        .unwrap_or(AgentProvider::Claude);
    let interactor = state.interactor();
    let option = interactor
        .create_launch_option(label, name, value, req.default_enabled, provider)
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(wire_launch_option(interactor, option)),
    ))
}

/// `PATCH /api/launch-options/{id}` — set a launch option's `default_enabled`
/// flag in place.
///
/// Updating in place preserves the option's id and `created_at` (a
/// delete+recreate would churn both); `name`, `value`, and `label` are immutable
/// through this endpoint. Returns the updated record so the client can render it
/// without a refetch, or `404` when no option has that id.
///
/// Applies to a row Delta ships exactly as it does to the user's own. That the
/// three content fields are immutable *here* is precisely what lets startup
/// refresh a shipped row's `label`/`name`/`value` from the declared catalog
/// without ever overwriting something the user typed.
///
/// Turning the flag *on* for an option that disables the agent's own safety
/// mechanism is a `400` `launch_option_rejected`, the same refusal the create
/// path gives; turning it off is always allowed, which is how a row registered
/// before that rule is disarmed. Turning it on while a sibling of the row's
/// choice group holds the default is refused the same way, so switching a
/// group's default is two requests: clear the holder, then set the new row.
pub(crate) async fn update_launch_option(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<WireUpdateLaunchOptionRequest>,
) -> Result<Json<WireLaunchOption>, ApiError> {
    let interactor = state.interactor();
    let option = interactor
        .set_launch_option_default_enabled(id, req.default_enabled)
        .await?;
    match option {
        Some(option) => Ok(Json(wire_launch_option(interactor, option))),
        None => Err(ApiError::NotFound(format!("no launch option with id {id}"))),
    }
}

/// `DELETE /api/launch-options/{id}` — remove a registered launch option.
///
/// Deleting an unknown id is a no-op, so this is idempotent and replies `204`.
/// A row Delta ships is refused with a `409` (see
/// [`delta_usecase::Error::LaunchOptionIsBuiltin`]): the declared catalog owns
/// those, so a removed row would simply reappear at the next startup. `PATCH` on
/// the same row still works — ticking `default_enabled` on a shipped option is
/// the point of shipping it.
pub(crate) async fn delete_launch_option(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    state.interactor().delete_launch_option(id).await?;
    Ok(StatusCode::NO_CONTENT)
}
