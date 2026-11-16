# Pha 04 — Hub kéo viền, lưu cỡ và trang

## Context links

- [scout-01-flow.md](scout/scout-01-flow.md) §5–6
- [phase-01-click-drag.md](phase-01-click-drag.md) (field `hub_w`, `hub_h`, `hub_page`)
- [phase-03-grok-visual.md](phase-03-grok-visual.md)

## Overview

- **Ngày:** 2026-11-15
- **Mô tả:** `AiLanAppWindow` resize thật giữa min và max. Lưu DIP và trang cuối. Trang reflow theo client. Pill không có grip.
- **Ưu tiên:** trung bình
- **Implementation status:** pending
- **Review status:** pending

## Key Insights

- Hub tạo với `WS_THICKFRAME` nhưng `handle_hub` `WM_NCHITTEST` chỉ trả `HTCAPTION` hoặc `HTCLIENT` (`hub_host.rs` ~453–468). Viền không nhận kéo.
- `HubController::target_size` chỉ Compact 380×600 hoặc Full 1000×680. `toggle_mode` lật bool.
- Trang vẽ vào một `Rect` (`pages/*/draw`). Full dùng `draw_full` (sidebar), compact dùng `draw_compact` (dock).
- `WM_NCCALCSIZE` trả 0: client = cả cửa sổ. Hit viền phải tự tính, vì non-client mặc định đã bị nuốt.
- Không có `WM_SIZING` clamp hôm nay.

## Requirements

- User kéo viền hub. Min 380×520 DIP. Max = min(1280×900, 90% `rcWork` của monitor đang chứa cửa sổ).
- Dưới 640 DIP rộng → layout dock (`draw_compact`). Từ 640 → sidebar (`draw_full`). Bool `is_compact` suy ra từ rộng, không phải hai cỡ cứng.
- Nút mode nếu còn: snap về 380×600 hoặc 1000×680, rồi user vẫn kéo tiếp được.
- Persist `hub_w`, `hub_h` (DIP) và `hub_page` qua debounce settings 1 s đã có. Mở lại đúng cỡ và trang.
- Island không thêm `HT*SIZE`.
- Đổi cỡ: một `ResizeBuffers` hub, không timer poll.

## Architecture

`WM_NCHITTEST`: nếu điểm nằm trong dải ~6 DIP sát cạnh client → `HTLEFT` / `HTRIGHT` / `HTTOP` / `HTBOTTOM` / góc. Còn lại, `y < TITLE_H` và không trúng control → `HTCAPTION`. Còn lại `HTCLIENT`.

`WM_SIZING`: đọc rect đề xuất, đổi sang DIP bằng `scale_x/scale_y`, kẹp min/max, ghi lại pixel.

`WM_SIZE`: `hub_ctrl` nhận client DIP, `gfx` hub resize, `needs_draw`. Không gọi `place()` của island.

## Related code files

- `D:\project\ailan\crates\ailan\src\app\hub_host.rs` — modify. `handle_hub`: `WM_NCHITTEST`, thêm `WM_SIZING`, `WM_SIZE`/`WM_EXITSIZEMOVE` để lưu.
- `D:\project\ailan\crates\ailan\src\hub\controller.rs` — modify. `target_size`, `toggle_mode`, `draw_full`, `draw_compact`, `is_caption`. Hàm layout theo `(w, h)` thay vì hai hằng duy nhất.
- `D:\project\ailan\crates\ailan\src\hub\pages\today.rs` — modify. `draw` / `draw_bento` / `draw_stacked` nhận bề rộng thật, không giả định 1000 hoặc 380.
- `D:\project\ailan\crates\ailan\src\hub\pages\todos.rs` — modify. `draw` co trong rect.
- `D:\project\ailan\crates\ailan\src\hub\pages\reminders.rs` — modify. `draw` + tab clock không tràn.
- `D:\project\ailan\crates\ailan\src\hub\pages\settings.rs` — modify. `draw` trong rect.
- `D:\project\ailan\crates\ailan\src\hub\pages\ai.rs` — modify. `draw` trong rect.
- `D:\project\ailan\crates\ailan\src\platform\appwindow.rs` — modify. Size tạo cửa sổ lấy settings, không cứng 1000×680.
- `D:\project\ailan\crates\ailan-core\src\settings.rs` — không thêm field mới nếu pha 01 đã thêm `hub_w`, `hub_h`, `hub_page`.

## Implementation Steps

1. `controller.rs`: `apply_size(w, h)` lưu DIP, đặt `is_compact = w < 640`. `draw_*` dùng `w/h` tham số (đã có) nhưng bỏ giả định sidebar chỉ tồn tại ở 1000. `SIDEBAR_W` giữ 220; phần content = `w - sidebar`. Compact: dock cao `DOCK_H`, content phía trên.
2. `handle_hub` `WM_NCHITTEST`: dải viền trước, rồi `is_caption`, rồi client. Xóa comment “no resize borders”.
3. `WM_SIZING`: kẹp. `WM_EXITSIZEMOVE` (và lần đầu mở): ghi `settings.hub_w/hub_h`, `mark_settings_dirty`. Đổi trang (`select_page`) ghi `hub_page`.
4. `appwindow.rs` + chỗ `open_hub`: nếu `hub_w/hub_h` có thì dùng, không thì 1000×680. Kẹp ngay lúc mở (monitor nhỏ).
5. Trang: mỗi `draw` chỉ dùng `r`. `today.rs` chọn bento khi `r.w` đủ, stacked khi hẹp (đã có `draw_stacked`). Không hard-code 1000/380 trong tọa độ nút.
6. `WM_ERASEBKGND` fill đen giữ nguyên để DWM không để dải đục sau resize.
7. Không đụng `WM_NCHITTEST` island.

## Todo list

- [ ] Hit viền hub, caption còn trên title trống
- [ ] `WM_SIZING` min 380×520, max 1280×900 và 90% work area
- [ ] Reflow dock/sidebar theo 640 DIP
- [ ] Năm trang vẽ trong rect thật
- [ ] Lưu DIP + `hub_page`, mở lại đúng
- [ ] Snap nút mode không khóa viền
- [ ] Pill không có size border

## Success Criteria

- Kéo góc hub đổi cả hai chiều, không nhỏ hơn min, không lớn hơn max.
- Dưới 640 thấy dock; rộng hơn thấy sidebar. Chữ không vẽ ra ngoài client.
- Tắt app, mở lại: cùng cỡ và cùng trang.
- File settings cũ thiếu field vẫn mở (default 1000×680, Today).
- Kéo island không bị hub nuốt; hai HWND độc lập.

## Risk Assessment

- `WM_NCCALCSIZE = 0` làm grip dễ lệch DPI. Tính dải bằng DIP rồi nhân scale.
- Trang AI/reminders có control cố định có thể chồng ở 380×520. Co chữ và đẩy phần dư vào scroll sẵn có (`scroll` trên controller), không thêm trang mới.
- `HTCAPTION` trên title vẫn cho system move. Hub được phép (cửa sổ app thường). Island thì không.

## Security Considerations

- Cỡ lưu là số DIP trong settings user. Kẹp lúc đọc để file sửa tay không tạo cửa sổ khổng lồ che desktop.

## Next steps

Pha 05: khớp hit/vẽ, xóa chữ Focus chết, docs, test core, kịch bản bấm tay.
