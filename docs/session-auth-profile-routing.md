# Session auth profile routing

## Goal

Each Slack session must bind to one managed Codex auth profile.

New sessions automatically select the usable profile with the most remaining Codex quota. After a session is bound, the broker must keep using that profile. If the bound profile becomes unavailable, the broker must not automatically switch to another profile. It must stop dispatch for that session, preserve pending Slack input, post one Slack message with the session page link, and wait for a human to switch the session profile from the session page.

## Current State

The broker already manages auth profiles under `.data/auth-profiles/profiles`. `AuthProfileService` can list profiles, probe account and quota state, and change the global active profile.

The agent runtime is currently global. A worker process creates one Codex app-server runtime with one `CODEX_HOME/auth.json`. Admin profile activation changes the global active auth and restarts the worker runtime. That is not session binding. It is unsafe for concurrent sessions because one session can change the auth identity used by another.

## Target Model

The worker owns an auth-profile runtime pool:

- one Codex runtime per auth profile,
- one isolated `CODEX_HOME` per runtime,
- one `auth.json` per runtime copied from the bound profile,
- one app-server port per runtime,
- one `AppServerClient` per runtime.

The session row stores:

- `auth_profile_name`,
- `auth_profile_bound_at`,
- `auth_blocked_at`,
- `auth_block_reason`,
- `auth_blocked_notice_posted_at`.

The broker routes every agent operation through the runtime selected by `session.authProfileName`.

## Profile Selection

New session selection uses probed profile snapshots:

- exclude profiles whose account or rate limit probe failed,
- exclude profiles whose primary or secondary Codex limit is exhausted,
- rank by effective remaining quota,
- use deterministic tie breaking.

Effective remaining quota is the conservative minimum of known primary and secondary remaining percentages. Missing windows are treated as 100 percent remaining for that window because some account types may omit one window.

## Bound Session Failure

Before dispatching a pending Slack input into Codex, the worker checks the bound profile. The check has three distinct outcomes:

- usable: dispatch normally through the bound profile runtime;
- known unavailable: the profile exists but the Codex quota or credits are exhausted;
- unknown: account or quota status could not be read.

Only known unavailable profiles require a manual auth switch. A probe failure is not evidence that the account has no quota; it is telemetry failure. For an already-bound session, the worker must keep using the bound profile when the latest probe result is unknown. For a new unbound session, unknown profiles are not eligible for automatic selection because there is no stable account identity to bind yet, but the user-facing reason must still say that status could not be read instead of saying the account has no quota.

If the bound profile is known unavailable:

- do not start or steer a Codex turn,
- leave inbound messages in `pending`,
- clear `active_turn_id`,
- set auth blocked fields on the session,
- record an agent trace event,
- post one Slack notice with the session link.

The notice is idempotent for one blocked state. Repeated Slack messages while the session remains blocked must not spam the thread.

## Bound Session Recovery

`auth_blocked_at` is not a permanent manual-action flag. It records the last time
the bound profile was observed unavailable. If the same bound profile later
becomes usable again, the broker must clear the blocked fields and continue
pending dispatch through that same profile. This is not an automatic profile
switch: the session keeps its existing `auth_profile_name`.

The admin UI must derive the visible "needs account switch" state from the
current bound profile status. A stale `auth_blocked_at` row must not keep showing
`账号待切换` after the bound profile's quota has recovered. Stale rows whose
reason is only `account_probe_failed` or `rate_limits_probe_failed` must also not
show `账号待切换`, because those rows came from an unknown probe state rather than
confirmed quota exhaustion.

## Manual Recovery

The session page exposes the current auth binding and available profile list. A user can choose a usable profile and click `切换并继续处理`.

The admin action must:

- validate the selected profile exists and is usable,
- update the session auth binding,
- clear auth blocked fields,
- reset `agent_session_id` because the old app-server thread belongs to a different profile runtime,
- record an agent trace event,
- ask the worker to resume pending dispatch for that session.

Admin is a separate process in deployment, so resuming work cannot be an in-process call. The admin process calls a local worker HTTP endpoint after updating the shared state database.

## Acceptance Criteria

- A new session automatically binds to the usable profile with the most effective remaining quota.
- Existing sessions keep their bound profile even when another profile later has more quota.
- If the bound profile is unavailable, the broker does not auto-switch.
- If the bound profile status probe fails, the broker does not treat it as quota exhaustion, does not post a manual-switch notice, and continues through the existing binding.
- The pending Slack input remains pending while auth is blocked.
- Slack receives exactly one blocked notice per blocked state, with the session page link.
- If the same bound profile becomes usable again, blocked fields are cleared and pending dispatch resumes without changing `auth_profile_name`.
- Admin session list/detail must not show `账号待切换` when the current bound profile is usable again.
- Admin session list/detail must not show `账号待切换` for probe-failure-only blocked rows.
- The session page shows the current binding, blocked reason, profile candidates, and a `切换并继续处理` action.
- Manual switch clears the blocked state, resets stale agent runtime state, and resumes pending dispatch through the newly selected profile runtime.
- Slack behavior is unchanged for healthy sessions.
