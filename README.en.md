<div align="center">

**English** | [简体中文](README.md)

</div>

# TexMantis (青刃)

A portable Windows system toolbox with a built-in LAN instant-messaging suite. Built with Tauri 2 (Rust backend, Svelte 4 frontend), shipped as a single executable — no installer, no account, no server required.

- Runs on: Windows 7 SP1 through Windows 11 (x64)
- License: MIT

[Download](https://github.com/kaka77/texmantis/releases) · [Building](#building) · [Source layout](#source-layout)

## Features

### System tools

| Module | Description |
|---|---|
| Processes | 2-second auto refresh; sort by CPU / memory / start time; digital-signature status check; kill a single process or a whole process tree (native `TerminateProcess` first, falls back to `taskkill /T /F`); locate the file in Explorer |
| Services | List / start / stop / disable / delete; Microsoft-signed services are filtered out by default |
| Startup | Registry `Run` / `RunOnce` (HKLM, HKCU, WOW6432Node) and the Startup folder; delete and locate |
| Scheduled tasks | Lists tasks outside the `\Microsoft` path; run / end / enable / disable / delete |
| Network monitor | TCP / UDP connection table refresh; optional offline IP geolocation via an ip2region database |
| Threat alerts | Subscribes to the Sysmon/Operational channel via `EvtSubscribe` and raises popups for built-in rules (e.g. a process opening LSASS with memory-read access). Requires Sysmon installed |
| File unlock | Queries locking processes through the Restart Manager; optionally kill them and force-delete the file |
| Encode/decode | Text Base64 and URL codec |
| Data extract | Bulk-extract IPv4 / IPv6 / URL / domain / hostname from pasted text, with dedupe and sorting |
| Tweaks | Double-Ctrl to show/restore the desktop (low-level keyboard hook); desktop shortcuts; Explorer context-menu entries |

### LAN chat

No server involved — two devices on the same LAN can talk once the app is running.

- **Discovery**: UDP broadcast (TCP and UDP share the 25100–25109 port range, auto-falls forward when occupied); manual radar scan supported
- **Messaging**: direct messages, group chat, emoji, a history viewer, right-click forwarding, read receipts, window shake
- **Files**: any size, chunked transfer; resume from offset, pause / cancel / retry; configurable receive directory (default `Downloads\TexMantis`)
- **Encryption**: each session negotiates a key with X25519; frames are encrypted with ChaCha20-Poly1305 (AEAD); negotiation is pinned to the peer's public-key fingerprint
- **Storage**: chat history (JSONL), received files and configuration live on the local disk; clearable from the settings page

### Local push API

An HTTP interface for scripts on the same machine, feeding custom messages into the app's notification popups:

```
GET/POST http://127.0.0.1:25110/push
Auth:    ?key=… / X-API-Key header / Authorization: Bearer … (10-char random key, generated in settings)
Params:  title (≤80 chars), body (≤600 chars)
Limits:  loopback only; last 10 pushes kept; popup popups throttled at 300 ms
```

## Download and usage

Grab a zip from [Releases](https://github.com/kaka77/texmantis/releases). Extracting yields:

```
TexMantis-portable/
├── TexMantis.exe                     main program (frontend assets embedded)
├── WebView2Loader.dll                WebView2 loader
├── bcryptprimitives.dll              Win7 shim: ProcessPrng → SystemFunction036
├── api-ms-win-core-synch-l1-2-0.dll  Win7 shim: WaitOnAddress / WakeByAddress*
└── 使用说明.txt                       readme (Chinese)
```

Copy the folder anywhere and run; uninstalling means deleting the folder. First launch shows a UAC prompt — refusing it keeps the app running with limited capabilities (the UI indicates what is restricted).

### Requirements

| Item | Requirement |
|---|---|
| OS | Windows 7 SP1 / Server 2008 R2 or later, x64 |
| WebView2 Runtime | Preinstalled on Win11 and recent Win10; on Win7 install it manually (the last Evergreen build supporting Win7 is 109; a fixed version works) |
| Privileges | Runs as a standard user; administrator unlocks service control, killing process trees, scheduled-task management |

### Notes on Windows 7 compatibility

Tauri 2 binaries reference Win8+ APIs by default. This project renames two imports after linking (`GetSystemTimePreciseAsFileTime` → `GetSystemTimeAsFileTime`, `SetProcessDpiAwarenessContext` → `SetProcessDPIAware`) and ships two hand-written shim DLLs covering the `WaitOnAddress` family and `ProcessPrng`. See `tools/win7_patch.py` and `src-tauri/win7compat/`.

## Building

### Prerequisites

| Dependency | Purpose |
|---|---|
| Rust stable (default rustup toolchain) | backend compilation |
| Node.js ≥ 20 + npm | frontend build |
| MinGW-w64 (`x86_64-w64-mingw32-gcc`) | linker when cross-compiling from Linux / macOS; MSVC works on Windows |
| Python 3 (stdlib only) | runs `tools/win7_patch.py` (import-table patch + build-path scrub) |
| MinGW gcc | only to rebuild the shim DLLs in `src-tauri/win7compat/` (prebuilt DLLs are included) |

WebView2 is a runtime requirement for end users; the build machine does not need it.

### Steps (cross-compiling from Linux)

```bash
npm install
rustup target add x86_64-pc-windows-gnu
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
npx tauri build --no-bundle --target x86_64-pc-windows-gnu
# output: target/x86_64-pc-windows-gnu/release/texmantis.exe
python3 tools/win7_patch.py target/x86_64-pc-windows-gnu/release/texmantis.exe
```

Then place the exe next to `WebView2Loader.dll` and the two shim DLLs.

On Windows: `npm run tauri dev` (hot reload) / `npm run tauri build`.

## Source layout

```
src/                      Svelte 4 frontend
  App.svelte              shell: title bar, navigation, view routing
  lib/Social.svelte       LAN chat (messaging / groups / files / settings)
  lib/ApiView.svelte      push-interface view
  lib/api.ts              typed Tauri invoke wrappers
src-tauri/src/
  main.rs                 entry point, command registration
  social.rs               chat core: discovery, sessions, transfer, encryption, receipts
  monitor.rs              Sysmon alert engine (EvtSubscribe)
  pushapi.rs              push-interface HTTP service (127.0.0.1:25110)
  commands.rs             processes / services / startup / tasks (sysinfo + PowerShell JSON)
  optimize.rs             tweaks (keyboard hook, context menus, …)
  settings.rs             config persistence (config.json in the data directory)
  win7compat/             Win7 shims (C sources + prebuilt DLLs)
tools/win7_patch.py       PE post-processing: import renames, panic-path scrub
```

## Privacy and data

- The app talks to no server. Traffic is limited to LAN discovery and messaging (UDP/TCP 25100–25109) and the loopback push interface (127.0.0.1:25110)
- Messages and files are encrypted in transit; chat history and received files are stored as plaintext in the local data directory
- Released binaries have build-machine paths scrubbed from panic location strings after linking (`tools/win7_patch.py`); feel free to verify
- This is a system-management tool (processes, services, file deletion). Use it only on machines you are authorized to control; you are responsible for the consequences

## Known limitations

- On Win7, WebView2 must be installed manually and tops out at version 109
- LAN discovery relies on UDP broadcast — devices on different subnets/VLANs are invisible to each other
- Threat alerts depend on Sysmon; without it the page stays empty
- Group chats have no read receipts; chat history is stored as local plaintext

## About

A personal project written in the author's spare time for fun and for their own use — no roadmap, updates are irregular. Issues and PRs are welcome, but responses are equally unhurried.

## License

[MIT](LICENSE)
