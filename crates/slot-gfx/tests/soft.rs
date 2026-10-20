use slot_gfx::soft::{shared_frame, Compositor, SharedFrame};
use slot_gfx::{Draw, SRC_H, SRC_W};

const SMART: (u32, u32) = (240, 320);
const WINDOW: (u32, u32) = (320, 240);

fn smart() -> (Compositor, SharedFrame) {
    let frame = shared_frame(SMART.0, SMART.1);
    let mut c = Compositor::with_frame(frame.clone());
    c.set_panel_turn(SMART, false).unwrap();
    (c, frame)
}

fn frame_of(c: &mut Compositor, items: &[Draw]) {
    c.fit(WINDOW);
    c.begin_frame();
    c.draw_list(items);
    c.end_frame(WINDOW);
}

fn solid(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
    rgba.repeat((w * h) as usize)
}

fn at(frame: &SharedFrame, x: u32, y: u32) -> u32 {
    let f = frame.borrow();
    f.px[(y * f.w + x) as usize]
}

/// The picture's pixel (x, y) once turned clockwise onto the portrait panel.
fn shown(frame: &SharedFrame, x: u32, y: u32) -> u32 {
    at(frame, WINDOW.1 - 1 - y, x)
}

#[test]
fn a_clockwise_turn_puts_the_top_left_corner_at_the_panels_top_right() {
    let (mut c, frame) = smart();
    let red = c.create_texture_nearest(1, 1, &[255, 0, 0, 255]);
    frame_of(
        &mut c,
        &[Draw::Tex {
            x: 0.0,
            y: 0.0,
            w: 2.0,
            h: 2.0,
            tex: red,
            alpha: 1.0,
        }],
    );
    assert_eq!(at(&frame, SMART.0 - 1, 0), 0xff0000);
    assert_eq!(at(&frame, 0, 0), 0);
    assert_eq!(frame.borrow().w, SMART.0);
}

#[test]
fn a_counter_clockwise_turn_puts_it_at_the_bottom_left() {
    let frame = shared_frame(SMART.0, SMART.1);
    let mut c = Compositor::with_frame(frame.clone());
    c.set_panel_turn(SMART, true).unwrap();
    let red = c.create_texture_nearest(1, 1, &[255, 0, 0, 255]);
    frame_of(
        &mut c,
        &[Draw::Tex {
            x: 0.0,
            y: 0.0,
            w: 2.0,
            h: 2.0,
            tex: red,
            alpha: 1.0,
        }],
    );
    assert_eq!(at(&frame, 0, SMART.1 - 1), 0xff0000);
}

#[test]
fn the_canvas_is_twice_the_panel_so_a_logical_rect_covers_half_as_many_pixels() {
    let (mut c, frame) = smart();
    frame_of(
        &mut c,
        &[Draw::Rect {
            x: 20.0,
            y: 10.0,
            w: 40.0,
            h: 20.0,
            colour: [0.0, 1.0, 0.0, 1.0],
        }],
    );
    assert_eq!(shown(&frame, 10, 5), 0x00ff00);
    assert_eq!(shown(&frame, 29, 14), 0x00ff00);
    assert_eq!(shown(&frame, 30, 14), 0);
    assert_eq!(shown(&frame, 9, 5), 0);
    assert_eq!(shown(&frame, 10, 15), 0);
}

#[test]
fn half_alpha_blends_source_over() {
    let (mut c, frame) = smart();
    frame_of(
        &mut c,
        &[
            Draw::Rect {
                x: 0.0,
                y: 0.0,
                w: 640.0,
                h: 480.0,
                colour: [1.0, 1.0, 1.0, 1.0],
            },
            Draw::Rect {
                x: 0.0,
                y: 0.0,
                w: 640.0,
                h: 480.0,
                colour: [0.0, 0.0, 0.0, 0.5],
            },
        ],
    );
    let p = shown(&frame, 100, 100);
    for c in [p >> 16, (p >> 8) & 0xff, p & 0xff] {
        assert!((126..=129).contains(&c), "{p:06x}");
    }
}

#[test]
fn a_canvas_sized_texture_is_kept_at_half_size_and_shown_one_to_one() {
    let (mut c, frame) = smart();
    let mut rgba = Vec::new();
    for y in 0..480u32 {
        for x in 0..640u32 {
            let lit = (x / 2 + y / 2) % 2 == 0;
            rgba.extend_from_slice(if lit {
                &[200, 100, 50, 255]
            } else {
                &[0, 0, 0, 255]
            });
        }
    }
    let tex = c.create_texture(640, 480, &rgba);
    frame_of(
        &mut c,
        &[Draw::Tex {
            x: 0.0,
            y: 0.0,
            w: 640.0,
            h: 480.0,
            tex,
            alpha: 1.0,
        }],
    );
    assert_eq!(shown(&frame, 0, 0), 0xc86432);
    assert_eq!(shown(&frame, 1, 0), 0);
    assert_eq!(shown(&frame, 1, 1), 0xc86432);
    assert_eq!(shown(&frame, 319, 239), 0xc86432);
}

