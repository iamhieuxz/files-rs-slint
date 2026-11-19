# Manual test checklist

Chạy checklist này trước mỗi bản phát hành. Đánh dấu từng mục khi đã làm và
kết quả đúng như mô tả. Automation UIA/DPI không ổn định trên máy có cửa sổ
khác tranh focus, nên các mục kéo-thả phải làm bằng chuột thật.

## Chuẩn bị

- [ ] `cargo test --workspace` xanh.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` xanh.
- [ ] `cargo build --release` xanh.
- [ ] Tạo thư mục thử với: một thư mục con, một tệp văn bản, một tệp ZIP,
      một ảnh, và một thư mục đích trống.

## 1. Khởi động và cửa sổ

- [ ] App mở ở thư mục người dùng, tab đầu tiên hiện tên thư mục.
- [ ] Kéo vùng trống title bar: cửa sổ di chuyển.
- [ ] Thu nhỏ, phóng to, khôi phục, đóng hoạt động đúng glyph.
- [ ] Kéo 8 cạnh/góc: cửa sổ đổi kích thước hai chiều, con trỏ đổi đúng cạnh.
- [ ] Mở cửa sổ thứ hai: mỗi cửa sổ có tab và thư mục riêng.

## 2. Điều hướng và sidebar

- [ ] Ctrl+L, gõ đường dẫn, Enter: mở đúng thư mục.
- [ ] Back/Forward/Up và F5 hoạt động.
- [ ] Click thư mục trong sidebar: mở trong khung đang nhận lệnh.
- [ ] Ghim/bỏ ghim thư mục; ghim còn sau khi khởi động lại.
- [ ] Ổ đĩa mở đúng, nhóm Ổ đĩa thu gọn/mở được.

## 3. Hai khung

- [ ] Nút "hai khung" trên command bar bật/tắt được; trạng thái nút đổi màu.
- [ ] Khi bật: hai khung rộng bằng nhau, hai thanh đường dẫn thẳng hàng.
- [ ] Click khung phải: thanh accent và nền thanh đường dẫn chuyển sang phải.
- [ ] Điều hướng khung phải bằng address bar hoặc double-click không đổi khung trái.
- [ ] Điều hướng khung trái không đổi khung phải.
- [ ] Thêm tab mới: tab thuộc khung trái và khung trái được kích hoạt.
- [ ] Tắt hai khung: khung trái vẫn ở đúng thư mục, khung phải biến mất.
- [ ] Ẩn/hiện sidebar và preview khi đang hai khung: hai khung vẫn chia đều.

## 4. Kéo-thả nội bộ

- [ ] Kéo một tệp từ khung trái sang khung phải: tệp được **di chuyển**.
- [ ] Giữ Ctrl khi kéo: tệp được **sao chép**, bản gốc còn nguyên.
- [ ] Trong lúc kéo: có ghost nhỏ gần con trỏ, ghi "Di chuyển/Sao chép N mục".
- [ ] Khung đích có viền/nền accent và dòng "Thả vào thư mục này".
- [ ] Con trỏ đổi thành move/copy; ngoài vùng thả đổi thành no-drop.
- [ ] Thả vào khoảng trống của pane: vào đúng thư mục đang mở của pane đó.
- [ ] Thả vào một mục sidebar: tệp vào đúng thư mục của mục đó.
- [ ] Thả vào chính thư mục nguồn: không có thao tác lạ.
- [ ] Kéo từ lưới (grid) hoạt động giống details.
- [ ] Sau khi thả, cả hai pane tự làm mới; trạng thái/thông báo đúng.

## 5. Tệp kéo từ Explorer/ứng dụng khác

- [ ] Kéo tệp từ Explorer vào pane: tệp được sao chép vào thư mục pane.
- [ ] Trong lúc kéo, pane đích sáng lên; con trỏ là copy.
- [ ] Kéo vào một mục sidebar: tệp được sao chép vào đúng thư mục đó.
- [ ] Kéo nhiều tệp cùng lúc: đếm đúng số mục, tất cả được chép.
- [ ] Kéo ra ngoài cửa sổ rồi thả: không có tệp nào bị chép.
- [ ] Tệp trùng tên: bản mới được đổi tên "(2)", không ghi đè.

## 6. Thao tác tệp và phản hồi

- [ ] Sao chép/Cắt/Dán trong pane đang active; menu và phím tắt cùng kết quả.
- [ ] Đổi tên, tạo tệp/thư mục, xoá vào Thùng rác, xoá vĩnh viễn.
- [ ] Nén thư mục lớn: thẻ tiến trình hiện sau ~300ms, có đếm giây.
- [ ] Thẻ tiến trình không hiện với thao tác xong trước 300ms.
- [ ] Thao tác nhiều mục: thẻ hiện "đã xong/tổng mục" và phần trăm.
- [ ] Nén/giải nén/ một mục: thanh chạy vô định + đếm giây.
- [ ] Huỷ thao tác nhiều mục: dừng ở ranh giới tệp, thông báo "Đã huỷ…".
- [ ] Khi xong: toast xanh "Hoàn tất sau N giây"; lỗi: toast đỏ; tự đóng sau ~4 giây.
- [ ] Nút đóng toast hoạt động; thẻ đang chạy không che nút Huỷ.
- [ ] Sau thao tác, cả hai pane (nếu bật) làm mới và giữ vị trí hợp lý.
- [ ] Hoàn tác/làm lại chạy nền, không treo UI.

## 7. Menu chuột phải và Open with

- [ ] Menu rộng 300px; nhãn dài bị elide; phím tắt căn phải.
- [ ] Menu không tràn khỏi cửa sổ; menu dài cuộn được.
- [ ] Menu mở đúng điểm chuột và không tự đóng khi nhả chuột phải.
- [ ] Nhóm "Thẻ màu" và menu con "Mở bằng" hiển thị đúng.
- [ ] "Mở bằng" liệt kê ứng dụng thật; chọn app mở đúng tệp.
- [ ] "Ứng dụng khác…" mở hộp thoại Open with của Windows.
- [ ] Open with không hiện cho thư mục.

## 8. Danh sách, cột và panel

- [ ] Click header để sort; mũi tên đổi chiều.
- [ ] Kéo vạch chia cột: đổi rộng; điểm neo sáng khi hover.
- [ ] Kéo header ngang: đổi thứ tự cột; layout còn sau khởi động lại.
- [ ] Ở chế độ hai khung, cột phụ tự ẩn; không có thanh cuộn ngang lạ.
- [ ] Chuyển details/grid: cả hai pane đổi đúng.
- [ ] Preview cập nhật khi chọn tệp/ảnh; panel ẩn/hiện được.
- [ ] Tìm nhanh Ctrl+F và tìm đệ quy Ctrl+Shift+F; huỷ được.
- [ ] Mục ẩn/phần mở rộng/thư mục trước tệp áp dụng cho cả hai pane.

## 9. Lỗi và biên

- [ ] Mở thư mục không tồn tại: thông báo lỗi, không mất tab.
- [ ] Thư mục không có quyền: thông báo có gợi ý chạy quyền cao hơn.
- [ ] Danh sách rỗng: trạng thái rỗng đúng, không có dòng giả.
- [ ] Tên tệp rất dài: elide, không đẩy cột khác.
- [ ] Menu/thẻ tiến trình/dialog không đè lên nhau sai thứ tự.
- [ ] Đóng cửa sổ khi thao tác đang chạy: app vẫn thoát sạch.

## Ghi chú automation

- UIA chỉ đọc được cây accessibility sau khi cửa sổ hiện; chụp ảnh nên dùng
  `PrintWindow`.
- Trên máy có nhiều cửa sổ tranh focus, thao tác chuột theo toạ độ có thể
  lệch do DPI. Các mục kéo-thả phải kiểm tra bằng tay.
