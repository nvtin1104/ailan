# Scout 01 — island flow + UI (read-only)

Docs present: `docs/system-design.md`, `docs/optimization.md` only.
Missing: `docs/codebase-summary.md`, `docs/code-standards.md`, `docs/development-rules.md`, `docs/design-guidelines.md`.

No persisted window position, size, or DPI scale. Island is always re-placed top-center of the primary monitor.

---

## 1. Hover / collapse / click / pin

Constants: `crates/ailan/src/app/mod.rs` 44–52.

| Name | Value | Role |
|---|---|---|
| `T_HOVER` | timer id 2 | hover-intent |
| `T_COLLAPSE` | timer id 3 | leave delay |
| `HOVER_INTENT_MS` | **95** | arm expand |
| `COLLAPSE_DELAY_MS` | **340** | arm collapse |

Handler: `crates/ailan/src/app/notch.rs`.

- `WM_MOUSEMOVE` 372–418. First entry only (`!mouse_inside`): `TrackMouseEvent(TME_LEAVE)`, kill `T_COLLAPSE`, if not expanded `SetTimer(T_HOVER, 95)`. Later moves only update hover hit + `hover_anim`. `dwHoverTime` is 0; intent is a manual timer, not `TME_HOVER`.
- `WM_TIMER` `T_HOVER` 334–340: kill timer; if `mouse_inside` → `expand()`.
- `WM_MOUSELEAVE` 419–432: clear inside/cursor/hover, kill `T_HOVER`; if expanded and not editing → `SetTimer(T_COLLAPSE, 340)`.
- `T_COLLAPSE` 342–348: collapse only if `!mouse_inside && input.is_none()`.
- `WM_LBUTTONDOWN` 433–439: `hit_test` → `on_action`. `Action::Expand` (225–228) calls `expand()` immediately (no 95 ms). `Action::DismissBanner` dismisses then expands.
- `stop_editing` 179–181: if expanded and pointer already left, arms collapse 340 ms.

`expand()` 73–95: sets `expanded`, picks tab from collapsed mode (Media→Music, Todo→Todo, else `primary_activity`), snaps `tab_pos`, **`dismiss_banner()`**, `after_change()`.
`collapse()` 97–106: **no-op if `pinned`**. Clears hover, sets expand spring target 0 via `after_change`.

Pin is CLI only. `App.pinned` (`app/mod.rs` 134, 229) is `init.pin_tab.is_some()`. `main.rs` 60+ `--expand=…` (`Init.pin_tab` 152–153). Not a settings field. `collapse()` cannot close a pinned island.

Springs set in `after_change` (`app/mod.rs` 322–350): `w`/`h` → `expanded_size` or `collapsed_size`; `e` → 1 or 0; `tab_pos` → visible index. `reduce_motion` snaps them. Initial springs 192–210: size `(380, 40)`, expand `(340, 37)`, hover `(620, 38)`, tab `(440, 34)`, eyes `(220, 26)`.

Hub mouse (`hub_host.rs` 477–505) tracks leave for cursor/hover only. It does not expand the island.

---

## 2. Window origin, DPI, display

`App::place` `app/mod.rs` 261–284.

- Monitor: `MonitorFromPoint(0,0)` + `MONITOR_DEFAULTTOPRIMARY`, `rcMonitor` (full monitor, not work area).
- DPI: `GetDpiForMonitor(MDT_EFFECTIVE_DPI)`; `scale = dx/96` (**dy ignored**). Process is Per-Monitor V2 (`main.rs` 96).
- Size: `WIN_W * scale` × `WIN_H * scale` (480×152 DIP).
- Origin: `x = left + (width - wpx) / 2`, `y = rcMonitor.top`. `SetWindowPos(HWND_TOPMOST, …, SWP_NOACTIVATE)`. Stored in `origin` (116–117) for eyes and flight end point.
- `WM_DPICHANGED | WM_DISPLAYCHANGE` (`notch.rs` 475–478) calls `place()` again. Suggested rect in `lparam` is ignored. Always primary, always top-center. A window on another monitor is not tracked.

