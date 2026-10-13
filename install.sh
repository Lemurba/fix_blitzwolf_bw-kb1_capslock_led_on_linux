#!/usr/bin/env bash
# Installs blitzwolf-capsled for the current user.
#
# Usage: ./install.sh [--device VID:PID] [--no-udev] [--yes]
#   --device VID:PID  USB ID of the keyboard (see `lsusb`); default 258a:0049
#   --no-udev         do not install the udev rule (no sudo needed)
#   --yes             do not ask anything, use the given/default values
set -euo pipefail

cd "$(dirname "$0")"
DEFAULT_DEVICE=258a:0049
BIN_DIR="$HOME/.local/bin"
UNIT_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
RULE=/etc/udev/rules.d/70-blitzwolf-capsled.rules
MUSL_TARGET=x86_64-unknown-linux-musl

DEVICE=""
UDEV=1
ASK=1
[ -t 0 ] || ASK=0

usage() { sed -n '2,8s/^# \{0,1\}//p' "$0"; }

while [ $# -gt 0 ]; do
    case "$1" in
        -d|--device) DEVICE="${2:-}"; shift 2 ;;
        --device=*) DEVICE="${1#*=}"; shift ;;
        --no-udev) UDEV=0; shift ;;
        -y|--yes) ASK=0; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done

[ "$(id -u)" -ne 0 ] || { echo "Run as a regular user, not with sudo." >&2; exit 1; }

# Lists connected USB keyboards: "vid:pid  manufacturer product"
list_keyboards() {
    local dev intf
    for dev in /sys/bus/usb/devices/*; do
        [ -r "$dev/idVendor" ] || continue
        for intf in "$dev"/*:*; do
            [ "$(cat "$intf/bInterfaceClass" 2>/dev/null)" = 03 ] || continue
            [ "$(cat "$intf/bInterfaceProtocol" 2>/dev/null)" = 01 ] || continue
            printf '%s:%s  %s %s\n' "$(cat "$dev/idVendor")" "$(cat "$dev/idProduct")" \
                "$(cat "$dev/manufacturer" 2>/dev/null)" "$(cat "$dev/product" 2>/dev/null)"
            break
        done
    done | sort -u
}

# 1. Device ID
if [ -z "$DEVICE" ] && [ "$ASK" -eq 1 ]; then
    mapfile -t KBDS < <(list_keyboards)
    if [ "${#KBDS[@]}" -gt 0 ]; then
        echo "Connected USB keyboards:"
        for i in "${!KBDS[@]}"; do echo "  $((i + 1))) ${KBDS[$i]}"; done
    fi
    read -r -p "Keyboard ID (list number or VID:PID) [$DEFAULT_DEVICE]: " REPLY
    if [[ "$REPLY" =~ ^[0-9]+$ ]] && [ "$REPLY" -ge 1 ] && [ "$REPLY" -le "${#KBDS[@]}" ]; then
        DEVICE="${KBDS[$((REPLY - 1))]%% *}"
    else
        DEVICE="$REPLY"
    fi
fi
DEVICE="${DEVICE:-$DEFAULT_DEVICE}"
DEVICE="${DEVICE,,}"
if ! [[ "$DEVICE" =~ ^[0-9a-f]{4}:[0-9a-f]{4}$ ]]; then
    echo "invalid ID: '$DEVICE' (expected VID:PID, e.g. $DEFAULT_DEVICE)" >&2
    exit 2
fi
VID="${DEVICE%:*}"
PID="${DEVICE#*:}"
echo "device: $DEVICE"

render() { sed -e "s/@VID@/$VID/g" -e "s/@PID@/$PID/g" "$1"; }

# 2. Binary: build with cargo if available, otherwise use the prebuilt one in dist/.
if command -v cargo >/dev/null && [ -f Cargo.toml ]; then
    if rustup target list --installed 2>/dev/null | grep -qx "$MUSL_TARGET"; then
        cargo build --release --target "$MUSL_TARGET"
        BIN="target/$MUSL_TARGET/release/blitzwolf-capsled"
    else
        cargo build --release
        BIN=target/release/blitzwolf-capsled
    fi
elif [ -x dist/blitzwolf-capsled ]; then
    BIN=dist/blitzwolf-capsled
else
    echo "Neither cargo nor dist/blitzwolf-capsled was found." >&2
    exit 1
fi
install -Dm755 "$BIN" "$BIN_DIR/blitzwolf-capsled"
echo "binary installed to $BIN_DIR/blitzwolf-capsled"

# 3. udev rule: gives the logged-in user access to the keyboard's hidraw node.
if [ "$UDEV" -eq 1 ]; then
    TMP_RULE="$(mktemp)"
    trap 'rm -f "$TMP_RULE"' EXIT
    render packaging/70-blitzwolf-capsled.rules.in > "$TMP_RULE"
    if ! cmp -s "$TMP_RULE" "$RULE"; then
        echo "installing udev rule to $RULE (requires sudo)..."
        sudo install -Dm644 "$TMP_RULE" "$RULE"
        sudo udevadm control --reload-rules
        sudo udevadm trigger --subsystem-match=hidraw --action=change
    fi
fi

# 4. User service, started on every login.
mkdir -p "$UNIT_DIR"
render packaging/blitzwolf-capsled.service.in > "$UNIT_DIR/blitzwolf-capsled.service"
systemctl --user daemon-reload
systemctl --user enable blitzwolf-capsled.service
systemctl --user restart blitzwolf-capsled.service

sleep 1
systemctl --user --no-pager --lines=3 status blitzwolf-capsled.service || true
echo
echo "Done. Log: journalctl --user -u blitzwolf-capsled -b"
