# Island interaction model

**Recommendation:** click-to-toggle + manual drag (threshold) + content-sized pill inside a **cap-sized HWND**. Keep `WS_EX_NOACTIVATE`. No hover-expand, no `HTCAPTION`, no user-resize.

Fits current design (`docs/system-design.md` §4): popup styles, `SetWindowRgn` = pill, one coalescable timer, focus only for to-do text, raw input only for idle eyes, fullscreen via `ABN_FULLSCREENAPP`.

## Approach

1. **HWND** stays at the **cap** (max card, DIP → px at current monitor DPI). `SetWindowPos` on anchor move, DPI, display change, or cap change — never per animation frame. Swapchain matches HWND (`ResizeBuffers` only then).
2. **Pill** is a centered round-rect inside that HWND. Width target = DirectWrite measure, clamped. Spring (or instant if reduced-motion) updates **region + paint only**.
3. **Hit test:** `SetWindowRgn` = pill. Outside the region, clicks reach the app below. Do not use `HTTRANSPARENT` for that.
4. **Pointer:** pill is `HTCLIENT`. `WM_LBUTTONDOWN` → `SetCapture`. Move past `GetSystemMetricsForDpi(SM_CXDRAG/SM_CYDRAG)` → drag. `WM_LBUTTONUP` without drag → click. `ReleaseCapture` on up / `WM_CAPTURECHANGED`.
5. **Click:** collapsed pill toggles expand. Expanded: `HitTarget` wins; empty chrome toggles collapse; header chrome (not buttons, not the text field) may drag. Ignore a second click within `GetDoubleClickTime()` after a toggle. No `CS_DBLCLKS`.
6. **Drag:** screen delta from `GetCursorPos` (not client coords). `SetWindowPos(..., SWP_NOSIZE|SWP_NOZORDER|SWP_NOACTIVATE)`. On up: clamp pill fully inside nearest monitor `rcWork`; if top is within ~24 DIP of that work-area top, snap to the top edge. Persist anchor once (existing 1s settings debounce).
7. **Focus:** default `WM_MOUSEACTIVATE` → `MA_NOACTIVATE` (belt + `WS_EX_NOACTIVATE`). Text field only: drop `WS_EX_NOACTIVATE`, `SetWindowPos(SWP_FRAMECHANGED|SWP_NOACTIVATE)`, `SetForegroundWindow` **inside** that click, then accept keys. `WM_KILLFOCUS` / `WM_ACTIVATE` deactivate → restore `NOACTIVATE` + `SWP_FRAMECHANGED|SWP_NOACTIVATE`. Keep `WS_EX_TOOLWINDOW` always.
8. **Idle:** no hover timer, no `GetCursorPos` poll, no `RIDEV_INPUTSINK` except eyes mode. Width measure only when the string or DPI changes.

## Why this one

| Choice | Rejected | Why |
|---|---|---|
| Click, not hover ≥160ms | Hover-intent | A top-edge pill sits on the path to browser tabs. Click matches "do not steal focus." |
| Manual move | `WM_NCHITTEST` → `HTCAPTION` | Caption-down enters the system move loop (`WM_ENTERSIZEMOVE`). Dblclk caption → maximize/`SC_RESTORE`. Threshold vs toggle is harder. Loop can disturb no-activate. |
| Region, not resize borders | `HT*SIZE` | Borders must sit **inside** the region or they are not hittable. A 34px pill has no room. User size fights content width (ellipsis vs override). YAGNI until a surface truly needs it. |
| Cap HWND + region | `SetWindowPos` every spring frame | Resize reallocates the swapchain and fights drag. Region + draw is enough. |

## Topic notes

### 1. Click without taking focus

`WS_EX_NOACTIVATE`: a click does not foreground the popup; another app's minimize/close also will not foreground it. `DefWindowProc` answers `WM_MOUSEACTIVATE` with `MA_NOACTIVATE`, so `WM_LBUTTONDOWN/UP` still arrive (they are not eaten). Return `MA_NOACTIVATE` yourself so a style race cannot activate.

