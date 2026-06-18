# Credits

CapsX is a Rust rewrite of the **BarsCaps** utility.

---

## BarsCaps (original project)

| Field | Value |
|---|---|
| **Author** | Mikhail Svarichevsky |
| **Repository** | <https://github.com/BarsMonster/BarsCaps> |
| **Homepage** | <https://3.14.by/> |
| **Language** | C++ / Win32 |
| **License** | MIT |

BarsCaps pioneered the core idea: intercept CapsLock at the low-level keyboard
hook layer, suppress the caps-lock toggle, and instead cycle through the
installed keyboard layouts.  The original C++ implementation is compact
(~200 lines) and has been in production use for over a decade.

### Features inherited from BarsCaps

- Low-level `WH_KEYBOARD_LL` keyboard hook for system-wide intercept
- Modifier + CapsLock passthrough for real CapsLock toggle (`-shift`/`-ctrl`/`-alt`)
- Up to N installed layouts, cycled in sequence
- `WM_INPUTLANGCHANGEREQUEST`-based layout switching (compatible with all apps)
- Named Win32 mutex for single-instance enforcement
- System-tray icon with right-click context menu
- x86 / x64 / ARM64 Windows support

---

## CapsX additions

CapsX brings improvements that were listed as intentionally deferred in the
original project or not present at all:

| Addition | Details |
|---|---|
| **Dynamic language icon** | Tray icon displays the current 2-letter ISO 639 language code ("EN", "RU", …) and updates on every switch |
| **LED indicator** | CapsLock LED mirrors layout parity (even index → off, odd index → on); toggle via `-led` flag or tray menu.  Works reliably for 2-layout setups |
| **No layout-count limit** | Layout list is dynamically enumerated (no hardcoded 16-item cap) |
| **CI/CD pipeline** | GitHub Actions workflows for `cargo check`, Clippy, and multi-arch release builds |
| **Rust implementation** | Memory-safe, no runtime dependencies, 100–200 KB binary |

---

## License

CapsX is released under the [MIT License](LICENSE), the same license as
the original BarsCaps project.
