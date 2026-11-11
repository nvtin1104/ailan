//! AI Command Center: a local Smart Brief (overdue work, what's ahead, the next free focus
//! window) and a natural-language command box whose parsed plan previews live as you type.
//! Everything runs on-device through `ailan_core::{brief, command}` — no network, no model.

use ailan_core::brief::smart_brief;
use ailan_core::command::{parse_command, Plan};
use ailan_core::i18n::{t, weekday, Key, Lang};
use ailan_core::reminder::{fmt_due, fmt_repeat, Repeat};
use ailan_core::time::civil;

use crate::gfx::{Align, Canvas, Font, Rect, TextStyle};
use crate::hub::widgets::*;
use crate::hub::{Field, HubAction, PageCtx};
use crate::ui::{G_BELL, G_BOLT, G_CHECKMARK, G_PLAY, G_TIMER};

pub fn examples(lang: Lang) -> [&'static str; 3] {
    match lang {
        Lang::Vi => [
            "Lên lịch ôn tập Rust lúc 20h mỗi thứ 2, 4, 6 và bật Pomodoro 45p",
            "Nhắc tôi uống nước mỗi ngày 10h sáng",
            "Gọi mẹ ngày mai lúc 9h",
        ],
        Lang::En => [
            "Review Rust at 8pm every Mon, Wed, Fri and focus 45m",
            "Remind me to drink water every day at 10am",
            "Call mom tomorrow at 9am",
        ],
    }
}

fn hhmm(ms: i64, off: i32) -> String {
    let c = civil(ms, off);
    format!("{:02}:{:02}", c.hour, c.minute)
}

pub fn draw(cv: &mut Canvas, cx: &mut PageCtx, r: Rect) {
    let lang = cx.lang();
    let mut y = r.y - cx.scroll;

    // Header
    let tile = Rect::new(r.x, y, 40.0, 40.0);
    cv.fill_circle_radial(tile.x + 20.0, tile.y + 20.0, 44.0, GLOW_ACCENT, 0.6);
    cv.fill_round_linear(tile, RADIUS_ITEM, ACCENT_FILL, (tile.x, tile.y), (tile.right(), tile.bottom()), 1.0);
    cv.hairline_round_linear(tile, RADIUS_ITEM, RIM_HOT, (tile.x, tile.y), (tile.x, tile.bottom()), 1.0);
    cv.text("✦", Font::text(18.0, 400, Align::Center), tile, WHITE, 1.0);
    cv.text(t(lang, Key::CommandCenter), if cx.compact { Font::text(17.0, 700, Align::Left) } else { FONT_HERO }, Rect::new(r.x + 54.0, y - 2.0, r.w - 54.0, 26.0), TEXT_HERO, 1.0);
    cv.fill_circle(r.x + 58.0, y + 32.0, 3.0, ACCENT_MINT, 1.0);
    cv.text(t(lang, Key::LocalOnly), FONT_META, Rect::new(r.x + 66.0, y + 24.0, r.w - 66.0, 16.0), TEXT_SECTION, 1.0);
    y += 56.0;

    // Command box + plan (top: it's the primary action)
    let box_r = Rect::new(r.x, y, r.w, 50.0);
    let plan = parse_command(cx.copilot);
    let focused = cx.focus == Some(Field::Copilot);
    let act = HubAction::FocusField(Field::Copilot);
    draw_text_input(cv, box_r, cx.copilot, t(lang, Key::CopilotHint), focused, cx.hover_t(act), Font::text(13.5, 450, Align::Left), Some(G_BOLT), 92.0);
    let run = HubAction::SubmitField(Field::Copilot);
    let run_r = Rect::new(box_r.right() - 92.0, box_r.y + 8.0, 84.0, box_r.h - 16.0);
    if plan.is_actionable() {
        draw_button(cv, run_r, t(lang, Key::PlanRun), ButtonKind::Primary, cx.hover_t(run));
        cx.hit(run_r, run);
    } else {
        cv.fill_round(run_r, RADIUS_BUTTON, WHITE, 0.04);
        cv.text(t(lang, Key::PlanRun), Font::text(12.0, 600, Align::Center), run_r, TEXT_SECTION, 1.0);
    }
    cx.hit(box_r, act);
    y = box_r.bottom() + 12.0;

    if cx.copilot.trim().is_empty() {
        draw_section_header(cv, Rect::new(r.x + 2.0, y, r.w, 16.0), t(lang, Key::Examples));
        y += 24.0;
        for (i, ex) in examples(lang).iter().enumerate() {
            let er = Rect::new(r.x, y, r.w, 34.0);
            let a = HubAction::UseExample(i as u8);
            let h = cx.hover_t(a);
            cv.fill_round(er, RADIUS_BUTTON, WHITE, 0.03 + 0.04 * h);
            cv.hairline_round(er, RADIUS_BUTTON, WHITE, 0.06 + 0.08 * h);
            cv.glyph(G_BOLT, 10.0, Rect::new(er.x + 8.0, er.y, 20.0, er.h), if h > 0.0 { ACCENT_PRIMARY_HI } else { TEXT_SECTION }, 1.0);
            cv.text(ex, Font::text(12.5, 450, Align::Left), Rect::new(er.x + 32.0, er.y, er.w - 44.0, er.h), if h > 0.0 { TEXT_HERO } else { TEXT_BODY }, 1.0);
            cx.hit(er, a);
            y += 40.0;
        }
    } else {
        y = draw_plan(cv, cx, Rect::new(r.x, y, r.w, 0.0), &plan);
    }
    y += 14.0;

    // Smart brief
    let brief_h = if cx.compact { 214.0 } else { 196.0 };
    let card = Rect::new(r.x, y, r.w, brief_h);
    draw_brief(cv, cx, card);
    cx.content_h = Some(card.bottom() + cx.scroll - r.y + 12.0);
}

