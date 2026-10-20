#!/bin/sh
# Times slot on the TrimUI Smart: the soft compositor drawing the shelf and a cart, and each core
# running a game from slot's own shelves with no drawing at all. The screen does not change while
# it runs, for a minute or two; the numbers land in bench-results.txt beside this script.
HERE="$(cd "$(dirname "$0")" && pwd)"
SD="${SDCARD_PATH:-/mnt/SDCARD}"
SYS="$SD/slot/System"
GAMES="$SD/slot/Games"
OUT="$HERE/bench-results.txt"
CPU=/sys/devices/system/cpu/cpu0/cpufreq

first() {
	for f in "$@"; do
		[ -f "$f" ] && { echo "$f"; return; }
	done
}

governor="$(cat "$CPU/scaling_governor" 2>/dev/null)"
speed="$(cat "$CPU/scaling_setspeed" 2>/dev/null)"
echo performance > "$CPU/scaling_governor" 2>/dev/null

{
	date
	echo "cpu $(cat "$CPU/cpuinfo_cur_freq" 2>/dev/null) kHz, governor $(cat "$CPU/scaling_governor" 2>/dev/null)"
	echo
	echo "== drawing, no emulation"
	SLOT_SILENT=1 "$HERE/trimui_bench" 2>/dev/null

	gba="$(first "$GAMES"/GBA/*.gba)"
	gb="$(first "$GAMES"/GB/*.gb "$GAMES"/GBC/*.gbc "$GAMES"/GB/*.gbc "$GAMES"/GBC/*.gb)"
	bench() {
		echo
		echo "== $1 on $(basename "$3")"
		"$HERE/core_bench" "$SYS/$2_libretro.so" "$3" --system "$SD/slot/BIOS" \
			--frames 600 --warmup 120 --repeats 3 2>&1 | grep -E "repeat|median|error"
		echo "-- a save state, as rewind takes one every other frame"
		"$HERE/core_bench" "$SYS/$2_libretro.so" "$3" --system "$SD/slot/BIOS" \
			--frames 120 --serialize 2>&1 | tail -2
	}
	if [ -n "$gba" ]; then
		bench gpSP gpsp "$gba"
		bench mGBA mgba "$gba"
	else
		echo "no .gba in $GAMES/GBA"
	fi
	if [ -n "$gb" ]; then
		bench mGBA mgba "$gb"
	else
		echo "no .gb or .gbc in $GAMES/GB or $GAMES/GBC"
	fi
	echo
	echo "done $(date)"
} > "$OUT" 2>&1

if [ -n "$governor" ]; then
	echo "$governor" > "$CPU/scaling_governor" 2>/dev/null
	case "$speed" in
	'' | *[!0-9]*) ;;
	*) echo "$speed" > "$CPU/scaling_setspeed" 2>/dev/null ;;
	esac
fi
sync
