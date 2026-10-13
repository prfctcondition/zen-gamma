use crate::config::AppConfig;
use std::sync::mpsc::{self, Sender};
use std::thread;
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    Icon, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

#[cfg(windows)]
use windows::Win32::{
    Foundation::HWND,
    UI::{
        Input::KeyboardAndMouse::{
            RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL,
            MOD_NOREPEAT, MOD_SHIFT, VK_BACK, VK_DELETE, VK_DOWN, VK_END, VK_F1, VK_F10, VK_F11,
            VK_F12, VK_F2, VK_F3, VK_F4, VK_F5, VK_F6, VK_F7, VK_F8, VK_F9, VK_HOME, VK_INSERT,
            VK_LEFT, VK_NEXT, VK_PRIOR, VK_RIGHT, VK_SPACE, VK_TAB, VK_UP,
        },
        WindowsAndMessaging::{GetMessageW, PeekMessageW, MSG, PM_NOREMOVE, WM_HOTKEY},
    },
};

/// Hotkey ID reserved for the global emergency gamma reset (`Ctrl + Shift + R`).
pub const EMERGENCY_RESET_HOTKEY_ID: i32 = 9999;

/// Events emitted by the background hotkey and tray worker threads to the Slint UI.
#[derive(Debug, Clone)]
pub enum BackgroundEvent {
    /// A preset hotkey was pressed globally: `(game_index, preset_index)`
    ActivatePreset { game_idx: usize, preset_idx: usize },
    /// The emergency reset hotkey (`Ctrl+Shift+R`) or Tray Reset item was triggered
    EmergencyReset,
    /// User clicked "Show ZenGamma" or double-clicked the system tray icon
    RestoreWindow,
    /// User clicked "Quit & Restore Gamma" in the system tray menu
    QuitApplication,
}

/// Commands sent from the UI thread to the Hotkey worker thread when bindings change.
#[derive(Debug, Clone)]
pub enum HotkeyCommand {
    SyncBindings(Vec<(i32, String)>),
    Shutdown,
}

/// Thread-safe handle to communicate with the background Win32 hotkey worker thread.
pub struct BackgroundServices {
    hotkey_tx: std::sync::Mutex<Sender<HotkeyCommand>>,
    #[cfg(windows)]
    worker_thread_id: u32,
}

impl BackgroundServices {
    /// Synchronizes preset hotkeys from `AppConfig` with the Win32 `RegisterHotKey` worker.
    /// IMPORTANT: Registers hotkeys ONLY for the currently selected active game tab (`config.active_game_idx`),
    /// so identical hotkeys across different games (e.g. DayZ vs Arma) never conflict or cross-trigger!
    pub fn sync_hotkeys_from_config(&self, config: &AppConfig) {
        let mut bindings = Vec::new();
        if !config.settings.reset_hotkey.trim().is_empty() {
            bindings.push((EMERGENCY_RESET_HOTKEY_ID, config.settings.reset_hotkey.clone()));
        }
        let g_idx = config
            .active_game_idx
            .min(config.games.len().saturating_sub(1));
        if let Some(active_game) = config.games.get(g_idx) {
            for (p_idx, preset) in active_game.presets.iter().enumerate() {
                if !preset.hotkey.trim().is_empty() {
                    let hk_id = ((g_idx as i32) + 1) * 100 + (p_idx as i32);
                    bindings.push((hk_id, preset.hotkey.clone()));
                }
            }
        }
        if let Ok(tx) = self.hotkey_tx.lock() {
            let _ = tx.send(HotkeyCommand::SyncBindings(bindings));
        }
        #[cfg(windows)]
        unsafe {
            use windows::Win32::Foundation::{LPARAM, WPARAM};
            use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_APP};
            let _ = PostThreadMessageW(self.worker_thread_id, WM_APP + 1, WPARAM(0), LPARAM(0));
        }
    }
}

