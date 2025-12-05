use chrono::Local;

pub enum Level {
    Info,
    Warn,
    Error,
    Debug,
}

/// 统一日志输出函数
/// 格式: [Time] [LEVEL] [Target      ] Message
pub fn print(level: Level, target: &str, args: std::fmt::Arguments) {
    let now = Local::now().format("%H:%M:%S");

    // ANSI 颜色代码
    let gray = "\x1b[90m";
    let reset = "\x1b[0m";
    let cyan = "\x1b[36m";

    // Level 颜色与标签
    let (color, level_str) = match level {
        Level::Info => ("\x1b[32m", "INFO"),  // Green
        Level::Warn => ("\x1b[33m", "WARN"),  // Yellow
        Level::Error => ("\x1b[31m", "ERRO"), // Red
        Level::Debug => ("\x1b[34m", "DEBG"), // Blue
    };

    println!(
        "{}[{}] {}[{}] {} {}{}{} {}",
        gray,
        now,
        color,
        level_str,
        reset,
        cyan,
        format_args!("[{}]", target),
        reset,
        args
    );
}

#[macro_export]
macro_rules! info {
    (target: $target:expr, $($arg:tt)+) => (
        $crate::log::print($crate::log::Level::Info, $target, format_args!($($arg)+))
    );
    ($($arg:tt)+) => (
        $crate::log::print($crate::log::Level::Info, "System", format_args!($($arg)+))
    );
}

#[macro_export]
macro_rules! warn {
    (target: $target:expr, $($arg:tt)+) => (
        $crate::log::print($crate::log::Level::Warn, $target, format_args!($($arg)+))
    );
    ($($arg:tt)+) => (
        $crate::log::print($crate::log::Level::Warn, "System", format_args!($($arg)+))
    );
}

#[macro_export]
macro_rules! error {
    (target: $target:expr, $($arg:tt)+) => (
        $crate::log::print($crate::log::Level::Error, $target, format_args!($($arg)+))
    );
    ($($arg:tt)+) => (
        $crate::log::print($crate::log::Level::Error, "System", format_args!($($arg)+))
    );
}

#[macro_export]
macro_rules! debug {
    (target: $target:expr, $($arg:tt)+) => (
        $crate::log::print($crate::log::Level::Debug, $target, format_args!($($arg)+))
    );
    ($($arg:tt)+) => (
        $crate::log::print($crate::log::Level::Debug, "System", format_args!($($arg)+))
    );
}
