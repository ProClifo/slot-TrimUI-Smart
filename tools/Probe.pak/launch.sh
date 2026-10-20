#!/bin/sh
# Collects what a slot port to the TrimUI Smart needs to know and that a spec sheet does not say:
# the libc and loader, the framebuffer's format and paging, the input devices and the code
# behind each button, the sound card, the battery and LED nodes, and the stock launcher.
# It is a MinUI pak: copy Probe.pak to /Tools/trimui/ on the card and open it from Tools. It writes
# its findings beside itself and changes nothing else.

HERE="$(cd "$(dirname "$0")" && pwd)"
exec > "$HERE/probe-log.txt" 2>&1
set -x
SERIAL="$(grep -i '^Serial' /proc/cpuinfo 2>/dev/null | awk '{print $NF}')"
[ -n "$SERIAL" ] || SERIAL="$(cat /sys/class/sunxi_info/sys_info 2>/dev/null | grep -i chipid | awk '{print $NF}')"
[ -n "$SERIAL" ] || SERIAL="unknown"
OUT="$HERE/probe-$SERIAL.txt"
EVENTS="$HERE/probe-$SERIAL-events"
PRESS_SECONDS="${PRESS_SECONDS:-30}"

section() {
	printf '\n===== %s =====\n' "$1"
}

run() {
	section "$*"
	sh -c "$*" 2>&1
}

dump() {
	for f in "$@"; do
		[ -e "$f" ] || continue
		if [ -d "$f" ]; then
			printf '%s/\n' "$f"
		elif [ -r "$f" ]; then
			printf '%s: %s\n' "$f" "$(head -c 4096 "$f" 2>/dev/null | tr '\n' ' ')"
		fi
	done
}

{
	run 'date; uname -a; cat /proc/version'
	run 'cat /proc/cpuinfo'
	run 'cat /proc/meminfo | head -8'
	run 'cat /proc/cmdline'
	run 'cat /etc/os-release /etc/openwrt_release /etc/issue /etc/*version* 2>/dev/null'
	run 'echo "$SHELL"; ls -l /bin/sh; busybox 2>&1 | head -2; busybox --list 2>/dev/null | tr "\n" " "'
	run 'ls -l /lib /usr/lib 2>/dev/null | grep -E "ld-|libc[.-]|libc\.so|libm\.|libpthread|libdl|libasound|libSDL|libEGL|libGLES|libmali|libstdc"'
	run 'for f in /lib/ld-*; do echo "$f"; done; ldd --version 2>&1 | head -1; /lib/libc.so 2>&1 | head -2'
	run 'mount; df -k'
	run 'ls -la /mnt /mnt/* 2>/dev/null | head -60'

	section framebuffer
	ls -l /dev/fb* 2>&1
	for fb in /sys/class/graphics/fb*; do
		dump "$fb/name" "$fb/mode" "$fb/modes" "$fb/bits_per_pixel" "$fb/virtual_size" \
			"$fb/stride" "$fb/rotate" "$fb/pan" "$fb/blank"
	done
	command -v fbset > /dev/null 2>&1 && fbset -i 2>&1
	ls /sys/class/disp /sys/class/disp/disp/attr 2>/dev/null
	ls -l /dev/disp /dev/g2d /dev/ion /dev/cedar_dev 2>&1

	section input
	cat /proc/bus/input/devices 2>&1
	ls -l /dev/input 2>&1

	section sound
	cat /proc/asound/cards /proc/asound/pcm 2>&1
	ls -l /dev/snd 2>&1
	command -v aplay > /dev/null 2>&1 && aplay -l 2>&1
	command -v amixer > /dev/null 2>&1 && amixer 2>&1 | head -80
	ls /etc/asound.conf /usr/share/alsa 2>&1

	section power
	for p in /sys/class/power_supply/*; do
		echo "$p"
		cat "$p/uevent" 2>/dev/null
	done
	ls /sys/class/leds /sys/class/backlight /sys/class/pwm 2>&1
	for b in /sys/class/backlight/*; do
		dump "$b/brightness" "$b/max_brightness" "$b/bl_power"
	done
	for c in /sys/devices/system/cpu/cpu0/cpufreq; do
		dump "$c/scaling_governor" "$c/scaling_available_governors" \
			"$c/scaling_available_frequencies" "$c/scaling_min_freq" "$c/scaling_max_freq" \
			"$c/cpuinfo_cur_freq"
	done
	ls /sys/power 2>&1
	dump /sys/power/state /sys/power/mem_sleep

	section 'network and rtc'
	ls /sys/class/net /sys/class/rfkill /sys/class/rtc 2>&1
	dump /sys/class/rtc/rtc0/name

	section 'stock launcher'
	ps 2>&1 || ps w 2>&1
	ls -la /etc/init.d 2>&1
	cat /etc/inittab 2>/dev/null
	ls -la /usr/trimui /usr/trimui/bin /usr/trimui/scripts 2>/dev/null | head -80
	env
} > "$OUT" 2>&1

# Button codes: read every event node raw while the buttons are pressed. On this 32-bit
# kernel each event is 16 bytes, which the dump below keeps whole.
mkdir -p "$EVENTS"
pids=""
for dev in /dev/input/event*; do
	[ -e "$dev" ] || continue
	cat "$dev" > "$EVENTS/$(basename "$dev").bin" 2>/dev/null &
	pids="$pids $!"
done
if [ -n "$pids" ]; then
	echo "Press, one at a time and in this order: UP DOWN LEFT RIGHT A B X Y L R SELECT START MENU (then wait)" > "$HERE/probe-PRESS-BUTTONS-NOW.txt"
	sleep "$PRESS_SECONDS"
	kill $pids 2> /dev/null
	wait 2> /dev/null
	rm -f "$HERE/probe-PRESS-BUTTONS-NOW.txt"
	{
		section 'button events (16 byte input_event, 32-bit)'
		for f in "$EVENTS"/*.bin; do
			echo "$f: $(wc -c < "$f") bytes"
			hexdump -v -e '2/4 "%08x " 2/2 "%04x " 1/4 "%08x" "\n"' "$f" 2>/dev/null \
				|| od -A d -t x4 -v "$f" 2>/dev/null
		done
	} >> "$OUT" 2>&1
fi

sync
echo "wrote $OUT"
