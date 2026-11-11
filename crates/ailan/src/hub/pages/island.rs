//! Island page: interactive live preview (the real notch renderer, miniaturised), notch mode
//! switcher, and the activity Priority Matrix (reorder + pin + smart minute peek).

use ailan_core::i18n::{t, Key};
use ailan_core::settings::{NotchMode, Tab};
use ailan_core::state::Collapsed;

use crate::gfx::{Align, Canvas, Font, Gradient, Rect, TextStyle};
use crate::hub::controller::tab_index;
use crate::hub::widgets::*;
use crate::hub::{HubAction, PageCtx};
use crate::ui::{self, G_BELL, G_CHECKMARK, G_MUSIC, G_TIMER};

const ROW_H: f32 = 54.0;
const G_CHEVRON_UP: char = '\u{E70E}';
const G_LOCK: char = '\u{E72E}';

/// Night-sky "desktop" behind the preview notch.
const WALLPAPER: Gradient = Gradient::new(&[(0.0, 0x1E1B4B, 1.0), (0.55, 0x0F172A, 1.0), (1.0, 0x05060A, 1.0)]);

fn mode_name(lang: ailan_core::i18n::Lang, m: Collapsed) -> String {
    let s = match m {
        Collapsed::Minimal => t(lang, Key::ModeShortMinimal),
        Collapsed::Clock => t(lang, Key::ModeShortAlways),
        Collapsed::Timer => t(lang, Key::TabFocus),
        Collapsed::Media => t(lang, Key::TabMusic),
        Collapsed::Todo => t(lang, Key::TabTodo),
        Collapsed::PinPeek => t(lang, Key::PinnedLabel),
        Collapsed::Osd => t(lang, Key::Volume),
        Collapsed::Banner => t(lang, Key::Reminder),
    };
    s.to_string()
}

