'use strict'

const core = require('@actions/core')
const { GitHub, context } = require('@actions/github')

const main = async () => {
    const token = core.getInput('github-token');
    const merge_title = core.getInput('merge-title');
    const merge_message = core.getInput('merge-message');
    const number = core.getInput('number');
    const allowed_usernames = core.getInput('allowed-usernames-regex');
    const filter_label = core.getInput('filter-label');
    const merge_method = core.getInput('merge-method');// merge|squash|rebase|fast-forward

    const octokit = new GitHub(token);

    if (!context.actor.match(allowed_usernames)) {
        core.warning('Ignored, the username does not match.');
        return;
    } else {
        core.info('Username matched.');
    }

    const pullRequest = await octokit.pulls.get({
        ...context.repo,
        ...context.owner,
        pull_number: number
    });

    if (pullRequest.data.state === 'closed') {
        core.warning('Ignored, the pull-request is closed.');
        return;
    } else {
        core.info('The pull-request is open.');
    }

    if (pullRequest.data.labels.indexOf(filter_label) !== -1) {
        core.warning('Ignored, the label does not exist on the pull-request.');
        return;
    } else {
        core.info('Label matched.');
    }

    if (merge_method === 'fast-forward') {
        core.info('Updating to: ' + pullRequest.data.base.ref + '@' + pullRequest.data.head.sha);
        await octokit.git.updateRef({
            force: false,
            ...context.repo,
            ...context.owner,
            ref: 'refs/heads/' + pullRequest.data.base.ref,
            sha: pullRequest.data.head.sha,
        });
    } else {
        /**
         * @type {Octokit.PullsMergeParamsDeprecatedNumber}
         */
        const mergeData = {
            merge_method: merge_method,
            ...context.repo,
            ...context.owner,
            pull_number: number,
            sha: pullRequest.data.head.sha,
        };
        if (merge_message.trim().length > 0) {
            mergeData.commit_message = merge_message;
        }
        if (merge_title.trim().length > 0) {
            mergeData.commit_title = merge_title;
        }
        await octokit.pulls.merge(mergeData)
    }
}

main().catch(err => core.setFailed(err.message))
