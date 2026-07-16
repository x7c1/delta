//! The engine: react to the client's JSON-RPC frames on stdin, writing
//! response/notification/server-request frames on stdout.

use std::io::{BufRead, StdoutLock, Write};

use serde_json::{json, Map, Value};

use crate::scenario::{Emit, Scenario};

/// Resolve the scenario and serve JSON-RPC frames until stdin closes.
pub fn run() -> Result<(), String> {
    let scenario = Scenario::resolve()?;
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut server = Server {
        scenario,
        out: stdout.lock(),
        server_request_seq: 0,
        pending: None,
    };
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| format!("read stdin: {e}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let frame: Value =
            serde_json::from_str(&line).map_err(|e| format!("parse frame `{line}`: {e}"))?;
        server.handle(frame)?;
    }
    // Stdin closed: the client dropped the connection, so the server shuts down
    // (a real `codex app-server` exits when its controller goes away).
    Ok(())
}

struct Server<'a> {
    scenario: Scenario,
    out: StdoutLock<'a>,
    /// Mints ids for server → client requests (`*/requestApproval`).
    server_request_seq: u64,
    /// The suspended remainder of a turn, set when a `blocking` approval was
    /// emitted and awaiting the client's decision. Resumed (and cleared) when the
    /// client's response frame arrives. `None` when no turn is gated.
    pending: Option<PendingTurn>,
}

/// A turn suspended on a `blocking` approval: the emits still to play once the
/// client answers, plus the thread they belong to.
struct PendingTurn {
    thread_id: String,
    remaining: Vec<Emit>,
}

