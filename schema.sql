-- temporary solution until an actual migration system is in place
-- run using `sqlite3 -bail -batch filename.db <schema.sql`

BEGIN;


CREATE TABLE IF NOT EXISTS pull_request (
  owner TEXT NOT NULL,
  repo TEXT NOT NULL,
  number INTEGER NOT NULL,
  commit_hash TEXT NOT NULL,
  state TEXT NOT NULL,
  merge_attempt TEXT,
  timestamp INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS pull_request_owner_repo_number
ON pull_request (owner, repo, number);

CREATE INDEX IF NOT EXISTS pull_request_merge_attempt
ON pull_request (merge_attempt);

CREATE INDEX IF NOT EXISTS pull_request_state_timestamp
ON pull_request (state, timestamp);


CREATE TABLE IF NOT EXISTS merge_attempt (
  id TEXT NOT NULL,
  owner TEXT NOT NULL,
  repo TEXT NOT NULL,
  state TEXT NOT NULL,
  timestamp INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS merge_attempt_id
ON merge_attempt (id);

CREATE INDEX IF NOT EXISTS merge_attempt_owner_repo_state
ON merge_attempt (owner, repo, state);

CREATE INDEX IF NOT EXISTS merge_attempt_state_timestamp
ON merge_attempt (state, timestamp);


COMMIT;
