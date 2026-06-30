Zoomix is a Linux Mint X11 utility for screen zoom, annotation, and image snip capture.

Launch Zoomix from the Linux Mint application menu by searching for "Zoomix".
The launcher starts the background hotkey listener without opening a terminal.

Default hotkeys:

- Ctrl+Shift+1: static zoom
- Ctrl+Shift+4: live zoom using Cinnamon's native magnifier
- Ctrl+Shift+2: draw
- Ctrl+Shift+3: snip

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
