// Copyright 2025 CapsX Contributors
//
// Originally based on BarsCaps by Mikhail Svarichevsky.
// Licensed under the MIT License — see LICENSE for details.
//
// ─────────────────────────────────────────────────────────────────────────────
// CapsX v0.1.0 — CapsLock keyboard-layout switcher for Windows
//
// Behaviour
// ─────────
// • CapsLock alone      → switch foreground window to the next installed layout
// • Modifier + CapsLock → real CapsLock toggle  (default modifier: Shift)
//   Modifier selectable at startup or from the tray menu.
// • -led flag (or tray menu toggle) → use the CapsLock LED as a layout-parity
//   indicator (even index = LED off, odd index = LED on).
//
// Enhancements over the original BarsCaps
// ────────────────────────────────────────
// • Dynamic tray icon shows the current layout code ("EN", "RU", …).
// • LED indicator elegantly achieved via injected events tagged with a magic
//   dwExtraInfo value so our own hook passes them through without recursing.
// • Dynamic layout enumeration — no hardcoded 16-layout limit.
// • Full CI/CD pipeline on GitHub Actions.
//
// Implementation note
// ───────────────────
// We use `windows-sys` (not the higher-level `windows` crate) because
// `windows-sys` compiles on non-Windows hosts (macOS, Linux), enabling
// `cargo check` for local development.  All APIs are raw/unsafe, but
// the code is straightforward Win32 idioms.
// ─────────────────────────────────────────────────────────────────────────────

#![windows_subsystem = "windows"]


use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicUsize, Ordering};

use windows_sys::Win32::{
    Foundation::*,
    Globalization::GetLocaleInfoW,
    Graphics::Gdi::*,
    System::{
        LibraryLoader::GetModuleHandleW,
        Registry::{
            HKEY, RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
            HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_SZ,
        },
    },
    UI::{
        Input::KeyboardAndMouse::*,
        Shell::{
            Shell_NotifyIconW, ShellExecuteW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD,
            NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
        },
        WindowsAndMessaging::*,
    },
};


// ── Compile-time constants ────────────────────────────────────────────────────

const APP_NAME: &str = "CapsX";
const APP_VERSION: &str = "0.1.0";

const TRAY_ICON_UID: u32 = 1001;

const ID_EXIT:      usize = 4001;
const ID_ABOUT:     usize = 4002;
const ID_GITHUB:    usize = 4003;
const ID_TOGGLE_LED: usize = 4005;
const ID_MOD_SHIFT: usize = 4010;   // modifier selector items
const ID_MOD_CTRL:  usize = 4011;
const ID_MOD_ALT:   usize = 4012;
const ID_AUTOSTART: usize = 4013;   // Start with Windows toggle

/// User-defined message for the tray icon callback (WM_APP + 1).
const TRAY_MSG: u32 = WM_APP + 1;

/// Magic value placed in `KEYBDINPUT.dwExtraInfo` when CapsX injects a
/// CapsLock key event to update the physical LED.  The hook callback checks
/// for this and passes the event through without switching layouts.
const MAGIC_LED: usize = 0xCA75_1ED5; // "CapsX LED"

/// ISO 639 two-letter language name (0x59), e.g. "en", "ru", "de".
const LOCALE_SISO639LANGNAME: u32 = 0x0000_0059;

// ── Global state ──────────────────────────────────────────────────────────────

static G_MODIFIER_VK: AtomicU32 = AtomicU32::new(VK_SHIFT as u32); // default: Shift+CapsLock = real toggle
static G_MODIFIER_COMBO: AtomicBool = AtomicBool::new(false);
static G_HOOK: AtomicUsize = AtomicUsize::new(0);
static G_PREV_INDEX: AtomicI32 = AtomicI32::new(-1);
static G_ENABLE_LED: AtomicBool = AtomicBool::new(false);
static G_HWND: AtomicUsize = AtomicUsize::new(0);

// ─────────────────────────────────────────────────────────────────────────────
// String helpers (replaces the w!() macro from the `windows` crate)
// ─────────────────────────────────────────────────────────────────────────────

