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

/// Bề rộng của một ký tự theo nhóm, để ước lượng sát hơn một hằng số chung.
///
/// Font tỉ lệ: `i`, `l`, dấu cách rất hẹp; `W`, `M` rất rộng. Dùng một hệ số cho
/// mọi ký tự khiến cột toàn chữ hoa bị hụt và cột toàn chữ thường bị thừa.
fn char_px(c: char) -> f32 {
    match c {
        // Hẹp: dấu cách, dấu câu mảnh, chữ thường có nét dọc.
        ' ' | '.' | ',' | ':' | ';' | '\'' | '!' | '|' | 'i' | 'l' | 'j' | 'I' => 4.0,
        // Rộng: chữ hoa và chữ thường có nhiều nét ngang.
        'W' | 'M' | 'w' | 'm' | 'Q' | 'O' | '@' => 12.0,
        // Số và chữ hoa trung bình rộng hơn chữ thường.
        c if c.is_ascii_digit() => 8.0,
        c if c.is_ascii_uppercase() => 9.5,
        // Ký tự có dấu tiếng Việt chiếm gần như chữ thường cùng cỡ.
        _ => AVG_CHAR_PX,
    }
}

/// Bề rộng cần cho một chuỗi, trước khi kẹp theo giới hạn cột.
pub fn text_width_px(text: &str) -> u32 {
    let width: f32 = text.chars().map(char_px).sum();
    (width + CELL_PADDING_PX).ceil() as u32
}

/// Bề rộng vừa nội dung, kẹp trong `(min, max)`.
///
/// `texts` là các chuỗi sẽ hiển thị; `extra` là phần cố định của ô (icon, khoảng
/// cách) mà chuỗi không tính tới — cột tên và cột thẻ dùng tham số này.
pub fn fit_width<'a>(texts: impl Iterator<Item = &'a str>, min: u32, max: u32, extra: u32) -> u32 {
    let widest = texts.map(text_width_px).max().unwrap_or(0);
    (widest + extra).clamp(min, max)
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
        assert_eq!(fit_width(["a"].into_iter(), 120, 600, 0), 120);
        // Rất dài: không được vượt max.
        let long = "W".repeat(200);
        assert_eq!(fit_width([long.as_str()].into_iter(), 120, 600, 0), 600);
    }

    #[test]
    fn fit_width_uses_the_widest_entry() {
        let narrow = fit_width(["abc"].into_iter(), 50, 1000, 0);
        let wide = fit_width(["abc", "wwwwwwwwwwwwwwwwwwww"].into_iter(), 50, 1000, 0);
        assert!(wide > narrow, "mục dài nhất phải quyết định bề rộng");
    }

    #[test]
    fn extra_pixels_are_added_once() {
        let base = fit_width(["abcdefghij"].into_iter(), 10, 1000, 0);
        let with_icon = fit_width(["abcdefghij"].into_iter(), 10, 1000, 20);
        assert_eq!(with_icon, base + 20);
    }

    #[test]
    fn wide_letters_ask_for_more_room_than_narrow_ones() {
        // "WWWW" rộng hơn "iiii" dù cùng số ký tự — đây là lý do phải ước lượng
        // theo từng ký tự thay vì một hệ số chung.
        assert!(text_width_px("WWWW") > text_width_px("iiii"));
    }

    #[test]
    fn uppercase_and_digits_are_wider_than_lowercase() {
        assert!(text_width_px("AAAA") > text_width_px("aaaa"));
        assert!(text_width_px("1234") > text_width_px("iiii"));
    }
}
