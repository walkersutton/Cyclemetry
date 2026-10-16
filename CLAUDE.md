# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

Cyclemetry is a desktop app for creating video overlays with cycling telemetry data (speed, power, heart rate, elevation, etc.) from GPX files. Built with Tauri 2 (Rust shell) wrapping a Svelte 5 frontend. All rendering is native Rust (no Python backend).

## Commands

All commands run from the repo root via pnpm.

```bash
# Development
pnpm dev              # Run Tauri app in dev mode (hot-reload frontend, no sidecar)

# Build & release
pnpm build            # Full Tauri distribution build (DMG + signed update artifact)
pnpm release          # Sync versions, commit, tag, and push — triggers CI release build
                      # Bump version in src-tauri/Cargo.toml first (single source of truth)

# Code quality
pnpm lint             # ESLint (frontend)
pnpm format           # Prettier (frontend)
```

Frontend uses `pnpm` with ESLint + Prettier.

## Architecture

Two-tier desktop app:

```
Svelte Frontend (Vite/app/)
        │
        │ Tauri IPC commands (invoke)
        ↓
Tauri Runtime (src-tauri/ — Rust)
  ├── Template loading, GPX parsing, activity data
  ├── Native Rust render pipeline (src-tauri/src/render/)
  └── FFmpeg for final video encoding
```

The dev launcher picks an available localhost port for Vite and passes the same URL to Tauri. All logic lives in Rust — no sidecar process.

## Key Files

- `src-tauri/src/lib.rs` — All Tauri command handlers and app setup (menu, updater, recent GPX)
- `src-tauri/src/render/` — Native Rust rendering pipeline (activity, scene, frame, chart, template)
- `app/src/app.svelte` — Root component; menu event listeners, GPX loading, render trigger
- `app/src/state/appState.svelte.js` — All frontend state (Svelte 5 runes + localStorage persistence)
- `app/src/api/backend.js` — Frontend API client (calls Tauri invoke commands)

## Template System

Overlay configuration is JSON-based. Templates are stored at their authored resolution (4K) and scaled uniformly to the chosen output resolution at render time.

Two locations:
- **Bundled templates**: `templates/` (repo root, shipped with app)
- **User templates**: `/tmp/cyclemetry/templates/` (user-created, persisted locally)

Templates define scene timing, metric label positions/styles, and visual properties.

## Path Resolution

- **Dev**: Rust resolves fonts from `resources/fonts/`, ffmpeg from `resources/ffmpeg`, templates from `templates/`
- **Production**: bundled into `Contents/Resources/` inside the `.app`; user templates and render output go to `/tmp/cyclemetry/`

Final videos land in `~/Movies/Cyclemetry/`.

## Notes

- macOS (aarch64 + x86_64 via universal dmg), Windows, and Linux
- FFmpeg binary is in `resources/ffmpeg` (gitignored; fetched in CI via Homebrew) — not a system dependency
- Render progress is polled via `native_progress` Tauri command

## Design System
Always read DESIGN.md before making any visual or UI decisions.
All font choices, colors, spacing, and aesthetic direction are defined there.
Do not deviate without explicit user approval.
In QA mode, flag any code that doesn't match DESIGN.md.

## UI Conventions
- Every clickable element (`button`, `a`, or interactive `div`) must have `cursor-pointer` in its Tailwind classes, except directional drag handles which use directional cursors such as `cursor-ew-resize` or `cursor-ns-resize`.
