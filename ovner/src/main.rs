//! Ovner: browser ringan untuk Windows di atas WebView2 (mesin Edge yang sudah ada di Windows)
//! dengan pemblokir iklan Brave (`adblock-rust`), tab yang ditidurkan otomatis, dan instalasi
//! tanpa hak admin.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
// Sebagian kode hanya dipakai di Windows; di Linux (untuk pengujian) jangan dianggap mati.
#![cfg_attr(not(windows), allow(dead_code))]

mod blocker;
mod icon;
mod install;
#[cfg(windows)]
mod netfilter;
mod settings;
mod update;
mod win;

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tao::dpi::LogicalSize;
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy};
use tao::window::{Icon, Window, WindowBuilder};
use wry::dpi::{LogicalPosition as WLogicalPosition, LogicalSize as WLogicalSize};
use wry::{NewWindowResponse, PageLoadEvent, Rect, WebContext, WebView, WebViewBuilder};

use blocker::Blocker;
use settings::Settings;

const TOOLBAR_H: f64 = 77.0;
const TOOLBAR_HTML: &str = include_str!("ui/toolbar.html");
const NEWTAB_HTML: &str = include_str!("ui/newtab.html");
const CONTENT_JS: &str = include_str!("ui/content.js");
/// Halaman tab baru disajikan lewat protokol `ovner://`. Di Windows, wry memetakannya ke
/// `http://ovner.<path>/` (WebView2 belum mendukung skema sendiri secara penuh).
#[cfg(windows)]
const NEWTAB_URL: &str = "http://ovner.newtab/";
#[cfg(not(windows))]
const NEWTAB_URL: &str = "ovner://newtab/";
const INSTANCE_PORT: u16 = 47893;
const MAGIC: &str = "OVNER-1";
/// Seberapa sering tab diperiksa untuk ditidurkan dan penghitung iklan diperbarui.
const TICK: Duration = Duration::from_secs(2);
const SLEEP_CHECK: Duration = Duration::from_secs(30);
/// Argumen mesin WebView2. Fitur yang dimatikan tidak berguna untuk browsing biasa tetapi
/// memakan RAM/CPU (proses renderer cadangan, cache maju-mundur, terjemahan, cast media, ...).
const DISABLED_FEATURES: &str = "msWebOOUI,msPdfOOUI,SpareRendererForSitePerProcess,BackForwardCache,Translate,MediaRouter,OptimizationHints,AutofillServerCommunication,msEdgeShoppingAssistant";

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

fn data_dir() -> &'static Path {
    DATA_DIR.get().map(PathBuf::as_path).unwrap_or(Path::new("."))
}

/// Catatan sederhana di `<data>/ovner.log` untuk menelusuri masalah.
pub fn log(msg: &str) {
    let p = data_dir().join("ovner.log");
    if std::fs::metadata(&p).map(|m| m.len() > 256 * 1024).unwrap_or(false) {
        let _ = std::fs::rename(&p, data_dir().join("ovner.old.log"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = writeln!(f, "{secs} {msg}");
    }
    if cfg!(debug_assertions) {
        eprintln!("[ovner] {msg}");
    }
}

// ---------------------------------------------------------------------------
// Alamat

/// Teks dari kolom alamat menjadi URL: alamat apa adanya, domain diberi https://,
/// alamat lokal diberi http://, selain itu dicari dengan mesin pencari.
fn to_url(text: &str, settings: &Settings) -> String {
    let t = text.trim();
    let lower = t.to_ascii_lowercase();
    if ["http://", "https://", "file://", "about:", "edge://", "view-source:"].iter().any(|p| lower.starts_with(p)) {
        return t.to_string();
    }
    if t.contains(char::is_whitespace) || t.is_empty() {
        return settings.search_url(t);
    }
    let host = lower.split(['/', '?', '#']).next().unwrap_or("");
    let host_no_port = host.rsplit_once(':').map(|(h, p)| if p.chars().all(|c| c.is_ascii_digit()) { h } else { host }).unwrap_or(host);
    let local = host_no_port == "localhost"
        || host_no_port.ends_with(".local")
        || host_no_port.parse::<std::net::Ipv4Addr>().is_ok_and(|ip| ip.is_private() || ip.is_loopback());
    if local {
        return format!("http://{t}");
    }
    let looks_like_domain = host_no_port.contains('.')
        && !host_no_port.starts_with('.')
        && !host_no_port.ends_with('.')
        && host_no_port.rsplit('.').next().is_some_and(|tld| tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic()));
    if looks_like_domain || host_no_port.parse::<std::net::Ipv4Addr>().is_ok() {
        format!("https://{t}")
    } else {
        settings.search_url(t)
    }
}

fn is_newtab(url: &str) -> bool {
    url.is_empty() || url.starts_with(NEWTAB_URL) || url == "about:blank"
}

/// Alamat yang ditampilkan di kolom alamat (tab baru: kosong).
fn display_url(url: &str) -> String {
    if is_newtab(url) {
        String::new()
    } else {
        url.to_string()
    }
}

fn host_of(url: &str) -> Option<String> {
    url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string))
}

// ---------------------------------------------------------------------------
// Kejadian

enum Remote {
    Show,
    Open(String),
    Exit,
}

enum UserEvent {
    Toolbar(String),
    Content(u64, String),
    Title(u64, String),
    PageLoad(u64, PageLoadEvent, String),
    NewTab(String),
    Navigate(u64, String),
    EngineReady(Vec<u8>),
    UpdateReady(String),
    Menu(String),
    Remote(Remote),
}

