#!/bin/sh
# Tests slot.pak's boot script for the TrimUI Smart off-device: a stand-in slot names carts or
# exits, stand-in MinUI paks record what they were asked to play, and the power switch wait is cut
# short by the first blink.
set -eu

HERE="$(cd "$(dirname "$0")" && pwd)"
SCRIPT="$HERE/trimui/slot.pak/launch.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT HUP INT TERM

fail() {
	echo "$1" >&2
	exit 1
}

# One stand-in card per case. $TMP/case/slot.todo holds what each run of slot does, a line per run:
# "cart GBA name.gba" names a cart, "quit" exits with none, "off" asks for the power switch.
setup() {
	C="$TMP/$1"
	rm -rf "$C"
	mkdir -p "$C/sd/slot/System" "$C/sd/slot/Config" "$C/sd/slot/Games/GBA" "$C/sd/slot/Games/GB" "$C/bin" \
		"$C/sd/.system/trimuismart/paks/Emus" "$C/sd/.userdata/shared/.minui" "$C/cpu"
	: > "$C/sd/slot/Games/GBA/Emerald.gba"
	: > "$C/sd/slot/Games/GB/Tetris.gb"
	echo userspace > "$C/cpu/scaling_governor"
	echo 1536000 > "$C/cpu/scaling_setspeed"
	cat > "$C/sd/slot/System/slot" <<EOF
#!/bin/sh
echo "slot \$(cat "$C/cpu/scaling_governor")" >> "$C/ran"
line="\$(head -n 1 "$C/slot.todo")"
sed -i '1d' "$C/slot.todo"
case "\$line" in
cart*) set -- \$line; printf '%s\n%s\n' "\$2" "$C/sd/slot/Games/\$2/\$3" > "\$SLOT_HANDOFF" ;;
off) : > "$C/poweroff" ;;
esac
EOF
	for tag in GBA GB GBC; do
		mkdir -p "$C/sd/.system/trimuismart/paks/Emus/$tag.pak"
		cat > "$C/sd/.system/trimuismart/paks/Emus/$tag.pak/launch.sh" <<EOF
#!/bin/sh
slot=""
[ -f "$C/resume_slot" ] && slot=" from \$(cat "$C/resume_slot")" && rm -f "$C/resume_slot"
echo "$tag \$(basename "\$1")\$slot, playing \$(tail -n 1 "$C/sd/slot/Config/playing.txt" | xargs basename), governor \$(cat "$C/cpu/scaling_governor")" >> "$C/ran"
[ -f "$C/pak.off" ] && rm -f "$C/pak.off" && : > "$C/poweroff"
exit 0
EOF
	done
	printf '#!/bin/sh\necho shutdown >> "%s/ran"\n' "$C" > "$C/bin/shutdown"
	printf '#!/bin/sh\n:\n' > "$C/bin/leds_on"
	printf '#!/bin/sh\n:\n' > "$C/bin/killall"
	printf '#!/bin/sh\necho blink >> "%s/ran"\nkill -TERM $PPID\n' "$C" > "$C/bin/leds_off"
	chmod +x "$C/sd/slot/System/slot" "$C/bin/"*
	: > "$C/ran"
}

run() {
	PATH="$C/bin:$PATH" SDCARD_PATH="$C/sd" SLOT_HANDOFF="$C/next" SLOT_POWEROFF="$C/poweroff" \
		SLOT_RESUME_SLOT="$C/resume_slot" SLOT_CPUFREQ="$C/cpu" \
		sh "$SCRIPT" > /dev/null 2>&1 || true
}

expect() {
	got="$(cat "$C/ran")"
	[ "$got" = "$1" ] || fail "$2:
expected:
$1
got:
$got"
}

setup plays
printf 'cart GBA Emerald.gba\ncart GB Tetris.gb\nquit\n' > "$C/slot.todo"
run
expect "slot performance
GBA Emerald.gba, playing Emerald.gba, governor userspace
slot performance
GB Tetris.gb, playing Tetris.gb, governor userspace
slot performance" "a cart named by slot plays on its platform's pak, and slot comes back after"
[ ! -f "$C/sd/slot/Config/playing.txt" ] || fail "a quit game is still marked as playing"
[ "$(cat "$C/cpu/scaling_setspeed")" = 1536000 ] || fail "MinUI's CPU speed was not put back"

setup cut
printf 'GBA\n%s\n' "$C/sd/slot/Games/GBA/Emerald.gba" > "$C/sd/slot/Config/playing.txt"
printf 'quit\n' > "$C/slot.todo"
run
expect "GBA Emerald.gba, playing Emerald.gba, governor userspace
slot performance" "a game the power switch cut off comes back at boot, from its battery save"

setup resume
printf 'GBA\n%s\n' "$C/sd/slot/Games/GBA/Emerald.gba" > "$C/sd/slot/Config/playing.txt"
printf '/slot/Games/GB/Tetris.gb' > "$C/sd/.userdata/shared/.minui/auto_resume.txt"
printf 'quit\n' > "$C/slot.todo"
run
expect "GB Tetris.gb from 9, playing Tetris.gb, governor userspace
slot performance" "a game MinUI saved itself out of resumes from slot 9 and wins over playing.txt"
[ ! -f "$C/sd/.userdata/shared/.minui/auto_resume.txt" ] || fail "auto_resume.txt was left for MinUI to resume again"

setup minui
printf '/Roms/Game Boy (GB)/Tetris.gb' > "$C/sd/.userdata/shared/.minui/auto_resume.txt"
printf 'quit\n' > "$C/slot.todo"
run
expect "slot performance" "a game from MinUI's own Roms is left for MinUI's menu to resume"
[ -f "$C/sd/.userdata/shared/.minui/auto_resume.txt" ] || fail "MinUI's own resume was taken"

setup off-in-game
printf 'cart GBA Emerald.gba\nquit\n' > "$C/slot.todo"
: > "$C/pak.off"
run
expect "slot performance
GBA Emerald.gba, playing Emerald.gba, governor userspace
shutdown
blink" "powering off from MinUI's menu waits for the switch instead of returning to slot"

setup off-in-slot
printf 'off\n' > "$C/slot.todo"
run
expect "slot performance
shutdown
blink" "powering off from slot waits for the switch"

setup missing
printf 'cart GBA Gone.gba\nquit\n' > "$C/slot.todo"
run
expect "slot performance
slot performance" "a rom that is not there plays nothing and slot comes back"

echo "trimui launch: ok"
