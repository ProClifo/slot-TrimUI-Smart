#!/bin/sh
# Boots the TrimUI Smart into slot. MinUI runs .userdata/trimuismart/auto.sh once at boot, before
# its menu, so copying this file there opens slot first; MinUI's menu follows if slot exits.
# Delete that copy to boot into MinUI again.
sh "${SDCARD_PATH:-/mnt/SDCARD}/Tools/trimuismart/slot.pak/launch.sh"
