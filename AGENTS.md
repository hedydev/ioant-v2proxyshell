# AGENTS.md

## Product
ioant-v2proxyshell is a macOS control shell for an existing local V2rayU/Xray installation.

## Rules
- Preserve the user's existing V2rayU configuration; mutations must be explicit, previewable where practical, backed up, and atomically written.
- Treat `~/.V2rayU/config.json` as the observed runtime configuration, not an invented source of truth. V2rayU may regenerate it.
- Read-only network monitoring must never change proxy, DNS, route, TUN, or system settings.
- Do not hide direct connections: distinguish PROXY, DIRECT, MIXED, LOCAL, SYSTEM, and XRAY.
- Do not classify LOCAL/Tailscale traffic as public leakage.
- Before Git writes, re-read the current branch HEAD and affected file SHA.
- Do not run GitHub Actions.
- Commit with `AI-Agent: ChatGPT` and this chat's own eight-character `AI-Session`.

## Validation
- `npm run build`
- `cargo check --manifest-path src-tauri/Cargo.toml`
- Verify routing writes against a temporary fixture before changing the real user config.
