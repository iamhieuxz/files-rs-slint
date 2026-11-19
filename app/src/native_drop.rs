//! Nhận tệp kéo từ Explorer hoặc ứng dụng khác qua OLE IDropTarget.
//!
//! Slint/winit không chuyển tiếp drag-and-drop của hệ điều hành, nên cửa sổ
//! đăng ký một drop target Win32 riêng. Các callback OLE chạy trên luồng UI;
//! chúng cập nhật cùng trạng thái `drag` mà thao tác kéo nội bộ đang dùng.

#[cfg(windows)]
mod imp {
    use crate::drag;
    use crate::state::App;
    use crate::{logging, window, AppWindow};
    use slint::ComponentHandle;
    use std::cell::{Cell, RefCell};
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;
    use std::ptr;
    use std::rc::Rc;
    use windows::core::implement;
    use windows::Win32::Foundation::{HWND, POINT, POINTL};
    use windows::Win32::Graphics::Gdi::ScreenToClient;
    use windows::Win32::System::Com::{IDataObject, DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
    use windows::Win32::System::Ole::{
        IDropTarget, IDropTarget_Impl, OleInitialize, RegisterDragDrop, RevokeDragDrop, CF_HDROP,
        DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE,
    };
    use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
    use windows::Win32::UI::Shell::{DragFinish, DragQueryFileW, HDROP};

    thread_local! {
        static TARGETS: RefCell<Vec<(isize, IDropTarget)>> = const { RefCell::new(Vec::new()) };
    }

    #[implement(IDropTarget)]
    struct DropTarget {
        hwnd: HWND,
        ui: slint::Weak<AppWindow>,
        app: Rc<RefCell<App>>,
        paths: RefCell<Vec<PathBuf>>,
        effect: Cell<DROPEFFECT>,
    }

    impl DropTarget {
        fn new(hwnd: HWND, ui: slint::Weak<AppWindow>, app: Rc<RefCell<App>>) -> Self {
            Self {
                hwnd,
                ui,
                app,
                paths: RefCell::new(Vec::new()),
                effect: Cell::new(DROPEFFECT_NONE),
            }
        }

        fn pointer(&self, pt: &POINTL) -> Option<(f32, f32)> {
            let mut p = POINT { x: pt.x, y: pt.y };
            // SAFETY: hwnd thuộc cửa sổ đang sống; ScreenToClient chỉ đọc/đổi POINT.
            if !unsafe { ScreenToClient(self.hwnd, &mut p) }.as_bool() {
                return None;
            }
            let ui = self.ui.upgrade()?;
            let scale = ui.window().scale_factor();
            Some((p.x as f32 / scale, p.y as f32 / scale))
        }

        fn show(&self, pt: &POINTL) {
            let Some(ui) = self.ui.upgrade() else { return };
            let Some((x, y)) = self.pointer(pt) else {
                return;
            };
            let count = self.paths.borrow().len();
            drag::native_show(&ui, &self.app, count, x, y);
        }
    }

    /// Đọc danh sách đường dẫn từ `CF_HDROP` của một `IDataObject`.
    ///
    /// # Safety
    ///
    /// `data_obj` phải là một `IDataObject` còn hiệu lực, và hàm phải được gọi
    /// trên luồng đã khởi tạo OLE (luồng UI). `medium.u.hGlobal` là handle do
    /// OLE cấp và chỉ được dùng trong hàm này: nó được `DragQueryFileW` đọc ngay
    /// và `DragFinish` giải phóng trước khi trả về, nên không có handle nào sống
    /// sót ra ngoài. Bộ đệm UTF-16 được cấp theo độ dài Windows báo, và chỉ đọc
    /// `len` phần tử đầu.
    unsafe fn paths_from(data_obj: &IDataObject) -> Vec<PathBuf> {
        let format = FORMATETC {
            cfFormat: CF_HDROP.0,
            ptd: ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };
        // SAFETY: `format` mô tả đúng clipboard format và sống qua lời gọi;
        // `data_obj` do OLE bảo đảm còn hiệu lực trong suốt callback.
        let Ok(medium) = (unsafe { data_obj.GetData(&format) }) else {
            return Vec::new();
        };
        let hdrop = HDROP(medium.u.hGlobal.0 as _);
        // SAFETY: `hdrop` là HDROP hợp lệ vừa lấy từ `GetData`; chỉ số
        // `0xFFFF_FFFF` là quy ước "cho biết số tệp" của `DragQueryFileW`.
        let count = unsafe { DragQueryFileW(hdrop, 0xFFFF_FFFF, None) };
        let mut paths = Vec::with_capacity(count as usize);
        for index in 0..count {
            // SAFETY: `index` nằm trong `0..count` mà Windows vừa trả về.
            let len = unsafe { DragQueryFileW(hdrop, index, None) } as usize;
            let mut buffer = vec![0u16; len + 1];
            // SAFETY: bộ đệm dài `len + 1` phần tử, đúng độ dài Windows yêu cầu
            // cho tên này; hàm ghi tối đa chừng ấy phần tử UTF-16.
            unsafe { DragQueryFileW(hdrop, index, Some(&mut buffer)) };
            paths.push(PathBuf::from(OsString::from_wide(&buffer[..len])));
        }
        // SAFETY: giải phóng `hdrop` đúng một lần sau khi đã đọc xong; không
        // dùng lại handle này ở bất kỳ đâu phía sau.
        unsafe { DragFinish(hdrop) };
        paths
    }

    #[allow(non_snake_case)]
    impl IDropTarget_Impl for DropTarget_Impl {
        fn DragEnter(
            &self,
            pDataObj: windows::core::Ref<'_, IDataObject>,
            _grfKeyState: MODIFIERKEYS_FLAGS,
            pt: &POINTL,
            pdwEffect: *mut DROPEFFECT,
        ) -> windows::core::Result<()> {
            let paths = match pDataObj.as_ref() {
                // SAFETY: pDataObj sống trong suốt callback; paths được copy ra
                // và HDROP được DragFinish ngay sau khi đọc.
                Some(obj) => unsafe { paths_from(obj) },
                None => Vec::new(),
            };
            logging::write(&format!("drag enter: {} tệp", paths.len()));
            let effect = if paths.is_empty() {
                DROPEFFECT_NONE
            } else {
                DROPEFFECT_COPY
            };
            self.effect.set(effect);
            self.paths.replace(paths);
            if effect == DROPEFFECT_COPY {
                self.show(pt);
            }
            // SAFETY: OLE truyền `pdwEffect` trỏ tới bộ nhớ của nó, hợp lệ ghi
            // trong suốt callback; ghi đúng một giá trị DROPEFFECT.
            unsafe { *pdwEffect = effect };
            Ok(())
        }

        fn DragOver(
            &self,
            _grfKeyState: MODIFIERKEYS_FLAGS,
            pt: &POINTL,
            pdwEffect: *mut DROPEFFECT,
        ) -> windows::core::Result<()> {
            let effect = if self.paths.borrow().is_empty() {
                DROPEFFECT_NONE
            } else {
                DROPEFFECT_COPY
            };
            self.effect.set(effect);
            if effect == DROPEFFECT_COPY {
                self.show(pt);
            }
            // SAFETY: như DragEnter — `pdwEffect` do OLE cấp cho callback này.
            unsafe { *pdwEffect = effect };
            Ok(())
        }

        fn DragLeave(&self) -> windows::core::Result<()> {
            self.paths.borrow_mut().clear();
            if let Some(ui) = self.ui.upgrade() {
                drag::native_hide(&ui);
            }
            Ok(())
        }

        fn Drop(
            &self,
            pDataObj: windows::core::Ref<'_, IDataObject>,
            _grfKeyState: MODIFIERKEYS_FLAGS,
            pt: &POINTL,
            pdwEffect: *mut DROPEFFECT,
        ) -> windows::core::Result<()> {
            let mut paths = std::mem::take(&mut *self.paths.borrow_mut());
            if paths.is_empty() {
                if let Some(obj) = pDataObj.as_ref() {
                    // SAFETY: như DragEnter.
                    paths = unsafe { paths_from(obj) };
                }
            }
            let Some(ui) = self.ui.upgrade() else {
                // SAFETY: `pdwEffect` do OLE cấp cho callback này; ghi một giá trị.
                unsafe { *pdwEffect = DROPEFFECT_NONE };
                return Ok(());
            };
            match self.pointer(pt) {
                Some((x, y)) if !paths.is_empty() => {
                    drag::native_drop(&ui, &self.app, paths, x, y);
                    // SAFETY: như trên; thả được chấp nhận nên báo COPY.
                    unsafe { *pdwEffect = DROPEFFECT_COPY };
                }
                _ => {
                    drag::native_hide(&ui);
                    // SAFETY: như trên; không có đích hợp lệ nên báo NONE.
                    unsafe { *pdwEffect = DROPEFFECT_NONE };
                }
            }
            Ok(())
        }
    }

    fn register(ui: &AppWindow, app: &Rc<RefCell<App>>, hwnd: HWND) {
        let already =
            TARGETS.with(|targets| targets.borrow().iter().any(|(h, _)| *h == hwnd.0 as isize));
        if already {
            return;
        }
        // SAFETY: gọi trên luồng UI đã có cửa sổ; `hwnd` lấy từ chính cửa sổ
        // đó. `RevokeDragDrop` trả lỗi nếu chưa từng đăng ký, nên bỏ qua an toàn.
        unsafe {
            if let Err(e) = OleInitialize(None) {
                logging::write(&format!("OleInitialize lỗi: {e}"));
            }
            let _ = RevokeDragDrop(hwnd);
        }
        let target: IDropTarget = DropTarget::new(hwnd, ui.as_weak(), app.clone()).into();
        // SAFETY: `target` là COM object đã AddRef qua `into()`; `hwnd` hợp lệ.
        // Tham chiếu được giữ trong `TARGETS` bên dưới nên sống bằng cửa sổ.
        match unsafe { RegisterDragDrop(hwnd, &target) } {
            Ok(()) => {
                logging::write(&format!("đăng ký nhận tệp kéo-thả: hwnd={:?}", hwnd.0));
                TARGETS.with(|targets| {
                    targets.borrow_mut().push((hwnd.0 as isize, target));
                });
            }
            Err(e) => logging::write(&format!("RegisterDragDrop lỗi: {e}")),
        }
    }

    pub fn install(ui: &AppWindow, app: &Rc<RefCell<App>>) -> Rc<slint::Timer> {
        // HWND chỉ có sau khi winit xử lý ít nhất một vòng sự kiện; thử lại
        // bằng timer thay vì bỏ mất đăng ký.
        let w = ui.as_weak();
        let a = app.clone();
        let timer = Rc::new(slint::Timer::default());
        let weak_timer = Rc::downgrade(&timer);
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(100),
            move || {
                let Some(ui) = w.upgrade() else { return };
                let Some(hwnd) = window::native_hwnd(&ui) else {
                    return;
                };
                register(&ui, &a, hwnd);
                if let Some(timer) = weak_timer.upgrade() {
                    timer.stop();
                }
            },
        );
        // Trả về timer (không `mem::forget`): cửa sổ sở hữu nó và dừng khi đóng.
        timer
    }

    pub fn revoke(ui: &AppWindow) {
        let Some(hwnd) = window::native_hwnd(ui) else {
            return;
        };
        // SAFETY: gỡ đúng HWND của cửa sổ này; nếu chưa từng đăng ký thì lỗi được bỏ qua.
        unsafe {
            let _ = RevokeDragDrop(hwnd);
        }
        TARGETS.with(|targets| {
            targets.borrow_mut().retain(|(h, _)| *h != hwnd.0 as isize);
        });
    }
}

#[cfg(not(windows))]
mod imp {
    use crate::state::App;
    use crate::AppWindow;
    use std::cell::RefCell;
    use std::rc::Rc;

    pub fn install(_ui: &AppWindow, _app: &Rc<RefCell<App>>) -> Rc<slint::Timer> {
        Rc::new(slint::Timer::default())
    }
    pub fn revoke(_ui: &AppWindow) {}
}

pub use imp::{install, revoke};
