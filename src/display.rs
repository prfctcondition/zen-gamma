use crate::config::DisplayValues;
use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};

#[cfg(windows)]
use windows::{
    core::s,
    Win32::{
        Foundation::{BOOL, HWND},
        Graphics::Gdi::{GetDC, ReleaseDC, HDC},
        System::{
            Console::{SetConsoleCtrlHandler, CTRL_CLOSE_EVENT, CTRL_C_EVENT, CTRL_SHUTDOWN_EVENT},
            LibraryLoader::{GetProcAddress, LoadLibraryA},
        },
        UI::ColorSystem::{GetDeviceGammaRamp, SetDeviceGammaRamp},
    },
};

/// Standard 3x256 16-bit LUT required by Win32 `SetDeviceGammaRamp` / `GetDeviceGammaRamp`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GammaRamp {
    pub red: [u16; 256],
    pub green: [u16; 256],
    pub blue: [u16; 256],
}

impl Default for GammaRamp {
    /// Identity linear gamma ramp (`i * 257` maps `0..=255` onto `0..=65535`).
    fn default() -> Self {
        let mut channel = [0u16; 256];
        for (i, val) in channel.iter_mut().enumerate() {
            *val = (i as u16) * 257;
        }
        Self {
            red: channel,
            green: channel,
            blue: channel,
        }
    }
}

/// Identifies which Saturation / Digital Vibrance backend is active on the current machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VibranceBackend {
    /// NVIDIA Driver-level Digital Vibrance Control (NVAPI)
    NvidiaDvc,
    /// AMD Display Library (ADL) Driver Saturation
    AmdAdl,
    /// Native Win32 GDI Hardware LUT Chroma Curve
    HardwareLut,
}

impl VibranceBackend {
    pub fn status_description(self) -> &'static str {
        match self {
            VibranceBackend::NvidiaDvc => "GDI Hardware LUT + NVIDIA DVC",
            VibranceBackend::AmdAdl => "GDI Hardware LUT + AMD ADL Driver",
            VibranceBackend::HardwareLut => "GDI Hardware LUT + Chroma Curve",
        }
    }
}

/// Dynamically uninitializes any residual Windows Magnification API state left by prior runs
/// to guarantee Windows DWM restores 100% unconstrained Hardware DirectFlip / MPO.
#[cfg(windows)]
pub fn cleanup_legacy_magnification() {
    unsafe {
        if let Ok(hmod) = LoadLibraryA(s!("Magnification.dll")) {
            if let Some(uninit_addr) = GetProcAddress(hmod, s!("MagUninitialize")) {
                let mag_uninit: unsafe extern "system" fn() -> BOOL = std::mem::transmute(uninit_addr);
                let _ = mag_uninit();
            }
        }
    }
}

#[cfg(not(windows))]
pub fn cleanup_legacy_magnification() {}

