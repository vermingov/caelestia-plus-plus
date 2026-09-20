//! How much of the surface is repainted for a frame.
//!
//! The surface is as tall as the tallest popout, and the bar is the top fifty
//! pixels of it. WebKit asks for its whole widget to be redrawn for every
//! frame it produces, whatever on the page changed, and GDK answers a redraw
//! of a translucent GL window by clearing a buffer the size of the request
//! and uploading it again. For this surface that is 4.8 MB each way, per
//! frame, to move a spectrum along a strip a tenth of that size — and it was
//! half of what the main thread did while music played.
//!
//! While nothing hangs below the strip, nothing below the strip can have
//! changed, so the request is cut down to the strip before GDK acts on it.
//! GDK has a hook for exactly this, and does its own bookkeeping for older
//! buffers after the hook has run: a buffer that last held an open popout is
//! still repainted in full the first time it comes round again.

use gtk::glib::translate::ToGlibPtr;
use gtk::prelude::*;

/// How far below the strip the pill's shadow reaches: six pixels down and
/// eighteen of blur, pulled in by eight of spread. The logo overhangs the
/// pill by twelve, which is inside it.
const SHADOW: i32 = 16;

/// Cuts a redraw request down to the strip. GDK calls this on the main
/// thread, for every invalidation of the window or of anything in it.
unsafe extern "C" fn keep_to_strip(
    _window: *mut gtk::gdk::ffi::GdkWindow,
    request: *mut gtk::cairo::ffi::cairo_region_t,
) {
    let mut strip = gtk::cairo::ffi::cairo_rectangle_int_t {
        x: 0,
        y: 0,
        width: super::ANY_WIDTH,
        height: super::HEIGHT + SHADOW,
    };
    gtk::cairo::ffi::cairo_region_intersect_rectangle(request, &mut strip);
}

/// Starts a bar off repainting its strip alone, which is how it spends nearly
/// all of its time.
///
/// For a window that has not been shown yet. There is no surface to tell
/// until GTK realises the window, and showing it from `setup` does not do
/// that on the spot, so this waits to be told — as often as it happens.
pub fn begin(gtk_window: &gtk::ApplicationWindow) {
    gtk_window.connect_realize(|gtk_window| follow(gtk_window, false));
}

/// Repaints the strip alone, or the whole surface while something hangs
/// below it.
///
/// Wrong in the safe direction when the two are out of step: a surface left
/// whole costs what it always did, where a popout drawn on a surface still
/// cut to the strip would not show at all.
pub fn follow(gtk_window: &gtk::ApplicationWindow, overhang: bool) {
    let Some(surface) = gtk_window.window() else { return };
    let handler: gtk::gdk::ffi::GdkWindowInvalidateHandlerFunc =
        if overhang { None } else { Some(keep_to_strip) };
    // SAFETY: the surface is alive for the call, and the handler is a plain
    // function that outlives every window and touches only its arguments.
    unsafe { gtk::gdk::ffi::gdk_window_set_invalidate_handler(surface.to_glib_none().0, handler) };

    // The page may have drawn its first frame of the popout before word of
    // it got here, and that frame was cut short.
    if overhang {
        gtk_window.queue_draw();
    }
}
