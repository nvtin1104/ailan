# Pha 02 — Hình học cap, banner, một layout

## Context links

- [researcher-01-interaction.md](research/researcher-01-interaction.md) §4
- [researcher-02-grok-ui.md](research/researcher-02-grok-ui.md) §3
- [scout-01-flow.md](scout/scout-01-flow.md) §3–4, §7
- [phase-01-click-drag.md](phase-01-click-drag.md)

## Overview

- **Ngày:** 2026-11-15
- **Mô tả:** HWND đứng yên ở cap. Pill đổi rộng bằng region + vẽ. Một không gian DIP. Mọi tab một chiều cao. Điểm đáp flight theo origin mới.
- **Ưu tiên:** cao
- **Implementation status:** pending
- **Review status:** pending

## Key Insights

- `expanded_size` (380×114/127) và `expanded_layout` (420) lệch nhau. `draw` scale theo `380/420`. Cao Music 127 vs Todo 114.
- `collapsed_size`: nghỉ 150×30, OSD 240×34, banner 250×34. Rộng không đo chữ.
- `update_region` là hình chữ nhật cộng dải tai, gọi mỗi frame nhưng có cache key.
- `pill_rect` căn giữa trong `WIN_W`, `y = 0`.
- `launch_flight` (`effects.rs`) đã lấy tâm pill = `origin + pill_rect * scale`. Đổi origin mà không đổi công thức này là đủ, nếu pill vẫn nằm trong HWND.

## Requirements

- HWND = cap: rộng 560 DIP, cao = cao expanded (128 DIP). `SetWindowPos` chỉ khi đổi neo, DPI, monitor, hoặc cap. Không gọi mỗi frame spring.
- Swapchain `ResizeBuffers` cùng lúc đó.
- Nghỉ: 150×32, không đo chữ. Banner và OSD: một dòng, cao 34, rộng = chữ + 28 (+16 nếu có glyph), kẹp 150–420, quá thì ellipsis.
- Expanded: một cỡ cho mọi tab kể cả rỗng. Rộng 380 (nằm trong 320–560). Cao 128. Radius pha 03 mới gắn; pha này chỉ thống nhất số.
- Xóa `expanded_layout` và scale 380/420. Tọa độ tab tương đối `pill_rect`.
- Region bo theo capsule/pill, thụt ~1 px. Bỏ dải tai. Bỏ qua nếu key pixel không đổi.
- Giảm chuyển động: một lần region + một paint, không tick spring rộng.
- Đo chữ khi chuỗi hoặc DPI đổi, cache layout. Không tạo layout mỗi frame.

## Architecture

Spring `w`/`h` vẫn chạy trong `after_change`. Target đặt một lần. Frame chỉ `SetWindowRgn` khi rect vật lý đổi ≥ 1 px.

```text
fn alert_width(text_dip: f32, glyph: bool) -> f32 {
  let pad = 28.0 + if glyph { 16.0 } else { 0.0 };
  (text_dip + pad).clamp(150.0, 420.0)
}
```

Nghỉ (`Minimal`, `Media`, `Todo`, `Countdown`, `Stopwatch`): `(150, 32)`.  
`Osd` / `Banner`: `(alert_width(...), 34)`.  
Expanded: `(380, 128)` mọi `Tab`. Hằng `EXPANDED_MIN_W = 320`, `EXPANDED_MAX_W = 560` để lần sau kẹp, chưa có grip.

`pill_rect(w, h)` vẫn căn giữa cap theo X, dính mép trên. Banner nở đều hai bên, HWND không nhích.

## Related code files

