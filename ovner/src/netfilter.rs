//! Bagian khusus WebView2 (Windows): pemblokir permintaan jaringan dan beberapa fitur
//! yang belum disediakan wry (cek audio, dialog unduhan).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{take_pwstr, WebResourceRequestedEventHandler};
use windows::core::{Interface, BOOL, HSTRING, PWSTR};
use wry::WebViewExtWindows;

use crate::blocker::{self, Blocker};
use crate::settings::Settings;

/// Data yang dibagi antara tab dan penyaring jaringannya.
#[derive(Clone)]
pub struct FilterCtx {
    pub blocker: Rc<RefCell<Blocker>>,
    pub settings: Rc<RefCell<Settings>>,
    /// Jumlah permintaan yang diblokir di halaman ini.
    pub blocked: Rc<Cell<u32>>,
    /// Alamat navigasi halaman utama yang sedang berjalan (sumber untuk aturan $third-party).
    pub page: Rc<RefCell<String>>,
}

/// Pasang penyaring iklan pada tab ini.
pub fn attach(wv: &wry::WebView, ctx: FilterCtx) -> windows::core::Result<()> {
    let core = wv.webview();
    let env = wv.environment();
    unsafe {
        let filter = HSTRING::from("*");
        // API baru juga menangkap permintaan dari iframe dan worker.
        match core.cast::<ICoreWebView2_22>() {
            Ok(w22) => w22.AddWebResourceRequestedFilterWithRequestSourceKinds(
                &filter,
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
            )?,
            Err(_) => core.AddWebResourceRequestedFilter(&filter, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)?,
        }
        let mut token: i64 = 0;
        core.add_WebResourceRequested(
            &WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let page = ctx.page.borrow().clone();
                {
                    let s = ctx.settings.borrow();
                    if !s.block_ads {
                        return Ok(());
                    }
                    let host = url::Url::parse(&page).ok().and_then(|u| u.host_str().map(str::to_string));
                    if host.is_some_and(|h| s.is_exception(&h)) {
                        return Ok(());
                    }
                }
                let req = args.Request()?;
                let mut uri = PWSTR::null();
                req.Uri(&mut uri)?;
                let uri = take_pwstr(uri);
                let mut kind = COREWEBVIEW2_WEB_RESOURCE_CONTEXT::default();
                args.ResourceContext(&mut kind)?;
                let is_main = kind == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT && (uri == page || page.is_empty());
                let rtype = blocker::request_type_from_webview2(kind.0, is_main);
                if ctx.blocker.borrow().should_block(&uri, &page, rtype) {
                    let resp = env.CreateWebResourceResponse(None, 403, &HSTRING::from("Diblokir Ovner"), &HSTRING::new())?;
                    args.SetResponse(&resp)?;
                    ctx.blocked.set(ctx.blocked.get() + 1);
                }
                Ok(())
            })),
            &mut token,
        )?;
    }
    Ok(())
}

/// Apakah tab ini sedang memutar suara (tab seperti ini tidak ditidurkan).
pub fn is_playing_audio(wv: &wry::WebView) -> bool {
    unsafe {
        let Ok(w8) = wv.webview().cast::<ICoreWebView2_8>() else { return false };
        let mut playing = BOOL::default();
        w8.IsDocumentPlayingAudio(&mut playing).is_ok() && playing.as_bool()
    }
}

/// Tampilkan panel unduhan bawaan WebView2.
pub fn open_downloads(wv: &wry::WebView) {
    unsafe {
        if let Ok(w9) = wv.webview().cast::<ICoreWebView2_9>() {
            let _ = w9.OpenDefaultDownloadDialog();
        }
    }
}

/// Kembali/maju dengan API WebView2 (lebih andal daripada `history.back()` lewat skrip).
pub fn go_back(wv: &wry::WebView) {
    unsafe {
        let _ = wv.webview().GoBack();
    }
}

pub fn go_forward(wv: &wry::WebView) {
    unsafe {
        let _ = wv.webview().GoForward();
    }
}

pub fn stop(wv: &wry::WebView) {
    unsafe {
        let _ = wv.webview().Stop();
    }
}
