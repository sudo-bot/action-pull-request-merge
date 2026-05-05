# merge a pull-request

GitHub Action that merges a pull-request when an event triggers the workflow.
Written in Rust, runs as a Docker container action — no Node.js runtime
needed on the runner.

Marketplace: https://github.com/marketplace/actions/pull-request-merge

## Inputs

| Name                      | Required | Default   | Description                                                                                           |
| ------------------------- | :------: | --------- | ----------------------------------------------------------------------------------------------------- |
| `github-token`            |    yes   | —         | GitHub token used to call the REST API. Usually `${{ secrets.GITHUB_TOKEN }}`.                        |
| `number`                  |    yes   | —         | Pull-request number to merge.                                                                         |
| `merge-method`            |    no    | `merge`   | One of `merge`, `squash`, `rebase`, `fast-forward`, `fast-forward_or_merge`.                          |
| `allowed-usernames-regex` |    no    | `^.*$`    | Regex the triggering actor (`github.actor`) must match. Skips the merge otherwise.                    |
| `filter-label`            |    no    | *(empty)* | Regex matched against PR labels. When set, the merge is skipped unless a label matches, and the first matching label is removed after a successful merge. |
| `merge-title`             |    no    | *(empty)* | Commit title used by the merge/squash/rebase API. Ignored for `fast-forward`.                         |
| `merge-message`           |    no    | *(empty)* | Commit body used by the merge/squash/rebase API. Ignored for `fast-forward`.                          |

### `merge-method` notes

- `merge` / `squash` / `rebase` — call `PUT /repos/{owner}/{repo}/pulls/{n}/merge`.
- `fast-forward` — call `PATCH /repos/{owner}/{repo}/git/refs/heads/{base}` to
  move the base branch to the PR's head SHA. This is a *true* fast-forward: the
  base ref must already be an ancestor of the head, otherwise GitHub refuses
  the update. No merge commit is created.
- `fast-forward_or_merge` — attempt a fast-forward first; if the base branch is
  not an ancestor of the head (i.e. the fast-forward fails), fall back to a
  regular `merge`.

## Required permissions

The workflow's `GITHUB_TOKEN` needs write access to both pull requests and
repository contents. Declare this explicitly at the top of your workflow:

```yml
permissions:
    contents: write        # merge / fast-forward the base branch
    pull-requests: write   # perform the merge and remove the filter label
```

## Examples

### Merge a PR that has a specific label

```yml
name: auto-merge
on:
    pull_request:
        types: [labeled]

permissions:
    contents: write
    pull-requests: write

jobs:
    merge:
        runs-on: ubuntu-latest
        steps:
            - uses: sudo-bot/action-pull-request-merge@v2
              with:
                  github-token: ${{ secrets.GITHUB_TOKEN }}
                  number: ${{ github.event.pull_request.number }}
                  filter-label: merge-it
                  allowed-usernames-regex: ^williamdes$
```

### Squash-merge with a custom commit title and message

```yml
- uses: sudo-bot/action-pull-request-merge@v2
  with:
      github-token: ${{ secrets.GITHUB_TOKEN }}
      number: ${{ github.event.pull_request.number }}
      merge-method: squash
      merge-title: "${{ github.event.pull_request.title }} (#${{ github.event.pull_request.number }})"
      merge-message: ${{ github.event.pull_request.body }}
```

### Fast-forward the base branch to the PR's head

```yml
- uses: sudo-bot/action-pull-request-merge@v2
  with:
      github-token: ${{ secrets.GITHUB_TOKEN }}
      number: ${{ github.event.pull_request.number }}
      merge-method: fast-forward
      filter-label: merge-it
```

### Allow multiple maintainers via a regex

```yml
- uses: sudo-bot/action-pull-request-merge@v2
  with:
      github-token: ${{ secrets.GITHUB_TOKEN }}
      number: ${{ github.event.pull_request.number }}
      allowed-usernames-regex: ^(williamdes|alice|bob)$
      filter-label: ^(merge-it|ship-it)$
```

## Behaviour

The action performs these checks, in order, and logs what it's doing using
standard `::warning::` / `::error::` workflow commands:

1. If `github.actor` does not match `allowed-usernames-regex`, the step emits
   a warning and exits successfully (the workflow is not failed).
2. The PR is fetched. If its state is `closed`, the step warns and exits.
3. If `filter-label` is set and no label on the PR matches, the step warns
   and exits.
4. The merge (or fast-forward) is performed.
5. If `filter-label` was set, the matching label is removed. A failure here
   produces a warning but does not fail the step.

Any network or API failure during step 4 *does* fail the step.

## Gitea (self-hosted) support

The action also runs on [Gitea Actions](https://docs.gitea.com/usage/actions/overview).
Detection is automatic: when the runner sets `GITEA_ACTIONS=true` (or
`GITHUB_API_URL` ends in `/api/v1`), the action talks to Gitea's REST API
instead of GitHub's. From the workflow author's perspective, the inputs and
the step usage are identical.

Under the hood, three Gitea-specific differences are handled for you:

- **Merge** — Gitea uses `POST /repos/{o}/{r}/pulls/{n}/merge` (not `PUT`)
  with the `Do` / `MergeTitleField` / `MergeMessageField` / `head_commit_id`
  body shape.
- **Label removal** — Gitea's `DELETE` endpoint requires the numeric label
  *id*, so the action looks up the issue's labels first and resolves the
  configured name to its id.
- **Fast-forward** — `PATCH /repos/{o}/{r}/git/refs/{ref}` is wire-compatible
  with GitHub. Gitea ≥ 1.20 is required.

### Writing a workflow that runs on both GitHub and Gitea

The action itself is portable, but workflow *triggers* are not always
identical between the two forges. Two gotchas to know about:

1. **`pull_request` event payload** — On GitHub the `labeled` /
   `unlabeled` action fills in `github.event.label.name` at the top level;
   on Gitea the comparable event is `pull_request_label` (with action
   `label_updated` / `label_cleared`) and the label is *only* present
   inside `github.event.pull_request.labels[]`. A gate that reads
   `github.event.label.name` therefore evaluates to `null` on Gitea and
   the job is silently skipped.

   **Portable form:** test against the labels array, which is populated
   on both forges:

    ```yml
    if: contains(github.event.pull_request.labels.*.name, '/merge')
    ```

   Trade-off: the job re-runs harmlessly when *any* label changes while
   the gating label is still attached (the action's idempotency checks
   skip closed PRs and missing labels, so this is not a correctness
   issue — just an extra evaluation).

2. **Container image registry** — Gitea runners must be able to pull
   `ghcr.io/sudo-bot/action-pull-request-merge:latest`. If your runner
   only has access to a private registry, pre-pull or mirror the image
   there.

### Gitea-compatible example

```yml
name: auto-merge
on:
    pull_request:
        types: [labeled, opened, synchronize]    # GitHub
    pull_request_label:                          # Gitea
        types: [label_updated]

permissions:
    contents: write
    pull-requests: write

jobs:
    merge:
        runs-on: ubuntu-latest
        # Works on both forges: read the labels array, never the top-level
        # `event.label.name`.
        if: contains(github.event.pull_request.labels.*.name, 'merge-it')
        steps:
            - uses: sudo-bot/action-pull-request-merge@v2
              with:
                  github-token: ${{ secrets.GITHUB_TOKEN }}
                  number: ${{ github.event.pull_request.number }}
                  filter-label: merge-it
                  allowed-usernames-regex: ^williamdes$
```
