# Loading states

The default loading signal is a 1.7 / 24 px graphite ring, rendered by the shared GPUI `components::loading` module. Participant activity uses three quiet dots. Browser-only design alternatives remain under `assets/loading/preview.html`.

## Placement

| Wait | Presentation |
| --- | --- |
| Initial task, Inbox or file list without cached content | Local loading indicator; keep navigation available |
| Initial conversation without cached messages | Explicit “Loading messages”, separate from waiting for the first reply |
| Inbox detail or artifact preview | Indicator only inside the detail pane |
| Older messages or history | Inline indicator; preserve existing rows and scroll position |
| Message send, task creation or cancellation | Replace the action icon while its request is pending |
| Connection / agent save, model discovery, device rename | Indicator inside the initiating button |
| File save | Indicate actual file writing; selecting a destination only disables duplicate invocation |
| Running participant | Dots beside the existing activity label; pause during transcript scrolling |

Use cached content immediately. Silent synchronization and history refresh do not display loading. Offline outbox entries, external authorization, scanning and human approval remain explicit waiting states. No branded full-window loading screen is introduced.

## Lifecycle

The indicator reserves its dimensions immediately and appears after 200 ms. A visible indicator schedules at most one pending timer, at roughly 30 Hz; clipped or unmounted indicators stop scheduling. Reduced motion keeps a static glyph. The SVG contains only geometry; GPUI owns the clock and rotation. There is no reliance on SVG CSS animation in the native renderer.

Message requests carry a generation, so an old success or error cannot change the current conversation loading state. Execution-detail requests also carry a generation. File list loading / failure does not display the empty-list guidance.

## Validation

`headless_loading` exercises delay, movement, reduced motion, clipping, removal and activity pause with the real offscreen renderer. It is included in `scripts/test-desktop-headless.py`. Shared loading stories are available in the GPUI component catalog.

Task-specific measurements and screenshots are stored under `artifacts/loading/`; final validation results are reported with the implementation delivery.
