# Luồng island kiểu Grok

**Ngày:** 2026-11-15  
**Trạng thái:** pending  
**Tiến độ:** 0%  
**Phạm vi:** dựng lại tương tác island, hình học pill, token Grok, resize hub, test và docs. Plan không sửa source.

## Pha

| Pha | File | Trạng thái | Tiến độ | Phụ thuộc |
|---|---|---|---|---|
| 01 Click / kéo | [phase-01-click-drag.md](phase-01-click-drag.md) | pending | 0% | không |
| 02 Hình học | [phase-02-geometry.md](phase-02-geometry.md) | pending | 0% | 01 (`place`, không hover) |
| 03 Visual Grok | [phase-03-grok-visual.md](phase-03-grok-visual.md) | pending | 0% | 02 (một layout, một cao) |
| 04 Resize hub | [phase-04-hub-resize.md](phase-04-hub-resize.md) | pending | 0% | field `Settings` thêm ở 01 |
| 05 UI, docs, test | [phase-05-ui-fixes-docs-tests.md](phase-05-ui-fixes-docs-tests.md) | pending | 0% | 01–04 |

Pha 01 thêm field settings (neo + kích thước hub) một lần. Pha 04 chỉ ghi/đọc. Pha 03 mới xóa mắt và thanh audio.

## Phụ thuộc

- Logic + test: `crates/ailan-core`. Win32/D2D: `crates/ailan`.
- Một `SetCoalescableTimer`. Không poll. Không low-level mouse hook.
- `SetWindowRgn` là mặt nạ hit. HWND giữ kích thước cap. Spring chỉ đổi region + paint.

## Giả định (đã chốt)

- Hover không mở nội dung. Click mới toggle. Leave không thu. Không `HTCAPTION`, không `HT*SIZE` trên pill.
- Phóng to / thu nhỏ là cửa sổ hub, không phải grip trên pill 32px.
- Hex là xấp xỉ. Nghỉ 150×32. Banner một dòng, cao 34, rộng kẹp 150–420.
- Bỏ mắt nhìn và thanh audio lúc thu. Capsule yên, vẫn 150×32, không đăng ký raw mouse.
- Neo mặc định = giữa mép trên `rcWork`. Lưu tên monitor + phân số work area.
- IME mặc định: `EDIT` con tại chỗ. TSF để sau.
- Không hồi `pin_todo`, priority, Pomodoro, tab Focus.

## Câu hỏi còn mở

1. Exclusive fullscreen có luôn gửi `ABN_FULLSCREENAPP` không? Chưa thêm poll.
2. Hairline `#2A2A2A` có mất trên wallpaper tối không? Chỉ đổi `#3A3A3A` hoặc bóng nhẹ sau screenshot thật.
