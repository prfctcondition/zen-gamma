// Hide console window in release builds on Windows for a pure desktop utility experience
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod display;
mod game_icons;
mod hotkey_tray;
mod win32_window;

use config::{AppConfig, DisplayValues, GameProfile, Preset};
use display::DisplayController;
use game_icons::GameIconManager;
use hotkey_tray::{
    format_captured_key_event, init_background_services, BackgroundEvent, BackgroundServices,
};
use slint::{ComponentHandle, ModelRc, SharedString, Timer, VecModel};
use std::env;
use std::sync::{Arc, Mutex};
use std::time::Duration;

slint::include_modules!();

struct AppState {
    config: AppConfig,
    display: DisplayController,
    icons: GameIconManager,
    bg_services: Option<BackgroundServices>,
    applied_target: Option<(usize, usize)>,
}

/// Reorders an item in a `Vec<T>` from index `from` to index `to`, smoothly adjusting `cur_active`.
fn reorder_slice<T>(vec: &mut Vec<T>, from: usize, to: usize, cur_active: usize) -> usize {
    if from == to || from >= vec.len() || to >= vec.len() {
        return cur_active;
    }
    let item = vec.remove(from);
    vec.insert(to, item);
    if cur_active == from {
        to
    } else if from < to && cur_active > from && cur_active <= to {
        cur_active - 1
    } else if from > to && cur_active >= to && cur_active < from {
        cur_active + 1
    } else {
        cur_active
    }
}

/// Synchronizes the Slint UI properties with the currently selected game and preset in `AppState`.
fn sync_ui_from_state(ui: &AppWindow, state: &AppState) {
    let g_idx = state
        .config
        .active_game_idx
        .min(state.config.games.len().saturating_sub(1));

    // Populate sidebar games list model with real decoded .ico images
    let game_items: Vec<GameItem> = state
        .config
        .games
        .iter()
        .enumerate()
        .map(|(idx, g)| GameItem {
            id: idx as i32,
            name: SharedString::from(g.game_name.as_str()),
            icon: state.icons.get_slint_image(&g.icon_file),
            icon_file: SharedString::from(g.icon_file.as_str()),
            is_builtin: g.is_builtin_game,
        })
        .collect();
    ui.set_games(ModelRc::new(VecModel::from(game_items)));

    let game = &state.config.games[g_idx];
    let p_idx = state
        .config
        .active_preset_idx
        .min(game.presets.len().saturating_sub(1));

    ui.set_selected_game_index(g_idx as i32);
    ui.set_selected_preset_index(p_idx as i32);
    ui.set_reset_hotkey(SharedString::from(&state.config.settings.reset_hotkey));

    // Populate sidebar preset list model for the active game tab
    let preset_items: Vec<PresetItem> = game
        .presets
        .iter()
        .enumerate()
        .map(|(idx, p)| PresetItem {
            id: idx as i32,
            name: SharedString::from(p.name.as_str()),
            hotkey: SharedString::from(p.hotkey.as_str()),
            is_active: state.applied_target == Some((g_idx, idx)),
            is_builtin: p.is_builtin,
        })
        .collect();
    ui.set_presets(ModelRc::new(VecModel::from(preset_items)));

    if let Some(preset) = game.presets.get(p_idx) {
        ui.set_current_preset_name(SharedString::from(preset.name.as_str()));
        ui.set_current_preset_is_builtin(preset.is_builtin);
        ui.set_current_hotkey(SharedString::from(if preset.hotkey.is_empty() {
            "None"
        } else {
            preset.hotkey.as_str()
        }));

        if state.applied_target.is_some() {
            let v = &preset.values;
            ui.set_brightness(v.brightness);
            ui.set_contrast(v.contrast);
            ui.set_gamma(v.gamma);
            ui.set_rgb_split_enabled(v.rgb_split);
            ui.set_gamma_red(v.gamma_r);
            ui.set_gamma_green(v.gamma_g);
            ui.set_gamma_blue(v.gamma_b);
            ui.set_saturation(v.saturation);
        } else {
            let def = DisplayValues::default();
            ui.set_brightness(def.brightness);
            ui.set_contrast(def.contrast);
            ui.set_gamma(def.gamma);
            ui.set_rgb_split_enabled(def.rgb_split);
            ui.set_gamma_red(def.gamma_r);
            ui.set_gamma_green(def.gamma_g);
            ui.set_gamma_blue(def.gamma_b);
            ui.set_saturation(def.saturation);
        }
    }

    // Update active profile status banner
    if let Some((ag, ap)) = state.applied_target {
        if let Some(active_game) = state.config.games.get(ag) {
            if let Some(active_preset) = active_game.presets.get(ap) {
                ui.set_profile_engaged(true);
                ui.set_active_profile_label(SharedString::from(format!(
                    "{} / {}",
                    active_game.game_name, active_preset.name
                )));
                return;
            }
        }
    }

    ui.set_profile_engaged(false);
    ui.set_active_profile_label(SharedString::from("Default Monitor"));
}

