# Repository content

The repository contains product source, automated tests, fixed fixtures, dependency
lockfiles, shared architecture contracts and the inputs needed to build each app.
Keep required fonts, icons, model/shader inputs and their original provenance and
licenses alongside the code. Generated files that serve as intentional comparison
baselines are retained with their source metadata.

Local credentials, databases, device state, personal deployment helpers, build
caches and raw test output do not belong in Git. Keep them in ignored local
directories. Personal environment skills use locally excluded groups; new clones
configure their own hosts and storage without editing shared instructions.

The design workspace keeps the reference prototype used by the handbook and
selected component comparison baselines. Other archived explorations, migration
records and machine-specific reports remain local. Preserve references and
reproducible preparation steps before excluding a generated or archived asset.

GPU lexer benchmarks are an independent experiment, outside the production Cargo
workspace. Their source, pinned input archive, shaders, weights and fixture hashes
are tracked. Run logs, process snapshots and reports are generated locally.

## Optional CI cache

CI can use an R2-backed kache cache when repository variables `KACHE_S3_ENDPOINT`
and optionally `KACHE_S3_BUCKET` are set, together with the existing
`KACHE_S3_ACCESS_KEY` and `KACHE_S3_SECRET_KEY` secrets. Without this configuration,
the workflow performs an ordinary build. Do not put account-specific endpoints or
secret values in workflow source. Container builds accept the same explicit
`KACHE_S3_*` environment settings.

## Publication checks

Inspect the exact candidate tree, including untracked files, before publishing.
Use a clean checkout with frozen dependencies to verify that build inputs are
complete. Scan the candidate for secrets and check shared source metadata for
private paths. Review the selected Git refs as well as the final file tree: a
new deletion commit does not remove earlier content from its ancestors.

Native node archives belong in GitHub Releases; desktop app distribution has a
separate packaging and signing flow. Source publication does not imply that
installable releases or platform validation have been completed.