/// Dynamically resolved NVIDIA NVAPI Digital Vibrance Control FFI table.
struct NvApiDvcDriver {
    display_handle: isize,
    default_level: i32,
    min_level: i32,
    max_level: i32,
    set_dvc_level_ex: unsafe extern "C" fn(isize, u32, *const NvDisplayDvcInfoEx) -> i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NvDisplayDvcInfoEx {
    version: u32,
    current_level: i32,
    min_level: i32,
    max_level: i32,
    default_level: i32,
}

impl NvApiDvcDriver {
    #[cfg(windows)]
    fn try_init() -> Option<Self> {
        const NVAPI_INITIALIZE_ID: u32 = 0x0150E828;
        const NVAPI_ENUM_NVIDIA_DISPLAY_HANDLE_ID: u32 = 0x9ABDD40D;
        const NVAPI_GET_DVC_INFO_EX_ID: u32 = 0x0E45002D;
        const NVAPI_SET_DVC_LEVEL_EX_ID: u32 = 0x4A82C2B1;

        type QueryInterfaceFn = unsafe extern "C" fn(u32) -> *const c_void;
        type InitializeFn = unsafe extern "C" fn() -> i32;
        type EnumDisplayHandleFn = unsafe extern "C" fn(u32, *mut isize) -> i32;
        type GetDvcInfoExFn = unsafe extern "C" fn(isize, u32, *mut NvDisplayDvcInfoEx) -> i32;
        type SetDvcLevelExFn = unsafe extern "C" fn(isize, u32, *const NvDisplayDvcInfoEx) -> i32;

        unsafe {
            let hmod = LoadLibraryA(s!("nvapi64.dll")).ok()?;
            let query_addr = GetProcAddress(hmod, s!("nvapi_QueryInterface"))?;
            let query_interface: QueryInterfaceFn = std::mem::transmute(query_addr);

            let init_ptr = query_interface(NVAPI_INITIALIZE_ID);
            let enum_ptr = query_interface(NVAPI_ENUM_NVIDIA_DISPLAY_HANDLE_ID);
            let get_dvc_ptr = query_interface(NVAPI_GET_DVC_INFO_EX_ID);
            let set_dvc_ptr = query_interface(NVAPI_SET_DVC_LEVEL_EX_ID);

            if init_ptr.is_null()
                || enum_ptr.is_null()
                || get_dvc_ptr.is_null()
                || set_dvc_ptr.is_null()
            {
                return None;
            }

            let nv_init: InitializeFn = std::mem::transmute(init_ptr);
            if nv_init() != 0 {
                return None;
            }

            let nv_enum_disp: EnumDisplayHandleFn = std::mem::transmute(enum_ptr);
            let mut disp_handle: isize = 0;
            if nv_enum_disp(0, &mut disp_handle) != 0 || disp_handle == 0 {
                return None;
            }

            let nv_get_dvc: GetDvcInfoExFn = std::mem::transmute(get_dvc_ptr);
            let nv_set_dvc: SetDvcLevelExFn = std::mem::transmute(set_dvc_ptr);

            let mut info = NvDisplayDvcInfoEx {
                version: (std::mem::size_of::<NvDisplayDvcInfoEx>() as u32) | (1 << 16),
                ..Default::default()
            };

            if nv_get_dvc(disp_handle, 0, &mut info) != 0 {
                return None;
            }

            Some(Self {
                display_handle: disp_handle,
                default_level: info.current_level,
                min_level: info.min_level,
                max_level: info.max_level.max(1),
                set_dvc_level_ex: nv_set_dvc,
            })
        }
    }

    #[cfg(not(windows))]
    fn try_init() -> Option<Self> {
        None
    }

    /// Maps `saturation_pct` (`0.0..=200.0`, where `100.0` = neutral) to NVIDIA DVC.
    fn set_saturation(&self, saturation_pct: f32) {
        let clamped = saturation_pct.clamp(0.0, 200.0);
        let target_level = if clamped >= 100.0 {
            let t = (clamped - 100.0) / 100.0;
            self.default_level
                + ((self.max_level - self.default_level) as f32 * t).round() as i32
        } else {
            let t = clamped / 100.0;
            self.min_level + ((self.default_level - self.min_level) as f32 * t).round() as i32
        };

        let info = NvDisplayDvcInfoEx {
            version: (std::mem::size_of::<NvDisplayDvcInfoEx>() as u32) | (1 << 16),
            current_level: target_level.clamp(self.min_level, self.max_level),
            min_level: self.min_level,
            max_level: self.max_level,
            default_level: self.default_level,
        };

        unsafe {
            let _ = (self.set_dvc_level_ex)(self.display_handle, 0, &info);
        }
    }