- `D:\project\ailan\crates\ailan\src\ui\mod.rs` — modify. `WIN_W=560`, `WIN_H=128`. `collapsed_size`, `expanded_size`. Xóa `expanded_layout`. `pill_rect`.
- `D:\project\ailan\crates\ailan\src\ui\tabs.rs` — modify. Header, mũi tên, music theo `p` thật. Xóa mốc `x = 146`, `x = 395`, `scale_rect`.
- `D:\project\ailan\crates\ailan\src\ui\collapsed.rs` — modify. Vẽ banner/OSD trong pill đã đo; ellipsis ở 420.
- `D:\project\ailan\crates\ailan\src\app\mod.rs` — modify. `place` dùng cap. `update_region` bo tròn. `after_change` target rộng một lần.
- `D:\project\ailan\crates\ailan\src\app\effects.rs` — modify. `launch_flight` xác nhận end = tâm pill theo `origin` mới.
- `D:\project\ailan\crates\ailan\src\gfx\mod.rs` — modify chỉ nếu cần helper round-rect region; vẽ tai để pha 03.

## Implementation Steps

1. `ui/mod.rs`: hằng cap và size như Architecture. `expanded_size` bỏ nhánh rỗng 40, Todo 114, Music 127. Xóa `expanded_layout`.
2. Cache đo: khi text banner/OSD hoặc `scale` đổi, `IDWriteTextLayout` (hoặc `DetermineMinWidth`) một lần, lưu DIP đã kẹp. `after_change` đọc cache, `w.set_target`. Không đo trong `frame`.
3. `collapsed_size` nhận rộng đã đo (tham số hoặc trường trên view). Đừng gọi DirectWrite từ `ailan-core`.
4. `update_region`: `CreateRoundRectRgn` theo pill * scale, inset 1 px. Xóa `CreateRectRgn` tai. `SetWindowRgn` nuốt HRGN. Key gồm left/top/right/bottom/radius px.
5. `place`: size HWND = cap * scale, không phải size pill đang animate. Gọi lại `gfx.resize` chỉ ở đây.
6. `tabs.rs`: vòng pin/ngày/giờ tính từ `p.x + 16`, không `p.x + 146`. Home và mũi tên từ `p.right() - pad`. Nốt nhạc `y` theo `p`, không số magic của layout 420. Hit cùng rect đã vẽ.
7. `effects.rs` `launch_flight`: end point giữ công thức tâm `pill_rect`. Sau khi pill không còn dính `rcMonitor.top` của primary, không cộng thêm offset cũ. Không đổi `flight.rs` nếu overlay đã nhận điểm màn hình.
8. `reduce_motion`: `w.snap()` / `h.snap()` rồi region một lần (đã có snap trong `after_change`).

## Todo list

- [ ] Cap HWND 560×128, resize swapchain chỉ khi cap/DPI/neo
- [ ] Cache rộng banner/OSD, kẹp 150–420, cao 34
- [ ] Nghỉ 150×32
- [ ] Một expanded 380×128
- [ ] Xóa layout 420 và scale
- [ ] `tabs.rs` theo `pill_rect`
- [ ] Region bo, inset 1 px
- [ ] Flight end = tâm pill mới

## Success Criteria

- Spring rộng không gọi `SetWindowPos` / `ResizeBuffers`.
- Banner ngắn ≥ 150, dài dừng 420 và có ellipsis.
- Đổi tab Music ↔ Todo không đổi cao.
- Click sát viền mờ không trúng (region thụt).
- Icon bay rơi vào tâm pill sau khi user đã kéo island.

## Risk Assessment

- Cap 560×128 luôn chiếm swapchain kể cả lúc thu. Chấp nhận (researcher 01). Không thu HWND khi idle.
- Ellipsis phụ thuộc trim DirectWrite. Test tay một chuỗi dài, một chuỗi rỗng.
- Region mỗi px trong spring tốn GDI. Chỉ gọi khi key đổi.

## Security Considerations

- Không mở hit ra ngoài pill. Region sai = nuốt click app dưới. So khớp region với pill sau mỗi đổi mode.

## Next steps

Pha 03 gắn token, xóa tai/mắt/audio, radius capsule và 16.
