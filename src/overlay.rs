/// Native Windows always-on-top status pill.
/// Shows recording / transcribing / cleaning-up state near the top-right corner.
/// Uses UpdateLayeredWindow with a 32-bit ARGB bitmap for proper per-pixel alpha.
#[cfg(windows)]
mod win {
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::sync::Arc;

    use windows::core::w;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Graphics::Gdi::*;
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

                    let mut prev_state = STATE_HIDDEN;

                    loop {
                        let cur = state2.load(Ordering::Relaxed);
                        let visible = cur != STATE_HIDDEN;
                        let is_visible = IsWindowVisible(hwnd).as_bool();

                        if visible && !is_visible {
                            paint_layered(hwnd, cur);
                            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                        } else if !visible && is_visible {
                            let _ = ShowWindow(hwnd, SW_HIDE);
                        }

                        // If state changed while visible, repaint
                        if cur != prev_state && visible {
                            paint_layered(hwnd, cur);
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
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
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

    /// Render the pill into a 32-bit ARGB bitmap and call UpdateLayeredWindow.
    unsafe fn paint_layered(hwnd: HWND, state: u8) {
        let (dot_r, dot_g, dot_b, label) = match state {
            STATE_RECORDING => (255u8, 70, 70, "Recording"),
            STATE_TRANSCRIBING => (255, 184, 64, "Transcribing\u{2026}"),
            STATE_CLEANING => (32, 160, 224, "Cleaning up\u{2026}"),
            _ => (255, 70, 70, "Recording"),
        };

        let w = WIDTH as usize;
        let h = HEIGHT as usize;
        let mut pixels = vec![0u32; w * h];

        // Pill radius = half the height
        let radius = h as f32 / 2.0;

        // Fill pill-shaped background with per-pixel alpha
        let bg_r: u8 = 30;
        let bg_g: u8 = 30;
        let bg_b: u8 = 30;
        let bg_a: u8 = 220;

        for y in 0..h {
            for x in 0..w {
                if is_inside_pill(x as f32, y as f32, w as f32, h as f32, radius) {
                    // Premultiplied alpha: each channel = channel * alpha / 255
                    let pa = bg_a as u32;
                    let pr = (bg_r as u32 * pa) / 255;
                    let pg = (bg_g as u32 * pa) / 255;
                    let pb = (bg_b as u32 * pa) / 255;
                    pixels[y * w + x] = (pa << 24) | (pr << 16) | (pg << 8) | pb;
                }
            }
        }

        // Draw the colored dot (circle) at left side
        let dot_cx = 18.0_f32;
        let dot_cy = h as f32 / 2.0;
        let dot_radius = 6.0_f32;
        for y in 0..h {
            for x in 0..w {
                let dx = x as f32 - dot_cx;
                let dy = y as f32 - dot_cy;
                let dist = (dx * dx + dy * dy).sqrt();
                if dist <= dot_radius {
                    let a = 255u32;
                    let r = dot_r as u32;
                    let g = dot_g as u32;
                    let b = dot_b as u32;
                    pixels[y * w + x] = (a << 24) | (r << 16) | (g << 8) | b;
                }
            }
        }

        // Render text using GDI onto the bitmap
        let screen_dc = GetDC(HWND(std::ptr::null_mut()));
        let mem_dc = CreateCompatibleDC(screen_dc);

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: WIDTH,
                biHeight: -HEIGHT, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0, // BI_RGB
                ..Default::default()
            },
            ..Default::default()
        };

        let mut bits_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        let dib = CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut bits_ptr, None, 0)
            .expect("CreateDIBSection");

        let old_bmp = SelectObject(mem_dc, dib);

        // Copy our pixel buffer into the DIB
        std::ptr::copy_nonoverlapping(
            pixels.as_ptr() as *const u8,
            bits_ptr as *mut u8,
            w * h * 4,
        );

        // Draw text with GDI (premultiplied white)
        let _ = SetBkMode(mem_dc, TRANSPARENT);
        // Use white text - premultiplied at full alpha
        SetTextColor(mem_dc, windows::Win32::Foundation::COLORREF(0x00_FF_FF_FF));

        let font = CreateFontW(
            16, 0, 0, 0, 600, 0, 0, 0, 0, 0, 0, 0, 0, w!("Segoe UI"),
        );
        let old_font = SelectObject(mem_dc, font);

        let mut text_buf: Vec<u16> = label.encode_utf16().collect();
        let mut text_rect = windows::Win32::Foundation::RECT {
            left: 32,
            top: 0,
            right: WIDTH - 8,
            bottom: HEIGHT,
        };
        DrawTextW(
            mem_dc,
            &mut text_buf,
            &mut text_rect,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
        );

