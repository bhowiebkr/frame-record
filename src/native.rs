//! Window geometry only; egui renders every visible control.
use crate::recorder::Region;
use std::{
    cell::Cell,
    ffi::OsString,
    mem::{size_of, zeroed},
    os::windows::ffi::OsStringExt,
    path::PathBuf,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Dwm::*, Gdi::*},
    System::{Com::CoTaskMemFree, JobObjects::*, SystemInformation::GetLocalTime},
    UI::{Shell::*, WindowsAndMessaging::*},
};

pub const HEADER: f32 = 92.0;
pub const BORDER: f32 = 4.0;
const SUBCLASS: usize = 0x4652;
type Shape = (i32, i32, i32, i32, Option<[i32; 4]>);
struct State {
    scale: Cell<f32>,
    recording: Cell<bool>,
}
pub struct NativeFrame {
    pub hwnd: HWND,
    state: Box<State>,
    shape: Cell<Option<Shape>>,
}

impl NativeFrame {
    /// # Safety
    /// `hwnd` must be the live top-level window that owns this frame; it is subclassed until drop.
    pub unsafe fn new(hwnd: HWND, scale: f32) -> Self {
        let state = Box::new(State {
            scale: Cell::new(scale),
            recording: Cell::new(false),
        });
        SetWindowSubclass(
            hwnd,
            Some(window_proc),
            SUBCLASS,
            (&*state as *const State) as usize,
        );
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        SetWindowLongPtrW(
            hwnd,
            GWL_STYLE,
            ((style | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX) & !WS_CAPTION & !WS_MAXIMIZEBOX)
                as isize,
        );
        SetWindowPos(
            hwnd,
            null_mut(),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED,
        );
        let corners = DWMWCP_DONOTROUND;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            (&corners as *const DWM_WINDOW_CORNER_PREFERENCE).cast(),
            size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
        );
        let frame = Self {
            hwnd,
            state,
            shape: Cell::new(None),
        };
        frame.update(scale, false, None);
        frame
    }

    pub fn update(&self, scale: f32, recording: bool, overlay: Option<eframe::egui::Rect>) {
        self.state.scale.set(scale);
        self.state.recording.set(recording);
        unsafe {
            let mut rect = zeroed();
            GetClientRect(self.hwnd, &mut rect);
            if IsIconic(self.hwnd) != 0 || rect.right < 2 {
                return;
            }
            let border = (BORDER * scale).round() as i32;
            let header = (HEADER * scale).round() as i32;
            let overlay = overlay.map(|r| {
                [
                    (r.left() * scale).floor() as i32,
                    (r.top() * scale).floor() as i32,
                    (r.right() * scale).ceil() as i32,
                    (r.bottom() * scale).ceil() as i32,
                ]
            });
            let shape = (rect.right, rect.bottom, border, header, overlay);
            if self.shape.replace(Some(shape)) == Some(shape) {
                return;
            }
            let outer = CreateRectRgn(0, 0, rect.right, rect.bottom);
            let inner = CreateRectRgn(border, header, rect.right - border, rect.bottom - border);
            CombineRgn(outer, outer, inner, RGN_DIFF);
            DeleteObject(inner);
            if let Some([left, top, right, bottom]) = overlay {
                let popup = CreateRectRgn(left, top, right, bottom);
                CombineRgn(outer, outer, popup, RGN_OR);
                DeleteObject(popup);
            }
            if SetWindowRgn(self.hwnd, outer, 1) == 0 {
                DeleteObject(outer);
            }
        }
    }

    pub fn region(&self) -> Region {
        unsafe {
            let border = (BORDER * self.state.scale.get()).round() as i32;
            let header = (HEADER * self.state.scale.get()).round() as i32;
            let mut rect = zeroed();
            GetClientRect(self.hwnd, &mut rect);
            let mut origin = POINT {
                x: border,
                y: header,
            };
            ClientToScreen(self.hwnd, &mut origin);
            Region {
                x: origin.x,
                y: origin.y,
                width: (rect.right - 2 * border) & !1,
                height: (rect.bottom - header - border) & !1,
            }
        }
    }

    pub fn validate_region(&self) -> Result<Region, String> {
        let region = self.region();
        unsafe {
            let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
            if region.x < x
                || region.y < y
                || region.x + region.width > x + GetSystemMetrics(SM_CXVIRTUALSCREEN)
                || region.y + region.height > y + GetSystemMetrics(SM_CYVIRTUALSCREEN)
            {
                return Err("Move the whole frame onto the screen before recording.".into());
            }
        }
        Ok(region)
    }
}

