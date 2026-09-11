# Zork Mesh library integration

The Gateway owns `managed::Runtime` and shares its `MeshNode` handle with
product and enrollment services. `managed::start` acquires the Synch lifecycle
lock, initializes existing-or-new node state, calls `synch_engine::Node::open`,
and starts the engine loops on the caller's Tokio runtime. The Gateway awaits
background failure and clean shutdown. Supervisor only manages the Gateway
and Agent processes; it does not open or supervise a Synch node.

`MeshNode` calls typed Rust APIs for trust, delegation, source publication,
verified content reads and socket connections. There is no `synch-cli`, local
gRPC client/server, control socket/token, protobuf code generation or command
interpreter. A stopped library handle cannot operate on the node; releasing
its engine reference also allows immediate rebinding of the same UDP port.

The desktop's remote transport uses the same library API on its background
executor. Its shared handle is explicitly rebound after network settings
change; no transport helper is launched.

The remote `sync/sock/1` protocol and fixed eBPF bridge remain compatible with
existing peers. The bridge carries authenticated remote product requests to
the Gateway ingress; local engine operations never travel through it.

Synch is pinned to v0.1.8, Git revision
`6d6283f09c32476dc77c09f76a2b2529a42a558d`, in Cargo.toml and Cargo.lock.
The dependency's license is retained in `LICENSE.synchronicity`.

Upgrading from the former daemon/supervisor-owned implementation requires a
full node stop/start so the previous owner releases the identity lock. Starting
a second owner is rejected; the library does not kill a process to take over.
The identity/database/CAS directory is reused. Obsolete control socket/token
files are removed only after acquiring exclusive ownership.