impl Drop for BackgroundServices {
    fn drop(&mut self) {
        if let Ok(tx) = self.hotkey_tx.lock() {
            let _ = tx.send(HotkeyCommand::Shutdown);
        }
        #[cfg(windows)]
        unsafe {
            use windows::Win32::Foundation::{LPARAM, WPARAM};
            use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
            let _ = PostThreadMessageW(self.worker_thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
}

/// Maps a Cyrillic character to its corresponding Latin QWERTY physical key.
fn map_cyrillic_to_latin(c: char) -> Option<char> {
    match c {
        'й' | 'Й' => Some('Q'),
        'ц' | 'Ц' => Some('W'),
        'у' | 'У' => Some('E'),
        'к' | 'К' => Some('R'),
        'е' | 'Е' => Some('T'),
        'н' | 'Н' => Some('Y'),
        'г' | 'Г' => Some('U'),
        'ш' | 'Ш' => Some('I'),
        'щ' | 'Щ' => Some('O'),
        'з' | 'З' => Some('P'),
        'ф' | 'Ф' => Some('A'),
        'ы' | 'Ы' => Some('S'),
        'в' | 'В' => Some('D'),
        'а' | 'А' => Some('F'),
        'п' | 'П' => Some('G'),
        'р' | 'Р' => Some('H'),
        'о' | 'О' => Some('J'),
        'л' | 'Л' => Some('K'),
        'д' | 'Д' => Some('L'),
        'я' | 'Я' => Some('Z'),
        'ч' | 'Ч' => Some('X'),
        'с' | 'С' => Some('C'),
        'м' | 'М' => Some('V'),
        'и' | 'И' => Some('B'),
        'т' | 'Т' => Some('N'),
        'ь' | 'Ь' => Some('M'),
        _ => None,
    }
}

/// Converts a Slint `KeyEvent` into a canonical hotkey string (e.g. `"Ctrl+F1"`, `"Alt+G"`, `"F4"`, `"Home"`).
/// Returns:
/// - `Some(Ok(hotkey_string))` when a valid main key was pressed (empty string `""` means cleared via Backspace/Delete)
/// - `Some(Err(()))` when `Escape` was pressed (cancel recording without modifying)
/// - `None` when only a standalone modifier key (Ctrl/Shift/Alt) was pressed so the UI keeps waiting for the main key.
pub fn format_captured_key_event(
    ctrl: bool,
    alt: bool,
    shift: bool,
    key_text: &str,
) -> Option<Result<String, ()>> {
    let mut chars = key_text.chars();
    let first_char = chars.next()?;

    // Check Slint special key private-use unicode range or standard control chars
    let key_name: String = match first_char {
        '\u{001b}' => return Some(Err(())), // Escape cancels recording
        '\u{0008}' | '\u{007f}' if !ctrl && !alt && !shift => {
            // Backspace or Delete alone clears the hotkey
            return Some(Ok(String::new()));
        }
        '\u{0008}' => "Backspace".to_string(),
        '\u{007f}' => "Delete".to_string(),
        '\u{0009}' => "Tab".to_string(),
        ' ' => "Space".to_string(),
        // Slint Function keys F1..F12 in Private Use Area (0xF704..=0xF70F)
        '\u{f704}' => "F1".to_string(),
        '\u{f705}' => "F2".to_string(),
        '\u{f706}' => "F3".to_string(),
        '\u{f707}' => "F4".to_string(),
        '\u{f708}' => "F5".to_string(),
        '\u{f709}' => "F6".to_string(),
        '\u{f70a}' => "F7".to_string(),
        '\u{f70b}' => "F8".to_string(),
        '\u{f70c}' => "F9".to_string(),
        '\u{f70d}' => "F10".to_string(),
        '\u{f70e}' => "F11".to_string(),
        '\u{f70f}' => "F12".to_string(),
        // Slint Navigation & Arrow keys
        '\u{f700}' => "Up".to_string(),
        '\u{f701}' => "Down".to_string(),
        '\u{f702}' => "Left".to_string(),
        '\u{f703}' => "Right".to_string(),
        '\u{f727}' => "Insert".to_string(),
        '\u{f728}' => {
            if !ctrl && !alt && !shift {
                return Some(Ok(String::new()));
            }
            "Delete".to_string()
        }
        '\u{f729}' => "Home".to_string(),
        '\u{f72b}' => "End".to_string(),
        '\u{f72c}' => "PageUp".to_string(),
        '\u{f72d}' => "PageDown".to_string(),
        c if c.is_ascii_alphanumeric() => c.to_ascii_uppercase().to_string(),
        c if map_cyrillic_to_latin(c).is_some() => {
            map_cyrillic_to_latin(c).unwrap().to_string()
        }
        c if matches!(c, '-' | '=' | '[' | ']' | '\\' | ';' | '\'' | ',' | '.' | '/' | '`') => {
            c.to_string()
        }
        // Ignore standalone modifier key presses (Shift, Ctrl, Alt, Meta)
        _ => return None,
    };

    let mut parts = Vec::new();
    if ctrl {
        parts.push("Ctrl".to_string());
    }
    if alt {
        parts.push("Alt".to_string());
    }
    if shift {
        parts.push("Shift".to_string());
    }
    parts.push(key_name);

    Some(Ok(parts.join("+")))
}

/// Parses a human-readable hotkey string (e.g. `"Ctrl+F1"`, `"Alt+X"`, `"Shift+1"`, `"F5"`, `"Home"`) into Win32 modifiers + VK code.
#[cfg(windows)]
fn parse_hotkey_string(hotkey: &str) -> Option<(HOT_KEY_MODIFIERS, u32)> {
    let trimmed = hotkey.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("None") {
        return None;
    }

    let mut modifiers = MOD_NOREPEAT;
    let mut vk_code: Option<u32> = None;

    for part in trimmed.split('+').map(|s| s.trim().to_uppercase()) {
        match part.as_str() {
            "CTRL" | "CONTROL" => modifiers |= MOD_CONTROL,
            "ALT" => modifiers |= MOD_ALT,
            "SHIFT" => modifiers |= MOD_SHIFT,
            "F1" => vk_code = Some(VK_F1.0 as u32),
            "F2" => vk_code = Some(VK_F2.0 as u32),
            "F3" => vk_code = Some(VK_F3.0 as u32),
            "F4" => vk_code = Some(VK_F4.0 as u32),
            "F5" => vk_code = Some(VK_F5.0 as u32),
            "F6" => vk_code = Some(VK_F6.0 as u32),
            "F7" => vk_code = Some(VK_F7.0 as u32),
            "F8" => vk_code = Some(VK_F8.0 as u32),
            "F9" => vk_code = Some(VK_F9.0 as u32),
            "F10" => vk_code = Some(VK_F10.0 as u32),
            "F11" => vk_code = Some(VK_F11.0 as u32),
            "F12" => vk_code = Some(VK_F12.0 as u32),
            "NUMPAD0" | "NUM0" => vk_code = Some(0x60),
            "NUMPAD1" | "NUM1" => vk_code = Some(0x61),
            "NUMPAD2" | "NUM2" => vk_code = Some(0x62),
            "NUMPAD3" | "NUM3" => vk_code = Some(0x63),
            "NUMPAD4" | "NUM4" => vk_code = Some(0x64),
            "NUMPAD5" | "NUM5" => vk_code = Some(0x65),
            "NUMPAD6" | "NUM6" => vk_code = Some(0x66),
            "NUMPAD7" | "NUM7" => vk_code = Some(0x67),
            "NUMPAD8" | "NUM8" => vk_code = Some(0x68),
            "NUMPAD9" | "NUM9" => vk_code = Some(0x69),
            "SPACE" => vk_code = Some(VK_SPACE.0 as u32),
            "TAB" => vk_code = Some(VK_TAB.0 as u32),
            "BACKSPACE" => vk_code = Some(VK_BACK.0 as u32),
            "DELETE" => vk_code = Some(VK_DELETE.0 as u32),
            "INSERT" => vk_code = Some(VK_INSERT.0 as u32),
            "HOME" => vk_code = Some(VK_HOME.0 as u32),
            "END" => vk_code = Some(VK_END.0 as u32),
            "PAGEUP" => vk_code = Some(VK_PRIOR.0 as u32),
            "PAGEDOWN" => vk_code = Some(VK_NEXT.0 as u32),
            "UP" => vk_code = Some(VK_UP.0 as u32),
            "DOWN" => vk_code = Some(VK_DOWN.0 as u32),
            "LEFT" => vk_code = Some(VK_LEFT.0 as u32),
            "RIGHT" => vk_code = Some(VK_RIGHT.0 as u32),
            "-" => vk_code = Some(0xBD),
            "=" => vk_code = Some(0xBB),
            "[" => vk_code = Some(0xDB),
            "]" => vk_code = Some(0xDD),
            "\\" => vk_code = Some(0xDC),
            ";" => vk_code = Some(0xBA),
            "'" => vk_code = Some(0xDE),
            "," => vk_code = Some(0xBC),
            "." => vk_code = Some(0xBE),
            "/" => vk_code = Some(0xBF),
            "`" => vk_code = Some(0xC0),
            s if s.len() == 1 => {
                let ch = s.chars().next().unwrap();
                if ch.is_ascii_alphanumeric() {
                    // Win32 VK codes for '0'..='9' (0x30..=0x39) and 'A'..='Z' (0x41..=0x5A) match ASCII uppercase
                    vk_code = Some(ch as u32);
                } else if let Some(mapped) = map_cyrillic_to_latin(ch) {
                    vk_code = Some(mapped as u32);
                }
            }
            _ => {}
        }
    }

    vk_code.map(|vk| (modifiers, vk))
}

fn dist_to_segment(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32) -> f32 {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let len_sq = dx * dx + dy * dy;
    let t = if len_sq > 0.0 {
        (((px - x0) * dx + (py - y0) * dy) / len_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let proj_x = x0 + t * dx;
    let proj_y = y0 + t * dy;
    ((px - proj_x).powi(2) + (py - proj_y).powi(2)).sqrt()
}

fn sd_rounded_box(px: f32, py: f32, cx: f32, cy: f32, hx: f32, hy: f32, r: f32) -> f32 {
    let qx = (px - cx).abs() - hx + r;
    let qy = (py - cy).abs() - hy + r;
    let out_dist = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    let in_dist = qx.max(qy).min(0.0);
    out_dist + in_dist - r
}

/// Generates a 32x32 RGBA icon matching the exact ZEN GAMMA logo next to the title:
/// Deep Navy (`#1B2C54`) rounded square with white zig-zag polyline `M 2 12 L 6 4 L 10 9 L 14 2`.
fn build_brand_tray_icon() -> Option<Icon> {
    let size = 32u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];

    let map_x = |svg_x: f32| -> f32 { 7.0 + (svg_x / 16.0) * 18.0 };
    let map_y = |svg_y: f32| -> f32 { 7.0 + (svg_y / 16.0) * 18.0 };

    let pts = [
        (map_x(2.0), map_y(12.0)),
        (map_x(6.0), map_y(4.0)),
        (map_x(10.0), map_y(9.0)),
        (map_x(14.0), map_y(2.0)),
    ];

    for y in 0..size {
        for x in 0..size {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;

            let box_sdf = sd_rounded_box(px, py, 16.0, 16.0, 14.5, 14.5, 6.5);
            let box_alpha = (0.5 - box_sdf).clamp(0.0, 1.0);
            if box_alpha <= 0.0 {
                continue;
            }

            let d1 = dist_to_segment(px, py, pts[0].0, pts[0].1, pts[1].0, pts[1].1);
            let d2 = dist_to_segment(px, py, pts[1].0, pts[1].1, pts[2].0, pts[2].1);
            let d3 = dist_to_segment(px, py, pts[2].0, pts[2].1, pts[3].0, pts[3].1);
            let min_d = d1.min(d2).min(d3);

            let stroke_t = (1.55 - min_d).clamp(0.0, 1.0);

            let r = (0x1B as f32 * (1.0 - stroke_t) + 255.0 * stroke_t).round() as u8;
            let g = (0x2C as f32 * (1.0 - stroke_t) + 255.0 * stroke_t).round() as u8;
            let b = (0x54 as f32 * (1.0 - stroke_t) + 255.0 * stroke_t).round() as u8;
            let a = (box_alpha * 255.0).round() as u8;

            let idx = ((y * size + x) * 4) as usize;
            rgba[idx] = r;
            rgba[idx + 1] = g;
            rgba[idx + 2] = b;
            rgba[idx + 3] = a;
        }
    }

    Icon::from_rgba(rgba, size, size).ok()
}

/// Starts the 0%-CPU-idle Win32 `RegisterHotKey` worker thread and initializes the system tray icon.
pub fn init_background_services<F>(
    initial_config: &AppConfig,
    on_event: F,
) -> (BackgroundServices, Option<TrayIcon>)
where
    F: Fn(BackgroundEvent) + Send + Sync + Clone + 'static,
{
    let (hotkey_tx, hotkey_rx) = mpsc::channel::<HotkeyCommand>();
    let (tid_tx, tid_rx) = mpsc::channel::<u32>();

    let hotkey_callback = on_event.clone();

    // 1. Spawn dedicated Win32 GetMessageW global hotkey worker thread (0% CPU when idle)
    thread::Builder::new()
        .name("zengamma-hotkey-worker".to_string())
        .spawn(move || {
            #[cfg(windows)]
            unsafe {
                use windows::Win32::System::Threading::GetCurrentThreadId;
                use windows::Win32::UI::WindowsAndMessaging::WM_APP;

                // Force creation of the thread message queue before reporting thread ID
                let mut dummy_msg = MSG::default();
                let _ = PeekMessageW(&mut dummy_msg, HWND::default(), 0, 0, PM_NOREMOVE);
                let _ = tid_tx.send(GetCurrentThreadId());

                let mut active_ids: Vec<i32> = Vec::new();
                let mut msg = MSG::default();

                // Blocking kernel wait — consumes 0% CPU until hotkey or thread message arrives
                while GetMessageW(&mut msg, HWND::default(), 0, 0).as_bool() {
                    if msg.message == WM_APP + 1 {
                        while let Ok(cmd) = hotkey_rx.try_recv() {
                            match cmd {
                                HotkeyCommand::SyncBindings(new_bindings) => {
                                    for old_id in active_ids.drain(..) {
                                        let _ = UnregisterHotKey(HWND::default(), old_id);
                                    }
                                    for (hk_id, hk_str) in new_bindings {
                                        if let Some((mods, vk)) = parse_hotkey_string(&hk_str) {
                                            if RegisterHotKey(HWND::default(), hk_id, mods, vk)
                                                .is_ok()
                                            {
                                                active_ids.push(hk_id);
                                            }
                                        }
                                    }
                                }
                                HotkeyCommand::Shutdown => {
                                    for old_id in active_ids.drain(..) {
                                        let _ = UnregisterHotKey(HWND::default(), old_id);
                                    }
                                    return;
                                }
                            }
                        }
                    } else if msg.message == WM_HOTKEY {
                        let hk_id = msg.wParam.0 as i32;
                        if hk_id == EMERGENCY_RESET_HOTKEY_ID {
                            hotkey_callback(BackgroundEvent::EmergencyReset);
                        } else if hk_id >= 100 {
                            let game_idx = ((hk_id / 100) - 1) as usize;
                            let preset_idx = (hk_id % 100) as usize;
                            hotkey_callback(BackgroundEvent::ActivatePreset {
                                game_idx,
                                preset_idx,
                            });
                        }
                    }
                }

                for old_id in active_ids.drain(..) {
                    let _ = UnregisterHotKey(HWND::default(), old_id);
                }
            }

            #[cfg(not(windows))]
            {
                let _ = tid_tx.send(0);
                while let Ok(cmd) = hotkey_rx.recv() {
                    if matches!(cmd, HotkeyCommand::Shutdown) {
                        break;
                    }
                }
            }
        })
        .expect("Failed to spawn global hotkey thread");

    let worker_thread_id = tid_rx.recv().unwrap_or(0);

    // 2. Build System Tray Icon & Context Menu
    let tray_menu = Menu::new();
    let show_item = MenuItem::new("Show ZenGamma", true, None);
    let reset_label = if initial_config.settings.reset_hotkey.trim().is_empty() {
        "Reset to Default".to_string()
    } else {
        format!("Reset to Default ({})", initial_config.settings.reset_hotkey.trim())
    };
    let reset_item = MenuItem::new(&reset_label, true, None);
    let quit_item = MenuItem::new("Quit & Restore Gamma", true, None);

    let show_id = show_item.id().clone();
    let reset_id = reset_item.id().clone();
    let quit_id = quit_item.id().clone();

    let _ = tray_menu.append_items(&[
        &show_item,
        &reset_item,
        &PredefinedMenuItem::separator(),
        &quit_item,
    ]);

    let tray_icon = build_brand_tray_icon().and_then(|icon| {
        TrayIconBuilder::new()
            .with_menu(Box::new(tray_menu))
            .with_tooltip("ZenGamma — Display & Color Profile Manager")
            .with_icon(icon)
            .build()
            .ok()
    });

    // 3. Spawn blocking listener threads for Tray Menu & Tray Icon click events (0% CPU idle)
    let menu_callback = on_event.clone();
    thread::Builder::new()
        .name("zengamma-tray-menu".to_string())
        .spawn(move || {
            let receiver = MenuEvent::receiver();
            while let Ok(event) = receiver.recv() {
                if event.id == show_id {
                    menu_callback(BackgroundEvent::RestoreWindow);
                } else if event.id == reset_id {
                    menu_callback(BackgroundEvent::EmergencyReset);
                } else if event.id == quit_id {
                    menu_callback(BackgroundEvent::QuitApplication);
                    break;
                }
            }
        })
        .ok();

    let icon_callback = on_event;
    thread::Builder::new()
        .name("zengamma-tray-click".to_string())
        .spawn(move || {
            let receiver = TrayIconEvent::receiver();
            while let Ok(event) = receiver.recv() {
                if let TrayIconEvent::DoubleClick { .. } = event {
                    icon_callback(BackgroundEvent::RestoreWindow);
                }
            }
        })
        .ok();

    let services = BackgroundServices {
        hotkey_tx: std::sync::Mutex::new(hotkey_tx),
        #[cfg(windows)]
        worker_thread_id,
    };

    services.sync_hotkeys_from_config(initial_config);
    (services, tray_icon)
}