impl Drop for NativeFrame {
    fn drop(&mut self) {
        unsafe {
            if IsWindow(self.hwnd) != 0 {
                RemoveWindowSubclass(self.hwnd, Some(window_proc), SUBCLASS);
            }
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    data: usize,
) -> LRESULT {
    let state = &*(data as *const State);
    let px = |n: f32| (n * state.scale.get()).round() as i32;
    match message {
        WM_NCCALCSIZE => 0,
        WM_NCHITTEST => {
            let mut point = POINT {
                x: lparam as i16 as i32,
                y: (lparam >> 16) as i16 as i32,
            };
            ScreenToClient(hwnd, &mut point);
            let mut rect = zeroed();
            GetClientRect(hwnd, &mut rect);
            if state.recording.get() {
                return HTCLIENT as isize;
            }
            let b = px(BORDER);
            let c = px(14.0);
            let left = point.x < b;
            let right = point.x >= rect.right - b;
            let top = point.y < b;
            let bottom = point.y >= rect.bottom - b;
            let hit = if (left && point.y < c) || (top && point.x < c) {
                HTTOPLEFT
            } else if (right && point.y < c) || (top && point.x >= rect.right - c) {
                HTTOPRIGHT
            } else if (left && point.y >= rect.bottom - c) || (bottom && point.x < c) {
                HTBOTTOMLEFT
            } else if (right && point.y >= rect.bottom - c) || (bottom && point.x >= rect.right - c)
            {
                HTBOTTOMRIGHT
            } else if left {
                HTLEFT
            } else if right {
                HTRIGHT
            } else if top {
                HTTOP
            } else if bottom {
                HTBOTTOM
            } else if point.y < px(32.0) && point.x < rect.right - px(84.0) {
                HTCAPTION
            } else {
                HTCLIENT
            };
            hit as isize
        }
        WM_WINDOWPOSCHANGING => {
            let pos = &mut *(lparam as *mut WINDOWPOS);
            if pos.flags & SWP_NOSIZE == 0 && IsIconic(hwnd) == 0 {
                let b = px(BORDER);
                let h = px(HEADER);
                pos.cx = ((pos.cx - 2 * b).max(2) & !1) + 2 * b;
                pos.cy = ((pos.cy - h - b).max(2) & !1) + h + b;
            }
            DefSubclassProc(hwnd, message, wparam, lparam)
        }
        WM_GETMINMAXINFO => {
            DefSubclassProc(hwnd, message, wparam, lparam);
            (*(lparam as *mut MINMAXINFO)).ptMinTrackSize = POINT {
                x: px(640.0),
                y: px(220.0),
            };
            0
        }
        WM_NCLBUTTONDBLCLK => 0,
        WM_SYSCOMMAND
            if (wparam as u32 & 0xfff0) == SC_MAXIMIZE
                || (state.recording.get()
                    && matches!(wparam as u32 & 0xfff0, SC_MOVE | SC_SIZE)) =>
        {
            0
        }
        _ => DefSubclassProc(hwnd, message, wparam, lparam),
    }
}

pub fn videos_folder() -> Result<PathBuf, String> {
    unsafe {
        let mut value = null_mut();
        if SHGetKnownFolderPath(&FOLDERID_Videos, 0, null_mut(), &mut value) < 0 || value.is_null()
        {
            return Err("Windows could not locate your Videos folder.".into());
        }
        let mut length = 0;
        while *value.add(length) != 0 {
            length += 1;
        }
        let path = PathBuf::from(OsString::from_wide(std::slice::from_raw_parts(
            value, length,
        )));
        CoTaskMemFree(value.cast());
        Ok(path)
    }
}

pub fn default_name() -> String {
    unsafe {
        let mut time = zeroed();
        GetLocalTime(&mut time);
        format!(
            "Recording {:04}-{:02}-{:02} {:02}-{:02}-{:02}",
            time.wYear, time.wMonth, time.wDay, time.wHour, time.wMinute, time.wSecond
        )
    }
}

/// Ends the child process when this handle closes, including when Frame Record itself
/// crashes or is killed, so FFmpeg never keeps recording without a visible frame.
pub struct KillOnClose(HANDLE);

impl KillOnClose {
    pub fn new(child: &std::process::Child) -> Option<Self> {
        use std::os::windows::io::AsRawHandle;
        unsafe {
            let job = CreateJobObjectW(null(), null());
            if job.is_null() {
                return None;
            }
            let job = Self(job);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            (SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) != 0
                && AssignProcessToJobObject(job.0, child.as_raw_handle()) != 0)
                .then_some(job)
        }
    }
}

impl Drop for KillOnClose {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

pub fn open_folder(path: &std::path::Path) {
    use std::os::windows::ffi::OsStrExt;
    let verb: Vec<u16> = "open\0".encode_utf16().collect();
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        ShellExecuteW(
            null_mut(),
            verb.as_ptr(),
            path.as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::KillOnClose;
    use std::{
        process::{Command, Stdio},
        thread,
        time::{Duration, Instant},
    };

    #[test]
    fn closing_the_job_ends_the_child() {
        let mut child = Command::new("ping")
            .args(["-n", "30", "127.0.0.1"])
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        drop(KillOnClose::new(&child).expect("job object"));
        let deadline = Instant::now() + Duration::from_secs(5);
        while child.try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline, "child outlived its job");
            thread::sleep(Duration::from_millis(20));
        }
    }
}
