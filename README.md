# Zoomix

Zoomix is a Linux Mint Cinnamon/X11 screen zoom, annotation, and image snip utility inspired by Microsoft Sysinternals ZoomIt.

<img width="990" height="540" alt="image" src="https://github.com/user-attachments/assets/182c102b-754f-4b3e-896d-29a70ce71cde" />

## Scope

Version 0.1 targets X11 only and implements:

- Frozen screen zoom and live zoom.
- On-screen drawing, shapes, highlights, arrows, text, undo, and clear.
- Image-only snip capture to PNG and clipboard.

Video capture, timer, DemoType, OCR, and Wayland support are intentionally out of scope for this release.

## Default Hotkeys

- `Ctrl+1`: static zoom, centered on the current cursor position; view-only until draw is activated
- `Ctrl+4`: live zoom placeholder, currently a stable cursor-centered zoom capture; view-only until draw is activated
- `Ctrl+2`: draw; from idle this is 1:1, from zoom/live zoom it keeps the active zoom level
- `Ctrl+3`: snip at 1:1

Overlay controls:

- Mouse wheel, `+`, `-`: change zoom level
- `p`: pen
- `1`/`p`: pen
- `2`/`r`: rectangle
- `3`/`a`: arrow
- `4`/`l`: line
- `5`/`e`: ellipse
- `6`/`h`: highlight
- `7`/`x`: eraser
- Space or Tab: cycle drawing tool
- `t`: text mode
- `Shift+R`: red
- `g`: green
- `b`: blue
- `y`: yellow
- `k`: black
- `w`: white
- `z` or Backspace: undo
- `c` or Delete: clear annotations
- `Esc`: exit overlay
- In snip mode, drag a region and release to save/copy the PNG.

## Build Requirements

Rust is not vendored in this repo. Install it before building:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup component add rustfmt clippy
```

Linux Mint/Ubuntu dependencies:

```bash
sudo apt install build-essential pkg-config libgtk-3-dev libx11-dev libcairo2-dev libpango1.0-dev libgdk-pixbuf-2.0-dev
```

Build and test:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

Run from source:

```bash
cargo run
```

## Configuration

Zoomix reads `~/.config/zoomix/config.toml`. If the file is absent, defaults are used. See [examples/config.toml](examples/config.toml).

Hotkeys use `+`-separated modifier and key names, for example `Ctrl+1`, `Alt+Shift+Z`, or `Super+S`. Supported modifiers are `Ctrl`, `Alt`, `Shift`, and `Super`.

## Logging

Zoomix writes normal startup, mode transition, save, and error logs to `~/.cache/zoomix/zoomix.log`.
High-volume diagnostics such as raw key events and per-frame overlay draws are disabled by default.

Enable verbose logging for troubleshooting:

```bash
ZOOMIX_VERBOSE_LOG=1 cargo run
```

## Debian Package

Build a local `.deb` package with Debian/Ubuntu packaging tools:

```bash
sudo apt install build-essential debhelper-compat cargo rustc pkg-config libgtk-3-dev libx11-dev libcairo2-dev libpango1.0-dev libgdk-pixbuf-2.0-dev
dpkg-buildpackage -us -uc
```

Install the package:

```bash
sudo apt install ../zoomix_0.1.0_amd64.deb
```

After installation, launch Zoomix from the Linux Mint application menu by searching for `Zoomix`. The launcher runs `zoomix` directly and does not open a terminal window. The package installs:

- `/usr/bin/zoomix`
- `/usr/share/applications/io.github.zoomix.desktop`
- `/usr/share/icons/hicolor/scalable/apps/io.github.zoomix.svg`
- `/usr/share/man/man1/zoomix.1`
- `/etc/xdg/autostart/io.github.zoomix.desktop`

Autostart is installed as an opt-in template and is disabled by default. Enable it for your user account:

```bash
mkdir -p ~/.config/autostart
cp /etc/xdg/autostart/io.github.zoomix.desktop ~/.config/autostart/
sed -i 's/X-GNOME-Autostart-enabled=false/X-GNOME-Autostart-enabled=true/' ~/.config/autostart/io.github.zoomix.desktop
```

Disable autostart:

```bash
rm -f ~/.config/autostart/io.github.zoomix.desktop
```

Uninstall Zoomix:

```bash
sudo apt remove zoomix
```

## Acknowledgements

Zoomix is inspired by the original ZoomIt utility from Mark Russinovich and Microsoft Sysinternals. This project is an independent Linux Mint/X11 implementation and is not affiliated with or endorsed by Microsoft.