/// Encode a UTF-8 string literal as a null-terminated UTF-16 `Vec<u16>`.
/// Use `.as_ptr()` to obtain the `PCWSTR` required by Win32 APIs.
fn wz(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────────────────────

fn main() {
    // ── Single-instance guard ─────────────────────────────────────────────────
    // We check for a window with our unique class name before registering it.
    // If one exists, another instance is already running; exit silently.
    // (Less robust than a named mutex but avoids the Win32_System_Threading
    // feature dependency and works fine for a tray-only application.)
    let class_name_pre = wz("CapsXWnd");
    if unsafe { !FindWindowW(class_name_pre.as_ptr(), std::ptr::null()).is_null() } {
        return;
    }

    // ── CLI arguments ─────────────────────────────────────────────────────────
    for arg in std::env::args() {
        match arg.as_str() {
            "-shift" => G_MODIFIER_VK.store(VK_SHIFT as u32, Ordering::Relaxed),
            "-ctrl"  => G_MODIFIER_VK.store(VK_CONTROL as u32, Ordering::Relaxed),
            "-alt"   => G_MODIFIER_VK.store(VK_MENU as u32, Ordering::Relaxed),
            "-led"   => G_ENABLE_LED.store(true, Ordering::Relaxed),
            _ => {}
        }
    }

    unsafe { run() }
}

// ─────────────────────────────────────────────────────────────────────────────
// Core application setup + message loop
// ─────────────────────────────────────────────────────────────────────────────

unsafe fn run() {
    let hmodule = GetModuleHandleW(std::ptr::null());
    if hmodule.is_null() { return; }
    let hinstance = hmodule as HINSTANCE;

    // ── Register a minimal window class ───────────────────────────────────────
    let class_name = wz("CapsXWnd");
    let mut wc: WNDCLASSEXW = std::mem::zeroed();
    wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
    wc.lpfnWndProc = Some(wnd_proc);
    wc.hInstance = hinstance;
    wc.lpszClassName = class_name.as_ptr();
    RegisterClassExW(&wc);

    // ── Create message-only (invisible) window ────────────────────────────────
    let window_name = wz("CapsX");
    let hwnd = CreateWindowExW(
        0,                     // dwExStyle
        class_name.as_ptr(),
        window_name.as_ptr(),
        0,                     // dwStyle
        0, 0, 0, 0,
        HWND_MESSAGE,
        0 as HMENU,
        hinstance,
        std::ptr::null(),
    );
    if hwnd.is_null() { return; }

    G_HWND.store(hwnd as usize, Ordering::Relaxed);

    // ── Initial tray icon: show the current foreground layout ─────────────────
    let init_hkl = detect_foreground_hkl();
    let init_abbr = get_lang_abbr(init_hkl);
    let hicon = create_lang_icon(&init_abbr);
    add_tray_icon(hwnd, hicon, &init_abbr);

    // ── Install low-level keyboard hook ───────────────────────────────────────
    let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(ll_kb_proc), 0 as HINSTANCE, 0);
    if hook.is_null() {
        remove_tray_icon(hwnd);
        return;
    }
    G_HOOK.store(hook as usize, Ordering::Relaxed);

    // Sync LED to initial layout parity.
    let init_idx = G_PREV_INDEX.load(Ordering::Relaxed).max(0) as usize;
    toggle_caps_led(init_idx % 2 == 1);

    // ── Standard Win32 message loop ───────────────────────────────────────────
    let mut msg: MSG = std::mem::zeroed();
    loop {
        let ret = GetMessageW(&mut msg, 0 as HWND, 0, 0);
        if ret == 0 || ret == -1 { break; }
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Language detection
// ─────────────────────────────────────────────────────────────────────────────

unsafe fn detect_foreground_hkl() -> HKL {
    let hwnd = GetForegroundWindow();
    if hwnd.is_null() { return std::ptr::null_mut(); }
    let tid = GetWindowThreadProcessId(hwnd, std::ptr::null_mut());
    GetKeyboardLayout(tid)
}

/// Enumerate all installed keyboard layouts dynamically (no fixed-size limit).
unsafe fn enumerate_layouts() -> Vec<HKL> {
    let count = GetKeyboardLayoutList(0, std::ptr::null_mut()) as usize;
    if count == 0 { return Vec::new(); }
    let mut v: Vec<HKL> = vec![std::ptr::null_mut(); count];
    let actual = GetKeyboardLayoutList(v.len() as i32, v.as_mut_ptr()) as usize;
    v.truncate(actual);
    v
}

// ─────────────────────────────────────────────────────────────────────────────
// Language abbreviation from HKL
// ─────────────────────────────────────────────────────────────────────────────

/// Return a 2-char uppercase ISO 639 abbreviation for an HKL (e.g. "EN", "RU").
/// Stores result as a null-terminated `[u16; 3]`.  Returns "??" on failure.
unsafe fn get_lang_abbr(hkl: HKL) -> [u16; 3] {
    const FALLBACK: [u16; 3] = [b'?' as u16, b'?' as u16, 0];
    if hkl.is_null() { return FALLBACK; }

    // Low 16 bits of HKL = LANGID = LCID for sort-default locale.
    let lang_id = (hkl as usize & 0xFFFF) as u32;
    let mut buf = [0u16; 16];

    // Returns char count including NUL (≥3 means at least 2 real chars).
    let n = GetLocaleInfoW(lang_id, LOCALE_SISO639LANGNAME, buf.as_mut_ptr(), buf.len() as i32);
    if n < 3 { return FALLBACK; }

    [ascii_upper(buf[0]), ascii_upper(buf[1]), 0]
}

#[inline]
fn ascii_upper(c: u16) -> u16 {
    if c >= b'a' as u16 && c <= b'z' as u16 { c - 32 } else { c }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tray icon (GDI-rendered, dynamically updated)
// ─────────────────────────────────────────────────────────────────────────────

/// Render a 32×32 tray icon showing a 2-character language code.
/// Dark charcoal background (#181818), white text, Segoe UI Bold 17pt.
unsafe fn create_lang_icon(abbr: &[u16; 3]) -> HICON {
    const SZ: i32 = 32;

    let hdc_screen = GetDC(0 as HWND);
    let hdc = CreateCompatibleDC(hdc_screen);

    let hbm_color = CreateCompatibleBitmap(hdc_screen, SZ, SZ);
    let hbm_mask = CreateBitmap(SZ, SZ, 1, 1, std::ptr::null());

    let old_bm = SelectObject(hdc, hbm_color as HGDIOBJ);

    // Background
    let full = RECT { left: 0, top: 0, right: SZ, bottom: SZ };
    let bg = CreateSolidBrush(0x00_18_18_18); // near-black (COLORREF = BGR)
    FillRect(hdc, &full, bg);
    DeleteObject(bg as HGDIOBJ);

    // Text
    SetTextColor(hdc, 0x00_FF_FF_FF); // white
    SetBkMode(hdc, TRANSPARENT as i32);

    let face = wz("Segoe UI");
    let hfont = CreateFontW(
        17, 0, 0, 0,
        FW_BOLD as i32,
        0, 0, 0,
        DEFAULT_CHARSET as u32,
        OUT_DEFAULT_PRECIS as u32,
        CLIP_DEFAULT_PRECIS as u32,
        CLEARTYPE_QUALITY as u32,
        0,
        face.as_ptr(),
    );
    let old_font = SelectObject(hdc, hfont as HGDIOBJ);

    let char_count = if abbr[1] == 0 { 1i32 } else { 2i32 };
    let mut dr = full;
    DrawTextW(
        hdc,
        abbr.as_ptr(),
        char_count,
        &mut dr,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );

    SelectObject(hdc, old_font);
    DeleteObject(hfont as HGDIOBJ);
    SelectObject(hdc, old_bm);

    let mut ii: ICONINFO = std::mem::zeroed();
    ii.fIcon = TRUE;
    ii.hbmMask = hbm_mask;
    ii.hbmColor = hbm_color;
    let hicon = CreateIconIndirect(&ii);

    DeleteObject(hbm_color as HGDIOBJ);
    DeleteObject(hbm_mask as HGDIOBJ);
    DeleteDC(hdc);
    ReleaseDC(0 as HWND, hdc_screen);

    hicon
}

/// Register the tray icon with the shell for the first time.
unsafe fn add_tray_icon(hwnd: HWND, hicon: HICON, abbr: &[u16; 3]) {
    let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_ICON_UID;
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    nid.uCallbackMessage = TRAY_MSG;
    nid.hIcon = hicon;
    nid.szTip = build_tooltip(abbr);
    Shell_NotifyIconW(NIM_ADD, &nid);
}

/// Update the tray icon and tooltip after a layout change.
unsafe fn update_tray_icon(hwnd: HWND, hicon: HICON, abbr: &[u16; 3]) {
    let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_ICON_UID;
    nid.uFlags = NIF_ICON | NIF_TIP;
    nid.hIcon = hicon;
    nid.szTip = build_tooltip(abbr);
    Shell_NotifyIconW(NIM_MODIFY, &nid);
}

/// Remove the tray icon from the notification area.
unsafe fn remove_tray_icon(hwnd: HWND) {
    let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_ICON_UID;
    Shell_NotifyIconW(NIM_DELETE, &nid);
}

/// Build a 128-char null-terminated tooltip: "CapsX v0.1.0 — EN".
fn build_tooltip(abbr: &[u16; 3]) -> [u16; 128] {
    let lang = String::from_utf16_lossy(&abbr[..2]);
    let text = format!("{} v{} \u{2014} {}", APP_NAME, APP_VERSION, lang);
    let mut tip = [0u16; 128];
    let wide: Vec<u16> = text.encode_utf16().collect();
    let n = wide.len().min(127);
    tip[..n].copy_from_slice(&wide[..n]);
    tip
}

// ─────────────────────────────────────────────────────────────────────────────
// CapsLock LED indicator
// ─────────────────────────────────────────────────────────────────────────────
//
// Achievement that the original BarsCaps listed as "fragile/complex":
//
// We inject a synthetic CapsLock key-down + key-up pair tagged with MAGIC_LED
// in dwExtraInfo.  Our hook callback recognises MAGIC_LED and passes the event
// through without invoking layout switching.  The OS sees a normal CapsLock
// press and toggles the LED accordingly.
//
// For a 2-layout setup: layout 0 → LED off, layout 1 → LED on.  The user gets
// an instant visual indicator without any external indicator hardware.

unsafe fn toggle_caps_led(want_on: bool) {
    if !G_ENABLE_LED.load(Ordering::Relaxed) { return; }

    // Bit 0 of GetKeyState = CapsLock toggle state (1 = on/engaged).
    let current_on = GetKeyState(VK_CAPITAL as i32) & 1 != 0;
    if current_on == want_on { return; }

    let mut down: INPUT = std::mem::zeroed();
    down.r#type = INPUT_KEYBOARD;
    // SAFETY: we are writing the `ki` variant of the anonymous union.
    down.Anonymous.ki.wVk = VK_CAPITAL;
    down.Anonymous.ki.dwFlags = 0;               // KEYEVENTF_KEYDOWN = 0
    down.Anonymous.ki.dwExtraInfo = MAGIC_LED;

    let mut up: INPUT = std::mem::zeroed();
    up.r#type = INPUT_KEYBOARD;
    up.Anonymous.ki.wVk = VK_CAPITAL;
    up.Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;
    up.Anonymous.ki.dwExtraInfo = MAGIC_LED;

    let inputs = [down, up];
    SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32);
}

// ─────────────────────────────────────────────────────────────────────────────
// Language switching
// ─────────────────────────────────────────────────────────────────────────────

unsafe fn switch_language() {
    let hwnd_fg = GetForegroundWindow();
    if hwnd_fg.is_null() { return; }

    let layouts = enumerate_layouts();
    if layouts.is_empty() { return; }

    let tid = GetWindowThreadProcessId(hwnd_fg, std::ptr::null_mut());
    let cur_hkl = GetKeyboardLayout(tid);
    let cur_lang = cur_hkl as usize & 0xFFFF;

    let prev = G_PREV_INDEX.load(Ordering::Relaxed);
    let cur_idx = layouts
        .iter()
        .position(|&h| h as usize & 0xFFFF == cur_lang)
        .map(|i| i as i32)
        .unwrap_or(if prev >= 0 { prev } else { 0 });

    let next_idx = ((cur_idx + 1) as usize) % layouts.len();
    G_PREV_INDEX.store(next_idx as i32, Ordering::Relaxed);

    let next_hkl = layouts[next_idx];
    SendMessageW(hwnd_fg, WM_INPUTLANGCHANGEREQUEST, 0, next_hkl as LPARAM);

    // Update tray icon, tooltip, and LED.
    let hwnd_self = G_HWND.load(Ordering::Relaxed) as HWND;
    if !hwnd_self.is_null() {
        let abbr = get_lang_abbr(next_hkl);
        let hicon = create_lang_icon(&abbr);
        update_tray_icon(hwnd_self, hicon, &abbr);
        toggle_caps_led(next_idx % 2 == 1);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Low-level keyboard hook
// ─────────────────────────────────────────────────────────────────────────────

unsafe extern "system" fn ll_kb_proc(n_code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    if n_code == HC_ACTION as i32 {
        let kbd = &*(l_param as *const KBDLLHOOKSTRUCT);

        // Pass through LED-injection events (tagged by us).
        if kbd.dwExtraInfo == MAGIC_LED {
            return CallNextHookEx(G_HOOK.load(Ordering::Relaxed) as HHOOK, n_code, w_param, l_param);
        }

        if kbd.vkCode == VK_CAPITAL as u32 {
            if w_param == WM_KEYDOWN as usize || w_param == WM_SYSKEYDOWN as usize {
                let mod_vk = G_MODIFIER_VK.load(Ordering::Relaxed) as i32;
                let modifier_held = GetKeyState(mod_vk) < 0; // high-order bit = pressed

                if modifier_held {
                    G_MODIFIER_COMBO.store(true, Ordering::Relaxed);
                    // Fall through: let the real CapsLock toggle fire.
                } else {
                    switch_language();
                    return 1; // Swallow the keypress.
                }
            } else if w_param == WM_KEYUP as usize || w_param == WM_SYSKEYUP as usize {
                G_MODIFIER_COMBO.store(false, Ordering::Relaxed);
            }
        }
    }

    CallNextHookEx(G_HOOK.load(Ordering::Relaxed) as HHOOK, n_code, w_param, l_param)
}

// ─────────────────────────────────────────────────────────────────────────────
// Window procedure
// ─────────────────────────────────────────────────────────────────────────────

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        m if m == TRAY_MSG => {
            let event = l as u32 & 0xFFFF;
            if event == WM_RBUTTONUP || event == WM_CONTEXTMENU {
                show_context_menu(hwnd);
            }
            0
        }

        WM_COMMAND => {
            on_command(hwnd, w & 0xFFFF);
            0
        }

        WM_DESTROY => {
            remove_tray_icon(hwnd);
            let hook = G_HOOK.swap(0, Ordering::Relaxed) as HHOOK;
            if !hook.is_null() {
                UnhookWindowsHookEx(hook);
            }
            PostQuitMessage(0);
            0
        }

        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Autostart (HKCU\...\Run registry entry)
// ─────────────────────────────────────────────────────────────────────────────

/// Returns `true` when a "CapsX" value exists under the user's Run key.
unsafe fn check_autostart() -> bool {
    let sub_key  = wz("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let val_name = wz("CapsX");
    let mut hkey: HKEY = std::ptr::null_mut();

    if RegOpenKeyExW(HKEY_CURRENT_USER, sub_key.as_ptr(), 0, KEY_READ, &mut hkey) != 0 {
        return false;
    }
    let mut data_size: u32 = 0;
    let found = RegQueryValueExW(
        hkey,
        val_name.as_ptr(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        &mut data_size,
    ) == 0; // 0 = ERROR_SUCCESS
    RegCloseKey(hkey);
    found
}

/// Add or remove CapsX from the Windows startup programs list.
/// The stored command is the quoted path to the current executable.
unsafe fn set_autostart(enable: bool) {
    let sub_key  = wz("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let val_name = wz("CapsX");
    let mut hkey: HKEY = std::ptr::null_mut();

    if RegOpenKeyExW(HKEY_CURRENT_USER, sub_key.as_ptr(), 0, KEY_SET_VALUE, &mut hkey) != 0 {
        return;
    }

    if enable {
        let exe = std::env::current_exe()
            .ok()
            .and_then(|p| p.to_str().map(str::to_owned))
            .unwrap_or_default();
        let value = wz(&format!("\"{}\"", exe)); // quote path in case it contains spaces
        RegSetValueExW(
            hkey,
            val_name.as_ptr(),
            0,
            REG_SZ,
            value.as_ptr() as *const u8,
            (value.len() * 2) as u32, // byte count including null terminator
        );
    } else {
        RegDeleteValueW(hkey, val_name.as_ptr());
    }

    RegCloseKey(hkey);
}

// ─────────────────────────────────────────────────────────────────────────────
// Context menu
// ─────────────────────────────────────────────────────────────────────────────

unsafe fn show_context_menu(hwnd: HWND) {
    let hmenu = CreatePopupMenu();
    if hmenu.is_null() { return; }

    // ── Header ───────────────────────────────────────────────────────────────
    AppendMenuW(hmenu, MF_STRING, ID_ABOUT,  wz("&About CapsX").as_ptr());
    AppendMenuW(hmenu, MF_STRING, ID_GITHUB, wz("&GitHub ↗").as_ptr());
    AppendMenuW(hmenu, MF_SEPARATOR, 0, std::ptr::null());

    // ── Modifier selector (radio-button style checkmarks) ─────────────────────
    // A grayed label acts as a section header.
    AppendMenuW(hmenu, MF_STRING | MF_GRAYED, 0, wz("Modifier key:").as_ptr());

    let cur_mod = G_MODIFIER_VK.load(Ordering::Relaxed) as u16;
    let flag_shift = MF_STRING | if cur_mod == VK_SHIFT   { MF_CHECKED } else { 0 };
    let flag_ctrl  = MF_STRING | if cur_mod == VK_CONTROL { MF_CHECKED } else { 0 };
    let flag_alt   = MF_STRING | if cur_mod == VK_MENU    { MF_CHECKED } else { 0 };
    AppendMenuW(hmenu, flag_shift, ID_MOD_SHIFT, wz("  Shift + CapsLock = real toggle").as_ptr());
    AppendMenuW(hmenu, flag_ctrl,  ID_MOD_CTRL,  wz("  Ctrl + CapsLock = real toggle").as_ptr());
    AppendMenuW(hmenu, flag_alt,   ID_MOD_ALT,   wz("  Alt + CapsLock = real toggle").as_ptr());
    AppendMenuW(hmenu, MF_SEPARATOR, 0, std::ptr::null());

    // ── Feature toggles ───────────────────────────────────────────────────────
    let led_flag      = MF_STRING | if G_ENABLE_LED.load(Ordering::Relaxed) { MF_CHECKED } else { 0 };
    let autostart_flag = MF_STRING | if check_autostart() { MF_CHECKED } else { 0 };
    AppendMenuW(hmenu, led_flag,       ID_TOGGLE_LED, wz("&LED language indicator").as_ptr());
    AppendMenuW(hmenu, autostart_flag, ID_AUTOSTART,  wz("&Start with Windows").as_ptr());
    AppendMenuW(hmenu, MF_SEPARATOR, 0, std::ptr::null());

    AppendMenuW(hmenu, MF_STRING, ID_EXIT, wz("E&xit").as_ptr());

    let mut pt: POINT = std::mem::zeroed();
    GetCursorPos(&mut pt);
    SetForegroundWindow(hwnd);
    TrackPopupMenu(hmenu, TPM_RIGHTBUTTON | TPM_RIGHTALIGN, pt.x, pt.y, 0, hwnd, std::ptr::null());
    PostMessageW(hwnd, WM_NULL, 0, 0);
    DestroyMenu(hmenu);
}

// ─────────────────────────────────────────────────────────────────────────────
// Command handlers
// ─────────────────────────────────────────────────────────────────────────────

unsafe fn on_command(hwnd: HWND, cmd: usize) {
    match cmd {
        ID_EXIT => { DestroyWindow(hwnd); }

        // ── Modifier selection ────────────────────────────────────────────────
        ID_MOD_SHIFT => G_MODIFIER_VK.store(VK_SHIFT   as u32, Ordering::Relaxed),
        ID_MOD_CTRL  => G_MODIFIER_VK.store(VK_CONTROL as u32, Ordering::Relaxed),
        ID_MOD_ALT   => G_MODIFIER_VK.store(VK_MENU    as u32, Ordering::Relaxed),

        // ── LED toggle ────────────────────────────────────────────────────────
        ID_TOGGLE_LED => {
            let was = G_ENABLE_LED.fetch_xor(true, Ordering::Relaxed);
            if !was {
                let idx = G_PREV_INDEX.load(Ordering::Relaxed).max(0) as usize;
                toggle_caps_led(idx % 2 == 1);
            }
        }

        // ── Autostart toggle ──────────────────────────────────────────────────
        ID_AUTOSTART => {
            let currently = check_autostart();
            set_autostart(!currently);
        }

        // ── About ─────────────────────────────────────────────────────────────
        ID_ABOUT => {
            let modifier = match G_MODIFIER_VK.load(Ordering::Relaxed) as u16 {
                VK_SHIFT   => "Shift",
                VK_CONTROL => "Ctrl",
                _          => "Alt",
            };
            let led       = if G_ENABLE_LED.load(Ordering::Relaxed) { "on" } else { "off" };
            let autostart = if check_autostart() { "yes" } else { "no" };
            let body = format!(
                "{} v{} ({})\r\n\r\n\
                 CapsLock keyboard-layout switcher for Windows.\r\n\r\n\
                 \u{2022} CapsLock \u{2192} switch to next keyboard layout\r\n\
                 \u{2022} {} + CapsLock \u{2192} real CapsLock toggle\r\n\
                 \u{2022} LED indicator: {}\r\n\
                 \u{2022} Start with Windows: {}\r\n\r\n\
                 Tray icon shows the current layout (e.g. EN, RU).\r\n\r\n\
                 Startup flags: -shift | -ctrl | -alt | -led\r\n\r\n\
                 Based on BarsCaps by Mikhail Svarichevsky\r\n\
                 github.com/BarsMonster/BarsCaps\r\n\r\n\
                 MIT License",
                APP_NAME, APP_VERSION, arch_name(), modifier, led, autostart,
            );
            let title_w = wz(&format!("About {}", APP_NAME));
            let body_w  = wz(&body);
            MessageBoxW(hwnd, body_w.as_ptr(), title_w.as_ptr(), MB_OK | MB_ICONINFORMATION);
        }

        // ── GitHub ────────────────────────────────────────────────────────────
        ID_GITHUB => {
            let url = wz("https://github.com/thelok1s/CapsX");
            let op  = wz("open");
            ShellExecuteW(hwnd, op.as_ptr(), url.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL);
        }

        _ => {}
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Utilities
// ─────────────────────────────────────────────────────────────────────────────

fn arch_name() -> &'static str {
    #[cfg(target_arch = "x86_64")]  return "x64";
    #[cfg(target_arch = "x86")]     return "x86";
    #[cfg(target_arch = "aarch64")] return "ARM64";
    #[allow(unreachable_code)]      "unknown"
}