    fn restore_default(&self) {
        let info = NvDisplayDvcInfoEx {
            version: (std::mem::size_of::<NvDisplayDvcInfoEx>() as u32) | (1 << 16),
            current_level: self.default_level,
            min_level: self.min_level,
            max_level: self.max_level,
            default_level: self.default_level,
        };
        unsafe {
            let _ = (self.set_dvc_level_ex)(self.display_handle, 0, &info);
        }
    }
}

/// Dynamically resolved AMD Display Library (`atiadlxx.dll`) Color Saturation controller.
/// Tracks all active `(adapter_idx, display_idx)` pairs so multi-output AMD GPUs update reliably.
struct AmdAdlDriver {
    targets: Vec<(i32, i32, i32, i32, i32)>, // (adapter, display, default_sat, min_sat, max_sat)
    set_color: unsafe extern "C" fn(i32, i32, i32, i32) -> i32,
}

unsafe extern "C" fn adl_malloc_callback(size: i32) -> *mut c_void {
    if size <= 0 {
        return std::ptr::null_mut();
    }
    let layout = std::alloc::Layout::from_size_align(size as usize, 8).unwrap();
    unsafe { std::alloc::alloc(layout) as *mut c_void }
}

impl AmdAdlDriver {
    #[cfg(windows)]
    fn try_init() -> Option<Self> {
        const ADL_DISPLAY_COLOR_SATURATION: i32 = 4;

        type AdlMainControlCreateFn =
            unsafe extern "C" fn(unsafe extern "C" fn(i32) -> *mut c_void, i32) -> i32;
        type AdlDisplayColorGetFn = unsafe extern "C" fn(
            i32,
            i32,
            i32,
            *mut i32,
            *mut i32,
            *mut i32,
            *mut i32,
            *mut i32,
        ) -> i32;
        type AdlDisplayColorSetFn = unsafe extern "C" fn(i32, i32, i32, i32) -> i32;

        unsafe {
            let hmod = LoadLibraryA(s!("atiadlxx.dll")).ok()?;
            let create_addr = GetProcAddress(hmod, s!("ADL_Main_Control_Create"))?;
            let get_color_addr = GetProcAddress(hmod, s!("ADL_Display_Color_Get"))?;
            let set_color_addr = GetProcAddress(hmod, s!("ADL_Display_Color_Set"))?;

            let adl_create: AdlMainControlCreateFn = std::mem::transmute(create_addr);
            let adl_get_color: AdlDisplayColorGetFn = std::mem::transmute(get_color_addr);
            let adl_set_color: AdlDisplayColorSetFn = std::mem::transmute(set_color_addr);

            if adl_create(adl_malloc_callback, 1) != 0 {
                return None;
            }

            let mut targets = Vec::new();
            for adapter_idx in 0..8 {
                for display_idx in 0..6 {
                    let (mut cur, mut def, mut min, mut max, mut step) = (0, 0, 0, 0, 0);
                    if adl_get_color(
                        adapter_idx,
                        display_idx,
                        ADL_DISPLAY_COLOR_SATURATION,
                        &mut cur,
                        &mut def,
                        &mut min,
                        &mut max,
                        &mut step,
                    ) == 0
                        && max > min
                    {
                        targets.push((adapter_idx, display_idx, cur, min, max));
                    }
                }
            }

            if targets.is_empty() {
                return None;
            }

            Some(Self {
                targets,
                set_color: adl_set_color,
            })
        }
    }

    #[cfg(not(windows))]
    fn try_init() -> Option<Self> {
        None
    }

    /// Maps `saturation_pct` (`0.0..=200.0`, where `100.0` = neutral) to AMD ADL saturation across all active outputs.
    fn set_saturation(&self, saturation_pct: f32) {
        const ADL_DISPLAY_COLOR_SATURATION: i32 = 4;
        let clamped = saturation_pct.clamp(0.0, 200.0);

        for &(adapter, display, default_sat, min_sat, max_sat) in &self.targets {
            let target = if clamped >= 100.0 {
                let t = (clamped - 100.0) / 100.0;
                default_sat + ((max_sat - default_sat) as f32 * t).round() as i32
            } else {
                let t = clamped / 100.0;
                min_sat + ((default_sat - min_sat) as f32 * t).round() as i32
            };
            unsafe {
                let _ = (self.set_color)(
                    adapter,
                    display,
                    ADL_DISPLAY_COLOR_SATURATION,
                    target.clamp(min_sat, max_sat),
                );
            }
        }
    }

