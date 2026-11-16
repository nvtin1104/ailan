# Researcher 02 — Grok visual language for a desktop island

Date: 2026-11-15. Scope: grok.com / Grok chat look, applied to a 34–150 DIP Windows pill. Not a chat clone. No code changes.

Sources (third-party, not live computed CSS): assistant-ui Grok clone dark tokens; Fudge grok.com audit (type, radius, accent); Swipefile grok.com new-conversation capture (2026-09-05, light). Hex values are **approximations**.

## 1. What actually reads as Grok

Grok’s dark UI is a quiet instrument: one centered task, huge empty field, almost no chrome. Separation is a surface step plus a 1px ring, not a palette and not a shadow.

| Trait | Observed (approx) | Generic dark mode |
|---|---|---|
| Canvas | `#141414` lifted black, not OLED `#000` | Pure black or navy `#0B1220` |
| Primary text | `#FCFCFC` / white | White |
| Support text | `#9E9E9E` (one gray only) | Zinc scale, 4–5 grays |
| Surface | Composer `#212121` on `#141414` | Cards at random elevations |
| Edge | Hairline `#2A2A2A`, 1px; rare 2px selected rule | Glow, blur, gradient border |
| Shape | Panels 8–24px; compact controls full pill; send is a circle | 8px on everything |
| Accent | `#FF6B35` promo/status only, never default fill | Purple/blue “AI” wash |
| Density | Empty state = wordmark + one composer | Sidebar, avatars, bubbles |
| Motion | Small state swaps (mic/send). No bounce | Springs, pulses, parallax |

Light grok.com exists (white field, dark text, same sparse layout). Product owner asked for the black UI, so use the dark column only.

Assistant replies sit on the canvas with a small icon row. User text is not a colorful bubble. Hover reveals message actions in the product; **do not copy that**. Owner rule: hover must not open anything. Click opens. Drag moves.

## 2. Copy vs reject (34–150 DIP)

**Copy:** charcoal pill fill, 1px hairline, white label, one muted gray, full-pill collapsed shape, 12–16px horizontal padding, centered resting position, width that follows text only for a banner, click-to-expand, drag-to-move, motion limited to opacity and width (~160–200ms).

**Reject:** chat bubbles, avatars, wordmark/orb (trademark), composer, mic/send, model picker, SuperGrok upsell, history sidebar, hover action bars, streaming caret, orange fills, message timestamps.

**Also drop current notch language** (it fights Grok): concave ears, gaze eyes, audio bars, album-art pulse, Segoe MDL2 icon parade. Collapsed island is text-first. One optional glyph.

## 3. Layout

All sizes DIP. Island stays centered until the user drags; later widths grow from the dragged anchor, not from a corner.

- **Resting:** fixed **150 × 32**. Do not reflow on hover or on idle text. Full pill (`radius = height/2` ≈ 16).
- **Banner:** one line, height **34**. `width = textWidth + 28` (14px pad each side; +16 if a dot/glyph). **Min 150, max 420.** Longer copy ellipsizes. This is the only state that grows from string length.
- **Expanded shell:** content-sized. **Min 320 × 48, max width 560**, height grows with features, cap later (~70% work area). Radius **16** (inside the 8–24 band). Same shell for every feature so adding a panel does not invent a new shape. Click opens; click-off or an explicit control closes. Drag target = empty chrome, not buttons.

Hover may brighten the hairline (`#2A2A2A` → `#3A3A3A`). It must not change width, height, or contents.

## 4. Tokens (approx)

| Token | Hex | Note |
|---|---|---|
| bg | `#141414` | Dark-canvas clone. Replaces `#000000` |
| surface | `#212121` | Expanded well / pressed row |
| text | `#FCFCFC` | Fudge primary |
| muted | `#9E9E9E` | Fudge support. Clone `#6B6B6B` fails small-type contrast on `#141414` |
| hairline | `#2A2A2A` | 1px. Floating pill on a black wallpaper may need `#3A3A3A` |
| accent | none | Optional 8px status dot `#FF6B35` for a real alert only |
| radius | h/2 collapsed, 16 expanded | No ears |
| type | 13 / 11 / 15 | Label 13 semibold, meta 11, expanded title 15. Segoe UI Variable. No webfont |
| pad | 14×6 collapsed, 16 expanded | Fudge control pad is 12–16 |
| shadow | none in-product | Floating adaptation only: `0 8px 24px rgba(0,0,0,.45)` if the ring vanishes on dark wallpapers |
| motion | 160–200ms, ease-out | Width and opacity. No layout on hover |

## 5. Why `#000` + white is not Grok yet

`crates/ailan/src/ui/mod.rs` is already monochrome and still reads as a Dynamic Island:

- `BG #000000` with no ring. Grok’s step is `#141414` body + `#212121` surface + `#2A2A2A` hairline.
- `RADIUS_COLLAPSED 14` on a 30px bar is a rounded rect. Grok’s small control is a capsule. `RADIUS_EXPANDED 29` plus `EAR_*` concave corners are notch geometry.
- `TEXT_SOFT #A1A1AA` is close to `#9E9E9E` but sits on pure black, so it looks like generic zinc-on-OLED.
- Collapsed size is already **150×30**; banner/OSD are **fixed 250×34 / 240×34**. Banner must measure text and clamp 150–420.
- Expanded is locked at **380 / 420**. Shell needs min 320 / max 560 so it can grow later.
- `hover_t`, eye gaze, tab slide, and `audio_bars` change the silhouette or the drawing without a click. Hover stays a color change. Click expands. Drag repositions.
- MDL2 transport glyphs and waveform are a media remote. Grok’s chrome is one line of type.

Smallest diff that changes the read: lift the fill off `#000`, add the hairline, make the resting shape a pill, delete the ears, stop hover from opening anything.

## Unresolved

- Live grok.com CSS was not sampled (tool cap). Tokens are clone + design audit, not getComputedStyle.
- `#FF6B35` is a marketing accent in the Fudge audit. Unconfirmed inside the chat composer. Leave it off the island until checked.
- X-app Grok on Lights Out is closer to `#000` and would **not** separate Ailan from “already black”. This proposal follows grok.com’s lifted canvas, not X.
- `#2A2A2A` on `#141414` is a weak edge over a dark desktop. Confirm `#3A3A3A` or the whisper shadow on a real wallpaper.
- Banner ellipsis vs wrap: assumption is single line + ellipsis at 420 DIP.
- Drag vs click on a 32px target needs a movement threshold. Not specified by Grok.
