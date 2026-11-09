//! Tiny string table (English / Vietnamese).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    #[default]
    En,
    Vi,
}

impl Lang {
    /// Pick from a BCP-47 locale name such as `vi-VN`.
    pub fn from_locale(locale: &str) -> Self {
        if locale.to_ascii_lowercase().starts_with("vi") { Lang::Vi } else { Lang::En }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    TabMusic,
    TabFocus,
    TabTodo,
    NothingPlaying,
    NothingPlayingHint,
    Focus,
    ShortBreak,
    LongBreak,
    Start,
    Pause,
    Reset,
    Skip,
    FocusDone,
    BreakDone,
    SessionsToday,
    AddTask,
    AddTaskHint,
    NoTasks,
    Reminder,
    Charging,
    BatteryLow,
    HeadphonesConnected,
    Volume,
    Muted,
    Brightness,
    Offline,
    StartWithWindows,
    MinimalNotch,
    Language,
    About,
    Exit,
    MinuteShort,
}

pub fn t(lang: Lang, key: Key) -> &'static str {
    use Key::*;
    match lang {
        Lang::En => match key {
            TabMusic => "Music",
            TabFocus => "Focus",
            TabTodo => "To-do",
            NothingPlaying => "Nothing playing",
            NothingPlayingHint => "Play something in Spotify, a browser or any player",
            Focus => "Focus",
            ShortBreak => "Short break",
            LongBreak => "Long break",
            Start => "Start",
            Pause => "Pause",
            Reset => "Reset",
            Skip => "Skip",
            FocusDone => "Focus session done — take a break",
            BreakDone => "Break over — ready to focus?",
            SessionsToday => "sessions today",
            AddTask => "Add a task…",
            AddTaskHint => "Enter to add · \"@15:30\" sets a reminder",
            NoTasks => "Nothing to do. Nice.",
            Reminder => "Reminder",
            Charging => "Charging",
            BatteryLow => "Battery low",
            HeadphonesConnected => "Connected",
            Volume => "Volume",
            Muted => "Muted",
            Brightness => "Brightness",
            Offline => "Offline",
            StartWithWindows => "Start with Windows",
            MinimalNotch => "Minimal notch",
            Language => "Tiếng Việt",
            About => "About Ailan",
            Exit => "Exit",
            MinuteShort => "m",
        },
        Lang::Vi => match key {
            TabMusic => "Nhạc",
            TabFocus => "Tập trung",
            TabTodo => "Việc cần làm",
            NothingPlaying => "Không có gì đang phát",
            NothingPlayingHint => "Phát nhạc trên Spotify, trình duyệt hoặc bất kỳ trình phát nào",
            Focus => "Tập trung",
            ShortBreak => "Nghỉ ngắn",
            LongBreak => "Nghỉ dài",
            Start => "Bắt đầu",
            Pause => "Tạm dừng",
            Reset => "Đặt lại",
            Skip => "Bỏ qua",
            FocusDone => "Xong phiên tập trung — nghỉ chút nhé",
            BreakDone => "Hết giờ nghỉ — tập trung tiếp?",
            SessionsToday => "phiên hôm nay",
            AddTask => "Thêm việc…",
            AddTaskHint => "Enter để thêm · \"@15:30\" để đặt nhắc",
            NoTasks => "Không còn việc gì. Tuyệt!",
            Reminder => "Nhắc việc",
            Charging => "Đang sạc",
            BatteryLow => "Pin yếu",
            HeadphonesConnected => "Đã kết nối",
            Volume => "Âm lượng",
            Muted => "Tắt tiếng",
            Brightness => "Độ sáng",
            Offline => "Mất mạng",
            StartWithWindows => "Khởi động cùng Windows",
            MinimalNotch => "Notch tối giản",
            Language => "English",
            About => "Giới thiệu Ailan",
            Exit => "Thoát",
            MinuteShort => "p",
        },
    }
}

pub fn weekday(lang: Lang, wd: u32) -> &'static str {
    const EN: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    const VI: [&str; 7] = ["T2", "T3", "T4", "T5", "T6", "T7", "CN"];
    match lang {
        Lang::En => EN[wd as usize % 7],
        Lang::Vi => VI[wd as usize % 7],
    }
}

/// Short date such as "Tue 10 Nov" / "T3, 10/11".
pub fn short_date(lang: Lang, c: &crate::time::Civil) -> String {
    const MON: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    match lang {
        Lang::En => format!("{} {} {}", weekday(lang, c.weekday), c.day, MON[(c.month as usize + 11) % 12]),
        Lang::Vi => format!("{}, {}/{}", weekday(lang, c.weekday), c.day, c.month),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_detection() {
        assert_eq!(Lang::from_locale("vi-VN"), Lang::Vi);
        assert_eq!(Lang::from_locale("en-US"), Lang::En);
        assert_eq!(Lang::from_locale(""), Lang::En);
    }
}
