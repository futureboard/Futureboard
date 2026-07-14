# SphereDiscordRPC

Background Discord Rich Presence worker for Futureboard Studio. Discord IPC
never runs on the GPUI thread, reconnects when Discord starts later, and is
cleared during normal app shutdown.

Configuration:

- `.discordrpcsecret` at the repository root: local Discord application ID,
  read by the native build script, embedded in the executable, and intentionally
  ignored by Git.
- `FUTUREBOARD_DISCORD_CLIENT_ID`: optional build/runtime override for CI and
  distributable builds.
- `FUTUREBOARD_DISCORD_LARGE_IMAGE`: optional registered Rich Presence asset
  key.
- `FUTUREBOARD_DISCORD_LARGE_TEXT`: optional image hover text.
- `FUTUREBOARD_DISCORD_SHOW_PROJECT_NAME=1`: opt in to showing project names;
  names remain private by default.
- `FUTUREBOARD_DISCORD_RPC_DEBUG=1`: connection/retry diagnostics.

Users can enable or disable the integration immediately from Settings >
Advanced.

Example (PowerShell):

```powershell
$env:FUTUREBOARD_DISCORD_CLIENT_ID = "your-application-id"
$env:FUTUREBOARD_DISCORD_RPC_DEBUG = "1"
cargo run -p futureboard_native
```
