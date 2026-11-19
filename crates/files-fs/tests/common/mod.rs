//! Helper dùng chung cho các test tích hợp.
//!
//! Test môi trường (PowerShell, git, junction, Shell icon) có thể không chạy được
//! trên một số máy. Trước đây các test này `return` im lặng nên vẫn báo pass.
//! `skip` làm cho việc bỏ qua hiện rõ:
//! - Trên CI (biến `CI` được đặt): panic, vì CI phải có đủ điều kiện.
//! - Trên máy local: in `SKIPPED: <lý do>` ra stderr rồi để test kết thúc.
//!   Chạy với `cargo test -- --nocapture` để thấy các dòng này.

#![allow(dead_code)]

pub fn skip(reason: &str) {
    if std::env::var_os("CI").is_some() {
        panic!("test không được bỏ qua trên CI: {reason}");
    }
    eprintln!("SKIPPED: {reason}");
}