    fn restore_default(&self) {
        const ADL_DISPLAY_COLOR_SATURATION: i32 = 4;
        for &(adapter, display, default_sat, _, _) in &self.targets {
            unsafe {
                let _ = (self.set_color)(adapter, display, ADL_DISPLAY_COLOR_SATURATION, default_sat);
            }
        }
    }
}

/// Global emergency restoration state so panics, Ctrl+C, or abrupt exits always restore the desktop ramp.
static EMERGENCY_RESTORE_RAMP: OnceLock<Mutex<GammaRamp>> = OnceLock::new();

/// Computes a Win32k-valid 3x256 16-bit Gamma Ramp from the user's `DisplayValues`.
pub fn calculate_gamma_ramp(
    values: &DisplayValues,
    apply_lut_vibrance_fallback: bool,
) -> GammaRamp {
    let gamma_r = if values.rgb_split {
        values.gamma_r
    } else {
        values.gamma
    }
    .clamp(0.50, 3.00);
    let gamma_g = if values.rgb_split {
        values.gamma_g
    } else {
        values.gamma
    }
    .clamp(0.50, 3.00);
    let gamma_b = if values.rgb_split {
        values.gamma_b
    } else {
        values.gamma
    }
    .clamp(0.50, 3.00);

    // Brightness: -50..+50 maps to -0.225..+0.225 black-level lift
    let brightness_offset = (values.brightness.clamp(-50.0, 50.0) / 100.0) * 0.45;
    // Contrast: 0..100 (50 neutral) maps to 0.50..1.50 slope around 0.5 midpoint
    let contrast_factor = 0.50 + (values.contrast.clamp(0.0, 100.0) / 100.0);

    // Optional fallback chroma/vibrance curve differential (neutral at saturation = 100.0)
    let sat_delta = if apply_lut_vibrance_fallback {
        (values.saturation.clamp(0.0, 200.0) - 100.0) / 200.0
    } else {
        0.0
    };

    let build_channel = |channel_gamma: f32, chroma_bias: f32| -> [u16; 256] {
        let mut lut = [0u16; 256];
        let eff_gamma = (channel_gamma + chroma_bias).clamp(0.45, 3.20);
        let inv_gamma = 1.0 / eff_gamma;
        let effective_contrast = (contrast_factor + sat_delta * 0.16).clamp(0.40, 1.85);

        // Shadow toe-boost factor for night-vision & pitch-black presets (active when gamma > 1.3 and brightness > 0)
        let shadow_boost = ((eff_gamma - 1.30).max(0.0) / 1.70).clamp(0.0, 1.0)
            * (values.brightness.max(0.0) / 50.0).clamp(0.0, 1.0);

        // Shift the contrast pivot into the shadow region when lifting shadows so high contrast
        // stretches dark details (`RGB 1..30`) instead of crushing them toward zero.
        let pivot = if shadow_boost > 0.01 {
            (0.50 - shadow_boost * 0.34).clamp(0.16, 0.50)
        } else {
            0.50
        };

        let mut prev_val = 0u16;
        for (i, slot) in lut.iter_mut().enumerate() {
            let x = (i as f32) / 255.0;

            // 1. Power-law gamma response (lifts dark shadows when gamma > 1.0)
            let mut y = x.powf(inv_gamma);

            // 2. Exponential Toe Shadow-Expander: steeply separates pitch-black levels (`i = 1..32`)
            if shadow_boost > 0.01 && i > 0 {
                let steepness = 14.0 + shadow_boost * 34.0;
                let toe = (1.0 - (-x * steepness).exp()) * (0.26 * shadow_boost);
                y = (y + toe).min(1.0);
            }

            // 3. Dynamic range contrast pivoted around shadow-aware midpoint
            y = (y - pivot) * effective_contrast + pivot;

            // 4. Brightness black-level lift
            y += brightness_offset;

            // Convert to 16-bit unsigned integer [0..=65535]
            let raw_u16 = (y.clamp(0.0, 1.0) * 65535.0).round() as u32;

            // 5. Enforce Windows win32kfull.sys GDI sanity bounds so SetDeviceGammaRamp never fails:
            //    win32k requires each ramp entry `i` to stay within `[i * 128, i * 256 + 32512]`
            //    and be monotonically non-decreasing.
            let win32_min = (i as u32) * 128;
            let win32_max = ((i as u32) * 256 + 32512).min(65535);
            let bounded = raw_u16.clamp(win32_min, win32_max) as u16;

            let monotonic = bounded.max(prev_val);
            *slot = monotonic;
            prev_val = monotonic;
        }
        lut
    };

    let r_bias = sat_delta * 0.05;
    let g_bias = sat_delta * 0.02;
    let b_bias = -sat_delta * 0.05;

    GammaRamp {
        red: build_channel(gamma_r, r_bias),
        green: build_channel(gamma_g, g_bias),
        blue: build_channel(gamma_b, b_bias),
    }
}

/// Applies a raw `GammaRamp` to the primary Windows display via `SetDeviceGammaRamp`.
#[cfg(windows)]
pub fn apply_hardware_ramp(ramp: &GammaRamp) -> Result<(), String> {
    unsafe {
        let hdc: HDC = GetDC(HWND::default());
        if hdc.is_invalid() {
            return Err("Failed to acquire primary display HDC via GetDC(NULL)".to_string());
        }

        let ok: BOOL = SetDeviceGammaRamp(hdc, ramp as *const GammaRamp as *const c_void);
        let _ = ReleaseDC(HWND::default(), hdc);

        if ok.as_bool() {
            Ok(())
        } else {
            Err("SetDeviceGammaRamp rejected the LUT or display driver denied access".to_string())
        }
    }
}

#[cfg(not(windows))]
pub fn apply_hardware_ramp(_ramp: &GammaRamp) -> Result<(), String> {
    Ok(())
}

/// Reads the current hardware `GammaRamp` from the primary Windows display.
#[cfg(windows)]
pub fn read_hardware_ramp() -> GammaRamp {
    unsafe {
        let hdc: HDC = GetDC(HWND::default());
        if hdc.is_invalid() {
            return GammaRamp::default();
        }

        let mut ramp = GammaRamp::default();
        let ok: BOOL = GetDeviceGammaRamp(hdc, &mut ramp as *mut GammaRamp as *mut c_void);
        let _ = ReleaseDC(HWND::default(), hdc);

        if ok.as_bool() {
            ramp
        } else {
            GammaRamp::default()
        }
    }
}

#[cfg(not(windows))]
pub fn read_hardware_ramp() -> GammaRamp {
    GammaRamp::default()
}

/// Immediately restores the display to the true calibrated hardware identity ramp.
pub fn emergency_restore_display() {
    let _ = apply_hardware_ramp(&GammaRamp::default());
}

/// Installs global panic and console termination hooks to guarantee clean gamma restoration.
pub fn install_panic_and_exit_hooks(original_ramp: GammaRamp) {
    let _ = EMERGENCY_RESTORE_RAMP.set(Mutex::new(original_ramp));

    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        emergency_restore_display();
        default_panic(panic_info);
    }));

    #[cfg(windows)]
    unsafe {
        unsafe extern "system" fn ctrl_handler(ctrl_type: u32) -> BOOL {
            match ctrl_type {
                CTRL_C_EVENT | CTRL_CLOSE_EVENT | CTRL_SHUTDOWN_EVENT => {
                    emergency_restore_display();
                    BOOL(0)
                }
                _ => BOOL(0),
            }
        }
        let _ = SetConsoleCtrlHandler(Some(ctrl_handler), true);
    }
}