// ---------------------------------------------------------------------------
// Tab

struct Tab {
    id: u64,
    webview: Option<WebView>,
    url: String,
    title: String,
    loading: bool,
    /// Kapan tab ini terakhir aktif (dipakai untuk menidurkan tab).
    last_active: Instant,
    blocked: Rc<Cell<u32>>,
    page: Rc<RefCell<String>>,
    cos_exceptions: HashSet<String>,
    generichide: bool,
}

#[derive(Serialize, Deserialize, Default)]
struct Session {
    tabs: Vec<SessionTab>,
    active: usize,
}

#[derive(Serialize, Deserialize)]
struct SessionTab {
    url: String,
    title: String,
}

struct App {
    window: Window,
    toolbar: WebView,
    toolbar_ready: bool,
    tabs: Vec<Tab>,
    active: usize,
    next_id: u64,
    closed: Vec<String>,
    settings: Rc<RefCell<Settings>>,
    blocker: Rc<RefCell<Blocker>>,
    web_context: WebContext,
    proxy: EventLoopProxy<UserEvent>,
    browser_args: String,
    update_ready: Option<String>,
    last_sleep_check: Instant,
    last_render_key: String,
    menu: muda::Menu,
}

fn send(proxy: &EventLoopProxy<UserEvent>, e: UserEvent) {
    let _ = proxy.send_event(e);
}

fn browser_args(s: &Settings) -> String {
    let mut a = format!("--disable-features={DISABLED_FEATURES} --disable-background-networking");
    if s.save_ram {
        // V8 mengutamakan ukuran memori daripada kecepatan puncak.
        a.push_str(" --js-flags=--optimize-for-size");
    }
    a
}

fn newtab_response() -> wry::http::Response<Cow<'static, [u8]>> {
    wry::http::Response::builder()
        .header("Content-Type", "text/html; charset=utf-8")
        .body(Cow::Borrowed(NEWTAB_HTML.as_bytes()))
        .unwrap()
}

impl App {
    fn content_rect(&self) -> Rect {
        let size: LogicalSize<f64> = self.window.inner_size().to_logical(self.window.scale_factor());
        Rect {
            position: WLogicalPosition::new(0.0, TOOLBAR_H).into(),
            size: WLogicalSize::new(size.width.max(1.0), (size.height - TOOLBAR_H).max(1.0)).into(),
        }
    }

    fn toolbar_rect(&self) -> Rect {
        let size: LogicalSize<f64> = self.window.inner_size().to_logical(self.window.scale_factor());
        Rect { position: WLogicalPosition::new(0.0, 0.0).into(), size: WLogicalSize::new(size.width.max(1.0), TOOLBAR_H).into() }
    }

    fn tab_index(&self, id: u64) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    fn new_tab_struct(&mut self, url: &str, title: &str) -> Tab {
        self.next_id += 1;
        Tab {
            id: self.next_id,
            webview: None,
            url: url.to_string(),
            title: title.to_string(),
            loading: false,
            last_active: Instant::now(),
            blocked: Rc::new(Cell::new(0)),
            page: Rc::new(RefCell::new(String::new())),
            cos_exceptions: HashSet::new(),
            generichide: false,
        }
    }

    /// Buat WebView untuk tab `i` (tab baru atau tab yang dibangunkan).
    fn wake(&mut self, i: usize) {
        if self.tabs[i].webview.is_some() {
            return;
        }
        let rect = self.content_rect();
        let tab = &self.tabs[i];
        let (id, url) = (tab.id, if tab.url.is_empty() { NEWTAB_URL.to_string() } else { tab.url.clone() });
        let (blocked, page) = (tab.blocked.clone(), tab.page.clone());
        *page.borrow_mut() = url.clone();
        let visible = i == self.active;

        let p_ipc = self.proxy.clone();
        let p_title = self.proxy.clone();
        let p_load = self.proxy.clone();
        let p_nav = self.proxy.clone();
        let p_new = self.proxy.clone();
        let (nav_blocked, nav_page) = (blocked.clone(), page.clone());

        let builder = WebViewBuilder::new_with_web_context(&mut self.web_context)
            .with_url(url)
            .with_bounds(rect)
            .with_visible(visible)
            .with_focused(visible)
            .with_hotkeys_zoom(true)
            .with_devtools(true)
            .with_initialization_script(CONTENT_JS)
            .with_custom_protocol("ovner".into(), |_, _| newtab_response())
            .with_ipc_handler(move |req| send(&p_ipc, UserEvent::Content(id, req.body().clone())))
            .with_document_title_changed_handler(move |t| send(&p_title, UserEvent::Title(id, t)))
            .with_on_page_load_handler(move |ev, url| send(&p_load, UserEvent::PageLoad(id, ev, url)))
            .with_navigation_handler(move |url| {
                // Buang parameter pelacak klik (fbclid, utm_*, ...) sebelum halaman dibuka.
                if let Some(clean) = blocker::strip_tracking_params(&url) {
                    send(&p_nav, UserEvent::Navigate(id, clean));
                    return false;
                }
                *nav_page.borrow_mut() = url;
                nav_blocked.set(0);
                true
            })
            .with_new_window_req_handler(move |url, features| {
                // Jendela kecil dengan ukuran tertentu (login Google/Microsoft, pembayaran)
                // dibiarkan sebagai jendela sendiri; tautan biasa dibuka di tab baru.
                if features.size.is_some() {
                    NewWindowResponse::Allow
                } else {
                    send(&p_new, UserEvent::NewTab(url));
                    NewWindowResponse::Deny
                }
            });

        #[cfg(windows)]
        let builder = {
            use wry::WebViewBuilderExtWindows;
            builder
                .with_additional_browser_args(self.browser_args.clone())
                .with_browser_accelerator_keys(true)
                .with_default_context_menus(true)
        };

        match builder.build_as_child(&self.window) {
            Ok(wv) => {
                #[cfg(windows)]
                {
                    let ctx = netfilter::FilterCtx {
                        blocker: self.blocker.clone(),
                        settings: self.settings.clone(),
                        blocked: blocked.clone(),
                        page: page.clone(),
                    };
                    if let Err(e) = netfilter::attach(&wv, ctx) {
                        log(&format!("pemblokir tidak terpasang: {e}"));
                    }
                }
                self.tabs[i].webview = Some(wv);
            }
            Err(e) => log(&format!("gagal membuat tab: {e}")),
        }
        let _ = (&blocked, &self.browser_args);
    }