/// Live plan chips under the command box. Returns the next free y.
fn draw_plan(cv: &mut Canvas, cx: &mut PageCtx, r: Rect, plan: &Plan) -> f32 {
    let lang = cx.lang();
    let off = cx.st.offset_min;
    draw_section_header(cv, Rect::new(r.x + 2.0, r.y, r.w, 16.0), t(lang, Key::PlanTitle));
    let mut chips: Vec<(char, &str, String, u32)> = Vec::new();
    if !plan.title.is_empty() {
        chips.push((G_CHECKMARK, t(lang, Key::PlanTask), plan.title.clone(), ACCENT_MINT));
    }
    if plan.has_schedule() {
        if let Some(due) = plan.due(cx.now, off) {
            let mut v = fmt_due(due, false, cx.now, off, lang);
            match plan.repeat {
                Repeat::None => {}
                Repeat::Weekly(mask) => {
                    let days: Vec<&str> = (0..7).filter(|d| mask & (1 << d) != 0).map(|d| weekday(lang, d)).collect();
                    v.push_str(&format!(" · {} ({})", fmt_repeat(plan.repeat, lang), days.join(", ")));
                }
                rep => v.push_str(&format!(" · {}", fmt_repeat(rep, lang))),
            }
            chips.push((G_BELL, t(lang, Key::PlanRemind), v, ACCENT_AMBER));
        }
    }
    if let Some(m) = plan.focus_min {
        chips.push((G_TIMER, t(lang, Key::PlanFocusSet), format!("{m} {}", t(lang, Key::MinuteShort)), ACCENT_PRIMARY_HI));
    }
    if plan.start_focus {
        chips.push((G_PLAY, t(lang, Key::PlanFocusStart), String::new(), ACCENT_PRIMARY_HI));
    }
    let mut y = r.y + 24.0;
    if chips.is_empty() {
        cv.text(t(lang, Key::PlanEmpty), Font::text(12.5, 450, Align::Left), Rect::new(r.x + 2.0, y, r.w, 22.0), TEXT_SECTION, 1.0);
        return y + 26.0;
    }
    // Flow layout with wrapping.
    let (gap, ch) = (8.0, 32.0);
    let mut x = r.x;
    let lf = Font::text(11.0, 600, Align::Left);
    let vf = Font::text(12.5, 500, Align::Left);
    for (glyph, label, value, rgb) in chips {
        let lw = cv.text_width(label, lf, TextStyle::default());
        let vw = if value.is_empty() { 0.0 } else { cv.text_width(&value, vf, TextStyle::TABULAR) + 10.0 };
        let w = (34.0 + lw + vw + 14.0).min(r.w);
        if x + w > r.right() + 0.5 && x > r.x {
            x = r.x;
            y += ch + gap;
        }
        let c = Rect::new(x, y, w, ch);
        cv.fill_round(c, ch * 0.5, rgb, 0.10);
        cv.hairline_round(c, ch * 0.5, rgb, 0.30);
        cv.glyph(glyph, 10.0, Rect::new(c.x + 8.0, c.y, 20.0, ch), rgb, 1.0);
        cv.text(label, lf, Rect::new(c.x + 30.0, c.y, lw + 2.0, ch), mix(rgb, WHITE, 0.35), 1.0);
        if !value.is_empty() {
            let vr = Rect::new(c.x + 30.0 + lw + 8.0, c.y, (c.right() - (c.x + 30.0 + lw + 8.0) - 12.0).max(0.0), ch);
            cv.text_styled(&value, vf, vr, TEXT_HERO, 1.0, TextStyle::TABULAR);
        }
        x += w + gap;
    }
    y + ch + 4.0
}

