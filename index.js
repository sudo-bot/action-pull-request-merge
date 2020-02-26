'use strict'

const core = require('@actions/core')
const { GitHub, context } = require('@actions/github')

const main = async () => {
    const token = core.getInput('github-token', {
        required: true
    });
    const merge_title = core.getInput('merge-title', {
        required: false
    });
    const merge_message = core.getInput('merge-message', {
        required: false
    });
    const number = core.getInput('number', {
        required: true
    });
    const allowed_usernames = core.getInput('allowed-usernames-regex', {
        required: false
    });
    const filter_label = core.getInput('filter-label', {
        required: false
    });
    const merge_method = core.getInput('merge-method', {
        required: false
    });// merge|squash|rebase|fast-forward

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

    if (filter_label.length > 0) {
        if (pullRequest.data.labels.indexOf(filter_label) !== -1) {
            core.warning('Ignored, the label does not exist on the pull-request.');
            return;
        } else {
            core.info('Label matched.');
        }
    } else {
        core.info('Label check is disabled.');
    }

    if (merge_method === 'fast-forward') {
        core.info('Updating to: ' + 'heads/' + pullRequest.data.base.ref + '@' + pullRequest.data.head.sha);
        await octokit.git.updateRef({
            force: false,
            ...context.repo,
            ...context.owner,
            ref: 'heads/' + pullRequest.data.base.ref,
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
    if (filter_label.length > 0) {
        try {
            await octokit.issues.removeLabel({
                ...context.repo,
                ...context.owner,
                issue_number: number,
                name: filter_label
            });
        } catch (error) {
            core.warning(error.message || 'Removing the label could have failed.');
        }
    }
}

main().catch(err => core.setFailed(err.message))
