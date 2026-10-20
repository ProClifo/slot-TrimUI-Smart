//! The real frontend drawn through the soft compositor onto the TrimUI Smart's 240x320 portrait
//! panel. Set SCRATCH_PNG_DIR to keep the frames, as the panel shows them and turned upright.
#![cfg(feature = "trimui")]

mod common;

use std::collections::VecDeque;

use common::{clocked, tmp_root_with_carts};
use slot::frontend::Frontend;
use slot_gfx::soft::{shared_frame, Compositor, SharedFrame};
use slot_input::{InputSource, Millis, RawEvent};
use slot_power::SimPlatform;

const PANEL: (u32, u32) = (240, 320);
const WINDOW: (u32, u32) = (320, 240);

struct Script(VecDeque<Vec<RawEvent>>);

impl InputSource for Script {
    fn poll(&mut self, _now: Millis) -> Vec<RawEvent> {
        self.0.pop_front().unwrap_or_default()
    }
}

/// The landscape picture back out of the clockwise-turned panel.
fn upright(frame: &SharedFrame) -> Vec<u32> {
    let f = frame.borrow();
    let mut out = vec![0; (WINDOW.0 * WINDOW.1) as usize];
    for y in 0..WINDOW.1 {
        for x in 0..WINDOW.0 {
            out[(y * WINDOW.0 + x) as usize] = f.px[(x * f.w + (WINDOW.1 - 1 - y)) as usize];
        }
    }
    out
}

fn write_png(name: &str, w: u32, h: u32, px: &[u32]) {
    let Ok(dir) = std::env::var("SCRATCH_PNG_DIR") else {
        return;
    };
    let rgba: Vec<u8> = px
        .iter()
        .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8, 0xff])
        .collect();
    let file = std::fs::File::create(format!("{dir}/{name}.png")).expect("create png");
    let mut e = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header()
        .expect("png header")
        .write_image_data(&rgba)
        .expect("png data");
}

fn smart() -> (Compositor, SharedFrame) {
    let frame = shared_frame(PANEL.0, PANEL.1);
    let mut c = Compositor::with_frame(frame.clone());
    c.set_panel_turn(PANEL, false).unwrap();
    (c, frame)
}

fn lit(px: &[u32]) -> usize {
    px.iter()
        .filter(|p| ((*p >> 16) & 0xff) + ((*p >> 8) & 0xff) + (*p & 0xff) > 30)
        .count()
}

fn shot(f: &mut Frontend, c: &mut Compositor, frame: &SharedFrame, name: &str) -> Vec<u32> {
    f.render(c, WINDOW);
    let raw = frame.borrow().px.clone();
    write_png(&format!("{name}-panel"), PANEL.0, PANEL.1, &raw);
    let up = upright(frame);
    write_png(name, WINDOW.0, WINDOW.1, &up);
    up
}

#[test]
fn the_shelf_fills_the_turned_panel() {
    let (mut c, frame) = smart();
    let d = tmp_root_with_carts(&["Emerald", "Zzz"]);
    clocked(d.path());
    let mut f = Frontend::boot(Box::new(SimPlatform::at(d.path().to_path_buf())));
    f.upload_faces(&mut c);
    f.upload_bezel(&mut c, WINDOW);
    let mut input = Script(VecDeque::new());
    for _ in 0..30 {
        f.advance(&mut input);
    }
    let up = shot(&mut f, &mut c, &frame, "trimui-shelf");
    assert_eq!(c.frame_size(), WINDOW);
    assert_eq!(frame.borrow().w, PANEL.0);
    let top = &up[..WINDOW.0 as usize];
    assert!(lit(top) > 300, "the top row is black: {} lit", lit(top));
    assert!(
        lit(&up) > 40_000,
        "the shelf is mostly black: {} lit",
        lit(&up)
    );
    let mb = c.texture_bytes() as f64 / (1 << 20) as f64;
    eprintln!("textures after the shelf: {mb:.1} MB");
    assert!(mb < 24.0, "the shelf's textures take {mb:.1} MB of 128");
}
