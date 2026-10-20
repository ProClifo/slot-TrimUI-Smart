//! Times the soft compositor drawing slot's shelf and a running cart at the TrimUI Smart's 320x240,
//! with no panel: the frames go to memory. Run it on the device to see what drawing alone costs.
//! It boots on a scratch root under /tmp with a few blank carts, writes nothing anywhere else and
//! plays no sound.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use slot::frontend::Frontend;
use slot_gfx::soft::{shared_frame, Compositor};
use slot_input::{Btn, InputSource, Millis, RawEvent};
use slot_power::SimPlatform;

const PANEL: (u32, u32) = (240, 320);
const WINDOW: (u32, u32) = (320, 240);
const FRAMES: usize = 300;

struct Script(VecDeque<Vec<RawEvent>>);

impl InputSource for Script {
    fn poll(&mut self, _now: Millis) -> Vec<RawEvent> {
        self.0.pop_front().unwrap_or_default()
    }
}

fn report(what: &str, mut took: Vec<Duration>) {
    took.sort();
    let ms = |d: Duration| d.as_secs_f64() * 1e3;
    let mean = took.iter().sum::<Duration>() / took.len() as u32;
    println!(
        "{what}: mean {:.2} ms, median {:.2} ms, p95 {:.2} ms, max {:.2} ms over {} frames",
        ms(mean),
        ms(took[took.len() / 2]),
        ms(took[took.len() * 95 / 100]),
        ms(took[took.len() - 1]),
        took.len()
    );
}

fn main() {
    let root = std::env::temp_dir().join(format!("slot-bench-{}", std::process::id()));
    slot::root::ensure(&root);
    for (i, stem) in ["Emerald", "Minish Cap", "Advance Wars", "Golden Sun"]
        .iter()
        .enumerate()
    {
        let mut rom = vec![0u8; 0x100];
        let title = format!("BENCH{i}");
        rom[0xa0..0xa0 + title.len()].copy_from_slice(title.as_bytes());
        let _ = std::fs::write(root.join("Games/GBA").join(format!("{stem}.gba")), rom);
    }
    let mut state = slot_store::read_slot_state(&root);
    state.clock_set = true;
    let _ = slot_store::write_slot_state(&root, &state);

    let frame = shared_frame(PANEL.0, PANEL.1);
    let mut c = Compositor::with_frame(frame);
    c.set_panel_turn(PANEL, true).unwrap();
    let platform = Box::new(SimPlatform::at(PathBuf::from(&root)));
    let mut f = Frontend::boot_with(platform, Box::new(slot::audio::StubSink::draining()));
    f.upload_faces(&mut c);
    f.upload_bezel(&mut c, WINDOW);

    let mut idle = Script(VecDeque::new());
    for _ in 0..30 {
        f.advance(&mut idle);
        f.render(&mut c, WINDOW);
    }
    let mut took = Vec::with_capacity(FRAMES);
    for _ in 0..FRAMES {
        f.advance(&mut idle);
        let began = Instant::now();
        f.render(&mut c, WINDOW);
        took.push(began.elapsed());
    }
    report("shelf, still", took);

    let mut scroll: VecDeque<Vec<RawEvent>> = VecDeque::new();
    for i in 0..FRAMES {
        let btn = if (i / 40) % 2 == 0 {
            Btn::Right
        } else {
            Btn::Left
        };
        scroll.push_back(match i % 20 {
            0 => vec![RawEvent::Down(btn)],
            1 => vec![RawEvent::Up(btn)],
            _ => Vec::new(),
        });
    }
    let mut input = Script(scroll);
    let mut took = Vec::with_capacity(FRAMES);
    for _ in 0..FRAMES {
        f.advance(&mut input);
        let began = Instant::now();
        f.render(&mut c, WINDOW);
        took.push(began.elapsed());
    }
    report("shelf, scrolling", took);

    let mut play: VecDeque<Vec<RawEvent>> = VecDeque::new();
    play.push_back(vec![RawEvent::Down(Btn::A)]);
    play.push_back(vec![RawEvent::Up(Btn::A)]);
    let mut input = Script(play);
    let present = Duration::from_micros(16_742);
    for _ in 0..240 {
        f.advance(&mut input);
        f.drive_emulator();
        f.step_emulator(present, Duration::from_millis(12));
        f.render(&mut c, WINDOW);
    }
    let mut took = Vec::with_capacity(FRAMES);
    for _ in 0..FRAMES {
        f.advance(&mut input);
        f.step_emulator(present, Duration::from_millis(12));
        let began = Instant::now();
        f.render(&mut c, WINDOW);
        took.push(began.elapsed());
    }
    report("playing (mock core)", took);
    let _ = std::fs::remove_dir_all(&root);
}
