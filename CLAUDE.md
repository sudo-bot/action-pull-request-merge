# action-pull-request-merge

GitHub / Gitea Action that merges a pull request when an event triggers the
workflow. Written in Rust, distributed as a Docker container action — no
Node.js runtime required on the runner.

Inputs: `github-token`, `number`, `merge-method`,
`allowed-usernames-regex`, `filter-label`, `merge-title`, `merge-message`.
See `action.yml` and `README.md`.

## Layout

```
src/
  main.rs            Tiny entrypoint. Picks the client backend and hands off.
  lib.rs             Re-exports + pick_backend(&ctx) -> Backend.
  action.rs          Decision logic: actor gate, PR fetch, label gate,
                     merge / fast-forward, label cleanup. Uses traits only,
                     so it can be exercised end-to-end with fakes.
  context.rs         Reads GITHUB_* env vars. Detects Gitea via
                     GITEA_ACTIONS=true OR a `/api/v1` URL suffix.
  inputs.rs          Reads INPUT_<NAME> env vars (matches @actions/core
                     name normalisation).
  github_client.rs   GithubClient trait + OctocrabClient (real GitHub
                     impl) + path-segment percent-encoder.
  gitea_client.rs    GiteaClient (real Gitea impl). Same trait, different
                     wire shape.
  logger.rs          Logger trait, WriteLogger<W: Write>, StdoutLogger
                     (= WriteLogger<io::Stdout>), CaptureLogger (test).
tests/
  integration.rs     action::run driven by a fake client.
  wire.rs            wiremock servers — pin the actual HTTP method, URL,
                     headers and body each client sends.
docker/
  Dockerfile         Multi-stage: rust:1-alpine build, alpine:3.23 runtime.
.github/workflows/   build / lock / merge / release.
```

## How it's wired

`action::run` knows nothing about HTTP. It calls four trait methods on a
`&dyn GithubClient`:

```
get_pull     GET            /repos/{o}/{r}/pulls/{n}
fast_forward PATCH | POST   /repos/{o}/{r}/git/refs/heads/{base}    (GitHub)
                            /repos/{o}/{r}/pulls/{n}/merge          (Gitea)
merge_pull   PUT | POST     /repos/{o}/{r}/pulls/{n}/merge
remove_label DELETE         /repos/{o}/{r}/issues/{n}/labels/{name|id}
```

Two implementations: `OctocrabClient` (GitHub) and `GiteaClient`. Both use
`octocrab::Octocrab` purely as an authenticated HTTP client; the typed
GitHub helpers from octocrab are *not* used. Selection happens once at
startup via `pick_backend(&ctx)`.

### Four places Gitea diverges from GitHub

These are the only behavioural differences between the two clients —
everything else is shared trait + identical wire calls:

1. **Merge endpoint method+body.** GitHub: `PUT` with
   `{merge_method, sha, commit_title, commit_message}`. Gitea: `POST` with
   CamelCase `{Do, MergeTitleField, MergeMessageField, head_commit_id}`.
2. **Empty merge response.** Gitea returns `200` with an empty body, so
   `GiteaClient::merge_pull` uses the low-level `_post` helper and checks
   the status manually instead of the typed `.post()` deserialiser.
3. **Label removal by id.** GitHub takes the label *name* in the URL;
   Gitea takes the numeric *id*. `GiteaClient::remove_label` does
   `GET .../issues/{n}/labels` first and resolves name → id via
   `resolve_label_id` (a pure helper kept testable on its own).
4. **Fast-forward goes through the merge endpoint.** GitHub's
   `fast_forward` PATCHes `git/refs/heads/{base}` to the head SHA — a
   true fast-forward with `force: false`. Gitea's `git/refs` API is
   read-only (PATCH there returns `405 Method Not Allowed`), so
   `GiteaClient::fast_forward` POSTs to `/pulls/{n}/merge` with
   `Do: "fast-forward-only"` and `head_commit_id`. Requires Gitea ≥ 1.22.

`get_pull` is wire-compatible across both forges.

## Build / test / lint

```sh
cargo build --release          # what the Dockerfile runs
cargo test                     # 76 tests across unit + integration + wire
cargo fmt --check              # style
cargo clippy --all-targets -- -D warnings   # lints
make docker-build              # local image build (linux/amd64 by default)
```

Pre-push gate: `cargo fmt --check && cargo clippy --all-targets -- -D
warnings && cargo test`. fmt is sub-second; clippy and test catch real
bugs.

## Testing model

Three layers, in increasing order of fidelity:

1. **Unit tests in each module.** Pure-function level: serde body shapes,
   `encode_path_segment`, `resolve_label_id`, `escape_data`, env parsing,
   `pick_backend`. Fastest, most numerous.
2. **`tests/integration.rs`.** Drives `action::run` against a fake
   `GithubClient` to verify decision logic end-to-end (skip cases, merge,
   fast-forward, label cleanup). No HTTP.
3. **`tests/wire.rs`.** Stands up an in-process `wiremock` server and
   asserts the exact HTTP method, path, body and `Authorization` header
   each client sends. This is the only layer that catches "we shipped
   `PUT` instead of `POST`" or "we put the label name in the URL when
   Gitea wants the id" — the body-shape unit tests can't.

Always add or extend a wire test when changing how a request is built.

## Conventions / gotchas

