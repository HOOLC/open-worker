# Same-host Synch connections

Desktop Gateway and client transport have separate Synch identities. They keep
Synch's membership checks, QUIC peer authentication, recovery, publication, and
replication. Same-host discovery only supplies an address to the existing iroh
address lookup interface; it does not add trust or implement a second protocol.

On macOS, running endpoints publish their loopback UDP addresses under
`~/Library/Caches/zork/mesh-loopback-v1`. A private directory contains one cached
hint per endpoint. Its JSON file is locked for the lifetime of that endpoint.
Readers probe the same inode they read, so a restarted process cannot make an
old port appear live. The separate identity lock serializes publishers. Cached
files from stopped processes remain useful as restart hints, but their addresses
are not returned without the live file lease. Only loopback addresses are accepted.

The endpoint registers before Synch's startup readoption. Previously paired
desktop clients resume alongside the local Gateway, using the Gateway identity
saved from its authenticated snapshot. This avoids waiting for a Gateway snapshot
before starting the very client that Gateway recovery is trying to contact.
Ephemeral ports are not copied into durable Mesh membership or peer configuration.

Normal Synch connections are reused by Synch. Public discovery and relay remain
available for other hosts. Transport tests can set `ZORK_MESH_LOCAL_DISCOVERY=0`
to force the ordinary relay path between processes on the same test machine.

GPUI allows only 200 ms for quit callbacks. A new client therefore also checks
the supervisor lock: a missing status response may mean the previous supervisor
is still draining. It waits for that lock rather than spawning a doomed duplicate.
A live desktop-owned supervisor can transfer its lease; independently started
or background supervisors retain their existing lifecycle.

Gateway closes its enrollment endpoint and Synch runtime concurrently. Closing
enrollment first can spend a second draining public address probes before Synch
even starts closing, outliving GPUI's quit callback window. Both endpoint closes
are still awaited; no transport drain or storage lifecycle lock is skipped.

Validation includes offline authenticated transfer without explicit peer ports,
simultaneous restart with persisted identities, untrusted-peer refusal, stale
address handling, normal relay transfer and revocation, and rapid desktop relaunch.
