//! Windows presenter for translucent windows. softbuffer's Win32 backend drops the alpha channel
//! (its `Premultiplied` mode isn't offered there), so these windows are blitted here: a top-down
//! 32-bit DIB section whose header declares an alpha mask, copied to the window with `BitBlt`.
//! winit makes the window transparent (`DwmEnableBlurBehindWindow` with an empty region), and DWM
//! then composites the premultiplied pixels over the desktop. Written without a Windows compiler to
//! hand: CI compiles it.

use std::{ffi::c_void, mem, ptr};

use windows_sys::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject, BITMAPINFO, BITMAPV5HEADER,
    BI_BITFIELDS, DIB_RGB_COLORS, SRCCOPY,
};

pub struct Presenter {
    dc: isize,
    bitmap: isize,
    old: isize,
    bits: *mut u8,
    size: (u32, u32),
}

impl Default for Presenter {
    fn default() -> Self {
        Presenter { dc: 0, bitmap: 0, old: 0, bits: ptr::null_mut(), size: (0, 0) }
    }
}

impl Presenter {
    fn release(&mut self) {
        // SAFETY: the handles were made by `resize` and are freed once.
        unsafe {
            if self.dc != 0 {
                SelectObject(self.dc, self.old);
                DeleteObject(self.bitmap);
                DeleteDC(self.dc);
            }
        }
        // Field by field: assigning a whole new `Presenter` would drop the old one, and `Drop` calls this.
        self.dc = 0;
        self.bitmap = 0;
        self.old = 0;
        self.bits = ptr::null_mut();
        self.size = (0, 0);
    }

    fn resize(&mut self, hwnd: isize, w: u32, h: u32) -> bool {
        if self.size == (w, h) && self.dc != 0 {
            return true;
        }
        self.release();
        // SAFETY: plain GDI calls on a window of this process; failures leave the presenter empty.
        unsafe {
            let window_dc = GetDC(hwnd);
            if window_dc == 0 {
                return false;
            }
            let mut header: BITMAPV5HEADER = mem::zeroed();
            header.bV5Size = mem::size_of::<BITMAPV5HEADER>() as u32;
            header.bV5Width = w as i32;
            header.bV5Height = -(h as i32); // top-down
            header.bV5Planes = 1;
            header.bV5BitCount = 32;
            header.bV5Compression = BI_BITFIELDS;
            header.bV5RedMask = 0x00ff_0000;
            header.bV5GreenMask = 0x0000_ff00;
            header.bV5BlueMask = 0x0000_00ff;
            header.bV5AlphaMask = 0xff00_0000;
            let mut bits: *mut c_void = ptr::null_mut();
            let bitmap = CreateDIBSection(window_dc, &header as *const BITMAPV5HEADER as *const BITMAPINFO, DIB_RGB_COLORS, &mut bits, 0, 0);
            let dc = CreateCompatibleDC(window_dc);
            ReleaseDC(hwnd, window_dc);
            if bitmap == 0 || dc == 0 || bits.is_null() {
                if bitmap != 0 {
                    DeleteObject(bitmap);
                }
                if dc != 0 {
                    DeleteDC(dc);
                }
                return false;
            }
            self.old = SelectObject(dc, bitmap);
            self.dc = dc;
            self.bitmap = bitmap;
            self.bits = bits as *mut u8;
            self.size = (w, h);
        }
        true
    }

    /// Paint one frame (`paint` fills `w*h*4` BGRA premultiplied bytes) and show it.
    pub fn present(&mut self, hwnd: isize, w: u32, h: u32, paint: impl FnOnce(&mut [u8])) {
        if !self.resize(hwnd, w, h) {
            return;
        }
        // SAFETY: `bits` points at the DIB section's `w*h*4` bytes, alive until `release`.
        unsafe {
            paint(std::slice::from_raw_parts_mut(self.bits, w as usize * h as usize * 4));
            let window_dc = GetDC(hwnd);
            if window_dc != 0 {
                BitBlt(window_dc, 0, 0, w as i32, h as i32, self.dc, 0, 0, SRCCOPY);
                ReleaseDC(hwnd, window_dc);
            }
        }
    }
}

impl Drop for Presenter {
    fn drop(&mut self) {
        self.release();
    }
}
