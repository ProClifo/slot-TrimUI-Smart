//! The compositor for panels with no GPU, such as the TrimUI Smart's. It takes the same draw
//! lists and calls as the GL one and reproduces its geometry: sprites blend source-over with
//! bilinear or nearest sampling on pixel centres, the game is sampled nearest into the same
//! rect, and the frame is blitted, warmed and turned onto the panel the same way. The screen
//! effects are the exception: the game is always drawn plain.

#[cfg(unix)]
mod fb;

use std::cell::RefCell;
use std::rc::Rc;

use crate::bezel::{framed, game_rect};
use crate::draw::{Draw, TexId};
use crate::grade::blue_light_gain;
use crate::lcd3x::ScreenEffect;
use crate::pipeline::{SRC_H, SRC_W, WHOLE_TEXTURE};
use crate::power::{screen_brightness, screen_rect_above};
use crate::surface::{blit_rect, GfxError, OUT_H, OUT_W};
use crate::{canvas_size, Canvas};

#[cfg(unix)]
pub use fb::FbdevSurface;

/// Panel pixels as 0x00RRGGBB, top row first, in the panel's own orientation.
pub struct Frame {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u32>,
}

pub type SharedFrame = Rc<RefCell<Frame>>;

pub fn shared_frame(w: u32, h: u32) -> SharedFrame {
    Rc::new(RefCell::new(Frame {
        w,
        h,
        px: vec![0; (w * h) as usize],
    }))
}

/// Texels packed as 0xAARRGGBB, straight alpha, so a blend works on whole words.
struct Texture {
    w: u32,
    h: u32,
    px: Vec<u32>,
    nearest: bool,
}

#[derive(Copy, Clone, Debug)]
struct Viewport {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

struct Game {
    /// The core's frame as 0x00RRGGBB.
    px: Vec<u32>,
    paper_size: f32,
    effect: ScreenEffect,
    power: f32,
    fills: bool,
    bottom: f32,
    src: [f32; 4],
    pic: [f32; 4],
}

impl Game {
    fn placed(&self) -> (f32, f32, f32, f32) {
        match self.fills {
            true => (0.0, 0.0, OUT_W as f32, OUT_H as f32 - self.bottom),
            false => screen_rect_above(self.power, self.bottom),
        }
    }

    fn bright(&self) -> f32 {
        match self.fills {
            true => 1.0,
            false => screen_brightness(self.power),
        }
    }
}

struct Turn {
    panel: (u32, u32),
    ccw: bool,
}

pub struct Compositor {
    canvas: Vec<u32>,
    size: (u32, u32),
    target: Canvas,
    textures: Vec<Texture>,
    shrink: u32,
    gain: [f32; 3],
    shake: f32,
    window: (u32, u32),
    game: Game,
    game_bezel: Option<TexId>,
    sp_frame: bool,
    turn: Option<Turn>,
    out: SharedFrame,
    scratch: Vec<u32>,
}

impl Compositor {
    #[cfg(unix)]
    pub fn new(surface: &FbdevSurface) -> Result<Self, GfxError> {
        Ok(Self::with_frame(surface.frame()))
    }

    /// Draws into `out`. Textures made for a canvas at least twice the panel's resolution are
    /// kept at half size, which is all the panel can show of them: slot always draws landscape,
    /// so a portrait panel is judged turned.
    pub fn with_frame(out: SharedFrame) -> Self {
        let panel = {
            let f = out.borrow();
            (f.w.max(f.h), f.w.min(f.h))
        };
        let shrink = match framed(panel) && panel.1 * 2 <= OUT_H {
            true => 2,
            false => 1,
        };
        Compositor {
            canvas: vec![0; (OUT_W * OUT_H) as usize],
            size: (OUT_W, OUT_H),
            target: Canvas::FRAMED,
            textures: Vec::new(),
            shrink,
            gain: blue_light_gain(0),
            shake: 0.0,
            window: (OUT_W, OUT_H),
            game: Game {
                px: vec![0; (SRC_W * SRC_H) as usize],
                paper_size: 1.0,
                effect: ScreenEffect::Lcd3x,
                power: 1.0,
                fills: false,
                bottom: 0.0,
                src: WHOLE_TEXTURE,
                pic: WHOLE_TEXTURE,
            },
            game_bezel: None,
            sp_frame: true,
            turn: None,
            out,
            scratch: Vec::new(),
        }
    }

