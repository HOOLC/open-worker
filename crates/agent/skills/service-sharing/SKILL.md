---
name: service-sharing
description: Start, reuse, troubleshoot, and share persistent Web services through Zork Mesh when a user needs a running application or remote preview. Does not cover static file delivery or MCP server installation.
---

# Service sharing

Use this workflow for a live preview, a persistent Web app, or a shared service link that is not working. Deliver a static file with `chat.send` attachments to the explicit channel. An existing human-facing page can be delivered directly without creating a new service.

## Choose the service and execution node

Service tools act on the current Agent's execution node and Session. The user's viewing device may be somewhere else. Manage an existing service from its owning task/Session; another Session on the same node does not acquire that ownership. If work belongs on another node, delegate it to an Agent executing there; do not interpret that node's filesystem paths or localhost addresses as your own.

Check `service.list` for an existing service that matches the task. Use `service.inspect` to distinguish a running process, a reachable port and enabled sharing. A stopped or unshared service still has an identity and can be reused.

Use `service.start` when Gateway should own the command and restore it on restart. Use `service.attach` for a server already managed elsewhere. Attaching does not give Gateway control of the external process or access to its logs. Do not start a duplicate server just to obtain a link.

For a managed service, choose a stable workspace directory and an explicit port. Keep the command in the foreground so its lifetime remains observable. Pass an argv array; when shell syntax is necessary, use an explicit shell command that ends in `exec` for the server. Avoid daemonization, `nohup`, trailing `&`, and port fallback: an orphan or an automatically changed port breaks process ownership or the registered endpoint.

Bind the server to `127.0.0.1`. Gateway handles Mesh transport; opening the server on all interfaces is normally unnecessary. Configure a development server to accept `*.localhost` Host headers. Build asset, API and WebSocket URLs from the current page origin; do not hardcode the execution node's localhost port into browser-facing URLs. For HMR, avoid overrides that send the client's browser to a different localhost port.

## Verify and deliver

A successful `service.start` response means the operation was recorded; it does not establish application readiness. Inspect the service. If the process runs but `ready` is false, allow startup to progress and then inspect again. For applications with a health route, check that route on the execution node as well: a TCP listener alone is not application-level health.

Once the requested app is usable, enable access with `service.share`. Deliver the returned URL verbatim using `chat.post_page`, with a human-readable title and the explicit `chat_id` and `target` from the incoming channel message. This posts a visible link and adds the page to that conversation’s Files and pages list. Do not construct the node/service identifiers yourself or send the server's localhost URL as the remote preview link. Zork opens the shared link in its built-in browser on an authorized Mesh client; a separate public forwarding service is not needed.

When the user wants an application to keep using across tasks, separately call `page.publish` with its title and exact URL. This adds the persistent entry to the client’s global application start page. A delivered page, a shared service or a long-running process does not automatically become a published application. Use `page.unpublish` to remove an application you published; it does not stop the service or remove prior conversation references.

Keep the service when the user asks for persistent availability. Closing a browser view does not stop it. Gateway restores the saved running/sharing intent after restart. Stopping a managed service prevents that automatic launch while preserving its configuration. Unsharing disables access while leaving the process running; re-sharing preserves the URL. Choose these operations according to what the user actually wants to end.

## Diagnose using existing filesystem tools

Start with `service.inspect`: use `state`, `ready`, `last_exit_code` and `last_error` to identify process versus connection failures. Use the existing filesystem tools for log excerpts. For managed services, inspect returns their owning node, directory and stdout/stderr paths.

On that node, use `file.read` or `shell.run` with a small `tail` or a targeted `rg` search. Look at the most recent stderr first for a startup failure, then stdout for application context. If the relevant output has rotated, use the numbered files in the returned directory. Do not load every log file into context. External services need the logging mechanism of their own manager.

If the logs are on another node, have the Agent on that node read the relevant excerpt or use an already-authorized file transfer. A returned absolute path is not automatically a local file or a public Mesh file URL.

Delivery deduplication is handled by the runtime. If a result reports an uncertain outcome, inspect/list the existing service before deciding any further action; do not repeat a start or restart with unknown effects. A revision/configuration conflict requires reading current state; do not remove the revision check to overwrite it. Keep the registered ID and URL during recovery instead of creating new names on every retry.

A process that exits is reported with its exit status; Gateway does not repeatedly restart a failing command in a tight loop. Fix the cause from its status and logs, then restart that service. Do not stop unrelated services to free a port.