/// Greedy two-line wrap: as many words as fit on the first line, the rest on the second.
fn wrap2<'s>(cv: &mut Canvas, s: &'s str, font: Font, width: f32) -> (&'s str, &'s str) {
    let mut cut = None;
    for (i, c) in s.char_indices() {
        if c == ' ' && cv.text_width(&s[..i], font, TextStyle::default()) <= width {
            cut = Some(i);
        }
    }
    if cv.text_width(s, font, TextStyle::default()) <= width {
        return (s, "");
    }
    match cut {
        Some(i) => (&s[..i], s[i + 1..].trim_start()),
        None => (s, ""),
    }
}

fn draw_brief(cv: &mut Canvas, cx: &mut PageCtx, r: Rect) {
    let lang = cx.lang();
    let st = cx.st;
    let off = st.offset_min;
    let b = smart_brief(&st.data.todos, cx.now, off, st.pomodoro.durations.focus_min);
    cx.card(cv, r, RADIUS_CARD, 0.0);
    cv.text("✦", Font::text(12.0, 400, Align::Center), Rect::new(r.x + 12.0, r.y + 10.0, 18.0, 20.0), ACCENT_PRIMARY_HI, 1.0);
    draw_section_header(cv, Rect::new(r.x + 34.0, r.y + 12.0, r.w - 50.0, 16.0), t(lang, Key::BriefTitle));

    // Stat tiles
    let stats = [
        (b.overdue.len(), t(lang, Key::BriefOverdue), ACCENT_ROSE),
        (b.due_today, t(lang, Key::BriefDueToday), ACCENT_AMBER),
        (b.open, t(lang, Key::BriefOpen), ACCENT_PRIMARY_HI),
    ];
    let pad = 12.0;
    let tw = (r.w - pad * 2.0 - 16.0) / 3.0;
    let ty = r.y + 38.0;
    let th = if cx.compact { 78.0 } else { 66.0 };
    let trad = concentric(RADIUS_CARD, pad);
    for (i, (n, label, rgb)) in stats.iter().enumerate() {
        let tr = Rect::new(r.x + pad + i as f32 * (tw + 8.0), ty, tw, th);
        let lit = *n > 0;
        cv.fill_round(tr, trad, if lit { *rgb } else { WHITE }, if lit { 0.09 } else { 0.03 });
        cv.hairline_round(tr, trad, if lit { *rgb } else { WHITE }, if lit { 0.26 } else { 0.07 });
        cv.text_styled(&n.to_string(), Font::text(22.0, 700, Align::Left), Rect::new(tr.x + 12.0, tr.y + 8.0, tr.w - 20.0, 28.0), if lit { mix(*rgb, WHITE, 0.25) } else { TEXT_BODY }, 1.0, TextStyle::TABULAR);
        let lf = Font::text(11.0, 450, Align::Left);
        if cx.compact {
            // Narrow tiles: wrap onto two lines at the word boundary nearest the middle.
            let (a, b) = wrap2(cv, label, lf, tr.w - 16.0);
            cv.text(a, lf, Rect::new(tr.x + 12.0, tr.y + 38.0, tr.w - 16.0, 16.0), TEXT_BODY, 1.0);
            cv.text(b, lf, Rect::new(tr.x + 12.0, tr.y + 54.0, tr.w - 16.0, 16.0), TEXT_BODY, 1.0);
        } else {
            cv.text(label, lf, Rect::new(tr.x + 12.0, tr.y + 38.0, tr.w - 16.0, 18.0), TEXT_BODY, 1.0);
        }
    }

    // Free focus window
    let fy = ty + th + 12.0;
    let fr = Rect::new(r.x + pad, fy, r.w - pad * 2.0, 44.0);
    cv.fill_round(fr, trad, WHITE, 0.035);
    match b.free_slot {
        Some((s, e)) => {
            cv.glyph(G_TIMER, 12.0, Rect::new(fr.x + 8.0, fr.y, 22.0, fr.h), ACCENT_MINT, 1.0);
            let span = format!("{} – {}", hhmm(s, off), hhmm(e, off));
            let sw = cv.text_width(&span, Font::text(14.0, 700, Align::Left), TextStyle::TABULAR);
            cv.text_styled(&span, Font::text(14.0, 700, Align::Left), Rect::new(fr.x + 34.0, fr.y, sw + 2.0, fr.h), TEXT_HERO, 1.0, TextStyle::TABULAR);
            let bw = 92.0;
            if !cx.compact {
                cv.text(t(lang, Key::BriefFreeSlot), FONT_META, Rect::new(fr.x + 44.0 + sw, fr.y, (fr.w - sw - bw - 60.0).max(0.0), fr.h), TEXT_BODY, 1.0);
            }
            let start = HubAction::PomoToggle;
            let br = Rect::new(fr.right() - bw - 6.0, fr.y + 6.0, bw, fr.h - 12.0);
            let lbl = if st.pomodoro.is_running() { t(lang, Key::Pause) } else { t(lang, Key::Start) };
            draw_button(cv, br, lbl, ButtonKind::Primary, cx.hover_t(start));
            cx.hit(br, start);
        }
        None => {
            cv.glyph(G_TIMER, 12.0, Rect::new(fr.x + 8.0, fr.y, 22.0, fr.h), TEXT_SECTION, 1.0);
            cv.text(t(lang, Key::BriefNoSlot), Font::text(12.5, 500, Align::Left), Rect::new(fr.x + 34.0, fr.y, fr.w - 40.0, fr.h), TEXT_BODY, 1.0);
        }
    }

    // Headline: first overdue item, else next reminder, else calm
    let hy = fr.bottom() + 8.0;
    let hr = Rect::new(r.x + pad + 4.0, hy, r.w - pad * 2.0 - 8.0, (r.bottom() - hy - 6.0).max(18.0));
    let find = |id: u64| st.data.todos.items.iter().find(|x| x.id == id);
    if let Some(item) = b.overdue.first().and_then(|id| find(*id)) {
        let due = item.snoozed_until.or(item.due).unwrap_or(cx.now);
        let txt = format!("{} · {}", fmt_due(due, item.all_day, cx.now, off, lang), item.text);
        cv.fill_circle(hr.x + 3.0, hr.y + hr.h * 0.5, 3.0, ACCENT_ROSE, 1.0);
        cv.text(&txt, Font::text(12.0, 500, Align::Left), Rect::new(hr.x + 12.0, hr.y, hr.w - 12.0, hr.h), TEXT_TITLE, 1.0);
    } else if let Some((item, due)) = b.next.and_then(|(id, d)| find(id).map(|i| (i, d))) {
        let txt = format!("{} · {} · {}", t(lang, Key::BriefNext), fmt_due(due, item.all_day, cx.now, off, lang), item.text);
        cv.fill_circle(hr.x + 3.0, hr.y + hr.h * 0.5, 3.0, ACCENT_AMBER, 1.0);
        cv.text(&txt, Font::text(12.0, 500, Align::Left), Rect::new(hr.x + 12.0, hr.y, hr.w - 12.0, hr.h), TEXT_TITLE, 1.0);
    } else {
        cv.text(t(lang, Key::BriefCalm), Font::text(12.0, 450, Align::Left), hr, TEXT_BODY, 1.0);
    }
}
