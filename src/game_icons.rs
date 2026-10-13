use serde::{Deserialize, Serialize};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;

// Include build-time bundled game catalog and 139 .ico files
include!(concat!(env!("OUT_DIR"), "/bundled_assets.rs"));

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogGameEntry {
    pub title: String,
    pub icon: String,
    #[serde(default)]
    pub appid: u32,
}

pub struct GameIconManager {
    pub catalog: Vec<CatalogGameEntry>,
    icons_dir: PathBuf,
    decoded_cache: HashMap<String, SharedPixelBuffer<Rgba8Pixel>>,
    fallback_buf: SharedPixelBuffer<Rgba8Pixel>,
}

impl GameIconManager {
    pub fn new() -> Self {
        let base_dir = Self::resolve_assets_dir();
        let icons_dir = base_dir.join("game_icons");
        let catalog_path = base_dir.join("games_catalog.json");

        // Ensure physical directory & bundled files exist on disk so user can inspect/add icons
        let _ = fs::create_dir_all(&icons_dir);
        if !catalog_path.exists() {
            let _ = fs::write(&catalog_path, BUNDLED_CATALOG_JSON);
        }
        for &(fname, bytes) in BUNDLED_GAME_ICONS {
            let target = icons_dir.join(fname);
            if !target.exists() {
                let _ = fs::write(&target, bytes);
            }
        }

        // Load catalog from disk first (allowing user edits), fallback to bundled JSON
        let catalog: Vec<CatalogGameEntry> = fs::read_to_string(&catalog_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| serde_json::from_str(BUNDLED_CATALOG_JSON).unwrap_or_default());

        let mut fallback = vec![0u8; 32 * 32 * 4];
        for y in 4..28 {
            for x in 4..28 {
                let idx = (y * 32 + x) * 4;
                fallback[idx] = 0x1B;
                fallback[idx + 1] = 0x2C;
                fallback[idx + 2] = 0x54;
                fallback[idx + 3] = 255;
            }
        }
        let fallback_buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&fallback, 32, 32);

        let mut mgr = Self {
            catalog,
            icons_dir,
            decoded_cache: HashMap::with_capacity(BUNDLED_GAME_ICONS.len() + 16),
            fallback_buf,
        };
        mgr.preload_all_icons();
        mgr
    }

    fn resolve_assets_dir() -> PathBuf {
        let local_assets = PathBuf::from("assets");
        if local_assets.join("games_catalog.json").exists() {
            return local_assets;
        }
        if let Ok(exe) = env::current_exe() {
            if let Some(parent) = exe.parent() {
                let exe_assets = parent.join("assets");
                if exe_assets.exists()
                    || fs::create_dir_all(exe_assets.join("game_icons")).is_ok()
                {
                    return exe_assets;
                }
            }
        }
        if let Some(mut cfg) = dirs::config_dir() {
            cfg.push("ZenGamma");
            cfg.push("assets");
            let _ = fs::create_dir_all(&cfg);
            return cfg;
        }
        local_assets
    }

    fn preload_all_icons(&mut self) {
        // 1. Decode from bundled icons table
        for &(fname, bytes) in BUNDLED_GAME_ICONS {
            if let Some(buf) = Self::decode_ico_bytes(bytes) {
                self.decoded_cache.insert(fname.to_string(), buf);
            }
        }
        // 2. Also scan icons_dir for any custom .ico/.png files dropped by the user
        if let Ok(entries) = fs::read_dir(&self.icons_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(fname) = path.file_name().and_then(|s| s.to_str()) {
                    if !self.decoded_cache.contains_key(fname) {
                        if let Ok(bytes) = fs::read(&path) {
                            if let Some(buf) = Self::decode_ico_bytes(&bytes) {
                                self.decoded_cache.insert(fname.to_string(), buf);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Decodes `.ico`/`.png` bytes and caps dimensions at 48x48 so GPU texture memory stays tiny (<2MB total).
    fn decode_ico_bytes(bytes: &[u8]) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
        let img = image::load_from_memory(bytes).ok()?;
        let (w, h) = (img.width(), img.height());
        let rgba = if w > 48 || h > 48 {
            img.resize(48, 48, image::imageops::FilterType::Triangle)
                .to_rgba8()
        } else {
            img.to_rgba8()
        };
        let (rw, rh) = rgba.dimensions();
        Some(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
            rgba.as_raw(),
            rw,
            rh,
        ))
    }

    /// Returns a zero-copy `slint::Image` backed by the shared `SharedPixelBuffer` so FemtoVG reuses GPU textures.
    pub fn get_slint_image(&self, icon_file: &str) -> Image {
        if let Some(buf) = self.decoded_cache.get(icon_file) {
            return Image::from_rgba8(buf.clone());
        }
        Image::from_rgba8(self.fallback_buf.clone())
    }

    /// Searches the local `games_catalog.json` offline by substring (case-insensitive).
    pub fn search_catalog(&self, query: &str, max_results: usize) -> Vec<CatalogGameEntry> {
        let q = query.trim().to_lowercase();
        let mut matches: Vec<CatalogGameEntry> = self
            .catalog
            .iter()
            .filter(|entry| q.is_empty() || entry.title.to_lowercase().contains(&q))
            .cloned()
            .collect();

        // Prioritize prefix matches when a query is entered
        if !q.is_empty() {
            matches.sort_by_key(|e| !e.title.to_lowercase().starts_with(&q));
        }
        matches.truncate(max_results);
        matches
    }

    /// Returns a deduplicated list of `(title, icon_filename)` for the visual Icon Picker Grid.
    pub fn all_available_icons(&self) -> Vec<(String, String)> {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();

        for entry in &self.catalog {
            if self.decoded_cache.contains_key(&entry.icon) && seen.insert(entry.icon.clone()) {
                result.push((entry.title.clone(), entry.icon.clone()));
            }
        }

        // Also append any extra user-dropped .ico files in assets/game_icons/
        let mut extra_keys: Vec<String> = self
            .decoded_cache
            .keys()
            .filter(|k| !seen.contains(*k))
            .cloned()
            .collect();
        extra_keys.sort();
        for k in extra_keys {
            let label = k.trim_end_matches(".ico").replace('_', " ");
            result.push((label, k));
        }

        result
    }
}
