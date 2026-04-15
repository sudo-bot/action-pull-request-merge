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
| `merge-method`            |    no    | `merge`   | One of `merge`, `squash`, `rebase`, `fast-forward`.                                                   |
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