Create size is 1×1 (`main.rs` 204–216) until first `place`. Styles: `WS_POPUP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`. Docs claim `WS_EX_NOREDIRECTIONBITMAP`; that flag is only on the flight overlay (`platform/flight.rs` 79), not the notch.

**Persisted position: none.**

---

## 3. Size constants and springs

`crates/ailan/src/ui/mod.rs` 51–132.

| Symbol | DIP | Lines |
|---|---|---|
| `WIN_W` / `WIN_H` | 480 × 152 (swapchain box, includes ears) | 53–54 |
| `expanded_size` empty | 380 × 40 | 58–59 |
| `expanded_size` Todo | 380 × 114 | 62 |
| `expanded_size` Music | 380 × 127 | 63 |
| `expanded_layout` empty | 420 × 44 | 68–69 |
| `expanded_layout` Todo | 420 × 126 | 72 |
| `expanded_layout` Music | 420 × 140 | 73 |
| collapsed Minimal/Media/Todo/Countdown/Stopwatch | 150 × 30 | 87–88 |
| collapsed OSD | 240 × 34 | 90 |
| collapsed Banner | 250 × 34 | 91 |
| radius collapsed / expanded | 14 / 29; OSD+Banner base 16 | 77–78, 105–110 |
| ear collapsed / expanded | 10 / 20; OSD+Banner base 11 | 80–81, 96–101 |

`pill_rect` 130–132: horizontally centered in `WIN_W`, `y = 0`.

Draw path 252–266: layout is authored at `expanded_layout`, then scaled by `expanded_size.w / layout.w` (`380/420`) into `pill_rect(expanded_size)`. Hit rects scaled the same way. Height is **not** the layout height; the spring targets `expanded_size`.

`SetWindowRgn` `app/mod.rs` 286–312: axis-aligned union of pill rect + ear strip (`ear+2` px tall). Not a rounded region. Key cached in `region`. Updated every frame (`frame` 473).

Flight overlay has its own `SetWindowRgn` (`platform/flight.rs` 168–172).

Spring targets: see §1. Landing gulp adds velocity `w += 320`, `h += 80` (`effects.rs` 159–164), then `after_change` retargets to the real size.

`optimization.md` L37 still says swapchain ≈ 440×190 DIP. Code is 480×152.

---

## 4. Dynamic content priority and width

`Collapsed` order and `collapsed()`: `crates/ailan-core/src/state.rs` 55–66, 335–353.

1. OSD if `until > now` (`OSD_MS` 1500, L10) → width **240**
2. Banner (`BANNER_MS` 4000, reminder 25000, alarm/timer 60000) → width **250**
3. Countdown active → **150**
4. Stopwatch active → **150**
5. `primary_activity`: Music if playing, else Todo if today has unfinished work (`ACTIVITY_ORDER` L69 = `[Music, Todo]`) → **150**
6. else Minimal eyes → **150**

Comment L330 still says "Focus wins". `Tab` is only Music | Todo (`settings.rs` 11–18). Focus session is gone. `BannerIcon::Focus` / `Break` remain (state.rs 38–39); About menu still paints a Focus banner (`effects.rs` 108–116).

Hover pages (`hover_activities` 194–204): Music if playing **or title non-empty**; Todo if today pending non-empty. Order fixed. Empty → expanded island is the 380×40 stub; `draw_expanded` returns after the header (`tabs.rs` 47–51).

Width is **not** set inside hover/banner modules. `collapsed_size(mode)` / `expanded_size` pick it; `after_change` writes spring targets. Banner/OSD drawing (`ui/collapsed.rs` 74–136) uses whatever `v.pill` already is. Banner snooze/done buttons do not change width.

