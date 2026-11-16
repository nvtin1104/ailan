# Pha 01 — Click toggle và kéo tay

## Context links

- [researcher-01-interaction.md](research/researcher-01-interaction.md)
- [scout-01-flow.md](scout/scout-01-flow.md) §1–2
- [plan.md](plan.md)

## Overview

- **Ngày:** 2026-11-15
- **Mô tả:** Bỏ hover-expand và leave-collapse. Click mới mở/đóng. Kéo sau ngưỡng hệ thống. Lưu neo theo work area. Fullscreen hủy kéo.
- **Ưu tiên:** cao (mọi pha sau dựa vào input đúng)
- **Implementation status:** pending
- **Review status:** pending

## Key Insights

- Timer hiện tại: `T_HOVER` 95 ms, `T_COLLAPSE` 340 ms (`app/mod.rs`). Docs nói 160/380. Cả hai đều sai hướng sản phẩm.
- `WM_LBUTTONDOWN` gọi `on_action` ngay (`notch.rs`). Không có capture, không phân biệt click và kéo.
- `place()` luôn `MonitorFromPoint(0,0)` + `rcMonitor.top`, `scale = dx/96`, bỏ `dy`. Không có field vị trí trong `Settings`.
- `collapse()` no-op khi `pinned` (chỉ `--expand`). Giữ cờ CLI. Không thêm pin vào settings.
- Menu chuột phải nằm ở tray (`WM_APP_TRAY`). Không đưa `TrackPopupMenu` lên island.

## Requirements

- Hover không đổi bề rộng, cao, hay nội dung. Leave không gọi `collapse()`.
- Click lên chrome thu gọn → `expand()`. Click chrome rỗng lúc mở → `collapse()`. Nút vẫn `on_action`.
- Kéo chỉ sau `GetSystemMetricsForDpi(SM_CXDRAG/SM_CYDRAG)`. Delta lấy `GetCursorPos`, không lấy client.
- `WM_MOUSEACTIVATE` trả `MA_NOACTIVATE`. Giữ `WS_EX_NOACTIVATE` + `WS_EX_TOOLWINDOW`.
- Nuốt click thứ hai trong `GetDoubleClickTime()` sau mỗi toggle. Không `CS_DBLCLKS`. Không `HTCAPTION`.
- Focus chỉ khi bấm ô chữ. IME tiếng Việt: `EDIT` con. TSF không làm pha này.
- Thả chuột: pill nằm trọn trong `rcWork`. Cách mép trên ≤ 24 DIP thì dính mép trên. Lưu `szDevice` + phân số. Monitor mất → primary, top-center.
- Đang kéo: bỏ rect gợi ý của `WM_DPICHANGED`. `ABN_FULLSCREENAPP` thả capture, hủy kéo, không expand.
- Field mới có `#[serde(default)]`. Test legacy và partial settings vẫn xanh.

## Architecture

Một state pointer trên `App`. HWND không đổi size trong pha này (pha 02 mới siết cap). `place()` là nơi duy nhất đặt origin lúc không kéo.

```text
LBUTTONDOWN (trong region):
  nếu now < eat_until: return
  SetCapture
  ptr = Down { cursor, origin, dragging: false }

MOUSEMOVE khi đang capture:
  nếu !dragging và |dx|>=cxdrag hoặc |dy|>=cydrag: dragging = true
  nếu dragging:
    SetWindowPos(origin + delta, SWP_NOSIZE|SWP_NOZORDER|SWP_NOACTIVATE)

LBUTTONUP:
  ReleaseCapture
  nếu dragging: clamp pill vào rcWork; snap top nếu ≤24 DIP; ghi settings
  nếu không: hit nút → on_action; chrome rỗng mở → collapse; thu → expand
             eat_until = now + GetDoubleClickTime()

CAPTURECHANGED khi chưa UP: ptr = Idle, không toggle

MOUSELEAVE: xóa hover vẽ; không collapse
```

DPI: `scale_x = dx/96`, `scale_y = dy/96`. Trên monitor thường hai giá trị bằng nhau. Dùng cả hai để chiều dọc không lệch khi `dy != dx`.

## Related code files

- `D:\project\ailan\crates\ailan\src\app\mod.rs` — modify. Xóa `T_HOVER`, `T_COLLAPSE`, `HOVER_INTENT_MS`, `COLLAPSE_DELAY_MS`. Sửa `place`, `after_change` (không arm hover). Thêm state kéo + `eat_until`.
- `D:\project\ailan\crates\ailan\src\app\notch.rs` — modify. `expand`, `collapse`, `begin_editing`, `stop_editing`, `set_hidden`, wndproc chuột/timer/DPI/appbar.
- `D:\project\ailan\crates\ailan-core\src\settings.rs` — modify. `Settings`: `monitor: Option<String>`, `anchor_x/anchor_y: Option<f32>`, `hub_w/hub_h: Option<f32>`, `hub_page: HubPage`. `Default` + `#[serde(default)]` sẵn có trên struct.
- `D:\project\ailan\crates\ailan\src\main.rs` — modify chỉ nếu class style có `CS_DBLCLKS` (bỏ). Không thêm hook.

