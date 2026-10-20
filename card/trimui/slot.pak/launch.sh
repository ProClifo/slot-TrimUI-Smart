#!/bin/sh
# Runs slot on the TrimUI Smart, from MinUI's Tools or from auto.sh at boot. slot is the shelf and
# MinUI's own emulator plays the carts: when a cart goes in, slot names it in $NEXT and exits, this
# opens MinUI's pak for its platform, and slot comes back when the game is quit. slot keeps its
# games, labels and settings in /slot on the card, apart from MinUI's; the games save where MinUI
# saves. MinUI's menu returns if slot exits without a cart.
SD="${SDCARD_PATH:-/mnt/SDCARD}"
ROOT="$SD/slot"
SYS="$ROOT/System"
LOGS="$ROOT/Logs"
EMUS="${SYSTEM_PATH:-$SD/.system/trimuismart}/paks/Emus"
AUTO_RESUME="${SHARED_USERDATA_PATH:-$SD/.userdata/shared}/.minui/auto_resume.txt"
NEXT="${SLOT_HANDOFF:-/tmp/slot-next}"
PLAYING="$ROOT/Config/playing.txt"
POWEROFF="${SLOT_POWEROFF:-/tmp/poweroff}"
RESUME_SLOT="${SLOT_RESUME_SLOT:-/tmp/resume_slot.txt}"
CPU="${SLOT_CPUFREQ:-/sys/devices/system/cpu/cpu0/cpufreq}"
export SLOT_ROOT="$ROOT"
export SLOT_HANDOFF="$NEXT"

# The panel is portrait, so slot turns its picture a quarter turn onto it. If the picture is
# upside down, change ccw to cw.
export SLOT_ROTATE="${SLOT_ROTATE:-ccw}"

mkdir -p "$LOGS" "$ROOT/Config"

log() {
	echo "$(date '+%F %T') $*" >> "$LOGS/slot-session.log" 2>/dev/null
}

governor="$(cat "$CPU/scaling_governor" 2>/dev/null)"
speed="$(cat "$CPU/scaling_setspeed" 2>/dev/null)"

# slot gets the whole CPU. MinUI's emulator sets its own speed, which needs the governor MinUI left.
fast() {
	echo performance > "$CPU/scaling_governor" 2>/dev/null
}

restore() {
	[ -n "$governor" ] || return
	echo "$governor" > "$CPU/scaling_governor" 2>/dev/null
	case "$speed" in
	'' | *[!0-9]*) ;;
	*) echo "$speed" > "$CPU/scaling_setspeed" 2>/dev/null ;;
	esac
}

# Plays $2 on MinUI's pak named $1, from save state $3 if given. playing.txt stays on the card
# while it runs, so a game cut off by the power switch comes back at the next boot.
play() {
	pak="$EMUS/$1.pak/launch.sh"
	if [ ! -f "$pak" ] || [ ! -f "$2" ]; then
		log "cannot play $2 on $pak"
		return
	fi
	printf '%s\n%s\n' "$1" "$2" > "$PLAYING"
	sync
	[ -n "$3" ] && echo "$3" > "$RESUME_SLOT"
	log "play $2 on $1${3:+ from slot $3}"
	restore
	sh "$pak" "$2"
	log "quit $2"
	rm -f "$PLAYING"
	sync
}

# MinUI's power off cannot throw the switch either: it leaves $POWEROFF, and like MinUI's own
# boot script this saves the clock and blinks until the switch is flipped.
switch_off() {
	[ -f "$POWEROFF" ] || return
	rm -f "$POWEROFF"
	log "waiting for the power switch"
	killall keymon.elf 2>/dev/null
	shutdown
	echo 60000 > "$CPU/scaling_setspeed" 2>/dev/null
	while :; do
		leds_off
		sleep 0.5
		leds_on
		sleep 0.5
	done
}

platform_of() {
	case "$1" in
	"$ROOT/Games/GBA/"*) echo GBA ;;
	"$ROOT/Games/GBC/"*) echo GBC ;;
	"$ROOT/Games/GB/"*) echo GB ;;
	esac
}

# A cart MinUI's emulator saved itself out of, by its menu's power off or by sleeping, comes back
# from that state. One the power switch cut off comes back from its battery save.
if [ -f "$AUTO_RESUME" ]; then
	rom="$SD$(cat "$AUTO_RESUME")"
	tag="$(platform_of "$rom")"
	if [ -n "$tag" ]; then
		rm -f "$AUTO_RESUME" "$PLAYING"
		play "$tag" "$rom" 9
		switch_off
	fi
elif [ -f "$PLAYING" ]; then
	{
		read -r tag
		read -r rom
	} < "$PLAYING"
	play "$tag" "$rom"
	switch_off
fi

while :; do
	rm -f "$NEXT"
	[ -f "$LOGS/slot.log" ] && mv -f "$LOGS/slot.log" "$LOGS/slot.log.1"
	fast
	"$SYS/slot" > "$LOGS/slot.log" 2>&1
	status=$?
	restore
	log "slot exited with $status"
	[ -f "$NEXT" ] || break
	{
		read -r tag
		read -r rom
	} < "$NEXT"
	rm -f "$NEXT"
	play "$tag" "$rom"
	switch_off
done

switch_off
