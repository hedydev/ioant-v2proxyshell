# Architecture

## Product boundary

V2Proxy Shell is a macOS control shell around an existing V2rayU/Xray installation. It does not replace the proxy core. The client adds observability and a safer, more legible control surface.

## Technology

The first implementation follows the proven Mac-client pattern used in the Hero Skills Free Master architecture: a React/Tauri shell with a Rust backend, with read-only collectors separated from explicit mutation actions. Script/system-command behavior can remain independently testable while stable/high-frequency collectors migrate to native Rust over time.

```text
React UI
  -> Tauri command boundary
      -> collectors/
           process + socket state
           nettop traffic counters
           V2rayU/Xray runtime state
      -> adapters/
           active generated config (~/.V2rayU/config.json)
           V2rayU routing database
           V2rayU UserDefaults state
      -> actions/
           save routing profile
           activate routing profile
           start / stop / restart V2rayU
```

## V2rayU facts mapped from upstream source

V2rayU persists routing profiles in a SQLite `routing` table. A routing entity contains `uuid`, `name`, `remark`, `domainStrategy`, `domainMatcher`, `block`, `proxy`, `direct`, and `sort`. The active routing UUID is mirrored by the `runningRouting` UserDefaults value. The active run mode is similarly persisted through `runMode`.

V2rayU then generates `~/.V2rayU/config.json` for Xray. The generated file is useful as runtime evidence, but editing only that file is not a durable profile edit because V2rayU can regenerate it.

Therefore routing edits in this app should target the persisted routing profile first, then explicitly reload/restart the V2rayU runtime when the user presses Apply.

## Traffic model

Each process is classified independently:

- `PROXY`: socket connects to local HTTP/SOCKS proxy ports (default 1087/1080).
- `DIRECT`: socket connects directly to a public destination.
- `MIXED`: the same process has both proxy and direct public sockets.
- `LOCAL`: loopback/LAN/private traffic.
- `SYSTEM`: expected system networking such as Tailscale.
- `XRAY`: Xray/V2rayU runtime traffic.

Traffic rate is calculated from macOS `nettop` cumulative process counters across samples. Socket path comes from `lsof`. These are separate signals and must not be conflated.

## Routing editor

The routing page is profile-oriented, not generated-JSON-oriented:

1. list all persisted V2rayU routing profiles;
2. show the currently selected/running profile;
3. select a profile to inspect its Direct / Proxy / Block line lists;
4. add, edit, or remove individual lines;
5. update strategy/matcher;
6. Apply explicitly;
7. persist the profile and reload the runtime;
8. retain a backup before mutation.

Raw generated JSON remains useful as a diagnostics view later.

## Runtime control

Start/stop/restart is explicit. Mode switching (PAC / Manual / Global / TUN) should integrate with V2rayU's actual persisted `runMode` and runtime behavior rather than independently changing macOS proxy settings behind V2rayU's back.

## Privilege direction

The MVP uses normal user-visible macOS commands and may have incomplete visibility for processes owned by other users. If full-machine socket/process coverage is required, add a narrow privileged helper later rather than repeatedly prompting for administrator access.
