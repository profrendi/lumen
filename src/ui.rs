//! UI browser (tab + toolbar + popup) digambar dengan egui di atas jendela, sedangkan halaman web
//! dirender oleh Servo ke framebuffer terpisah lalu di-blit ke area di bawah toolbar.
//! Modul ini tidak tahu apa-apa tentang Servo: ia menerima `Model` dan mengembalikan `Action`.

use egui::{Color32, Context, Key, Margin, Pos2, RichText, Ui};

pub struct TabInfo {
    pub title: String,
    pub loading: bool,
}

pub struct Model<'a> {
    pub tabs: &'a [TabInfo],
    pub active: usize,
    pub url: String,
    pub can_back: bool,
    pub can_forward: bool,
    pub blocked: u32,
    pub block_trackers: bool,
    pub https_only: bool,
    pub clear_on_exit: bool,
    pub private_mode: bool,
    pub version: &'a str,
}

pub enum Action {
    Navigate(String),
    Back,
    Forward,
    Reload,
    NewTab,
    CloseTab(usize),
    SelectTab(usize),
    SetBlockTrackers(bool),
    SetHttpsOnly(bool),
    SetClearOnExit(bool),
    ClearDataNow,
    Popup(PopupResult),
}

/// Satu baris pada dropdown `<select>`; `id: None` berarti judul grup.
pub struct SelectRow {
    pub id: Option<usize>,
    pub label: String,
    pub disabled: bool,
    pub selected: bool,
}

pub enum MenuRow {
    Item { index: usize, label: String, enabled: bool },
    Separator,
}

/// Kontrol yang diminta halaman web dan harus digambar oleh browser, bukan oleh halaman.
pub enum Popup {
    Alert { origin: String, message: String },
    Confirm { origin: String, message: String },
    Prompt { origin: String, message: String, value: String },
    FilePath { value: String },
    Select { pos: Pos2, rows: Vec<SelectRow> },
    Menu { pos: Pos2, rows: Vec<MenuRow> },
}

pub enum PopupResult {
    Accept,
    Dismiss,
    Pick(usize),
}

#[derive(Default)]
pub struct AddressBar {
    pub text: String,
    /// Diisi true oleh Ctrl+L; dibaca (dan di-reset) oleh `draw`.
    pub focus_requested: bool,
}

fn short(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

/// Menggambar chrome browser. Mengembalikan aksi pengguna dan batas bawah toolbar (dalam point).
pub fn draw(ui: &mut Ui, m: &Model, addr: &mut AddressBar, popup: &mut Option<Popup>) -> (Vec<Action>, f32) {
    let ctx = ui.ctx().clone();
    let mut actions = Vec::new();
    let bar_bg = if m.private_mode { Color32::from_rgb(44, 30, 62) } else { Color32::from_rgb(32, 33, 36) };

    let panel = egui::Panel::top("chrome")
        .frame(egui::Frame::new().fill(bar_bg).inner_margin(Margin::symmetric(8, 6)))
        .show(ui, |ui| {
            // Baris tab
            ui.horizontal(|ui| {
                for (i, tab) in m.tabs.iter().enumerate() {
                    let label = format!("{}{}", if tab.loading { "… " } else { "" }, short(&tab.title, 20));
                    if ui.selectable_label(i == m.active, label).clicked() {
                        actions.push(Action::SelectTab(i));
                    }
                    if ui.small_button("×").on_hover_text("Tutup tab (Ctrl+W)").clicked() {
                        actions.push(Action::CloseTab(i));
                    }
                    ui.add_space(4.0);
                }
                if ui.button("+").on_hover_text("Tab baru (Ctrl+T)").clicked() {
                    actions.push(Action::NewTab);
                }
                if m.private_mode {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new("Mode privat").color(Color32::from_rgb(200, 170, 255)));
                    });
                }
            });
            ui.add_space(4.0);

            // Baris navigasi
            ui.horizontal(|ui| {
                if ui.add_enabled(m.can_back, egui::Button::new("←")).on_hover_text("Kembali (Alt+←)").clicked() {
                    actions.push(Action::Back);
                }
                if ui.add_enabled(m.can_forward, egui::Button::new("→")).on_hover_text("Maju (Alt+→)").clicked() {
                    actions.push(Action::Forward);
                }
                if ui.button("↻").on_hover_text("Muat ulang (Ctrl+R / F5)").clicked() {
                    actions.push(Action::Reload);
                }

                let privacy_width = 110.0;
                let width = (ui.available_width() - privacy_width - ui.spacing().item_spacing.x).max(100.0);
                let id = egui::Id::unique("address-bar");
                let focused_before = ctx.memory(|mem| mem.has_focus(id));
                if !focused_before && !addr.focus_requested {
                    addr.text = m.url.clone();
                }
                let response = ui.add(
                    egui::TextEdit::singleline(&mut addr.text)
                        .id(id)
                        .desired_width(width)
                        .hint_text("Cari atau ketik alamat")
                        .margin(Margin::symmetric(8, 5)),
                );
                if addr.focus_requested {
                    response.request_focus();
                    addr.focus_requested = false;
                }
                if response.gained_focus() {
                    if let Some(mut state) = egui::TextEdit::load_state(&ctx, id) {
                        let end = egui::text::CCursor::new(addr.text.chars().count());
                        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                            egui::text::CCursor::new(0),
                            end,
                        )));
                        state.store(&ctx, id);
                    }
                }
                if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    actions.push(Action::Navigate(addr.text.clone()));
                }

                let label = if m.blocked > 0 {
                    RichText::new(format!("Privasi · {}", m.blocked))
                } else {
                    RichText::new("Privasi")
                };
                ui.menu_button(label, |ui| {
                    let mut trackers = m.block_trackers;
                    if ui.checkbox(&mut trackers, "Blokir pelacak & iklan").changed() {
                        actions.push(Action::SetBlockTrackers(trackers));
                    }
                    let mut https = m.https_only;
                    if ui.checkbox(&mut https, "Mode HTTPS-saja").changed() {
                        actions.push(Action::SetHttpsOnly(https));
                    }
                    let mut clear = m.clear_on_exit;
                    if ui.checkbox(&mut clear, "Hapus cookie & data situs saat keluar").changed() {
                        actions.push(Action::SetClearOnExit(clear));
                    }
                    ui.separator();
                    if ui.button("Hapus data penjelajahan sekarang").clicked() {
                        actions.push(Action::ClearDataNow);
                        ui.close();
                    }
                    ui.separator();
                    ui.label(format!("{} permintaan diblokir sesi ini", m.blocked));
                    ui.small("Tanpa telemetry. Izin kamera, mikrofon, lokasi,\ndan notifikasi selalu ditolak.");
                    ui.small(m.version);
                });
            });
        });

    if let Some(result) = draw_popup(&ctx, popup) {
        actions.push(Action::Popup(result));
    }

    (actions, panel.response.rect.max.y)
}

