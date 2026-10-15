//! A permission dialog currently awaiting a human answer.

use delta_usecase::PendingPermission;
use serde::Serialize;
use ts_rs::TS;

use crate::file_change_detail::WireFileChangeDetail;

/// A permission dialog currently awaiting a human answer, as reported on the
/// REST surface.
///
/// This is the queryable counterpart of the `permission_requested` event
/// (same fields, minus the session id the URL already names): the event is
/// lost for a client whose socket was down when it fired, so a reconnecting
/// client rebuilds its permission notice from this instead.
///
/// Several requests can be pending at once (a provider running tool calls in
/// parallel), and the envelope reports the queue's **head** here plus the depth
/// in `permission_count` — the dialog to show, and how many answers are owed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "PendingPermission")]
pub struct WirePendingPermission {
    /// The request row id `POST /api/permissions/{id}/decision` answers.
    pub request_id: i64,
    pub tool_name: String,
    /// The tool input, serialized as JSON text.
    pub tool_input: String,
    /// What allowing the request would do to files on disk, when the provider
    /// stated it. The same detail the `permission_requested` event carries, so a
    /// client that missed that event rebuilds the *same* card from this refetch
    /// rather than one without the change view.
    ///
    /// Absent from the envelope entirely when nothing is known, which is every
    /// request that is not a file change.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub file_change: Option<WireFileChangeDetail>,
    /// A directory the request also asks to be allowed to write under for the
    /// rest of the session, when the provider asked for one — the same field the
    /// `permission_requested` event carries, and independent of `file_change` in
    /// the same way: a request can carry this and no change set at all.
    ///
    /// Absent from the envelope entirely when the provider asked for no root.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub grant_root: Option<String>,
}

impl From<&PendingPermission> for WirePendingPermission {
    fn from(pending: &PendingPermission) -> Self {
        WirePendingPermission {
            request_id: pending.request_id,
            tool_name: pending.tool_name.clone(),
            tool_input: pending.tool_input_json.clone(),
            file_change: pending.file_change.as_ref().map(WireFileChangeDetail::from),
            grant_root: pending.grant_root.clone(),
        }
    }
}