/// High-level Display Controller managing GDI Gamma Ramps, Driver Digital Vibrance, and RAII cleanup.
/// Uses a dedicated coalescing background worker thread so slider drags never block the GUI thread.
#[derive(Clone, Copy)]
enum WorkerJob {
    Apply(DisplayValues),
    Reset,
    Shutdown,
}

struct WorkerSlot {
    pending: Option<WorkerJob>,
}

// Safety: NvApiDvcDriver and AmdAdlDriver hold function pointers and integer handles that are safe to invoke from our dedicated hardware worker thread.
unsafe impl Send for NvApiDvcDriver {}
unsafe impl Send for AmdAdlDriver {}

pub struct DisplayController {
    backend_kind: VibranceBackend,
    worker_state: std::sync::Arc<(Mutex<WorkerSlot>, std::sync::Condvar)>,
}

impl DisplayController {
    pub fn new() -> Self {
        cleanup_legacy_magnification();
        install_panic_and_exit_hooks(read_hardware_ramp());

        let nvapi_driver = NvApiDvcDriver::try_init();
        let amd_driver = if nvapi_driver.is_none() {
            AmdAdlDriver::try_init()
        } else {
            None
        };

        let backend_kind = if nvapi_driver.is_some() {
            VibranceBackend::NvidiaDvc
        } else if amd_driver.is_some() {
            VibranceBackend::AmdAdl
        } else {
            VibranceBackend::HardwareLut
        };

        let use_lut_fallback = nvapi_driver.is_none();
        let worker_state = std::sync::Arc::new((
            Mutex::new(WorkerSlot { pending: None }),
            std::sync::Condvar::new(),
        ));
        let worker_state_bg = std::sync::Arc::clone(&worker_state);

        std::thread::Builder::new()
            .name("zengamma-display-worker".to_string())
            .spawn(move || {
                let (lock, cvar) = &*worker_state_bg;
                let mut last_saturation: f32 = 100.0;

                loop {
                    let job = {
                        let mut slot = match lock.lock() {
                            Ok(g) => g,
                            Err(poisoned) => poisoned.into_inner(),
                        };
                        while slot.pending.is_none() {
                            slot = match cvar.wait(slot) {
                                Ok(g) => g,
                                Err(poisoned) => poisoned.into_inner(),
                            };
                        }
                        slot.pending.take().unwrap()
                    };

                    match job {
                        WorkerJob::Apply(values) => {
                            let ramp = calculate_gamma_ramp(&values, use_lut_fallback);
                            let _ = apply_hardware_ramp(&ramp);

                            if (values.saturation - last_saturation).abs() > 0.05 {
                                last_saturation = values.saturation;
                                if let Some(ref nvapi) = nvapi_driver {
                                    nvapi.set_saturation(values.saturation);
                                } else if let Some(ref amd) = amd_driver {
                                    amd.set_saturation(values.saturation);
                                }
                            }
                        }
                        WorkerJob::Reset => {
                            let _ = apply_hardware_ramp(&GammaRamp::default());
                            if (last_saturation - 100.0).abs() > 0.05 {
                                last_saturation = 100.0;
                                if let Some(ref nvapi) = nvapi_driver {
                                    nvapi.restore_default();
                                }
                                if let Some(ref amd) = amd_driver {
                                    amd.restore_default();
                                }
                            }
                        }
                        WorkerJob::Shutdown => {
                            let _ = apply_hardware_ramp(&GammaRamp::default());
                            if let Some(ref nvapi) = nvapi_driver {
                                nvapi.restore_default();
                            }
                            if let Some(ref amd) = amd_driver {
                                amd.restore_default();
                            }
                            break;
                        }
                    }
                }
            })
            .expect("Failed to spawn display worker thread");

        Self {
            backend_kind,
            worker_state,
        }
    }