    pub fn set_panel_turn(&mut self, panel: (u32, u32), ccw: bool) -> Result<(), GfxError> {
        self.turn = Some(Turn { panel, ccw });
        Ok(())
    }

    pub fn upload_game(&mut self, xrgb8888: &[u8]) {
        let n = (SRC_W * SRC_H * 4) as usize;
        if xrgb8888.len() >= n {
            for (o, p) in self.game.px.iter_mut().zip(xrgb8888.chunks_exact(4)) {
                *o = u32::from_le_bytes([p[0], p[1], p[2], 0]);
            }
        }
    }

    pub fn draw_game(&mut self) {
        let vp = self.game_viewport();
        self.game_pass(vp, None);
    }

    pub fn draw_list(&mut self, items: &[Draw]) {
        let mut from = 0;
        for (i, item) in items.iter().enumerate() {
            match *item {
                Draw::Game => {
                    self.sprites(&items[from..i]);
                    let vp = self.game_viewport();
                    self.game_pass(vp, None);
                    let panel_bezel = self.panel_bezel();
                    if panel_bezel.is_none() {
                        self.draw_game_bezel(vp);
                    }
                    if let Some(bezel) = panel_bezel {
                        self.sprites(&[bezel]);
                    }
                }
                Draw::Shot { tex } => {
                    self.sprites(&items[from..i]);
                    if tex.0 < self.textures.len() {
                        let vp = self.game_viewport();
                        self.game_pass(vp, Some(tex));
                    }
                }
                _ => continue,
            }
            from = i + 1;
        }
        self.sprites(&items[from..]);
    }

    pub fn create_texture(&mut self, w: u32, h: u32, rgba: &[u8]) -> TexId {
        self.push(w, h, rgba, false)
    }

    pub fn create_texture_nearest(&mut self, w: u32, h: u32, rgba: &[u8]) -> TexId {
        self.push(w, h, rgba, true)
    }

    fn push(&mut self, w: u32, h: u32, rgba: &[u8], nearest: bool) -> TexId {
        let tex = self.texture(w, h, rgba, nearest);
        self.textures.push(tex);
        TexId(self.textures.len() - 1)
    }

    fn texture(&self, w: u32, h: u32, rgba: &[u8], nearest: bool) -> Texture {
        let n = (w * h * 4) as usize;
        if w == 0 || h == 0 || rgba.len() < n {
            return Texture {
                w: 1,
                h: 1,
                px: vec![0],
                nearest,
            };
        }
        let px: Vec<u32> = rgba[..n]
            .chunks_exact(4)
            .map(|p| u32::from_le_bytes([p[2], p[1], p[0], p[3]]))
            .collect();
        if nearest || self.shrink == 1 || w < 2 || h < 2 {
            return Texture { w, h, px, nearest };
        }
        let (hw, hh) = (w.div_ceil(2), h.div_ceil(2));
        let mut half = vec![0u32; (hw * hh) as usize];
        let at = |x: u32, y: u32| px[(y * w + x) as usize];
        for y in 0..hh {
            let (y0, y1) = ((2 * y).min(h - 1), (2 * y + 1).min(h - 1));
            for x in 0..hw {
                let (x0, x1) = ((2 * x).min(w - 1), (2 * x + 1).min(w - 1));
                let quad = [at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1)];
                let mean = |shift: u32| {
                    let sum: u32 = quad.iter().map(|p| (p >> shift) & 0xff).sum();
                    ((sum + 2) / 4) << shift
                };
                half[(y * hw + x) as usize] = mean(24) | mean(16) | mean(8) | mean(0);
            }
        }
        Texture {
            w: hw,
            h: hh,
            px: half,
            nearest,
        }
    }

