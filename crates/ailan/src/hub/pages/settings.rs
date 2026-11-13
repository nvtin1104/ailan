//! Settings page: Language, Autostart, Reduce Motion, Pomodoro intervals, and About information.

use ailan_core::i18n::{t, Key, Lang};
use ailan_core::AppState;

use crate::gfx::{Align, Canvas, Font, Rect};
use crate::hub::widgets::*;
use crate::hub::{HubAction, HubHit};
use crate::platform::autostart;
use crate::ui::BG;

pub fn draw(
    cv: &mut Canvas,
    st: &AppState,
    r: Rect,
    _now: i64,
    scroll: f32,
    hover: Option<HubAction>,
    hits: &mut Vec<HubHit>,
) {
    let lang = st.lang();
    let mut y = r.y - scroll;

    // 1. General Section
    draw_section_hdr(cv, Rect::new(r.x, y, r.w, 22.0), t(lang, Key::General));
    y += 24.0;

    // Card 1: Language selector card
    let lang_card_r = Rect::new(r.x, y, r.w, 54.0);
    draw_glass_card(cv, lang_card_r, RADIUS_ITEM, 0.0);

    let l_txt_r = Rect::new(lang_card_r.x + 14.0, lang_card_r.y + 10.0, lang_card_r.w - 220.0, 18.0);
    cv.text(
        t(lang, Key::LanguageLabel),
        Font::text(12.5, 600, Align::Left),
        l_txt_r,
        TEXT_PRIMARY,
        0.98,
    );
    let l_sub_r = Rect::new(lang_card_r.x + 14.0, lang_card_r.y + 28.0, lang_card_r.w - 220.0, 16.0);
    cv.text(
        t(lang, Key::LanguageSub),
        Font::text(11.0, 400, Align::Left),
        l_sub_r,
        TEXT_FAINT,
        0.85,
    );

    // Segmented control: Tiếng Việt / English
    let seg_w = 190.0;
    let seg_h = 32.0;
    let seg_r = Rect::new(lang_card_r.right() - seg_w - 12.0, lang_card_r.y + (lang_card_r.h - seg_h) * 0.5, seg_w, seg_h);
    cv.fill_round(seg_r, RADIUS_PILL, GLASS_CARD, 0.65);
    cv.hairline_round(seg_r, RADIUS_PILL, GLASS_SPECULAR, 0.14);

    let half_w = seg_w * 0.5;
    let vi_r = Rect::new(seg_r.x + 2.0, seg_r.y + 2.0, half_w - 3.0, seg_h - 4.0);
    let en_r = Rect::new(seg_r.x + half_w + 1.0, seg_r.y + 2.0, half_w - 3.0, seg_h - 4.0);

    let is_vi = matches!(lang, Lang::Vi);
    if is_vi {
        cv.fill_round(vi_r, RADIUS_PILL, TEXT_PRIMARY, 0.95);
        cv.text("Tiếng Việt", Font::text(11.0, 650, Align::Center), vi_r, BG, 1.0);
        cv.text("English", Font::text(11.0, 500, Align::Center), en_r, TEXT_MUTED, 0.85);
    } else {
        cv.fill_round(en_r, RADIUS_PILL, TEXT_PRIMARY, 0.95);
        cv.text("Tiếng Việt", Font::text(11.0, 500, Align::Center), vi_r, TEXT_MUTED, 0.85);
        cv.text("English", Font::text(11.0, 650, Align::Center), en_r, BG, 1.0);
    }

    hits.push(HubHit {
        rect: vi_r,
        action: HubAction::SetLanguage(Lang::Vi),
    });
    hits.push(HubHit {
        rect: en_r,
        action: HubAction::SetLanguage(Lang::En),
    });

    y += 62.0;

    // Card 2: Autostart toggle
    let auto_row = Rect::new(r.x, y, r.w, 52.0);
    let autostart_on = autostart::is_enabled();
    let h_auto = if hover == Some(HubAction::ToggleAutostart) { 1.0 } else { 0.0 };
    draw_switch_row(
        cv,
        auto_row,
        t(lang, Key::AutostartLabel),
        Some(t(lang, Key::AutostartSub)),
        autostart_on,
        h_auto,
    );
    hits.push(HubHit {
        rect: auto_row,
        action: HubAction::ToggleAutostart,
    });

    y += 60.0;

    // Card 3: Reduce Motion toggle
    let motion_row = Rect::new(r.x, y, r.w, 52.0);
    let motion_on = st.settings.reduce_motion;
    let h_motion = if hover == Some(HubAction::ToggleReduceMotion) { 1.0 } else { 0.0 };
    draw_switch_row(
        cv,
        motion_row,
        t(lang, Key::ReduceMotionLabel),
        Some(t(lang, Key::ReduceMotionSub)),
        motion_on,
        h_motion,
    );
    hits.push(HubHit {
        rect: motion_row,
        action: HubAction::ToggleReduceMotion,
    });

    y += 74.0;

    // 2. About Card
    draw_section_hdr(cv, Rect::new(r.x, y, r.w, 22.0), t(lang, Key::AboutAiLan));
    y += 24.0;

    let about_r = Rect::new(r.x, y, r.w, 74.0);
    draw_glass_card(cv, about_r, RADIUS_CARD, 0.0);

    let logo_r = Rect::new(about_r.x + 14.0, about_r.y + 16.0, 42.0, 42.0);
    draw_app_logo(cv, logo_r);

    let about_info_x = about_r.x + 68.0;
    let about_info_w = about_r.w - 82.0;
    cv.text("AiLan — Dynamic Island for Windows", Font::text(13.0, 650, Align::Left), Rect::new(about_info_x, about_r.y + 12.0, about_info_w, 18.0), TEXT_PRIMARY, 1.0);
    cv.text(
        "\"Ai Lan\" = Island (tiếng Việt) + AI Assistant · Fluent Liquid Glass cao cấp.",
        Font::text(11.0, 400, Align::Left),
        Rect::new(about_info_x, about_r.y + 32.0, about_info_w, 16.0),
        TEXT_MUTED,
        0.90,
    );
    cv.text(
        "Phiên bản 0.2.1 · Made with Rust & Direct2D",
        Font::text(10.5, 450, Align::Left),
        Rect::new(about_info_x, about_r.y + 49.0, about_info_w, 16.0),
        TEXT_FAINT,
        0.80,
    );
}
