//! Inti browser: jendela, tab, delegate Servo, dan loop render.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::{Duration, Instant};

use euclid::default::{Point2D, Rect, Size2D};
use euclid::Scale;
use glow::HasContext as _;
use servo::{
    ClipboardAction, ContextMenuItem, DevicePoint, EmbedderControl, EmbedderControlId, EventLoopWaker,
    InputEvent, LoadStatus, MouseButtonAction, MouseButtonEvent, MouseMoveEvent, OffscreenRenderingContext,
    PermissionRequest, PrefValue, Preferences, RenderingContext, SelectElementOptionOrOptgroup, Servo,
    ServoBuilder, SimpleDialog, StorageType, WebResourceLoad, WebResourceResponse, WebView, WebViewBuilder,
    WebViewDelegate, WebViewPoint, WheelDelta, WheelEvent, WheelMode, WindowRenderingContext,
};
use url::Url;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key as WKey, ModifiersState, NamedKey as WNamed};
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::{Window, WindowId};

use crate::input;
use crate::paths::Paths;
use crate::privacy::{self, Privacy};
use crate::settings::Settings;
use crate::ui::{self, Action, AddressBar, MenuRow, Popup, PopupResult, SelectRow, TabInfo};

/// Jarak antar-frame saat halaman sedang beranimasi (~60 fps).
const FRAME: Duration = Duration::from_millis(16);

/// Apa yang diminta pengguna lewat baris perintah.
pub struct Launch {
    pub paths: Paths,
    /// Mode privat: tidak ada cookie/penyimpanan yang ditulis ke disk.
    pub private: bool,
    pub urls: Vec<String>,
}

pub fn version_line() -> String {
    let build = option_env!("LUMEN_BUILD").unwrap_or("build lokal");
    let ua = Preferences::default().user_agent;
    let engine = ua.split_whitespace().find(|t| t.starts_with("Servo/")).unwrap_or("Servo").replace('/', " ");
    format!("Lumen Nightly {} ({build}) · {engine}", env!("CARGO_PKG_VERSION"))
}

/// Preferensi Servo yang diperketat. Prinsipnya mengikuti LibreWolf: API yang membuka akses
/// perangkat atau menambah permukaan sidik jari dimatikan, tidak ada cache di disk.
pub fn hardened_preferences(settings: &Settings) -> Preferences {
    let mut p = Preferences::default();

    // Akses perangkat & sidik jari: mati.
    p.dom_gamepad_enabled = false;
    p.dom_webxr_enabled = false;
    p.dom_bluetooth_enabled = false;
    p.dom_geolocation_enabled = false;
    p.dom_notification_enabled = false;
    p.dom_wakelock_enabled = false;
    p.dom_webrtc_enabled = false; // WebRTC bisa membocorkan alamat IP lokal
    p.dom_webgl2_enabled = false;
    p.dom_webgpu_enabled = false;
    p.dom_async_clipboard_enabled = false; // halaman tidak boleh membaca clipboard
    p.dom_serviceworker_enabled = false;
    p.dom_navigator_protocol_handlers_enabled = false;
    p.dom_credential_management_enabled = false;

    // Tidak ada port debug yang terbuka, tidak ada cache di disk.
    p.devtools_server_enabled = false;
    p.network_http_disk_cache = String::new();

    // HTTPS-saja ditegakkan Servo di lapisan jaringan (termasuk gambar, skrip, iframe).
    p.network_enforce_tls_enabled = settings.https_only;

    // Fitur web yang dibutuhkan situs modern dan tidak menambah risiko privasi.
    p.dom_fontface_enabled = true;
    p.dom_intersection_observer_enabled = true;
    p.dom_exec_command_enabled = true;
    p.dom_offscreen_canvas_enabled = true;
    p.dom_sanitizer_enabled = true;
    p.dom_permissions_enabled = true;
    p.dom_storage_manager_api_enabled = true;
    p.dom_indexeddb_enabled = true;
    p.layout_css_columns_enabled = true;
    p.layout_css_font_variations_enabled = true;
    p
}

fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Halaman awal bawaan: sepenuhnya lokal (data: URL), jadi membuka browser tidak menghubungi siapa pun.
pub fn home_url(settings: &Settings, private: bool) -> Url {
    let template = settings.search.replace('\\', "\\\\").replace('"', "\\\"").replace('<', "\\u003c");
    let mode = if private { "Mode privat: tidak ada yang disimpan ke disk." } else { "" };
    let html = format!(
        r#"<!doctype html><meta charset="utf-8"><title>Tab baru</title>
<style>
html,body{{height:100%;margin:0}}
body{{background:#1b1c1f;color:#e8eaed;font:16px system-ui,"Segoe UI",sans-serif;display:flex;align-items:center;justify-content:center}}
main{{width:min(560px,86vw);text-align:center;margin-bottom:12vh}}
h1{{font-weight:600;font-size:40px;letter-spacing:.02em;margin:0 0 6px}}
h1 span{{color:#f2b84b}}
p{{color:#9aa0a6;font-size:13px;margin:6px 0}}
input{{width:100%;box-sizing:border-box;margin:22px 0 14px;padding:13px 18px;border-radius:24px;border:1px solid #3c4043;background:#26272b;color:#e8eaed;font-size:16px;outline:none}}
input:focus{{border-color:#f2b84b}}
</style>
<main>
<h1>Lu<span>men</span></h1>
<p>Nightly</p>
<form id="f"><input id="q" autofocus autocomplete="off" placeholder="Cari di web"></form>
<p>Pelacak diblokir · HTTPS-saja · tanpa telemetry</p>
<p>{mode}</p>
</main>
<script>
document.getElementById("f").addEventListener("submit", function (e) {{
  e.preventDefault();
  var q = document.getElementById("q").value.trim();
  if (q) location.href = "{template}".replace("%s", encodeURIComponent(q));
}});
</script>"#
    );
    Url::parse(&format!("data:text/html;charset=utf-8,{}", percent_encode(&html))).expect("URL halaman awal")
}

// ---------------------------------------------------------------------------------------------
// Pembangun event loop <-> Servo
// ---------------------------------------------------------------------------------------------

#[derive(Debug)]
pub struct WakerEvent;

#[derive(Clone)]
pub struct Waker(EventLoopProxy<WakerEvent>);

impl Waker {
    pub fn new(event_loop: &EventLoop<WakerEvent>) -> Self {
        Self(event_loop.create_proxy())
    }
}

impl EventLoopWaker for Waker {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(self.clone())
    }
    fn wake(&self) {
        let _ = self.0.send_event(WakerEvent);
    }
}

// ---------------------------------------------------------------------------------------------
// Status UI egui
// ---------------------------------------------------------------------------------------------

struct EguiUi {
    ctx: egui::Context,
    state: egui_winit::State,
    painter: egui_glow::Painter,
    address: AddressBar,
    popup: Option<Popup>,
}

// ---------------------------------------------------------------------------------------------
// Browser
// ---------------------------------------------------------------------------------------------

// Urutan field menentukan urutan drop: tab dan kontrol dulu, lalu Servo, lalu konteks GL, dan
// jendela paling akhir (konteks GL masih memegang handle jendela).
pub struct Browser {
    me: Weak<Browser>,
    tabs: RefCell<Vec<WebView>>,
    /// Kontrol halaman (dialog, dropdown, menu konteks) yang sedang menunggu jawaban pengguna.
    control: RefCell<Option<EmbedderControl>>,
    servo: Servo,
    egui: RefCell<EguiUi>,
    content_ctx: Rc<OffscreenRenderingContext>,
    window_ctx: Rc<WindowRenderingContext>,
    active: Cell<usize>,
    privacy: Privacy,
    settings: RefCell<Settings>,
    paths: Paths,
    private: bool,
    home: Url,
    version: String,
    modifiers: Cell<ModifiersState>,
    cursor_pos: Cell<(f64, f64)>,
    toolbar_px: Cell<u32>,
    content_size: Cell<PhysicalSize<u32>>,
    web_mouse_down: Cell<bool>,
    window_title: RefCell<String>,
    /// Servo punya frame baru yang belum digambar ke framebuffer konten.
    needs_paint: Cell<bool>,
    next_frame: Cell<Instant>,
    want_exit: Cell<bool>,
    window: Window,
}

impl Browser {
    fn new(event_loop: &ActiveEventLoop, waker: Waker, launch: Launch) -> Rc<Browser> {
        let Launch { paths, private, urls } = launch;
        let settings = Settings::load(&paths.data);
        let window = event_loop
            .create_window(
                Window::default_attributes()
                    .with_title(if private { "Lumen Nightly (privat)" } else { "Lumen Nightly" })
                    .with_inner_size(LogicalSize::new(1200.0, 720.0))
                    .with_min_inner_size(LogicalSize::new(480.0, 320.0)),
            )
            .expect("Gagal membuat jendela");

        let display_handle = event_loop.display_handle().expect("display handle");
        let window_handle = window.window_handle().expect("window handle");
        let size = window.inner_size();

        let window_ctx = Rc::new(
            WindowRenderingContext::new(display_handle, window_handle, size)
                .expect("Gagal membuat konteks render (driver OpenGL/ANGLE?)"),
        );
        let _ = window_ctx.make_current();

        let content_ctx = Rc::new(window_ctx.offscreen_context(PhysicalSize::new(size.width.max(1), size.height.max(1))));

        // Profil portabel: seluruh data Servo diarahkan ke folder Data/profile.
        // Mode privat: tidak ada folder profil sama sekali, penyimpanan hanya sementara.
        let mut opts = servo::Opts::default();
        opts.config_dir = (!private).then(|| paths.profile.clone());
        opts.temporary_storage = private;
        opts.multiprocess = cfg!(feature = "multiprocess");
        let servo = ServoBuilder::default()
            .opts(opts)
            .preferences(hardened_preferences(&settings))
            .event_loop_waker(Box::new(waker))
            .build();
        servo.setup_logging();

        // egui di atas GL context jendela yang sama.
        let gl: Arc<glow::Context> = window_ctx.glow_gl_api();
        let egui_ctx = egui::Context::default();
        ui::setup(&egui_ctx);
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );
        let painter = egui_glow::Painter::new(gl, "", None, false).expect("Gagal membuat painter egui");

        let privacy = Privacy::load(&paths.data);
        let home = home_url(&settings, private);

        let browser = Rc::new_cyclic(|me| Browser {
            me: me.clone(),
            tabs: RefCell::new(Vec::new()),
            control: RefCell::new(None),
            servo,
            egui: RefCell::new(EguiUi {
                ctx: egui_ctx,
                state: egui_state,
                painter,
                address: AddressBar::default(),
                popup: None,
            }),
            content_ctx,
            window_ctx,
            active: Cell::new(0),
            privacy,
            settings: RefCell::new(settings),
            paths,
            private,
            home,
            version: version_line(),
            modifiers: Cell::new(ModifiersState::empty()),
            cursor_pos: Cell::new((0.0, 0.0)),
            toolbar_px: Cell::new(90),
            content_size: Cell::new(PhysicalSize::new(1, 1)),
            web_mouse_down: Cell::new(false),
            window_title: RefCell::new(String::new()),
            needs_paint: Cell::new(true),
            next_frame: Cell::new(Instant::now()),
            want_exit: Cell::new(false),
            window,
        });

        let targets: Vec<Url> = urls.iter().filter_map(|u| browser.resolve_input(u)).collect();
        if targets.is_empty() {
            browser.open_tab(Some(browser.start_url()));
        } else {
            for url in targets {
                browser.open_tab(Some(url));
            }
        }
        browser
    }

    /// Halaman awal: `homepage` di settings.ini bila diisi, kalau tidak halaman bawaan (lokal).
    fn start_url(&self) -> Url {
        let custom = self.settings.borrow().homepage.clone();
        self.resolve_input(&custom).unwrap_or_else(|| self.home.clone())
    }

    fn resolve_input(&self, input: &str) -> Option<Url> {
        normalize_input(input, &self.settings.borrow())
    }

    /// Hapus cookie, penyimpanan situs, dan cache memori untuk semua situs.
    fn clear_browsing_data(&self) {
        let manager = self.servo.site_data_manager();
        let types = StorageType::Cookies | StorageType::Local | StorageType::Session;
        let sites: Vec<String> = manager.site_data(types).iter().map(|s| s.name()).collect();
        let names: Vec<&str> = sites.iter().map(String::as_str).collect();
        manager.clear_site_data(&names, types);
        manager.clear_cookies(None);
        self.servo.network_manager().clear_cache();
        self.privacy.blocked_count.set(0);
    }

    /// Dipanggil sekali saat keluar: lepaskan semua yang memegang `Rc<Browser>` supaya Servo bisa
    /// mati dengan rapi (menulis lalu menutup berkasnya) sebelum profil dibersihkan.
    fn shutdown(&self) {
        self.settings.borrow().save(&self.paths.data);
        if self.settings.borrow().clear_on_exit && !self.private {
            self.clear_browsing_data();
        }
        self.dismiss_control();
        self.tabs.borrow_mut().clear();
        let _ = self.window_ctx.make_current();
        self.egui.borrow_mut().painter.destroy();
    }

    fn scale(&self) -> Scale<f32, servo::DeviceIndependentPixel, servo::DevicePixel> {
        Scale::new(self.window.scale_factor() as f32)
    }

    fn active_webview(&self) -> Option<WebView> {
        self.tabs.borrow().get(self.active.get()).cloned()
    }

    fn open_tab(&self, url: Option<Url>) {
        let me = self.me.upgrade().expect("browser dropped");
        let mut builder = WebViewBuilder::new(&self.servo, self.content_ctx.clone())
            .hidpi_scale_factor(self.scale())
            .delegate(me);
        if let Some(url) = url {
            builder = builder.url(url);
        }
        let webview = builder.build();
        self.add_tab(webview);
    }

    fn add_tab(&self, webview: WebView) {
        if let Some(current) = self.active_webview() {
            current.hide();
        }
        let idx = {
            let mut tabs = self.tabs.borrow_mut();
            tabs.push(webview.clone());
            tabs.len() - 1
        };
        self.active.set(idx);
        webview.show();
        webview.set_focused(true);
        webview.resize(self.content_size.get());
        self.repaint_content();
    }

    /// Minta halaman digambar ulang (frame Servo baru, ganti tab, ganti ukuran).
    fn repaint_content(&self) {
        self.needs_paint.set(true);
        self.window.request_redraw();
    }

    fn select_tab(&self, idx: usize) {
        if idx >= self.tabs.borrow().len() || idx == self.active.get() {
            return;
        }
        self.dismiss_control();
        if let Some(current) = self.active_webview() {
            current.hide();
            current.set_focused(false);
        }
        self.active.set(idx);
        if let Some(wv) = self.active_webview() {
            wv.show();
            wv.set_focused(true);
            wv.resize(self.content_size.get());
        }
        self.repaint_content();
    }

    fn close_tab(&self, idx: usize) {
        if idx == self.active.get() {
            self.dismiss_control();
        }
        let removed = {
            let mut tabs = self.tabs.borrow_mut();
            if idx >= tabs.len() {
                return;
            }
            tabs.remove(idx)
        };
        drop(removed);
        let len = self.tabs.borrow().len();
        if len == 0 {
            self.want_exit.set(true);
            return;
        }
        let active = self.active.get();
        let new_active = if idx < active || active >= len { active.saturating_sub(1).min(len - 1) } else { active };
        self.active.set(new_active);
        if let Some(wv) = self.active_webview() {
            wv.show();
            wv.set_focused(true);
            wv.resize(self.content_size.get());
        }
        self.repaint_content();
    }

    fn cycle_tab(&self, forward: bool) {
        let len = self.tabs.borrow().len();
        if len < 2 {
            return;
        }
        let cur = self.active.get();
        let next = if forward { (cur + 1) % len } else { (cur + len - 1) % len };
        self.select_tab(next);
    }

    fn navigate(&self, input: &str) {
        let Some(url) = self.resolve_input(input) else { return };
        if let Some(wv) = self.active_webview() {
            wv.load(url);
            wv.set_focused(true);
        }
    }

    fn apply(&self, action: Action) {
        match action {
            Action::Navigate(text) => self.navigate(&text),
            Action::Back => {
                if let Some(wv) = self.active_webview() {
                    if wv.can_go_back() {
                        wv.go_back(1);
                    }
                }
            }
            Action::Forward => {
                if let Some(wv) = self.active_webview() {
                    if wv.can_go_forward() {
                        wv.go_forward(1);
                    }
                }
            }
            Action::Reload => {
                if let Some(wv) = self.active_webview() {
                    wv.reload();
                }
            }
            Action::NewTab => {
                self.open_tab(Some(self.start_url()));
                self.egui.borrow_mut().address.focus_requested = true;
            }
            Action::CloseTab(i) => self.close_tab(i),
            Action::SelectTab(i) => self.select_tab(i),
            Action::SetBlockTrackers(v) => self.update_settings(|s| s.block_trackers = v),
            Action::SetHttpsOnly(v) => {
                self.update_settings(|s| s.https_only = v);
                self.servo.set_preference("network_enforce_tls_enabled", PrefValue::Bool(v));
            }
            Action::SetClearOnExit(v) => self.update_settings(|s| s.clear_on_exit = v),
            Action::ClearDataNow => self.clear_browsing_data(),
            Action::Popup(result) => self.resolve_control(result),
        }
    }

    fn update_settings(&self, change: impl FnOnce(&mut Settings)) {
        let mut settings = self.settings.borrow_mut();
        change(&mut settings);
        settings.save(&self.paths.data);
    }

    // -----------------------------------------------------------------------------------------
    // Kontrol halaman: dialog, dropdown <select>, menu konteks, pemilih file
    // -----------------------------------------------------------------------------------------

    /// Titik (piksel perangkat, relatif area konten) -> posisi egui (point, relatif jendela).
    fn content_to_ui(&self, x: i32, y: i32) -> egui::Pos2 {
        let ppp = self.window.scale_factor() as f32;
        egui::pos2(x as f32 / ppp, (y as f32 + self.toolbar_px.get() as f32) / ppp)
    }

    fn present_control(&self, control: EmbedderControl) {
        let origin = self
            .active_webview()
            .and_then(|wv| wv.url())
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_else(|| "Halaman ini".to_string());
        let popup = match &control {
            EmbedderControl::SimpleDialog(SimpleDialog::Alert(d)) => {
                Popup::Alert { origin, message: d.message().to_string() }
            }
            EmbedderControl::SimpleDialog(SimpleDialog::Confirm(d)) => {
                Popup::Confirm { origin, message: d.message().to_string() }
            }
            EmbedderControl::SimpleDialog(SimpleDialog::Prompt(d)) => {
                Popup::Prompt { origin, message: d.message().to_string(), value: d.current_value().to_string() }
            }
            EmbedderControl::FilePicker(_) => Popup::FilePath { value: String::new() },
            EmbedderControl::SelectElement(select) => {
                let chosen = select.selected_options();
                let row = |o: &servo::SelectElementOption| SelectRow {
                    id: Some(o.id),
                    label: o.label.clone(),
                    disabled: o.is_disabled,
                    selected: chosen.contains(&o.id),
                };
                let mut rows = Vec::new();
                for entry in select.options() {
                    match entry {
                        SelectElementOptionOrOptgroup::Option(o) => rows.push(row(o)),
                        SelectElementOptionOrOptgroup::Optgroup { label, options } => {
                            rows.push(SelectRow { id: None, label: label.clone(), disabled: true, selected: false });
                            rows.extend(options.iter().map(row));
                        }
                    }
                }
                let rect = select.position();
                Popup::Select { pos: self.content_to_ui(rect.min.x, rect.max.y), rows }
            }
            EmbedderControl::ContextMenu(menu) => {
                let rows = menu
                    .items()
                    .iter()
                    .enumerate()
                    .map(|(index, item)| match item {
                        ContextMenuItem::Item { label, enabled, .. } => {
                            MenuRow::Item { index, label: label.clone(), enabled: *enabled }
                        }
                        ContextMenuItem::Separator => MenuRow::Separator,
                    })
                    .collect();
                let rect = menu.position();
                Popup::Menu { pos: self.content_to_ui(rect.min.x, rect.min.y), rows }
            }
            // Pemilih warna dan IME belum didukung: dilepas, Servo memakai jawaban bawaannya.
            _ => return,
        };
        self.dismiss_control();
        *self.control.borrow_mut() = Some(control);
        self.egui.borrow_mut().popup = Some(popup);
        self.window.request_redraw();
    }

    /// Tutup kontrol yang sedang tampil tanpa jawaban (Servo menerima jawaban bawaan saat di-drop).
    fn dismiss_control(&self) {
        let control = self.control.borrow_mut().take();
        self.egui.borrow_mut().popup = None;
        drop(control);
    }

    fn resolve_control(&self, result: PopupResult) {
        let control = self.control.borrow_mut().take();
        let popup = self.egui.borrow_mut().popup.take();
        match (control, result) {
            (Some(EmbedderControl::SimpleDialog(SimpleDialog::Prompt(mut dialog))), PopupResult::Accept) => {
                if let Some(Popup::Prompt { value, .. }) = &popup {
                    dialog.set_current_value(value);
                }
                dialog.confirm();
            }
            (Some(EmbedderControl::SimpleDialog(dialog)), PopupResult::Accept) => dialog.confirm(),
            (Some(EmbedderControl::SimpleDialog(dialog)), _) => dialog.dismiss(),
            (Some(EmbedderControl::SelectElement(mut select)), PopupResult::Pick(id)) => {
                select.select(vec![id]);
                select.submit();
            }
            (Some(EmbedderControl::ContextMenu(menu)), PopupResult::Pick(index)) => {
                let action = match menu.items().get(index) {
                    Some(ContextMenuItem::Item { action, .. }) => Some(*action),
                    _ => None,
                };
                match action {
                    Some(action) => menu.select(action),
                    None => menu.dismiss(),
                }
            }
            (Some(EmbedderControl::FilePicker(mut picker)), PopupResult::Accept) => {
                let path = match &popup {
                    Some(Popup::FilePath { value }) => PathBuf::from(value.trim().trim_matches('"')),
                    _ => PathBuf::new(),
                };
                if path.is_file() {
                    picker.select(&[path]);
                    picker.submit();
                } else {
                    picker.dismiss();
                }
            }
            // Sisanya (batal, klik di luar): drop mengirim jawaban bawaan.
            _ => {}
        }
        self.window.request_redraw();
    }

    fn control_open(&self) -> bool {
        self.control.borrow().is_some()
    }

    // -----------------------------------------------------------------------------------------
    // Render
    // -----------------------------------------------------------------------------------------

    fn redraw(&self) {
        self.servo.spin_event_loop();
        let win_size = self.window.inner_size();
        if win_size.width == 0 || win_size.height == 0 {
            return;
        }

        // Model UI dari state Servo
        let infos: Vec<TabInfo> = self
            .tabs
            .borrow()
            .iter()
            .map(|wv| TabInfo {
                title: wv
                    .page_title()
                    .filter(|t| !t.is_empty())
                    .or_else(|| wv.url().and_then(|u| u.host_str().map(str::to_string)))
                    .unwrap_or_else(|| "Tab baru".to_string()),
                loading: wv.load_status() != LoadStatus::Complete,
            })
            .collect();
        let active_wv = self.active_webview();
        // Halaman awal bawaan adalah data: URL panjang; address bar dibiarkan kosong untuknya.
        let url = active_wv
            .as_ref()
            .and_then(|wv| wv.url())
            .filter(|u| *u != self.home && u.as_str() != "about:blank")
            .map(|u| u.to_string())
            .unwrap_or_default();
        let settings = self.settings.borrow().clone();
        let model = ui::Model {
            tabs: &infos,
            active: self.active.get(),
            url,
            can_back: active_wv.as_ref().is_some_and(|wv| wv.can_go_back()),
            can_forward: active_wv.as_ref().is_some_and(|wv| wv.can_go_forward()),
            blocked: self.privacy.blocked_count.get(),
            block_trackers: settings.block_trackers,
            https_only: settings.https_only,
            clear_on_exit: settings.clear_on_exit,
            private_mode: self.private,
            version: &self.version,
        };

        // Jalankan egui
        let ppp = self.window.scale_factor() as f32;
        let (mut full_output, actions, bottom_pts) = {
            let mut e = self.egui.borrow_mut();
            let raw_input = e.state.take_egui_input(&self.window);
            let ctx = e.ctx.clone();
            let mut actions = Vec::new();
            let mut bottom = 0.0f32;
            let EguiUi { address, popup, .. } = &mut *e;
            let output = ctx.run_ui(raw_input, |ui| {
                let (a, b) = ui::draw(ui, &model, address, popup);
                actions = a;
                bottom = b;
            });
            (output, actions, bottom)
        };
        {
            let mut e = self.egui.borrow_mut();
            e.state.handle_platform_output(&self.window, full_output.platform_output.clone());
        }
        self.toolbar_px.set((bottom_pts * ppp).round() as u32);
        for action in actions {
            self.apply(action);
        }
        if let Some(tab) = infos.get(self.active.get()) {
            let title = format!("{} — Lumen Nightly", tab.title);
            if *self.window_title.borrow() != title {
                self.window.set_title(&title);
                *self.window_title.borrow_mut() = title;
            }
        }
        // egui mengatur kursor untuk UI-nya sendiri; di atas halaman web, kursor milik Servo.
        if self.web_point().is_some() {
            if let Some(wv) = self.active_webview() {
                self.window.set_cursor(input::cursor_icon(wv.cursor()));
            }
        }

        // Ukuran area konten = jendela dikurangi toolbar
        let top = self.toolbar_px.get().min(win_size.height.saturating_sub(1));
        let content = PhysicalSize::new(win_size.width.max(1), (win_size.height - top).max(1));
        if content != self.content_size.get() {
            self.content_size.set(content);
            if let Some(wv) = self.active_webview() {
                wv.resize(content);
            }
            self.needs_paint.set(true);
        }

        // Gambar: halaman web ke framebuffer konten -> latar -> salin halaman -> egui di atasnya
        let _ = self.window_ctx.make_current();
        let active = self.active_webview();
        // Hanya minta Servo menggambar bila ada frame baru; selain itu (mis. kursor bergerak
        // di atas toolbar) cukup salin ulang framebuffer konten yang sudah ada.
        if let Some(wv) = &active {
            if self.needs_paint.replace(false) {
                wv.paint();
            }
        }
        // `paint()` meninggalkan framebuffer konten dalam keadaan terikat. Ikat lagi framebuffer
        // jendela sebelum menggambar apa pun: salinan di bawah membersihkan framebuffer yang
        // sedang terikat, dan tanpa ini yang terhapus justru isi halaman (layar hitam).
        self.window_ctx.prepare_for_rendering();
        let gl = self.window_ctx.glow_gl_api();
        unsafe {
            gl.viewport(0, 0, win_size.width as i32, win_size.height as i32);
            gl.clear_color(0.12, 0.12, 0.13, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
        }
        if active.is_some() {
            if let Some(blit) = self.content_ctx.render_to_parent_callback() {
                // Koordinat GL berasal dari kiri-bawah, jadi area konten (di bawah toolbar) mulai dari (0,0).
                let target = Rect::new(Point2D::new(0, 0), Size2D::new(content.width as i32, content.height as i32));
                blit(&gl, target);
            }
        }
        {
            let mut e = self.egui.borrow_mut();
            let clipped = e.ctx.tessellate(full_output.shapes, full_output.pixels_per_point);
            e.painter.paint_and_update_textures(
                [win_size.width, win_size.height],
                full_output.pixels_per_point,
                &clipped,
                &mut full_output.textures_delta,
            );
        }
        self.window_ctx.present();

        // Jadwalkan gambar ulang bila egui butuh animasi
        if full_output
            .viewport_output
            .values()
            .any(|v| v.repaint_delay.is_zero())
        {
            self.window.request_redraw();
        }
    }

    // -----------------------------------------------------------------------------------------
    // Input
    // -----------------------------------------------------------------------------------------

    /// Posisi kursor di dalam halaman web, atau None bila kursor ada di atas UI browser
    /// (toolbar, menu Privasi yang terbuka, popup) atau sebuah dialog halaman sedang tampil.
    fn web_point(&self) -> Option<DevicePoint> {
        if self.control_open() {
            return None;
        }
        let (x, y) = self.cursor_pos.get();
        let top = self.toolbar_px.get() as f64;
        if y < top {
            return None;
        }
        let over_ui = {
            let e = self.egui.borrow();
            let ppp = e.ctx.pixels_per_point();
            let layer = e.ctx.layer_id_at(egui::pos2(x as f32 / ppp, y as f32 / ppp));
            layer.is_some_and(|l| l.order != egui::Order::Background)
        };
        (!over_ui).then(|| DevicePoint::new(x as f32, (y - top) as f32))
    }

    fn egui_wants_keyboard(&self) -> bool {
        self.control_open() || self.egui.borrow().ctx.egui_wants_keyboard_input()
    }

    /// Pintasan keyboard browser. Mengembalikan true bila event sudah ditangani.
    fn shortcut(&self, event: &winit::event::KeyEvent) -> bool {
        if event.state != ElementState::Pressed || event.repeat {
            return false;
        }
        let m = self.modifiers.get();
        let ctrl = m.control_key();
        let alt = m.alt_key();
        let shift = m.shift_key();

        match &event.logical_key {
            WKey::Named(WNamed::F5) => {
                self.apply(Action::Reload);
                return true;
            }
            WKey::Named(WNamed::F6) => {
                self.egui.borrow_mut().address.focus_requested = true;
                return true;
            }
            WKey::Named(WNamed::ArrowLeft) if alt => {
                self.apply(Action::Back);
                return true;
            }
            WKey::Named(WNamed::ArrowRight) if alt => {
                self.apply(Action::Forward);
                return true;
            }
            WKey::Named(WNamed::Tab) if ctrl => {
                self.cycle_tab(!shift);
                return true;
            }
            _ => {}
        }
        if !ctrl {
            return false;
        }
        let WKey::Character(c) = &event.logical_key else { return false };
        match c.to_lowercase().as_str() {
            "t" => self.apply(Action::NewTab),
            "w" => self.close_tab(self.active.get()),
            "l" => self.egui.borrow_mut().address.focus_requested = true,
            "r" => self.apply(Action::Reload),
            "=" | "+" => self.zoom(1.1),
            "-" => self.zoom(1.0 / 1.1),
            "0" => {
                if let Some(wv) = self.active_webview() {
                    wv.set_page_zoom(1.0);
                }
            }
            "c" | "x" | "v" if !self.egui_wants_keyboard() => {
                let action = match c.to_lowercase().as_str() {
                    "c" => ClipboardAction::Copy,
                    "x" => ClipboardAction::Cut,
                    _ => ClipboardAction::Paste,
                };
                if let Some(wv) = self.active_webview() {
                    wv.notify_input_event(InputEvent::EditingAction(action));
                }
            }
            s if s.len() == 1 && s.as_bytes()[0].is_ascii_digit() && s != "0" => {
                let n = (s.as_bytes()[0] - b'1') as usize;
                let len = self.tabs.borrow().len();
                self.select_tab(if n == 8 { len.saturating_sub(1) } else { n });
            }
            _ => return false,
        }
        true
    }

    fn zoom(&self, factor: f32) {
        if let Some(wv) = self.active_webview() {
            wv.set_page_zoom((wv.page_zoom() * factor).clamp(0.3, 5.0));
        }
    }

    fn window_event(&self, event_loop: &ActiveEventLoop, event: WindowEvent) {
        self.servo.spin_event_loop();

        // egui dulu: toolbar/address bar mendapat kesempatan pertama. Ketikan hanya diteruskan ke
        // egui bila kolom alamat atau sebuah dialog memegang fokus; kalau tidak, Tab yang ditekan
        // di dalam halaman web akan ikut memindahkan fokus ke toolbar.
        let for_egui = match &event {
            WindowEvent::KeyboardInput { .. } | WindowEvent::Ime(_) => self.egui_wants_keyboard(),
            _ => true,
        };
        let response = if for_egui {
            let mut e = self.egui.borrow_mut();
            let e = &mut *e;
            e.state.on_window_event(&self.window, &event)
        } else {
            egui_winit::EventResponse { consumed: false, repaint: false }
        };
        if response.repaint {
            self.window.request_redraw();
        }

        match event {
            WindowEvent::CloseRequested => self.want_exit.set(true),
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::Resized(size) => {
                if size.width > 0 && size.height > 0 {
                    self.window_ctx.resize(size);
                }
                self.window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                for wv in self.tabs.borrow().iter() {
                    wv.set_hidpi_scale_factor(Scale::new(scale_factor as f32));
                }
                self.repaint_content();
            }
            WindowEvent::ModifiersChanged(m) => self.modifiers.set(m.state()),
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_pos.set((position.x, position.y));
                if let (Some(p), Some(wv)) = (self.web_point(), self.active_webview()) {
                    wv.notify_input_event(InputEvent::MouseMove(MouseMoveEvent::new(WebViewPoint::Device(p))));
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let action = match state {
                    ElementState::Pressed => MouseButtonAction::Down,
                    ElementState::Released => MouseButtonAction::Up,
                };
                let point = match (action, self.web_point()) {
                    (_, Some(p)) => Some(p),
                    // Tombol dilepas di atas toolbar setelah ditekan di halaman: tetap kirim Up.
                    (MouseButtonAction::Up, None) if self.web_mouse_down.get() => {
                        Some(DevicePoint::new(self.cursor_pos.get().0 as f32, 0.0))
                    }
                    _ => None,
                };
                if let (Some(p), Some(wv)) = (point, self.active_webview()) {
                    match action {
                        MouseButtonAction::Down => {
                            self.web_mouse_down.set(true);
                            // Klik di halaman = alamat tidak lagi memegang fokus keyboard.
                            let ctx = self.egui.borrow().ctx.clone();
                            ctx.memory_mut(|mem| {
                                if let Some(id) = mem.focused() {
                                    mem.surrender_focus(id);
                                }
                            });
                            wv.set_focused(true);
                        }
                        MouseButtonAction::Up => self.web_mouse_down.set(false),
                    }
                    wv.notify_input_event(InputEvent::MouseButton(MouseButtonEvent::new(
                        action,
                        input::mouse_button(button),
                        WebViewPoint::Device(p),
                    )));
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if let (Some(p), Some(wv)) = (self.web_point(), self.active_webview()) {
                    let (dx, dy, mode) = match delta {
                        MouseScrollDelta::LineDelta(dx, dy) => ((dx * 76.0) as f64, (dy * 76.0) as f64, WheelMode::DeltaLine),
                        MouseScrollDelta::PixelDelta(d) => (d.x, d.y, WheelMode::DeltaPixel),
                    };
                    wv.notify_input_event(InputEvent::Wheel(WheelEvent::new(
                        WheelDelta { x: dx, y: dy, z: 0.0, mode },
                        WebViewPoint::Device(p),
                    )));
                }
            }
            WindowEvent::KeyboardInput { event: key, .. } => {
                if self.shortcut(&key) {
                    self.window.request_redraw();
                } else if !response.consumed && !self.egui_wants_keyboard() {
                    if let (Some(ev), Some(wv)) = (input::keyboard_event(&key, self.modifiers.get()), self.active_webview()) {
                        wv.notify_input_event(InputEvent::Keyboard(ev));
                    }
                }
            }
            _ => {}
        }

        if self.want_exit.get() {
            event_loop.exit();
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Delegate Servo
// ---------------------------------------------------------------------------------------------

impl WebViewDelegate for Browser {
    fn notify_new_frame_ready(&self, webview: WebView) {
        if self.active_webview().is_some_and(|active| active.id() == webview.id()) {
            self.repaint_content();
        }
    }
    fn notify_animating_changed(&self, _: WebView, _: bool) {
        self.window.request_redraw();
    }
    fn notify_crashed(&self, _: WebView, reason: String, _: Option<String>) {
        log::error!("Halaman macet: {reason}");
        self.window.request_redraw();
    }
    fn notify_url_changed(&self, _: WebView, _: Url) {
        self.window.request_redraw();
    }
    fn notify_page_title_changed(&self, _: WebView, _: Option<String>) {
        self.window.request_redraw();
    }
    fn notify_load_status_changed(&self, _: WebView, _: LoadStatus) {
        self.window.request_redraw();
    }
    fn notify_history_changed(&self, _: WebView, _: Vec<Url>, _: usize) {
        self.window.request_redraw();
    }
    fn notify_cursor_changed(&self, _: WebView, cursor: servo::Cursor) {
        if self.web_point().is_some() {
            self.window.set_cursor(input::cursor_icon(cursor));
        }
    }
    fn notify_closed(&self, webview: WebView) {
        let idx = self.tabs.borrow().iter().position(|t| t.id() == webview.id());
        if let Some(idx) = idx {
            self.close_tab(idx);
        }
    }

    /// Dialog, dropdown, menu konteks, dan pemilih file digambar oleh UI browser.
    fn show_embedder_control(&self, webview: WebView, control: EmbedderControl) {
        // Tab di latar belakang tidak boleh memunculkan dialog; dilepas = jawaban bawaan.
        if self.active_webview().is_some_and(|active| active.id() == webview.id()) {
            self.present_control(control);
        }
    }

    fn hide_embedder_control(&self, _: WebView, id: EmbedderControlId) {
        if self.control.borrow().as_ref().is_some_and(|c| c.id() == id) {
            self.dismiss_control();
            self.window.request_redraw();
        }
    }

    /// Privasi-dulu: semua permintaan izin (kamera, mikrofon, lokasi, notifikasi, dll.) ditolak.
    fn request_permission(&self, _: WebView, request: PermissionRequest) {
        request.deny();
    }

    /// Link dengan target=_blank / window.open() membuka tab baru.
    fn request_create_new(&self, _parent: WebView, request: servo::CreateNewWebViewRequest) {
        let me = self.me.upgrade().expect("browser dropped");
        let webview = request
            .builder(self.content_ctx.clone())
            .hidpi_scale_factor(self.scale())
            .delegate(me)
            .build();
        self.add_tab(webview);
    }

    /// Pemblokir iklan/pelacak (adblock-rust): batalkan sub-resource yang cocok dengan daftar filter.
    fn load_web_resource(&self, webview: WebView, load: WebResourceLoad) {
        if !self.settings.borrow().block_trackers {
            return;
        }
        let req = load.request();
        if req.is_for_main_frame || !matches!(req.url.scheme(), "http" | "https") {
            return;
        }
        // Halaman sumber menentukan aturan $third-party dan $domain=.
        let source = webview
            .url()
            .map(|u| u.to_string())
            .or_else(|| req.referrer_url.as_ref().map(|u| u.as_str().to_string()))
            .unwrap_or_default();
        let kind = privacy::request_type(&format!("{:?}", req.destination));
        if self.privacy.should_block(req.url.as_str(), &source, kind) {
            self.privacy.blocked_count.set(self.privacy.blocked_count.get() + 1);
            log::info!("Diblokir: [{kind}] {}", req.url.as_str());
            let url = req.url.clone().into_url();
            load.intercept(WebResourceResponse::new(url)).cancel();
            self.window.request_redraw();
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Input address bar -> URL
// ---------------------------------------------------------------------------------------------

/// URL lengkap dipakai apa adanya; "contoh.com" -> https://contoh.com; selain itu dicari lewat
/// mesin pencari di pengaturan. Parameter pelacak (utm_*, fbclid, ...) dibuang dari alamat.
pub fn normalize_input(input: &str, settings: &Settings) -> Option<Url> {
    let mut url = parse_input(input, settings)?;
    privacy::strip_tracking_params(&mut url);
    Some(url)
}

fn parse_input(input: &str, settings: &Settings) -> Option<Url> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(u) = Url::parse(s) {
        if matches!(u.scheme(), "http" | "https" | "file" | "about" | "data") && (u.has_host() || u.scheme() != "http") {
            return Some(u);
        }
    }
    let looks_like_host = !s.contains(char::is_whitespace)
        && (s.contains('.') || s.starts_with("localhost"))
        && !s.starts_with('.')
        && !s.ends_with('.');
    if looks_like_host {
        let scheme = if privacy::is_local_host(s.split(['/', ':']).next().unwrap_or("")) { "http" } else { "https" };
        if let Ok(u) = Url::parse(&format!("{scheme}://{s}")) {
            return Some(u);
        }
    }
    Url::parse(&settings.search_url(s)).ok()
}

// ---------------------------------------------------------------------------------------------
// Aplikasi winit
// ---------------------------------------------------------------------------------------------

pub enum App {
    Initial(Waker, Option<Launch>),
    Running(Rc<Browser>),
    Finished,
}

impl App {
    pub fn new(event_loop: &EventLoop<WakerEvent>, launch: Launch) -> Self {
        App::Initial(Waker::new(event_loop), Some(launch))
    }

    /// Dipanggil setelah event loop berhenti. Mematikan Servo dengan rapi, lalu (bila "hapus saat
    /// keluar" aktif) mengosongkan folder profil sehingga tidak ada jejak sesi yang tertinggal.
    pub fn finish(&mut self) {
        let App::Running(browser) = std::mem::replace(self, App::Finished) else { return };
        let wipe = browser.settings.borrow().clear_on_exit && !browser.private;
        let profile = browser.paths.profile.clone();
        browser.shutdown();
        match Rc::try_unwrap(browser) {
            // Drop Browser -> drop Servo, yang menunggu semua thread-nya selesai menulis.
            Ok(browser) => drop(browser),
            Err(_) => log::warn!("Browser masih direferensikan saat keluar"),
        }
        if wipe {
            crate::paths::wipe_profile(&profile);
        }
    }
}

impl ApplicationHandler<WakerEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let App::Initial(waker, launch) = self {
            let launch = launch.take().expect("launch");
            let browser = Browser::new(event_loop, waker.clone(), launch);
            *self = App::Running(browser);
        }
    }

    /// Selama halaman beranimasi, Servo harus diputar dan digambar secara berkala.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let App::Running(b) = self else { return };
        let animating = b.active_webview().is_some_and(|wv| wv.animating());
        if !animating {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }
        let now = Instant::now();
        if now >= b.next_frame.get() {
            b.next_frame.set(now + FRAME);
            b.servo.spin_event_loop();
            b.repaint_content();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(b.next_frame.get()));
    }

    fn user_event(&mut self, _: &ActiveEventLoop, _: WakerEvent) {
        if let App::Running(b) = self {
            b.servo.spin_event_loop();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if let App::Running(b) = self {
            b.window_event(event_loop, event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{hardened_preferences, home_url, Settings};

    fn normalize_input(input: &str) -> Option<url::Url> {
        super::normalize_input(input, &Settings::default())
    }

    #[test]
    fn full_urls_kept() {
        assert_eq!(normalize_input("https://example.com/a?b=1").unwrap().as_str(), "https://example.com/a?b=1");
    }

    #[test]
    fn bare_domain_gets_https() {
        assert_eq!(normalize_input("example.com").unwrap().as_str(), "https://example.com/");
    }

    #[test]
    fn localhost_gets_http() {
        assert_eq!(normalize_input("localhost:3000").unwrap().as_str(), "http://localhost:3000/");
    }

    #[test]
    fn text_becomes_search() {
        let u = normalize_input("cara membuat browser rust").unwrap();
        assert_eq!(u.host_str(), Some("duckduckgo.com"));
        assert!(u.as_str().contains("q=cara+membuat+browser+rust"));
    }

    #[test]
    fn empty_is_none() {
        assert!(normalize_input("   ").is_none());
    }

    #[test]
    fn typed_urls_lose_tracking_params() {
        let u = normalize_input("example.com/artikel?id=3&utm_source=wa&fbclid=x").unwrap();
        assert_eq!(u.as_str(), "https://example.com/artikel?id=3");
    }

    #[test]
    fn custom_search_engine_is_used() {
        let settings = Settings { search: "https://cari.contoh/?k=%s".into(), ..Settings::default() };
        let u = super::normalize_input("dua kata", &settings).unwrap();
        assert_eq!(u.as_str(), "https://cari.contoh/?k=dua+kata");
    }

    #[test]
    fn hardened_preferences_follow_settings() {
        let on = hardened_preferences(&Settings::default());
        assert!(on.network_enforce_tls_enabled);
        assert!(!on.dom_webrtc_enabled && !on.dom_geolocation_enabled && !on.dom_notification_enabled);
        assert!(!on.devtools_server_enabled && on.network_http_disk_cache.is_empty());

        let off = hardened_preferences(&Settings { https_only: false, ..Settings::default() });
        assert!(!off.network_enforce_tls_enabled);
    }

    #[test]
    fn home_page_is_local_and_carries_search_template() {
        let settings = Settings { search: "https://cari.contoh/?k=%s".into(), ..Settings::default() };
        let u = home_url(&settings, false);
        assert_eq!(u.scheme(), "data");
        // Tanda kutip di templat tidak boleh bisa keluar dari string JavaScript.
        let hostile = Settings { search: "https://x/?q=%s\";alert(1);//".into(), ..Settings::default() };
        let page = home_url(&hostile, false).to_string();
        assert!(page.contains("%5C%22%3Balert") && !page.contains("%3D%s%22%3Balert"));
        assert!(u.to_string().contains("cari.contoh"));
    }
}