    pub fn update_texture(&mut self, id: TexId, w: u32, h: u32, rgba: &[u8]) {
        let Some(nearest) = self.textures.get(id.0).map(|t| t.nearest) else {
            return;
        };
        if rgba.len() < (w * h * 4) as usize {
            return;
        }
        self.textures[id.0] = self.texture(w, h, rgba, nearest);
    }

    pub fn set_blue_light(&mut self, step: u8) {
        self.gain = blue_light_gain(step);
    }

    pub fn set_screen_effect(&mut self, effect: ScreenEffect) {
        self.game.effect = effect;
    }

    pub fn set_picture(&mut self, rect: [f32; 4]) {
        self.game.pic = rect;
    }

    pub fn set_game_bezel(&mut self, tex: Option<TexId>) {
        self.game_bezel = tex;
    }

    pub fn set_sp_frame(&mut self, on: bool) {
        self.sp_frame = on;
    }

    pub fn set_paper(&mut self, size: u32, _gray: &[u8]) {
        self.game.paper_size = size as f32;
    }

    pub fn fit(&mut self, window: (u32, u32)) {
        self.window = window;
        self.resize(canvas_size(window));
    }

    fn native(&self) -> bool {
        framed(self.window)
    }

    fn resize(&mut self, size: (u32, u32)) {
        if size != self.size {
            self.size = size;
            self.canvas = vec![0; (size.0 * size.1) as usize];
        }
    }

    pub fn set_screen_power(&mut self, t: f32) {
        self.game.power = t.clamp(0.0, 1.0);
    }

    pub fn set_screen_footer(&mut self, px: f32) {
        self.game.bottom = px.max(0.0);
    }

    pub fn set_game_fills_frame(&mut self, on: bool) {
        self.game.fills = on;
    }

    pub fn set_game_source_rect(&mut self, rect: [f32; 4]) {
        self.game.src = rect;
    }

    pub fn set_shake(&mut self, dx: f32) {
        self.shake = dx;
    }

    pub fn frame_size(&self) -> (u32, u32) {
        self.size
    }

    /// The memory the textures hold, which the TrimUI Smart's 128 MB has to find room for.
    pub fn texture_bytes(&self) -> usize {
        self.textures.iter().map(|t| t.px.len() * 4).sum()
    }

    pub fn read_frame(&self) -> Vec<u8> {
        rgba_of(&self.canvas)
    }

    pub fn read_window(&mut self, window: (u32, u32)) -> Vec<u8> {
        let mut px = vec![0; (window.0 * window.1) as usize];
        self.output(window, &mut px);
        rgba_of(&px)
    }

    pub fn begin_frame(&mut self) {
        let size = match self.native() {
            true => self.window,
            false => canvas_size(self.window),
        };
        self.resize(size);
        self.target = Canvas::of(self.window);
        self.canvas.fill(0);
    }

    pub fn end_frame(&mut self, window: (u32, u32)) {
        let out = self.out.clone();
        let mut frame = out.borrow_mut();
        let Some(turn) = &self.turn else {
            frame.w = window.0;
            frame.h = window.1;
            frame.px.resize((window.0 * window.1) as usize, 0);
            self.output(window, &mut frame.px);
            return;
        };
        let (panel, ccw) = (turn.panel, turn.ccw);
        if self.native() && self.size == window && panel == (window.1, window.0) {
            frame.w = panel.0;
            frame.h = panel.1;
            frame.px.resize((panel.0 * panel.1) as usize, 0);
            self.turn_native(window, &mut frame.px, ccw);
            return;
        }
        let mut img = std::mem::take(&mut self.scratch);
        img.resize((window.0 * window.1) as usize, 0);
        self.output(window, &mut img);
        frame.w = panel.0;
        frame.h = panel.1;
        frame.px.resize((panel.0 * panel.1) as usize, 0);
        turn_onto(&img, window, &mut frame.px, panel, ccw);
        self.scratch = img;
    }

