# Disk Cleanup Live Hotfix Restoration Plan

## Goal

Restore the live worker's disk-cleanup safety behavior in source before the
next package release, so a normal npm deployment cannot replace the current
manual hotfix with less safe cleanup code.

## Current state

Production is running the worker from
`npm-0.1.26-hotfix-process-guard-20260804T1247Z`. Compared with the published
`@agent-session-broker/worker@0.1.26`, that release changes exactly one compiled
runtime file: `disk-pressure-cleanup-service.js`.

The live-only behavior has two parts:

- cleanup candidate, deletion, and failure detail logs are sampled to five
  entries per category, followed by aggregate counts and byte totals;
- before destructive session or cache cleanup, the worker inspects running
  process commands once and protects session and job roots that a process still
  references. If process inspection fails, session and cache deletion fails
  closed.

Neither behavior exists in the source on `main`, in a reachable remote branch,
or in a prior pull request. PR #75 does not overlap the cleanup implementation
or tests.

## Proposed changes

1. Add regression tests for the live safety contract before changing the
   implementation:
   - a referenced session root protects inactive-session deletion;
   - a referenced job root protects both session and cache cleanup;
   - failed process inspection skips destructive session/cache cleanup;
   - dry runs do not need process inspection;
   - cleanup detail logs stop at five samples while aggregate counts and bytes
     describe the full operation.
2. Remove the current unguarded calls that delete inactive sessions and expired
   caches without process evidence.
3. Add an injectable process-command provider whose production default runs
   `/bin/ps -axo command=`. Read it once per non-dry-run cleanup pass, pass the
   result into session/cache candidate filtering, and fail closed when it
   cannot be read.
4. Bound per-item cleanup logging and emit aggregate summaries. Reuse file
   sizes gathered during log discovery instead of restatting every old log.
5. Keep configuration, state schema, admin APIs, and cleanup thresholds
   unchanged. Deliver this restoration independently from quota display and
   release-version changes.

## If we do not change it

Publishing and deploying a clean package from current source would overwrite
the production-only hotfix. A cleanup pass could delete a session or cache
directory that a still-running process references, and a large cleanup could
again emit unbounded per-item logs.

## Expected result

Source builds reproduce the live worker's safety behavior. Destructive cleanup
continues for eligible, unreferenced stale data, but skips referenced roots and
fails closed when running processes cannot be inspected. Large cleanup passes
retain bounded diagnostic samples plus complete aggregate telemetry.

## Acceptance criteria

- New focused tests fail on the current unguarded implementation and pass after
  the restoration.
- Process commands are read once for a non-dry-run cleanup pass and not read for
  a dry run.
- A command referencing the session root or one of its background-job roots
  prevents that session's cache and inactive-session deletion.
- A process inspection error logs the fail-closed reason and deletes no session
  cache or inactive session; old-log cleanup remains independent.
- Candidate, deletion, and failure detail logs are capped at five per cleanup
  category, with aggregate counts and byte totals for the full operation.
- Targeted tests, full tests, formatting, lint, build, and release packaging
  pass.
- A manual test using isolated temporary roots and an injected process-command
  provider confirms referenced data remains and unreferenced data is removed.
- The implementation is delivered as a separate draft pull request. No npm
  release or production deployment is triggered by this change.