    fn open_tab(&mut self, url: &str, activate: bool) {
        let title = if is_newtab(url) { "Tab baru" } else { "" };
        let tab = self.new_tab_struct(url, title);
        let at = if self.tabs.is_empty() { 0 } else { self.active + 1 };
        self.tabs.insert(at, tab);
        if at <= self.active && !self.tabs.is_empty() && self.tabs.len() > 1 {
            self.active += 1;
        }
        if activate {
            self.activate(at);
            if is_newtab(url) {
                self.focus_address();
            }
        } else {
            self.wake(at);
        }
        self.save_session();
        self.render();
    }

    fn activate(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        let now = Instant::now();
        if let Some(old) = self.tabs.get_mut(self.active) {
            old.last_active = now;
        }
        self.active = i;
        self.wake(i);
        for (j, t) in self.tabs.iter_mut().enumerate() {
            if let Some(wv) = &t.webview {
                let on = j == i;
                let _ = wv.set_visible(on);
                set_memory_low(wv, !on);
            }
        }
        let rect = self.content_rect();
        let t = &mut self.tabs[i];
        t.last_active = now;
        if let Some(wv) = &t.webview {
            let _ = wv.set_bounds(rect);
            let _ = wv.focus();
        }
        self.save_session();
        self.render();
    }

