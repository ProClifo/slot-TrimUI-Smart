//! The TrimUI Smart plays its carts in MinUI's emulator. slot names the cart in a file and exits,
//! and the boot script in slot.pak opens MinUI's pak for its platform, then slot again after.

use std::path::{Path, PathBuf};

use slot_store::Platform;

const NEXT: &str = "/tmp/slot-next";
const NEXT_VAR: &str = "SLOT_HANDOFF";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handoff {
    pub platform: Platform,
    pub rom: PathBuf,
}

impl Handoff {
    /// The file's two lines: MinUI's pak for the platform, then the rom.
    pub fn text(&self) -> String {
        format!("{}\n{}\n", self.platform.dir_name(), self.rom.display())
    }
}

pub fn path() -> PathBuf {
    std::env::var_os(NEXT_VAR)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(NEXT))
}

pub fn write(path: &Path, handoff: &Handoff) -> std::io::Result<()> {
    slot_store::atomic_write(path, handoff.text().as_bytes())
}