    /// The blit and the turn in one pass, for the usual case of a canvas the size of the
    /// window: each panel pixel reads the canvas pixel the turn lands on it, warmed and shaken.
    fn turn_native(&self, window: (u32, u32), out: &mut [u32], ccw: bool) {
        let (w, h) = (window.0 as usize, window.1 as usize);
        let canvas = Canvas::of(window);
        let dx = (self.shake * window.0 as f32 / canvas.w).round() as isize;
        let warm = Warm::new(self.gain);
        for (py, line) in out.chunks_exact_mut(h).enumerate() {
            let ix = match ccw {
                false => py,
                true => w - 1 - py,
            } as isize
                - dx;
            if ix < 0 || ix >= w as isize {
                line.fill(0);
                continue;
            }
            let column = &self.canvas[ix as usize..];
            for (px, o) in line.iter_mut().enumerate() {
                let iy = match ccw {
                    false => h - 1 - px,
                    true => px,
                };
                *o = warm.apply(column[iy * w]);
            }
        }
    }

    fn output(&self, window: (u32, u32), out: &mut [u32]) {
        out.fill(0);
        let warm = Warm::new(self.gain);
        let warm = |p: u32| warm.apply(p);
        let (cw, ch) = self.size;
        let (x, y, w, h) = match self.native() {
            true => {
                let canvas = Canvas::of(window);
                let dx = (self.shake * window.0 as f32 / canvas.w).round() as i32;
                (dx, 0, window.0 as i32, window.1 as i32)
            }
            false => {
                let (x, y, w, h) = blit_rect(window, self.shake);
                (x, window.1 as i32 - y - h, w, h)
            }
        };
        if w <= 0 || h <= 0 {
            return;
        }
        let cols = x.max(0)..(x + w).min(window.0 as i32);
        let xs: Vec<usize> = cols
            .clone()
            .map(|ox| ((ox - x) as i64 * cw as i64 / w as i64) as usize)
            .collect();
        for oy in y.max(0)..(y + h).min(window.1 as i32) {
            let sy = ((oy - y) as i64 * ch as i64 / h as i64) as u32;
            let row = &self.canvas[(sy * cw) as usize..][..cw as usize];
            let line = &mut out[(oy as u32 * window.0) as usize..][..window.0 as usize];
            for (ox, &sx) in cols.clone().zip(&xs) {
                line[ox as usize] = warm(row[sx]);
            }
        }
    }

    fn panel_bezel(&self) -> Option<Draw> {
        let tex = self.game_bezel.filter(|_| framed(self.window))?;
        let canvas = Canvas::of(self.window);
        Some(Draw::Tex {
            x: 0.0,
            y: 0.0,
            w: canvas.w,
            h: canvas.h,
            tex,
            alpha: 1.0,
        })
    }

    fn whole(&self) -> Viewport {
        Viewport {
            x: 0,
            y: 0,
            w: self.size.0 as i32,
            h: self.size.1 as i32,
        }
    }

    fn game_viewport(&self) -> Viewport {
        if !self.native() {
            return self.whole();
        }
        let (x, y, w, h) = game_rect(self.window, self.sp_frame);
        Viewport { x, y, w, h }
    }

    fn draw_game_bezel(&mut self, vp: Viewport) {
        let Some(tex) = self.game_bezel else {
            return;
        };
        let (x, y, w, h) = self.game.placed();
        let item = Draw::Tex {
            x,
            y,
            w,
            h,
            tex,
            alpha: 1.0,
        };
        self.sprites_in(vp, Canvas::FRAMED, &[item]);
    }

