//! The engine: react to the client's JSON-RPC frames on stdin, writing
//! response/notification/server-request frames on stdout.

use std::io::{BufRead, StdoutLock, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::scenario::{Emit, Scenario};

/// A fixed timestamp (Unix ms) stamped into the item lifecycle envelope
/// (`startedAtMs` / `completedAtMs`). Real servers emit the wall-clock instant;
/// the fake uses a constant so scripted turns stay byte-deterministic (nothing
/// asserts on the value — it exists to exercise the real envelope shape).
const ENVELOPE_TS_MS: i64 = 1_784_272_338_000;

/// Resolve the scenario and serve JSON-RPC frames until stdin closes.
pub fn run() -> Result<(), String> {
    let scenario = Scenario::resolve()?;
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut server = Server {
        scenario,
        out: stdout.lock(),
        server_request_seq: 0,
        turn_index: 0,
        pending: None,
        // A sidecar record of the items each `thread/inject_items` carried, so a
        // full-loop branch test can prove the hidden context reached the server.
        // Off unless the client hands the fake a path via this env var.
        inject_log: std::env::var_os("FAKE_CODEX_INJECT_LOG").map(PathBuf::from),
        // The same idea for `thread/start`: record the params the client sent,
        // so a full-loop test can prove the session's launch options reached the
        // server as real `ThreadStartParams` fields.
        thread_start_log: std::env::var_os("FAKE_CODEX_THREAD_START_LOG").map(PathBuf::from),
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
    /// How many `turn/start` requests this session has served, used to pick the
    /// turn from a scenario's `turns` sequence so successive turns can carry
    /// distinct ids. Ignored when the scenario uses the single `turn`.
    turn_index: usize,
    /// The suspended remainder of a turn, set when a `blocking` approval was
    /// emitted and awaiting the client's decision. Resumed (and cleared) when the
    /// client's response frame arrives. `None` when no turn is gated.
    pending: Option<PendingTurn>,
    /// Where to append each `thread/inject_items` payload (one JSON line per
    /// call), when the client set `FAKE_CODEX_INJECT_LOG`. `None` disables the
    /// record.
    inject_log: Option<PathBuf>,
    /// Where to append each `thread/start` params object (one JSON line per
    /// call), when the client set `FAKE_CODEX_THREAD_START_LOG`. `None` disables
    /// the record.
    thread_start_log: Option<PathBuf>,
}

/// A turn suspended on a `blocking` approval: the emits still to play once the
/// client answers, plus the thread and turn they belong to.
struct PendingTurn {
    thread_id: String,
    turn_id: String,
    remaining: Vec<Emit>,
}

impl Server<'_> {
    /// The body a `thread/start` / `thread/resume` response shares: the thread
    /// (its id plus the scenario's `gitInfo`, when it names one) alongside the
    /// resolved `model`. `gitInfo` is omitted entirely when the scenario names
    /// none — the shape a thread outside a git working tree gets.
    fn thread_response(&self, thread_id: &str) -> Value {
        let mut thread = Map::new();
        thread.insert("id".to_owned(), json!(thread_id));
        if let Some(git_info) = &self.scenario.git_info {
            thread.insert("gitInfo".to_owned(), git_info.clone());
        }
        json!({ "thread": Value::Object(thread), "model": self.scenario.model })
    }

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
                // The real `codex app-server` validates `clientInfo`: its
                // `ClientInfo` requires BOTH `name` and `version`, and it rejects
                // an `initialize` missing either with `[-32600] Invalid request:
                // missing field '<field>'`. Re-enact that here so the fake cannot
                // drift green while the real server would reject the handshake
                // (the gap the C4 real-codex canary caught).
                for field in ["name", "version"] {
                    let present = params
                        .get("clientInfo")
                        .and_then(|c| c.get(field))
                        .and_then(Value::as_str)
                        .is_some();
                    if !present {
                        return self.respond_error(
                            id,
                            -32600,
                            &format!("Invalid request: missing field `{field}`"),
                        );
                    }
                }
                let server_info = self.scenario.server_info.clone();
                self.respond(id, json!({ "serverInfo": server_info }))
            }
            "thread/start" => {
                // Record the params verbatim (for a test to assert the session's
                // launch options arrived as `ThreadStartParams` fields), then
                // answer. Real `thread/start` returns the started thread under
                // `result.thread` (a `Thread`, whose `id` is the thread id).
                append_record(self.thread_start_log.as_deref(), params, "thread start log")?;
                // The response also announces what the server decided and saw:
                // the real `ThreadStartResponse` requires a top-level `model`,
                // and its `Thread` carries the `gitInfo` captured from the
                // thread's working directory. The scenario's model is answered
                // verbatim, *ignoring* any `model` the client sent — which is how
                // a real server behaves when the user's config or its own default
                // wins.
                self.respond(id, self.thread_response(&self.scenario.thread_id.clone()))
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
                // `ThreadResumeResponse` carries the same top-level `model` and
                // `thread.gitInfo` as the start response, so a resumed thread
                // reports what it is running, and where, just like a fresh one.
                self.respond(id, self.thread_response(&thread_id))
            }
            "thread/inject_items" => {
                // Hidden per-turn context: the client appends Responses API
                // items to the thread's model-visible history before a branch
                // turn. Record what arrived (for the branch test to assert) and
                // reply with the empty object the real `ThreadInjectItemsResponse`
                // is. A missing `items` is recorded as `null` rather than failing,
                // so the record shape is always a value.
                let items = params.get("items").cloned().unwrap_or(Value::Null);
                self.record_injected_items(&items)?;
                self.respond(id, json!({}))
            }
            "turn/start" => {
                // The turn is scoped to the thread the client named, falling
                // back to the scenario's thread id.
                let thread_id = params
                    .get("threadId")
                    .and_then(Value::as_str)
                    .unwrap_or(&self.scenario.thread_id)
                    .to_owned();
                // Pick this session's next turn from the scenario: the `turns`
                // sequence (one per `turn/start`, distinct ids) when provided, or
                // the single `turn` replayed otherwise.
                let turn = self.scenario.turn_at(self.turn_index);
                self.turn_index += 1;
                let turn_id = turn
                    .as_ref()
                    .map(|t| t.turn_id.clone())
                    .unwrap_or_else(|| "turn_fake_0001".to_owned());
                // Real `turn/start` returns the started turn under `result.turn`
                // (a `Turn`, whose `id` is the turn id).
                self.respond(id, json!({ "turn": turn_object(&turn_id, "inProgress") }))?;
                if let Some(turn) = turn {
                    self.play_emits(&turn.emit, &thread_id, &turn_id)?;
                }
                Ok(())
            }
            "turn/interrupt" => {
                let thread_id = params
                    .get("threadId")
                    .and_then(Value::as_str)
                    .unwrap_or(&self.scenario.thread_id)
                    .to_owned();
                // Real `turn/interrupt` requires `{threadId, turnId}`. Asserting
                // the turn id here is how the fake proves the client sends it: a
                // client that omits it gets a JSON-RPC error, failing the loop
                // rather than silently "interrupting" nothing.
                let Some(turn_id) = params.get("turnId").and_then(Value::as_str) else {
                    return self.respond_error(id, -32602, "turn/interrupt requires a turnId");
                };
                let turn_id = turn_id.to_owned();
                self.respond(id, json!({}))?;
                // An interrupted turn ends with a `turn/completed` carrying the
                // interrupted status, echoing the interrupted turn's id back.
                self.emit_notification(
                    "turn/completed",
                    with_thread_id(
                        json!({ "turn": turn_object(&turn_id, "interrupted") }),
                        &thread_id,
                    ),
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
    fn play_emits(&mut self, emits: &[Emit], thread_id: &str, turn_id: &str) -> Result<(), String> {
        for (i, emit) in emits.iter().enumerate() {
            match emit {
                // Real `item/started` / `item/completed` wrap the item alongside
                // the `threadId` / `turnId` / `startedAtMs`|`completedAtMs`
                // envelope (see the vendored ItemStarted/ItemCompleted schemas).
                Emit::ItemStarted { item } => self.emit_notification(
                    "item/started",
                    with_thread_id(
                        json!({ "item": item, "turnId": turn_id, "startedAtMs": ENVELOPE_TS_MS }),
                        thread_id,
                    ),
                )?,
                Emit::ItemCompleted { item } => self.emit_notification(
                    "item/completed",
                    with_thread_id(
                        json!({ "item": item, "turnId": turn_id, "completedAtMs": ENVELOPE_TS_MS }),
                        thread_id,
                    ),
                )?,
                // Real `item/agentMessage/delta`: `{ itemId, delta, turnId }`.
                Emit::AgentMessageDelta { item_id, delta } => self.emit_notification(
                    "item/agentMessage/delta",
                    with_thread_id(
                        json!({ "itemId": item_id, "delta": delta, "turnId": turn_id }),
                        thread_id,
                    ),
                )?,
                // Real `turn/started` / `turn/completed` wrap a `Turn` under
                // `params.turn` (whose `id`/`status` the client reads).
                Emit::TurnStarted => self.emit_notification(
                    "turn/started",
                    with_thread_id(
                        json!({ "turn": turn_object(turn_id, "inProgress") }),
                        thread_id,
                    ),
                )?,
                Emit::TurnCompleted { status } => self.emit_notification(
                    "turn/completed",
                    with_thread_id(json!({ "turn": turn_object(turn_id, status) }), thread_id),
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
                            turn_id: turn_id.to_owned(),
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
        let turn_id = pending.turn_id;
        // Echo the decision as an assistant message, on a dedicated item id so it
        // never collides with the turn's own items.
        let echo_id = format!("approval_echo_{}", self.server_request_seq);
        self.emit_notification(
            "item/started",
            with_thread_id(
                json!({ "item": { "id": echo_id, "type": "agentMessage" }, "turnId": turn_id, "startedAtMs": ENVELOPE_TS_MS }),
                &thread_id,
            ),
        )?;
        self.emit_notification(
            "item/completed",
            with_thread_id(
                json!({ "item": { "id": echo_id, "type": "agentMessage", "text": decision }, "turnId": turn_id, "completedAtMs": ENVELOPE_TS_MS }),
                &thread_id,
            ),
        )?;
        self.play_emits(&pending.remaining, &thread_id, &turn_id)
    }

    /// Append one `thread/inject_items` payload to the sidecar record, when the
    /// client enabled it via `FAKE_CODEX_INJECT_LOG`. A no-op otherwise. Each
    /// call writes one JSON line, so a test can read back every injection in
    /// order.
    fn record_injected_items(&self, items: &Value) -> Result<(), String> {
        append_record(self.inject_log.as_deref(), items, "inject log")
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

/// Append one JSON line to a sidecar record file, when the client asked for one
/// (`path` is `None` otherwise, and this is a no-op). Every record the fake
/// keeps works this way — one JSON value per line, appended in call order — so a
/// test reads back exactly what the client sent, in order. `what` names the
/// record in error messages.
fn append_record(path: Option<&Path>, value: &Value, what: &str) -> Result<(), String> {
    let Some(path) = path else {
        return Ok(());
    };
    let mut line = serde_json::to_string(value).map_err(|e| format!("encode {what}: {e}"))?;
    line.push('\n');
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("open {what} `{}`: {e}", path.display()))?;
    file.write_all(line.as_bytes())
        .map_err(|e| format!("write {what}: {e}"))
}

/// A minimal `Turn` object as the real schema shapes it: the `id`, a `status`,
/// and an (empty) `items` array — the three fields the `Turn` definition marks
/// required. Carried under `result.turn` (on `turn/start`) and `params.turn` (on
/// `turn/started` / `turn/completed`).
fn turn_object(turn_id: &str, status: &str) -> Value {
    json!({ "id": turn_id, "status": status, "items": [] })
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
