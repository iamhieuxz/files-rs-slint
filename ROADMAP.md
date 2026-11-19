# ROADMAP.md — kế hoạch phát triển files-rs-slint

> Nguồn sự thật duy nhất về việc còn thiếu. Các kế hoạch cũ đã hoàn thành nằm
> trong `docs/archive/` và chỉ dùng để tra cứu lịch sử.
>
> Cách dùng: làm từ trên xuống. Xong mục nào đánh `[x]` ngay tại mục đó và ghi
> số đo thật. Không đánh dấu khi chưa chạy kiểm chứng tương ứng.

## Trạng thái nền

Đo ngày 2026-11-19, sau khi hoàn thành giai đoạn A.

| Chỉ số | Giá trị |
|---|---|
| Test | 249 pass, 0 hỏng (`cargo test --workspace`) |
| Clippy | 0 cảnh báo (`--all-targets -- -D warnings`) |
| Format | `cargo fmt --all -- --check` sạch |
| Module > 800 dòng | `app.slint` 2166, `commands.rs` 990, `wiring.rs` 912 |
| Action bản gốc Files | 173 file; ước tính bản Rust phủ ~55–60% |
| Settings page bản gốc | 9 trang; bản Rust có 0 |

Độ khó: **D** dưới nửa ngày · **T** 1–3 ngày · **K** 1–2 tuần · **RK** nhiều tuần.

---

## Giai đoạn A: Hoàn tất nền tảng còn dở (làm trước)

Các mục này nhỏ nhưng chặn chất lượng của mọi thứ phía sau.

- [x] **A1. `// SAFETY:` cho `native_drop.rs`** — D
  - 17 điểm `unsafe` đều có `// SAFETY:`; `paths_from` có khối `# Safety` riêng.
- [x] **A2. Watcher không bỏ sót sự kiện** — D
  - Thêm `App::watch_dirty`: sự kiện tới trong lúc đang tải được ghi nhớ và xếp lại sau.
  - `panes::activate` nạp lại khung vừa kích hoạt vì watcher chỉ theo khung active.
  - Test: `a_pending_watch_event_is_remembered_until_a_reload_is_queued`.
- [x] **A3. Kiểm tra khoảng trắng trong CI** — D
  - Bước `Diff hygiene` quét tệp văn bản đã track (bỏ qua png/ico/zip/exe).
  - `git diff --check` trên checkout sạch luôn rỗng nên không dùng được.
- [x] **A4. Đồng bộ README với thực tế** — D
  - Số test, danh sách tính năng, kiến trúc, mục "Giới hạn đã biết".
  - Hướng dẫn gỡ đăng ký khớp đúng bốn khoá `unregister` thật sự xoá.

---

## Giai đoạn B: Thùng rác đầy đủ

Thiếu lớn nhất so với bản gốc và người dùng gặp hằng ngày.

- [ ] **B1. Duyệt Thùng rác** — K
  - Dùng `SHGetDesktopFolder` + `IShellFolder::EnumObjects` trên `CSIDL_BITBUCKET` (cách `SHGetKnownFolderPath` hiện trả `E_FAIL`).
  - Hiện tên gốc, ngày xoá, kích thước.
  - Bật `can_browse()` chỉ khi đã chạy được.
- [ ] **B2. Khôi phục từng mục** — K
  - Lấy verb `undelete` qua `IContextMenu` của từng mục, gọi `InvokeCommand`.
  - Chạy trên luồng STA, giải phóng COM đúng thứ tự.
  - Bật `can_restore()` sau khi có test thật.
- [ ] **B3. Khôi phục tất cả** — T (phụ thuộc B2).
- [ ] **B4. Nhóm theo thư mục gốc / ngày xoá** — T (phụ thuộc B1).

**Rủi ro:** COM + `IContextMenu` dễ rò bộ nhớ và treo; phải có test trên máy Windows thật, không chạy trên CI không có Thùng rác.

---

## Giai đoạn C: Settings trong UI

