//! Perlindungan privasi: pemblokir iklan/pelacak memakai `adblock-rust` (mesin Brave Shields)
//! dan pembersih parameter pelacak. Tidak ada telemetry, tidak ada panggilan jaringan oleh
//! browser itu sendiri: daftar filter dibaca dari disk, diperbarui lewat skrip terpisah.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::time::{Instant, UNIX_EPOCH};

use adblock::lists::{FilterFormat, FilterSet, ParseOptions};
use adblock::request::Request;
use adblock::Engine;
use url::Url;

/// Daftar bawaan kecil (format host). Selalu aktif, jadi browser tetap memblokir pelacak besar
/// walaupun folder `Data/filters` kosong.
const BUILTIN_HOSTS: &[&str] = &[
    "doubleclick.net", "googlesyndication.com", "googleadservices.com", "google-analytics.com",
    "googletagmanager.com", "googletagservices.com", "adservice.google.com", "2mdn.net",
    "connect.facebook.net", "pixel.facebook.com", "analytics.twitter.com", "ads-twitter.com",
    "ads.linkedin.com", "px.ads.linkedin.com", "bat.bing.com", "clarity.ms",
    "scorecardresearch.com", "quantserve.com", "quantcount.com", "hotjar.com", "hotjar.io",
    "mixpanel.com", "segment.io", "segment.com", "amplitude.com", "fullstory.com",
    "criteo.com", "criteo.net", "taboola.com", "outbrain.com", "adnxs.com", "rubiconproject.com",
    "pubmatic.com", "openx.net", "casalemedia.com", "moatads.com", "adsrvr.org",
    "advertising.com", "bluekai.com", "demdex.net", "omtrdc.net", "everesttech.net",
    "krxd.net", "agkn.com", "mathtag.com", "tapad.com", "yieldmo.com", "sharethrough.com",
    "smartadserver.com", "adform.net", "teads.tv", "media.net", "popads.net", "mgid.com",
];

/// Parameter URL yang hanya dipakai untuk melacak klik lintas situs (seperti daftar Brave/LibreWolf).
const TRACKING_PARAMS: &[&str] = &[
    "fbclid", "gclid", "gclsrc", "dclid", "gbraid", "wbraid", "msclkid", "yclid", "twclid", "ttclid",
    "igshid", "mc_eid", "mc_cid", "_hsenc", "_hsmi", "mkt_tok", "vero_id", "oly_enc_id", "oly_anon_id",
    "s_cid",
];

/// Naikkan angka ini bila cara menyusun mesin berubah, supaya cache lama tidak dipakai.
const CACHE_FORMAT: u32 = 1;
const CACHE_FILE: &str = ".mesin-filter.bin";
/// Format serialisasi mesin bisa berubah antar versi adblock-rust; samakan dengan Cargo.toml.
const ADBLOCK_VERSION: &str = "0.13";

fn is_tracking_param(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with("utm_") || TRACKING_PARAMS.contains(&name.as_str())
}

/// Buang parameter pelacak dari URL. Mengembalikan true bila ada yang dibuang.
pub fn strip_tracking_params(url: &mut Url) -> bool {
    if !matches!(url.scheme(), "http" | "https") || url.query().is_none() {
        return false;
    }
    let all: Vec<(String, String)> = url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
    let kept: Vec<&(String, String)> = all.iter().filter(|(k, _)| !is_tracking_param(k)).collect();
    if kept.len() == all.len() {
        return false;
    }
    if kept.is_empty() {
        url.set_query(None);
    } else {
        url.query_pairs_mut().clear().extend_pairs(kept.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    }
    true
}

/// Satu berkas daftar yang dibaca saat start.
struct ListFile {
    path: PathBuf,
    format: FilterFormat,
}

/// Daftar yang dipakai: semua `Data/filters/*.txt` (format EasyList/uBlock) dan, untuk
/// kompatibilitas, `Data/blocklist.txt` (format hosts atau satu domain per baris).
fn list_files(data_dir: &Path) -> Vec<ListFile> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(data_dir.join("filters")) {
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("txt")))
            .collect();
        paths.sort();
        files.extend(paths.into_iter().map(|path| ListFile { path, format: FilterFormat::Standard }));
    }
    let hosts = data_dir.join("blocklist.txt");
    if hosts.is_file() {
        files.push(ListFile { path: hosts, format: FilterFormat::Hosts });
    }
    files
}

