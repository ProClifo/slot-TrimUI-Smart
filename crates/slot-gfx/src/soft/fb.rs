//! /dev/fb0 as the soft compositor's surface: the frame goes into whichever page of the
//! framebuffer is hidden, then the panel is panned onto it.

use std::ffi::{c_ulong, c_void};
use std::os::unix::io::AsRawFd;
use std::time::{Duration, Instant};

use super::{shared_frame, SharedFrame};
use crate::surface::{GfxError, Surface};

const FB0: &str = "/dev/fb0";

const FBIOGET_VSCREENINFO: c_ulong = 0x4600;
const FBIOGET_FSCREENINFO: c_ulong = 0x4602;
const FBIOPAN_DISPLAY: c_ulong = 0x4606;
const FBIO_WAITFORVSYNC: c_ulong = 0x4004_4620;

/// Swaps it takes to learn whether panning already waits for the panel.
const PROBE_SWAPS: u32 = 8;

/// A pan this slow is taken to have waited for the vertical blank itself.
const BLOCKING_PAN: Duration = Duration::from_millis(4);

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Bitfield {
    offset: u32,
    length: u32,
    msb_right: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct VarInfo {
    xres: u32,
    yres: u32,
    xres_virtual: u32,
    yres_virtual: u32,
    xoffset: u32,
    yoffset: u32,
    bits_per_pixel: u32,
    grayscale: u32,
    red: Bitfield,
    green: Bitfield,
    blue: Bitfield,
    transp: Bitfield,
    nonstd: u32,
    activate: u32,
    height: u32,
    width: u32,
    accel_flags: u32,
    pixclock: u32,
    left_margin: u32,
    right_margin: u32,
    upper_margin: u32,
    lower_margin: u32,
    hsync_len: u32,
    vsync_len: u32,
    sync: u32,
    vmode: u32,
    rotate: u32,
    colorspace: u32,
    reserved: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FixInfo {
    id: [u8; 16],
    smem_start: c_ulong,
    smem_len: u32,
    kind: u32,
    type_aux: u32,
    visual: u32,
    xpanstep: u16,
    ypanstep: u16,
    ywrapstep: u16,
    line_length: u32,
    mmio_start: c_ulong,
    mmio_len: u32,
    accel: u32,
    capabilities: u16,
    reserved: [u16; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Vsync {
    Probing,
    Pan,
    Wait,
    None,
}

pub struct FbdevSurface {
    file: std::fs::File,
    map: *mut u8,
    map_len: usize,
    var: VarInfo,
    stride: usize,
    pages: u32,
    page: u32,
    frame: SharedFrame,
    vsync: Vsync,
    swaps: u32,
    pan_time: Duration,
    wait_time: Duration,
    wait_ok: bool,
}

fn ioctl<T>(fd: i32, request: c_ulong, arg: *mut T) -> bool {
    unsafe { libc::ioctl(fd, request as _, arg as *mut c_void) == 0 }
}

impl FbdevSurface {
    pub fn new() -> Result<Self, GfxError> {
        let fail = |what: &str| {
            GfxError::Context(format!(
                "{FB0}: {what}: {}",
                std::io::Error::last_os_error()
            ))
        };
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(FB0)
            .map_err(|e| GfxError::Context(format!("{FB0}: {e}")))?;
        let fd = file.as_raw_fd();
        let mut var = VarInfo::default();
        if !ioctl(fd, FBIOGET_VSCREENINFO, &mut var) {
            return Err(fail("FBIOGET_VSCREENINFO"));
        }
        let mut fix: FixInfo = unsafe { std::mem::zeroed() };
        if !ioctl(fd, FBIOGET_FSCREENINFO, &mut fix) {
            return Err(fail("FBIOGET_FSCREENINFO"));
        }
        if var.bits_per_pixel != 16 && var.bits_per_pixel != 32 {
            return Err(GfxError::Context(format!(
                "{FB0}: {} bits per pixel, only 16 and 32 are drawn",
                var.bits_per_pixel
            )));
        }
        let stride = fix.line_length as usize;
        let pages = match var.yres_virtual >= var.yres * 2 && fix.ypanstep > 0 {
            true => 2,
            false => 1,
        };
        let map_len = (fix.smem_len as usize).min(stride * var.yres_virtual as usize);
        let needed = stride * (var.yres * pages) as usize;
        if map_len < needed || var.xres == 0 || var.yres == 0 {
            return Err(GfxError::Context(format!(
                "{FB0}: {map_len} bytes mapped, {needed} needed"
            )));
        }
        let map = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                map_len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        if map == libc::MAP_FAILED {
            return Err(fail("mmap"));
        }
        eprintln!(
            "slot: fb0 {}x{} {} bpp, stride {stride}, virtual {}x{}, {pages} page(s), rgb offsets {}/{}/{}",
            var.xres,
            var.yres,
            var.bits_per_pixel,
            var.xres_virtual,
            var.yres_virtual,
            var.red.offset,
            var.green.offset,
            var.blue.offset
        );
        let page = var.yoffset / var.yres.max(1) % pages;
        let vsync = match std::env::var("SLOT_FB_VSYNC").as_deref() {
            Ok("pan") => Vsync::Pan,
            Ok("wait") => Vsync::Wait,
            Ok("none") => Vsync::None,
            _ => Vsync::Probing,
        };
        Ok(FbdevSurface {
            file,
            map: map as *mut u8,
            map_len,
            var,
            stride,
            pages,
            page,
            frame: shared_frame(var.xres, var.yres),
            vsync,
            swaps: 0,
            pan_time: Duration::ZERO,
            wait_time: Duration::ZERO,
            wait_ok: true,
        })
    }

    pub fn frame(&self) -> SharedFrame {
        self.frame.clone()
    }

    fn write_page(&mut self, page: u32) {
        let frame = self.frame.borrow();
        let (w, h) = (self.var.xres as usize, self.var.yres as usize);
        if frame.w as usize != w || frame.h as usize != h {
            return;
        }
        let base = self.stride * h * page as usize;
        let (r, g, b) = (self.var.red, self.var.green, self.var.blue);
        for y in 0..h {
            let src = &frame.px[y * w..][..w];
            let at = base + y * self.stride;
            match self.var.bits_per_pixel {
                16 => {
                    let line =
                        unsafe { std::slice::from_raw_parts_mut(self.map.add(at) as *mut u16, w) };
                    for (o, &p) in line.iter_mut().zip(src) {
                        *o = pack(p, r, g, b) as u16;
                    }
                }
                _ => {
                    let line =
                        unsafe { std::slice::from_raw_parts_mut(self.map.add(at) as *mut u32, w) };
                    for (o, &p) in line.iter_mut().zip(src) {
                        *o = pack(p, r, g, b) | 0xff00_0000;
                    }
                }
            }
        }
    }

    fn pan(&mut self, page: u32) -> bool {
        let mut var = self.var;
        var.xoffset = 0;
        var.yoffset = page * self.var.yres;
        ioctl(self.file.as_raw_fd(), FBIOPAN_DISPLAY, &mut var)
    }

    fn wait(&mut self) -> bool {
        let mut zero: u32 = 0;
        ioctl(self.file.as_raw_fd(), FBIO_WAITFORVSYNC, &mut zero)
    }

    /// Learns, over the first few swaps, whether a pan waits for the panel on its own. If it
    /// does, waiting as well would cost a whole frame; if not, FBIO_WAITFORVSYNC paces it.
    fn settle(&mut self) {
        if self.vsync != Vsync::Probing || self.swaps < PROBE_SWAPS {
            return;
        }
        let pan = self.pan_time / PROBE_SWAPS;
        let wait = self.wait_time / PROBE_SWAPS;
        self.vsync = if pan >= BLOCKING_PAN {
            Vsync::Pan
        } else if self.wait_ok && wait >= Duration::from_millis(1) {
            Vsync::Wait
        } else {
            Vsync::None
        };
        eprintln!(
            "slot: fb0 pan {:.2} ms, vsync wait {:.2} ms ({}), pacing by {:?}",
            pan.as_secs_f64() * 1e3,
            wait.as_secs_f64() * 1e3,
            if self.wait_ok { "works" } else { "refused" },
            self.vsync
        );
    }
}

fn pack(p: u32, r: Bitfield, g: Bitfield, b: Bitfield) -> u32 {
    let channel = |v: u32, f: Bitfield| (v >> (8 - f.length.min(8))) << f.offset;
    channel((p >> 16) & 0xff, r) | channel((p >> 8) & 0xff, g) | channel(p & 0xff, b)
}

impl Surface for FbdevSurface {
    fn make_current(&mut self) -> Result<(), GfxError> {
        Ok(())
    }

    fn window_size(&self) -> (u32, u32) {
        (self.var.xres, self.var.yres)
    }

    fn swap(&mut self) -> Result<(), GfxError> {
        let next = (self.page + 1) % self.pages;
        self.write_page(next);
        if self.pages > 1 {
            let began = Instant::now();
            if !self.pan(next) {
                return Err(GfxError::Context(format!(
                    "{FB0}: FBIOPAN_DISPLAY: {}",
                    std::io::Error::last_os_error()
                )));
            }
            self.pan_time += began.elapsed();
        }
        self.page = next;
        match self.vsync {
            Vsync::Wait => {
                self.wait();
            }
            Vsync::Probing => {
                let began = Instant::now();
                self.wait_ok &= self.wait();
                self.wait_time += began.elapsed();
            }
            Vsync::Pan | Vsync::None => {}
        }
        self.swaps += 1;
        self.settle();
        Ok(())
    }

    fn proc_address(&self, _name: &str) -> *const c_void {
        std::ptr::null()
    }
}

impl Drop for FbdevSurface {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.map as *mut c_void, self.map_len) };
    }
}
