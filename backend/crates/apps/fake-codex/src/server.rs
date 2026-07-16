//! The engine: react to the client's JSON-RPC frames on stdin, writing
//! response/notification/server-request frames on stdout.

use std::io::{BufRead, StdoutLock, Write};

use serde_json::{json, Map, Value};

use crate::scenario::{Emit, Scenario, Turn};

/// Resolve the scenario and serve JSON-RPC frames until stdin closes.
pub fn run() -> Result<(), String> {
    let scenario = Scenario::resolve()?;
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut server = Server {
        scenario,
        out: stdout.lock(),
        server_request_seq: 0,
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
            // A response to one of our server → client requests. Nothing to do
            // in this phase; log so a captured run shows the client answered.
            (None, Some(id)) => {
                eprintln!("fake-codex: client answered server request {id}");
                Ok(())
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
                    self.play_turn(&turn, &thread_id)?;
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

    /// Play a turn's scripted emissions, in order, stamping `threadId` into each.
    fn play_turn(&mut self, turn: &Turn, thread_id: &str) -> Result<(), String> {
        for emit in &turn.emit {
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
                Emit::RequestApproval { method, params } => {
                    // A server → client request. The fake emits it and continues
                    // (it does not block on the client's reply in this phase).
                    self.emit_server_request(method, with_thread_id(params.clone(), thread_id))?
                }
                Emit::Notification { method, params } => {
                    self.emit_notification(method, with_thread_id(params.clone(), thread_id))?
                }
            }
        }
        Ok(())
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
