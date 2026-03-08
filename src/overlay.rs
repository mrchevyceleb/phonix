/// Native Windows always-on-top status pill.
/// Shows recording / transcribing / cleaning-up state near the top-right corner.
#[cfg(windows)]
mod win {
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::sync::Arc;

    use windows::core::w;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        BeginPaint, CreateFontW, CreateRoundRectRgn, CreateSolidBrush, Ellipse, EndPaint,
        FillRect, GetStockObject, InvalidateRect, SelectObject, SetBkMode, SetTextColor,
        SetWindowRgn, NULL_PEN, PAINTSTRUCT, TRANSPARENT,
    };
    use windows::Win32::UI::WindowsAndMessaging::*;

    const WIDTH: i32 = 180;
    const HEIGHT: i32 = 36;
    const CLASS_NAME: windows::core::PCWSTR = w!("PhonixRecOverlay");

    /// Overlay states (stored as AtomicU8)
    pub const STATE_HIDDEN: u8 = 0;
    pub const STATE_RECORDING: u8 = 1;
    pub const STATE_TRANSCRIBING: u8 = 2;
    pub const STATE_CLEANING: u8 = 3;

    pub struct Overlay {
        state: Arc<AtomicU8>,
    }

    unsafe impl Send for Overlay {}

    impl Overlay {
        pub fn new() -> Option<Self> {
            let state = Arc::new(AtomicU8::new(STATE_HIDDEN));
            let state2 = Arc::clone(&state);

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

                    // Store state pointer in window user data so wnd_proc can read it
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, Arc::into_raw(Arc::clone(&state2)) as isize);

                    let mut prev_state = STATE_HIDDEN;

                    loop {
                        let cur = state2.load(Ordering::Relaxed);
                        let visible = cur != STATE_HIDDEN;
                        let is_visible = IsWindowVisible(hwnd).as_bool();

                        if visible && !is_visible {
                            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                        } else if !visible && is_visible {
                            let _ = ShowWindow(hwnd, SW_HIDE);
                        }

                        // If state changed while visible, force a repaint
                        if cur != prev_state && visible {
                            let _ = InvalidateRect(hwnd, None, true);
                        }
                        prev_state = cur;

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

            Some(Self { state })
        }

        pub fn set_state(&self, s: u8) {
            self.state.store(s, Ordering::Relaxed);
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
        let x = screen_w - WIDTH - 20;
        let y = 12;

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
                let _ = SetLayeredWindowAttributes(hwnd, None, 230, LWA_ALPHA);
                LRESULT(0)
            }
            WM_PAINT => {
                // Read current state from window user data
                let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const AtomicU8;
                let cur_state = if !state_ptr.is_null() {
                    (*state_ptr).load(Ordering::Relaxed)
                } else {
                    STATE_RECORDING
                };

                let (dot_color, text_color, label) = match cur_state {
                    STATE_RECORDING => (0x00_46_46_FF, 0x00_60_60_FF, "Recording"),
                    STATE_TRANSCRIBING => (0x00_40_B8_FF, 0x00_50_D0_FF, "Transcribing\u{2026}"),
                    STATE_CLEANING => (0x00_E0_A0_20, 0x00_F0_C0_40, "Cleaning up\u{2026}"),
                    _ => (0x00_46_46_FF, 0x00_60_60_FF, "Recording"),
                };

                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);

                // Dark background
                let bg = CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x00_22_22_22));
                let _ = FillRect(hdc, &ps.rcPaint, bg);

                // Colored circle dot (no outline)
                let null_pen = GetStockObject(NULL_PEN);
                let old_pen = SelectObject(hdc, null_pen);
                let dot_brush = CreateSolidBrush(windows::Win32::Foundation::COLORREF(dot_color));
                let old_brush = SelectObject(hdc, dot_brush);
                let _ = Ellipse(hdc, 12, 10, 26, 24);
                let _ = SelectObject(hdc, old_brush);
                let _ = SelectObject(hdc, old_pen);

                // Status text
                let _ = SetBkMode(hdc, TRANSPARENT);
                SetTextColor(hdc, windows::Win32::Foundation::COLORREF(text_color));
                let font = CreateFontW(
                    17, 0, 0, 0, 600, 0, 0, 0, 0, 0, 0, 0, 0, w!("Segoe UI"),
                );
                let old_font = SelectObject(hdc, font);
                let mut text_buf: Vec<u16> = label.encode_utf16().collect();
                let mut text_rect = windows::Win32::Foundation::RECT {
                    left: 32,
                    top: 0,
                    right: WIDTH - 8,
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
#[cfg(windows)]
pub use win::{STATE_CLEANING, STATE_HIDDEN, STATE_RECORDING, STATE_TRANSCRIBING};

#[cfg(not(windows))]
pub struct Overlay;

#[cfg(not(windows))]
pub const STATE_HIDDEN: u8 = 0;
#[cfg(not(windows))]
pub const STATE_RECORDING: u8 = 1;
#[cfg(not(windows))]
pub const STATE_TRANSCRIBING: u8 = 2;
#[cfg(not(windows))]
pub const STATE_CLEANING: u8 = 3;

#[cfg(not(windows))]
impl Overlay {
    pub fn new() -> Option<Self> {
        Some(Self)
    }
    pub fn set_state(&self, _s: u8) {}
}
