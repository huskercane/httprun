# Parallel execution design notes

Exploring whether `httprun` could execute requests from a `.http` file
concurrently, and what the tradeoffs look like.

## The hard part

`.http` files have **implicit ordering via JS handlers**. Request N's handler
can `client.global.set("token", ...)`, and request N+1's URL / headers / body
may reference `{{token}}`. You can't reorder or parallelize without knowing
those edges, and they're only discoverable by *running* the handler — static
analysis of arbitrary JS is undecidable.

## Three options, ranked

### 1. Explicit opt-in groups (recommended start)

Add a comment directive like `# @parallel groupA` (similar to existing
`# @name`). Requests in the same group run concurrently; the runner waits for
the whole group to finish before moving on.

- Pros: lowest engineering cost, zero false positives, matches how users
  already think about their files (login is sequential, smoke tests are
  parallel).
- Cons: requires user annotation.
- Estimated effort: ~50 lines.

### 2. Dependency inference from variable references

Build a DAG:
- Scan each request's URL / headers / body for `{{var}}`.
- An edge exists from any earlier request whose handler writes that var
  (via `client.global.set`) to the request that reads it.
- Run the topological levels in parallel.

- Pros: catches the easy cases automatically.
- Cons: fails on dynamic keys (`client.global.set(computedName, ...)`),
  variables set by `pre-request` scripts, and implicit dependencies like
  "this login must succeed before anything else even though no var crosses."
- Mitigation: needs an escape hatch (`# @sequential` to force ordering).
- Useful as a default if gated behind `--auto-parallel` so the default stays
  surprise-free.

### 3. Speculative execution + replay on conflict

Run everything in parallel optimistically, snapshot global state, detect
read-after-write conflicts, replay losers.

- Verdict: way too much machinery for a CLI. Skip.

## Recommendation

Start with **option 1**. Layer option 2 on top later if there's demand,
gated behind `--auto-parallel`.

## Async runtime implications

Going parallel is the trigger for moving off blocking `reqwest`. When that
day comes:

- `tokio` with `rt` (single-thread) feature, not `rt-multi-thread` — a CLI
  doesn't need a worker pool.
- `reqwest` already has the async client; swap `blocking::Client` → `Client`.
- Boa's `Context` isn't `Send`. Either run JS handlers off the runtime
  entirely, or wrap each in `tokio::task::spawn_blocking`.
- `main` becomes `#[tokio::main(flavor = "current_thread")] async fn main()`
  — macro-pinned, so the "prefer `impl Future`" rule from the global
  CLAUDE.md doesn't apply to it.

Until parallelism (or watch mode, streaming bodies, etc.) is actually built,
stay blocking. Simpler code, faster compiles, smaller binary.
