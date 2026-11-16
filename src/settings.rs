//! Pengaturan portabel, disimpan sebagai teks `kunci = nilai` di `Data/settings.ini`.
//! Sengaja tanpa dependensi tambahan: filenya bisa diedit dengan Notepad.

use std::path::{Path, PathBuf};

pub const DEFAULT_SEARCH: &str = "https://duckduckgo.com/?q=%s";

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Blokir permintaan ke host pelacak/iklan.
    pub block_trackers: bool,
    /// Semua http:// dinaikkan ke https:// di tingkat jaringan.
    pub https_only: bool,
    /// Hapus cookie dan data situs setiap kali browser ditutup (bawaan LibreWolf).
    pub clear_on_exit: bool,
    /// Kosong = halaman awal bawaan (lokal, tanpa jaringan).
    pub homepage: String,
    /// Templat mesin pencari; `%s` diganti kata kunci.
    pub search: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            block_trackers: true,
            https_only: true,
            clear_on_exit: true,
            homepage: String::new(),
            search: DEFAULT_SEARCH.to_string(),
        }
    }
}

fn parse_bool(v: &str) -> Option<bool> {
    match v.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" | "ya" => Some(true),
        "false" | "0" | "no" | "off" | "tidak" => Some(false),
        _ => None,
    }
}

impl Settings {
    pub fn file(data_dir: &Path) -> PathBuf {
        data_dir.join("settings.ini")
    }

    /// Baris yang tidak dikenal atau nilainya rusak diabaikan (nilai bawaan dipakai).
    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') || line.starts_with('[') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let (key, value) = (key.trim(), value.trim());
            match key {
                "block_trackers" => s.block_trackers = parse_bool(value).unwrap_or(s.block_trackers),
                "https_only" => s.https_only = parse_bool(value).unwrap_or(s.https_only),
                "clear_on_exit" => s.clear_on_exit = parse_bool(value).unwrap_or(s.clear_on_exit),
                "homepage" => s.homepage = value.to_string(),
                // Templat tanpa %s tidak bisa dipakai mencari; pertahankan bawaan.
                "search" if value.contains("%s") => s.search = value.to_string(),
                _ => {}
            }
        }
        s
    }

    pub fn serialize(&self) -> String {
        format!(
            "# Pengaturan Lumen. Edit saat browser tertutup.\n\
             block_trackers = {}\n\
             https_only = {}\n\
             clear_on_exit = {}\n\
             # Kosongkan untuk halaman awal bawaan (lokal).\n\
             homepage = {}\n\
             # %s diganti kata kunci pencarian.\n\
             search = {}\n",
            self.block_trackers, self.https_only, self.clear_on_exit, self.homepage, self.search
        )
    }

    pub fn load(data_dir: &Path) -> Settings {
        match std::fs::read_to_string(Self::file(data_dir)) {
            Ok(text) => Self::parse(&text),
            Err(_) => Settings::default(),
        }
    }

    pub fn save(&self, data_dir: &Path) {
        if let Err(e) = std::fs::write(Self::file(data_dir), self.serialize()) {
            log::error!("Gagal menyimpan pengaturan: {e}");
        }
    }

    pub fn search_url(&self, query: &str) -> String {
        let q: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
        self.search.replacen("%s", &q, 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_private() {
        let s = Settings::default();
        assert!(s.block_trackers && s.https_only && s.clear_on_exit);
    }

    #[test]
    fn roundtrip() {
        let s = Settings {
            block_trackers: false,
            https_only: true,
            clear_on_exit: false,
            homepage: "https://example.com/?a=b".into(),
            search: "https://search.example/?q=%s&x=1".into(),
        };
        assert_eq!(Settings::parse(&s.serialize()), s);
    }

    #[test]
    fn bad_lines_fall_back_to_defaults() {
        let s = Settings::parse("https_only = mungkin\nsearch = https://tanpa-placeholder/\nrandom\n[bagian]\nclear_on_exit=off");
        assert!(s.https_only);
        assert_eq!(s.search, DEFAULT_SEARCH);
        assert!(!s.clear_on_exit);
    }

    #[test]
    fn search_url_encodes_query() {
        let s = Settings::default();
        assert_eq!(s.search_url("a b&c"), "https://duckduckgo.com/?q=a+b%26c");
    }
}
