# Agent guidance

## Read first

- `CONTEXT.md` defines project terms; keep it conceptual.
- `docs/adr/` records durable architectural decisions.
- `CONTRIBUTING.md` defines contribution, testing, PR, and changelog rules.

## Invariants

- Windows x64 is the primary target. Windows ARM64 is an experimental build target; do not claim emulator compatibility without hardware validation.
- This is a Tauri desktop application. Do not add an HTTP server or browser deployment path.
- Vue accesses devices only through the commands registered in `src-tauri/src/lib.rs`.
- `DeviceManager` owns ADB discovery, process execution, timeouts, config, device state, frames, and tap validation.
- Never invoke a shell for ADB. Pass the executable and every argument separately, with a timeout.
- Preserve one active device. Switching devices invalidates the frame and selected coordinates.
- Transfer screenshot bytes directly; do not add Base64 or re-encoding.
- Do not bundle/download ADB or copy OnmyojiAutoScript GPLv3 code, game assets, or templates.

## Completion checks

Keep changes narrow and test observable behavior. Run `pnpm check` and `pnpm frontend:build`; run the affected `pnpm build:windows:x64` and/or `pnpm build:windows:arm64` target on Windows. Build artifact names must contain version, platform, and architecture. Update user documentation and `CHANGELOG.md` when behavior changes.
