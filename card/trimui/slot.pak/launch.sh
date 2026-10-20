#!/bin/sh
# Starts slot from MinUI's Tools on the TrimUI Smart. slot keeps its games, saves and settings in
# /slot on the card, apart from MinUI's, and MinUI comes back when it exits.
SD="${SDCARD_PATH:-/mnt/SDCARD}"
ROOT="$SD/slot"
SYS="$ROOT/System"
LOGS="$ROOT/Logs"
CPU=/sys/devices/system/cpu/cpu0/cpufreq
export SLOT_ROOT="$ROOT"

# The panel is portrait, so slot turns its picture a quarter turn onto it. If the picture is
# upside down, change ccw to cw.
export SLOT_ROTATE="${SLOT_ROTATE:-ccw}"

mkdir -p "$LOGS"
[ -f "$LOGS/slot.log" ] && mv -f "$LOGS/slot.log" "$LOGS/slot.log.1"

governor="$(cat "$CPU/scaling_governor" 2>/dev/null)"
speed="$(cat "$CPU/scaling_setspeed" 2>/dev/null)"
echo performance > "$CPU/scaling_governor" 2>/dev/null

"$SYS/slot" > "$LOGS/slot.log" 2>&1
echo "slot exited with $?" >> "$LOGS/slot.log"

if [ -n "$governor" ]; then
	echo "$governor" > "$CPU/scaling_governor" 2>/dev/null
	case "$speed" in
	'' | *[!0-9]*) ;;
	*) echo "$speed" > "$CPU/scaling_setspeed" 2>/dev/null ;;
	esac
fi
