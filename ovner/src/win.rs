//! Fungsi khusus Windows, semuanya di level pengguna (HKCU, tanpa hak admin).
//! Di platform lain berupa stub supaya proyek tetap bisa dicek dan diuji di Linux.

use std::path::Path;

pub const AUMID: &str = "Ovner.Browser";

/// Buka folder di Explorer.
pub fn open_folder(path: &Path) {
    let _ = std::process::Command::new(if cfg!(windows) { "explorer" } else { "xdg-open" })
        .arg(path)
        .spawn();
}

/// Hapus folder-folder ini beberapa detik setelah proses ini keluar.
pub fn delete_dirs_later(dirs: &[&Path]) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut script = String::from("/c ping 127.0.0.1 -n 4 >nul");
        for d in dirs {
            script.push_str(&format!(" & rmdir /s /q \"{}\"", d.display()));
        }
        let _ = std::process::Command::new("cmd")
            .raw_arg(script)
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .spawn();
    }
    #[cfg(not(windows))]
    let _ = dirs;
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::core::{Interface, HSTRING};

    fn message_box(text: &str, flags: windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE) -> i32 {
        use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_SETFOREGROUND, MB_TOPMOST};
        unsafe {
            MessageBoxW(None, &HSTRING::from(text), &HSTRING::from("Ovner"), flags | MB_TOPMOST | MB_SETFOREGROUND).0
        }
    }

    pub fn confirm(text: &str) -> bool {
        use windows::Win32::UI::WindowsAndMessaging::{IDYES, MB_ICONQUESTION, MB_YESNO};
        message_box(text, MB_YESNO | MB_ICONQUESTION) == IDYES.0
    }

    pub fn info(text: &str) {
        use windows::Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_OK};
        message_box(text, MB_OK | MB_ICONINFORMATION);
    }

    /// Identitas aplikasi untuk taskbar (ikon & pengelompokan jendela).
    pub fn init_identity() {
        use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
        let _ = unsafe { SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(AUMID)) };
    }

    /// Shortcut .lnk dengan AppUserModelID yang sama dengan jendela, supaya ikon taskbar menyatu.
    pub fn create_shortcut(lnk: &Path, exe: &Path, description: &str) -> Result<(), String> {
        use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, IPersistFile, StructuredStorage::PROPVARIANT, CLSCTX_INPROC_SERVER,
            COINIT_APARTMENTTHREADED,
        };
        use windows::Win32::UI::Shell::{IShellLinkW, PropertiesSystem::IPropertyStore, ShellLink};

        if let Some(dir) = lnk.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(|e| e.to_string())?;
            link.SetPath(&HSTRING::from(exe.as_os_str())).map_err(|e| e.to_string())?;
            if let Some(d) = exe.parent() {
                let _ = link.SetWorkingDirectory(&HSTRING::from(d.as_os_str()));
            }
            let _ = link.SetDescription(&HSTRING::from(description));
            let _ = link.SetIconLocation(&HSTRING::from(exe.as_os_str()), 0);

            let store: IPropertyStore = link.cast().map_err(|e| e.to_string())?;
            let pv = PROPVARIANT::from(AUMID);
            store.SetValue(&PKEY_AppUserModel_ID, &pv).map_err(|e| e.to_string())?;
            store.Commit().map_err(|e| e.to_string())?;

            let file: IPersistFile = link.cast().map_err(|e| e.to_string())?;
            file.Save(&HSTRING::from(lnk.as_os_str()), true).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Folder Desktop pengguna yang sebenarnya (bisa dialihkan ke OneDrive oleh kebijakan kantor).
    pub fn desktop_dir() -> Option<std::path::PathBuf> {
        use windows::Win32::System::Com::CoTaskMemFree;
        use windows::Win32::UI::Shell::{FOLDERID_Desktop, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
        unsafe {
            let p = SHGetKnownFolderPath(&FOLDERID_Desktop, KF_FLAG_DEFAULT, None).ok()?;
            let s = p.to_string().ok();
            CoTaskMemFree(Some(p.0 as *const _));
            s.map(std::path::PathBuf::from)
        }
    }

    /// Unduh URL ke file (memakai pengaturan proxy Windows, seperti Edge).
    pub fn download(url: &str, dest: &Path) -> Result<(), String> {
        use windows::Win32::System::Com::Urlmon::URLDownloadToFileW;
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            URLDownloadToFileW(None, &HSTRING::from(url), &HSTRING::from(dest.as_os_str()), 0, None)
                .map_err(|e| format!("unduh gagal: {e}"))
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    pub fn confirm(text: &str) -> bool {
        eprintln!("[konfirmasi] {text}");
        false
    }
    pub fn info(text: &str) {
        eprintln!("[info] {text}");
    }
    pub fn init_identity() {}
    pub fn create_shortcut(_lnk: &Path, _exe: &Path, _description: &str) -> Result<(), String> {
        Ok(())
    }
    pub fn desktop_dir() -> Option<std::path::PathBuf> {
        None
    }
    /// Di Linux (untuk pengujian) unduhan memakai curl.
    pub fn download(url: &str, dest: &Path) -> Result<(), String> {
        let ok = std::process::Command::new("curl")
            .args(["-fsSL", "--retry", "2", "-o"])
            .arg(dest)
            .arg(url)
            .status()
            .map_err(|e| e.to_string())?
            .success();
        if ok {
            Ok(())
        } else {
            Err("unduh gagal".into())
        }
    }
}

pub use imp::*;