/// Populates the offline catalog search results inside the Game Editor Modal.
fn populate_catalog_results(ui: &AppWindow, icons: &GameIconManager, query: &str) {
    let matches = icons.search_catalog(query, 150);
    let items: Vec<CatalogEntryItem> = matches
        .into_iter()
        .map(|entry| CatalogEntryItem {
            title: SharedString::from(entry.title.as_str()),
            icon_file: SharedString::from(entry.icon.as_str()),
            icon: icons.get_slint_image(&entry.icon),
        })
        .collect();
    ui.set_catalog_results(ModelRc::new(VecModel::from(items)));
}

/// Reads the live slider values directly from the Slint UI.
fn read_values_from_ui(ui: &AppWindow) -> DisplayValues {
    let rgb_split = ui.get_rgb_split_enabled();
    let master_gamma = ui.get_gamma();
    DisplayValues {
        brightness: ui.get_brightness(),
        contrast: ui.get_contrast(),
        gamma: master_gamma,
        rgb_split,
        gamma_r: if rgb_split {
            ui.get_gamma_red()
        } else {
            master_gamma
        },
        gamma_g: if rgb_split {
            ui.get_gamma_green()
        } else {
            master_gamma
        },
        gamma_b: if rgb_split {
            ui.get_gamma_blue()
        } else {
            master_gamma
        },
        saturation: ui.get_saturation(),
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let args: Vec<String> = env::args().collect();
    let launched_minimized = args.iter().any(|a| a == "--minimized");

    let mut config = AppConfig::load();
    config.settings.autostart = win32_window::is_windows_autostart_enabled();

    let display = DisplayController::new();
    let icons = GameIconManager::new();

    let ui = AppWindow::new()?;
    ui.set_backend_status_label(SharedString::from(display.backend_status()));
    ui.set_presets_file_path_label(SharedString::from(
        AppConfig::user_presets_path()
            .to_string_lossy()
            .as_ref(),
    ));

    // Populate the static all-icons gallery and initial catalog results once at startup
    let gallery_items: Vec<CatalogEntryItem> = icons
        .all_available_icons()
        .into_iter()
        .map(|(title, icon_file)| CatalogEntryItem {
            title: SharedString::from(title.as_str()),
            icon: icons.get_slint_image(&icon_file),
            icon_file: SharedString::from(icon_file.as_str()),
        })
        .collect();
    ui.set_all_icons_gallery(ModelRc::new(VecModel::from(gallery_items)));
    populate_catalog_results(&ui, &icons, "");

    let start_hidden = launched_minimized || config.settings.start_minimized;

    let state = Arc::new(Mutex::new(AppState {
        config,
        display,
        icons,
        bg_services: None,
        applied_target: None,
    }));

    // Initialize Global Hotkeys (Active Tab Scoped) & System Tray Icon worker threads
    let ui_weak_for_bg = ui.as_weak();
    let state_weak_for_bg = Arc::downgrade(&state);

    let (bg_services, _tray_icon) = {
        let initial_cfg = state.lock().unwrap().config.clone();
        init_background_services(&initial_cfg, move |event| {
            let ui_weak = ui_weak_for_bg.clone();
            let state_weak = state_weak_for_bg.clone();

            let _ = slint::invoke_from_event_loop(move || {
                let (Some(ui), Some(state_arc)) = (ui_weak.upgrade(), state_weak.upgrade()) else {
                    return;
                };
                let Ok(mut st) = state_arc.lock() else {
                    return;
                };

                match event {
                    BackgroundEvent::ActivatePreset {
                        game_idx,
                        preset_idx,
                    } => {
                        // Enforce active-tab isolation: only trigger if game_idx == active_game_idx
                        if game_idx != st.config.active_game_idx {
                            return;
                        }
                        if let Some(game) = st.config.games.get(game_idx) {
                            if let Some(preset) = game.presets.get(preset_idx) {
                                let values = preset.values;
                                if st.display.apply_values(&values).is_ok() {
                                    st.config.active_preset_idx = preset_idx;
                                    st.applied_target = Some((game_idx, preset_idx));
                                    sync_ui_from_state(&ui, &st);
                                }
                            }
                        }
                    }
                    BackgroundEvent::EmergencyReset => {
                        let _ = st.display.reset_to_default();
                        st.applied_target = None;
                        sync_ui_from_state(&ui, &st);
                    }
                    BackgroundEvent::RestoreWindow => {
                        let _ = ui.show();
                        win32_window::restore_window_from_taskbar();
                        win32_window::apply_taskbar_icon_and_frame();
                    }
                    BackgroundEvent::QuitApplication => {
                        let _ = st.display.reset_to_default();
                        let _ = slint::quit_event_loop();
                    }
                }
            });
        })
    };

    if let Ok(mut st) = state.lock() {
        st.bg_services = Some(bg_services);
        sync_ui_from_state(&ui, &st);
    }

    // --- Custom Frameless TitleBar Window Callbacks ---
    ui.on_drag_window(|| {
        win32_window::begin_window_drag();
    });

    ui.on_drag_window_move(|| {
        win32_window::update_window_drag();
    });

    ui.on_drag_window_end(|| {
        win32_window::end_window_drag();
    });

    ui.on_minimize_window(|| {
        win32_window::minimize_window();
    });

    ui.on_maximize_window(|| {
        win32_window::toggle_maximize_window();
    });

    {
        let ui_weak = ui.as_weak();
        let state_arc = Arc::clone(&state);
        ui.on_close_window(move || {
            let close_to_tray = state_arc
                .lock()
                .map(|st| st.config.settings.close_to_tray)
                .unwrap_or(true);
            if close_to_tray {
                if let Some(ui) = ui_weak.upgrade() {
                    let _ = ui.hide();
                }
            } else {
                if let Ok(st) = state_arc.lock() {
                    let _ = st.display.reset_to_default();
                }
                let _ = slint::quit_event_loop();
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        ui.on_minimize_to_tray(move || {
            if let Some(ui) = ui_weak.upgrade() {
                let _ = ui.hide();
            }
        });
    }

    // --- Callback: Select Game Category (Re-scopes Global Hotkeys to Selected Game!) ---
    {
        let ui_weak = ui.as_weak();
        let state_arc = Arc::clone(&state);
        ui.on_select_game(move |game_idx| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let Ok(mut st) = state_arc.lock() else { return };
            let g_idx = (game_idx as usize).min(st.config.games.len().saturating_sub(1));
            st.config.active_game_idx = g_idx;
            st.config.active_preset_idx = 0;
            let _ = st.config.save();

            // Immediately re-bind hotkeys exclusively to the newly selected game tab!
            if let Some(ref bg) = st.bg_services {
                bg.sync_hotkeys_from_config(&st.config);
            }
            sync_ui_from_state(&ui, &st);
        });
    }

    // --- Callback: Open Add Game Modal ---
    {
        let ui_weak = ui.as_weak();
        let state_arc = Arc::clone(&state);
        ui.on_open_add_game(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let Ok(st) = state_arc.lock() else { return };

            let default_icon = "cs2_730.ico";
            ui.set_game_modal_is_new(true);
            ui.set_game_modal_is_builtin(false);
            ui.set_game_modal_name(SharedString::from("Counter-Strike 2"));
            ui.set_game_modal_icon_file(SharedString::from(default_icon));
            ui.set_game_modal_icon_preview(st.icons.get_slint_image(default_icon));
            if !ui.get_game_modal_search().is_empty() {
                ui.set_game_modal_search(SharedString::from(""));
                populate_catalog_results(&ui, &st.icons, "");
            }
            ui.set_show_game_modal(true);
        });
    }

    // --- Callback: Open Edit Game Modal (⚙ button on any game row) ---
    {
        let ui_weak = ui.as_weak();
        let state_arc = Arc::clone(&state);
        ui.on_open_edit_game(move |game_idx| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let Ok(st) = state_arc.lock() else { return };
            let idx = (game_idx as usize).min(st.config.games.len().saturating_sub(1));
            let Some(game) = st.config.games.get(idx) else {
                return;
            };

            ui.set_game_modal_is_new(false);
            ui.set_game_modal_target_idx(idx as i32);
            ui.set_game_modal_is_builtin(game.is_builtin_game);
            ui.set_game_modal_name(SharedString::from(game.game_name.as_str()));
            ui.set_game_modal_icon_file(SharedString::from(game.icon_file.as_str()));
            ui.set_game_modal_icon_preview(st.icons.get_slint_image(&game.icon_file));
            if !ui.get_game_modal_search().is_empty() {
                ui.set_game_modal_search(SharedString::from(""));
                populate_catalog_results(&ui, &st.icons, "");
            }
            ui.set_show_game_modal(true);
        });
    }

    // --- Callback: Close Game Modal (Deferred to next event loop tick to avoid breaking active TouchArea) ---
    {
        let ui_weak = ui.as_weak();
        ui.on_close_game_modal(move || {
            let ui_weak_inner = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak_inner.upgrade() {
                    ui.set_show_game_modal(false);
                }
            });
        });
    }

    // --- Callback: Offline Catalog Search Query Changed ---
    {
        let ui_weak = ui.as_weak();
        let state_arc = Arc::clone(&state);
        ui.on_search_catalog_changed(move |query| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let Ok(st) = state_arc.lock() else { return };
            populate_catalog_results(&ui, &st.icons, query.as_str());
        });
    }

    // --- Callback: Save Game Profile from Modal ---
    {
        let ui_weak = ui.as_weak();
        let state_arc = Arc::clone(&state);
        ui.on_save_game_modal(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let name = ui.get_game_modal_name().trim().to_string();
            let icon_file = ui.get_game_modal_icon_file().trim().to_string();
            if name.is_empty() {
                return;
            }

            let Ok(mut st) = state_arc.lock() else { return };
            if ui.get_game_modal_is_new() {
                let factory_match = AppConfig::builtin_games().into_iter().find(|fg| {
                    fg.game_name.eq_ignore_ascii_case(&name)
                        || fg.icon_file.eq_ignore_ascii_case(&icon_file)
                });

                if let Some(mut fg) = factory_match {
                    let already_exists = st.config.games.iter().any(|g| g.id == fg.id);
                    if !already_exists {
                        st.config.deleted_builtin_games.retain(|id| id != &fg.id);
                        fg.game_name = name;
                        fg.icon_file = icon_file;
                        st.config.games.push(fg);
                    } else {
                        let unique_id = format!(
                            "user_game_{}_{}",
                            st.config.games.len() + 1,
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs())
                                .unwrap_or(0)
                        );
                        st.config.games.push(GameProfile {
                            id: unique_id,
                            game_name: name,
                            icon_file,
                            is_builtin_game: false,
                            presets: fg.presets,
                        });
                    }
                } else {
                    let unique_id = format!(
                        "user_game_{}_{}",
                        st.config.games.len() + 1,
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0)
                    );
                    st.config.games.push(GameProfile {
                        id: unique_id,
                        game_name: name,
                        icon_file,
                        is_builtin_game: false,
                        presets: AppConfig::default_presets_for_new_game(),
                    });
                }
                st.config.active_game_idx = st.config.games.len() - 1;
                st.config.active_preset_idx = 0;
            } else {
                let idx = (ui.get_game_modal_target_idx() as usize)
                    .min(st.config.games.len().saturating_sub(1));
                if let Some(game) = st.config.games.get_mut(idx) {
                    game.game_name = name;
                    game.icon_file = icon_file;
                }
            }

            let _ = st.config.save();
            if let Some(ref bg) = st.bg_services {
                bg.sync_hotkeys_from_config(&st.config);
            }
            sync_ui_from_state(&ui, &st);
            drop(st);

            let ui_weak_inner = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak_inner.upgrade() {
                    ui.set_show_game_modal(false);
                }
            });
        });
    }

    // --- Callback: Reorder / Drag and Drop Game Tab ---
    {
        let ui_weak = ui.as_weak();
        let state_arc = Arc::clone(&state);
        ui.on_reorder_game(move |from_idx, to_idx| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let Ok(mut st) = state_arc.lock() else { return };
            let from = from_idx as usize;
            let to = to_idx as usize;
            let games_len = st.config.games.len();
            if from < games_len && to < games_len && from != to {
                let cur_game = st.config.active_game_idx;
                let new_game = reorder_slice(&mut st.config.games, from, to, cur_game);
                st.config.active_game_idx = new_game;

                // Also update applied_target game index if engaged
                if let Some((app_g, app_p)) = st.applied_target {
                    let new_app_g = if app_g == from {
                        to
                    } else if from < to && app_g > from && app_g <= to {
                        app_g - 1
                    } else if from > to && app_g >= to && app_g < from {
                        app_g + 1
                    } else {
                        app_g
                    };
                    st.applied_target = Some((new_app_g, app_p));
                }

                let _ = st.config.save();
                if let Some(ref bg) = st.bg_services {
                    bg.sync_hotkeys_from_config(&st.config);
                }
                sync_ui_from_state(&ui, &st);
            }
        });
    }

    // --- Callback: Delete Game Profile Directly from Sidebar (✕ button on game row) ---
    {
        let ui_weak = ui.as_weak();
        let state_arc = Arc::clone(&state);
        ui.on_delete_game_profile(move |game_idx| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let Ok(mut st) = state_arc.lock() else { return };
            if st.config.games.len() <= 1 {
                return;
            }
            let idx = (game_idx as usize).min(st.config.games.len().saturating_sub(1));
            let removed = st.config.games.remove(idx);
            if removed.is_builtin_game && !st.config.deleted_builtin_games.contains(&removed.id) {
                st.config.deleted_builtin_games.push(removed.id);
            }

            if st.config.active_game_idx >= st.config.games.len() {
                st.config.active_game_idx = st.config.games.len() - 1;
            } else if idx < st.config.active_game_idx {
                st.config.active_game_idx = st.config.active_game_idx.saturating_sub(1);
            }
            st.config.active_preset_idx = 0;
            st.applied_target = None;

            let _ = st.config.save();
            if let Some(ref bg) = st.bg_services {
                bg.sync_hotkeys_from_config(&st.config);
            }
            sync_ui_from_state(&ui, &st);
        });
    }

    // --- Callback: Delete Game Profile from Modal ("Delete Profile" button) ---
    {
        let ui_weak = ui.as_weak();
        let state_arc = Arc::clone(&state);
        ui.on_delete_game_modal(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let Ok(mut st) = state_arc.lock() else { return };
            if st.config.games.len() <= 1 {
                return;
            }
            let idx = (ui.get_game_modal_target_idx() as usize)
                .min(st.config.games.len().saturating_sub(1));

            let removed = st.config.games.remove(idx);
            if removed.is_builtin_game && !st.config.deleted_builtin_games.contains(&removed.id) {
                st.config.deleted_builtin_games.push(removed.id);
            }

            if st.config.active_game_idx >= st.config.games.len() {
                st.config.active_game_idx = st.config.games.len() - 1;
            } else if idx < st.config.active_game_idx {
                st.config.active_game_idx = st.config.active_game_idx.saturating_sub(1);
            }
            st.config.active_preset_idx = 0;
            st.applied_target = None;

            let _ = st.config.save();
            if let Some(ref bg) = st.bg_services {
                bg.sync_hotkeys_from_config(&st.config);
            }
            sync_ui_from_state(&ui, &st);
            drop(st);

            let ui_weak_inner = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak_inner.upgrade() {
                    ui.set_show_game_modal(false);
                }
            });
        });
    }

    // --- Callback: Open App Settings Modal ---
    {
        let ui_weak = ui.as_weak();
        let state_arc = Arc::clone(&state);
        ui.on_open_app_settings(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let Ok(st) = state_arc.lock() else { return };
            ui.set_setting_close_to_tray(st.config.settings.close_to_tray);
            ui.set_setting_autostart(win32_window::is_windows_autostart_enabled());
            ui.set_setting_start_minimized(st.config.settings.start_minimized);
            ui.set_reset_hotkey(SharedString::from(&st.config.settings.reset_hotkey));
            ui.set_recording_reset_hotkey(false);
            ui.set_show_settings_modal(true);
        });
    }

    // --- Callback: Save App Settings Modal ---
    {
        let ui_weak = ui.as_weak();
        let state_arc = Arc::clone(&state);
        ui.on_save_app_settings(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let close_to_tray = ui.get_setting_close_to_tray();
            let autostart = ui.get_setting_autostart();
            let start_minimized = ui.get_setting_start_minimized();

            let _ = win32_window::set_windows_autostart(autostart);

            if let Ok(mut st) = state_arc.lock() {
                st.config.settings.close_to_tray = close_to_tray;
                st.config.settings.autostart = autostart;
                st.config.settings.start_minimized = start_minimized;
                let _ = st.config.save();
                if let Some(ref bg) = st.bg_services {
                    bg.sync_hotkeys_from_config(&st.config);
                }
            }

            let ui_weak_inner = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak_inner.upgrade() {
                    ui.set_show_settings_modal(false);
                }
            });
        });
    }

    // --- Callback: Select Preset ---
    {
        let ui_weak = ui.as_weak();
        let state_arc = Arc::clone(&state);
        ui.on_select_preset(move |preset_idx| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let Ok(mut st) = state_arc.lock() else { return };
            let g_idx = st.config.active_game_idx;
            let p_idx =
                (preset_idx as usize).min(st.config.games[g_idx].presets.len().saturating_sub(1));
            st.config.active_preset_idx = p_idx;
            if let Some(preset) = st.config.games[g_idx].presets.get(p_idx) {
                let vals = preset.values;
                if st.display.apply_values(&vals).is_ok() {
                    st.applied_target = Some((g_idx, p_idx));
                }
            }
            sync_ui_from_state(&ui, &st);
        });
    }

    // --- Callback: Create New User Preset ---
    {
        let ui_weak = ui.as_weak();
        let state_arc = Arc::clone(&state);
        ui.on_create_preset(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let Ok(mut st) = state_arc.lock() else { return };
            let g_idx = st.config.active_game_idx;
            let user_count = st.config.games[g_idx]
                .presets
                .iter()
                .filter(|p| !p.is_builtin)
                .count()
                + 1;
            st.config.games[g_idx].presets.push(Preset {
                name: format!("My Preset {}", user_count),
                hotkey: String::new(),
                values: DisplayValues {
                    brightness: 5.0,
                    contrast: 60.0,
                    gamma: 1.35,
                    rgb_split: false,
                    gamma_r: 1.35,
                    gamma_g: 1.35,
                    gamma_b: 1.35,
                    saturation: 155.0,
                },
                is_builtin: false,
            });
            st.config.active_preset_idx = st.config.games[g_idx].presets.len() - 1;
            let _ = st.config.save();
            sync_ui_from_state(&ui, &st);
        });
    }

    // --- Callback: Move / Reorder Preset (Drag and Drop or Up/Down buttons) ---
    {
        let ui_weak = ui.as_weak();
        let state_arc = Arc::clone(&state);
        ui.on_move_preset(move |from_idx, to_idx| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let Ok(mut st) = state_arc.lock() else { return };
            let g_idx = st.config.active_game_idx;
            let from = from_idx as usize;
            let to = to_idx as usize;

            let presets_len = st.config.games[g_idx].presets.len();
            if from < presets_len && to < presets_len && from != to {
                let cur_active = st.config.active_preset_idx;
                let new_active =
                    reorder_slice(&mut st.config.games[g_idx].presets, from, to, cur_active);
                st.config.active_preset_idx = new_active;

                // Track applied_target if applicable
                if let Some((app_g, app_p)) = st.applied_target {
                    if app_g == g_idx {
                        let new_app_p = if app_p == from {
                            to
                        } else if from < to && app_p > from && app_p <= to {
                            app_p - 1
                        } else if from > to && app_p >= to && app_p < from {
                            app_p + 1
                        } else {
                            app_p
                        };
                        st.applied_target = Some((app_g, new_app_p));
                    }
                }

                let _ = st.config.save();
                if let Some(ref bg) = st.bg_services {
                    bg.sync_hotkeys_from_config(&st.config);
                }
                sync_ui_from_state(&ui, &st);
            }
        });
    }

    // --- Callback: Delete Current User Preset (Built-in presets are protected) ---
    {
        let ui_weak = ui.as_weak();
        let state_arc = Arc::clone(&state);
        ui.on_delete_preset(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let Ok(mut st) = state_arc.lock() else { return };
            let g_idx = st.config.active_game_idx;
            let p_idx = st.config.active_preset_idx;

            if let Some(preset) = st.config.games[g_idx].presets.get(p_idx) {
                if preset.is_builtin {
                    return;
                }
            }

            st.config.games[g_idx].presets.remove(p_idx);
            st.config.active_preset_idx = p_idx.saturating_sub(1);
            if st.applied_target == Some((g_idx, p_idx)) {
                st.applied_target = None;
            }
            let _ = st.config.save();
            if let Some(ref bg) = st.bg_services {
                bg.sync_hotkeys_from_config(&st.config);
            }
            sync_ui_from_state(&ui, &st);
        });
    }

    // --- Callback: Live Slider Parameters Changed ---
    {
        let ui_weak = ui.as_weak();
        let state_arc = Arc::clone(&state);
        ui.on_parameters_changed(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let values = read_values_from_ui(&ui);
            let Ok(mut st) = state_arc.lock() else { return };
            let g_idx = st.config.active_game_idx;
            let p_idx = st.config.active_preset_idx;
            if let Some(preset) = st.config.games[g_idx].presets.get_mut(p_idx) {
                preset.values = values;
            }

            if st.display.apply_values(&values).is_ok() {
                let target = Some((g_idx, p_idx));
                if st.applied_target != target {
                    st.applied_target = target;
                    if let Some(game) = st.config.games.get(g_idx) {
                        if let Some(preset) = game.presets.get(p_idx) {
                            ui.set_profile_engaged(true);
                            ui.set_active_profile_label(SharedString::from(format!(
                                "{} / {}",
                                game.game_name, preset.name
                            )));
                        }
                    }
                }
            }
        });
    }

    // --- Callback: Press-to-Bind Key Captured (Deduplicates ONLY within the Active Game!) ---
    {
        let ui_weak = ui.as_weak();
        let state_arc = Arc::clone(&state);
        ui.on_key_captured(move |ctrl, alt, shift, key_text| {
            let Some(ui) = ui_weak.upgrade() else { return };

            match format_captured_key_event(ctrl, alt, shift, key_text.as_str()) {
                None => {}
                Some(Err(())) => {
                    ui.set_recording_hotkey(false);
                }
                Some(Ok(new_hk)) => {
                    ui.set_recording_hotkey(false);
                    let Ok(mut st) = state_arc.lock() else { return };
                    let g_idx = st.config.active_game_idx;
                    let p_idx = st.config.active_preset_idx;

                    // Only clear duplicate hotkey inside the SAME game tab; other games can share the same hotkey!
                    if !new_hk.is_empty() {
                        if let Some(active_game) = st.config.games.get_mut(g_idx) {
                            for preset in &mut active_game.presets {
                                if preset.hotkey.eq_ignore_ascii_case(&new_hk) {
                                    preset.hotkey.clear();
                                }
                            }
                        }
                    }

                    if let Some(preset) = st.config.games[g_idx].presets.get_mut(p_idx) {
                        preset.hotkey = new_hk;
                    }
                    let _ = st.config.save();
                    if let Some(ref bg) = st.bg_services {
                        bg.sync_hotkeys_from_config(&st.config);
                    }
                    sync_ui_from_state(&ui, &st);
                }
            }
        });
    }

    // --- Callback: Start Recording Reset to Default Hotkey ---
    {
        let ui_weak = ui.as_weak();
        ui.on_start_record_reset_hotkey(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            ui.set_recording_hotkey(false);
            ui.set_recording_reset_hotkey(true);
        });
    }

    // --- Callback: Reset to Default Hotkey Captured ---
    {
        let ui_weak = ui.as_weak();
        let state_arc = Arc::clone(&state);
        ui.on_reset_hotkey_captured(move |ctrl, alt, shift, key_text| {
            let Some(ui) = ui_weak.upgrade() else { return };
            match format_captured_key_event(ctrl, alt, shift, key_text.as_str()) {
                None => {}
                Some(Err(())) => {
                    ui.set_recording_reset_hotkey(false);
                }
                Some(Ok(new_hk)) => {
                    ui.set_recording_reset_hotkey(false);
                    let Ok(mut st) = state_arc.lock() else { return };
                    st.config.settings.reset_hotkey = new_hk.clone();
                    ui.set_reset_hotkey(SharedString::from(&new_hk));
                    let _ = st.config.save();
                    if let Some(ref bg) = st.bg_services {
                        bg.sync_hotkeys_from_config(&st.config);
                    }
                    sync_ui_from_state(&ui, &st);
                }
            }
        });
    }

    // --- Callback: Save Preset to user_presets.json ---
    {
        let ui_weak = ui.as_weak();
        let state_arc = Arc::clone(&state);
        ui.on_save_preset(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let values = read_values_from_ui(&ui);
            let edited_name = ui.get_current_preset_name().to_string();

            let Ok(mut st) = state_arc.lock() else { return };
            let g_idx = st.config.active_game_idx;
            let p_idx = st.config.active_preset_idx;

            if let Some(preset) = st.config.games[g_idx].presets.get_mut(p_idx) {
                if !preset.is_builtin && !edited_name.trim().is_empty() {
                    preset.name = edited_name.trim().to_string();
                }
                preset.values = values;
            }
            let _ = st.config.save();
            if let Some(ref bg) = st.bg_services {
                bg.sync_hotkeys_from_config(&st.config);
            }
            sync_ui_from_state(&ui, &st);
        });
    }

    // --- Callback: Reset to Default ---
    {
        let ui_weak = ui.as_weak();
        let state_arc = Arc::clone(&state);
        ui.on_reset_to_default(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let Ok(mut st) = state_arc.lock() else { return };
            let _ = st.display.reset_to_default();
            st.applied_target = None;
            sync_ui_from_state(&ui, &st);
        });
    }

    // OS close event (e.g. Alt+F4 or Taskbar Close)
    {
        let state_arc = Arc::clone(&state);
        ui.window().on_close_requested(move || {
            if let Ok(st) = state_arc.lock() {
                let _ = st.display.reset_to_default();
            }
            let _ = slint::quit_event_loop();
            slint::CloseRequestResponse::HideWindow
        });
    }

    if !start_hidden {
        ui.show()?;
        // Apply native Win32 WM_SETICON & taskbar frame styles once the window HWND is visible
        Timer::single_shot(Duration::from_millis(60), move || {
            win32_window::apply_taskbar_icon_and_frame();
        });
    }

    slint::run_event_loop_until_quit()?;

    if let Ok(st) = state.lock() {
        let _ = st.display.reset_to_default();
    }
    Ok(())
}
