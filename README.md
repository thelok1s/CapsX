# CapsX

[![CI](https://github.com/your-org/capsx/actions/workflows/ci.yml/badge.svg)](https://github.com/your-org/capsx/actions/workflows/ci.yml)
[![Build and Release](https://github.com/your-org/capsx/actions/workflows/release.yml/badge.svg)](https://github.com/your-org/capsx/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

A tiny Windows system-tray utility that switches keyboard layouts with
**CapsLock** instead of toggling caps state.

The tray icon displays the current layout code ("EN", "RU", …) and updates
on every switch.  An optional LED indicator uses the CapsLock LED to show
which layout is active on a 2-layout system.

> **Based on BarsCaps** by Mikhail Svarichevsky — see [CREDITS.md](CREDITS.md).

---

## How it works

| Key combination | Action |
|---|---|
| **CapsLock** | Switch to the next installed keyboard layout (wraps around) |
| **Alt + CapsLock** | Real CapsLock toggle *(modifier is configurable)* |

Additional behaviour:

- Tray icon dynamically shows the current 2-letter language code.
- Right-click tray menu → **LED language indicator** toggles the CapsLock LED
  as a visual indicator (even layout index → LED off, odd → LED on).
  Most useful for 2-language setups.
- Single-instance enforcement via a named Win32 mutex.
- No hardcoded layout-count limit.

---

## Installation

1. **Download** the latest release from
   [Releases](https://github.com/your-org/capsx/releases/latest).
2. Choose the binary for your architecture:
   - `capsx_x64.exe` — 64-bit Intel/AMD
   - `capsx_x86.exe` — 32-bit Intel/AMD
   - `capsx_arm64.exe` — ARM64 (e.g. Snapdragon X Elite)
3. **Run once** to verify the tray icon appears showing your current language.

### Recommended: Task Scheduler (works with admin apps too)

From an **elevated** command prompt (replace `full-path`):

```cmd
schtasks /create /sc onlogon /delay 0000:10 /tn "CapsX" /tr "c:\full-path\capsx_x64.exe -alt -led" /rl highest /f
schtasks /run /tn "CapsX"
```

Open Task Scheduler and for the CapsX task:
- **Untick** "Stop the task if it runs longer than"
- **Untick** "Stop if the computer switches to battery power"
- **Untick** "Start the task only if the computer is on AC power"

### Simpler: Startup folder

Copy the executable into:

```
C:\ProgramData\Microsoft\Windows\Start Menu\Programs\StartUp
```

> ⚠️ This method won't switch layouts in programs running with admin privileges.

---

## Command-line options

```
capsx.exe [-shift | -ctrl | -alt] [-led]
```

| Flag | Meaning |
|---|---|
| `-alt` | **Alt** + CapsLock = real toggle *(default)* |
| `-shift` | Shift + CapsLock = real toggle |
| `-ctrl` | Ctrl + CapsLock = real toggle |
| `-led` | Enable CapsLock LED as language-parity indicator on startup |

The LED can also be toggled at runtime from the tray icon context menu.

---

## Uninstall

1. Exit CapsX from the tray icon context menu.
2. Delete the executable.
3. If using Task Scheduler:
   ```cmd
   schtasks /delete /tn "CapsX" /f
   ```

---

## Build from source

Requires [Rust](https://rustup.rs/) with the MSVC toolchain.

```bash
# Native x64 build
cargo build --release

# Cross-compile for 32-bit
rustup target add i686-pc-windows-msvc
cargo build --release --target i686-pc-windows-msvc

# Cross-compile for ARM64
rustup target add aarch64-pc-windows-msvc
cargo build --release --target aarch64-pc-windows-msvc
```

### CI / Releases

Push a version tag to trigger the release pipeline:

```bash
git tag v0.1.0
git push origin v0.1.0
```

GitHub Actions builds all three architectures in parallel and publishes a
GitHub Release with `capsx_x64.exe`, `capsx_x86.exe`, and `capsx_arm64.exe`.

---

## Credits

See [CREDITS.md](CREDITS.md) for the full credits, including the original
BarsCaps project this is based on.

---

## License

MIT — see [LICENSE](LICENSE).