    fn sprites(&mut self, items: &[Draw]) {
        let (vp, target) = (self.whole(), self.target);
        self.sprites_in(vp, target, items);
    }

    fn sprites_in(&mut self, vp: Viewport, target: Canvas, items: &[Draw]) {
        let whole = WHOLE_TEXTURE;
        for item in items {
            let (rect, tex, colour, turn, uv) = match *item {
                Draw::Rect { x, y, w, h, colour } => ([x, y, w, h], None, colour, 0.0, whole),
                Draw::Tex {
                    x,
                    y,
                    w,
                    h,
                    tex,
                    alpha,
                } => ([x, y, w, h], Some(tex), [1.0, 1.0, 1.0, alpha], 0.0, whole),
                Draw::Part {
                    x,
                    y,
                    w,
                    h,
                    tex,
                    alpha,
                    uv,
                } => ([x, y, w, h], Some(tex), [1.0, 1.0, 1.0, alpha], 0.0, uv),
                Draw::Turned {
                    x,
                    y,
                    w,
                    h,
                    tex,
                    alpha,
                    turn,
                } => ([x, y, w, h], Some(tex), [1.0, 1.0, 1.0, alpha], turn, whole),
                Draw::Game | Draw::Shot { .. } => continue,
            };
            let tex = match tex {
                Some(id) => match self.textures.get(id.0) {
                    Some(t) => Some(t),
                    None => continue,
                },
                None => None,
            };
            let place = Place::new(vp, target, rect);
            match turn == 0.0 {
                true => sprite(&mut self.canvas, self.size, &place, tex, colour, uv),
                false => sprite_turned(&mut self.canvas, self.size, &place, tex, colour, turn),
            }
        }
    }

    fn game_pass(&mut self, vp: Viewport, still: Option<TexId>) {
        let (x, y, w, h) = self.game.placed();
        let mut src = match still {
            Some(_) => WHOLE_TEXTURE,
            None => self.game.src,
        };
        if self.game.bottom > 0.0 {
            src[3] *= (OUT_H as f32 - self.game.bottom) / OUT_H as f32;
        }
        let bright = (self.game.bright() * 256.0).round() as u32;
        let (pixels, tw, th): (&[u32], u32, u32) = match still {
            Some(id) => {
                let t = &self.textures[id.0];
                (&t.px, t.w, t.h)
            }
            None => (&self.game.px, SRC_W, SRC_H),
        };
        let place = Place::new(vp, Canvas::FRAMED, [x, y, w, h]);
        let Some((cols, rows)) = place.span(self.size) else {
            return;
        };
        let xs: Vec<usize> = cols
            .clone()
            .map(|i| nearest(src[0] + place.a_x(i) * src[2], tw))
            .collect();
        let stride = self.size.0 as usize;
        let lift = |p: u32| -> u32 {
            let c = |shift: u32| ((((p >> shift) & 0xff) * bright) >> 8).min(255) << shift;
            c(16) | c(8) | c(0)
        };
        for j in rows {
            let ty = nearest(src[1] + place.a_y(j) * src[3], th);
            let row = &pixels[ty * tw as usize..][..tw as usize];
            let line =
                &mut self.canvas[j as usize * stride..][cols.start as usize..cols.end as usize];
            match bright {
                256 => {
                    for (o, &tx) in line.iter_mut().zip(&xs) {
                        *o = row[tx] & 0xff_ffff;
                    }
                }
                _ => {
                    for (o, &tx) in line.iter_mut().zip(&xs) {
                        *o = lift(row[tx]);
                    }
                }
            }
        }
    }
}

/// Where a rect in `target` units lands among the canvas's pixels once the viewport scales it.
struct Place {
    vp: Viewport,
    sx: f32,
    sy: f32,
    rect: [f32; 4],
}

impl Place {
    fn new(vp: Viewport, target: Canvas, rect: [f32; 4]) -> Self {
        Place {
            vp,
            sx: vp.w as f32 / target.w,
            sy: vp.h as f32 / target.h,
            rect,
        }
    }

