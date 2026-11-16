# Pha 05 — Khớp UI, docs, test

## Context links

- [scout-01-flow.md](scout/scout-01-flow.md) §7–8 và mục system-design
- [phase-01-click-drag.md](phase-01-click-drag.md) … [phase-04-hub-resize.md](phase-04-hub-resize.md)
- `D:\project\ailan\docs\system-design.md` §4
- `D:\project\ailan\docs\optimization.md` dòng swapchain

## Overview

- **Ngày:** 2026-11-15
- **Mô tả:** Hit trùng hình vẽ, xóa palette chết và chữ Focus, docs khớp hành vi mới, test core giữ xanh, kịch bản thử tay.
- **Ưu tiên:** cao trước khi nhận là xong
- **Implementation status:** pending
- **Review status:** pending

## Key Insights

- Region cũ là chữ nhật + tai trong khi vẽ bo lõm: góc vuông vẫn ăn hover.
- Hit banner lớn hơn glyph. `hit_test` ưu tiên nút hơn pill, giữ quy tắc đó.
- Comment Focus còn ở `tabs.rs`, `collapsed.rs`, `state.rs`, banner About trong `effects.rs`. Tab chỉ Music | Todo.
- `docs/system-design.md` §4: hover 160 ms, top-center cố định, pill 240/250, ba tab kể cả Tập trung, tray mở notch 4 s. Code tray đã toggle hub.
- `optimization.md` swapchain ≈ 440×190. Cap mới 560×128 DIP.
- Không có test nào assert 95 ms hay pixel. Vỡ nếu đổi thứ tự `collapsed()` hoặc deadline scheduler.

## Requirements

- Vùng hit = vùng bo đã vẽ (đã thụt 1 px ở pha 02). Nút banner/todo trùng rect vẽ, không `inset` lớn hơn glyph đến mức nuốt cả pill.
- Xóa hằng màu không dùng, hoặc chúng chính là token pha 03.
- Sửa comment Focus khi đụng file. Không dựng lại tab Focus, Pomodoro, `pin_todo`.
- `BannerIcon::Focus` / `Break`: chỉ xóa hoặc đổi comment nếu dòng đó được sửa. About không được vẽ session Focus giả.
- Sửa §4 và một dòng swapchain. Sửa bảng §5 cho hết “thời lượng Pomodoro” và ghi field neo/hub, nếu không bảng storage nói dối.
- Không thêm file docs mới.
- Test core liệt kê dưới phải xanh. Không đổi assertion cho hợp code mới nếu nghĩa sản phẩm không đổi.

## Architecture

Thứ tự `collapsed()` giữ nguyên: OSD, banner, countdown, stopwatch, music, todo, minimal. Rộng chỉ là việc của crate `ailan`.

Scheduler: expanded vẫn deadline phút; minimal không wake; progress nhạc chỉ khi expanded + tab Music.

## Related code files

- `D:\project\ailan\crates\ailan\src\ui\mod.rs` — modify. `hit_test`, xóa palette chết.
- `D:\project\ailan\crates\ailan\src\ui\collapsed.rs` — modify. Hit nút = rect vẽ. Comment đầu file.
- `D:\project\ailan\crates\ailan\src\ui\tabs.rs` — modify. Comment “Music, Focus, Todo”.
- `D:\project\ailan\crates\ailan\src\app\effects.rs` — modify. About banner nếu còn icon Focus.
- `D:\project\ailan\crates\ailan-core\src\state.rs` — modify comment “Focus wins” / thứ tự. Không đổi `collapsed()` / `hover_activities` trừ khi comment sai lệch code.
- `D:\project\ailan\docs\system-design.md` — modify §4 và bảng §5.
- `D:\project\ailan\docs\optimization.md` — modify dòng swapchain và câu Pomodoro tick nếu còn nói tab Tập trung.
- `D:\project\ailan\crates\ailan-core\src\storage.rs` — không đổi test; chạy để chứng minh field mới default.

## Implementation Steps