Idle eyes: raw mouse only while `!hidden && !expanded && mode == Minimal && !reduce_motion` (`notch.rs` 25–48). Gaze via `eye_offset` (`ui/mod.rs` 116–127), ±3.5 / ±2.5 DIP.

---

## 5. Hub vs island — what “the app” is

Notch is the pill. The app window is a second HWND.

| Piece | Path | What |
|---|---|---|
| Create | `platform/appwindow.rs` 25–74 | class `AiLanAppWindow`, `WS_EX_APPWINDOW`, popup+thickframe, initial 1000×680, DWM round 8, acrylic |
| Host | `app/hub_host.rs` | open/close/resize, actions, `handle_hub` |
| Layout | `hub/controller.rs` 21–35, 124–137 | Compact **380×600**, Full **1000×680**. No free resize (`hub_host.rs` 462–468). Titlebar drag = `HTCAPTION` |
| Pages | `NAV_PAGES` controller.rs 34–35 | Today, Todos, Reminders, Settings, AI |
| Reminders tabs | `hub/pages/reminders.rs` 27–35, `clock.rs` | 0 reminders, 1 alarms, 2 timer, 3 stopwatch, 4 world clock |
| Open | tray left-click toggles hub (`notch.rs` 315–324); island home glyph → Settings (`tabs.rs` 36–42, `notch.rs` 236) | |
| Fly | `effects.rs` 136–157 | from last hub press to pill center, 0.52 s (`flight.rs` 19). Skipped if reduced motion, hidden, hub closed, or island already expanded. Call sites: copilot submit, alarm add, countdown start, todo add (`hub_host.rs` 188–189, 213, 240, 301–303). While flying, `after_change` keeps previous `mode` (`app/mod.rs` 324–328) |

Island UI besides the pill: tray menu (autostart, language, about, exit — `effects.rs` 90–95), flight overlay, toasts via tray for reminder/alarm/timer.

---

## 6. Settings persisted today

`Settings` `crates/ailan-core/src/settings.rs` 44–52, default 65–68:

- `lang: Option<Lang>`
- `tab: Tab` (Music default; unknown string → Music, `de_tab` 32–38)
- `reduce_motion: bool`

`#[serde(default)]`. Unknown keys ignored, not fatal (`storage.rs` test `legacy_island_settings_are_ignored_not_fatal` 144–170 drops `minimal`, `mode`, `pin_todo`, `[priority]`, `[minimal_ui]`, `[app_window]`).

`Data` (`settings.rs` 77–86) is `data.json`: todos, alarms, world_clocks, `focus_day`, `focus_today`. Not window geometry.

**No position, size, or scale field.**

`system-design.md` §5 (L104–107) is stale: it still lists Pomodoro duration in `settings.toml`.

---

## 7. UI bugs / inconsistencies in code

