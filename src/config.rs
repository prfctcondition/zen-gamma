use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;

/// Display parameters for a single preset.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DisplayValues {
    /// Black-level lift / offset: -50.0 to +50.0 (0.0 = neutral)
    pub brightness: f32,
    /// Dynamic range contrast: 0.0 to 100.0 (50.0 = neutral 1.0x)
    pub contrast: f32,
    /// Master gamma exponent: 0.50 to 3.00 (1.00 = linear)
    pub gamma: f32,
    /// Whether per-channel R/G/B gamma curves are active
    pub rgb_split: bool,
    /// Individual channel gamma values (0.50 to 3.00)
    pub gamma_r: f32,
    pub gamma_g: f32,
    pub gamma_b: f32,
    /// Color Saturation / Digital Vibrance: 0.0 to 200.0 (100.0 = neutral 1.0x, 200.0 = max boost)
    pub saturation: f32,
}

impl Default for DisplayValues {
    fn default() -> Self {
        Self {
            brightness: 0.0,
            contrast: 50.0,
            gamma: 1.00,
            rgb_split: false,
            gamma_r: 1.00,
            gamma_g: 1.00,
            gamma_b: 1.00,
            saturation: 100.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub hotkey: String,
    pub values: DisplayValues,
    #[serde(default)]
    pub is_builtin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameProfile {
    /// Unique stable identifier for the game profile (e.g. `"builtin_hunt"`, `"user_game_1"`)
    pub id: String,
    pub game_name: String,
    pub icon_file: String,
    #[serde(default)]
    pub is_builtin_game: bool,
    pub presets: Vec<Preset>,
}

/// Global application settings configured in the App Settings modal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    /// When true, clicking the top-right `(X)` button hides ZenGamma to the system tray instead of exiting.
    pub close_to_tray: bool,
    /// Launch ZenGamma automatically on Windows login (`HKCU\...\Run`).
    pub autostart: bool,
    /// Start minimized to the system tray when launched.
    pub start_minimized: bool,
    /// Global hotkey for emergency "Reset to Default" display restoration.
    #[serde(default = "default_reset_hotkey")]
    pub reset_hotkey: String,
}

fn default_reset_hotkey() -> String {
    "Ctrl+Shift+R".to_string()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            close_to_tray: true,
            autostart: false,
            start_minimized: false,
            reset_hotkey: default_reset_hotkey(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetOverride {
    pub hotkey: String,
    pub values: DisplayValues,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltinGameMetaOverride {
    pub game_name: String,
    pub icon_file: String,
}

/// Dedicated `user_presets.json` schema storing user presets, user-added games, and app settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserPresetsFile {
    /// Custom presets created by the user inside built-in games, keyed by `game.id`
    #[serde(default)]
    pub custom_presets: HashMap<String, Vec<Preset>>,
    /// Hotkey/value overrides for built-in presets, keyed by `"game_id::preset_name"`
    #[serde(default)]
    pub builtin_overrides: HashMap<String, PresetOverride>,
    /// Name/icon overrides for built-in games, keyed by `game.id`
    #[serde(default)]
    pub builtin_game_meta: HashMap<String, BuiltinGameMetaOverride>,
    /// User-customized ordering of presets per game, keyed by `game.id`
    #[serde(default)]
    pub preset_order: HashMap<String, Vec<String>>,
    /// User-customized ordering of game tabs across the entire sidebar, list of `game.id`
    #[serde(default)]
    pub game_order: Vec<String>,
    /// IDs of built-in game profiles that the user explicitly deleted
    #[serde(default)]
    pub deleted_builtin_games: Vec<String>,
    /// Entire user-added game profiles
    #[serde(default)]
    pub user_games: Vec<GameProfile>,
    #[serde(default)]
    pub settings: AppSettings,
    #[serde(default)]
    pub last_game_idx: usize,
    #[serde(default)]
    pub last_preset_idx: usize,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub games: Vec<GameProfile>,
    pub deleted_builtin_games: Vec<String>,
    pub settings: AppSettings,
    pub active_game_idx: usize,
    pub active_preset_idx: usize,
}

impl AppConfig {
    /// Returns the built-in factory game profiles (Escape From Tarkov, Hunt: Showdown, Arma 3, DayZ).
    /// Notice each game can use `Ctrl+F1..Ctrl+F7` independently because hotkeys are scoped to the active tab!
    pub fn builtin_games() -> Vec<GameProfile> {
        vec![
            GameProfile {
                id: "builtin_tarkov".to_string(),
                game_name: "Escape From Tarkov".to_string(),
                icon_file: "escape_from_tarkov.ico".to_string(),
                is_builtin_game: true,
                presets: vec![
                    Preset {
                        name: "[ULTRA EXTREME] Zero-Light Abyss (No Sources)".to_string(),
                        hotkey: "Ctrl+F1".to_string(),
                        is_builtin: true,
                        values: DisplayValues {
                            brightness: 50.0,
                            contrast: 95.0,
                            gamma: 3.00,
                            rgb_split: true,
                            gamma_r: 2.95,
                            gamma_g: 3.00,
                            gamma_b: 2.85,
                            saturation: 170.0,
                        },
                    },
                    Preset {
                        name: "[Night No-NVG] Pitch-Black Raid (00:00 - 04:00)".to_string(),
                        hotkey: "Ctrl+F2".to_string(),
                        is_builtin: true,
                        values: DisplayValues {
                            brightness: 38.0,
                            contrast: 82.0,
                            gamma: 2.85,
                            rgb_split: true,
                            gamma_r: 2.75,
                            gamma_g: 2.95,
                            gamma_b: 2.70,
                            saturation: 150.0,
                        },
                    },
                    Preset {
                        name: "[Night No-NVG] Moonlight & Clear Sky (22:00 - 05:00)".to_string(),
                        hotkey: "Ctrl+F3".to_string(),
                        is_builtin: true,
                        values: DisplayValues {
                            brightness: 28.0,
                            contrast: 72.0,
                            gamma: 2.55,
                            rgb_split: true,
                            gamma_r: 2.45,
                            gamma_g: 2.65,
                            gamma_b: 2.48,
                            saturation: 140.0,
                        },
                    },
                    Preset {
                        name: "[Indoor / Bunker] Interchange, Labs & D-2".to_string(),
                        hotkey: "Ctrl+F4".to_string(),
                        is_builtin: true,
                        values: DisplayValues {
                            brightness: 25.0,
                            contrast: 70.0,
                            gamma: 2.35,
                            rgb_split: false,
                            gamma_r: 2.35,
                            gamma_g: 2.35,
                            gamma_b: 2.35,
                            saturation: 155.0,
                        },
                    },
                    Preset {
                        name: "[Dusk / Dawn] Sunrise & Sunset Long Shadows".to_string(),
                        hotkey: "Ctrl+F5".to_string(),
                        is_builtin: true,
                        values: DisplayValues {
                            brightness: 16.0,
                            contrast: 64.0,
                            gamma: 2.00,
                            rgb_split: true,
                            gamma_r: 1.92,
                            gamma_g: 2.05,
                            gamma_b: 2.12,
                            saturation: 148.0,
                        },
                    },
                    Preset {
                        name: "[Daylight] Clear Sunny Raid & Foliage Spotter".to_string(),
                        hotkey: "Ctrl+F6".to_string(),
                        is_builtin: true,
                        values: DisplayValues {
                            brightness: 8.0,
                            contrast: 60.0,
                            gamma: 1.48,
                            rgb_split: false,
                            gamma_r: 1.48,
                            gamma_g: 1.48,
                            gamma_b: 1.48,
                            saturation: 160.0,
                        },
                    },
                    Preset {
                        name: "[Weather] Heavy Fog & Rain Piercer".to_string(),
                        hotkey: "Ctrl+F7".to_string(),
                        is_builtin: true,
                        values: DisplayValues {
                            brightness: -2.0,
                            contrast: 78.0,
                            gamma: 1.40,
                            rgb_split: true,
                            gamma_r: 1.48,
                            gamma_g: 1.40,
                            gamma_b: 1.24,
                            saturation: 172.0,
                        },
                    },
                ],
            },
            GameProfile {
                id: "builtin_hunt".to_string(),
                game_name: "Hunt: Showdown 1896".to_string(),
                icon_file: "hunt_showdown_594650.ico".to_string(),
                is_builtin_game: true,
                presets: vec![
                    Preset {
                        name: "[ULTRA EXTREME] Zero-Light Abyss (Fullbright)".to_string(),
                        hotkey: "Ctrl+F1".to_string(),
                        is_builtin: true,
                        values: DisplayValues {
                            brightness: 50.0,
                            contrast: 94.0,
                            gamma: 3.00,
                            rgb_split: true,
                            gamma_r: 2.95,
                            gamma_g: 3.00,
                            gamma_b: 2.85,
                            saturation: 165.0,
                        },
                    },
                    Preset {
                        name: "[Tryhard] Max Night Vision".to_string(),
                        hotkey: "Ctrl+F2".to_string(),
                        is_builtin: true,
                        values: DisplayValues {
                            brightness: 26.0,
                            contrast: 68.0,
                            gamma: 2.45,
                            rgb_split: false,
                            gamma_r: 2.45,
                            gamma_g: 2.45,
                            gamma_b: 2.45,
                            saturation: 135.0,
                        },
                    },
                    Preset {
                        name: "[Tryhard] Fog & Ash Piercer".to_string(),
                        hotkey: "Ctrl+F3".to_string(),
                        is_builtin: true,
                        values: DisplayValues {
                            brightness: -5.0,
                            contrast: 74.0,
                            gamma: 1.35,
                            rgb_split: true,
                            gamma_r: 1.42,
                            gamma_g: 1.35,
                            gamma_b: 1.20,
                            saturation: 155.0,
                        },
                    },
                    Preset {
                        name: "[Tryhard] Boss Lair & Dark Corners".to_string(),
                        hotkey: "Ctrl+F4".to_string(),
                        is_builtin: true,
                        values: DisplayValues {
                            brightness: 16.0,
                            contrast: 62.0,
                            gamma: 1.95,
                            rgb_split: false,
                            gamma_r: 1.95,
                            gamma_g: 1.95,
                            gamma_b: 1.95,
                            saturation: 135.0,
                        },
                    },
                    Preset {
                        name: "[PvP] Daylight Compound Pro".to_string(),
                        hotkey: "Ctrl+F5".to_string(),
                        is_builtin: true,
                        values: DisplayValues {
                            brightness: 4.0,
                            contrast: 58.0,
                            gamma: 1.30,
                            rgb_split: false,
                            gamma_r: 1.30,
                            gamma_g: 1.30,
                            gamma_b: 1.30,
                            saturation: 145.0,
                        },
                    },
                    Preset {
                        name: "[Beautiful] Golden Bayou Sunset".to_string(),
                        hotkey: "Ctrl+F6".to_string(),
                        is_builtin: true,
                        values: DisplayValues {
                            brightness: 1.0,
                            contrast: 60.0,
                            gamma: 1.15,
                            rgb_split: true,
                            gamma_r: 1.26,
                            gamma_g: 1.16,
                            gamma_b: 1.02,
                            saturation: 165.0,
                        },
                    },
                    Preset {
                        name: "[Cinematic] Grimdark Louisiana Noir".to_string(),
                        hotkey: "Ctrl+F7".to_string(),
                        is_builtin: true,
                        values: DisplayValues {
                            brightness: -7.0,
                            contrast: 70.0,
                            gamma: 0.95,
                            rgb_split: true,
                            gamma_r: 0.92,
                            gamma_g: 0.95,
                            gamma_b: 1.04,
                            saturation: 68.0,
                        },
                    },
                ],
            },
            GameProfile {
                id: "builtin_arma".to_string(),
                game_name: "Arma 3 / Arma Reforger".to_string(),
                icon_file: "arma_3_107410.ico".to_string(),
                is_builtin_game: true,
                presets: vec![
                    Preset {
                        name: "[ULTRA EXTREME] Zero-Light Abyss (Fullbright)".to_string(),
                        hotkey: "Ctrl+F1".to_string(),
                        is_builtin: true,
                        values: DisplayValues {
                            brightness: 50.0,
                            contrast: 94.0,
                            gamma: 3.00,
                            rgb_split: true,
                            gamma_r: 2.95,
                            gamma_g: 3.00,
                            gamma_b: 2.85,
                            saturation: 160.0,
                        },
                    },
                    Preset {
                        name: "[Tryhard] No-NVG Fullbright Night".to_string(),
                        hotkey: "Ctrl+F2".to_string(),
                        is_builtin: true,
                        values: DisplayValues {
                            brightness: 30.0,
                            contrast: 70.0,
                            gamma: 2.65,
                            rgb_split: true,
                            gamma_r: 2.50,
                            gamma_g: 2.75,
                            gamma_b: 2.50,
                            saturation: 130.0,
                        },
                    },
                    Preset {
                        name: "[Tryhard] Bush & Ghillie Spotter".to_string(),
                        hotkey: "Ctrl+F3".to_string(),
                        is_builtin: true,
                        values: DisplayValues {
                            brightness: 6.0,
                            contrast: 64.0,
                            gamma: 1.42,
                            rgb_split: true,
                            gamma_r: 1.52,
                            gamma_g: 1.34,
                            gamma_b: 1.25,
                            saturation: 175.0,
                        },
                    },
                    Preset {
                        name: "[PvP] Long-Range Recon Clarity".to_string(),
                        hotkey: "Ctrl+F4".to_string(),
                        is_builtin: true,
                        values: DisplayValues {
                            brightness: 3.0,
                            contrast: 72.0,
                            gamma: 1.25,
                            rgb_split: false,
                            gamma_r: 1.25,
                            gamma_g: 1.25,
                            gamma_b: 1.25,
                            saturation: 138.0,
                        },
                    },
                    Preset {
                        name: "[Beautiful] Vibrant Altis & Everon".to_string(),
                        hotkey: "Ctrl+F5".to_string(),
                        is_builtin: true,
                        values: DisplayValues {
                            brightness: 0.0,
                            contrast: 58.0,
                            gamma: 1.10,
                            rgb_split: true,
                            gamma_r: 1.16,
                            gamma_g: 1.12,
                            gamma_b: 1.04,
                            saturation: 160.0,
                        },
                    },
                    Preset {
                        name: "[Cinematic] Cold War Documentary".to_string(),
                        hotkey: "Ctrl+F6".to_string(),
                        is_builtin: true,
                        values: DisplayValues {
                            brightness: -4.0,
                            contrast: 66.0,
                            gamma: 0.96,
                            rgb_split: false,
                            gamma_r: 0.96,
                            gamma_g: 0.96,
                            gamma_b: 0.96,
                            saturation: 75.0,
                        },
                    },
                ],
            },
            GameProfile {
                id: "builtin_dayz".to_string(),
                game_name: "DayZ".to_string(),
                icon_file: "dayz_221100.ico".to_string(),
                is_builtin_game: true,
                presets: vec![
                    Preset {
                        name: "[ULTRA EXTREME] Zero-Light 1PP Night Abyss".to_string(),
                        hotkey: "Ctrl+F1".to_string(),
                        is_builtin: true,
                        values: DisplayValues {
                            brightness: 50.0,
                            contrast: 95.0,
                            gamma: 3.00,
                            rgb_split: true,
                            gamma_r: 2.95,
                            gamma_g: 3.00,
                            gamma_b: 2.85,
                            saturation: 165.0,
                        },
                    },
                    Preset {
                        name: "[Tryhard] Pitch Black 1PP Night".to_string(),
                        hotkey: "Ctrl+F2".to_string(),
                        is_builtin: true,
                        values: DisplayValues {
                            brightness: 34.0,
                            contrast: 74.0,
                            gamma: 2.75,
                            rgb_split: false,
                            gamma_r: 2.75,
                            gamma_g: 2.75,
                            gamma_b: 2.75,
                            saturation: 130.0,
                        },
                    },
                    Preset {
                        name: "[Tryhard] Overcast Rain & Pine Woods".to_string(),
                        hotkey: "Ctrl+F3".to_string(),
                        is_builtin: true,
                        values: DisplayValues {
                            brightness: 12.0,
                            contrast: 62.0,
                            gamma: 1.72,
                            rgb_split: false,
                            gamma_r: 1.72,
                            gamma_g: 1.72,
                            gamma_b: 1.72,
                            saturation: 155.0,
                        },
                    },
                    Preset {
                        name: "[PvP] NWAF & Tisy High-Contrast".to_string(),
                        hotkey: "Ctrl+F4".to_string(),
                        is_builtin: true,
                        values: DisplayValues {
                            brightness: 5.0,
                            contrast: 64.0,
                            gamma: 1.35,
                            rgb_split: false,
                            gamma_r: 1.35,
                            gamma_g: 1.35,
                            gamma_b: 1.35,
                            saturation: 148.0,
                        },
                    },
                    Preset {
                        name: "[Beautiful] Autumn Chernarus".to_string(),
                        hotkey: "Ctrl+F5".to_string(),
                        is_builtin: true,
                        values: DisplayValues {
                            brightness: 0.0,
                            contrast: 60.0,
                            gamma: 1.12,
                            rgb_split: true,
                            gamma_r: 1.24,
                            gamma_g: 1.14,
                            gamma_b: 1.00,
                            saturation: 168.0,
                        },
                    },
                    Preset {
                        name: "[Cinematic] Post-Apocalyptic Bleak".to_string(),
                        hotkey: "Ctrl+F6".to_string(),
                        is_builtin: true,
                        values: DisplayValues {
                            brightness: -6.0,
                            contrast: 72.0,
                            gamma: 0.94,
                            rgb_split: true,
                            gamma_r: 0.90,
                            gamma_g: 0.95,
                            gamma_b: 1.08,
                            saturation: 62.0,
                        },
                    },
                ],
            },
        ]
    }

    /// Default preset template given to newly added user games (single Base Vibrant preset).
    pub fn default_presets_for_new_game() -> Vec<Preset> {
        vec![Preset {
            name: "Base Vibrant".to_string(),
            hotkey: "Ctrl+F1".to_string(),
            is_builtin: true,
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
        }]
    }

    /// Path to `user_presets.json` storing the user's custom presets, added games, and settings.
    pub fn user_presets_path() -> PathBuf {
        if let Ok(exe_path) = env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let candidate = exe_dir.join("user_presets.json");
                if candidate.exists() {
                    return candidate;
                }
                if fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .open(&candidate)
                    .is_ok()
                {
                    let _ = fs::remove_file(&candidate);
                    return candidate;
                }
            }
        }
        if let Some(mut dir) = dirs::config_dir() {
            dir.push("ZenGamma");
            let _ = fs::create_dir_all(&dir);
            dir.push("user_presets.json");
            dir
        } else {
            PathBuf::from("user_presets.json")
        }
    }

    /// Loads built-in games and merges user games, presets, ordering, and settings from `user_presets.json`.
    pub fn load() -> Self {
        let mut games = Self::builtin_games();
        let path = Self::user_presets_path();

        let mut deleted_builtin_games = Vec::new();
        let mut settings = AppSettings::default();
        let mut active_game_idx = 0;
        let mut active_preset_idx = 0;

        if let Ok(raw) = fs::read_to_string(&path) {
            if let Ok(user_file) = serde_json::from_str::<UserPresetsFile>(&raw) {
                settings = user_file.settings;
                deleted_builtin_games = user_file.deleted_builtin_games;

                // Remove any built-in games that the user explicitly deleted
                games.retain(|g| !deleted_builtin_games.contains(&g.id));

                for game in &mut games {
                    if let Some(meta) = user_file.builtin_game_meta.get(&game.id) {
                        if game.id == "builtin_arma" && meta.game_name == "Arma 3" {
                            game.game_name = "Arma 3 / Arma Reforger".to_string();
                        } else {
                            game.game_name = meta.game_name.clone();
                        }
                        game.icon_file = meta.icon_file.clone();
                    }

                    for builtin in &mut game.presets {
                        let key = format!("{}::{}", game.id, builtin.name);
                        if let Some(ovr) = user_file.builtin_overrides.get(&key) {
                            builtin.hotkey = ovr.hotkey.clone();
                            builtin.values = ovr.values;
                        }
                    }

                    if let Some(custom_list) = user_file.custom_presets.get(&game.id) {
                        for mut user_preset in custom_list.clone() {
                            user_preset.is_builtin = false;
                            game.presets.push(user_preset);
                        }
                    }

                    // Restore custom preset order if the user reordered presets in this game
                    if let Some(order) = user_file.preset_order.get(&game.id) {
                        game.presets.sort_by_key(|p| {
                            order
                                .iter()
                                .position(|name| name == &p.name)
                                .unwrap_or(usize::MAX)
                        });
                    }
                }

                for mut ug in user_file.user_games {
                    ug.is_builtin_game = false;
                    if ug.presets.is_empty() {
                        ug.presets = Self::default_presets_for_new_game();
                    }
                    games.push(ug);
                }

                // Restore custom game tab order across the entire sidebar
                if !user_file.game_order.is_empty() {
                    games.sort_by_key(|g| {
                        user_file
                            .game_order
                            .iter()
                            .position(|id| id == &g.id)
                            .unwrap_or(usize::MAX)
                    });
                }

                if games.is_empty() {
                    games = Self::builtin_games();
                    deleted_builtin_games.clear();
                }

                active_game_idx = user_file.last_game_idx.min(games.len().saturating_sub(1));
                active_preset_idx = user_file
                    .last_preset_idx
                    .min(games[active_game_idx].presets.len().saturating_sub(1));
            }
        }

        let cfg = Self {
            games,
            deleted_builtin_games,
            settings,
            active_game_idx,
            active_preset_idx,
        };
        let _ = cfg.save();
        cfg
    }

    /// Saves all user-created presets, preset orderings, game orderings, added games, deleted built-in games, and app settings to `user_presets.json`.
    pub fn save(&self) -> Result<(), String> {
        let mut user_file = UserPresetsFile {
            custom_presets: HashMap::new(),
            builtin_overrides: HashMap::new(),
            builtin_game_meta: HashMap::new(),
            preset_order: HashMap::new(),
            game_order: self.games.iter().map(|g| g.id.clone()).collect(),
            deleted_builtin_games: self.deleted_builtin_games.clone(),
            user_games: Vec::new(),
            settings: self.settings.clone(),
            last_game_idx: self.active_game_idx,
            last_preset_idx: self.active_preset_idx,
        };

        let factory_defaults = Self::builtin_games();

        for game in &self.games {
            if game.is_builtin_game {
                let factory_game = factory_defaults.iter().find(|fg| fg.id == game.id);
                if let Some(fg) = factory_game {
                    if fg.game_name != game.game_name || fg.icon_file != game.icon_file {
                        user_file.builtin_game_meta.insert(
                            game.id.clone(),
                            BuiltinGameMetaOverride {
                                game_name: game.game_name.clone(),
                                icon_file: game.icon_file.clone(),
                            },
                        );
                    }
                }

                user_file.preset_order.insert(
                    game.id.clone(),
                    game.presets.iter().map(|p| p.name.clone()).collect(),
                );

                let mut custom_for_game = Vec::new();
                for preset in &game.presets {
                    if preset.is_builtin {
                        let factory_match = factory_game
                            .and_then(|fg| fg.presets.iter().find(|fp| fp.name == preset.name));
                        let is_modified = match factory_match {
                            Some(fp) => fp.hotkey != preset.hotkey || fp.values != preset.values,
                            None => true,
                        };
                        if is_modified {
                            let key = format!("{}::{}", game.id, preset.name);
                            user_file.builtin_overrides.insert(
                                key,
                                PresetOverride {
                                    hotkey: preset.hotkey.clone(),
                                    values: preset.values,
                                },
                            );
                        }
                    } else {
                        custom_for_game.push(preset.clone());
                    }
                }
                if !custom_for_game.is_empty() {
                    user_file
                        .custom_presets
                        .insert(game.id.clone(), custom_for_game);
                }
            } else {
                user_file.user_games.push(game.clone());
            }
        }

        let path = Self::user_presets_path();
        let json = serde_json::to_string_pretty(&user_file).map_err(|e| e.to_string())?;
        fs::write(path, json).map_err(|e| e.to_string())
    }
}