    fn a_x(&self, i: i32) -> f32 {
        let at = (i as f32 + 0.5 - self.vp.x as f32) / self.sx;
        (at - self.rect[0]) / self.rect[2]
    }

    fn a_y(&self, j: i32) -> f32 {
        let at = (j as f32 + 0.5 - self.vp.y as f32) / self.sy;
        (at - self.rect[1]) / self.rect[3]
    }

    /// The pixels whose centres fall inside the rect, the viewport and the canvas.
    fn span(&self, size: (u32, u32)) -> Option<(std::ops::Range<i32>, std::ops::Range<i32>)> {
        let [x, y, w, h] = self.rect;
        if w == 0.0 || h == 0.0 || !(w.is_finite() && h.is_finite()) {
            return None;
        }
        let (vx, vy) = (self.vp.x as f32, self.vp.y as f32);
        let (x0, x1) = (vx + x * self.sx, vx + (x + w) * self.sx);
        let (y0, y1) = (vy + y * self.sy, vy + (y + h) * self.sy);
        let cols = covered(x0.min(x1), x0.max(x1), self.vp.x, self.vp.w, size.0);
        let rows = covered(y0.min(y1), y0.max(y1), self.vp.y, self.vp.h, size.1);
        (!cols.is_empty() && !rows.is_empty()).then_some((cols, rows))
    }
}

fn covered(lo: f32, hi: f32, v: i32, vlen: i32, size: u32) -> std::ops::Range<i32> {
    let first = (lo - 0.5).ceil().max(v as f32).max(0.0) as i32;
    let end = (hi - 0.5).ceil().min((v + vlen) as f32).min(size as f32) as i32;
    first..end.max(first)
}

fn nearest(u: f32, len: u32) -> usize {
    ((u * len as f32).floor() as i64).clamp(0, len as i64 - 1) as usize
}

/// One axis of a bilinear (or nearest) lookup: the two texels and the weight of the second.
#[derive(Copy, Clone)]
struct Tap {
    a: usize,
    b: usize,
    t: u32,
}

fn tap(u: f32, len: u32, nearest_only: bool) -> Tap {
    if nearest_only {
        let a = nearest(u, len);
        return Tap { a, b: a, t: 0 };
    }
    let f = u * len as f32 - 0.5;
    let base = f.floor();
    let t = ((f - base) * 256.0).round() as u32;
    let last = len as i64 - 1;
    let a = (base as i64).clamp(0, last) as usize;
    let b = (base as i64 + 1).clamp(0, last) as usize;
    match t {
        256 => Tap { a: b, b, t: 0 },
        _ => Tap { a, b, t },
    }
}

/// The blue-light warmth as per-channel multipliers out of 256, or none at all.
struct Warm {
    gain: Option<[u32; 3]>,
}

impl Warm {
    fn new(gain: [f32; 3]) -> Self {
        let gain = gain.map(|g| (g.clamp(0.0, 1.0) * 256.0).round() as u32);
        Warm {
            gain: (gain != [256; 3]).then_some(gain),
        }
    }

    #[inline(always)]
    fn apply(&self, p: u32) -> u32 {
        let Some(k) = self.gain else {
            return p;
        };
        let r = (((p >> 16) & 0xff) * k[0]) >> 8;
        let g = (((p >> 8) & 0xff) * k[1]) >> 8;
        let b = ((p & 0xff) * k[2]) >> 8;
        (r << 16) | (g << 8) | b
    }
}

/// `p` toward `q` by `t` out of 256, all four channels at once.
#[inline(always)]
fn lerp(p: u32, q: u32, t: u32) -> u32 {
    if t == 0 {
        return p;
    }
    let s = 256 - t;
    let rb = (((p & 0xff00ff) * s + (q & 0xff00ff) * t) >> 8) & 0xff00ff;
    let ag = (((p >> 8) & 0xff00ff) * s + ((q >> 8) & 0xff00ff) * t) & 0xff00ff00;
    rb | ag
}

