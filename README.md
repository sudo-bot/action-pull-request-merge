# merge a pull-request

This action merges a pull-request

Marketplace: https://github.com/marketplace/actions/pull-request-merge

## Example usages

```yml
steps:
  - name: merge a pull request
    uses: sudo-bot/action-pull-request-merge@v1.2.0
    with:
        github-token: ${{ secrets.GITHUB_TOKEN }}
        number: ${{ github.event.pull_request.number }}
        allowed-usernames-regex: ^williamdes$
        filter-label: merge-it

  - name: merge a pull request without any need of a label (automatic merge)
    uses: sudo-bot/action-pull-request-merge@v1.2.0
    with:
        github-token: ${{ secrets.GITHUB_TOKEN }}
        number: ${{ github.event.pull_request.number }}
        allowed-usernames-regex: ^williamdes$

  - name: merge a pull request with message and body (optional)
    uses: sudo-bot/action-pull-request-merge@v1.2.0
    with:
        github-token: ${{ secrets.GITHUB_TOKEN }}
        number: ${{ github.event.pull_request.number }}
        merge-method: merge
        allowed-usernames-regex: ^williamdes$
        filter-label: merge-it
        merge-title: "Merge #${{ github.event.pull_request.number }}"
        merge-message: "Merge #${{ github.event.pull_request.number }}"

  - name: merge a pull request using fast-forward
    uses: sudo-bot/action-pull-request-merge@v1.2.0
    with:
        github-token: ${{ secrets.GITHUB_TOKEN }}
        number: ${{ github.event.pull_request.number }}
        merge-method: fast-forward
        allowed-usernames-regex: ^williamdes$
        filter-label: merge-it
```
