# GUI migration status

The default app entry is `DesktopRoot`; its device views use `RootView::new_desktop` with shared `DeviceNavigation`. The `--gateway-url` entry still creates the legacy direct-Gateway shell. It is executable compatibility code, not an archive.

## Current desktop entry

Device → Leader → Task navigation; device-isolated drafts, caches and outbox; conversation comments and unread markers; Agent/model connection settings; conversation attachment preview and save are connected to the new desktop.

The execution history page can be reached from member information, but still uses its old visual design. It has not been redesigned or approved as part of the new interface.

## Still specific to the legacy shell

- Global Inbox page.
- Global task list/board.
- Standalone Drive page (conversation attachment preview is available in the desktop).
- Old task detail / manual accept, cancel and reopen controls. `render_center_pane` mounts these only when `device_navigation` is absent.

These are not all implied requirements for the new product flow; their migration or retirement remains unresolved. The current work completes the new main workflow, not every legacy screen. Local file upload and full installation self-upgrade are not implemented in either new workflow.

## Verification

The default installer now uses GPUI's actual headless desktop renderer with fixed data, a deterministic clock and two identical replay traces. Performance fixtures explicitly use the new desktop constructor and navigation so they cannot silently show the legacy shell again. Real-window benchmarks remain opt-in.