#[inline(always)]
fn sample(tex: &Texture, x: Tap, y: Tap) -> u32 {
    let w = tex.w as usize;
    let at = |tx: usize, ty: usize| tex.px[ty * w + tx];
    let top = lerp(at(x.a, y.a), at(x.b, y.a), x.t);
    match y.t {
        0 => top,
        t => lerp(top, lerp(at(x.a, y.b), at(x.b, y.b), x.t), t),
    }
}

/// Blends the colour of `src` over `dst` at alpha `a` out of 255, as SRC_ALPHA,
/// ONE_MINUS_SRC_ALPHA does, two channels to a multiply.
#[inline(always)]
fn over(dst: u32, src: u32, a: u32) -> u32 {
    match a {
        0 => dst,
        255.. => src & 0xff_ffff,
        _ => {
            let ia = 255 - a;
            let rb = (src & 0xff00ff) * a + (dst & 0xff00ff) * ia + 0x0080_0080;
            let g = (src & 0xff00) * a + (dst & 0xff00) * ia + 0x8000;
            let rb = ((rb + ((rb >> 8) & 0xff00ff)) >> 8) & 0xff00ff;
            let g = ((g + ((g >> 8) & 0xff00)) >> 8) & 0xff00;
            rb | g
        }
    }
}

/// A texel's alpha once the draw's own alpha, out of 256, scales it.
#[inline(always)]
fn alpha_of(p: u32, k: u32) -> u32 {
    match k {
        256 => p >> 24,
        _ => ((p >> 24) * k + 128) >> 8,
    }
}

fn colour_factors(colour: [f32; 4]) -> [u32; 4] {
    colour.map(|c| (c.clamp(0.0, 1.0) * 256.0).round() as u32)
}

/// A flat colour's texel: white scaled by the colour, with its alpha out of 255.
fn flat(k: [u32; 4]) -> u32 {
    let c = |i: usize| (255 * k[i]) >> 8;
    (c(3) << 24) | (c(0) << 16) | (c(1) << 8) | c(2)
}

fn sprite(
    canvas: &mut [u32],
    size: (u32, u32),
    place: &Place,
    tex: Option<&Texture>,
    colour: [f32; 4],
    uv: [f32; 4],
) {
    let Some((cols, rows)) = place.span(size) else {
        return;
    };
    let k = colour_factors(colour);
    let stride = size.0 as usize;
    let span = cols.start as usize..cols.end as usize;
    let Some(tex) = tex else {
        let src = flat(k);
        let a = src >> 24;
        for j in rows {
            for px in &mut canvas[j as usize * stride..][span.clone()] {
                *px = over(*px, src, a);
            }
        }
        return;
    };
    let ka = k[3];
    let xs: Vec<Tap> = cols
        .clone()
        .map(|i| tap(uv[0] + place.a_x(i) * uv[2], tex.w, tex.nearest))
        .collect();
    // Texel for pixel, one column after the next: the shelf's art mostly lands like this once
    // its textures are kept at the panel's scale, and then a row is a straight run of texels.
    let unit = xs
        .iter()
        .enumerate()
        .all(|(n, x)| x.t == 0 && x.a == xs[0].a + n);
    let w = tex.w as usize;
    for j in rows {
        let ty = tap(uv[1] + place.a_y(j) * uv[3], tex.h, tex.nearest);
        let line = &mut canvas[j as usize * stride..][span.clone()];
        if unit && ty.t == 0 {
            let run = &tex.px[ty.a * w + xs[0].a..][..line.len()];
            for (d, &p) in line.iter_mut().zip(run) {
                *d = over(*d, p, alpha_of(p, ka));
            }
            continue;
        }
        let (row_a, row_b) = (&tex.px[ty.a * w..][..w], &tex.px[ty.b * w..][..w]);
        for (d, x) in line.iter_mut().zip(&xs) {
            let top = lerp(row_a[x.a], row_a[x.b], x.t);
            let p = match ty.t {
                0 => top,
                t => lerp(top, lerp(row_b[x.a], row_b[x.b], x.t), t),
            };
            *d = over(*d, p, alpha_of(p, ka));
        }
    }
}