        let _ = SelectObject(mem_dc, old_font);

        // Now we need to fix premultiplied alpha for the text pixels.
        // GDI DrawText writes RGB but doesn't set alpha. We need to scan
        // the bitmap and set alpha=255 for any pixel that GDI touched
        // (where the pixel changed from what we had).
        let bmp_slice = std::slice::from_raw_parts_mut(bits_ptr as *mut u32, w * h);
        for i in 0..w * h {
            let current = bmp_slice[i];
            let original = pixels[i];
            if current != original {
                // GDI wrote here. It writes non-premultiplied RGB with A=0.
                // Extract the RGB that GDI wrote, blend with our background.
                let gdi_r = (current >> 16) & 0xFF;
                let gdi_g = (current >> 8) & 0xFF;
                let gdi_b = current & 0xFF;

                if gdi_r > 0 || gdi_g > 0 || gdi_b > 0 {
                    // Text pixel: set full alpha, premultiplied
                    bmp_slice[i] = (255 << 24) | (gdi_r << 16) | (gdi_g << 8) | gdi_b;
                } else {
                    // GDI cleared it but didn't write text here, restore original
                    bmp_slice[i] = original;
                }
            }
        }

        // UpdateLayeredWindow
        let pt_src = windows::Win32::Foundation::POINT { x: 0, y: 0 };
        let size = windows::Win32::Foundation::SIZE { cx: WIDTH, cy: HEIGHT };
        let blend = BLENDFUNCTION {
            BlendOp: 0, // AC_SRC_OVER
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: 1, // AC_SRC_ALPHA
        };

        let screen_x = GetSystemMetrics(SM_CXSCREEN) - WIDTH - 20;
        let pt_dst = windows::Win32::Foundation::POINT { x: screen_x, y: 12 };

        let _ = UpdateLayeredWindow(
            hwnd,
            screen_dc,
            Some(&pt_dst),
            Some(&size),
            mem_dc,
            Some(&pt_src),
            windows::Win32::Foundation::COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );

