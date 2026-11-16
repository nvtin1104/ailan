# Pha 03 — Token Grok, bỏ tai mắt sóng

## Context links

- [researcher-02-grok-ui.md](research/researcher-02-grok-ui.md)
- [scout-01-flow.md](scout/scout-01-flow.md) §7
- [phase-02-geometry.md](phase-02-geometry.md)

## Overview

- **Ngày:** 2026-11-15
- **Mô tả:** Nền than, hairline, capsule. Xóa tai lõm, mắt nhìn, thanh audio. Cùng token cho hub. Không clone chat Grok.
- **Ưu tiên:** trung bình (sau khi hit và size đúng)
- **Implementation status:** pending
- **Review status:** pending

## Key Insights

- `BG #000000`, radius 14/29, `EAR_*`, `fill_notch` tạo dáng Dynamic Island.
- `PANEL_BG` / `SURFACE` dead. Hub có palette riêng (`hub/widgets.rs`), có accent cam/mint.
- Mắt: `sync_raw_mouse` + `update_gaze` + `eye_offset`, chỉ lúc `Collapsed::Minimal`.
- Sóng: `audio_bars`, `sync_audio_timer`, `T_AUDIO` 30 ms. Đó là wake lúc phát nhạc, trái mục tiêu CPU nghỉ.
- Giả định sản phẩm: **bỏ** mắt và sóng. Idle thu = capsule 150×32, một glyph hoặc nhãn ngắn. Không đăng ký raw mouse.

## Requirements

- Token (hex xấp xỉ): bg `#141414`, surface `#212121`, text `#FCFCFC`, muted `#9E9E9E`, hairline `#2A2A2A`. Không fill cam. Không bóng ở pha này.
- Collapsed radius = cao/2. Expanded radius 16. Banner cùng capsule (cao 34 → radius 17).
- Hover chỉ đổi hairline lên `#3A3A3A`. Không đổi size.
- Xóa tai. `fill_notch` không còn đường lõm; vẽ round-rect thường.
- Xóa gaze và audio bars khỏi island thu và header expanded.
- Hub dùng cùng bg/surface/text/muted/hairline. Giữ layout hub, không thêm bubble, composer, avatar, wordmark.
- Chữ: nhãn 13 semibold, meta 11, title expanded 15. Segoe UI Variable. Không webfont.
- Pad: thu 14×6, expanded 16.

## Architecture

Một module màu trong `ui/mod.rs` (hằng `u32`). Hub `widgets.rs` trỏ cùng hằng, không copy hex lần hai. `hover_t` chỉ là hệ số màu viền.

Idle `Collapsed::Minimal`: vẽ một glyph (ví dụ chấm hoặc `G_HOME` nhỏ) hoặc chuỗi ngắn đã có i18n. Không `RegisterRawInputDevices`.

## Related code files

- `D:\project\ailan\crates\ailan\src\ui\mod.rs` — modify. Palette, radius, xóa `ear_for`, `eye_offset`, `EAR_*`. `draw` bỏ `fill_notch` tai và `audio_bars`.
- `D:\project\ailan\crates\ailan\src\ui\collapsed.rs` — modify. Capsule + nhãn/glyph. Banner một dòng màu mới.
- `D:\project\ailan\crates\ailan\src\ui\tabs.rs` — modify. Bỏ wave header. Nút/chữ theo pad 16, radius control ≤ 16.
- `D:\project\ailan\crates\ailan\src\gfx\mod.rs` — modify. `fill_notch` thành fill bo hoặc để caller gọi `fill_round`. Hairline 1 px.
- `D:\project\ailan\crates\ailan\src\app\notch.rs` — modify. Xóa `sync_raw_mouse`, `update_gaze`.
- `D:\project\ailan\crates\ailan\src\app\effects.rs` — modify. Xóa `sync_audio_timer` và lerp `audio_bars`.
- `D:\project\ailan\crates\ailan\src\app\mod.rs` — modify. Xóa field `audio_bars`, `raw_mouse`, timer `T_AUDIO`. `after_change` không gọi hai sync đó.
- `D:\project\ailan\crates\ailan\src\hub\widgets.rs` — modify. Token nền/chữ/hairline. Bỏ fill cam trên control mặc định.
- `D:\project\ailan\crates\ailan\src\ui\mod.rs` test `gaze_tests` — delete cùng `eye_offset`.

## Implementation Steps

1. Đổi hằng màu. Xóa `PANEL_BG`/`SURFACE` dead hoặc gán lại đúng token và dùng chúng (surface = well expanded).
2. `radius_for`: expand 0 → `h/2`; expand 1 → 16. Xóa nhánh ear. Region pha 02 dùng cùng radius.
3. `collapsed.rs`: Minimal không vẽ mắt. Một glyph căn giữa hoặc label ngắn, màu text, meta muted nếu có dòng phụ. Không animation idle.
4. `notch.rs`: gỡ đăng ký raw input mọi nhánh, kể cả `set_hidden`.
5. `effects.rs` + `mod.rs`: gỡ `T_AUDIO` / `AUDIO_FRAME_MS`. Nhạc mở rộng không còn sóng nên không cần tick 30 ms. Progress nhạc vẫn theo scheduler core (1 s khi tab Music mở).
6. `tabs.rs`: xóa `draw_wave_bars`. Header còn giờ, pin, nút home, nút tab. Hit không đổi contract `Action`.
7. `gfx`: stroke hairline `#2A2A2A`. Hover brighten lên `#3A3A3A` trên island. Không thêm shadow.
8. `widgets.rs` + backdrop hub: bg `#141414`, card `#212121`, chữ `#FCFCFC` / `#9E9E9E`, viền `#2A2A2A`. Accent mint/amber của flight (`FLY_TODO`, `FLY_REMIND`) giữ cho icon bay, không đổ lên nền pill.
9. Xóa test gaze. Không thêm test pixel.

## Todo list

- [ ] Token island + hub một nguồn
- [ ] Capsule radius, expanded 16, không tai
- [ ] Hover chỉ hairline
- [ ] Xóa mắt + raw mouse
- [ ] Xóa audio bars + timer 30 ms
- [ ] Idle 150×32 một glyph hoặc nhãn
- [ ] Không bubble / composer / wordmark

## Success Criteria

- Pill thu nhìn là viên than có viền, không tai, không mắt, không sóng.
- Đứng yên trên desktop: không `WM_INPUT`, không timer audio.
- Hub và island cùng nền/chữ/viền khi mở cạnh nhau.
- Click và kéo pha 01 vẫn đúng sau khi đổi vẽ.

## Risk Assessment

- `#2A2A2A` trên `#141414` yếu. Không đoán. Pha 05 chụp rồi mới nâng `#3A3A3A`.
- Xóa mắt làm `Collapsed::Minimal` trống nếu quên glyph. Bắt buộc còn một dấu.
- Hub đang acrylic. Token đặc có thể đánh nhau với blur. Giữ acrylic hiện có, chỉ đổi màu brush. Không thiết kế vật liệu mới.

## Security Considerations

- Gỡ `RIDEV_INPUTSINK` hết. Process không còn nhận chuột toàn màn hình.

## Next steps

Pha 04 resize hub trong khoảng min/max, cùng token vừa gắn.
