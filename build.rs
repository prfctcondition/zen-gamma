use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Distance from point `(px, py)` to line segment `(x0, y0) -> (x1, y1)`.
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

/// Signed distance to a rounded rectangle centered at `(cx, cy)` with half-size `(hx, hy)` and radius `r`.
fn sd_rounded_box(px: f32, py: f32, cx: f32, cy: f32, hx: f32, hy: f32, r: f32) -> f32 {
    let qx = (px - cx).abs() - hx + r;
    let qy = (py - cy).abs() - hy + r;
    let out_dist = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    let in_dist = qx.max(qy).min(0.0);
    out_dist + in_dist - r
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn write_png_chunk(out: &mut Vec<u8>, tag: &[u8; 4], payload: &[u8]) {
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(tag);
    out.extend_from_slice(payload);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// Writes a valid 64x64 RGBA `.png` file so Slint's `@image-url("app_icon.png")` sets the taskbar & window icon natively.
fn write_rgba_png(path: &Path, size: u32, rgba: &[u8]) {
    let mut raw_scanlines = Vec::with_capacity(((size * 4 + 1) * size) as usize);
    for y in 0..size {
        raw_scanlines.push(0u8); // Filter type 0 (None)
        let row_start = (y * size * 4) as usize;
        raw_scanlines.extend_from_slice(&rgba[row_start..row_start + (size * 4) as usize]);
    }

    // Wrap raw_scanlines in uncompressed zlib stream (RFC 1950 / 1951)
    let mut zlib = Vec::with_capacity(raw_scanlines.len() + 32);
    zlib.push(0x78);
    zlib.push(0x01);
    let chunks: Vec<&[u8]> = raw_scanlines.chunks(65535).collect();
    for (i, chunk) in chunks.iter().enumerate() {
        let is_last = i + 1 == chunks.len();
        zlib.push(if is_last { 0x01 } else { 0x00 });
        let len = chunk.len() as u16;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(chunk);
    }
    zlib.extend_from_slice(&adler32(&raw_scanlines).to_be_bytes());

    let mut png = Vec::new();
    png.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]); // PNG signature

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&size.to_be_bytes());
    ihdr.extend_from_slice(&size.to_be_bytes());
    ihdr.push(8); // bit depth
    ihdr.push(6); // color type RGBA
    ihdr.push(0);
    ihdr.push(0);
    ihdr.push(0);
    write_png_chunk(&mut png, b"IHDR", &ihdr);
    write_png_chunk(&mut png, b"IDAT", &zlib);
    write_png_chunk(&mut png, b"IEND", &[]);

    let _ = fs::write(path, png);
}

/// Generates both `ui/app_icon.ico` and `ui/app_icon.png` matching the exact ZEN GAMMA logo:
/// Deep Navy (#1B2C54) rounded square with white polyline `M 2 12 L 6 4 L 10 9 L 14 2`.
fn generate_zen_gamma_icons(ico_path: &Path, png_path: &Path) {
    let size: u32 = 64;
    let mut bgra_bottom_up = vec![0u8; (size * size * 4) as usize];
    let mut rgba_top_down = vec![0u8; (size * size * 4) as usize];

    let map_x = |svg_x: f32| -> f32 { 14.0 + (svg_x / 16.0) * 36.0 };
    let map_y = |svg_y: f32| -> f32 { 14.0 + (svg_y / 16.0) * 36.0 };

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

            let box_sdf = sd_rounded_box(px, py, 32.0, 32.0, 29.0, 29.0, 13.5);
            let box_alpha = (0.5 - box_sdf).clamp(0.0, 1.0);

            if box_alpha <= 0.0 {
                continue;
            }

            let d1 = dist_to_segment(px, py, pts[0].0, pts[0].1, pts[1].0, pts[1].1);
            let d2 = dist_to_segment(px, py, pts[1].0, pts[1].1, pts[2].0, pts[2].1);
            let d3 = dist_to_segment(px, py, pts[2].0, pts[2].1, pts[3].0, pts[3].1);
            let min_d = d1.min(d2).min(d3);

            let stroke_t = (2.7 - min_d).clamp(0.0, 1.0);

            let r = (0x1B as f32 * (1.0 - stroke_t) + 255.0 * stroke_t).round() as u8;
            let g = (0x2C as f32 * (1.0 - stroke_t) + 255.0 * stroke_t).round() as u8;
            let b = (0x54 as f32 * (1.0 - stroke_t) + 255.0 * stroke_t).round() as u8;
            let a = (box_alpha * 255.0).round() as u8;

            let td_idx = ((y * size + x) * 4) as usize;
            rgba_top_down[td_idx] = r;
            rgba_top_down[td_idx + 1] = g;
            rgba_top_down[td_idx + 2] = b;
            rgba_top_down[td_idx + 3] = a;

            let bu_y = size - 1 - y;
            let bu_idx = ((bu_y * size + x) * 4) as usize;
            bgra_bottom_up[bu_idx] = b;
            bgra_bottom_up[bu_idx + 1] = g;
            bgra_bottom_up[bu_idx + 2] = r;
            bgra_bottom_up[bu_idx + 3] = a;
        }
    }

    write_rgba_png(png_path, size, &rgba_top_down);

    let and_mask_row_bytes = ((size + 31) / 32) * 4;
    let and_mask_size = and_mask_row_bytes * size;
    let dib_header_size = 40u32;
    let image_data_size = dib_header_size + (bgra_bottom_up.len() as u32) + and_mask_size;

    let mut ico = Vec::with_capacity(22 + image_data_size as usize);
    ico.extend_from_slice(&0u16.to_le_bytes());
    ico.extend_from_slice(&1u16.to_le_bytes());
    ico.extend_from_slice(&1u16.to_le_bytes());

    ico.push(size as u8);
    ico.push(size as u8);
    ico.push(0);
    ico.push(0);
    ico.extend_from_slice(&1u16.to_le_bytes());
    ico.extend_from_slice(&32u16.to_le_bytes());
    ico.extend_from_slice(&image_data_size.to_le_bytes());
    ico.extend_from_slice(&22u32.to_le_bytes());

    ico.extend_from_slice(&dib_header_size.to_le_bytes());
    ico.extend_from_slice(&(size as i32).to_le_bytes());
    ico.extend_from_slice(&((size * 2) as i32).to_le_bytes());
    ico.extend_from_slice(&1u16.to_le_bytes());
    ico.extend_from_slice(&32u16.to_le_bytes());
    ico.extend_from_slice(&0u32.to_le_bytes());
    ico.extend_from_slice(&(bgra_bottom_up.len() as u32).to_le_bytes());
    ico.extend_from_slice(&0i32.to_le_bytes());
    ico.extend_from_slice(&0i32.to_le_bytes());
    ico.extend_from_slice(&0u32.to_le_bytes());
    ico.extend_from_slice(&0u32.to_le_bytes());

    ico.extend_from_slice(&bgra_bottom_up);
    ico.extend(std::iter::repeat(0u8).take(and_mask_size as usize));

    let _ = fs::write(ico_path, ico);
}

