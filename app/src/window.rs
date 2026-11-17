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
pub fn wire(ui: &crate::AppWindow) {
    use slint::winit_030::WinitWindowAccessor;

    {
        let w = ui.as_weak();
        ui.on_window_minimize(move || {
            let Some(ui) = w.upgrade() else { return };
            ui.window().with_winit_window(|win| {
                if let Some(hwnd) = hwnd_of(win) {
                    // SW_MINIMIZE = 6
                    let _ = unsafe { ShowWindow(hwnd, SW_MINIMIZE) };
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
                    // SW_MAXIMIZE = 3, SW_RESTORE = 9
                    let maximized = unsafe { IsZoomed(hwnd) }.as_bool();
                    unsafe {
                        let _ = ShowWindow(hwnd, if maximized { SW_RESTORE } else { SW_MAXIMIZE });
                    }
                }
            });
        });
    }

    {
        let w = ui.as_weak();
        ui.on_window_close(move || {
            let Some(ui) = w.upgrade() else { return };
            // Slint owns the loop; asking it to hide is the clean exit path.
            let _ = ui.hide();
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
                    eprintln!("[window] drag_window lỗi: {e}");
                }
            });
        });
    }
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

/// Non-Windows builds keep the callbacks but do nothing, so the UI still builds.
#[cfg(not(windows))]
pub fn wire(_ui: &crate::AppWindow) {}

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
            .with_winit_window(|win| {
                hwnd_of(win)
                    .map(|h| unsafe { IsZoomed(h) }.as_bool())
                    .unwrap_or(false)
            })
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
