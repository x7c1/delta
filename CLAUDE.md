# Claude AI Guidelines

## Repository layout

Delta has two top-level parts, treated as equals:

- `backend/` — Rust `cargo` workspace. Crates live under `crates/` and are
  split by architectural layer: `apps/` (bins), `gateway/` (external I/O),
  `domain/` (model + usecase), `libs/` (shared). Dependency direction is
  enforced by crate boundaries.
- `frontend/` — TypeScript pnpm workspace. Packages live under `packages/`
  and are split by layer: `apps/`, `ui/`, `gateway/`, `domain/`, `testing/`.
  Dependency direction is enforced by dependency-cruiser.

## Documentation

**DRY Principle**: Write each piece of information in ONE place only.

- **README.md**: Overview and command reference only.
- **docs/guides/**: Detailed explanations.

Never duplicate content across files.

### Markdown Files (100+ lines)

- Always include an Overview section at the beginning.
- The Overview should summarize the document's purpose and key points.
- Automated tools may read only the beginning of `.md` files, so without an
  Overview at the top they cannot understand the document's content.

## Code Quality

After changing backend code, run from `backend/`:

```bash
cargo build && cargo test && cargo clippy --all-targets -- -D warnings
```

After changing frontend code, run from `frontend/`:

```bash
pnpm install && pnpm -r build && pnpm -r test && pnpm -r lint
```

(The exact frontend scripts are finalized as the workspace is wired up.)

Fix any issues before considering the task complete.

### Fix issues as you find them

- When you notice code smells or inappropriate patterns (silent error
  suppression, missing logs, inconsistent naming, etc.) during implementation,
  fix them in the same PR. Do not leave them for later or require the user to
  point them out.
- Do not defer cleanup of code you just wrote to a future PR. Duplicated
  queries, awkward interfaces, and missing abstractions in newly written code
  should be addressed immediately — merging bad code and fixing it later costs
  more than getting it right now. Reserve "out of scope" for genuinely
  unrelated large-scale refactors, not for polish on your own changes.

## Language

Documentation, code comments, commit messages, and pull-request descriptions
are written in English.

## Commit and PR messages

Commit messages and pull-request descriptions must be self-contained. Do not
reference external planning labels (milestone or sub-plan identifiers) or any
private/internal repository or document.

## Git

- Do not push directly to `main`. Always create a branch and open a pull
  request.
- `main` requires a passing CI run and a pull request before merging.
- Merges into `main` are squash-only, and the source branch is deleted after
  merge.
