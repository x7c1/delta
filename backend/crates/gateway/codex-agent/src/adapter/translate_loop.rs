//! The per-session translation task.

use std::sync::{Arc, Mutex, Weak};

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use delta_usecase::{
    AgentEvent, AgentFileChange, AgentPermissionRequest, CommsLogSink, SessionEndReason,
};

use super::{record_inbound, SharedSessionState, METHOD_NOT_FOUND};
use crate::file_change_items::FileChangeItems;
use crate::translate::{
    classify_server_request, completed_item_id, file_change_item, is_turn_completed,
    started_turn_id, translate_notification, with_file_change_detail, ServerRequestKind,
};
use crate::wire::Notification;
use crate::{AppServerConnection, ThreadEvent};

/// The translation task for one session: drain the thread's frames, translate
/// each into neutral events, and push them onto the session's channel.
///
/// Server → client requests are handled here rather than in the pure translator
/// because they need I/O: a modeled approval is recorded (so a later decision
/// can answer it), and an unmodeled request is answered immediately with an
/// error so the session never hangs on it.
///
/// This is also the adapter's **receive-side comms-log tap**: every frame the
/// demux routes to this thread is mirrored into `comms` under `session_id`
/// before it is translated, which is what makes the inspector show the pushed
/// flow (`turn/*`, `item/*`, approval requests) and not just Delta's own calls.
/// It is done here rather than in the connection's reader because the reader
/// keys frames by provider thread id while the log is keyed by Delta session id,
/// and this task is the first place both are in scope.
pub(super) async fn translate_loop(
    session_id: String,
    mut events: UnboundedReceiver<ThreadEvent>,
    tx: UnboundedSender<AgentEvent>,
    conn: Weak<AppServerConnection>,
    comms: Arc<dyn CommsLogSink>,
    shared: SharedSessionState,
) {
    while let Some(event) = events.recv().await {
        record_inbound(&comms, &session_id, &event);
        match event {
            // The connection died (the app-server process exited or its pipe
            // broke). Surface it as the terminal neutral fact so the core can
            // settle everything a dead wire strands — the in-flight turn, the
            // approvals awaiting a decision that can no longer be written, the
            // session's open state — instead of the stream just going silent.
            // Distinct from `close()`'s `SessionEndReason::Closed`: that is an
            // orderly teardown Delta asked for, this is a loss it must react to.
            ThreadEvent::ConnectionLost => {
                // Nothing tracked can still be answered on a dead wire, and the
                // diffs are the bulky part of the map, so release them before
                // the session's terminal event goes out.
                shared
                    .file_change_items
                    .lock()
                    .expect("file change items mutex poisoned")
                    .clear();
                let _ = tx.send(AgentEvent::SessionEnded {
                    reason: SessionEndReason::ProcessExited,
                });
                // Release the session's comms-log buffer, the way `close` does
                // for the orderly path. The core settles a death by dropping the
                // session's adapter instead of calling `close`, so this is the
                // only place a died session's buffer is ever released — and it
                // must be released: the log's contract is that a session's
                // buffer goes when the session closes (a reconnect then shows an
                // empty log), and its own pruning reclaims only buffers that are
                // both empty and unwatched. Left behind, a dead session's ring —
                // and every browser still subscribed to it, whose socket would
                // hang instead of ending — would be held for the process's
                // lifetime.
                comms.discard(&session_id);
                // Terminal by construction: the transport announces it once,
                // last, so there is nothing further to translate.
                return;
            }
            ThreadEvent::Notification(notification) => {
                // Maintain the tracked turn id off the pushed `turn/*` frames: a
                // `turn/started` re-affirms the id (the `turn/start` response
                // already set it), and a `turn/completed` clears it so a later
                // interrupt does not reference a finished turn.
                if let Some(turn_id) = started_turn_id(&notification) {
                    *shared
                        .current_turn_id
                        .lock()
                        .expect("current turn id mutex poisoned") = Some(turn_id);
                } else if is_turn_completed(&notification) {
                    *shared
                        .current_turn_id
                        .lock()
                        .expect("current turn id mutex poisoned") = None;
                }
                // Done before the notification is translated so an approval
                // that follows in the same batch already sees it.
                maintain_file_change_items(&shared.file_change_items, &notification);
                for event in translate_notification(&notification) {
                    if tx.send(event).is_err() {
                        return;
                    }
                }
            }
            ThreadEvent::ServerRequest(request) => match classify_server_request(&request) {
                ServerRequestKind::Approval(mut permission) => {
                    attach_file_changes(&shared.file_change_items, &mut permission);
                    shared
                        .approvals
                        .lock()
                        .expect("approvals mutex poisoned")
                        .insert(permission.request_id.clone(), request.id.clone());
                    if tx
                        .send(AgentEvent::PermissionRequested {
                            request: permission,
                        })
                        .is_err()
                    {
                        return;
                    }
                }
                ServerRequestKind::Unsupported {
                    method,
                    detail_json,
                } => {
                    // Answer first so the server unblocks, then surface it.
                    if let Some(conn) = conn.upgrade() {
                        let _ = conn
                            .respond_error(
                                Some(&session_id),
                                &request.id,
                                METHOD_NOT_FOUND,
                                &format!("unsupported server request: {method}"),
                            )
                            .await;
                    }
                    if tx
                        .send(AgentEvent::UnsupportedInteraction {
                            method,
                            detail_json,
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            },
        }
    }
}

/// Fold one pushed notification into the session's file-change correlation map.
///
/// Three frames move it, and each is a different fact about the same patch:
///
/// - an `item/started` for a `fileChange` item, or an
///   `item/fileChange/patchUpdated`, **states** the item's change set (the
///   latter replacing what the former announced);
/// - an `item/completed` **retires** that item's entry — the patch is applied or
///   abandoned, so no approval can still be waiting on it;
/// - a `turn/completed` **empties** the map: nothing proposed by a turn that has
///   ended is still awaiting an answer, and this is the sweep that stops a
///   long-running session from accumulating every diff it ever showed.
fn maintain_file_change_items(items: &Arc<Mutex<FileChangeItems>>, notification: &Notification) {
    let mut items = items.lock().expect("file change items mutex poisoned");
    if let Some((item_id, changes)) = file_change_item(notification) {
        items.record(item_id, changes);
    }
    if let Some(item_id) = completed_item_id(notification) {
        items.forget(&item_id);
    }
    if is_turn_completed(notification) {
        items.clear();
    }
}

/// Complete an approval request with the change set of the item it gates, when
/// that item is known.
///
/// The lookup is by the request's `tool_use_id` — the Codex `itemId` — so a
/// command-execution approval (whose item is never tracked) and a file-change
/// approval whose item was missed both simply find nothing and keep
/// `file_change: None`. That `None` is the deliberate fallback: the browser card
/// then renders the request's own params exactly as it did before this
/// correlation existed, rather than an empty or invented detail block.
///
/// An item that names **no** file takes the same fallback, and that is what
/// makes "a detail is present" mean "these files would change" everywhere
/// downstream. An item can end up naming none — a `changes: []` array, or one
/// whose every entry was dropped for lacking a path — and a detail minted from
/// it would say nothing the input summary does not, while obliging every client
/// (not just Delta's own card) to carry an empty-state branch the wire contract
/// promises it will never need.
fn attach_file_changes(
    items: &Arc<Mutex<FileChangeItems>>,
    permission: &mut AgentPermissionRequest,
) {
    let Some(item_id) = permission.tool_use_id.clone() else {
        return;
    };
    let changes: Option<Vec<AgentFileChange>> = items
        .lock()
        .expect("file change items mutex poisoned")
        .get(&item_id)
        .filter(|changes| !changes.is_empty());
    if let Some(changes) = changes {
        with_file_change_detail(permission, changes);
    }
}
