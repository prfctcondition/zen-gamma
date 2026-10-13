#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[allow(dead_code)]
#[path = "../win32_window.rs"]
mod win32_window;

use slint::{ComponentHandle, SharedString, Timer};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

slint::include_modules!();

include!(concat!(env!("OUT_DIR"), "/bundled_assets.rs"));
include!(concat!(env!("OUT_DIR"), "/bundled_exe.rs"));

/// Creates a Windows `.lnk` shortcut using VBScript / PowerShell.
fn create_shortcut(lnk_path: &Path, target_exe: &Path, work_dir: &Path) {
    if let Some(parent) = lnk_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let target_clean = target_exe
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_string();
    let work_clean = work_dir
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_string();
    let lnk_clean = lnk_path
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_string();

    // 1. Direct WScript.Shell via temporary .vbs executed by wscript.exe
    let temp_vbs = std::env::temp_dir().join(format!("zengamma_shortcut_{}.vbs", std::process::id()));
    let vbs_code = format!(
        "Set ws = CreateObject(\"WScript.Shell\")\r\n\
         Set s = ws.CreateShortcut(\"{}\")\r\n\
         s.TargetPath = \"{}\"\r\n\
         s.WorkingDirectory = \"{}\"\r\n\
         s.IconLocation = \"{},0\"\r\n\
         s.Description = \"ZenGamma — Display & Color Profile Manager\"\r\n\
         s.Save\r\n",
        lnk_clean.replace('"', "\"\""),
        target_clean.replace('"', "\"\""),
        work_clean.replace('"', "\"\""),
        target_clean.replace('"', "\"\""),
    );

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        if fs::write(&temp_vbs, vbs_code).is_ok() {
            let _ = Command::new("wscript.exe")
                .creation_flags(CREATE_NO_WINDOW)
                .arg(&temp_vbs)
                .status();
            let _ = fs::remove_file(&temp_vbs);
        }

        // 2. Fallback to PowerShell if shortcut was not written
        if !lnk_path.exists() {
            let ps_script = format!(
                "$ws = New-Object -ComObject WScript.Shell; \
                 $s = $ws.CreateShortcut('{}'); \
                 $s.TargetPath = '{}'; \
                 $s.WorkingDirectory = '{}'; \
                 $s.IconLocation = '{},0'; \
                 $s.Description = 'ZenGamma — Display & Color Profile Manager'; \
                 $s.Save()",
                lnk_clean.replace('\'', "''"),
                target_clean.replace('\'', "''"),
                work_clean.replace('\'', "''"),
                target_clean.replace('\'', "''"),
            );
            let _ = Command::new("powershell")
                .creation_flags(CREATE_NO_WINDOW)
                .args(["-NoProfile", "-NonInteractive", "-Command", &ps_script])
                .status();
        }
    }

    #[cfg(not(windows))]
    {
        let _ = temp_vbs;
        let _ = vbs_code;
    }
}