- **Two size functions.** `expanded_size` vs `expanded_layout` (`ui/mod.rs` 57–75). Scale is width-only (`exp.0/lay.0`). Music layout 140 × 380/420 = 126.67 but spring height is **127**. Todo 126 × scale = 114 (matches). Empty 44 × scale = 39.8 vs spring **40**.
- **Tab height jump.** Music target 127, Todo 114 (`ui/mod.rs` 62–63). Comment L55–56 claims shared width so tabs never jump horizontally; vertical jump remains. `draw_todo` comment (`tabs.rs` 235) says empty todo is 420×82; that size is not returned.
- **Arrows authored in 420 space, asymmetric.** Right arrow always at `p.x + 395` (`tabs.rs` 68); left only when `active_tab != Music` (65–67). Circle r=11, hit 28×28 (`ui/mod.rs` 312–326) vs drawn 22.
- **Dead palette.** `PANEL_BG`, `SURFACE` `allow(dead_code)` (`ui/mod.rs` 20–23). Island is monochrome; hub uses a different palette in `hub/widgets.rs`.
- **Radii.** Island collapsed 14 / expanded 29 / OSD-banner base 16 (`ui/mod.rs` 77–110). Hub window corner is DWM 8 (`widgets.rs` 56–58 `RADIUS_WINDOW`). Hub cards 14, items 10, buttons 8. Banner buttons radius 11 on a 22-tall rect (`collapsed.rs` 116–124) while pill radius is 16. Todo field radius 12.5 inside a 29-radius island (`tabs.rs` 262).
- **Hit vs drawn.** `SetWindowRgn` is a rectangle + ear strip (`app/mod.rs` 305–307); drawn body is rounded with concave ears (`gfx` `fill_notch`). Square corners outside the curve still receive hover. Banner button hits are `inset` larger than the glyph (`collapsed.rs` 120, 125). `hit_test` (`ui/mod.rs` 331–336) prefers the first non-`Expand`/`TodoScroll` hit, so buttons beat the full-pill `DismissBanner`/`Expand` pushed later (`draw` 270–272).
- **Stale Focus copy.** `tabs.rs` 1 and 47, `collapsed.rs` 1, `state.rs` 182 and 330, `system-design.md` §4. No Focus tab. `tab_pos` clamped to `0..=2` at render (`app/mod.rs` 512) though only two activities exist (max index 1).
- **Header coords assume a wide layout.** Battery ring at `p.x + 146` (`tabs.rs` 18) inside a 420-wide layout. Fine at that scale; will collide if the layout width changes without moving those constants.
- **Docs vs timers.** Design says 160 / 380 ms. Code is 95 / 340.
- **Tray.** Design says left-click opens the notch for 4 s. Code toggles the hub (`notch.rs` 318–323).

---

## 8. ailan-core tests that move if hover/expand semantics change

No test asserts 95 ms, 340 ms, pin, or pixel sizes. Those live only in the binary. Breakage is the **state contract** the shell reads:

`crates/ailan-core/src/state.rs`

- `idle_rests_in_minimal` 421–426 — empty hover list; collapsed Minimal. Breaks if idle expands to a page or Todo shows when empty.
- `music_ranks_above_todo` 429–438 — collapsed Media over Todo; hover `[Music, Todo]`. Breaks if priority or hover visibility changes (paused-with-title is hover-visible but not "running"; this test uses `playing: true`).
- `clock_tools_rank_above_music_and_ring_as_banners` 441–463 — Stopwatch > Media, Countdown > Stopwatch, then Banner. Breaks if clock tools leave the collapsed priority or stop raising banners.
- `todo_only_live_while_today_has_unfinished_work` 466–476 — running vs not; completed/future → Minimal.
- `first_volume_event_is_silent_then_osd` 409–418 — first volume does not show OSD; later change → `Collapsed::Osd` for `OSD_MS`.
- `low_battery_and_headphones_banners` 493–505, `todo_reminder_fires_once` 508–517 — banner wins `collapsed()`.

`crates/ailan-core/src/scheduler.rs`

- `expanded_header_clock_ticks_each_minute` 72–76 — expanded ⇒ next minute. Breaks if the header clock is no longer tied to `view.expanded`.
- `minimal_and_hidden_idle_never_wake` 85–91 — collapsed idle ⇒ `None`. Breaks if eyes or a clock tick while collapsed.
- `media_progress_ticks_only_on_music_tab` 93–111 — progress timer only when expanded **and** tab Music. Breaks if the music page no longer shows a live position, or collapsed media starts ticking.
- `todo_island_wakes_at_midnight` 78–83 — collapsed Todo wakes at local midnight.
- `alarms_and_timers_wake_even_when_hidden_and_tick_seconds_when_shown` 113–122 — per-second only while countdown/stopwatch is the collapsed mode.

