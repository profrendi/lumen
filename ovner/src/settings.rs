//! Pengaturan sederhana di `settings.ini` (kunci = nilai), bisa diubah dengan Notepad.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// google, duckduckgo, bing, brave, atau URL berisi `%s`.
    pub search_engine: String,
    /// Tab yang tidak dibuka selama ini (menit) ditidurkan: halamannya dibuang dari RAM dan
    /// dimuat ulang saat diklik. 0 = tidak pernah.
    pub sleep_tabs_minutes: u64,
    pub block_ads: bool,
    /// Flag hemat memori untuk mesin WebView2 (V8 mengutamakan ukuran memori).
    pub save_ram: bool,
    /// Buka lagi tab terakhir saat Ovner dibuka (semuanya tidur kecuali tab aktif).
    pub restore_tabs: bool,
    pub check_updates: bool,
    /// Situs (host) tempat pemblokir iklan dimatikan.
    pub exceptions: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            search_engine: "google".into(),
            sleep_tabs_minutes: 15,
            block_ads: true,
            save_ram: true,
            restore_tabs: true,
            check_updates: true,
            exceptions: Vec::new(),
        }
    }
}

fn yes(v: &str) -> Option<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "ya" | "yes" | "true" | "1" | "on" | "aktif" => Some(true),
        "tidak" | "no" | "false" | "0" | "off" | "mati" => Some(false),
        _ => None,
    }
}

fn yn(b: bool) -> &'static str {
    if b {
        "ya"
    } else {
        "tidak"
    }
}

impl Settings {
    pub fn path(data_dir: &Path) -> PathBuf {
        data_dir.join("settings.ini")
    }

    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
            match k.as_str() {
                "mesin_cari" if !v.is_empty() => s.search_engine = v.to_string(),
                "tidurkan_tab_menit" => s.sleep_tabs_minutes = v.parse().unwrap_or(s.sleep_tabs_minutes),
                "blokir_iklan" => s.block_ads = yes(v).unwrap_or(s.block_ads),
                "hemat_ram" => s.save_ram = yes(v).unwrap_or(s.save_ram),
                "pulihkan_tab" => s.restore_tabs = yes(v).unwrap_or(s.restore_tabs),
                "periksa_update" => s.check_updates = yes(v).unwrap_or(s.check_updates),
                "pengecualian_iklan" => {
                    s.exceptions = v
                        .split(',')
                        .map(|h| h.trim().trim_start_matches("www.").to_ascii_lowercase())
                        .filter(|h| !h.is_empty())
                        .collect()
                }
                _ => {}
            }
        }
        s
    }

    pub fn load(data_dir: &Path) -> Settings {
        std::fs::read_to_string(Self::path(data_dir)).map(|t| Self::parse(&t)).unwrap_or_default()
    }

    pub fn to_ini(&self) -> String {
        format!(
            "# Pengaturan Ovner. Tutup Ovner sebelum mengubah file ini.\n\
             # mesin_cari: google, duckduckgo, bing, brave, atau URL dengan %s (contoh: https://www.ecosia.org/search?q=%s)\n\
             mesin_cari = {}\n\
             # Tab yang tidak dibuka selama sekian menit ditidurkan supaya RAM lega (0 = tidak pernah)\n\
             tidurkan_tab_menit = {}\n\
             blokir_iklan = {}\n\
             # Situs yang pemblokir iklannya dimatikan, pisahkan dengan koma\n\
             pengecualian_iklan = {}\n\
             hemat_ram = {}\n\
             pulihkan_tab = {}\n\
             periksa_update = {}\n",
            self.search_engine,
            self.sleep_tabs_minutes,
            yn(self.block_ads),
            self.exceptions.join(", "),
            yn(self.save_ram),
            yn(self.restore_tabs),
            yn(self.check_updates),
        )
    }

    pub fn save(&self, data_dir: &Path) {
        let _ = std::fs::create_dir_all(data_dir);
        let _ = std::fs::write(Self::path(data_dir), self.to_ini());
    }

    /// URL pencarian untuk teks yang diketik.
    pub fn search_url(&self, query: &str) -> String {
        let template = match self.search_engine.to_ascii_lowercase().as_str() {
            "duckduckgo" | "ddg" => "https://duckduckgo.com/?q=%s",
            "bing" => "https://www.bing.com/search?q=%s",
            "brave" => "https://search.brave.com/search?q=%s",
            "google" => "https://www.google.com/search?q=%s",
            other if other.contains("%s") && other.starts_with("http") => self.search_engine.as_str(),
            _ => "https://www.google.com/search?q=%s",
        };
        let q: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
        template.replacen("%s", &q, 1)
    }

    pub fn is_exception(&self, host: &str) -> bool {
        let host = host.trim_start_matches("www.").to_ascii_lowercase();
        self.exceptions.iter().any(|e| host == *e || host.ends_with(&format!(".{e}")))
    }

    /// Nyalakan/matikan pemblokir di situs ini. Mengembalikan true bila sekarang dikecualikan.
    pub fn toggle_exception(&mut self, host: &str) -> bool {
        let host = host.trim_start_matches("www.").to_ascii_lowercase();
        if host.is_empty() {
            return false;
        }
        if self.is_exception(&host) {
            self.exceptions.retain(|e| !(host == *e || host.ends_with(&format!(".{e}"))));
            false
        } else {
            self.exceptions.push(host);
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_defaults() {
        let mut s = Settings::default();
        s.exceptions = vec!["kompas.com".into()];
        s.sleep_tabs_minutes = 5;
        assert_eq!(Settings::parse(&s.to_ini()), s);
        assert_eq!(Settings::parse("ngawur\nblokir_iklan = mungkin\n"), Settings::default());
    }

    #[test]
    fn search_urls() {
        let s = Settings::default();
        assert_eq!(s.search_url("harga cabai"), "https://www.google.com/search?q=harga+cabai");
        let s = Settings { search_engine: "https://x.id/?q=%s".into(), ..Settings::default() };
        assert_eq!(s.search_url("a&b"), "https://x.id/?q=a%26b");
    }

    #[test]
    fn exceptions_match_subdomains_and_toggle() {
        let mut s = Settings::default();
        assert!(s.toggle_exception("www.kompas.com"));
        assert!(s.is_exception("kompas.com"));
        assert!(s.is_exception("money.kompas.com"));
        assert!(!s.is_exception("notkompas.com"));
        assert!(!s.toggle_exception("money.kompas.com"));
        assert!(!s.is_exception("kompas.com"));
    }
}
