//! Ước lượng bề rộng cột cho "tự vừa nội dung" (auto-fit).
//!
//! Không đo được chữ thật ở tầng dữ liệu — việc đó cần font engine của UI. Nên
//! ước lượng theo số ký tự với hệ số rộng trung bình của font Fluent ở cỡ 14px,
//! rồi kẹp theo giới hạn cột. Đủ để một tên dài không bị cắt oan và một cột toàn
//! số ngắn không chiếm chỗ vô ích.
//!
//! Hệ số là xấp xỉ có chủ ý: sai số vài pixel không làm hỏng bố cục, và người
//! dùng vẫn kéo lại được bằng tay.

/// Bề rộng trung bình của một ký tự ở cỡ chữ thân (14px), tính bằng pixel.
///
/// Đo trên font Segoe UI Variable ở cỡ 14: chữ thường vào khoảng 6.6–7.4 px.
/// Lấy 7.2 để nghiêng về phía rộng hơn — cắt bớt tên khó chịu hơn thừa vài pixel.
const AVG_CHAR_PX: f32 = 7.2;

/// Khoảng đệm hai bên và phần icon của một ô, tính bằng pixel.
const CELL_PADDING_PX: f32 = 24.0;

/// Bề rộng cần cho một chuỗi, trước khi kẹp theo giới hạn cột.
pub fn text_width_px(text: &str) -> u32 {
    let chars = text.chars().count() as f32;
    (chars * AVG_CHAR_PX + CELL_PADDING_PX).ceil() as u32
}

/// Bề rộng vừa nội dung, kẹp trong `(min, max)`.
///
/// `extra` là phần cố định của ô (icon, khoảng cách) mà chuỗi không tính tới;
/// `tag` và `git` dùng tham số này.
pub fn fit_width(texts: impl Iterator<Item = usize>, min: u32, max: u32, extra: u32) -> u32 {
    let widest = texts.max().unwrap_or(0);
    let needed = (widest as f32 * AVG_CHAR_PX + CELL_PADDING_PX + extra as f32).ceil() as u32;
    needed.clamp(min, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_text_still_gets_the_padding() {
        // Cột toàn giá trị rỗng vẫn phải đủ chỗ cho header.
        assert_eq!(text_width_px(""), CELL_PADDING_PX as u32);
    }

    #[test]
    fn longer_text_wants_more_room() {
        assert!(text_width_px("mot") < text_width_px("mot chuoi dai hon nhieu"));
    }

    #[test]
    fn fit_width_respects_the_bounds() {
        // Rất ngắn: không được nhỏ hơn min.
        assert_eq!(fit_width([1, 2].into_iter(), 120, 600, 0), 120);
        // Rất dài: không được vượt max.
        assert_eq!(fit_width([500].into_iter(), 120, 600, 0), 600);
    }

    #[test]
    fn fit_width_uses_the_widest_entry() {
        let narrow = fit_width([3].into_iter(), 50, 1000, 0);
        let wide = fit_width([3, 40].into_iter(), 50, 1000, 0);
        assert!(wide > narrow, "mục dài nhất phải quyết định bề rộng");
    }

    #[test]
    fn extra_pixels_are_added_once() {
        let base = fit_width([10].into_iter(), 10, 1000, 0);
        let with_icon = fit_width([10].into_iter(), 10, 1000, 20);
        assert_eq!(with_icon, base + 20);
    }
}