fn generate_bundled_assets_rs(out_dir: &Path) {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let catalog_path = manifest_dir.join("assets").join("games_catalog.json");
    let icons_dir = manifest_dir.join("assets").join("game_icons");

    let mut code = String::new();
    if catalog_path.exists() {
        let p_str = catalog_path.to_string_lossy().replace('\\', "/");
        code.push_str(&format!(
            "pub const BUNDLED_CATALOG_JSON: &str = include_str!(\"{}\");\n",
            p_str
        ));
    } else {
        code.push_str("pub const BUNDLED_CATALOG_JSON: &str = \"[]\";\n");
    }

    code.push_str("pub const BUNDLED_GAME_ICONS: &[(&str, &[u8])] = &[\n");
    if let Ok(entries) = fs::read_dir(&icons_dir) {
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|de| de.path()))
            .filter(|p| {
                p.extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.eq_ignore_ascii_case("ico"))
                    .unwrap_or(false)
            })
            .collect();
        paths.sort();

        for path in paths {
            if let Some(fname) = path.file_name().and_then(|s| s.to_str()) {
                let full_p = path.to_string_lossy().replace('\\', "/");
                code.push_str(&format!(
                    "    (\"{}\", include_bytes!(\"{}\")),\n",
                    fname, full_p
                ));
            }
        }
    }
    code.push_str("];\n");

    let dest = out_dir.join("bundled_assets.rs");
    let _ = fs::write(dest, code);

    let release_exe = manifest_dir
        .join("target")
        .join("release")
        .join("zen-gamma.exe");
    let exe_code = if release_exe.exists() {
        let p_str = release_exe.to_string_lossy().replace('\\', "/");
        format!(
            "pub const BUNDLED_MAIN_EXE: &[u8] = include_bytes!(\"{}\");\n",
            p_str
        )
    } else {
        "pub const BUNDLED_MAIN_EXE: &[u8] = &[];\n".to_string()
    };
    let _ = fs::write(out_dir.join("bundled_exe.rs"), exe_code);
}

fn compile_windows_icon_resource(out_dir: &Path, ico_path: &Path) {
    let rc_path = out_dir.join("zen_gamma.rc");
    let ico_abs = fs::canonicalize(ico_path).unwrap_or_else(|_| ico_path.to_path_buf());
    let ico_clean = ico_abs
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('\\', "/");

    let rc_content = format!("1 ICON \"{}\"\nIDI_ICON1 ICON \"{}\"\n", ico_clean, ico_clean);
    let _ = fs::write(&rc_path, rc_content);

    let obj_path = out_dir.join("zen_gamma_res.o");
    let status = Command::new("windres")
        .args([
            rc_path.to_str().unwrap(),
            "-O",
            "coff",
            "-o",
            obj_path.to_str().unwrap(),
        ])
        .status();

    if let Ok(st) = status {
        if st.success() && obj_path.exists() {
            println!("cargo:rustc-link-arg-bins={}", obj_path.display());
            return;
        }
    }

    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("ui/app_icon.ico");
        let _ = res.compile();
    }
}

fn main() {
    println!("cargo:rerun-if-changed=ui/app.slint");
    println!("cargo:rerun-if-changed=assets/games_catalog.json");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let ico_path = Path::new("ui/app_icon.ico");
    let png_path = Path::new("ui/app_icon.png");
    generate_zen_gamma_icons(ico_path, png_path);
    generate_bundled_assets_rs(&out_dir);
    compile_windows_icon_resource(&out_dir, ico_path);

    slint_build::compile("ui/app.slint").expect("Failed to compile Slint UI definition");
}
