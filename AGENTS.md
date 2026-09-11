# Agent guidance

## Read first

- `CONTEXT.md` defines project terms; keep it conceptual.
- `docs/adr/` records durable architectural decisions.
- `CONTRIBUTING.md` defines contribution, testing, PR, and changelog rules.

## Invariants

- Windows x64 is the only build and release target.
- This is a Tauri desktop application. Do not add an HTTP server or browser deployment path.
- Vue accesses devices only through the commands registered in `src-tauri/src/lib.rs`.
- `DeviceManager` owns ADB discovery, process execution, timeouts, config, device state, frames, and tap validation.
- Never invoke a shell for ADB. Pass the executable and every argument separately, with a timeout.
- Preserve one active device. Switching devices invalidates the frame and selected coordinates.
- Transfer screenshot bytes directly; do not add Base64 or re-encoding.
- Bundle only the pinned, checksum-verified open-source ADB declared in `src-tauri/adb-distribution.json`; never download ADB at application runtime, and preserve its license and notices.
- Keep the bundled ADB server isolated from external ADB consumers and stop only the server instance owned by this application.
- Do not copy OnmyojiAutoScript GPLv3 code, game assets, or templates.

## Completion checks

Keep changes narrow and test observable behavior. Run `pnpm check` and `pnpm frontend:build`; run `pnpm build:windows:x64` on Windows. Build artifact names must contain version, platform, and architecture. Update user documentation and `CHANGELOG.md` when behavior changes.
