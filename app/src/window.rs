//! Native window control.
//!
//! The window is created with `no-frame: true`, so Windows draws no title bar and
//! no resize border. Everything the user expects from a frame — drag, minimize,
//! maximize, close, and the snap layout on hover — has to be requested from the
//! window handle directly.
//!
//! Slint keeps owning the window; these calls only change its state.

use slint::ComponentHandle;

/// Attach the window commands to the UI callbacks.
///
/// Uses the winit backend's window accessor, which is the supported way to reach
/// the native handle from Slint without owning the event loop.
#[cfg(windows)]
pub fn wire(ui: &crate::AppWindow, app_id: *const std::cell::RefCell<crate::state::App>) {
    use slint::winit_030::WinitWindowAccessor;

    {
        let w = ui.as_weak();
        ui.on_window_minimize(move || {
            let Some(ui) = w.upgrade() else { return };
            ui.window().with_winit_window(|win| {
                if let Some(hwnd) = hwnd_of(win) {
                    minimize(hwnd);
                }
            });
        });
    }

    {
        let w = ui.as_weak();
        ui.on_window_toggle_maximize(move || {
            let Some(ui) = w.upgrade() else { return };
            ui.window().with_winit_window(|win| {
                if let Some(hwnd) = hwnd_of(win) {
                    set_maximized(hwnd, !is_zoomed(hwnd));
                }
            });
        });
    }

    {
        let w = ui.as_weak();
        ui.on_window_close(move || {
            let Some(ui) = w.upgrade() else { return };
            // Đóng cửa sổ phải đi qua registry: nếu chỉ `hide()` thì `App` và
            // luồng nền của cửa sổ đó vẫn sống mãi. Dùng địa chỉ của `App` để
            // đóng đúng cửa sổ, không đoán.
            crate::windows::close_by_app(app_id);
            if let Err(e) = ui.hide() {
                crate::logging::write(&format!("không ẩn được cửa sổ: {e}"));
            }
        });
    }

    {
        let w = ui.as_weak();
        ui.on_window_drag(move |_x, _y| {
            let Some(ui) = w.upgrade() else { return };
            // `winit` lo việc kéo cửa sổ: nó giữ đúng trạng thái chuột của
            // Slint và trả về ngay, thay vì chặn UI thread.
            //
            // Trước đây chỗ này gửi `WM_NCLBUTTONDOWN` + `HTCAPTION`. Cách đó
            // đúng với app Win32 thuần nhưng sai ở đây: `SendMessage` chạy vòng
            // lặp modal của Windows ngay trong handler của Slint, và Slint
            // không bao giờ nhận được `WM_LBUTTONUP` — nên nó tưởng chuột vẫn
            // đang nhấn và nuốt mọi cú click sau đó. Đó chính là lý do app
            // "không tương tác được".
            ui.window().with_winit_window(|win| {
                if let Err(e) = win.drag_window() {
                    // Không có console nên `eprintln!` sẽ mất hút.
                    crate::logging::write(&format!("drag_window lỗi: {e}"));
                }
            });
        });
    }

    {
        let w = ui.as_weak();
        ui.on_window_resize(move |edge| {
            let Some(ui) = w.upgrade() else { return };
            crate::logging::write(&format!("resize cửa sổ: {edge}"));
            ui.window().with_winit_window(|win| {
                use slint::winit_030::winit::window::ResizeDirection;
                let direction = match edge.as_str() {
                    "n" => ResizeDirection::North,
                    "s" => ResizeDirection::South,
                    "e" => ResizeDirection::East,
                    "w" => ResizeDirection::West,
                    "ne" => ResizeDirection::NorthEast,
                    "nw" => ResizeDirection::NorthWest,
                    "se" => ResizeDirection::SouthEast,
                    "sw" => ResizeDirection::SouthWest,
                    _ => return,
                };
                if let Err(e) = win.drag_resize_window(direction) {
                    crate::logging::write(&format!("drag_resize_window lỗi: {e}"));
                }
            });
        });
    }
}

/// Native HWND của cửa sổ Slint, dùng cho các API Win32 không có qua Slint.
#[cfg(windows)]
pub fn native_hwnd(ui: &crate::AppWindow) -> Option<HWND> {
    use slint::winit_030::WinitWindowAccessor;
    ui.window().with_winit_window(hwnd_of).flatten()
}

#[cfg(windows)]
fn hwnd_of(win: &slint::winit_030::winit::window::Window) -> Option<HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = win.window_handle().ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut core::ffi::c_void)),
        _ => None,
    }
}

#[cfg(windows)]
use windows::Win32::Foundation::HWND;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    IsZoomed, ShowWindow, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE,
};

// Toàn bộ `unsafe` của cửa sổ nằm trong ba hàm dưới đây. Điều kiện chung:
// `hwnd` lấy từ `hwnd_of` (cửa sổ winit đang sống, trên luồng UI). Win32 chỉ
// đọc hoặc đổi trạng thái cửa sổ; handle sai thì hàm trả lỗi/không làm gì,
// không gây UB.

/// Thu nhỏ cửa sổ.
#[cfg(windows)]
fn minimize(hwnd: HWND) {
    // SAFETY: xem khối chú thích trên. ShowWindow chỉ đổi trạng thái cửa sổ.
    // Giá trị BOOL trả về là trạng thái hiển thị trước đó, không phải lỗi,
    // nên bỏ qua là có chủ ý.
    let _ = unsafe { ShowWindow(hwnd, SW_MINIMIZE) };
}

/// Phóng to hoặc khôi phục cửa sổ (`true` = phóng to).
#[cfg(windows)]
fn set_maximized(hwnd: HWND, maximize: bool) {
    let cmd = if maximize { SW_MAXIMIZE } else { SW_RESTORE };
    // SAFETY: xem khối chú thích trên. Giá trị BOOL là trạng thái trước đó.
    let _ = unsafe { ShowWindow(hwnd, cmd) };
}

/// Cửa sổ đang phóng to hay không. Dùng cho nút và cho quyết định toggle.
#[cfg(windows)]
fn is_zoomed(hwnd: HWND) -> bool {
    // SAFETY: xem khối chú thích trên. IsZoomed chỉ đọc trạng thái.
    unsafe { IsZoomed(hwnd) }.as_bool()
}

/// Non-Windows builds keep the callbacks but do nothing, so the UI still builds.
#[cfg(not(windows))]
pub fn wire(_ui: &crate::AppWindow, _app_id: *const std::cell::RefCell<crate::state::App>) {}

/// Track whether the window is maximized so the button can show the right glyph.
///
/// Polled on a short timer instead of hooking `WM_SIZE`: Slint owns the message
/// loop, and a poll of one Win32 call is cheaper than fighting for the wndproc.
#[cfg(windows)]
pub fn install_state_poller(ui: &crate::AppWindow) {
    use slint::winit_030::WinitWindowAccessor;
    use slint::{Timer, TimerMode};
    use std::time::Duration;

    let w = ui.as_weak();
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(200), move || {
        let Some(ui) = w.upgrade() else { return };
        let maximized = ui
            .window()
            .with_winit_window(|win| hwnd_of(win).map(is_zoomed).unwrap_or(false))
            .unwrap_or(false);
        if ui.get_is_maximized() != maximized {
            ui.set_is_maximized(maximized);
        }
    });
    // Giữ timer sống theo cửa sổ.
    std::mem::forget(timer);
}

#[cfg(not(windows))]
pub fn install_state_poller(_ui: &crate::AppWindow) {}