/// Kunci cache: berubah bila ada daftar yang ditambah, dihapus, atau diperbarui.
fn cache_key(files: &[ListFile]) -> String {
    let mut key = format!("lumen-filter-v{CACHE_FORMAT};adblock-{ADBLOCK_VERSION}");
    for f in files {
        let meta = std::fs::metadata(&f.path).ok();
        let len = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let mtime = meta
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        key.push_str(&format!(";{}:{len}:{mtime}", f.path.file_name().unwrap_or_default().to_string_lossy()));
    }
    key
}

fn builtin_rules() -> String {
    BUILTIN_HOSTS.iter().map(|h| format!("||{h}^\n")).collect()
}

fn build_engine(files: &[ListFile]) -> (Engine, usize) {
    let mut set = FilterSet::new(false);
    set.add_filter_list(builtin_rules(), ParseOptions::default());
    let mut loaded = 0;
    for f in files {
        match std::fs::read_to_string(&f.path) {
            Ok(text) => {
                set.add_filter_list(text, ParseOptions { format: f.format, ..ParseOptions::default() });
                loaded += 1;
            }
            Err(e) => log::warn!("Gagal membaca daftar {}: {e}", f.path.display()),
        }
    }
    (Engine::new_with_filter_set(set), loaded)
}

/// Muat mesin dari cache bila daftar tidak berubah; kalau tidak, susun ulang lalu simpan cache.
fn load_engine(data_dir: &Path) -> Engine {
    let started = Instant::now();
    let files = list_files(data_dir);
    let key = cache_key(&files);
    let cache_path = data_dir.join("filters").join(CACHE_FILE);

    if let Ok(bytes) = std::fs::read(&cache_path) {
        if let Some(body) = bytes.strip_prefix(key.as_bytes()).and_then(|b| b.strip_prefix(b"\n")) {
            let mut engine = Engine::default();
            if engine.deserialize(body).is_ok() {
                log::info!("Filter iklan dimuat dari cache ({} daftar, {:?})", files.len(), started.elapsed());
                return engine;
            }
        }
    }

    let (engine, loaded) = build_engine(&files);
    log::info!("Filter iklan disusun dari {loaded} daftar ({:?})", started.elapsed());
    if loaded > 0 {
        let mut bytes = key.into_bytes();
        bytes.push(b'\n');
        bytes.extend_from_slice(&engine.serialize());
        let _ = std::fs::create_dir_all(data_dir.join("filters"));
        if let Err(e) = std::fs::write(&cache_path, bytes) {
            log::warn!("Gagal menyimpan cache filter {}: {e}", cache_path.display());
        }
    }
    engine
}

/// Jenis permintaan dalam istilah EasyList, dari `destination` Fetch milik Servo.
pub fn request_type(destination: &str) -> &'static str {
    match destination.to_ascii_lowercase().as_str() {
        "script" | "worker" | "sharedworker" | "serviceworker" | "audioworklet" | "paintworklet" | "xslt" => "script",
        "image" => "image",
        "style" => "stylesheet",
        "font" => "font",
        "audio" | "video" | "track" => "media",
        "iframe" | "frame" => "sub_frame",
        "object" | "embed" => "object",
        "document" => "main_frame",
        "report" => "ping",
        // Fetch/XHR tidak punya destination.
        "none" | "" => "xmlhttprequest",
        _ => "other",
    }
}

pub struct Privacy {
    engine: Engine,
    pub blocked_count: Cell<u32>,
}

impl Privacy {
    pub fn load(data_dir: &Path) -> Privacy {
        Privacy { engine: load_engine(data_dir), blocked_count: Cell::new(0) }
    }

    /// `true` bila sub-resource ini harus dibatalkan.
    /// `source` adalah alamat halaman yang memuatnya (untuk aturan `$third-party`/`$domain=`).
    pub fn should_block(&self, url: &str, source: &str, request_type: &str) -> bool {
        match Request::new(url, source, request_type, "GET") {
            Ok(req) => self.engine.check_network_request(&req).should_block(),
            Err(_) => false,
        }
    }
}

