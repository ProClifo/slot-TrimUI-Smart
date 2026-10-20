use std::path::{Path, PathBuf};

use slot_input::{Btn, RawEvent};

/// A struct input_event: a timeval, whose two longs are 4 bytes each on 32-bit ARM, then the
/// type, code and value.
pub const EVENT_BYTES: usize = 2 * std::mem::size_of::<std::ffi::c_long>() + 8;

/// Where the type field starts, past the timeval.
pub const EVENT_TYPE_AT: usize = EVENT_BYTES - 8;

pub const EV_SYN: u16 = 0x00;
pub const EV_KEY: u16 = 0x01;
pub const EV_SW: u16 = 0x05;
pub const SW_LID: u16 = 0x00;

pub const EV_ABS: u16 = 0x03;
pub const ABS_HAT0X: u16 = 0x10;
pub const ABS_HAT0Y: u16 = 0x11;
pub const ABS_LSX: u16 = 0x02;
pub const ABS_LSY: u16 = 0x03;

pub const STICK_DEADZONE: i32 = 2048;

const LEFT_STICK_TARGETS: [&str; 6] = [
    "rg34xxsp",
    "rg35xxh",
    "rg35xxpro",
    "rg40xxh",
    "rg40xxv",
    "rgcubexx",
];

pub fn has_left_stick(target: &str) -> bool {
    LEFT_STICK_TARGETS.contains(&target)
}

#[derive(Default)]
pub struct Hat {
    hat: [i32; 2],
    stick: [i32; 2],
    steers: bool,
}

impl Hat {
    pub fn steer_with_stick(&mut self, on: bool) {
        self.steers = on;
    }

    pub fn feed(&mut self, ev: Ev) -> Vec<RawEvent> {
        let tilt = |v: i32| match v {
            v if v > STICK_DEADZONE => 1,
            v if v < -STICK_DEADZONE => -1,
            _ => 0,
        };
        let steer = |v: i32| if self.steers { tilt(v) } else { 0 };
        let (axis, from_stick, value) = match ev.code {
            ABS_HAT0X => (0, false, ev.value.signum()),
            ABS_HAT0Y => (1, false, ev.value.signum()),
            ABS_LSX => (0, true, steer(ev.value)),
            ABS_LSY => (1, true, steer(ev.value)),
            _ => return Vec::new(),
        };
        let was = self.held(axis);
        match from_stick {
            true => self.stick[axis] = value,
            false => self.hat[axis] = value,
        }
        let now = self.held(axis);
        if now == was {
            return Vec::new();
        }
        let ends = [[Btn::Left, Btn::Right], [Btn::Up, Btn::Down]][axis];
        let btn = |v: i32| ends[usize::from(v > 0)];
        let mut out = Vec::new();
        if was != 0 {
            out.push(RawEvent::Up(btn(was)));
        }
        if now != 0 {
            out.push(RawEvent::Down(btn(now)));
        }
        out
    }

    fn held(&self, axis: usize) -> i32 {
        match self.hat[axis] {
            0 => self.stick[axis],
            hat => hat,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Ev {
    pub kind: u16,
    pub code: u16,
    pub value: i32,
}

pub fn decode(bytes: &[u8]) -> Option<Ev> {
    let packet: &[u8; EVENT_BYTES] = bytes.get(..EVENT_BYTES)?.try_into().ok()?;
    let at = EVENT_TYPE_AT;
    Some(Ev {
        kind: u16::from_le_bytes([packet[at], packet[at + 1]]),
        code: u16::from_le_bytes([packet[at + 2], packet[at + 3]]),
        value: i32::from_le_bytes([
            packet[at + 4],
            packet[at + 5],
            packet[at + 6],
            packet[at + 7],
        ]),
    })
}

pub fn to_raw(ev: Ev) -> Option<RawEvent> {
    let btn = match ev.kind {
        EV_KEY => device_code_to_btn(ev.code)?,
        EV_SW if ev.code == SW_LID => Btn::Lid,
        _ => return None,
    };
    match ev.value {
        0 => Some(RawEvent::Up(btn)),
        1 => Some(RawEvent::Down(btn)),
        _ => None,
    }
}

pub fn code_to_btn(code: u16) -> Option<Btn> {
    Some(match code {
        0x130 => Btn::A,
        0x131 => Btn::B,
        0x132 => Btn::Y,
        0x133 => Btn::X,
        0x134 => Btn::L1,
        0x135 => Btn::R1,
        0x136 => Btn::Select,
        0x137 => Btn::Start,
        0x138 => Btn::Menu,
        0x13a => Btn::L2,
        0x13b => Btn::R2,
        115 => Btn::VolUp,
        114 => Btn::VolDown,
        116 => Btn::Power,
        _ => return None,
    })
}

/// The TrimUI Smart's gpio-keys, which send keyboard codes, the D-pad's included. It has no
/// L2, R2, volume or power key.
pub fn trimui_code_to_btn(code: u16) -> Option<Btn> {
    Some(match code {
        103 => Btn::Up,
        108 => Btn::Down,
        105 => Btn::Left,
        106 => Btn::Right,
        57 => Btn::A,
        29 => Btn::B,
        42 => Btn::X,
        56 => Btn::Y,
        15 => Btn::L1,
        14 => Btn::R1,
        97 => Btn::Select,
        28 => Btn::Start,
        1 => Btn::Menu,
        _ => return None,
    })
}

const TRIMUI_WANTED: [u16; 13] = [103, 108, 105, 106, 57, 29, 42, 56, 15, 14, 97, 28, 1];

#[cfg(not(feature = "trimui"))]
fn device_code_to_btn(code: u16) -> Option<Btn> {
    code_to_btn(code)
}

#[cfg(feature = "trimui")]
fn device_code_to_btn(code: u16) -> Option<Btn> {
    trimui_code_to_btn(code)
}

const WANTED: [u16; 15] = [
    0x130, 0x131, 0x132, 0x133, 0x134, 0x135, 0x136, 0x137, 0x138, 0x13a, 0x13b, 0x162, 115, 114,
    116,
];

pub use slot_power::has_bit;

pub fn pick_devices(dev: &Path, sys: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dev) else {
        return Vec::new();
    };
    let mut nodes: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("event"))
        })
        .collect();
    nodes.sort();
    nodes.retain(|node| {
        let Some(name) = node.file_name() else {
            return false;
        };
        wanted_node(&sys.join(name).join("device"))
    });
    nodes
}

fn wanted_node(device: &Path) -> bool {
    let cap = |file: &str| std::fs::read_to_string(device.join("capabilities").join(file));
    let keys = cap("key").unwrap_or_default();
    let lid = cap("sw").is_ok_and(|sw| has_bit(&sw, SW_LID));
    let wanted: &[u16] = match cfg!(feature = "trimui") {
        true => &TRIMUI_WANTED,
        false => &WANTED,
    };
    lid || wanted.iter().any(|bit| has_bit(&keys, *bit))
}

pub fn device_name(sys: &Path, node: &Path) -> String {
    node.file_name()
        .and_then(|n| std::fs::read_to_string(sys.join(n).join("device/name")).ok())
        .map(|n| n.trim().to_string())
        .unwrap_or_else(|| node.display().to_string())
}