1. So khớp từng `Hit` đẩy trong `draw` với hình. Banner snooze/done: rect hit = rect nút. Pill `Expand` / `DismissBanner` vẫn phía sau để nút thắng.
2. Xóa `allow(dead_code)` bằng cách dùng token hoặc xóa hằng.
3. `effects.rs` About: bỏ banner “Focus”. Giới thiệu chỉ chữ app.
4. Comment `state.rs` ~182 và ~330, `tabs.rs` dòng 1, `collapsed.rs` dòng 1: Music và Todo, không Focus.
5. `system-design.md` §4 viết lại ngắn:
   - Click mở/đóng; leave không đóng; kéo sau ngưỡng; `MA_NOACTIVATE`.
   - Vị trí mặc định top-center `rcWork`; sau kéo thì nhớ monitor + phân số.
   - HWND cap 560×128; pill nghỉ 150×32; banner/OSD cao 34, rộng 150–420.
   - Tab mở: Nhạc · Việc cần làm. Không Tập trung.
   - Tray trái: bật/tắt hub. Phải: menu.
   - Ô chữ: `EDIT` con, trả `NOACTIVATE` khi blur.
   - Idle thu: capsule yên, không mắt, không raw mouse.
6. §5: settings = ngôn ngữ, tab, giảm chuyển động, neo tùy chọn, cỡ hub, trang hub. Bỏ thời lượng Pomodoro.
7. `optimization.md`: swapchain = cap 560×128 DIP, không 440×190. Xóa câu tick Pomodoro / tab Tập trung. Giữ một coalescable timer, EcoQoS khi thu.
8. Chạy test crate `ailan-core` (người implement, không phải plan). Danh sách phải xanh:
   - `storage::legacy_island_settings_are_ignored_not_fatal`
   - `storage::partial_settings_use_defaults`
   - `state::idle_rests_in_minimal`
   - `state::music_ranks_above_todo`
   - `state::clock_tools_rank_above_music_and_ring_as_banners`
   - `state::todo_only_live_while_today_has_unfinished_work`
   - `state::first_volume_event_is_silent_then_osd`
   - `state::low_battery_and_headphones_banners`
   - `state::todo_reminder_fires_once`
   - `scheduler::expanded_header_clock_ticks_each_minute`
   - `scheduler::minimal_and_hidden_idle_never_wake`
   - `scheduler::media_progress_ticks_only_on_music_tab`
   - `scheduler::todo_island_wakes_at_midnight`
   - `scheduler::alarms_and_timers_wake_even_when_hidden_and_tick_seconds_when_shown`
9. Kịch bản tay (không tự động):
   - Rê ngang pill trên đường lên tab trình duyệt: không mở.
   - Click mở, click nền đóng, click lần hai trong double-click không đóng ngay.
   - Kéo 2 px: coi là click. Kéo quá ngưỡng: cả pill đi, thả không toggle.
   - Thả sát mép trên: dính. Thả giữa màn: ở lại. Kéo sang màn hai, restart: còn đó.
   - Rút màn đó: về primary top-center.
   - Banner dài: rộng tới 420 rồi `...`. Banner ngắn: không dưới 150.
   - Fullscreen game/video: island ẩn, kéo đang dở bị hủy.
   - Gõ tiếng Việt có dấu trong ô to-do, blur xong phím không vào island.
   - Hub kéo nhỏ tới min, lớn tới max, tắt mở lại đúng trang.
   - Nhìn hairline trên wallpaper tối. Nếu mất: mới đổi `#3A3A3A` hoặc bóng `0 8px 24px rgba(0,0,0,.45)`. Ghi kết quả vào review, đừng đoán trước.

## Todo list

- [ ] Hit = hình vẽ
- [ ] Palette chết hết
- [ ] Comment / About không còn Focus
- [ ] §4, §5, optimization.md
- [ ] Test core xanh
- [ ] Chạy kịch bản tay, ghi hairline

## Success Criteria

- Không còn câu docs nói hover 160 ms, pill cố định 240/250, tab Tập trung, tray mở notch 4 s, swapchain 440×190.
- Test liệt kê pass mà không nới assertion.
- Kịch bản tay không còn mở nhầm khi rê, không còn nhảy cao khi đổi tab.

## Risk Assessment

- Sửa `collapsed()` “cho sạch” sẽ đỏ test priority. Cấm.
- Đổi `#3A3A3A` trước khi chụp là đoán. Chỉ sau mục 9.

## Security Considerations

- Docs không được hướng dẫn cài hook chuột. Giữ câu “region là hit mask, không low-level hook”.

## Next steps

Hết plan. Implement lần lượt 01 → 05. Câu hỏi mở chỉ còn fullscreen exclusive và màu hairline sau ảnh thật.