- **Env-touching tests must use `with_env` in `context.rs`.** It holds a
  process-wide `Mutex` so parallel tests can't observe a half-mutated
  environment. New tests that read or write env vars belong in that
  module or copy the same lock pattern.
- **Trait-first plumbing.** Don't add HTTP work directly into
  `action.rs`. Add a method to the `GithubClient` trait, implement it on
  both `OctocrabClient` and `GiteaClient`, and add a wire test on each
  side. The fake clients in `action.rs` and `tests/integration.rs` need
  matching impls for the test suite to compile.
- **URL building goes through `encode_path_segment`.** Hand-rolling
  `replace(' ', "%20")` is what the previous code did and it broke on
  `?`, `#`, `&`, `+`, `=`, `:`, and non-ASCII. The encoder handles every
  byte outside the RFC 3986 unreserved set.
- **Errors are propagated, not logged-and-swallowed.** Any failure in
  the merge / fast-forward step fails the action. Label removal is the
  one exception: failures there log a warning but still return success
  (parity with the original behaviour).
- **Outputs go through the `Logger` trait.** Don't `println!` from
  library code — write to the logger so tests can capture it. Workflow
  command bytes (`::warning::`, `::error::`, `%0A`/`%0D`/`%25` escapes)
  are pinned by tests in `logger.rs`.
- **Release profile is size-optimised** (`opt-level = "z"`, LTO, single
  codegen unit, strip). The runtime image is `alpine:3.23` with
  `ca-certificates` installed; binary is built on `rust:1-alpine` for
  matching musl ABI.

## Distribution

- Docker image: `ghcr.io/sudo-bot/action-pull-request-merge:latest`,
  also tagged with each release.
- Marketplace tag: `@v2` (moving — points at the latest 2.x). `Cargo.toml`
  is on `2.0.0` but no `v2.0.0` git tag exists; the marketplace
  convention is to keep `@v2` moving. Compare links in `CHANGELOG.md`
  use the moving `v2` tag for that reason.
- `make update-tags` re-points `v2` at `main` and force-pushes.

## When working on this

- For a behaviour change: edit `action.rs` (decision logic), update the
  fake client in tests, and check that integration + wire tests still
  pin the contract you intend.
- For a wire-shape change: edit one or both clients, **add a wire test**
  in `tests/wire.rs`, run `cargo test --test wire` first to iterate
  fast.
- For a new endpoint: extend the `GithubClient` trait, implement on
  both clients, write wire tests for both, then use it from `action.rs`.
- The Cargo edition is `2021`. Async runtime is tokio
  (`#[tokio::main]` in main, `#[tokio::test]` everywhere async is
  needed).

## Sister project: keeping in sync with action-pull-request-lock

This repo shares ~98% of its scaffolding with
[`sudo-bot/action-pull-request-lock`](https://github.com/sudo-bot/action-pull-request-lock):
the `GithubClient` trait pattern, env-based context, workflow-command
logger, fake-client integration tests, `WriteLogger<W>`, the
`is_gitea` detection rule, `pick_backend`, the `with_env` mutex, the
Dockerfile shape. **Their git histories are unified** (a single
`--allow-unrelated-histories` merge sits in the lock-action's log) so
shared scaffolding work can flow between repositories without manual
re-implementation.

### Setup

Add the sister repo as a remote:

```sh
git remote add lock-action git@github.com:sudo-bot/action-pull-request-lock.git
git fetch lock-action
```

### Files that should stay identical

Pure scaffolding with no domain content. If they drift, that drift is
almost always a bug:

- `src/logger.rs`

### Files that should track each other but allow domain divergence

The structure (function signatures, test patterns, error messages)
should match; the data inside differs:

- `src/context.rs` — same `is_gitea` detection, same `with_env` helper
  and `ENV_LOCK` mutex. Merge-action additionally has the `actor`
  field needed by its `allowed-usernames-regex` gate.
- `src/lib.rs` — both expose `Backend`, `pick_backend`, the same
  re-exports skeleton. Merge has more `inputs::*` re-exports
  (`MergeMethod`).
- `src/main.rs` — identical apart from the package name in `use`.
- `Cargo.toml` — same dep set apart from `regex` (only merge-action
  needs it) and package metadata. License is MPL-2.0 in both.
- `docker/Dockerfile`, `Makefile` — identical apart from the image
  name.

### Files that are intentionally divergent

Domain-specific. Don't try to keep these aligned beyond high-level
patterns:

- `src/action.rs`, `src/inputs.rs`,
  `src/github_client.rs`, `src/gitea_client.rs`,
  `tests/integration.rs`, `tests/wire.rs`.
- `action.yml`, `README.md`, `CHANGELOG.md`.

### Workflow

When you write a scaffolding change here:

1. Land it in this repo.
2. `cd ../action-pull-request-lock && git fetch <this-remote>`.
3. Cherry-pick the commit (`git cherry-pick <sha>`), or apply by hand
   if surrounding code has drifted.
4. Run that repo's `cargo test && cargo clippy && cargo fmt --check`.

When you find drift on a should-be-identical file, reconcile it
deliberately rather than letting each side mutate.

A useful diff:

```sh
diff -rq ../action-pull-request-lock/src ./src \
  | grep -v 'gitea_client\|github_client\|action\.rs\|inputs\.rs'
```

— anything else flagged is a candidate for sync.