impl Server<'_> {
    /// Dispatch one incoming frame by its JSON-RPC shape.
    fn handle(&mut self, frame: Value) -> Result<(), String> {
        let id = frame.get("id").cloned();
        let method = frame.get("method").and_then(Value::as_str);
        match (method, id) {
            // A request: method + id. Answer it (and play any side effects).
            (Some(method), Some(id)) => {
                let method = method.to_owned();
                let params = frame.get("params").cloned().unwrap_or(Value::Null);
                self.handle_request(id, &method, &params)
            }
            // A notification: method, no id (e.g. `initialized`). No reply.
            (Some(_method), None) => Ok(()),
            // A response to one of our server → client requests. If a turn is
            // suspended on a blocking approval, this is the decision it was
            // waiting for: resume the turn, echoing the received decision. When
            // nothing is pending (a fire-and-forget approval) just log it.
            (None, Some(id)) => {
                let decision = frame
                    .get("result")
                    .and_then(|r| r.get("decision"))
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_owned();
                eprintln!("fake-codex: client answered server request {id} with {decision}");
                self.resume_pending_turn(&decision)
            }
            (None, None) => Err(format!("frame is not a JSON-RPC message: {frame}")),
        }
    }

    fn handle_request(&mut self, id: Value, method: &str, params: &Value) -> Result<(), String> {
        match method {
            "initialize" => {
                let server_info = self.scenario.server_info.clone();
                self.respond(id, json!({ "serverInfo": server_info }))
            }
            "thread/start" => {
                let thread_id = self.scenario.thread_id.clone();
                self.respond(id, json!({ "threadId": thread_id }))
            }
            "thread/resume" => {
                // Resume echoes back the requested thread id (a real server
                // rebuilds that thread's history); fall back to the scenario's
                // id when the client did not name one.
                let thread_id = params
                    .get("threadId")
                    .and_then(Value::as_str)
                    .unwrap_or(&self.scenario.thread_id)
                    .to_owned();
                self.respond(id, json!({ "threadId": thread_id }))
            }
            "turn/start" => {
                // The turn is scoped to the thread the client named, falling
                // back to the scenario's thread id.
                let thread_id = params
                    .get("threadId")
                    .and_then(Value::as_str)
                    .unwrap_or(&self.scenario.thread_id)
                    .to_owned();
                let turn = self.scenario.turn.clone();
                let turn_id = turn.as_ref().map(|t| t.turn_id.clone()).unwrap_or_default();
                self.respond(id, json!({ "turnId": turn_id }))?;
                if let Some(turn) = turn {
                    self.play_emits(&turn.emit, &thread_id)?;
                }
                Ok(())
            }
            "turn/interrupt" => {
                let thread_id = params
                    .get("threadId")
                    .and_then(Value::as_str)
                    .unwrap_or(&self.scenario.thread_id)
                    .to_owned();
                self.respond(id, json!({}))?;
                // An interrupted turn ends with a `turn/completed` carrying the
                // interrupted status.
                self.emit_notification(
                    "turn/completed",
                    with_thread_id(json!({ "status": "interrupted" }), &thread_id),
                )
            }
            other => {
                // An unknown request gets a JSON-RPC method-not-found error,
                // which the client surfaces rather than hanging.
                self.respond_error(id, -32601, &format!("method not found: {other}"))
            }
        }
    }

    /// Play scripted emissions in order, stamping `threadId` into each. Stops
    /// early (suspending the turn) when a `blocking` approval is emitted: the
    /// emits after it are parked in [`Self::pending`] and replayed by
    /// [`Self::resume_pending_turn`] once the client answers.
    fn play_emits(&mut self, emits: &[Emit], thread_id: &str) -> Result<(), String> {
        for (i, emit) in emits.iter().enumerate() {
            match emit {
                Emit::ItemStarted { item } => self.emit_notification(
                    "item/started",
                    with_thread_id(json!({ "item": item }), thread_id),
                )?,
                Emit::ItemCompleted { item } => self.emit_notification(
                    "item/completed",
                    with_thread_id(json!({ "item": item }), thread_id),
                )?,
                Emit::TurnStarted => {
                    self.emit_notification("turn/started", with_thread_id(json!({}), thread_id))?
                }
                Emit::TurnCompleted { status } => self.emit_notification(
                    "turn/completed",
                    with_thread_id(json!({ "status": status }), thread_id),
                )?,
                Emit::RequestApproval {
                    method,
                    params,
                    blocking,
                } => {
                    // A server → client request.
                    self.emit_server_request(method, with_thread_id(params.clone(), thread_id))?;
                    if *blocking {
                        // Suspend the turn: park the rest and wait for the
                        // client's decision (resumed in `resume_pending_turn`).
                        self.pending = Some(PendingTurn {
                            thread_id: thread_id.to_owned(),
                            remaining: emits[i + 1..].to_vec(),
                        });
                        return Ok(());
                    }
                }
                Emit::Notification { method, params } => {
                    self.emit_notification(method, with_thread_id(params.clone(), thread_id))?
                }
            }
        }
        Ok(())
    }

    /// Resume a turn suspended on a `blocking` approval, now that the client has
    /// answered with `decision` (`accept`/`decline`). Echoes the received
    /// decision back as a completed assistant message — so the value that
    /// round-tripped to the server is observable end-to-end — then plays the
    /// parked remainder of the turn. A no-op when no turn is suspended.
    fn resume_pending_turn(&mut self, decision: &str) -> Result<(), String> {
        let Some(pending) = self.pending.take() else {
            return Ok(());
        };
        let thread_id = pending.thread_id;
        // Echo the decision as an assistant message, on a dedicated item id so it
        // never collides with the turn's own items.
        let echo_id = format!("approval_echo_{}", self.server_request_seq);
        self.emit_notification(
            "item/started",
            with_thread_id(
                json!({ "item": { "id": echo_id, "itemType": "agent_message" } }),
                &thread_id,
            ),
        )?;
        self.emit_notification(
            "item/completed",
            with_thread_id(
                json!({ "item": { "id": echo_id, "itemType": "agent_message", "text": decision } }),
                &thread_id,
            ),
        )?;
        self.play_emits(&pending.remaining, &thread_id)
    }

    fn respond(&mut self, id: Value, result: Value) -> Result<(), String> {
        self.write_frame(json!({ "id": id, "result": result }))
    }

    fn respond_error(&mut self, id: Value, code: i64, message: &str) -> Result<(), String> {
        self.write_frame(json!({ "id": id, "error": { "code": code, "message": message } }))
    }

    fn emit_notification(&mut self, method: &str, params: Value) -> Result<(), String> {
        self.write_frame(json!({ "method": method, "params": params }))
    }

    fn emit_server_request(&mut self, method: &str, params: Value) -> Result<(), String> {
        self.server_request_seq += 1;
        let id = format!("srv-{}", self.server_request_seq);
        self.write_frame(json!({ "id": id, "method": method, "params": params }))
    }

    /// Write one newline-delimited frame and flush (stdout is block-buffered
    /// when piped, so an unflushed frame would never reach the client).
    fn write_frame(&mut self, frame: Value) -> Result<(), String> {
        let mut line = serde_json::to_string(&frame).map_err(|e| format!("encode frame: {e}"))?;
        line.push('\n');
        self.out
            .write_all(line.as_bytes())
            .map_err(|e| format!("write frame: {e}"))?;
        self.out.flush().map_err(|e| format!("flush frame: {e}"))
    }
}

/// Ensure `params` is a JSON object and stamp `threadId` into it, so every
/// thread-scoped frame carries the id the client transport demuxes on. A
/// non-object `params` (including `Null`) is replaced by a fresh object — the
/// scenarios only ever attach object params.
fn with_thread_id(params: Value, thread_id: &str) -> Value {
    let mut object = match params {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    object.insert("threadId".to_owned(), json!(thread_id));
    Value::Object(object)
}