Non-client: only if `WM_NCHITTEST` returns something other than `HTCLIENT`. Stay `HTCLIENT` so input is `WM_LBUTTON*`, not `WM_NCLBUTTONDOWN`.

Keyboard stays with the foreground app. Escape, accelerators, and IME do not arrive. That is correct for media/focus/toggles.

**When focus is required:** to-do field (and any later text). `WM_CHAR` is not enough for Vietnamese IME — composition needs a focused window (edit control or TSF) for the duration of the edit. Activation must be the same click (`SetForegroundWindow` from the button handler); a later timer often fails the foreground lock. Restore `NOACTIVATE` as soon as the field blurs so the island does not stay a keyboard target. Child buttons do not need this.

`ShowWindow(SW_SHOWNOACTIVATE)` on reveal (already used).

### 2. Drag vs click

- Threshold = system drag size **for this DPI**, not a hardcoded 4px.
- One pointer owner: capture. If capture is lost, cancel drag and **do not** toggle.
- Suppress toggle if a drag started. A pure click never calls `SetWindowPos`.
- Multi-monitor: virtual-screen `GetCursorPos` + `MonitorFromPoint(MONITOR_DEFAULTTONEAREST)`. During drag, ignore `WM_DPICHANGED`'s suggested rect (it fights the cursor); apply DPI scale, keep following the cursor. Re-clamp on `WM_DISPLAYCHANGE` / `WM_DPICHANGED` when not dragging.
- Off-screen: never persist a rect that fails the work-area clamp. Missing monitor → primary top-center.
- Persist monitor device name (`szDevice`) + anchor as **fractions of that monitor's work area**, plus a DIP offset from the top. Raw virtual-screen pixels break on dock/DPI change.
- Snap is magnetic on drop, not a live edge stick (live stick makes side monitors painful).

### 3. Resizable shell — do not build it

`SetWindowRgn` clips hit-testing to the region. `HTLEFT` / `HTBOTTOMRIGHT` outside the pill never see the pointer. `HTTRANSPARENT` only continues the search among windows **on the same thread** — it does not pass clicks to other processes.

Per-pixel alpha hit-testing is an `UpdateLayeredWindow` behavior. This window is `WS_EX_NOREDIRECTIONBITMAP` + DirectComposition; DComp alpha is not a hit mask.

If a future tool truly needs resize: drop the "always cap" assumption, put a grip **inside** the region, handle `WM_SIZING` yourself, clamp DIP min (collapsed pill) and max (~40% of `rcWork`), persist DIP size. Do not mix that with content-driven width without an explicit rule (user size wins → ellipsis). Not this iteration.

### 4. Dynamic width, ~0 idle CPU

- On text/DPI change: cached `IDWriteTextLayout` (or `DetermineMinWidth`). Do not recreate the layout per frame. Ellipsis at the cap.
- Target width = clamp(padding + measured, pill min, cap). Set the spring target **once**.
- While the spring runs, existing animation wake redraws and, if the rounded rect changed by ≥1 physical px, `SetWindowRgn` (it consumes the `HRGN`). Skip identical rects.
- When the spring settles, region is stable. No timer is added. Clock / pomodoro / OSD still use the **one** `SetCoalescableTimer` deadline.
- Reduced motion: one region update, one paint, no spring ticks.
- Banner longer than the cap ellipsizes. The HWND does not grow past the cap.

### 5. Edge cases