`storage.rs` `legacy_island_settings_are_ignored_not_fatal` 144–170 fails if old keys (`pin_todo`, `priority`, `app_window.last_page`) become hard errors. It does **not** require those fields to stay absent; adding new fields with defaults is safe. It will fail if a new field is required and missing from that fixture... the fixture has no `tab`/`reduce_motion`, and `partial_settings_use_defaults` 132–141 depends on defaults.

`anim.rs` tests only the spring integrator, not island targets.

Binary-only: `ui/mod.rs` `gaze_tests` (eye offset). Not in ailan-core.

---

## system-design.md — quotes this rebuild invalidates

Hover timing (`docs/system-design.md` 80):

> Hover vào pill ≥ 160 ms (hover-intent, tránh mở nhầm khi rê chuột qua thanh tab trình duyệt) → mở rộng (spring); `WM_MOUSELEAVE` → thu gọn sau 380 ms.

Code is 95 ms / 340 ms (`app/mod.rs` 51–52). A rebuild that changes intent/leave again leaves this sentence false either way; it is already false.

Fixed top-center (opening L3, and L83):

> một viên thuốc (pill) nhỏ ở mép trên giữa màn hình

> `WM_DPICHANGED` / `WM_DISPLAYCHANGE` → đặt lại vị trí & kích thước swapchain.

`place()` always recenters on the primary `rcMonitor` top edge (`app/mod.rs` 261–275). Any user-placed or multi-monitor origin invalidates L3 and the "đặt lại vị trí" rule.

Fixed pill sizes (L89–90, L99–100):

> 1. OSD (âm lượng / độ sáng) — 1.5 s, pill 240×34
> 2. Banner … — pill 1 dòng 250×34
> …
> Thanh tab: **Nhạc · Tập trung · Việc cần làm**.

OSD 240×34 and banner 250×34 match `collapsed_size` today (`ui/mod.rs` 90–91). Everything else is not one fixed pill: collapsed activities are 150×30, expanded is 380×114 or 380×127, and the Focus tab does not exist. A rebuild that drops fixed DIP sizes invalidates L89–90 and the three-tab expanded spec (L99–100).

Also stale, same section: tray click opens the notch 4 s (L81). Code toggles the hub.

`optimization.md` L37 (≈ 440×190 DIP swapchain) does not match `WIN_W`/`WIN_H`.

---

## Highest-risk files

1. `crates/ailan/src/app/notch.rs` — enter/leave/click/pin/expand. Timer semantics live only here.
2. `crates/ailan/src/ui/mod.rs` — `expanded_size` + `expanded_layout` + `collapsed_size` + radii + `pill_rect`. Every pixel target.
3. `crates/ailan/src/app/mod.rs` — `place`, DPI, `SetWindowRgn`, spring targets in `after_change`.
4. `crates/ailan-core/src/state.rs` — priority + `hover_activities`. Tests listed in §8.
5. `crates/ailan/src/ui/tabs.rs` — layout constants baked to a 420-wide page (header x=146, arrow x=395, music y=115).
6. `crates/ailan/src/app/hub_host.rs` + `platform/flight.rs` — second window and fly-to-island end point uses `origin` + pill center. Moving the island without updating the end point breaks the flight.

---

## Unresolved questions

- Should the island stay top-center of the primary monitor, or remember a monitor/origin? Nothing in settings stores it today.
- Is pin only the `--expand` debug flag, or should a user pin survive restart?
- Hover intent: keep 95/340, restore the documented 160/380, or replace both?
- Expanded height: one size for every tab, or keep Music 127 vs Todo 114?
- Drop the 420-layout coordinate space and draw in on-screen DIP, or keep the scale?
- Focus/Pomodoro: delete remaining `BannerIcon::Focus`/`Break` and About-banner reuse, or is a focus session coming back?
- `WM_DPICHANGED` ignores the suggested rect and always jumps to the primary. Is that intended when the cursor is on a secondary monitor?
- Hit region stays an axis-aligned rect, or match the rounded notch (including ear concavity)?