#[test]
fn a_transparent_texture_leaves_what_is_under_it() {
    let (mut c, frame) = smart();
    let clear = c.create_texture(8, 8, &solid(8, 8, [255, 255, 255, 0]));
    frame_of(
        &mut c,
        &[
            Draw::Rect {
                x: 0.0,
                y: 0.0,
                w: 640.0,
                h: 480.0,
                colour: [0.0, 0.0, 1.0, 1.0],
            },
            Draw::Tex {
                x: 0.0,
                y: 0.0,
                w: 640.0,
                h: 480.0,
                tex: clear,
                alpha: 1.0,
            },
        ],
    );
    assert_eq!(shown(&frame, 50, 50), 0x0000ff);
}

#[test]
fn a_part_shows_only_its_corner_of_the_texture() {
    let (mut c, frame) = smart();
    let mut rgba = solid(2, 2, [0, 0, 0, 255]);
    rgba[4..8].copy_from_slice(&[255, 0, 0, 255]);
    let tex = c.create_texture_nearest(2, 2, &rgba);
    frame_of(
        &mut c,
        &[Draw::Part {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
            tex,
            alpha: 1.0,
            uv: [0.5, 0.0, 0.5, 0.5],
        }],
    );
    assert_eq!(shown(&frame, 25, 25), 0xff0000);
    assert_eq!(shown(&frame, 0, 49), 0xff0000);
}

#[test]
fn a_half_turn_flips_the_sprite_about_its_centre() {
    let (mut c, frame) = smart();
    let mut rgba = solid(2, 1, [0, 0, 255, 255]);
    rgba[..4].copy_from_slice(&[255, 0, 0, 255]);
    let tex = c.create_texture_nearest(2, 1, &rgba);
    frame_of(
        &mut c,
        &[Draw::Turned {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 100.0,
            tex,
            alpha: 1.0,
            turn: std::f32::consts::PI,
        }],
    );
    assert_eq!(shown(&frame, 10, 25), 0x0000ff);
    assert_eq!(shown(&frame, 90, 25), 0xff0000);
}

#[test]
fn the_game_fills_the_sp_frame_from_the_top_nearest_sampled() {
    let (mut c, frame) = smart();
    let mut game = vec![0u8; (SRC_W * SRC_H * 4) as usize];
    for px in game.chunks_exact_mut(4).take(SRC_W as usize) {
        px.copy_from_slice(&[0x10, 0x20, 0x30, 0xff]);
    }
    c.upload_game(&game);
    c.set_screen_power(1.0);
    c.set_game_fills_frame(true);
    frame_of(&mut c, &[Draw::Game]);
    assert_eq!(shown(&frame, 0, 0), 0x302010, "bgra in, rgb out");
    assert_eq!(shown(&frame, 319, 0), 0x302010);
    assert_eq!(
        shown(&frame, 0, 1),
        0,
        "pixel row 1 samples source row 1.13"
    );
    let (_, top, w, h) = slot_gfx::game_rect(WINDOW, true);
    assert_eq!((top, w, h), (0, 320, 213));
    assert_eq!(shown(&frame, 0, h as u32), 0, "below the game rect");
}

#[test]
fn the_warm_screen_dims_blue_last() {
    let (mut c, frame) = smart();
    c.set_blue_light(slot_gfx::BLUE_LIGHT_MAX);
    frame_of(
        &mut c,
        &[Draw::Rect {
            x: 0.0,
            y: 0.0,
            w: 640.0,
            h: 480.0,
            colour: [1.0, 1.0, 1.0, 1.0],
        }],
    );
    let p = shown(&frame, 5, 5);
    assert_eq!(p >> 16, 255);
    assert!((p & 0xff) < ((p >> 8) & 0xff));
}

#[test]
fn read_frame_returns_the_canvas_top_down() {
    let (mut c, _) = smart();
    frame_of(
        &mut c,
        &[Draw::Rect {
            x: 0.0,
            y: 0.0,
            w: 640.0,
            h: 2.0,
            colour: [1.0, 0.0, 0.0, 1.0],
        }],
    );
    assert_eq!(c.frame_size(), WINDOW);
    let rgba = c.read_frame();
    assert_eq!(&rgba[..4], &[255, 0, 0, 255]);
    assert_eq!(&rgba[320 * 4..320 * 4 + 4], &[0, 0, 0, 255]);
}