/// Kotak dialog diberi label asalnya supaya tidak bisa disangka pesan dari browser.
fn dialog(ctx: &Context, title: &str, body: impl FnOnce(&mut Ui) -> Option<PopupResult>) -> Option<PopupResult> {
    let mut result = None;
    let modal = egui::Modal::new(egui::Id::unique("page-dialog")).show(ctx, |ui| {
        ui.set_max_width(420.0);
        ui.label(RichText::new(title).small().weak());
        ui.add_space(6.0);
        result = body(ui);
    });
    if result.is_none() && modal.should_close() {
        result = Some(PopupResult::Dismiss);
    }
    result
}

fn ok_cancel(ui: &mut Ui, cancel: bool) -> Option<PopupResult> {
    let mut result = None;
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        if ui.button("   OK   ").clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
            result = Some(PopupResult::Accept);
        }
        if cancel && ui.button("Batal").clicked() {
            result = Some(PopupResult::Dismiss);
        }
    });
    result
}

fn floating(ctx: &Context, pos: Pos2, body: impl FnOnce(&mut Ui) -> Option<PopupResult>) -> Option<PopupResult> {
    let mut result = None;
    let area = egui::Area::new(egui::Id::unique("page-popup"))
        .order(egui::Order::Foreground)
        .constrain(true)
        .fixed_pos(pos)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    result = body(ui);
                });
            });
        });
    if result.is_none() {
        let clicked_outside = ctx.input(|i| {
            i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !area.response.rect.contains(p))
        });
        if clicked_outside || ctx.input(|i| i.key_pressed(Key::Escape)) {
            result = Some(PopupResult::Dismiss);
        }
    }
    result
}

fn draw_popup(ctx: &Context, popup: &mut Option<Popup>) -> Option<PopupResult> {
    match popup.as_mut()? {
        Popup::Alert { origin, message } => dialog(ctx, &format!("{origin} mengatakan:"), |ui| {
            ui.label(message.as_str());
            ok_cancel(ui, false)
        }),
        Popup::Confirm { origin, message } => dialog(ctx, &format!("{origin} bertanya:"), |ui| {
            ui.label(message.as_str());
            ok_cancel(ui, true)
        }),
        Popup::Prompt { origin, message, value } => dialog(ctx, &format!("{origin} meminta masukan:"), |ui| {
            ui.label(message.as_str());
            ui.add(egui::TextEdit::singleline(value).desired_width(380.0)).request_focus();
            ok_cancel(ui, true)
        }),
        Popup::FilePath { value } => dialog(ctx, "Pilih file untuk diunggah", |ui| {
            ui.label("Ketik atau tempel lokasi lengkap file:");
            ui.add(egui::TextEdit::singleline(value).desired_width(380.0).hint_text("C:\\Users\\...\\berkas.pdf"))
                .request_focus();
            ok_cancel(ui, true)
        }),
        Popup::Select { pos, rows } => floating(ctx, *pos, |ui| {
            let mut picked = None;
            for row in rows.iter() {
                match row.id {
                    None => {
                        ui.label(RichText::new(&row.label).strong());
                    }
                    Some(id) => {
                        let button = egui::Button::selectable(row.selected, row.label.as_str());
                        if ui.add_enabled(!row.disabled, button).clicked() {
                            picked = Some(PopupResult::Pick(id));
                        }
                    }
                }
            }
            picked
        }),
        Popup::Menu { pos, rows } => floating(ctx, *pos, |ui| {
            let mut picked = None;
            for row in rows.iter() {
                match row {
                    MenuRow::Separator => {
                        ui.separator();
                    }
                    MenuRow::Item { index, label, enabled } => {
                        if ui.add_enabled(*enabled, egui::Button::new(label.as_str()).frame(false)).clicked() {
                            picked = Some(PopupResult::Pick(*index));
                        }
                    }
                }
            }
            picked
        }),
    }
}

pub fn setup(ctx: &Context) {
    // Font UI bawaan (Ubuntu Light) tidak punya panah ← → ↻; Hack (juga bawaan egui) punya.
    let mut fonts = egui::FontDefinitions::default();
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        family.push("Hack".to_owned());
    }
    ctx.set_fonts(fonts);

    let mut visuals = egui::Visuals::dark();
    visuals.window_corner_radius = 8.0.into();
    ctx.set_visuals(visuals);
}
