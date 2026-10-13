#[cfg(windows)]
use std::sync::atomic::{AtomicIsize, Ordering};
#[cfg(windows)]
use std::sync::Mutex;

#[cfg(windows)]
use windows::{
    core::w,
    Win32::{
        Foundation::{BOOL, HWND, LPARAM, POINT, RECT, WPARAM},
        System::{
            LibraryLoader::GetModuleHandleW,
            Registry::{
                RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
                HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ,
            },
            Threading::GetCurrentProcessId,
        },
        UI::WindowsAndMessaging::{
            CreateIconFromResourceEx, EnumWindows, GetCursorPos, GetWindowRect,
            GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, IsZoomed, LoadIconW,
            PostMessageW, SetForegroundWindow, SetWindowPos, ShowWindow, ICON_BIG, ICON_SMALL,
            LR_DEFAULTCOLOR, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SW_MAXIMIZE, SW_MINIMIZE,
            SW_RESTORE, SW_SHOW, WM_SETICON,
        },
    },
};

const APP_ICON_ICO: &[u8] = include_bytes!("../ui/app_icon.ico");

#[cfg(windows)]
static CACHED_MAIN_HWND: AtomicIsize = AtomicIsize::new(0);

#[cfg(windows)]
#[derive(Clone, Copy, Default)]
struct DragOrigin {
    hwnd: isize,
    cursor_x: i32,
    cursor_y: i32,
    win_x: i32,
    win_y: i32,
    active: bool,
}

#[cfg(windows)]
static DRAG_ORIGIN: Mutex<DragOrigin> = Mutex::new(DragOrigin {
    hwnd: 0,
    cursor_x: 0,
    cursor_y: 0,
    win_x: 0,
    win_y: 0,
    active: false,
});

/// Finds and caches the main top-level GUI `HWND` belonging to the current process.
#[cfg(windows)]
pub fn find_main_hwnd() -> Option<HWND> {
    let cached = CACHED_MAIN_HWND.load(Ordering::Relaxed);
    if cached != 0 {
        let hwnd = HWND(cached as *mut core::ffi::c_void);
        if unsafe { IsWindow(hwnd).as_bool() } {
            return Some(hwnd);
        }
    }

    struct SearchData {
        target_pid: u32,
        found_hwnd: HWND,
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let data = &mut *(lparam.0 as *mut SearchData);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == data.target_pid && IsWindowVisible(hwnd).as_bool() {
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_ok() {
                let width = rect.right - rect.left;
                let height = rect.bottom - rect.top;
                // Only match the actual main GUI window (> 350x300), never tray/helper popups
                if width > 350 && height > 300 {
                    data.found_hwnd = hwnd;
                    return BOOL(0);
                }
            }
        }
        BOOL(1)
    }

    unsafe {
        let mut data = SearchData {
            target_pid: GetCurrentProcessId(),
            found_hwnd: HWND::default(),
        };
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut data as *mut SearchData as isize));
        if !data.found_hwnd.is_invalid() && !data.found_hwnd.0.is_null() {
            CACHED_MAIN_HWND.store(data.found_hwnd.0 as isize, Ordering::Relaxed);
            Some(data.found_hwnd)
        } else {
            None
        }
    }
}

/// Asynchronously posts `WM_SETICON` (`ICON_BIG` and `ICON_SMALL`) to the main `HWND` without altering Winit window styles.
#[cfg(windows)]
pub fn apply_taskbar_icon_and_frame() {
    let Some(hwnd) = find_main_hwnd() else {
        return;
    };

    unsafe {
        let mut hicon_opt = None;
        if let Ok(hmod) = GetModuleHandleW(None) {
            if let Ok(ic) = LoadIconW(hmod, windows::core::PCWSTR(1 as *const u16)) {
                if !ic.is_invalid() {
                    hicon_opt = Some(ic);
                }
            }
        }

        if hicon_opt.is_none() && APP_ICON_ICO.len() > 62 {
            let dib_slice = &APP_ICON_ICO[22..];
            if let Ok(ic) =
                CreateIconFromResourceEx(dib_slice, true, 0x00030000, 64, 64, LR_DEFAULTCOLOR)
            {
                if !ic.is_invalid() {
                    hicon_opt = Some(ic);
                }
            }
        }

        if let Some(hicon) = hicon_opt {
            let _ = PostMessageW(
                hwnd,
                WM_SETICON,
                WPARAM(ICON_BIG as usize),
                LPARAM(hicon.0 as isize),
            );
            let _ = PostMessageW(
                hwnd,
                WM_SETICON,
                WPARAM(ICON_SMALL as usize),
                LPARAM(hicon.0 as isize),
            );
        }
    }
}

#[cfg(not(windows))]
pub fn apply_taskbar_icon_and_frame() {}