        let _ = SelectObject(mem_dc, old_bmp);
        let _ = DeleteObject(dib);
        let _ = DeleteDC(mem_dc);
        ReleaseDC(HWND(std::ptr::null_mut()), screen_dc);
    }

    /// Check if a point is inside a pill (stadium) shape.
    fn is_inside_pill(x: f32, y: f32, w: f32, h: f32, r: f32) -> bool {
        let r = r.min(w / 2.0).min(h / 2.0);
        // The pill is a rectangle with semicircle caps on left and right
        if x >= r && x <= w - r {
            // In the central rectangle
            y >= 0.0 && y < h
        } else if x < r {
            // Left cap
            let dx = x - r;
            let dy = y - h / 2.0;
            dx * dx + dy * dy <= r * r
        } else {
            // Right cap
            let dx = x - (w - r);
            let dy = y - h / 2.0;
            dx * dx + dy * dy <= r * r
        }
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

#[cfg(windows)]
pub use win::Overlay;
#[cfg(windows)]
pub use win::{STATE_CLEANING, STATE_HIDDEN, STATE_RECORDING, STATE_TRANSCRIBING};

#[cfg(target_os = "macos")]
mod mac {
    use std::os::raw::c_void;
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::sync::Arc;

    use cocoa::appkit::{
        NSView, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
    };
    use cocoa::base::{id, nil, NO, YES};
    use cocoa::foundation::{NSAutoreleasePool, NSPoint, NSRect, NSSize, NSString};
    use objc::{class, msg_send, sel, sel_impl};

    const WIDTH: f64 = 180.0;
    const HEIGHT: f64 = 36.0;

    pub const STATE_HIDDEN: u8 = 0;
    pub const STATE_RECORDING: u8 = 1;
    pub const STATE_TRANSCRIBING: u8 = 2;
    pub const STATE_CLEANING: u8 = 3;

    /// Wrapper to send raw ObjC pointers across threads.
    #[derive(Clone, Copy)]
    struct SendId(id);
    unsafe impl Send for SendId {}

    pub struct Overlay {
        state: Arc<AtomicU8>,
    }

    unsafe impl Send for Overlay {}

    // ── GCD helpers ──────────────────────────────────────────────────────────

    extern "C" {
        fn dispatch_get_main_queue() -> id;
        fn dispatch_async_f(queue: id, context: *mut c_void, work: extern "C" fn(*mut c_void));
    }

    /// Dispatch a closure to the main thread via GCD. All AppKit operations
    /// must happen on the main thread; the polling thread uses this to
    /// marshal show/hide/update calls safely.
    unsafe fn dispatch_on_main<F: FnOnce() + Send + 'static>(f: F) {
        extern "C" fn trampoline<F: FnOnce()>(ctx: *mut c_void) {
            unsafe { Box::from_raw(ctx as *mut F)() }
        }
        let ctx = Box::into_raw(Box::new(f)) as *mut c_void;
        dispatch_async_f(dispatch_get_main_queue(), ctx, trampoline::<F>);
    }

    // ── Overlay ──────────────────────────────────────────────────────────────

    impl Overlay {
        /// Create the overlay. Must be called from the main thread (before
        /// eframe::run_native takes over) so NSWindow creation is safe.
        pub fn new() -> Option<Self> {
            let state = Arc::new(AtomicU8::new(STATE_HIDDEN));
            let state2 = Arc::clone(&state);

            // Create window on the main thread (we are on it right now).
            let (window, view) = unsafe { create_overlay_window() };
            let w = SendId(window);
            let v = SendId(view);

            // Background thread polls the atomic state and dispatches
            // AppKit show/hide/update calls to the main thread via GCD.
            std::thread::Builder::new()
                .name("phonix-overlay".into())
                .spawn(move || {
                    let mut prev_state = STATE_HIDDEN;

                    loop {
                        let cur = state2.load(Ordering::Relaxed);

                        if cur != prev_state {
                            let visible = cur != STATE_HIDDEN;
                            let was_visible = prev_state != STATE_HIDDEN;
                            let state_val = cur;

                            unsafe {
                                dispatch_on_main(move || {
                                    let _pool = NSAutoreleasePool::new(nil);
                                    if visible && !was_visible {
                                        update_pill_view(v.0, state_val);
                                        let _: () = msg_send![w.0, orderFrontRegardless];
                                    } else if !visible && was_visible {
                                        let _: () = msg_send![w.0, orderOut: nil];
                                    } else if visible {
                                        update_pill_view(v.0, state_val);
                                        let _: () = msg_send![v.0, setNeedsDisplay: YES];
                                    }
                                });
                            }
                            prev_state = cur;
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

    /// Create the NSWindow + content view on the current (main) thread.
    unsafe fn create_overlay_window() -> (id, id) {
        let _pool = NSAutoreleasePool::new(nil);

        let screen: id = msg_send![class!(NSScreen), mainScreen];
        let screen_frame: NSRect = msg_send![screen, frame];
        let screen_w = screen_frame.size.width;

        let x = screen_w - WIDTH - 20.0;
        let y = screen_frame.size.height - HEIGHT - 12.0; // macOS origin is bottom-left

        let frame = NSRect::new(NSPoint::new(x, y), NSSize::new(WIDTH, HEIGHT));
        let style = NSWindowStyleMask::NSBorderlessWindowMask;
        let window = NSWindow::alloc(nil).initWithContentRect_styleMask_backing_defer_(
            frame,
            style,
            cocoa::appkit::NSBackingStoreType::NSBackingStoreBuffered,
            NO,
        );

        window.setLevel_(20); // kCGStatusWindowLevel
        window.setOpaque_(NO);
        let clear: id = msg_send![class!(NSColor), clearColor];
        window.setBackgroundColor_(clear);
        let _: () = msg_send![window, setIgnoresMouseEvents: YES];
        window.setCollectionBehavior_(
            NSWindowCollectionBehavior::NSWindowCollectionBehaviorCanJoinAllSpaces
                | NSWindowCollectionBehavior::NSWindowCollectionBehaviorStationary,
        );
        let _: () = msg_send![window, setHasShadow: NO];

        let view = create_pill_view();
        window.setContentView_(view);

        (window, view)
    }

    /// Create the layer-backed pill view with dot + text sublayers.
    unsafe fn create_pill_view() -> id {
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WIDTH, HEIGHT));
        let view: id = msg_send![class!(NSView), alloc];
        let view: id = msg_send![view, initWithFrame: frame];

        let _: () = msg_send![view, setWantsLayer: YES];

        let layer: id = msg_send![view, layer];
        let _: () = msg_send![layer, setCornerRadius: HEIGHT / 2.0];
        let _: () = msg_send![layer, setMasksToBounds: YES];

        let bg_color: id = msg_send![class!(NSColor), colorWithRed:30.0/255.0 green:30.0/255.0 blue:30.0/255.0 alpha:220.0/255.0];
        let cg_color: id = msg_send![bg_color, CGColor];
        let _: () = msg_send![layer, setBackgroundColor: cg_color];

        // Dot sublayer (index 0)
        let dot_layer: id = msg_send![class!(CALayer), layer];
        let dot_size = 12.0_f64;
        let dot_frame = NSRect::new(
            NSPoint::new(12.0, (HEIGHT - dot_size) / 2.0),
            NSSize::new(dot_size, dot_size),
        );
        let _: () = msg_send![dot_layer, setFrame: dot_frame];
        let _: () = msg_send![dot_layer, setCornerRadius: dot_size / 2.0];
        let red: id = msg_send![class!(NSColor), colorWithRed:255.0/255.0 green:70.0/255.0 blue:70.0/255.0 alpha:1.0];
        let red_cg: id = msg_send![red, CGColor];
        let _: () = msg_send![dot_layer, setBackgroundColor: red_cg];
        let _: () = msg_send![layer, addSublayer: dot_layer];

        // Text sublayer (index 1)
        let text_layer: id = msg_send![class!(CATextLayer), layer];
        let text_frame = NSRect::new(
            NSPoint::new(30.0, 0.0),
            NSSize::new(WIDTH - 38.0, HEIGHT),
        );
        let _: () = msg_send![text_layer, setFrame: text_frame];
        let font_size: f64 = 13.0;
        let _: () = msg_send![text_layer, setFontSize: font_size];
        let white: id = msg_send![class!(NSColor), whiteColor];
        let white_cg: id = msg_send![white, CGColor];
        let _: () = msg_send![text_layer, setForegroundColor: white_cg];
        let _: () = msg_send![text_layer, setAlignmentMode: NSString::alloc(nil).init_str("left")];
        let _: () = msg_send![text_layer, setContentsScale: 2.0f64]; // Retina
        let _: () = msg_send![text_layer, setTruncationMode: NSString::alloc(nil).init_str("end")];
        let font: id = msg_send![class!(NSFont), systemFontOfSize:font_size weight:0.4];
        let _: () = msg_send![text_layer, setFont: font];
        let _: () = msg_send![layer, addSublayer: text_layer];

        view
    }

    /// Update the pill view's dot color and label text based on state.
    unsafe fn update_pill_view(view: id, state: u8) {
        let layer: id = msg_send![view, layer];
        let sublayers: id = msg_send![layer, sublayers];
        let count: usize = msg_send![sublayers, count];
        if count < 2 {
            return;
        }

        let dot_layer: id = msg_send![sublayers, objectAtIndex: 0usize];
        let text_layer: id = msg_send![sublayers, objectAtIndex: 1usize];

        let (r, g, b, label) = match state {
            STATE_RECORDING => (255.0/255.0, 70.0/255.0, 70.0/255.0, "Recording"),
            STATE_TRANSCRIBING => (255.0/255.0, 184.0/255.0, 64.0/255.0, "Transcribing\u{2026}"),
            STATE_CLEANING => (32.0/255.0, 160.0/255.0, 224.0/255.0, "Cleaning up\u{2026}"),
            _ => (255.0/255.0, 70.0/255.0, 70.0/255.0, "Recording"),
        };

        let color: id = msg_send![class!(NSColor), colorWithRed:r green:g blue:b alpha:1.0f64];
        let cg_color: id = msg_send![color, CGColor];
        let _: () = msg_send![dot_layer, setBackgroundColor: cg_color];

        let ns_string = NSString::alloc(nil).init_str(label);
        let _: () = msg_send![text_layer, setString: ns_string];
    }
}

#[cfg(target_os = "macos")]
pub use mac::Overlay;
#[cfg(target_os = "macos")]
pub use mac::{STATE_CLEANING, STATE_HIDDEN, STATE_RECORDING, STATE_TRANSCRIBING};

#[cfg(not(any(windows, target_os = "macos")))]
pub struct Overlay;

#[cfg(not(any(windows, target_os = "macos")))]
pub const STATE_HIDDEN: u8 = 0;
#[cfg(not(any(windows, target_os = "macos")))]
pub const STATE_RECORDING: u8 = 1;
#[cfg(not(any(windows, target_os = "macos")))]
pub const STATE_TRANSCRIBING: u8 = 2;
#[cfg(not(any(windows, target_os = "macos")))]
pub const STATE_CLEANING: u8 = 3;

#[cfg(not(any(windows, target_os = "macos")))]
impl Overlay {
    pub fn new() -> Option<Self> {
        Some(Self)
    }
    pub fn set_state(&self, _s: u8) {}
}
