# ioant-v2proxyshell

A macOS control shell for V2rayU/Xray focused on live network observability and safe routing control.

Initial scope:

- live per-process traffic and connection-path monitoring;
- inspect the active V2rayU/Xray runtime and routing profile;
- edit routing Direct / Proxy / Block rules with explicit Apply;
- start / stop / restart the V2rayU Xray core;
- change supported proxy modes without hiding the underlying generated configuration;
- preserve read-only diagnostics separately from mutating controls.

The project is initialized as a Tauri 2 + React/TypeScript macOS client with a Rust backend for process, filesystem, and network integration.