    fn close_tab(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        let t = self.tabs.remove(i);
        if !is_newtab(&t.url) {
            self.closed.push(t.url.clone());
            if self.closed.len() > 25 {
                self.closed.remove(0);
            }
        }
        drop(t); // WebView ikut dibuang (proses renderer-nya dilepas)
        if self.tabs.is_empty() {
            self.active = 0;
            self.open_tab(NEWTAB_URL, true);
            return;
        }
        if i < self.active || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1).min(self.tabs.len() - 1);
        }
        let a = self.active;
        self.activate(a);
    }

    fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    fn active_webview(&self) -> Option<&WebView> {
        self.active_tab().and_then(|t| t.webview.as_ref())
    }

    fn navigate_active(&mut self, url: &str) {
        let i = self.active;
        if self.tabs.is_empty() {
            self.open_tab(url, true);
            return;
        }
        self.tabs[i].url = url.to_string();
        if let Some(wv) = &self.tabs[i].webview {
            let _ = wv.load_url(url);
            let _ = wv.focus();
        } else {
            self.wake(i);
        }
        self.render();
    }

    fn focus_address(&self) {
        let _ = self.toolbar.focus();
        let _ = self.toolbar.evaluate_script("window.ovner && ovner.focusAddress()");
    }

    /// Tidurkan tab yang lama tidak dibuka: WebView-nya dibuang dan dimuat ulang saat diklik.
    fn sleep_idle_tabs(&mut self) {
        let mins = self.settings.borrow().sleep_tabs_minutes;
        if mins == 0 {
            return;
        }
        let limit = Duration::from_secs(mins * 60);
        let mut changed = false;
        for (j, t) in self.tabs.iter_mut().enumerate() {
            if j == self.active || t.webview.is_none() || t.last_active.elapsed() < limit {
                continue;
            }
            #[cfg(windows)]
            if t.webview.as_ref().is_some_and(netfilter::is_playing_audio) {
                continue;
            }
            if let Some(wv) = &t.webview {
                if let Ok(u) = wv.url() {
                    if !u.is_empty() {
                        t.url = u;
                    }
                }
            }
            t.webview = None;
            t.loading = false;
            changed = true;
            log(&format!("tab ditidurkan: {}", t.url));
        }
        if changed {
            self.render();
        }
    }

    fn render(&mut self) {
        if !self.toolbar_ready {
            return;
        }
        let s = self.settings.borrow();
        let tabs: Vec<Value> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(j, t)| {
                json!({
                    "id": t.id,
                    "title": if t.title.is_empty() { display_url(&t.url) } else { t.title.clone() },
                    "url": t.url,
                    "active": j == self.active,
                    "sleeping": t.webview.is_none() && j != self.active,
                    "loading": t.loading,
                })
            })
            .collect();
        let tab = self.tabs.get(self.active);
        let url = tab.map(|t| display_url(&t.url)).unwrap_or_default();
        let host = tab.and_then(|t| host_of(&t.url));
        let shield_on = s.block_ads && !host.as_deref().is_some_and(|h| s.is_exception(h));
        let state = json!({
            "tabs": tabs,
            "url": url,
            "loading": tab.is_some_and(|t| t.loading),
            "blocked": tab.map(|t| t.blocked.get()).unwrap_or(0),
            "shieldOn": shield_on,
            "blockAds": s.block_ads,
            "listsReady": self.blocker.borrow().full,
            "update": self.update_ready,
        });
        drop(s);
        let text = state.to_string();
        if text == self.last_render_key {
            return;
        }
        let _ = self.toolbar.evaluate_script(&format!("window.ovner && ovner.render({text})"));
        if let Some(t) = tab {
            let title = if t.title.is_empty() { "Ovner".to_string() } else { format!("{} - Ovner", t.title) };
            self.window.set_title(&title);
        }
        self.last_render_key = text;
    }

    fn save_session(&self) {
        let s = Session {
            tabs: self
                .tabs
                .iter()
                .filter(|t| !is_newtab(&t.url))
                .map(|t| SessionTab { url: t.url.clone(), title: t.title.clone() })
                .collect(),
            active: self.tabs.iter().take(self.active).filter(|t| !is_newtab(&t.url)).count(),
        };
        if let Ok(text) = serde_json::to_string(&s) {
            let _ = std::fs::write(data_dir().join("session.json"), text);
        }
    }

    fn layout(&mut self) {
        let _ = self.toolbar.set_bounds(self.toolbar_rect());
        let rect = self.content_rect();
        for t in &self.tabs {
            if let Some(wv) = &t.webview {
                let _ = wv.set_bounds(rect);
            }
        }
    }

    fn inject_css(&self, i: usize, css: &str) {
        if std::env::var_os("OVNER_DEBUG").is_some() {
            log(&format!("css kosmetik tab {}: {} byte", self.tabs[i].id, css.len()));
        }
        if css.is_empty() {
            return;
        }
        if let Some(wv) = &self.tabs[i].webview {
            let js = format!(
                "(function(c){{var s=document.getElementById('__ovner_sembunyi');if(!s){{s=document.createElement('style');\
                 s.id='__ovner_sembunyi';(document.head||document.documentElement).appendChild(s);}}s.textContent+=c;}})({});",
                serde_json::to_string(css).unwrap_or_default()
            );
            let _ = wv.evaluate_script(&js);
        }
    }

    fn blocking_active_for(&self, url: &str) -> bool {
        let s = self.settings.borrow();
        s.block_ads && !host_of(url).is_some_and(|h| s.is_exception(&h))
    }

    // ---- pesan dari halaman --------------------------------------------------

    fn on_content(&mut self, id: u64, body: &str) {
        let Some(i) = self.tab_index(id) else { return };
        let Ok(msg) = serde_json::from_str::<Value>(body) else { return };
        let current_url = self.tabs[i].webview.as_ref().and_then(|w| w.url().ok()).unwrap_or_default();
        if std::env::var_os("OVNER_DEBUG").is_some() {
            log(&format!("pesan tab {id}: {} ({} byte) url={current_url}", msg["t"], body.len()));
        }
        match msg["t"].as_str().unwrap_or("") {
            "key" => {
                if i == self.active {
                    self.on_key(msg["k"].as_str().unwrap_or(""));
                }
            }
            "url" => {
                if !current_url.is_empty() {
                    self.tabs[i].url = current_url;
                    self.render();
                }
            }
            "page" => {
                if current_url.is_empty() || !self.blocking_active_for(&current_url) {
                    return;
                }
                let (css, exc, generichide) = self.blocker.borrow().page_cosmetics(&current_url);
                self.tabs[i].cos_exceptions = exc;
                self.tabs[i].generichide = generichide;
                self.inject_css(i, &css);
            }
            "cos" => {
                if self.tabs[i].generichide || current_url.is_empty() || !self.blocking_active_for(&current_url) {
                    return;
                }
                let strings = |v: &Value| -> Vec<String> {
                    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
                };
                let (classes, ids) = (strings(&msg["c"]), strings(&msg["i"]));
                let css = self.blocker.borrow().generic_cosmetics(&classes, &ids, &self.tabs[i].cos_exceptions);
                self.inject_css(i, &css);
            }
            // Kotak cari di halaman tab baru (hanya diterima dari halaman tab baru itu sendiri).
            "go" if is_newtab(&current_url) => {
                if let Some(text) = msg["text"].as_str() {
                    let url = to_url(text, &self.settings.borrow());
                    self.navigate_active(&url);
                }
            }
            _ => {}
        }
    }

    fn on_key(&mut self, k: &str) {
        let n = self.tabs.len();
        match k {
            "newtab" => self.open_tab(NEWTAB_URL, true),
            "close" => self.close_tab(self.active),
            "reopen" => {
                if let Some(u) = self.closed.pop() {
                    self.open_tab(&u, true);
                }
            }
            "address" => self.focus_address(),
            "nexttab" if n > 0 => self.activate((self.active + 1) % n),
            "prevtab" if n > 0 => self.activate((self.active + n - 1) % n),
            "reload" => {
                if let Some(wv) = self.active_webview() {
                    let _ = wv.reload();
                }
            }
            "downloads" => {
                self.on_menu("downloads");
            }
            k if k.starts_with("tab") => {
                if let Ok(d) = k[3..].parse::<usize>() {
                    // Ctrl+9 = tab terakhir, seperti Chrome.
                    let idx = if d == 9 { n.saturating_sub(1) } else { d - 1 };
                    if idx < n {
                        self.activate(idx);
                    }
                }
            }
            _ => {}
        }
    }

    // ---- pesan dari toolbar --------------------------------------------------

    fn on_toolbar(&mut self, body: &str) -> bool {
        let Ok(msg) = serde_json::from_str::<Value>(body) else { return false };
        let id = msg["id"].as_u64().unwrap_or(0);
        match msg["cmd"].as_str().unwrap_or("") {
            "ready" => {
                self.toolbar_ready = true;
                self.last_render_key.clear();
                self.render();
            }
            "go" => {
                let url = to_url(msg["text"].as_str().unwrap_or(""), &self.settings.borrow());
                self.navigate_active(&url);
            }
            "newtab" => self.open_tab(NEWTAB_URL, true),
            "activate" => {
                if let Some(i) = self.tab_index(id) {
                    self.activate(i);
                }
            }
            "close" => {
                if let Some(i) = self.tab_index(id) {
                    self.close_tab(i);
                }
            }
            "back" => {
                if let Some(wv) = self.active_webview() {
                    #[cfg(windows)]
                    netfilter::go_back(wv);
                    #[cfg(not(windows))]
                    let _ = wv.evaluate_script("history.back()");
                }
            }
            "forward" => {
                if let Some(wv) = self.active_webview() {
                    #[cfg(windows)]
                    netfilter::go_forward(wv);
                    #[cfg(not(windows))]
                    let _ = wv.evaluate_script("history.forward()");
                }
            }
            "reload" => {
                if let Some(wv) = self.active_webview() {
                    let _ = wv.reload();
                }
            }
            "stop" => {
                if let Some(wv) = self.active_webview() {
                    #[cfg(windows)]
                    netfilter::stop(wv);
                    #[cfg(not(windows))]
                    let _ = wv.evaluate_script("window.stop()");
                }
            }
            "focus-page" => {
                if let Some(wv) = self.active_webview() {
                    let _ = wv.focus();
                }
            }
            "shield" => {
                let host = self.active_tab().and_then(|t| host_of(&t.url));
                if let Some(h) = host {
                    let off = self.settings.borrow_mut().toggle_exception(&h);
                    self.settings.borrow().save(data_dir());
                    log(&format!("pemblokir di {h}: {}", if off { "mati" } else { "nyala" }));
                    if let Some(wv) = self.active_webview() {
                        let _ = wv.reload();
                    }
                    self.render();
                }
            }
            "key" => self.on_key(msg["k"].as_str().unwrap_or("")),
            "menu" => {
                let x = msg["x"].as_f64().unwrap_or(0.0);
                let y = msg["y"].as_f64().unwrap_or(TOOLBAR_H);
                self.show_menu(x, y);
            }
            "restart" => return true,
            _ => {}
        }
        false
    }

    fn show_menu(&self, x: f64, y: f64) {
        #[cfg(windows)]
        {
            use muda::ContextMenu;
            use tao::platform::windows::WindowExtWindows;
            let pos = muda::dpi::Position::Logical(muda::dpi::LogicalPosition::new((x - 240.0).max(0.0), y));
            unsafe {
                self.menu.show_context_menu_for_hwnd(self.window.hwnd() as isize, Some(pos));
            }
        }
        #[cfg(not(windows))]
        {
            let _ = (&self.menu, x, y);
            log("menu hanya tersedia di Windows");
        }
    }

    /// Mengembalikan true bila Ovner harus ditutup.
    fn on_menu(&mut self, id: &str) -> bool {
        match id {
            "newtab" => self.open_tab(NEWTAB_URL, true),
            "reopen" => self.on_key("reopen"),
            "downloads" => {
                #[cfg(windows)]
                if let Some(wv) = self.active_webview() {
                    netfilter::open_downloads(wv);
                }
            }
            "print" => {
                if let Some(wv) = self.active_webview() {
                    let _ = wv.print();
                }
            }
            "filters" => {
                start_filter_update(self.proxy.clone(), true);
                win::info("Daftar filter iklan sedang diperbarui di latar. Halaman yang dibuka berikutnya memakai daftar terbaru.");
            }
            "settings" => {
                let p = Settings::path(data_dir());
                if !p.exists() {
                    self.settings.borrow().save(data_dir());
                }
                let _ = std::process::Command::new(if cfg!(windows) { "notepad" } else { "xdg-open" }).arg(p).spawn();
            }
            "data" => win::open_folder(data_dir()),
            "clear" => {
                if win::confirm("Hapus semua data penjelajahan (riwayat, cookie, login situs, cache)?\n\nAnda akan keluar dari semua situs.") {
                    if let Some(wv) = self.tabs.iter().find_map(|t| t.webview.as_ref()) {
                        let _ = wv.clear_all_browsing_data();
                    }
                }
            }
            "about" => {
                let engine = wry::webview_version().unwrap_or_else(|_| "?".into());
                let lists = if self.blocker.borrow().full { "daftar lengkap (EasyList, EasyPrivacy, ABPindo, uBlock)" } else { "daftar bawaan (daftar lengkap sedang diunduh)" };
                win::info(&format!(
                    "Ovner {}\n\nMesin: Microsoft Edge WebView2 {engine}\nPemblokir iklan: adblock-rust (Brave), {lists}\nData: {}\n\nhttps://github.com/{}",
                    env!("CARGO_PKG_VERSION"),
                    data_dir().display(),
                    update::REPO
                ));
            }
            "quit" => return true,
            _ => {}
        }
        false
    }
}