- [ ] **C1. Khung trang Settings** — T
  - View `settings` trong `app.slint`, mở bằng Ctrl+, và từ menu.
  - Bốn trang đầu: Appearance, Layout, Folders, About.
- [ ] **C2. Ghi qua `Settings::save`** — D
  - Mỗi thay đổi lưu ngay; giữ `with_ui_from`/`with_pins_from` cho đa cửa sổ.
- [ ] **C3. Trang Tags** — T
  - Liệt kê, đổi tên, đổi màu, thêm/xoá thẻ; không đổi `uid` để giữ liên kết.
- [ ] **C4. Trang Advanced** — T
  - Đường dẫn log/settings/tags, nút mở thư mục, mở tệp cấu hình.
- [ ] **C5. About** — D
  - Phiên bản, giấy phép, liên kết repo.

---

## Giai đoạn D: Chế độ xem và sắp xếp

- [ ] **D1. Virtualization danh sách** — K
  - Đo trước: mở thư mục 10.000 và 100.000 mục, ghi thời gian dựng và cuộn.
  - Thay `for row[i]` trong `ScrollView` bằng `ListView` của Slint.
  - Đo lại, ghi số liệu vào mục này.
- [ ] **D2. Auto-fit cột** — D
  - Đo độ rộng chữ dài nhất, kẹp theo `column_bounds` đã có.
- [ ] **D3. Nhóm (Group by)** — T
  - Nhóm theo tên, ngày sửa, loại, kích thước, thẻ.
  - Header nhóm trong danh sách; lưu lựa chọn vào settings.
- [ ] **D4. Ba biến thể sắp xếp** — D
  - Tách `folders_first` thành ba lựa chọn: thư mục trước, tệp trước, sắp chung.
- [ ] **D5. Chọn đảo + checkbox chọn** — D
  - `selection::invert`; checkbox ở cột đầu và `ToggleSelect`.
- [ ] **D6. Thumbnail trong grid** — T
  - Grid đang dùng glyph; dùng `has-icon-image` như details.

---

## Giai đoạn E: Cửa sổ, pane và tab

- [ ] **E1. Kéo tab đổi thứ tự** — T
- [ ] **E2. Kéo tab ra cửa sổ mới** — T (dùng lại `windows::open`).
- [ ] **E3. Chia pane ngang/dọc** — T
  - Hiện chỉ có hai khung cạnh nhau; thêm hướng chia và lưu vào settings.
- [ ] **E4. Đóng pane đang active** — D
- [ ] **E5. Fullscreen (F11)** — D
- [ ] **E6. Compact overlay** — T
  - `winit` đặt always-on-top và thu nhỏ.
- [ ] **E7. Info pane tách khỏi preview pane** — T

---

## Giai đoạn F: Thao tác tệp còn thiếu

- [ ] **F1. Tạo shortcut (.lnk)** — T
  - `IShellLinkW` + `IPersistFile`; đã có phần đọc đích nên tái dùng được.
- [ ] **F2. Dán dạng shortcut** — D (phụ thuộc F1).
- [ ] **F3. Gom mục đã chọn vào thư mục mới** — D
- [ ] **F4. Làm phẳng thư mục** — T
- [ ] **F5. Sao chép đường dẫn có ngoặc kép** — D
- [ ] **F6. Mở thư mục chứa mục đang chọn** — D
- [ ] **F7. Hộp thoại Properties của Windows** — T
  - `SHObjectProperties` hoặc `IContextMenu` verb `properties`.
- [ ] **F8. Alt+Enter mở thuộc tính** — D
- [ ] **F9. Xoay trái** — D (đã có xoay phải).
- [ ] **F10. Giải nén vào thư mục con tự chọn** — T
- [ ] **F11. Nén 7z** — K (cần thêm crate; cân nhắc trước).
- [ ] **F12. Alternate data stream** — T (rủi ro, chỉ làm nếu có yêu cầu).

---

## Giai đoạn G: Shell và hệ thống

Chỉ làm khi người dùng yêu cầu rõ — nhóm này chạm hệ thống và có rủi ro cao.