    pub fn backend_status(&self) -> &'static str {
        self.backend_kind.status_description()
    }

    /// Enqueues the latest display preset to the coalescing hardware worker thread without blocking the GUI.
    pub fn apply_values(&self, values: &DisplayValues) -> Result<(), String> {
        let (lock, cvar) = &*self.worker_state;
        if let Ok(mut slot) = lock.lock() {
            slot.pending = Some(WorkerJob::Apply(*values));
            cvar.notify_one();
        }
        Ok(())
    }

    /// Restores the display to the true calibrated hardware identity ramp and neutral saturation.
    pub fn reset_to_default(&self) -> Result<(), String> {
        let (lock, cvar) = &*self.worker_state;
        if let Ok(mut slot) = lock.lock() {
            slot.pending = Some(WorkerJob::Reset);
            cvar.notify_one();
        }
        let _ = apply_hardware_ramp(&GammaRamp::default());
        Ok(())
    }
}

impl Drop for DisplayController {
    fn drop(&mut self) {
        let (lock, cvar) = &*self.worker_state;
        if let Ok(mut slot) = lock.lock() {
            slot.pending = Some(WorkerJob::Shutdown);
            cvar.notify_one();
        }
        emergency_restore_display();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_neutral_gamma_ramp_is_monotonic_and_valid() {
        let neutral = DisplayValues::default();
        let ramp = calculate_gamma_ramp(&neutral, false);
        assert_eq!(ramp.red[0], 0);
        assert_eq!(ramp.red[255], 65535);
        for i in 1..256 {
            assert!(ramp.red[i] >= ramp.red[i - 1]);
            assert!(ramp.green[i] >= ramp.green[i - 1]);
            assert!(ramp.blue[i] >= ramp.blue[i - 1]);
        }
    }

    #[test]
    fn test_extreme_shooter_preset_respects_win32k_corridor() {
        let extreme = DisplayValues {
            brightness: 45.0,
            contrast: 95.0,
            gamma: 2.80,
            rgb_split: true,
            gamma_r: 2.90,
            gamma_g: 2.50,
            gamma_b: 1.80,
            saturation: 200.0,
        };
        let ramp = calculate_gamma_ramp(&extreme, true);
        for i in 0..256 {
            let min_bound = (i as u32 * 128) as u16;
            let max_bound = ((i as u32 * 256 + 32512).min(65535)) as u16;
            assert!(ramp.red[i] >= min_bound && ramp.red[i] <= max_bound);
            assert!(ramp.green[i] >= min_bound && ramp.green[i] <= max_bound);
            assert!(ramp.blue[i] >= min_bound && ramp.blue[i] <= max_bound);
        }
    }
}
