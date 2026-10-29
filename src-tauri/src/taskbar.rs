//! Whether the Windows taskbar is light or dark, so the tray icon uses the
//! right ink, and a watcher that reports when the user changes it.

const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";

/// True for a light taskbar. Missing value (old Windows 10): dark.
#[cfg(windows)]
pub fn uses_light_theme() -> bool {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    use windows::core::HSTRING;

    let mut value: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(PERSONALIZE),
            &HSTRING::from("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    status.is_ok() && value == 1
}

#[cfg(not(windows))]
pub fn uses_light_theme() -> bool {
    false
}

/// Windows' accent colour as `#rrggbb`, for the "system accent" setting.
#[cfg(windows)]
pub fn system_accent() -> Option<String> {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    use windows::core::HSTRING;

    let mut value: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(r"SoftwareMicrosoftWindowsDWM"),
            &HSTRING::from("AccentColor"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    // Stored as 0xAABBGGRR.
    status.is_ok().then(|| {
        let (r, g, b) = (value & 0xff, (value >> 8) & 0xff, (value >> 16) & 0xff);
        format!("#{r:02x}{g:02x}{b:02x}")
    })
}

#[cfg(not(windows))]
pub fn system_accent() -> Option<String> {
    None
}

/// Calls `on_change` whenever the personalisation settings change. The
/// thread sleeps in the kernel until then, so it costs nothing.
#[cfg(windows)]
pub fn watch_theme(on_change: impl Fn() + Send + 'static) {
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_NOTIFY, REG_NOTIFY_CHANGE_LAST_SET, RegCloseKey,
        RegNotifyChangeKeyValue, RegOpenKeyExW,
    };
    use windows::core::HSTRING;

    let spawned = std::thread::Builder::new()
        .name("rumor-theme-watch".into())
        .spawn(move || {
            let mut key = HKEY::default();
            let opened = unsafe {
                RegOpenKeyExW(
                    HKEY_CURRENT_USER,
                    &HSTRING::from(PERSONALIZE),
                    None,
                    KEY_NOTIFY,
                    &mut key,
                )
            };
            if opened.is_err() {
                log::warn!("taskbar theme changes won't be followed");
                return;
            }
            let mut light = uses_light_theme();
            loop {
                let waited = unsafe {
                    RegNotifyChangeKeyValue(key, false, REG_NOTIFY_CHANGE_LAST_SET, None, false)
                };
                if waited.is_err() {
                    break;
                }
                // The key also holds other settings; only redraw on a real change.
                let now = uses_light_theme();
                if now != light {
                    light = now;
                    on_change();
                }
            }
            unsafe {
                let _ = RegCloseKey(key);
            }
        });
    if let Err(e) = spawned {
        log::warn!("theme watcher not started: {e}");
    }
}

#[cfg(not(windows))]
pub fn watch_theme(_on_change: impl Fn() + Send + 'static) {}
