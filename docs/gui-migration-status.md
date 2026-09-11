# GUI migration status

The default app entry is `DesktopRoot`; its device views use `RootView::new_desktop` with shared `DeviceNavigation`. The direct-Gateway compatibility entry and its UI have been removed. DesktopRoot is the only application entry.

## Current desktop entry

Device → Leader → Task navigation; device-isolated drafts, caches and outbox; conversation comments and unread markers; Agent/model connection settings; conversation attachment preview and save are connected to the new desktop.

The execution history page can be reached from member information, but still uses its old visual design. It has not been redesigned or approved as part of the new interface.

## Retired screens

The old global Inbox, task list/board, standalone Drive and manual task lifecycle controls have been removed along with the SSH app launcher and compatibility CLI flags. Conversation files, execution history and current device/Leader/task navigation remain shared desktop features. Backend task APIs and stored message formats are unaffected by this UI retirement.

Local file upload and full installation self-upgrade are not implemented in the current workflow.

## Verification

The default installer now uses GPUI's actual headless desktop renderer with fixed data, a deterministic clock and two identical replay traces. Performance fixtures explicitly use the new desktop constructor and navigation so they cannot silently show the legacy shell again. Real-window benchmarks remain opt-in.
