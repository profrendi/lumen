//! Mode portabel: semua data (profil, cookie, blocklist, pengaturan) disimpan di folder `Data`
//! di sebelah file .exe. Tidak menulis ke AppData/registry. Bisa dipindah lewat flashdisk.
//! Folder bisa diganti dengan variabel lingkungan `LUMEN_DATA`.

use std::path::{Path, PathBuf};

pub struct Paths {
    pub data: PathBuf,
    pub profile: PathBuf,
}

impl Paths {
    pub fn init() -> Paths {
        let data = std::env::var_os("LUMEN_DATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join("Data")
            });
        let profile = data.join("profile");
        if let Err(e) = std::fs::create_dir_all(&profile) {
            log::error!("Gagal membuat folder data {}: {e}", profile.display());
        }
        Paths { data, profile }
    }
}

/// Daftar HSTS dipertahankan saat profil dibersihkan: isinya hanya "situs ini wajib HTTPS",
/// dan menghapusnya justru menurunkan keamanan kunjungan berikutnya.
const KEEP_ON_WIPE: &[&str] = &["hsts_list.json"];

/// Kosongkan folder profil (cookie, penyimpanan situs, cache otentikasi). Dipanggil setelah
/// Servo benar-benar berhenti, jadi tidak ada berkas yang masih ditulis.
pub fn wipe_profile(profile: &Path) {
    let Ok(entries) = std::fs::read_dir(profile) else { return };
    for entry in entries.flatten() {
        if KEEP_ON_WIPE.iter().any(|keep| entry.file_name() == *keep) {
            continue;
        }
        let path = entry.path();
        let result = if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
        if let Err(e) = result {
            log::warn!("Gagal menghapus {}: {e}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wipe_removes_everything_but_hsts() {
        let dir = std::env::temp_dir().join(format!("lumen-wipe-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("storage/sub")).unwrap();
        std::fs::write(dir.join("cookie_jar.json"), "{}").unwrap();
        std::fs::write(dir.join("storage/sub/data.bin"), "x").unwrap();
        std::fs::write(dir.join("hsts_list.json"), "{}").unwrap();

        wipe_profile(&dir);

        let left: Vec<String> =
            std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(left, vec!["hsts_list.json".to_string()]);
    }
}
