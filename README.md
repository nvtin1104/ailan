# Ailan

**Ailan: a living notch for Windows 10/11, written in native Rust.**

A small pill rests at the top-center of the work area. Click it to open music and your to-do list. Drag it to move it. Long alerts grow wider. The hub window can be resized.

| | |
|---|---|
| Binary | ~675 KB |
| RAM (working set, idle) | ~10 MB |
| CPU when idle | 0% (1 frame/minute for the clock) |
| Startup | ~130 ms to first frame |

## Features (MVP)

- **Notch shell**
  - spring animation, click to open, drag to move
  - auto-hides while a fullscreen app or game runs
  - one fixed behaviour: countdown, stopwatch, music, then today's to-dos; otherwise a quiet "Ailan" capsule
  - starting focus or adding a task in the Hub flies an icon from the button into the island
  - tray icon: Start with Windows, EN/VI language
- **Clock** with date.
- **Music** from any player through Windows SMTC (Spotify, browsers, Media Player…)
  - artwork, progress
  - previous / play-pause / next
- **Focus**: Pomodoro timer (25/5/15 by default), with a count of sessions completed today.
- **To-do**
  - type it and press Enter
  - `@15:30` at the end of a task sets a reminder (shown as a Dynamic Island banner)
- **Status and OSD**
  - status: Wi-Fi/Ethernet, battery and charging, headphones
  - OSD pops up when volume or brightness changes
  - banners for charging, low battery and headphones connected
- **Notification mirroring**
  - Zalo / mail / other app toasts appear as Dynamic Island banners (toggle in Hub → Settings)
  - needs notification access: Settings → Privacy → Notifications → Ailan

## Build and run

Requires Rust stable (MSVC toolchain) on Windows 10 1809+ / 11.

```bash
cargo run --release
```

```bash
cargo test --workspace
```

Developer flags:

```bash
cargo run -- --stats
```

```bash
cargo run -- --expand=focus
```

- `--stats` logs frames, wakeups and memory every 60 s.
- `--expand=music|focus|todo` opens the notch on that tab and keeps it open.

Data locations:
- settings: `%APPDATA%\Ailan\settings.toml`
- to-dos: `%APPDATA%\Ailan\data.json`
- logs: `%LOCALAPPDATA%\Ailan\logs\ailan.log`

## Architecture

- `crates/ailan-core`: pure logic with unit tests (reducer, Pomodoro, to-do, scheduler, spring animation, storage, i18n).
- `crates/ailan`: Win32 + Direct2D/DirectComposition, Core Audio, SMTC, WMI, power/network notifications.

See [docs/system-design.md](docs/system-design.md) for the system design and [docs/optimization.md](docs/optimization.md) for the optimization strategy and measured results.

## Roadmap

Synced lyrics, file shelf (drag-drop), clipboard history, water reminder, calendar/Today, installer (MSIX/Inno Setup), auto-update, settings inside the notch, per-monitor choice.
