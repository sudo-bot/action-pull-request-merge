# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed
- `Cargo.toml`'s `license` field said `MIT` while the actual `LICENSE`
  file in the repository (and the rest of the `sudo-bot` actions
  family) is the Mozilla Public License 2.0. Corrected the metadata
  to match: `license = "MPL-2.0"`. No code change.

### Added
- Gitea self-hosted support. The action auto-detects Gitea Actions (via
  the `GITEA_ACTIONS=true` env var the runner sets, or a `/api/v1` suffix
  on `GITHUB_API_URL`) and routes API calls through a Gitea-aware client
  that handles the three places Gitea diverges from GitHub: `POST` (not
  `PUT`) on `/pulls/{n}/merge` with the CamelCase
  `Do`/`MergeTitleField`/`MergeMessageField`/`head_commit_id` body, label
  removal by numeric id (with a name→id lookup), and an empty-body merge
  response. No new runtime dependency. The existing GitHub path is
  unchanged.
- README section documenting Gitea-compatible workflow gates — using
  `contains(github.event.pull_request.labels.*.name, '...')` instead of
  `github.event.label.name`, which Gitea does not populate.

### Fixed
- `remove_label` now percent-encodes the full label name. The previous
  encoder only handled `%`, space, and `/`, so labels containing `?`,
  `#`, `&`, `+`, `=`, `:`, or any non-ASCII byte (e.g. `café`, emoji)
  produced malformed URLs that GitHub would reject. Replaced with an
  RFC 3986 path-segment encoder that keeps the unreserved set and
  percent-encodes every other byte.

### Changed
- `OctocrabClient::get_pull` now deserialises responses straight into the
  action's minimal `PullRequest` projection instead of going through
  octocrab's typed `pulls().get()` API and serde-cycling the result.
  Same wire request, more resilient to forks that omit optional GitHub
  fields.

### Internal
- Test count grew from 19 to 74. New coverage: `wiremock` integration
  tests pin HTTP method, URL, headers and body for every endpoint on
  both clients; the `is_gitea` detection rule extracted to a unit-tested
  `pick_backend()`; `StdoutLogger` byte-level verification via a
  `WriteLogger<W: Write>` parameterisation; error-propagation paths,
  multi-label match semantics, and Gitea label-id resolution edge cases
  all now have direct tests.

## [v2.0.0] - 2026-04-20

### Changed
- **Action rewritten in Rust.** The action is now a single Rust binary
  shipped as a Docker container action and no longer requires a Node.js
  runtime on the runner. API calls go through
  [`octocrab`](https://crates.io/crates/octocrab). The user-facing
  inputs surface (`github-token`, `number`, `merge-method`,
  `allowed-usernames-regex`, `filter-label`, `merge-title`,
  `merge-message`) is unchanged.
- Distribution moved from Docker Hub to GitHub Container Registry
  (`ghcr.io/sudo-bot/action-pull-request-merge`). Published on the
  `latest` tag in addition to versioned tags.
- Build pipeline reworked: multi-stage Dockerfile, Debian build stage,
  Alpine runtime, Docker image smoke-tested in CI before publish.
- Workflows declare least-privilege `permissions` blocks; CI actions
  bumped to current versions.
- README rewritten with an inputs table, required permissions block,
  and worked examples for each merge method.

### Added
- `fast-forward` merge method. Calls
  `PATCH /repos/{o}/{r}/git/refs/heads/{base}` to advance the base
  branch to the PR's head SHA — a true fast-forward with no merge
  commit. Fails if the base is not an ancestor of the head.
- `fast-forward_or_merge` merge method. Attempts a fast-forward first
  and falls back to a regular merge if the fast-forward isn't possible.
- `filter-label` is interpreted as a regex (matched against each PR
  label's name). After a successful merge the *literal* matched label
  is removed, not the regex pattern.

### Fixed
- Label removal correctly removes the matched label even when
  `filter-label` is a regex (previously the regex string was sent to
  the delete endpoint).

## [v1.2.0] - 2022-07-10

- Upgrade @actions/core from 1.2.6 to 1.9.0
- Upgrade @actions/github from 3.0.0 to 5.0.3
- Bump node-fetch from 2.6.0 to 2.6.7
- Upgrade from node 12 to node 16

## [v1.1.1] - 2020-06-03

- Fix label matching rules
- Upgrade @actions/core from 1.2.3 to 1.2.4
- Upgrade @actions/github from 2.1.1 to 3.0.0
- Migrate the code after @actions/github upgrade

## [v1.1.0] - 2020-02-26 [DEPRECATED]

- Make filter-label optional ([#5](https://github.com/sudo-bot/action-pull-request-merge/issues/5))
- Upgrade dependencies
- Update examples in README.md
- Fix error when label is already removed

## [v1.0.3] - 2019-12-09 [DEPRECATED]

- First working version

## [v1.0.2] - 2019-12-09 [DEPRECATED]

- Some fixes

## [v1.0.1] - 2019-12-09 [DEPRECATED]

- Some fixes

## [v1.0.0] - 2019-12-09 [DEPRECATED]

- First stable version

[Unreleased]: https://github.com/sudo-bot/action-pull-request-merge/compare/v2...HEAD
[v2.0.0]: https://github.com/sudo-bot/action-pull-request-merge/compare/v1.2.0...v2
[v1.2.0]: https://github.com/sudo-bot/action-pull-request-merge/compare/v1.1.1...v1.2.0
[v1.1.1]: https://github.com/sudo-bot/action-pull-request-merge/compare/v1.1.0...v1.1.1
[v1.1.0]: https://github.com/sudo-bot/action-pull-request-merge/compare/v1.0.3...v1.1.0
[v1.0.3]: https://github.com/sudo-bot/action-pull-request-merge/compare/v1.0.2...v1.0.3
[v1.0.2]: https://github.com/sudo-bot/action-pull-request-merge/compare/v1.0.1...v1.0.2
[v1.0.1]: https://github.com/sudo-bot/action-pull-request-merge/compare/v1.0.0...v1.0.1
[v1.0.0]: https://github.com/sudo-bot/action-pull-request-merge/releases/tag/v1.0.0