fn set_memory_low(wv: &WebView, low: bool) {
    #[cfg(windows)]
    {
        use wry::{MemoryUsageLevel, WebViewExtWindows};
        let _ = wv.set_memory_usage_level(if low { MemoryUsageLevel::Low } else { MemoryUsageLevel::Normal });
    }
    #[cfg(not(windows))]
    let _ = (wv, low);
}

fn build_menu() -> muda::Menu {
    use muda::{Menu, MenuItem, PredefinedMenuItem};
    let m = Menu::new();
    let item = |id: &str, text: &str| MenuItem::with_id(id, text, true, None);
    let _ = m.append_items(&[
        &item("newtab", "Tab baru\tCtrl+T"),
        &item("reopen", "Buka lagi tab yang ditutup\tCtrl+Shift+T"),
        &PredefinedMenuItem::separator(),
        &item("downloads", "Unduhan\tCtrl+J"),
        &item("print", "Cetak...\tCtrl+P"),
        &PredefinedMenuItem::separator(),
        &item("filters", "Perbarui daftar filter iklan"),
        &item("settings", "Pengaturan (settings.ini)"),
        &item("data", "Buka folder data"),
        &item("clear", "Hapus data penjelajahan..."),
        &PredefinedMenuItem::separator(),
        &item("about", "Tentang Ovner"),
        &item("quit", "Keluar"),
    ]);
    m
}

