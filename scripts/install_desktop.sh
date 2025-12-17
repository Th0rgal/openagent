#!/bin/bash
# Install desktop automation dependencies for Open Agent
# Run this on the production server: bash scripts/install_desktop.sh

set -e

echo "=== Installing desktop automation packages ==="

# Update package list
apt update

# Install core X11 and window manager
echo "Installing Xvfb and i3..."
apt install -y xvfb i3 x11-utils

# Install automation tools
echo "Installing xdotool and screenshot tools..."
apt install -y xdotool scrot imagemagick

# Install Chromium browser
echo "Installing Chromium..."
apt install -y chromium chromium-sandbox || apt install -y chromium-browser

# Install accessibility tools (AT-SPI2)
echo "Installing AT-SPI2 for accessibility tree..."
apt install -y at-spi2-core libatspi2.0-0 python3-gi python3-gi-cairo gir1.2-atspi-2.0

# Install OCR
echo "Installing Tesseract OCR..."
apt install -y tesseract-ocr

# Install fonts for proper rendering
echo "Installing fonts..."
apt install -y fonts-liberation fonts-dejavu-core fonts-noto

# Create i3 config directory
echo "Creating i3 configuration..."
mkdir -p /root/.config/i3

# Write i3 config
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

echo "i3 configuration written to /root/.config/i3/config"

# Add DESKTOP_ENABLED to environment file
echo "Enabling desktop in environment..."
if ! grep -q "DESKTOP_ENABLED" /etc/open_agent/open_agent.env 2>/dev/null; then
    echo "" >> /etc/open_agent/open_agent.env
    echo "# Desktop automation" >> /etc/open_agent/open_agent.env
    echo "DESKTOP_ENABLED=true" >> /etc/open_agent/open_agent.env
    echo "DESKTOP_RESOLUTION=1920x1080" >> /etc/open_agent/open_agent.env
fi

# Create work and screenshots directories
echo "Creating working directories..."
mkdir -p /root/work/screenshots
mkdir -p /root/tools

# Test installation
echo ""
echo "=== Testing installation ==="

echo -n "Xvfb: "
which Xvfb && echo "OK" || echo "MISSING"

echo -n "i3: "
which i3 && echo "OK" || echo "MISSING"

echo -n "xdotool: "
which xdotool && echo "OK" || echo "MISSING"

echo -n "scrot: "
which scrot && echo "OK" || echo "MISSING"

echo -n "chromium: "
(which chromium || which chromium-browser) && echo "OK" || echo "MISSING"

echo -n "tesseract: "
which tesseract && echo "OK" || echo "MISSING"

echo -n "python3 with gi: "
python3 -c "import gi; print('OK')" 2>/dev/null || echo "MISSING"

echo ""
echo "=== Installation complete ==="
echo "Run: systemctl restart open_agent"
echo "To test manually:"
echo "  Xvfb :99 -screen 0 1920x1080x24 &"
echo "  DISPLAY=:99 i3 &"
echo "  DISPLAY=:99 chromium --no-sandbox &"
echo "  DISPLAY=:99 scrot /tmp/test.png"