/// Records the starting cursor and window position when the user presses the left mouse button on the TitleBar.
/// Uses direct `GetCursorPos` + `SetWindowPos` instead of `ReleaseCapture` + `WM_NCLBUTTONDOWN(HTCAPTION)`
/// so Windows never swallows `WM_LBUTTONUP` and Slint's pointer grab state never gets stuck!
#[cfg(windows)]
pub fn begin_window_drag() {
    let Some(hwnd) = find_main_hwnd() else {
        return;
    };
    unsafe {
        if IsZoomed(hwnd).as_bool() {
            return;
        }
        let mut pt = POINT::default();
        let mut rect = RECT::default();
        if GetCursorPos(&mut pt).is_ok() && GetWindowRect(hwnd, &mut rect).is_ok() {
            if let Ok(mut drag) = DRAG_ORIGIN.lock() {
                drag.hwnd = hwnd.0 as isize;
                drag.cursor_x = pt.x;
                drag.cursor_y = pt.y;
                drag.win_x = rect.left;
                drag.win_y = rect.top;
                drag.active = true;
            }
        }
    }
}

#[cfg(not(windows))]
pub fn begin_window_drag() {}

/// Moves the window smoothly to follow the cursor while the left mouse button is held down on the TitleBar.
#[cfg(windows)]
pub fn update_window_drag() {
    let origin = match DRAG_ORIGIN.lock() {
        Ok(g) if g.active && g.hwnd != 0 => *g,
        _ => return,
    };
    unsafe {
        let mut pt = POINT::default();
        if GetCursorPos(&mut pt).is_ok() {
            let dx = pt.x - origin.cursor_x;
            let dy = pt.y - origin.cursor_y;
            let hwnd = HWND(origin.hwnd as *mut core::ffi::c_void);
            let _ = SetWindowPos(
                hwnd,
                HWND::default(),
                origin.win_x + dx,
                origin.win_y + dy,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }
}

#[cfg(not(windows))]
pub fn update_window_drag() {}

/// Ends the active window drag when the mouse button is released.
#[cfg(windows)]
pub fn end_window_drag() {
    if let Ok(mut drag) = DRAG_ORIGIN.lock() {
        drag.active = false;
    }
}

#[cfg(not(windows))]
pub fn end_window_drag() {}

/// Minimizes the window to the Windows Taskbar.
#[cfg(windows)]
pub fn minimize_window() {
    if let Some(hwnd) = find_main_hwnd() {
        unsafe {
            let _ = ShowWindow(hwnd, SW_MINIMIZE);
        }
    }
}

#[cfg(not(windows))]
pub fn minimize_window() {}

/// Toggles between Maximized and Normal window state.
#[cfg(windows)]
pub fn toggle_maximize_window() {
    if let Some(hwnd) = find_main_hwnd() {
        unsafe {
            if IsZoomed(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            } else {
                let _ = ShowWindow(hwnd, SW_MAXIMIZE);
            }
        }
    }
}

#[cfg(not(windows))]
pub fn toggle_maximize_window() {}

/// Restores the window if it is minimized to taskbar or hidden in tray.
#[cfg(windows)]
pub fn restore_window_from_taskbar() {
    if let Some(hwnd) = find_main_hwnd() {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

#[cfg(not(windows))]
pub fn restore_window_from_taskbar() {}

/// Checks if `ZenGamma` is registered in `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
#[cfg(windows)]
pub fn is_windows_autostart_enabled() -> bool {
    unsafe {
        let mut hkey = HKEY::default();
        let subkey = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey, 0, KEY_READ, &mut hkey).is_err() {
            return false;
        }
        let exists = RegQueryValueExW(hkey, w!("ZenGamma"), None, None, None, None).is_ok();
        let _ = RegCloseKey(hkey);
        exists
    }
}

#[cfg(not(windows))]
pub fn is_windows_autostart_enabled() -> bool {
    false
}

/// Enables or disables launching ZenGamma automatically on Windows login via `HKCU\...\Run`.
#[cfg(windows)]
pub fn set_windows_autostart(enabled: bool) -> Result<(), String> {
    unsafe {
        let mut hkey = HKEY::default();
        let subkey = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
        RegOpenKeyExW(HKEY_CURRENT_USER, subkey, 0, KEY_WRITE, &mut hkey)
            .ok()
            .map_err(|e| e.to_string())?;

        if enabled {
            let exe_path = std::env::current_exe().map_err(|e| e.to_string())?;
            let clean_path = exe_path
                .to_string_lossy()
                .trim_start_matches(r"\\?\")
                .to_string();
            let cmd = format!("\"{}\" --minimized\0", clean_path);
            let wide: Vec<u16> = cmd.encode_utf16().collect();
            let bytes = std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
            let res = RegSetValueExW(hkey, w!("ZenGamma"), 0, REG_SZ, Some(bytes));
            let _ = RegCloseKey(hkey);
            res.ok().map_err(|e| e.to_string())
        } else {
            let _ = RegDeleteValueW(hkey, w!("ZenGamma"));
            let _ = RegCloseKey(hkey);
            Ok(())
        }
    }
}

#[cfg(not(windows))]
pub fn set_windows_autostart(_enabled: bool) -> Result<(), String> {
    Ok(())
}