/// Alamat lokal/privat (server dev, router, NAS): alamat tanpa skema yang diketik memakai http://.
pub fn is_local_host(host: &str) -> bool {
    host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host == "::1"
        || host == "[::1]"
        || host.starts_with("127.")
        || host.starts_with("10.")
        || host.starts_with("192.168.")
        || (host.starts_with("172.")
            && host.split('.').nth(1).and_then(|n| n.parse::<u8>().ok()).is_some_and(|n| (16..=31).contains(&n)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lumen-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const PAGE: &str = "https://berita.contoh.id/artikel";

    #[test]
    fn builtin_list_blocks_trackers_and_subdomains() {
        let p = Privacy::load(Path::new("/nonexistent"));
        assert!(p.should_block("https://www.googletagmanager.com/gtag/js?id=G-1", PAGE, "script"));
        assert!(p.should_block("https://stats.g.doubleclick.net/x", PAGE, "image"));
        assert!(!p.should_block("https://notdoubleclick.net/x", PAGE, "image"));
        assert!(!p.should_block("https://cdn.contoh.id/app.js", PAGE, "script"));
    }

    #[test]
    fn easylist_syntax_rules_apply() {
        let dir = temp_dir("easylist");
        std::fs::create_dir_all(dir.join("filters")).unwrap();
        std::fs::write(
            dir.join("filters/uji.txt"),
            "! Title: uji\n/ads/banner.\n||iklan.contoh.id^$third-party\n@@||contoh.id/ads/banner.js$script\n||video-iklan.example^$media\n",
        )
        .unwrap();
        let p = Privacy::load(&dir);
        // Aturan path (gaya EasyList)
        assert!(p.should_block("https://situs.lain/ads/banner.js", PAGE, "script"));
        // Pengecualian @@ menang
        assert!(!p.should_block("https://contoh.id/ads/banner.js", PAGE, "script"));
        // $third-party: diblokir dari situs lain, dibiarkan di situs sendiri
        assert!(p.should_block("https://iklan.contoh.id/a.png", "https://situs.lain/", "image"));
        assert!(!p.should_block("https://iklan.contoh.id/a.png", "https://iklan.contoh.id/", "image"));
        // Opsi jenis: $media hanya untuk audio/video
        assert!(p.should_block("https://video-iklan.example/x.mp4", PAGE, "media"));
        assert!(!p.should_block("https://video-iklan.example/x.png", PAGE, "image"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn hosts_file_still_supported() {
        let dir = temp_dir("hosts");
        std::fs::write(
            dir.join("blocklist.txt"),
            "# komentar\n0.0.0.0 pelacak.contoh.id\n127.0.0.1 localhost\n0.0.0.0 0.0.0.0\n",
        )
        .unwrap();
        let p = Privacy::load(&dir);
        assert!(p.should_block("https://pelacak.contoh.id/p.gif", PAGE, "image"));
        assert!(!p.should_block("http://localhost:8080/app.js", "http://localhost:8080/", "script"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cache_is_written_and_reused_then_invalidated() {
        let dir = temp_dir("cache");
        std::fs::create_dir_all(dir.join("filters")).unwrap();
        std::fs::write(dir.join("filters/a.txt"), "||satu.example^\n").unwrap();
        let first = Privacy::load(&dir);
        assert!(dir.join("filters").join(CACHE_FILE).is_file());
        assert!(first.should_block("https://satu.example/x.js", PAGE, "script"));

        // Dimuat dari cache: hasil sama.
        let cached = Privacy::load(&dir);
        assert!(cached.should_block("https://satu.example/x.js", PAGE, "script"));

        // Daftar berubah -> cache lama tidak dipakai.
        std::fs::write(dir.join("filters/a.txt"), "||dua.example^\n! ukuran berbeda\n").unwrap();
        let rebuilt = Privacy::load(&dir);
        assert!(!rebuilt.should_block("https://satu.example/x.js", PAGE, "script"));
        assert!(rebuilt.should_block("https://dua.example/x.js", PAGE, "script"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn request_types_map_from_servo_destinations() {
        assert_eq!(request_type("Script"), "script");
        assert_eq!(request_type("Image"), "image");
        assert_eq!(request_type("Style"), "stylesheet");
        assert_eq!(request_type("Video"), "media");
        assert_eq!(request_type("IFrame"), "sub_frame");
        assert_eq!(request_type("None"), "xmlhttprequest");
    }

    #[test]
    fn strips_only_tracking_params() {
        let mut u = Url::parse("https://example.com/p?id=7&utm_source=x&fbclid=abc&q=a+b#top").unwrap();
        assert!(strip_tracking_params(&mut u));
        assert_eq!(u.as_str(), "https://example.com/p?id=7&q=a+b#top");

        let mut only = Url::parse("https://example.com/?UTM_Campaign=z&gclid=1").unwrap();
        assert!(strip_tracking_params(&mut only));
        assert_eq!(only.as_str(), "https://example.com/");

        let mut clean = Url::parse("https://example.com/?id=7").unwrap();
        assert!(!strip_tracking_params(&mut clean));
        assert_eq!(clean.as_str(), "https://example.com/?id=7");
    }

    #[test]
    fn local_hosts_are_recognised() {
        assert!(is_local_host("localhost"));
        assert!(is_local_host("192.168.1.1"));
        assert!(is_local_host("172.20.0.5"));
        assert!(!is_local_host("172.32.0.5"));
        assert!(!is_local_host("example.com"));
    }
}
