#![cfg_attr(feature = "soft", allow(dead_code))]

mod bezel;
mod draw;
mod fbdev;
mod fbo;
mod gl;
mod grade;
#[cfg(target_os = "macos")]
mod headless;
#[cfg(feature = "host")]
mod host;
mod layout;
mod lcd3x;
mod pipeline;
mod power;
mod quad;
mod shaders;
pub mod soft;
mod surface;

pub use bezel::{framed, game_rect, lifted_rect, screen_rect};
pub use draw::{Draw, TexId};
#[cfg(not(feature = "soft"))]
pub use fbdev::FbdevSurface;
pub use fbdev::{egl_error, panel_mode, panel_size};
#[cfg(not(feature = "soft"))]
pub use fbo::Compositor;
pub use fbo::{canvas_size, BACKDROP};
pub use grade::{blue_light_gain, BLUE_LIGHT_MAX};
#[cfg(target_os = "macos")]
pub use headless::HeadlessSurface;
#[cfg(feature = "host")]
pub use host::HostSurface;
pub use layout::Canvas;
pub use lcd3x::{lcd3x_factors, lcd3x_mask, ScreenEffect};
pub use pipeline::{SCALE, SRC_H, SRC_W, WHOLE_TEXTURE};
pub use power::{screen_brightness, screen_rect_above, screen_scale, screen_width};
#[cfg(feature = "soft")]
pub use soft::{Compositor, FbdevSurface};
pub use surface::{blit_rect, blit_rect_fit, fit_rect, fit_scale, GfxError, Surface, OUT_H, OUT_W};
