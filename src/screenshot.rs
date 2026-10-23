//! Neutral backdrop for documentation screenshots. Never included in release builds.
use std::{mem::zeroed, ptr::null_mut};
use windows_sys::Win32::{Foundation::*, Graphics::Gdi::*, UI::WindowsAndMessaging::*};

pub struct Backdrop(HWND);

impl Backdrop {
    pub fn new(frame: HWND, scale: f32) -> Self {
        unsafe {
            let instance = GetWindowLongPtrW(frame, GWLP_HINSTANCE) as HINSTANCE;
            let name: Vec<u16> = "FrameRecordReadmeBackdrop\0".encode_utf16().collect();
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                lpszClassName: name.as_ptr(),
                ..zeroed()
            };
            assert_ne!(RegisterClassW(&class), 0);
            let mut rect = zeroed();
            GetWindowRect(frame, &mut rect);
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                name.as_ptr(),
                name.as_ptr(),
                WS_POPUP,
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                null_mut(),
                null_mut(),
                instance,
                ((scale * 1000.0) as usize) as *const _,
            );
            assert!(!hwnd.is_null());
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            UpdateWindow(hwnd);
            SetWindowPos(
                frame,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
            Self(hwnd)
        }
    }
}

impl Drop for Backdrop {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.0);
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            SetWindowLongPtrW(
                hwnd,
                GWLP_USERDATA,
                (*(lparam as *const CREATESTRUCTW)).lpCreateParams as isize,
            );
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            let mut paint = zeroed();
            let dc = BeginPaint(hwnd, &mut paint);
            let mut rect = zeroed();
            GetClientRect(hwnd, &mut rect);
            let tile = ((GetWindowLongPtrW(hwnd, GWLP_USERDATA) as f32 / 1000.0) * 28.0)
                .round()
                .max(1.0) as i32;
            for row in 0..=(rect.bottom / tile) {
                for column in 0..=(rect.right / tile) {
                    SetDCBrushColor(
                        dc,
                        if (row + column) % 2 == 0 {
                            0x181818
                        } else {
                            0x202020
                        },
                    );
                    FillRect(
                        dc,
                        &RECT {
                            left: column * tile,
                            top: row * tile,
                            right: (column + 1) * tile,
                            bottom: (row + 1) * tile,
                        },
                        GetStockObject(DC_BRUSH) as HBRUSH,
                    );
                }
            }
            EndPaint(hwnd, &paint);
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}
