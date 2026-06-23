//! Golden-corpus plumbing: loading cases, replaying them, and comparing
//! against the checked-in expectations.
//!
//! A corpus case is a directory under `tests/corpus/cases/<name>/` holding:
//!
//! - `transcript.jsonl` — a sanitized raw transcript in Claude Code's JSONL
//!   shape, parsed by the real gateway parser (`delta_transcript::parse_line`)
//!   with each message's `seq` set to its 0-based file line index, exactly as
//!   the production reader does;
//! - `overlay.json` — the session's overlay inputs: the `main` thread id and
//!   the sends (id, thread, optional semantic parent, text) in dispatch
//!   order;
//! - `expected.json` — the golden output: every parsed line's
//!   `(thread_id, semantic_parent_uuid)` assignment plus the ordered effect
//!   list the fold decides.
//!
//! Run with `UPDATE_GOLDEN=1` to rewrite every `expected.json` from the
//! current fold output (then review the diff).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use delta_attribution::{
    replay, Attributed, AttributionState, Effect, OutstandingSend, TranscriptMessage,
};
use delta_model::{MessageUuid, SessionId, ThreadId};

/// One loaded corpus case.
pub struct CorpusCase {
    pub name: String,
    pub lines: Vec<TranscriptMessage>,
    pub main_thread: ThreadId,
    pub sends: Vec<OutstandingSend>,
    pub expected_path: PathBuf,
}

impl CorpusCase {
    /// The session id a case replays under (assignments do not depend on it).
    pub fn session(&self) -> SessionId {
        SessionId::from(format!("corpus-{}", self.name))
    }

    /// Replay the whole case through the pure fold in one batch.
    pub fn replay(&self) -> Attributed {
        replay(
            &self.session(),
            self.main_thread,
            self.sends.clone(),
            self.lines.clone(),
        )
    }

    /// The state [`Self::replay`] starts from: a fresh session's carry seed
    /// with the whole send history queued in dispatch order. Exposed so the
    /// batch-split property can thread the same seed through partial folds.
    pub fn replay_seed(&self) -> AttributionState {
        AttributionState {
            carry_thread: self.main_thread,
            outstanding: self.sends.clone().into(),
            // Whole-history replay seeds no outstanding launch: every launch and
            // its completion fall within the replayed lines.
            launched_threads: std::collections::BTreeMap::new(),
            // Likewise no local-command group is carried in: each group's caveat
            // precedes its trailing lines within the replayed batch.
            local_command_prompts: std::collections::HashSet::new(),
        }
    }
}

/// The overlay inputs checked in next to a transcript fixture.
#[derive(Deserialize)]
struct OverlayFile {
    /// Free-form provenance note (e.g. the generating fake-claude scenario).
    #[serde(default, rename = "comment")]
    _comment: String,
    main_thread: i64,
    #[serde(default)]
    sends: Vec<OverlaySend>,
}

#[derive(Deserialize)]
struct OverlaySend {
    id: i64,
    thread_id: i64,
    #[serde(default)]
    semantic_parent_uuid: Option<String>,
    text: String,
}

/// The golden file: assignments in line order plus the ordered effects.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoldenCase {
    pub assignments: Vec<GoldenAssignment>,
    #[serde(default)]
    pub effects: Vec<GoldenEffect>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoldenAssignment {
    pub uuid: String,
    pub role: String,
    /// The 0-based transcript file line index (pins skipped-line gaps).
    pub seq: i64,
    pub thread_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_parent_uuid: Option<String>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GoldenEffect {
    SendMatched { send_id: i64, matched_uuid: String },
    TurnInterrupted,
    TurnAborted,
    LocalCommandTurnEnded,
    ResolvePermission { tool_use_id: String, allowed: bool },
    SubagentLaunched { tool_use_id: String, thread_id: i64 },
    SubagentCompleted { tool_use_id: String },
}

/// Project a fold outcome onto the golden shape.
pub fn golden_of(outcome: &Attributed) -> GoldenCase {
    GoldenCase {
        assignments: outcome
            .messages
            .iter()
            .map(|m| GoldenAssignment {
                uuid: m.uuid.as_str().to_owned(),
                role: m.role.as_str().to_owned(),
                seq: m.seq,
                thread_id: m.thread_id.value(),
                semantic_parent_uuid: m
                    .semantic_parent_uuid
                    .as_ref()
                    .map(|u| u.as_str().to_owned()),
            })
            .collect(),
        effects: outcome
            .effects
            .iter()
            .map(|e| match e {
                Effect::SendMatched {
                    send_id,
                    matched_uuid,
                } => GoldenEffect::SendMatched {
                    send_id: *send_id,
                    matched_uuid: matched_uuid.as_str().to_owned(),
                },
                Effect::TurnInterrupted => GoldenEffect::TurnInterrupted,
                Effect::TurnAborted => GoldenEffect::TurnAborted,
                Effect::LocalCommandTurnEnded => GoldenEffect::LocalCommandTurnEnded,
                Effect::ResolvePermission {
                    tool_use_id,
                    allowed,
                } => GoldenEffect::ResolvePermission {
                    tool_use_id: tool_use_id.clone(),
                    allowed: *allowed,
                },
                Effect::SubagentLaunched {
                    tool_use_id,
                    thread_id,
                } => GoldenEffect::SubagentLaunched {
                    tool_use_id: tool_use_id.clone(),
                    thread_id: thread_id.value(),
                },
                Effect::SubagentCompleted { tool_use_id } => GoldenEffect::SubagentCompleted {
                    tool_use_id: tool_use_id.clone(),
                },
            })
            .collect(),
    }
}

/// Load every case under `tests/corpus/cases/`, sorted by name.
pub fn load_cases() -> Vec<CorpusCase> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/cases");
    let mut cases: Vec<CorpusCase> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("read corpus dir {}: {e}", root.display()))
        .map(|entry| entry.expect("corpus dir entry").path())
        .filter(|p| p.is_dir())
        .map(|dir| load_case(&dir))
        .collect();
    assert!(!cases.is_empty(), "corpus at {} is empty", root.display());
    cases.sort_by(|a, b| a.name.cmp(&b.name));
    cases
}

