# Desktop Environment Setup

This guide covers setting up a headless desktop environment for the Open Agent to control browsers and graphical applications.

## Overview

The desktop automation stack consists of:
- **Xvfb**: Virtual framebuffer for headless X11
- **i3**: Minimal, deterministic window manager
- **xdotool**: Keyboard and mouse automation
- **scrot**: Screenshot capture
- **Chromium**: Web browser
- **AT-SPI2**: Accessibility tree extraction
- **Tesseract**: OCR fallback for text extraction

## Installation (Ubuntu/Debian)

```bash
# Update package list
apt update

# Install core X11 and window manager
apt install -y xvfb i3 x11-utils

# Install automation tools
apt install -y xdotool scrot imagemagick

# Install Chromium browser
apt install -y chromium chromium-sandbox

# Install accessibility tools (AT-SPI2)
apt install -y at-spi2-core libatspi2.0-0 python3-gi python3-gi-cairo gir1.2-atspi-2.0

# Install OCR
apt install -y tesseract-ocr

# Install fonts for proper rendering
apt install -y fonts-liberation fonts-dejavu-core
```

## i3 Configuration

Create a minimal, deterministic i3 config at `/root/.config/i3/config`:

```bash
mkdir -p /root/.config/i3
cat > /root/.config/i3/config << 'EOF'
# Open Agent i3 Config - Minimal and Deterministic
# No decorations, no animations, simple layout

# Use Super (Mod4) as modifier
set $mod Mod4

# Font for window titles (not shown due to no decorations)
font pango:DejaVu Sans Mono 10

# Remove window decorations
default_border none
default_floating_border none

# No gaps
gaps inner 0
gaps outer 0

# Focus follows mouse (predictable behavior)
focus_follows_mouse no

# Disable window titlebars completely
for_window [class=".*"] border pixel 0

# Make all windows float by default for easier positioning
# (comment out if you prefer tiling)
# for_window [class=".*"] floating enable

# Chromium-specific: maximize and remove sandbox issues
for_window [class="Chromium"] border pixel 0
for_window [class="chromium"] border pixel 0

# Keybindings (minimal set)
bindsym $mod+Return exec chromium --no-sandbox --disable-gpu
bindsym $mod+Shift+q kill
bindsym $mod+d exec dmenu_run

# Focus movement
bindsym $mod+h focus left
bindsym $mod+j focus down
bindsym $mod+k focus up
bindsym $mod+l focus right

# Exit i3
bindsym $mod+Shift+e exit

# Reload config
bindsym $mod+Shift+r reload

# Workspace setup (just workspace 1)
workspace 1 output primary
EOF
```

## Environment Variables

Add these to `/etc/open_agent/open_agent.env`:

```bash
# Enable desktop automation tools
DESKTOP_ENABLED=true

# Xvfb resolution (width x height)
DESKTOP_RESOLUTION=1920x1080

# Starting display number (will increment for concurrent sessions)
DESKTOP_DISPLAY_START=99
```

## Manual Testing

Test the setup manually before enabling for the agent:

```bash
# Start Xvfb on display :99
Xvfb :99 -screen 0 1920x1080x24 &
export DISPLAY=:99

# Start i3 window manager
i3 &

# Launch Chromium
chromium --no-sandbox --disable-gpu &

# Take a screenshot
sleep 2
scrot /tmp/test_screenshot.png

# Verify screenshot exists
ls -la /tmp/test_screenshot.png

# Test xdotool
xdotool getactivewindow

# Clean up
pkill -f "Xvfb :99"
```

## AT-SPI Accessibility Tree

Test accessibility tree extraction:

```bash
export DISPLAY=:99
export DBUS_SESSION_BUS_ADDRESS=unix:path=/tmp/dbus-session-$$

# Start dbus session (required for AT-SPI)
dbus-daemon --session --fork --address=$DBUS_SESSION_BUS_ADDRESS

# Python script to dump accessibility tree
python3 << 'EOF'
import gi
gi.require_version('Atspi', '2.0')
from gi.repository import Atspi

def print_tree(obj, indent=0):
    try:
        name = obj.get_name() or ""
        role = obj.get_role_name()
        if name or role != "unknown":
            print("  " * indent + f"[{role}] {name}")
        for i in range(obj.get_child_count()):
            child = obj.get_child_at_index(i)
            if child:
                print_tree(child, indent + 1)
    except Exception as e:
        pass

desktop = Atspi.get_desktop(0)
for i in range(desktop.get_child_count()):
    app = desktop.get_child_at_index(i)
    if app:
        print_tree(app)
EOF
```

## OCR with Tesseract

Test OCR on a screenshot:

```bash
# Take screenshot and run OCR
DISPLAY=:99 scrot /tmp/screen.png
tesseract /tmp/screen.png stdout

# With language hint
tesseract /tmp/screen.png stdout -l eng
```

## Troubleshooting

### Xvfb won't start
```bash
# Check if display is already in use
ls -la /tmp/.X*-lock
# Remove stale lock files
rm -f /tmp/.X99-lock /tmp/.X11-unix/X99
```

### Chromium sandbox issues
Always use `--no-sandbox` flag when running as root:
```bash
chromium --no-sandbox --disable-gpu
```

### xdotool can't find windows
```bash
# List all windows
xdotool search --name ""

# Ensure DISPLAY is set
echo $DISPLAY
```

### AT-SPI not working
```bash
# Ensure dbus is running
export $(dbus-launch)

# Enable AT-SPI for Chromium
chromium --force-renderer-accessibility --no-sandbox
```

### No fonts rendering
```bash
# Install additional fonts
apt install -y fonts-noto fonts-freefont-ttf

# Rebuild font cache
fc-cache -fv
```

## Security Considerations

- The agent runs with full system access
- Xvfb sessions are isolated per-task
- Sessions are cleaned up when tasks complete
- Chromium runs with `--no-sandbox` (required for root, but limits isolation)
- Consider running in a container for additional isolation

## Session Lifecycle

1. **Task starts**: Agent calls `desktop_start_session`
2. **Xvfb starts**: Virtual display created at `:99` (or next available)
3. **i3 starts**: Window manager provides predictable layout
4. **Browser launches**: Chromium opens (if requested)
5. **Agent works**: Screenshots, clicks, typing via desktop_* tools
6. **Task ends**: `desktop_stop_session` kills Xvfb and children
7. **Cleanup**: Any orphaned sessions killed on task failure