## Implementation Steps

1. `settings.rs`: thêm field optional ở trên. `Default`: monitor/anchor/hub size = `None` (nghĩa là chưa kéo / hub dùng 1000×680), `hub_page = Today`. Không đụng `pin_todo`, priority, pomodoro. `de_tab` giữ nguyên.
2. `mod.rs` `place()`: `MONITORINFOEXW` lấy `szDevice`. Có `monitor` + anchor → `EnumDisplayMonitors` khớp tên, không thấy thì primary. Dùng `rcWork`. Origin = pill theo phân số (tâm X, mép trên Y). Chưa có anchor → giữa `rcWork`, `y = rcWork.top`. `SetWindowPos(..., SWP_NOACTIVATE)`. Ghi `origin`. `scale_x/scale_y` từ `GetDpiForMonitor`. Đừng nhảy về primary khi user đã kéo.
3. `notch.rs` `WM_DPICHANGED | WM_DISPLAYCHANGE`: đang kéo thì chỉ cập nhật scale, bỏ `lParam` rect, tiếp tục theo con trỏ. Không kéo thì `place()` lại từ anchor đã lưu, rồi clamp.
4. Xóa nhánh `T_HOVER` / `T_COLLAPSE`. `WM_MOUSEMOVE`: `TrackMouseEvent(TME_LEAVE)` chỉ để biết hover vẽ. Không `SetTimer`.
5. `WM_MOUSELEAVE`: `mouse_inside = false`, hover về 0. Không `collapse()`.
6. Thay `WM_LBUTTONDOWN` bằng state machine ở trên. `WM_LBUTTONUP` và `WM_CAPTURECHANGED` đủ cặp. Ngưỡng = `GetSystemMetricsForDpi`.
7. `expand()` / `collapse()` giữ việc chọn tab và `after_change()`. `collapse()` vẫn return nếu `pinned`. Click lúc pin không đóng (hành vi CLI cũ).
8. `set_hidden(true)` (fullscreen): `ReleaseCapture`, `ptr = Idle`, không expand. Show lại không đổi anchor.
9. `begin_editing()`: tạo/hiện `EDIT` con đúng rect ô to-do. Trong cùng click: gỡ `WS_EX_NOACTIVATE`, `SetWindowPos(SWP_FRAMECHANGED|SWP_NOACTIVATE)`, `SetForegroundWindow`, `SetFocus(edit)`. `stop_editing` / `WM_KILLFOCUS` / mất focus edit: trả `NOACTIVATE`, `SWP_FRAMECHANGED|SWP_NOACTIVATE`, giữ `TOOLWINDOW`. Enter trên edit gọi `parse_input` như `on_char` hôm nay. Không dựa `WM_CHAR` cho IME.
10. Island `WM_RBUTTONUP`: không menu. Tray giữ nguyên.
11. `WM_MOUSEACTIVATE`: `MA_NOACTIVATE` mọi click không phải ô chữ.

## Todo list

- [ ] Xóa timer hover/leave
- [ ] State machine click vs kéo + ngưỡng DPI
- [ ] `place()` theo `rcWork` + anchor
- [ ] Clamp + snap 24 DIP + persist
- [ ] Fullscreen hủy kéo
- [ ] `EDIT` con, trả `NOACTIVATE` khi blur
- [ ] Nuốt double-click sau toggle
- [ ] Field settings default, không phá test cũ

## Success Criteria

- Rê qua pill không mở, rời chuột không đóng.
- Click một lần mở; click chrome đóng; click nút không kéo.
- Kéo quá ngưỡng không toggle. Thả capture giữa chừng không toggle.
- Sau restart, pill ở monitor và phân số đã thả. Monitor rút → primary top-center.
- OSD/banner vẫn tự hết theo deadline cũ (`T_TICK`), không theo chuột.
- CPU nghỉ không thêm timer mới.

## Risk Assessment

- `SetForegroundWindow` sau khi đã nhả style có thể thất bại nếu không còn trong cùng click. Không dùng timer trễ để cướp focus.
- Snap 24 DIP có thể kéo pill đang để sát mép. Chỉ snap lúc thả, không dính sống.
- `szDevice` đổi sau dock: coi như monitor mất, về primary. Đã chốt, không tìm id bền hơn.

## Security Considerations

- Không cài `WH_MOUSE_LL` / `RIDEV_INPUTSINK` cho kéo. Raw mouse mắt để pha 03 gỡ.
- Không lưu rect ngoài work area.
- `EDIT` là con của process mình. Settings vẫn file user, field thiếu = default.

## Next steps

Pha 02: HWND cap, rộng banner đo chữ, một hệ tọa độ. `place()` đã có origin đúng để flight dùng.
