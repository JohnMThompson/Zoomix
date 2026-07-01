Zoomix is a Linux Mint X11 utility for screen zoom, annotation, and image snip capture.

Launch Zoomix from the Linux Mint application menu by searching for "Zoomix".
The launcher starts the background hotkey listener without opening a terminal.

Default hotkeys:

- Ctrl+Shift+1: static zoom
- Ctrl+Shift+4: interactive live zoom using Cinnamon's native magnifier
- Ctrl+Shift+2: draw
- Ctrl+Shift+3: snip

Zoom, Draw, and Snip can be combined. Switching from a zoomed Draw view to Snip
preserves the magnification and annotations in the saved image, then returns to
the zoomed Draw view after capture.

Press the active mode's hotkey again to turn that mode off.

Live Zoom leaves the desktop interactive. Click, type, and scroll normally in
applications; use Ctrl+Shift+Wheel to change Live Zoom magnification.

Chrome reserves Ctrl+number for tab selection, so those combinations cannot be
reliably overridden. Zoomix uses Ctrl+Shift+number by default.

User configuration is read from ~/.config/zoomix/config.toml. See the upstream
examples/config.toml for all available settings.

Autostart is installed as an opt-in template at /etc/xdg/autostart/io.github.zoomix.desktop.
To enable it for one user:

    mkdir -p ~/.config/autostart
    cp /etc/xdg/autostart/io.github.zoomix.desktop ~/.config/autostart/
    sed -i 's/X-GNOME-Autostart-enabled=false/X-GNOME-Autostart-enabled=true/' ~/.config/autostart/io.github.zoomix.desktop

To disable autostart:

    rm -f ~/.config/autostart/io.github.zoomix.desktop

See the upstream README for build, configuration, and additional usage details.