- **Fullscreen:** `ABN_FULLSCREENAPP` hide. Release capture, cancel drag, unregister raw input, do not expand. Show restores the anchor. Startup: `SHQueryUserNotificationState` (already specified).
- **Eyes:** `RegisterRawInputDevices` + `RIDEV_INPUTSINK` only in that state (the island is never focused, so without `INPUTSINK` you get no `WM_INPUT`). Unregister on any other state. Redraw only when the quantized gaze bucket changes — not on every `WM_INPUT`.
- **Cross-monitor drag:** covered above. Drop commits one clamp + one persist.
- **Transparent pixels:** region, not alpha. Inset the region ~1px from the anti-aliased rim so the fuzzy edge is not a click trap. Rebuild the region only when geometry changes.
- **Double-click:** no `CS_DBLCLKS` (no dblclk → maximize path). Eat the second click inside `GetDoubleClickTime()` after a toggle so expand does not instantly collapse. Expand **downward**; keep the cursor on header chrome, not on a new button.
- **Reduced motion:** already a setting. Snap geometry; drag stays 1:1 (no throw animation).
- **Right-click on the island:** do not `TrackPopupMenu` there; the menu activates an owner and drops no-activate. Keep the menu on the tray icon.

## Tradeoffs

- Cap-sized swapchain stays allocated while collapsed (~card × 4 bytes, fine vs the 20–30 MB budget). Benefit: expand never reallocates mid-spring.
- No click-outside-to-collapse without a low-level hook or a full-screen catcher (both steal input or burn idle wakeups). Collapse = chrome click, or the existing deadline (OSD 1.5s, etc.).
- Text entry **does** take focus, briefly, on purpose. The rest of the island never does.
- Magnetic top snap can annoy someone who wanted the pill mid-screen. Clamp still allows any on-screen spot if the snap band is only ~24 DIP.
- `SetWindowRgn` during the spring is real work, but only while animating, and only when the pixel rect changes.

## Citations

- Extended styles `WS_EX_NOACTIVATE`, `WS_EX_TOOLWINDOW`, `WS_EX_NOREDIRECTIONBITMAP`: [Extended Window Styles](https://learn.microsoft.com/en-us/windows/win32/winmsg/extended-window-styles)
- `WM_MOUSEACTIVATE` / `MA_NOACTIVATE` vs `MA_NOACTIVATEANDEAT`: [WM_MOUSEACTIVATE](https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-mouseactivate)
- `WM_NCHITTEST` hit codes (`HTCLIENT`, `HTCAPTION`, `HT*SIZE`, `HTTRANSPARENT` same-thread): [WM_NCHITTEST](https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-nchittest)
- `SetWindowRgn` (system deletes the region; outside = not hit): [SetWindowRgn](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowrgn)
- `SetWindowPos` `SWP_NOACTIVATE`: [SetWindowPos](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos)
- Drag threshold `SM_CXDRAG` / `GetSystemMetricsForDpi`: [GetSystemMetrics](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getsystemmetrics)
- `WM_DPICHANGED` suggested rect: [WM_DPICHANGED](https://learn.microsoft.com/en-us/windows/win32/hidpi/wm-dpichanged)
- `SetCoalescableTimer`: [SetCoalescableTimer](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setcoalescabletimer)
- Raw input `RIDEV_INPUTSINK`: [RegisterRawInputDevices](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerrawinputdevices)
- `ABN_FULLSCREENAPP`: [SHAppBarMessage](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shappbarmessage)
- DirectWrite layout / min width: [IDWriteTextLayout](https://learn.microsoft.com/en-us/windows/win32/api/dwrite/nn-dwrite-idwritetextlayout)

## Unresolved questions

- Vietnamese to-do: in-place `EDIT` vs manual TSF while temporarily activated? `WM_CHAR` alone drops IME composition.
- Is exclusive fullscreen (FSE) reliably delivering `ABN_FULLSCREENAPP`, or do some games need a fallback that still is not a poll?
- Product: may the pill rest anywhere on the work area, or is it always a top-edge notch with horizontal drag only?
- Persist `szDevice` vs a stable monitor id across dock/undock (device name can change).
- After hours collapsed, is shrinking the HWND once (memory) worth a single `ResizeBuffers` on the next expand? Not required for CPU.
- Touch/pen: mouse promotion is enough, or do hit targets need `WM_POINTER*`? Unverified on hardware.