/// Registers ZenGamma in `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\ZenGamma`
/// and optionally in `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
#[cfg(windows)]
fn register_installation(install_dir: &Path, target_exe: &Path, autostart: bool) {
    use windows::{
        core::w,
        Win32::System::Registry::{
            RegCloseKey, RegCreateKeyW, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY,
            HKEY_CURRENT_USER, KEY_WRITE, REG_SZ,
        },
    };

    unsafe fn set_reg_sz(hkey: HKEY, name: windows::core::PCWSTR, val: &str) {
        let mut wide: Vec<u16> = val.encode_utf16().collect();
        wide.push(0);
        let bytes = std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
        let _ = RegSetValueExW(hkey, name, 0, REG_SZ, Some(bytes));
    }

    unsafe {
        let mut hkey = HKEY::default();
        let uninstall_subkey =
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\ZenGamma");
        if RegCreateKeyW(HKEY_CURRENT_USER, uninstall_subkey, &mut hkey).is_ok() {
            let exe_str = target_exe
                .to_string_lossy()
                .trim_start_matches(r"\\?\")
                .to_string();
            let dir_str = install_dir
                .to_string_lossy()
                .trim_start_matches(r"\\?\")
                .to_string();
            let uninstall_bat = install_dir.join("Uninstall-ZenGamma.bat");
            let uninst_cmd = format!(
                "\"{}\"",
                uninstall_bat
                    .to_string_lossy()
                    .trim_start_matches(r"\\?\")
            );

            set_reg_sz(hkey, w!("DisplayName"), "ZenGamma");
            set_reg_sz(hkey, w!("DisplayVersion"), "0.2.0");
            set_reg_sz(hkey, w!("Publisher"), "ZenGamma Contributors");
            set_reg_sz(hkey, w!("DisplayIcon"), &exe_str);
            set_reg_sz(hkey, w!("InstallLocation"), &dir_str);
            set_reg_sz(hkey, w!("UninstallString"), &uninst_cmd);
            let _ = RegCloseKey(hkey);
        }

        let mut run_key = HKEY::default();
        let run_subkey = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
        if RegOpenKeyExW(HKEY_CURRENT_USER, run_subkey, 0, KEY_WRITE, &mut run_key).is_ok() {
            if autostart {
                let clean_exe = target_exe
                    .to_string_lossy()
                    .trim_start_matches(r"\\?\")
                    .to_string();
                let cmd = format!("\"{}\" --minimized", clean_exe);
                set_reg_sz(run_key, w!("ZenGamma"), &cmd);
            } else {
                let _ = RegDeleteValueW(run_key, w!("ZenGamma"));
            }
            let _ = RegCloseKey(run_key);
        }
    }
}

#[cfg(not(windows))]
fn register_installation(_install_dir: &Path, _target_exe: &Path, _autostart: bool) {}

fn default_install_directory() -> PathBuf {
    if let Some(local_data) = dirs::data_local_dir() {
        local_data.join("Programs").join("ZenGamma")
    } else {
        PathBuf::from("C:\\ZenGamma")
    }
}

fn perform_installation(
    install_dir: &Path,
    desktop_shortcut: bool,
    start_menu_shortcut: bool,
    autostart: bool,
    launch_now: bool,
) -> Result<(), String> {
    let assets_dir = install_dir.join("assets");
    let icons_dir = assets_dir.join("game_icons");
    fs::create_dir_all(&icons_dir).map_err(|e| format!("Failed to create directory: {}", e))?;

    let target_exe = install_dir.join("zen-gamma.exe");

    // 1. Write zen-gamma.exe (from embedded bytes or adjacent build output)
    if !BUNDLED_MAIN_EXE.is_empty() {
        fs::write(&target_exe, BUNDLED_MAIN_EXE)
            .map_err(|e| format!("Failed to write zen-gamma.exe: {}", e))?;
    } else if let Ok(cur_exe) = env::current_exe() {
        if let Some(parent) = cur_exe.parent() {
            let sibling = parent.join("zen-gamma.exe");
            if sibling.exists() {
                fs::copy(&sibling, &target_exe)
                    .map_err(|e| format!("Failed to copy zen-gamma.exe: {}", e))?;
            }
        }
    }

    // 2. Write games_catalog.json and all 139 .ico files into assets/game_icons/
    fs::write(assets_dir.join("games_catalog.json"), BUNDLED_CATALOG_JSON)
        .map_err(|e| format!("Failed to write games_catalog.json: {}", e))?;

    for &(fname, bytes) in BUNDLED_GAME_ICONS {
        let _ = fs::write(icons_dir.join(fname), bytes);
    }

    // 3. Create Uninstaller script
    let uninstall_bat = install_dir.join("Uninstall-ZenGamma.bat");
    let bat_content = format!(
        "@echo off\r\n\
         taskkill /F /IM zen-gamma.exe >nul 2>&1\r\n\
         reg delete \"HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\" /v ZenGamma /f >nul 2>&1\r\n\
         reg delete \"HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\ZenGamma\" /f >nul 2>&1\r\n\
         del /F /Q \"%USERPROFILE%\\Desktop\\ZenGamma.lnk\" >nul 2>&1\r\n\
         del /F /Q \"%USERPROFILE%\\Desktop\\Zen Gamma.lnk\" >nul 2>&1\r\n\
         del /F /Q \"%APPDATA%\\Microsoft\\Windows\\Start Menu\\Programs\\ZenGamma.lnk\" >nul 2>&1\r\n\
         del /F /Q \"%APPDATA%\\Microsoft\\Windows\\Start Menu\\Programs\\Zen Gamma.lnk\" >nul 2>&1\r\n\
         cd /d \"%TEMP%\"\r\n\
         rmdir /S /Q \"{}\"\r\n",
        install_dir.to_string_lossy().trim_start_matches(r"\\?\")
    );
    let _ = fs::write(&uninstall_bat, bat_content);

    // 4. Create Desktop Shortcuts across detected desktop roots (Standard, OneDrive, Localized)
    if desktop_shortcut {
        let mut desktop_targets = Vec::new();
        if let Some(desktop) = dirs::desktop_dir() {
            desktop_targets.push(desktop);
        }
        if let Ok(userprofile) = env::var("USERPROFILE") {
            let direct = PathBuf::from(&userprofile).join("Desktop");
            if !desktop_targets.contains(&direct) {
                desktop_targets.push(direct);
            }
            let onedrive = PathBuf::from(&userprofile).join("OneDrive").join("Desktop");
            if onedrive.exists() && !desktop_targets.contains(&onedrive) {
                desktop_targets.push(onedrive);
            }
        }

        for dt in desktop_targets {
            create_shortcut(&dt.join("Zen Gamma.lnk"), &target_exe, install_dir);
            create_shortcut(&dt.join("ZenGamma.lnk"), &target_exe, install_dir);
        }
    }

    // 5. Create Start Menu Shortcuts
    if start_menu_shortcut {
        let mut sm_targets = Vec::new();
        if let Some(appdata) = dirs::data_dir() {
            sm_targets.push(
                appdata
                    .join("Microsoft")
                    .join("Windows")
                    .join("Start Menu")
                    .join("Programs"),
            );
        }
        if let Ok(appdata_var) = env::var("APPDATA") {
            let sm = PathBuf::from(appdata_var)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs");
            if !sm_targets.contains(&sm) {
                sm_targets.push(sm);
            }
        }

        for sm in sm_targets {
            create_shortcut(&sm.join("Zen Gamma.lnk"), &target_exe, install_dir);
            create_shortcut(&sm.join("ZenGamma.lnk"), &target_exe, install_dir);
        }
    }

    // 6. Register in Windows Uninstall & Startup registry keys
    register_installation(install_dir, &target_exe, autostart);

    // 7. Launch ZenGamma if requested
    if launch_now && target_exe.exists() {
        let _ = Command::new(&target_exe).current_dir(install_dir).spawn();
    }

    Ok(())
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = InstallerWindow::new()?;
    let def_dir = default_install_directory();
    ui.set_install_dir(SharedString::from(def_dir.to_string_lossy().as_ref()));

    ui.on_drag_installer(|| {
        win32_window::begin_window_drag();
    });

    ui.on_drag_installer_move(|| {
        win32_window::update_window_drag();
    });

    ui.on_drag_installer_end(|| {
        win32_window::end_window_drag();
    });

    ui.on_close_installer(|| {
        let _ = slint::quit_event_loop();
    });

    {
        let ui_weak = ui.as_weak();
        ui.on_run_install(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let target_dir = PathBuf::from(ui.get_install_dir().to_string());
            let desktop = ui.get_opt_desktop_shortcut();
            let start_menu = ui.get_opt_start_menu();
            let autostart = ui.get_opt_autostart();
            let launch_now = ui.get_opt_launch_now();

            match perform_installation(&target_dir, desktop, start_menu, autostart, launch_now) {
                Ok(()) => {
                    ui.set_install_finished(true);
                    ui.set_status_text(SharedString::from(
                        "ZenGamma v0.2 successfully installed!",
                    ));
                }
                Err(err) => {
                    ui.set_status_text(SharedString::from(err));
                }
            }
        });
    }

    ui.show()?;
    Timer::single_shot(Duration::from_millis(60), move || {
        win32_window::apply_taskbar_icon_and_frame();
    });

    slint::run_event_loop_until_quit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_shortcut() {
        let temp_dir = std::env::temp_dir();
        let lnk = temp_dir.join("test_zen_gamma.lnk");
        let target = temp_dir.join("notepad.exe");
        let _ = fs::write(&target, b"dummy");
        create_shortcut(&lnk, &target, &temp_dir);
        let exists = lnk.exists();
        let _ = fs::remove_file(&lnk);
        let _ = fs::remove_file(&target);
        assert!(exists, "Shortcut .lnk should be created");
    }
}
