# NPM Runtime Dependency Cold Start

## Goal

Publish admin and worker npm packages that can be installed and started from
their tarballs without relying on the private build workspace's
`node_modules`.

## Current State

The private root manifest declares the full runtime dependency set, but the
publishable admin and worker manifests do not. The `0.1.28` build, test, stage,
pack, and publish jobs all ran with the root dependencies installed, so they
did not exercise the dependency boundary of either public package.

Production installed the admin package successfully and switched its symlink,
then the restarted process failed during module loading with
`ERR_MODULE_NOT_FOUND: @larksuiteoapi/node-sdk`. The deployment request had
already returned success, so the operation record did not reflect the later
cold-start failure. Admin was rolled back to `0.1.26`; worker remained on its
existing `0.1.26` hotfix.

The live account-pool API still returns a weekly quota window, but the deployed
`0.1.26` UI reads that response shape incorrectly. The quota fix is already in
the current source tree and will become visible when a safe package release can
be deployed.

## Proposed Changes

1. Add an end-to-end release test that stages and packs both packages, installs
   the tarballs into clean temporary projects, runs the installed admin CLI,
   starts each real package entry point, and checks its health/readiness
   endpoint.
2. Keep the publishable admin and worker dependency declarations aligned with
   every external runtime module reachable from their packaged server code,
   and stage the complete relative-import closure of the admin CLI.
3. Bump the root, admin, and worker versions to `0.1.29`; keep the failed
   `0.1.28` tag and package versions immutable.
4. Run formatting, lint, build, focused tests, the full test suite, staging,
   packing, clean installation, and cold-start verification before opening a
   draft pull request.

## If We Do Not Change It

The registry can continue accepting packages that install successfully but
cannot start. Production deployment will fail after the symlink switch, the
operation log can misleadingly show success, and the already-implemented
account-pool quota fix will remain unavailable on the live admin UI.

## After the Change

Pull-request CI verifies the same clean package boundary used by production.
Both `0.1.29` tarballs must resolve their own runtime imports and boot from a
fresh install before they can be published. A human operator can then deploy
the new release and verify the account-pool card without retrying `0.1.28`.

## Acceptance Criteria

- Root, admin, and worker manifests all use version `0.1.29`.
- The admin and worker manifests declare every external dependency required by
  their packaged server entry points.
- A regression test fails against the `0.1.28` manifest boundary.
- The test packs and installs both artifacts without linking the repository or
  inheriting its `node_modules`.
- The installed `agent-session-broker-macos-bootstrap --help` command exits
  successfully using only files from the admin tarball.
- The installed admin entry point cold-starts and returns HTTP 200 from
  `/healthz`.
- The installed worker entry point cold-starts and returns HTTP 200 from both
  `/healthz` and `/readyz` while connected to an isolated mock Slack endpoint.
- Formatting, lint, build, focused tests, and the full test suite pass.
- Staged and packed manifests contain version `0.1.29` and the complete runtime
  dependency set.
- The release pull request is opened as a draft and passes exact-head CI before
  any merge or tag is considered.
- No production deployment is performed by this change.