/// Unduh daftar filter (bila perlu atau dipaksa) lalu susun ulang mesinnya di latar.
fn start_filter_update(proxy: EventLoopProxy<UserEvent>, force: bool) {
    let dir = data_dir().to_path_buf();
    std::thread::spawn(move || {
        let had_cache = blocker::load_cached(&dir).is_some();
        let mut changed = false;
        if force || blocker::lists_need_refresh(&dir) {
            if !force {
                std::thread::sleep(Duration::from_secs(3));
            }
            let n = blocker::download_lists(&dir);
            log(&format!("daftar filter diunduh: {n}/{}", blocker::FILTER_LISTS.len()));
            changed = n > 0;
        }
        if changed || !had_cache {
            let started = Instant::now();
            if let Some(bytes) = blocker::build_and_cache(&dir) {
                log(&format!("mesin filter disusun dalam {:?}", started.elapsed()));
                send(&proxy, UserEvent::EngineReady(bytes));
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Satu instance

enum Lock {
    Primary(TcpListener),
    Secondary,
    None,
}

fn acquire_lock(cmd: &str, restarted: bool) -> Lock {
    let addr = SocketAddr::from(([127, 0, 0, 1], INSTANCE_PORT));
    let attempts = if restarted { 40 } else { 2 };
    for _ in 0..attempts {
        if let Ok(l) = TcpListener::bind(addr) {
            return Lock::Primary(l);
        }
        let mut ours = false;
        if let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_millis(800)) {
            let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
            let mut line = String::new();
            if BufReader::new(&s).read_line(&mut line).is_ok() && line.trim() == MAGIC {
                ours = true;
                if !restarted {
                    let _ = s.write_all(format!("{cmd}\n").as_bytes());
                    return Lock::Secondary;
                }
            }
        }
        if !ours && !restarted {
            return Lock::None;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Lock::None
}

fn stop_running_instance() {
    let addr = SocketAddr::from(([127, 0, 0, 1], INSTANCE_PORT));
    let mut asked = false;
    if let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_millis(800)) {
        let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
        let mut line = String::new();
        if BufReader::new(&s).read_line(&mut line).is_ok() && line.trim() == MAGIC {
            let _ = s.write_all(b"exit\n");
            asked = true;
        }
    }
    if asked {
        for _ in 0..50 {
            if TcpListener::bind(addr).is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        std::thread::sleep(Duration::from_millis(800));
    }
}

fn serve_lock(listener: TcpListener, proxy: EventLoopProxy<UserEvent>) {
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut s) = conn else { continue };
            let _ = s.write_all(format!("{MAGIC}\n").as_bytes());
            let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
            let mut line = String::new();
            if BufReader::new(&s).read_line(&mut line).is_err() {
                continue;
            }
            let line = line.trim();
            let cmd = match line {
                "show" => Some(Remote::Show),
                "exit" => Some(Remote::Exit),
                _ => line
                    .strip_prefix("open ")
                    .filter(|u| u.starts_with("http://") || u.starts_with("https://"))
                    .map(|u| Remote::Open(u.to_string())),
            };
            if let Some(c) = cmd {
                send(&proxy, UserEvent::Remote(c));
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Start

struct Args {
    portable: bool,
    uninstall: bool,
    restarted: bool,
    urls: Vec<String>,
}

fn parse_args() -> Args {
    let mut a = Args { portable: false, uninstall: false, restarted: false, urls: Vec::new() };
    for v in std::env::args().skip(1) {
        match v.as_str() {
            "--portable" => a.portable = true,
            "--uninstall" => a.uninstall = true,
            "--restarted" => a.restarted = true,
            s if s.starts_with("http://") || s.starts_with("https://") => a.urls.push(s.to_string()),
            _ => {}
        }
    }
    a
}

fn run_uninstall(exe: &Path, portable: bool) {
    if !win::confirm("Hapus Ovner dari komputer ini?\n\nRiwayat, cookie, dan pengaturan Ovner juga akan dihapus.") {
        return;
    }
    stop_running_instance();
    install::remove_integration();
    let data = install::data_dir(exe, portable);
    if install::is_installed_copy(exe) {
        win::delete_dirs_later(&[&data, &install::install_dir()]);
        win::info("Ovner sudah dihapus.");
    } else {
        win::delete_dirs_later(&[&data]);
        win::info("Data Ovner sudah dihapus. File exe portabel bisa Anda hapus sendiri.");
    }
}

fn window_icon() -> Option<Icon> {
    Icon::from_rgba(icon::rgba(64), 64, 64).ok()
}

fn main() {
    let args = parse_args();
    let exe = std::env::current_exe().unwrap_or_default();
    let portable = install::is_portable(&exe, args.portable);
    update::cleanup_old();

    if args.uninstall {
        run_uninstall(&exe, portable);
        return;
    }
    // Dijalankan dari Downloads (Ovner-Setup.exe): pasang ke LocalAppData lalu jalankan salinannya.
    if install::should_install(&exe, args.portable) {
        match install::install_self(&exe, stop_running_instance) {
            Ok(target) => {
                if std::process::Command::new(&target).args(&args.urls).spawn().is_ok() {
                    return;
                }
            }
            Err(e) => win::info(&format!("Ovner tidak bisa dipasang ({e}).\n\nOvner dijalankan langsung dari file ini.")),
        }
    }

    let data = std::env::var_os("OVNER_DATA").map(PathBuf::from).unwrap_or_else(|| install::data_dir(&exe, portable));
    let _ = std::fs::create_dir_all(&data);
    let _ = DATA_DIR.set(data);
    if install::is_installed_copy(&exe) {
        install::ensure_integration(&exe, false);
    }

    let cmd = args.urls.first().map(|u| format!("open {u}")).unwrap_or_else(|| "show".into());
    let lock = acquire_lock(&cmd, args.restarted);
    if matches!(lock, Lock::Secondary) {
        return;
    }
    win::init_identity();
    log(&format!("Ovner {} mulai (portabel: {portable})", env!("CARGO_PKG_VERSION")));

    let event_loop: EventLoop<UserEvent> = EventLoopBuilder::with_user_event().build();
    let proxy = event_loop.create_proxy();
    if let Lock::Primary(l) = lock {
        serve_lock(l, proxy.clone());
    }
    {
        let p = Mutex::new(proxy.clone());
        muda::MenuEvent::set_event_handler(Some(move |e: muda::MenuEvent| {
            if let Ok(p) = p.lock() {
                send(&p, UserEvent::Menu(e.id.0.clone()));
            }
        }));
    }

    let settings = Settings::load(data_dir());
    if !Settings::path(data_dir()).exists() {
        settings.save(data_dir());
    }
    let cached = blocker::load_cached(data_dir());
    let full = cached.is_some();
    let blocker = Rc::new(RefCell::new(Blocker::new(cached.unwrap_or_else(blocker::builtin_engine), full)));
    start_filter_update(proxy.clone(), false);
    if settings.check_updates && cfg!(windows) {
        let p = proxy.clone();
        let dir = data_dir().to_path_buf();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(20));
            match update::check_and_stage(&dir) {
                Ok(Some(v)) => {
                    log(&format!("update {v} siap"));
                    send(&p, UserEvent::UpdateReady(v));
                }
                Ok(None) => {}
                Err(e) => log(&format!("cek update: {e}")),
            }
        });
    }

    let window = WindowBuilder::new()
        .with_title("Ovner")
        .with_inner_size(LogicalSize::new(1280.0, 800.0))
        .with_min_inner_size(LogicalSize::new(480.0, 320.0))
        .with_maximized(true)
        .with_window_icon(window_icon())
        .build(&event_loop)
        .expect("jendela Ovner");

    let mut web_context = WebContext::new(Some(data_dir().join("WebView2")));
    let args_str = browser_args(&settings);
    let toolbar = {
        let p = proxy.clone();
        let size: LogicalSize<f64> = window.inner_size().to_logical(window.scale_factor());
        let b = WebViewBuilder::new_with_web_context(&mut web_context)
            .with_html(TOOLBAR_HTML)
            .with_bounds(Rect {
                position: WLogicalPosition::new(0.0, 0.0).into(),
                size: WLogicalSize::new(size.width, TOOLBAR_H).into(),
            })
            .with_hotkeys_zoom(false)
            .with_devtools(cfg!(debug_assertions))
            .with_ipc_handler(move |req| send(&p, UserEvent::Toolbar(req.body().clone())));
        #[cfg(windows)]
        let b = {
            use wry::WebViewBuilderExtWindows;
            b.with_additional_browser_args(args_str.clone()).with_browser_accelerator_keys(false).with_default_context_menus(false)
        };
        match b.build_as_child(&window) {
            Ok(w) => w,
            Err(e) => {
                win::info(&format!(
                    "Ovner butuh Microsoft Edge WebView2 Runtime (sudah ada di Windows 11).\n\nGalat: {e}"
                ));
                return;
            }
        }
    };

    let mut app = App {
        window,
        toolbar,
        toolbar_ready: false,
        tabs: Vec::new(),
        active: 0,
        next_id: 0,
        closed: Vec::new(),
        settings: Rc::new(RefCell::new(settings)),
        blocker,
        web_context,
        proxy,
        browser_args: args_str,
        update_ready: None,
        last_sleep_check: Instant::now(),
        last_render_key: String::new(),
        menu: build_menu(),
    };

    // Pulihkan tab terakhir: hanya tab aktif yang dimuat, sisanya tidur sampai diklik.
    let restore = app.settings.borrow().restore_tabs;
    let session: Session = if restore {
        std::fs::read_to_string(data_dir().join("session.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    } else {
        Session::default()
    };
    for st in &session.tabs {
        let t = app.new_tab_struct(&st.url, &st.title);
        app.tabs.push(t);
    }
    for u in &args.urls {
        let t = app.new_tab_struct(u, "");
        app.tabs.push(t);
    }
    if app.tabs.is_empty() {
        let t = app.new_tab_struct(NEWTAB_URL, "Tab baru");
        app.tabs.push(t);
    }
    let first = if args.urls.is_empty() { session.active.min(app.tabs.len() - 1) } else { app.tabs.len() - 1 };
    app.activate(first);
    if is_newtab(&app.tabs[first].url) {
        app.focus_address();
    }

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + TICK);
        let mut quit = false;
        match event {
            // Linux (pengujian): toolbar dibuat sebelum jendela tampil sehingga tertutup kotak
            // GTK jendela; tampilkan ulang supaya naik ke atas.
            #[cfg(not(windows))]
            Event::NewEvents(StartCause::Init) => {
                let _ = app.toolbar.set_visible(false);
                let _ = app.toolbar.set_visible(true);
                app.layout();
            }
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
                if app.last_sleep_check.elapsed() >= SLEEP_CHECK {
                    app.last_sleep_check = Instant::now();
                    app.sleep_idle_tabs();
                }
                // Penghitung iklan yang diblokir berubah selama halaman memuat.
                app.render();
            }
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => quit = true,
                WindowEvent::Resized(size) => {
                    if size.width == 0 || size.height == 0 || app.window.is_minimized() {
                        for t in &app.tabs {
                            if let Some(wv) = &t.webview {
                                set_memory_low(wv, true);
                            }
                        }
                    } else {
                        if let Some(wv) = app.active_webview() {
                            set_memory_low(wv, false);
                        }
                        app.layout();
                    }
                }
                WindowEvent::ScaleFactorChanged { .. } => app.layout(),
                WindowEvent::Focused(true) => {
                    if let Some(wv) = app.active_webview() {
                        let _ = wv.focus();
                    }
                }
                _ => {}
            },
            Event::UserEvent(ev) => match ev {
                UserEvent::Toolbar(body) => {
                    if app.on_toolbar(&body) {
                        // Buka ulang untuk memakai versi baru.
                        app.save_session();
                        if let Ok(exe) = std::env::current_exe() {
                            let _ = std::process::Command::new(exe).arg("--restarted").spawn();
                        }
                        quit = true;
                    }
                }
                UserEvent::Content(id, body) => app.on_content(id, &body),
                UserEvent::Title(id, title) => {
                    if let Some(i) = app.tab_index(id) {
                        app.tabs[i].title = title;
                        app.render();
                    }
                }
                UserEvent::PageLoad(id, ev, url) => {
                    if let Some(i) = app.tab_index(id) {
                        let t = &mut app.tabs[i];
                        t.loading = matches!(ev, PageLoadEvent::Started);
                        if !url.is_empty() {
                            t.url = url;
                        }
                        if is_newtab(&t.url) {
                            t.title = "Tab baru".into();
                        }
                        if !t.loading {
                            app.save_session();
                        }
                        app.render();
                    }
                }
                UserEvent::NewTab(url) => app.open_tab(&url, true),
                UserEvent::Navigate(id, url) => {
                    if let Some(i) = app.tab_index(id) {
                        if let Some(wv) = &app.tabs[i].webview {
                            let _ = wv.load_url(&url);
                        }
                    }
                }
                UserEvent::EngineReady(bytes) => {
                    if let Some(engine) = blocker::engine_from_bytes(&bytes) {
                        *app.blocker.borrow_mut() = Blocker::new(engine, true);
                        log("pemblokir memakai daftar filter lengkap");
                        app.render();
                    }
                }
                UserEvent::UpdateReady(v) => {
                    app.update_ready = Some(v);
                    app.render();
                }
                UserEvent::Menu(id) => quit = app.on_menu(&id),
                UserEvent::Remote(r) => match r {
                    Remote::Show => {
                        app.window.set_minimized(false);
                        app.window.set_focus();
                        app.open_tab(NEWTAB_URL, true);
                    }
                    Remote::Open(u) => {
                        app.window.set_minimized(false);
                        app.window.set_focus();
                        app.open_tab(&u, true);
                    }
                    Remote::Exit => quit = true,
                },
            },
            _ => {}
        }
        if quit {
            app.save_session();
            // Buang semua WebView sebelum keluar supaya proses WebView2 ikut berhenti.
            app.tabs.clear();
            *control_flow = ControlFlow::Exit;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_text_to_url() {
        let s = Settings::default();
        assert_eq!(to_url("kompas.com", &s), "https://kompas.com");
        assert_eq!(to_url("detik.com/berita?id=1", &s), "https://detik.com/berita?id=1");
        assert_eq!(to_url("https://a.id/x", &s), "https://a.id/x");
        assert_eq!(to_url("localhost:8080", &s), "http://localhost:8080");
        assert_eq!(to_url("192.168.1.1", &s), "http://192.168.1.1");
        assert_eq!(to_url("harga emas hari ini", &s), "https://www.google.com/search?q=harga+emas+hari+ini");
        assert_eq!(to_url("rust", &s), "https://www.google.com/search?q=rust");
        assert_eq!(to_url("v1.2", &s), "https://www.google.com/search?q=v1.2");
        assert_eq!(to_url("8.8.8.8", &s), "https://8.8.8.8");
    }

    #[test]
    fn newtab_shows_empty_address() {
        assert_eq!(display_url(NEWTAB_URL), "");
        assert_eq!(display_url("about:blank"), "");
        assert_eq!(display_url("https://a.id/"), "https://a.id/");
    }
}