pub fn draw(cv: &mut Canvas, cx: &mut PageCtx, r: Rect) {
    let lang = cx.lang();
    let st = cx.st;
    let mut y = r.y - cx.scroll;

    // ---- 1. Live preview ---------------------------------------------------------------
    let card = Rect::new(r.x, y, r.w, if cx.compact { 176.0 } else { 206.0 });
    cx.card(cv, card, RADIUS_CARD, 0.0);
    draw_section_header(cv, Rect::new(card.x + 16.0, card.y + 12.0, card.w * 0.5, 16.0), t(lang, Key::LivePreview));
    let mode = st.collapsed(cx.now);
    let now_lbl = format!("{} · {}", t(lang, Key::NowShowing), mode_name(lang, mode));
    draw_badge_right(cv, card.right() - 12.0, card.y + 9.0, 22.0, &now_lbl, ACCENT_PRIMARY_HI, ACCENT_PRIMARY, 0.16);

    let strip = Rect::new(card.x + 12.0, card.y + 40.0, card.w - 24.0, card.h - 40.0 - 34.0);
    let srad = concentric(RADIUS_CARD, 12.0);
    cv.fill_round_linear(strip, srad, WALLPAPER, (strip.x, strip.y), (strip.right(), strip.bottom()), 1.0);
    cv.fill_circle_radial(strip.x + strip.w * 0.78, strip.bottom() - 6.0, strip.w * 0.35, GLOW_ACCENT, 0.35);
    cv.hairline_round_linear(strip, srad, RIM, (strip.x, strip.y), (strip.x, strip.bottom()), 1.0);
    // Monitor top bezel the notch hangs from.
    cv.fill_rect(Rect::new(strip.x + srad, strip.y, strip.w - srad * 2.0, 1.0), WHITE, 0.10);

    let hovered = cx.hover == Some(HubAction::PreviewIsland);
    let (w, h) = if hovered { ui::EXPANDED } else { ui::collapsed_size(st, mode) };
    let s = if hovered {
        ((strip.h - 6.0) / h).min((strip.w - 16.0) / w).min(1.0)
    } else {
        ((strip.w - 24.0) / w).min(1.6)
    };
    let visible = st.hover_activities();
    let tab = visible.first().copied().or_else(|| st.primary_activity(cx.now)).unwrap_or(Tab::Focus);
    let view = ui::View {
        now: cx.now,
        pill: ui::pill_rect(w, h),
        expand: if hovered { 1.0 } else { 0.0 },
        mode,
        tab,
        input: None,
        todo_scroll: 0,
        art: None,
        hover: None,
        hover_t: 0.0,
        tab_pos: visible.iter().position(|x| *x == tab).unwrap_or(0) as f32,
        cursor: None,
        audio_bars: [0.45, 0.85, 0.6, 0.95, 0.5],
        audio_peak: 0.0,
    };
    let prev_alpha = cv.alpha;
    cv.push_clip(strip);
    cv.set_transform(strip.x + strip.w * 0.5 - ui::WIN_W * 0.5 * s, strip.y, s);
    let _ = ui::draw(cv, st, &view);
    cv.reset_transform();
    cv.pop_clip();
    cv.alpha = prev_alpha;
    cx.hit(strip, HubAction::PreviewIsland);

    cv.glyph('\u{E7C9}', 10.0, Rect::new(card.x + 14.0, card.bottom() - 30.0, 16.0, 24.0), TEXT_SECTION, 1.0);
    cv.text(t(lang, Key::PreviewHoverHint), FONT_META, Rect::new(card.x + 34.0, card.bottom() - 30.0, card.w - 48.0, 24.0), TEXT_SECTION, 1.0);
    y = card.bottom() + 14.0;

    // ---- 2. Notch mode ---------------------------------------------------------------
    let modes = [NotchMode::DynamicWhenActive, NotchMode::AlwaysDynamic, NotchMode::MinimalOnly];
    let labels = [t(lang, Key::ModeShortDynamic), t(lang, Key::ModeShortAlways), t(lang, Key::ModeShortMinimal)];
    let cur = modes.iter().position(|m| *m == st.settings.mode).unwrap_or(0);
    let hover_seg = modes.iter().position(|m| cx.hover == Some(HubAction::SetNotchMode(*m)));
    let segs = draw_segmented(cv, Rect::new(r.x, y, r.w, 34.0), &labels, cur as f32, hover_seg);
    for (sr, m) in segs.iter().zip(modes) {
        cx.hit(*sr, HubAction::SetNotchMode(m));
    }
    y += 34.0 + 22.0;

    // ---- 3. Priority matrix ----------------------------------------------------------
    let hdr_w = if cx.compact { r.w } else { r.w * 0.45 };
    draw_section_header(cv, Rect::new(r.x, y, hdr_w, 18.0), t(lang, Key::PrioritySection));
    if !cx.compact {
        cv.text(t(lang, Key::PriorityHint), Font::text(11.5, 400, Align::Right), Rect::new(r.x + r.w * 0.4, y, r.w * 0.6, 18.0), TEXT_SECTION, 1.0);
    }
    y += 26.0;
    let pcard = Rect::new(r.x, y, r.w, 8.0 + ROW_H * 4.0 + 1.0 + ROW_H + 8.0);
    cx.card(cv, pcard, RADIUS_CARD, 0.0);
    let rows_y = pcard.y + 4.0;

    // Locked top row: reminders & system alerts always pre-empt activities.
    let lr = Rect::new(pcard.x + 4.0, rows_y, pcard.w - 8.0, ROW_H);
    rank_badge(cv, lr, None);
    icon_tile(cv, lr, G_BELL, ACCENT_AMBER);
    cv.text(t(lang, Key::SystemAlerts), Font::text(13.5, 600, Align::Left), Rect::new(lr.x + 88.0, lr.y + 9.0, lr.w - 136.0, 20.0), TEXT_TITLE, 1.0);
    cv.text(t(lang, Key::AlwaysFirst), FONT_META, Rect::new(lr.x + 88.0, lr.y + 28.0, lr.w - 136.0, 16.0), TEXT_SECTION, 1.0);
    cv.glyph(G_LOCK, 11.0, Rect::new(lr.right() - 40.0, lr.y, 28.0, lr.h), TEXT_SECTION, 1.0);

    let order = st.settings.priority.normalized_order();
    let running: Vec<Tab> = st.running_activities();
    for (rank, tab) in order.iter().enumerate() {
        let pos = cx.prio_pos[tab_index(*tab)];
        let row = Rect::new(pcard.x + 4.0, rows_y + (1.0 + pos) * ROW_H, pcard.w - 8.0, ROW_H);
        let (glyph, rgb, title, pin_act) = match tab {
            Tab::Focus => (G_TIMER, ACCENT_PRIMARY_HI, t(lang, Key::TabFocus), HubAction::PinFocus),
            Tab::Music => (G_MUSIC, ACCENT_SKY, t(lang, Key::TabMusic), HubAction::PinMedia),
            Tab::Todo => (G_CHECKMARK, ACCENT_MINT, t(lang, Key::TabTodo), HubAction::PinTodo),
        };
        let up = HubAction::PriorityUp(*tab);
        let down = HubAction::PriorityDown(*tab);
        let hot = [up, down, pin_act].iter().any(|a| cx.hover == Some(*a));
        if hot {
            cv.fill_round(row, RADIUS_ITEM, WHITE, 0.04);
        }
        draw_divider_h(cv, row.x + 12.0, row.y, row.w - 24.0);
        rank_badge(cv, row, Some(rank + 1));
        icon_tile(cv, row, glyph, rgb);
        cv.text(title, Font::text(13.5, 600, Align::Left), Rect::new(row.x + 88.0, row.y + 9.0, row.w - 240.0, 20.0), TEXT_TITLE, 1.0);
        let is_running = running.contains(tab);
        let pinned = st.settings.is_pinned(*tab);
        let (status, srgb) = if is_running {
            (t(lang, Key::ActiveLabel), ACCENT_MINT)
        } else if pinned {
            (t(lang, Key::PinnedLabel), ACCENT_PRIMARY_HI)
        } else {
            (t(lang, Key::IdleLabel), TEXT_SECTION)
        };
        cv.fill_circle(row.x + 92.0, row.y + 36.0, 3.0, srgb, 1.0);
        cv.text(status, FONT_META, Rect::new(row.x + 100.0, row.y + 28.0, row.w - 250.0, 16.0), srgb, 1.0);

        // Controls: [pin switch] [▲] [▼]
        let dn = Rect::new(row.right() - 40.0, row.y + 12.0, 30.0, 30.0);
        let upr = Rect::new(dn.x - 32.0, dn.y, 30.0, 30.0);
        let sw = Rect::new(upr.x - 56.0, row.y, 48.0, row.h);
        draw_switch(cv, sw, pinned, cx.hover_t(pin_act));
        cx.hit(sw, pin_act);
        if rank > 0 {
            draw_icon_button(cv, upr, G_CHEVRON_UP, 10.0, cx.hover_t(up), false, ACCENT_PRIMARY);
            cx.hit(upr, up);
        }
        if rank < 2 {
            draw_icon_button(cv, dn, G_CHEVRON_DOWN, 10.0, cx.hover_t(down), false, ACCENT_PRIMARY);
            cx.hit(dn, down);
        }
    }

    // Smart minute peek
    let pr = Rect::new(pcard.x + 4.0, rows_y + ROW_H * 4.0 + 1.0, pcard.w - 8.0, ROW_H);
    draw_divider_h(cv, pr.x + 12.0, pr.y, pr.w - 24.0);
    let peek = HubAction::ToggleSmartPeek;
    let on = st.settings.priority.smart_minute_peek;
    cv.text(t(lang, Key::SmartMinutePeek), Font::text(13.0, 600, Align::Left), Rect::new(pr.x + 16.0, pr.y + 9.0, pr.w - 90.0, 20.0), TEXT_TITLE, 1.0);
    cv.text(t(lang, Key::SmartMinutePeekDesc), FONT_META, Rect::new(pr.x + 16.0, pr.y + 28.0, pr.w - 90.0, 16.0), TEXT_SECTION, 1.0);
    draw_switch(cv, Rect::new(pr.right() - 58.0, pr.y, 48.0, pr.h), on, cx.hover_t(peek));
    cx.hit(pr, peek);
    y = pcard.bottom() + 22.0;

    // ---- 4. Idle behaviour -------------------------------------------------------------
    draw_section_header(cv, Rect::new(r.x, y, r.w, 18.0), t(lang, Key::NotchBehavior));
    y += 26.0;
    let min_row = Rect::new(r.x, y, r.w, 54.0);
    let act = HubAction::ToggleMinimalMode;
    draw_switch_row(cv, min_row, t(lang, Key::MinimalModeLabel), Some(t(lang, Key::MinimalModeSub)), st.settings.minimal_ui.hide_when_idle, cx.hover_t(act));
    cx.hit(min_row, act);
    cx.content_h = Some(min_row.bottom() + cx.scroll - r.y + 12.0);
}

fn rank_badge(cv: &mut Canvas, row: Rect, rank: Option<usize>) {
    let c = Rect::new(row.x + 12.0, row.y + (row.h - 22.0) * 0.5, 22.0, 22.0);
    match rank {
        Some(n) => {
            cv.stroke_circle(c.x + 11.0, c.y + 11.0, 10.5, 1.0, WHITE, 0.14);
            cv.text_styled(&n.to_string(), Font::text(11.5, 650, Align::Center), c, TEXT_BODY, 1.0, TextStyle::TABULAR);
        }
        None => cv.glyph(G_LOCK, 9.0, c, TEXT_SECTION, 1.0),
    }
}

fn icon_tile(cv: &mut Canvas, row: Rect, glyph: char, rgb: u32) {
    let tile = Rect::new(row.x + 44.0, row.y + (row.h - 32.0) * 0.5, 32.0, 32.0);
    cv.fill_round(tile, RADIUS_BUTTON, rgb, 0.14);
    cv.hairline_round(tile, RADIUS_BUTTON, rgb, 0.32);
    cv.glyph(glyph, 13.0, tile, mix(rgb, WHITE, 0.3), 1.0);
}
