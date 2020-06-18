# Webhooks

- `pull_request`
- `issue_comment` `pull_request_review`: command, approval
- `status`: status
- `check_suite`, `check_run`: status (if necessary)
- `push`: cancel merge if obsoleted

# Determining approval

- Require target branch to be listed in the config file
- Limitation: Can't tell whether reviews were made for the correct target branch
- [List all reviews](https://developer.github.com/v3/pulls/reviews/#list-reviews-on-a-pull-request)
- Filter only reviews made for this commit #
- Take latest review by each author
- If there are any changes requested, review is never considered approved
- Else, count approvals

# Constructing merges

Constructing merges depends on the strategy used.

- [Force-reset / fast-forward](https://developer.github.com/v3/git/refs/#update-a-reference)
- [Create a branch](https://developer.github.com/v3/git/refs/#create-a-reference)
- [Perform a merge](https://developer.github.com/v3/repos/merging/#perform-a-merge)
- [Create a commit](https://developer.github.com/v3/git/commits/#create-a-commit)

## Merge

- Create a new tmp branch from master
- For each PR:
  - Merge each PR into tmp
    - On merge conflict: Skip this PR, report merge conflict
- Force-reset staging to tmp
- Delete tmp

## Octopus Merge

- Create a new tmp branch from master
- For each PR:
  - Merge each PR into tmp
    - On merge conflict: Skip this PR, report merge conflict
- Create a new commit C with tree tmp, parents = PRs
- Force-reset staging to C
- Delete tmp

## Squash

- Create a new tmp branch from master
- Set S = tmp
- For each PR:
  - Merge each PR into tmp
    - On merge conflict: Skip this PR, report merge conflict
  - Create a new commit from tmp with parent S
  - Force-reset tmp to S
- Force-reset staging to tmp
- Delete tmp

## Batch squash

- Create a new tmp branch from master
- Set S = tmp
- For each PR:
  - Merge each PR into tmp
    - On merge conflict: Skip this PR, report merge conflict
- Create a new commit C with tree tmp, parent = S
- Force-reset staging to C
- Delete tmp

## Cherry-pick

- Use https://developer.github.com/v3/repos/commits/#compare-two-commits
  (with 3 dots - note that the `A..B` and `A...B` are different for `git diff` than everything else) to list all changed commits.
  Collect these for all PRs in the batch.
- Sort each into topological order by ancestry.  Call the feature commits A, B, C ...
  - If there are any merge commits among them, report an error and skip this PR
- Create a new tmp branch from master
- Set S = tmp
- For each PR:
  - Repeatedly merge tmp with A, B, C, ... to form A', B', C', ...
    Record the trees A', B', ... along the way
    - If there are any merge conflicts:
      Force-reset tmp to S and skip this PR, report merge conflict
  - Create commits A'' = A' with parent S, B'' = B' with parent A'', ...
  - Set S = Z''
  - Force-reset tmp to S
- Force-reset staging to tmp
- Delete tmp

# Database schema

## `pull_request`

- `owner`: string: repo owner
- `repo`: string: repo name
- `number`: int: PR number
- `commit_hash`: string: current commit hash
- `state`: string (REQUESTED, QUEUED, MERGING, SPLIT)
- `merge_attempt`?: string (possibly foreign key to `merge_attempt.id`)
- `timestamp`: int (epoch seconds): time of last state change
- (todo) priority?

indices:
- `owner, repo, number` (unique)
- `merge_attempt`
- `state`, `timestamp`

## `merge_attempt`

- `id`: string
- `owner`: string: repo owner
- `repo`: string: repo name
- `state`: string (CONSTRUCTING, TESTING, SUCCESS, SPLIT)
- `timestamp`: int (epoch seconds): time of last state change

indices:
- `id` (unique)
- `owner`, `repo`, `state`
- `state`, `timestamp`

# Merging flow

## Request
Triggers:
- Receive merge command

Actions:
- Ensure [repo, PR number] state == NONE (else report error)
- If PR is closed, report error
- If ready (non-draft, approved at commit, pre-status at commit):
  - Set state = QUEUED, commit #, timestamp; report OK
  - Trigger Construct
- Else
  - Set state = REQUESTED, commit #, timestamp; report waiting

## Initiate
Triggers:
- PR approved
- PR pre-status passed

Actions:
- If state != REQUESTED: return
- If commit # is out of date: delete PR state, report out of date, return
- If ready (non-draft, approved at commit #, pre-status at commit #):
  - Set state = QUEUED, timestamp
  - Trigger Construct

## Construct
Actions:
- If there are any merge attempts in the repo not in the SPLIT state, do nothing
- If there is any merge attempt in the repo in SPLIT state, construct that merge attempt (unless it has no PRs, in which case delete it and try again)
- Create/set merge attempt state = CONSTRUCTING, repo, staging branch name, timestamp
- Find all PRs in repo with QUEUED state
- Group by priority, take highest priority group
- If none are older than 10 minutes, return (wait for more to arrive)
- Record for each PR: state = MERGING, reference to merge attempt, timestamp
- Construct merged version
  - If there are conflicting PRs:
    - If there was just 1 PR to start with: report error, delete merge attempt, delete PR state, exit
    - Create new merge attempt with state = SPLIT
    - Set all conflicting PRs to state = SPLIT, reference to new merge attempt, timestamp
    - Send message
- Check merge attempt state = CONSTRUCTING, else exit
- Set merge attempt state = TESTING

## Test
Triggers:
- Staging branch status
 
Actions:
- If any checks failed:
  - Check corresponding merge attempt state = TESTING, else exit
  - Delete merge attempt state
  - If there is only 1 PR: delete PR state, report test failure, exit
  - Create two merge attempts with state = SPLIT
  - Partition PRs into two sets, set reference to new merge attempts
  - Set PR state = SPLIT, timestamp
- Determine if all checks succeeded, else exit
- Check corresponding merge attempt state = TESTING, else exit
- Set merge attempt state = SUCCESS
- Trigger Complete

## Complete
Actions:
- Fast-forward master to staging
  - On conflict report error, reset PR states to QUEUED, delete merge attempt state
- Delete merge attempt state, PR states
- Report success
- Trigger Construct

## Cancel
Triggers:
- Commit push
- Command

Actions:
- Look up cancelled PR's state
  - NONE: exit, report not found if from command
  - REQUESTED: Delete PR state
  - MERGING: Delete PR state, set merge attempt state to SPLIT, set all other PRs in merge attempt to SPLIT.
- Report cancellation

## Poll
Durations:
- poll timer: 10 minutes
- REQUESTED timeout (pre-status): 1 hour
- QUEUED timeout: 24 hours
- MERGING timeout: 24 hours
- SPLIT timeout: 24 hours
- CONSTRUCTING timeout: 15 minutes
- TESTING timeout (status): 1 hour
- SUCCESS timeout: 15 minutes

Triggers:
- Timer

Actions:
- Check all PR states:
  - REQUESTED, timestamp too old:
    - Delete PR state, report timeout
  - REQUESTED: Trigger Initiate
  - QUEUED, timestamp too old:
    - For each PR linked to same merge attempt:
      - Delete PR state, report timeout
    - Delete merge attempt
  - QUEUED: Trigger Construct
  - MERGING, timestamp too old:
    - For each PR linked to same merge attempt:
      - Delete PR state, report timeout
    - Delete merge attempt
  - SPLIT, timestamp too old:
    - For each PR linked to same merge attempt:
      - Delete PR state, report timeout
    - Delete merge attempt
- Check all merge attempt states:
  - CONSTRUCTING, timestamp too old:
    - For each PR linked to merge attempt:
      - Delete PR state, report timeout
    - Delete merge attempt
  - TESTING, timestamp too old:
    - For each PR linked to merge attempt:
      - Delete PR state, report timeout
    - Delete merge attempt
  - TESTING: Trigger Test
  - SUCCESS: Trigger Complete
  - SPLIT: Trigger Construct