fn load_case(dir: &Path) -> CorpusCase {
    let name = dir
        .file_name()
        .expect("case dir name")
        .to_string_lossy()
        .into_owned();
    let transcript = std::fs::read_to_string(dir.join("transcript.jsonl"))
        .unwrap_or_else(|e| panic!("case {name}: read transcript.jsonl: {e}"));
    let overlay: OverlayFile = serde_json::from_str(
        &std::fs::read_to_string(dir.join("overlay.json"))
            .unwrap_or_else(|e| panic!("case {name}: read overlay.json: {e}")),
    )
    .unwrap_or_else(|e| panic!("case {name}: parse overlay.json: {e}"));

    // Parse exactly like the production reader: every file line advances the
    // index (so `seq` keeps true file positions across skipped lines), and
    // only lines that parse to a message are kept.
    let lines = transcript
        .lines()
        .enumerate()
        .filter_map(|(idx, line)| {
            let parsed = delta_transcript::parse_line(line)
                .unwrap_or_else(|e| panic!("case {name}: line {idx} unparsable: {e}"));
            parsed.map(|mut msg| {
                msg.seq = idx as i64;
                msg
            })
        })
        .collect();

    CorpusCase {
        name,
        lines,
        main_thread: ThreadId::from(overlay.main_thread),
        sends: overlay
            .sends
            .into_iter()
            .map(|s| OutstandingSend {
                id: s.id,
                thread_id: ThreadId::from(s.thread_id),
                semantic_parent_uuid: s.semantic_parent_uuid.map(MessageUuid::from),
                text: s.text,
                task_id: None,
            })
            .collect(),
        expected_path: dir.join("expected.json"),
    }
}

/// Compare a case's replay against its golden file; on mismatch, fail with a
/// per-line diff of the pretty-printed JSON. With `UPDATE_GOLDEN=1` the
/// golden file is rewritten instead.
pub fn assert_matches_golden(case: &CorpusCase) {
    let actual = golden_of(&case.replay());
    let actual_json = serde_json::to_string_pretty(&actual).expect("serialize actual") + "\n";

    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&case.expected_path, &actual_json)
            .unwrap_or_else(|e| panic!("case {}: write golden: {e}", case.name));
        return;
    }

    let expected_json = std::fs::read_to_string(&case.expected_path).unwrap_or_else(|e| {
        panic!(
            "case {}: read {} ({e}); run with UPDATE_GOLDEN=1 to create it",
            case.name,
            case.expected_path.display()
        )
    });
    if expected_json == actual_json {
        return;
    }
    panic!(
        "case {}: replay diverges from {}\n{}\n(run with UPDATE_GOLDEN=1 to bless the new output)",
        case.name,
        case.expected_path.display(),
        diff(&expected_json, &actual_json),
    );
}

/// A minimal line diff: every differing line of the two pretty-printed JSON
/// documents, prefixed `-` (expected) / `+` (actual), with one line of
/// context above so the enclosing entry is identifiable.
fn diff(expected: &str, actual: &str) -> String {
    let exp: Vec<&str> = expected.lines().collect();
    let act: Vec<&str> = actual.lines().collect();
    let mut out = String::new();
    for i in 0..exp.len().max(act.len()) {
        let e = exp.get(i).copied();
        let a = act.get(i).copied();
        if e != a {
            if let (Some(prev), true) = (i.checked_sub(1).and_then(|p| act.get(p)), out.is_empty())
            {
                out.push_str(&format!("    {prev}\n"));
            }
            if let Some(e) = e {
                out.push_str(&format!("  - {e}\n"));
            }
            if let Some(a) = a {
                out.push_str(&format!("  + {a}\n"));
            }
        }
    }
    out
}