- [ ] **G1. Chạy as admin / as another user** — T
- [ ] **G2. Mở Terminal as admin** — D
- [ ] **G3. Chạy bằng PowerShell** — D
- [ ] **G4. Share item** — T
- [ ] **G5. Pin to Start / Unpin** — T
- [ ] **G6. Mở Storage Sense** — D
- [ ] **G7. Cài font / chứng chỉ / INF** — K (rủi ro cao, cần xác nhận).
- [ ] **G8. Format ổ đĩa** — K (rất rủi ro; cân nhắc từ chối làm).
- [ ] **G9. Đặt hình nền / lockscreen / slideshow** — T

---

## Giai đoạn H: Git ghi và đồng bộ

- [ ] **H1. Clone** — T
- [ ] **H2. Init** — D
- [ ] **H3. Fetch / Pull / Push / Sync** — K
  - Dùng `git` CLI qua `std::process::Command`, chạy nền, hiện tiến trình.
  - Push/Pull phải hỏi xác nhận trước.
  - Báo rõ khi máy chưa cài `git`.

---

## Giai đoạn I: Shelf và Sidebar nâng cao

- [ ] **I1. Shelf pane** — K
  - Vùng tạm giữ mục để chép/cắt/xoá; lưu bền.
- [ ] **I2. Sidebar mục con** — T
  - Đã có sườn `expandable`; cần nạp thư mục con và trạng thái mở rộng.
- [ ] **I3. Copy item from sidebar** — D

---

## Giai đoạn J: Accessibility

- [ ] **J1. Nhãn cho hàng tệp** — T
  - `accessible-role`, `accessible-label` gồm tên, loại, kích thước, trạng thái chọn.
- [ ] **J2. Trạng thái chọn đọc được** — T
  - Slint 1.19 chưa có `accessible-selected`; dùng `accessible-description`.
- [ ] **J3. Kiểm tra bằng Narrator + Accessibility Insights** — T
- [ ] **J4. Điều hướng bàn phím đầy đủ** — T
  - Home/End/PageUp/PageDown, gõ chữ để nhảy tới mục.

---

## Giai đoạn K: Cloud, mạng và FTP

- [ ] **K1. Network (UNC)** — T
  - Duyệt `\\server\share`; breadcrumb đã hỗ trợ UNC.
- [ ] **K2. FTP/SFTP** — K
- [ ] **K3. Cloud drive** — RK (phụ thuộc dịch vụ ngoài).

---

## Giai đoạn L: Đóng gói và phát hành

- [ ] **L1. CHANGELOG.md** — D
- [ ] **L2. Ký mã** — K (cần mua chứng chỉ; quyết định của người dùng).
- [ ] **L3. Installer (MSIX hoặc Inno Setup)** — T
- [ ] **L4. Tách `app.slint`** — T
  - Tách palette và dialog thành component nhận `in property`/`callback`.

---

## Thứ tự đề xuất

1. **A** (nền tảng) — xong nhanh, gỡ nợ kỹ thuật.
2. **B** (Thùng rác) — giá trị người dùng cao nhất trong nhóm còn thiếu.
3. **C** (Settings) — mở đường cho các tuỳ chọn của D và E.
4. **D1** (virtualization) — chặn trước khi thư mục lớn thành vấn đề.
5. **D2–D6, E, F** — theo phản hồi người dùng.
6. **G, H, I, J, K** — theo yêu cầu cụ thể.

## Quy ước khi làm

- Mỗi mục phải có test hồi quy hoặc lý do rõ ràng vì sao kiểm tra thủ công.
- Giữ `cargo test --workspace`, clippy `-D warnings` và `cargo fmt --check` xanh.
- Mục UI phải chạy app thật và ghi lại đã kiểm tra gì.
- Mục chạm Win32/COM phải có `// SAFETY:` và giải phóng tài nguyên đúng thứ tự.
- Không đánh dấu `[x]` nếu chưa đo lại số liệu.
