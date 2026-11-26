//! The one way to a web engine, for the conversation and for the
//! composer's Preview and forwarded message.
//!
//! [`WebView`] is a handle on one page drawn from HTML the app wrote
//! itself. Two adapters stand behind it, chosen when the app is built:
//! WebKitGTK on Linux, where the page is a GTK widget, and WKWebView on
//! macOS, where WebKitGTK does not exist and the page is a native view laid
//! over an empty GTK widget that holds its place.
//!
//! Each adapter offers the same names: [`WebView`] with its two kinds,
//! `reading` for the conversation and `sealed` for a page that runs no
//! script; [`WeakWebView`]; [`Request`], a picture the page asks for at an
//! address of the app's own scheme; [`Filter`] and [`compile_filter`], the
//! rules that block remote content; [`Finder`], which finds text;
//! [`holds_focus`]; and [`prepare`], which `main` calls before anything
//! else. A caller never names the engine beneath.

use std::cell::{Cell, RefCell};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;
#[cfg(target_os = "linux")]
mod webkitgtk;
#[cfg(target_os = "linux")]
pub use webkitgtk::*;

#[cfg(any(target_os = "macos", test))]
pub(crate) mod frame;

thread_local! {
    static TEXT_SCALE: Cell<f64> = const { Cell::new(1.0) };
    static PAGES: RefCell<Vec<WeakWebView>> = const { RefCell::new(Vec::new()) };
}

/// Scales open pages, including previews, while keeping each page's own zoom.
pub fn set_text_scale(factor: f64) {
    let old = TEXT_SCALE.replace(factor);
    let pages = PAGES.with_borrow_mut(|pages| {
        let mut live = Vec::new();
        pages.retain(|weak| {
            let Some(page) = weak.upgrade() else {
                return false;
            };
            live.push(page);
            true
        });
        live
    });
    for page in pages {
        page.set_page_zoom(page.zoom() / old * factor);
    }
}

impl WebView {
    fn register_for_text_scale(&self) {
        self.set_zoom(1.0);
        PAGES.with_borrow_mut(|pages| {
            pages.retain(|weak| weak.upgrade().is_some());
            pages.push(self.downgrade());
        });
    }

    pub fn set_zoom(&self, zoom: f64) {
        self.set_page_zoom(zoom * TEXT_SCALE.get());
    }
}
