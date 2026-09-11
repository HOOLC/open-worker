# Station naming and compatibility

The canonical repository is `HOOLC/zork`. The node service is **Station**.
The former `zork-gateway` executable is now `zork-station`.

| Surface | Current name |
| --- | --- |
| GitHub repository | `https://github.com/HOOLC/zork` |
| Cargo package and executable | `zork-station` |
| Source directory | `crates/station` |
| Development command | `pnpm dev:station` |
| Release-mode command | `pnpm start:station` |
| Optional npm launcher | `bin/zork-station.mjs` |
| macOS helper bundle | `ZorkStation.app` |
| Complete native node | `zork`, `zork-station`, `zork-agent`, `zork-gh` |

`zork` remains the supervisor and installation CLI. It starts one Station
process with the Agent runtime embedded. A component rename does not introduce
another runtime process or change which component owns business state.

## Existing state and clients

The data directory, database files, session IDs, provider profiles, mesh identity,
service registration and supervisor socket retain their existing formats.

Some technical identifiers retain `gateway` deliberately:

- The readiness marker remains `run/zork-gateway.pid`. `zork-config` maps
  `zork-station` to that file so an existing client can observe a new Station and
  a new client can observe an existing node. Readiness remains tied to its owning PID.
- Existing serialized fields such as `bind.gateway`, `urls.gateway` and the
  `gateway` section of node information keep their wire format. The ingress
  listener still acts as the gateway for external IM adapters.
- The macOS Station helper keeps its bundle identifier
  `surf.zork.desktop.gateway` while using the `ZorkStation.app` path and
  `Zork-Station` display name. The installed helper keeps its existing identity.
- Mesh enrollment kinds and authorization roles keep their existing values.
  Renaming a service must not silently change membership or privileges.
- Internal compatibility types such as `GatewayClient` continue to represent
  the existing node API; changing Rust names is separate from changing its protocol.
- Fixed GPU benchmark corpora retain their original bytes and hashes, including
  old repository and executable strings used as lexer input. They are test data,
  not installation instructions or runtime configuration.

The macOS installer recognizes both old and new process names while preserving
saved local-node intent. New bundles and release archives contain the Station
executable. Rebuild or replace the complete bundle; copying only one newly named
binary into an old installation is not a supported update.

The rename does not install or restart existing devices. It does not create a
native Release. Older installers embed their expected archive member names, so
use the bootstrap shipped with the selected release rather than a cached script
from before the rename. Source builds use the updated supervisor and Station
together. See [native releases](native-releases.md) for installation and upgrades.

## Public metadata

README links, npm repository metadata, invitation commands and default release
URLs use `HOOLC/zork`. Rust invitation and update code share
`zork_config::update::RELEASE_BASE`. The shell bootstrap remains self-contained;
its release source can be explicitly overridden for offline or trusted mirrors.

GitHub redirects the former repository location. Update existing clones with:

```sh
git remote set-url origin https://github.com/HOOLC/zork.git
```

Do not reuse the old repository name while those redirects are needed.
[GitHub's repository rename documentation](https://docs.github.com/en/repositories/creating-and-managing-repositories/renaming-a-repository)