fn sprite_turned(
    canvas: &mut [u32],
    size: (u32, u32),
    place: &Place,
    tex: Option<&Texture>,
    colour: [f32; 4],
    turn: f32,
) {
    let [x, y, w, h] = place.rect;
    if w == 0.0 || h == 0.0 {
        return;
    }
    let (cos, sin) = (turn.cos(), turn.sin());
    let (sx, sy) = (place.sx, place.sy);
    let (vx, vy) = (place.vp.x as f32, place.vp.y as f32);
    let (mx, my) = (x + w / 2.0, y + h / 2.0);
    let corners = [(-0.5f32, -0.5f32), (0.5, -0.5), (-0.5, 0.5), (0.5, 0.5)].map(|(ax, ay)| {
        let (lx, ly) = (ax * w, ay * h);
        (
            vx + (mx + cos * lx - sin * ly) * sx,
            vy + (my + sin * lx + cos * ly) * sy,
        )
    });
    let lo_x = corners.iter().map(|c| c.0).fold(f32::INFINITY, f32::min);
    let hi_x = corners
        .iter()
        .map(|c| c.0)
        .fold(f32::NEG_INFINITY, f32::max);
    let lo_y = corners.iter().map(|c| c.1).fold(f32::INFINITY, f32::min);
    let hi_y = corners
        .iter()
        .map(|c| c.1)
        .fold(f32::NEG_INFINITY, f32::max);
    let cols = covered(lo_x, hi_x, place.vp.x, place.vp.w, size.0);
    let rows = covered(lo_y, hi_y, place.vp.y, place.vp.h, size.1);
    let k = colour_factors(colour);
    let stride = size.0 as usize;
    for j in rows {
        let py = (j as f32 + 0.5 - vy) / sy - my;
        let line = &mut canvas[j as usize * stride..][..stride];
        for i in cols.clone() {
            let px = (i as f32 + 0.5 - vx) / sx - mx;
            let (lx, ly) = (cos * px + sin * py, -sin * px + cos * py);
            let (ax, ay) = (lx / w + 0.5, ly / h + 0.5);
            if !(0.0..=1.0).contains(&ax) || !(0.0..=1.0).contains(&ay) {
                continue;
            }
            let p = match tex {
                Some(t) => sample(t, tap(ax, t.w, t.nearest), tap(ay, t.h, t.nearest)),
                None => flat([k[0], k[1], k[2], 256]),
            };
            let d = &mut line[i as usize];
            *d = over(*d, p, alpha_of(p, k[3]));
        }
    }
}

/// Lays the landscape `img` onto the portrait `panel`, a quarter turn either way, as the GL
/// compositor's turn pass does: clockwise puts the picture's top along the panel's right edge.
fn turn_onto(img: &[u32], window: (u32, u32), out: &mut [u32], panel: (u32, u32), ccw: bool) {
    let (w, h) = (window.0 as usize, window.1 as usize);
    let (pw, ph) = (panel.0 as usize, panel.1 as usize);
    if pw != h || ph != w {
        out.fill(0);
        return;
    }
    for py in 0..ph {
        let line = &mut out[py * pw..][..pw];
        for (px, o) in line.iter_mut().enumerate() {
            let (ix, iy) = match ccw {
                false => (py, h - 1 - px),
                true => (w - 1 - py, px),
            };
            *o = img[iy * w + ix];
        }
    }
}

fn rgba_of(px: &[u32]) -> Vec<u8> {
    px.iter()
        .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8, 0xff])
        .collect()
}
