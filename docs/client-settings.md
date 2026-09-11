# Client settings

Client preferences belong to the client device, independently of each connected
node's agents, model connections and service settings.

On desktop, Help and diagnostics and About are separate sidebar tabs. Each opens
its own titled page, without an extra Client settings tab or in-page tab bar.
Desktop settings lists use full-row actions without decorative trailing arrows.
Mobile retains its trailing chevrons. Desktop task rows remain directly visible
without a leader fold button;
device-level folding and task pagination remain available.

Text uses the client defaults and normal platform scaling. There is no custom
font-size control; previously saved custom scale values are ignored.

- **Help and diagnostics:** request each saved device's node information without
  changing its configuration. Copied reports include version, operating system,
  check time and reachability, and exclude device names, addresses, identities,
  credentials and chat content.
- **About:** show the compiled application version and Zork's own MIT license.
- The desktop retains its existing account flow when account services are
  configured. Connection identity is not a daily client preference. The Notifications page controls desktop alerts, sound, private previews and
  conversation muting; see [Desktop notifications](notifications.md). Unsupported
  theme controls are not exposed.

User messages display their original text. Markdown punctuation, link syntax,
code fences, entity text and line breaks remain literal, including in copied
selections and quoted comments. Assistant messages continue to render Markdown.
This applies to saved history and pending user messages on desktop and Android.

Desktop uses shared GPUI controls and a cached plain document for user messages;
only assistant documents enter the Markdown parser. Android uses a selectable
plain TextView for user messages and its existing Markwon component for assistant
messages. Neither path changes the text sent to a device.

Validation artifacts are kept under `artifacts/client-settings-pc/` and
`artifacts/android/literal-user/`. CPU timings and emulator measurements are
separate from physical display FPS. Tests use isolated fixture data.

For native settings verification after rebuilding the normal GUI and node binaries,
run `scripts/test-client-settings.py` with the configured build environment. It uses
isolated devices and checks online/offline diagnostics, clipboard redaction, About
and license display. Set `ZORK_GUI_TEST_WINDOW_SIZE` to `1280x800` or `900x600`.
