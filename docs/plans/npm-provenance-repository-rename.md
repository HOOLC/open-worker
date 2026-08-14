# NPM Provenance Repository Rename

## Goal

Publish the admin and worker packages with provenance metadata that identifies
the canonical `HOOLC/open-worker` GitHub repository.

## Current State

The repository was renamed from `HOOLC/slack-codex-broker` to
`HOOLC/open-worker`, but the root and publishable package manifests still point
at the old repository. The `v0.1.27` workflow built, tested, staged, and packed
both packages, then npm rejected the admin publish with `E422` because the
package repository did not match the GitHub provenance source. The worker
publish was skipped, so neither `0.1.27` package exists in npm.

The old repository URL also remains in the isolated MCP OAuth client metadata.

## Proposed Changes

1. Add an end-to-end repository identity contract that compares the root,
   admin, worker, and OAuth client metadata with GitHub's canonical repository
   identity.
2. Replace the stale repository, homepage, issue, and OAuth client URLs with
   `HOOLC/open-worker`.
3. Bump the root, admin, and worker versions to `0.1.28`; keep the failed
   `v0.1.27` tag immutable.
4. Build, test, stage, and pack both packages, then inspect the staged and packed
   manifests before creating the release pull request.

## If We Do Not Change It

Every provenance-enabled publish from the current manifests will fail after the
build, leaving production unable to install a release newer than `0.1.26`.

## After the Change

Pull-request CI validates the same repository identity npm provenance uses.
The next versioned tag can publish both `0.1.28` packages from the canonical
repository without moving or reusing `v0.1.27`.

## Acceptance Criteria

- The root, admin, and worker manifests use the canonical repository, homepage,
  and issue URLs.
- The isolated MCP OAuth metadata uses the canonical repository URL.
- Root, admin, and worker versions are all `0.1.28`.
- The repository identity end-to-end test fails for the old URL and passes for
  `HOOLC/open-worker`.
- Formatting, lint, build, focused tests, and the full test suite pass.
- Staged and packed admin/worker manifests contain version `0.1.28` and the
  canonical repository URL.
- The release pull request passes exact-head CI before merge and tagging.
- A new immutable tag points directly to the merge commit, and npm serves both
  `@agent-session-broker/admin@0.1.28` and
  `@agent-session-broker/worker@0.1.28`.
