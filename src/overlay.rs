/// Native Windows always-on-top recording indicator.
/// Shows a small dark pill with a red dot and "REC" text near the top-right corner.
#[cfg(windows)]
mod win {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use windows::core::w;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        BeginPaint, CreateFontW, CreateRoundRectRgn, CreateSolidBrush, Ellipse, EndPaint,
        FillRect, GetStockObject, SelectObject, SetBkMode, SetTextColor, SetWindowRgn,
        NULL_PEN, PAINTSTRUCT, TRANSPARENT,
    };
    use windows::Win32::UI::WindowsAndMessaging::*;

    const WIDTH: i32 = 84;
    const HEIGHT: i32 = 32;
    const CLASS_NAME: windows::core::PCWSTR = w!("PhonixRecOverlay");

    pub struct Overlay {
        visible: Arc<AtomicBool>,
    }

    unsafe impl Send for Overlay {}

    impl Overlay {
        pub fn new() -> Option<Self> {
            let visible = Arc::new(AtomicBool::new(false));
            let visible2 = Arc::clone(&visible);

            std::thread::Builder::new()
                .name("phonix-overlay".into())
                .spawn(move || unsafe {
                    register_class();
                    let hwnd = create_window();
                    if hwnd.0 == std::ptr::null_mut() {
                        return;
                    }

                    // Clip the window to a rounded rectangle (pill shape)
                    let rgn = CreateRoundRectRgn(0, 0, WIDTH + 1, HEIGHT + 1, HEIGHT, HEIGHT);
                    SetWindowRgn(hwnd, rgn, false);

                    loop {
                        let want = visible2.load(Ordering::Relaxed);
                        let is = IsWindowVisible(hwnd).as_bool();

                        if want && !is {
                            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                        } else if !want && is {
                            let _ = ShowWindow(hwnd, SW_HIDE);
                        }

                        // Pump messages for this window
                        let mut msg = MSG::default();
                        while PeekMessageW(&mut msg, hwnd, 0, 0, PM_REMOVE).as_bool() {
                            let _ = TranslateMessage(&msg);
                            DispatchMessageW(&msg);
                        }

                        std::thread::sleep(std::time::Duration::from_millis(30));
                    }
                })
                .ok()?;

            Some(Self { visible })
        }

        pub fn show(&self) {
            self.visible.store(true, Ordering::Relaxed);
        }

        pub fn hide(&self) {
            self.visible.store(false, Ordering::Relaxed);
        }
    }

    unsafe fn register_class() {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        RegisterClassExW(&wc);
    }

    unsafe fn create_window() -> HWND {
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let x = screen_w - WIDTH - 16;
        let y = 8;

        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            CLASS_NAME,
            w!(""),
            WS_POPUP,
            x,
            y,
            WIDTH,
            HEIGHT,
            None,
            None,
            None,
            None,
        )
        .unwrap_or_default()
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_CREATE => {
                let _ = SetLayeredWindowAttributes(hwnd, None, 220, LWA_ALPHA);
                LRESULT(0)
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);

                // Dark background
                let bg = CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x00_28_28_28));
                let _ = FillRect(hdc, &ps.rcPaint, bg);

                // Red circle dot (no outline)
                let null_pen = GetStockObject(NULL_PEN);
                let old_pen = SelectObject(hdc, null_pen);
                let dot_brush = CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x00_46_46_FF));
                let old_brush = SelectObject(hdc, dot_brush);
                let _ = Ellipse(hdc, 10, 8, 24, 22);
                let _ = SelectObject(hdc, old_brush);
                let _ = SelectObject(hdc, old_pen);

                // "REC" text in red
                let _ = SetBkMode(hdc, TRANSPARENT);
                SetTextColor(hdc, windows::Win32::Foundation::COLORREF(0x00_50_50_FF));
                let font = CreateFontW(
                    16, 0, 0, 0, 700, 0, 0, 0, 0, 0, 0, 0, 0, w!("Segoe UI"),
                );
                let old_font = SelectObject(hdc, font);
                let mut text_buf: Vec<u16> = "REC".encode_utf16().collect();
                let mut text_rect = windows::Win32::Foundation::RECT {
                    left: 28,
                    top: 7,
                    right: WIDTH,
                    bottom: HEIGHT,
                };
                windows::Win32::Graphics::Gdi::DrawTextW(
                    hdc,
                    &mut text_buf,
                    &mut text_rect,
                    windows::Win32::Graphics::Gdi::DT_LEFT
                        | windows::Win32::Graphics::Gdi::DT_SINGLELINE
                        | windows::Win32::Graphics::Gdi::DT_VCENTER,
                );
                let _ = SelectObject(hdc, old_font);
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

#[cfg(windows)]
pub use win::Overlay;

#[cfg(not(windows))]
pub struct Overlay;

#[cfg(not(windows))]
impl Overlay {
    pub fn new() -> Option<Self> {
        Some(Self)
    }
    pub fn show(&self) {}
    pub fn hide(&self) {}
}
