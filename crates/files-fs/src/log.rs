//! A minimal log sink for this crate.
//!
//! The library reports non-fatal failures — a registry handle that will not
//! close, a Shell notification rejected — that must not abort the operation but
//! must not vanish either. It cannot call the application's logger directly
//! (that would invert the dependency), and `eprintln!` goes nowhere because the
//! shipping build has no console.
//!
//! So the application installs a sink once at startup. Until then, and in tests
//! that never install one, messages are dropped.

use std::sync::OnceLock;

type Sink = fn(&str);

static SINK: OnceLock<Sink> = OnceLock::new();

/// Install the process-wide sink. Later calls are ignored: one logger per
/// process, decided at startup, so two parts of the app cannot fight over it.
pub fn set_log_sink(f: Sink) {
    let _ = SINK.set(f);
}

/// Report a non-fatal problem. Does nothing when no sink is installed.
pub(crate) fn warn(msg: &str) {
    if let Some(sink) = SINK.get() {
        sink(msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warning_without_a_sink_is_silent_not_panicking() {
        // Chưa cài sink (hoặc test chạy trước khi app cài) thì phải im lặng,
        // tuyệt đối không panic.
        warn("không có ai nghe");
    }

    #[test]
    fn the_sink_can_only_be_installed_once() {
        // Cài lần hai phải bị bỏ qua chứ không ghi đè logger đang dùng.
        fn first(_: &str) {}
        fn second(_: &str) {}
        set_log_sink(first);
        set_log_sink(second);
        warn("vẫn gọi được");
    }
}
