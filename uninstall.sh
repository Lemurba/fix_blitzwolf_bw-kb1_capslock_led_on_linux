#!/usr/bin/env bash
# Removes blitzwolf-capsled for the current user.
set -uo pipefail

UNIT="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/blitzwolf-capsled.service"
RULE=/etc/udev/rules.d/70-blitzwolf-capsled.rules

systemctl --user disable --now blitzwolf-capsled.service 2>/dev/null
rm -f "$UNIT" "$HOME/.local/bin/blitzwolf-capsled"
systemctl --user daemon-reload

if [ -e "$RULE" ]; then
    echo "removing udev rule (requires sudo)..."
    sudo rm -f "$RULE"
    sudo udevadm control --reload-rules
fi
echo "Removed."
