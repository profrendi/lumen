# Lumen Nightly

Browser portabel yang ringan, ditulis dengan Rust di atas mesin [Servo](https://servo.org/).
Pengaturan privasinya mengikuti gaya LibreWolf dan Brave: ketat sejak pertama dibuka, tanpa telemetry.

> **Status: eksperimental.** Servo belum sekompatibel Chromium atau Firefox, jadi sebagian situs
> tampil kurang sempurna atau tidak jalan. Jangan dipakai untuk perbankan atau akun penting.

## Cara pakai

1. Unduh arsip nightly dari halaman **Releases** (tag `nightly`).
2. Ekstrak ke mana saja, termasuk flashdisk.
3. Jalankan `lumen.exe` (Windows) atau `./lumen` (Linux).

Tidak ada instalasi dan tidak ada yang ditulis ke registry atau AppData. Semua data ada di folder
`Data` di sebelah program. Hapus folder itu untuk kembali ke keadaan awal.

| Perintah | Fungsi |
|---|---|
| `lumen` | Buka halaman awal |
| `lumen alamat.com "kata kunci"` | Buka alamat atau pencarian, satu tab per argumen |
| `lumen --private` | Mode privat: cookie dan data situs tidak ditulis ke disk |
| `lumen --version` | Tampilkan versi |

## Privasi

| Perlindungan | Keterangan |
|---|---|
| Tanpa telemetry | Browser tidak pernah menghubungi server siapa pun atas kemauannya sendiri. Halaman awal bersifat lokal. |
| Pemblokir iklan dan pelacak | Memakai [adblock-rust](https://github.com/brave/adblock-rust), mesin yang sama dengan Brave Shields. Membaca daftar format EasyList/uBlock; permintaan yang cocok dibatalkan sebelum terkirim. |
| Mode HTTPS-saja | Semua `http://` dinaikkan ke `https://`, termasuk gambar, skrip, dan iframe. |
| Hapus saat keluar | Cookie dan data situs dihapus setiap browser ditutup (bisa dimatikan). |
| Izin selalu ditolak | Kamera, mikrofon, lokasi, dan notifikasi. |
| API berisiko dimatikan | WebRTC, Geolocation, Gamepad, WebXR, Bluetooth, WebGPU, WebGL2, Service Worker, baca clipboard. |
| Parameter pelacak dibuang | `utm_*`, `fbclid`, `gclid`, dan sejenisnya dibuang dari alamat yang diketik atau ditempel. |
| Tanpa cache disk | Cache halaman hanya ada di memori. |

Tiga perlindungan pertama yang bisa diubah ada di tombol **Privasi** pada toolbar.

Yang **belum** ada: perlindungan sidik jari (fingerprinting) seperti di Brave, isolasi cookie
per situs, dan pemblokiran kosmetik (aturan `##` untuk menyembunyikan kotak iklan yang kosong).

## Pintasan keyboard

| Pintasan | Fungsi |
|---|---|
| `Ctrl+T` / `Ctrl+W` | Tab baru / tutup tab |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | Tab berikutnya / sebelumnya |
| `Ctrl+1` … `Ctrl+8`, `Ctrl+9` | Pindah ke tab ke-n / tab terakhir |
| `Ctrl+L` atau `F6` | Fokus ke kolom alamat |
| `Ctrl+R` atau `F5` | Muat ulang |
| `Alt+←` / `Alt+→` | Kembali / maju |
| `Ctrl++` / `Ctrl+-` / `Ctrl+0` | Perbesar / perkecil / ukuran normal |

## Pengaturan

`Data/settings.ini` dibuat otomatis saat pengaturan pertama kali diubah. Edit saat browser tertutup.

```ini
block_trackers = true
https_only = true
clear_on_exit = true
# Kosongkan untuk halaman awal bawaan (lokal).
homepage =
# %s diganti kata kunci pencarian.
search = https://duckduckgo.com/?q=%s
```

### Daftar filter iklan

Arsip nightly sudah menyertakan daftar filter di `Data/filters`:

| Berkas | Isi |
|---|---|
| `easylist.txt` | EasyList, daftar iklan utama |
| `easyprivacy.txt` | EasyPrivacy, pelacak dan analitik |
| `abpindo.txt` | ABPindo, iklan situs Indonesia dan Malaysia |
| `ublock-filters.txt`, `ublock-privacy.txt` | Daftar tambahan uBlock Origin |

Skrip **Perbarui daftar blokir** mengunduh versi terbarunya (alamatnya ada di `filters.list`).
Daftar sendiri cukup diletakkan sebagai `*.txt` di `Data/filters`; format hosts lama tetap bisa
dipakai sebagai `Data/blocklist.txt`. Saat pertama dibuka setelah daftar berubah, Lumen menyusun
mesin filter (sekitar satu detik) lalu menyimpannya di `Data/filters/.mesin-filter.bin`, sehingga
pembukaan berikutnya cepat. Tanpa daftar apa pun, daftar kecil bawaan di dalam program tetap aktif.

Alamat di jaringan lokal yang hanya melayani `http://` (router, NAS) perlu mode HTTPS-saja
dimatikan dulu dari tombol **Privasi**. `localhost` dikecualikan otomatis.

## Batasan

- **Video dan audio tidak diputar.** Build ini belum menyertakan GStreamer (pemutar media Servo).
  YouTube juga belum bisa walaupun GStreamer dipasang, karena Servo belum mendukung Media Source
  Extensions (MSE) yang dipakai YouTube.
- Daftar bernomor (`<ol>`) tampil dengan angka 0 untuk setiap butir: bug mesin Servo 0.8.
- **Tidak ada unduhan berkas, ekstensi, pengelola kata sandi, riwayat, atau bookmark.**
- **Unggah berkas** dilakukan dengan mengetik lokasi berkas, belum ada jendela pemilih.
- **Satu proses untuk semua tab.** Lebih hemat RAM, tetapi satu halaman yang macet bisa menjatuhkan
  seluruh browser, dan tidak ada sandbox per situs seperti di Chromium. Fitur `multiprocess` tersedia
  saat build, namun belum diuji.
- Pemilih warna (`<input type=color>`) dan IME (ketik aksara Asia Timur) belum didukung.

## Build dari sumber

Butuh Rust stabil, Python 3.11+, CMake, dan Clang.

```sh
# Linux (Debian/Ubuntu/MX Linux)
sudo apt install build-essential clang libclang-dev cmake pkg-config m4 \
  libx11-dev libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxcb-shm0-dev \
  libxkbcommon-dev libwayland-dev libegl1-mesa-dev libgl1-mesa-dev \
  libfontconfig1-dev libfreetype-dev libunwind-dev libdbus-1-dev libudev-dev libssl-dev
cargo build --release
```

```powershell
# Windows: Visual Studio Build Tools (C++), LLVM, NASM, Python
cargo build --release --features angle
```

Di Windows, salin `libEGL.dll` dan `libGLESv2.dll` dari `target\release\build\mozangle-*\out\`
ke sebelah `lumen.exe`.

Build pertama lama (Servo terdiri dari ratusan crate) dan butuh beberapa GB ruang disk.
`cargo test` menjalankan uji unit.

## Build nightly otomatis

`.github/workflows/nightly.yml` membangun arsip portabel Windows dan Linux setiap malam, juga
setiap ada perubahan di cabang `main`, lalu memasangnya di rilis bergulir bertanda `nightly`.

Versi Servo dipatok di `Cargo.toml` (`servo = "=0.8.0"`) karena API embedding-nya masih berubah
tiap rilis. Menaikkan versinya dilakukan dengan sengaja, bukan otomatis.

## Susunan kode

| Berkas | Isi |
|---|---|
| `src/main.rs` | Argumen baris perintah dan siklus hidup program |
| `src/app.rs` | Jendela, tab, delegate Servo, render, input, preferensi yang diperketat |
| `src/ui.rs` | Toolbar, tab, menu Privasi, dialog dan popup halaman (egui) |
| `src/privacy.rs` | Pemblokir iklan (adblock-rust + cache), pembersih parameter pelacak |
| `src/settings.rs` | `settings.ini` |
| `src/paths.rs` | Lokasi folder portabel dan pembersihan profil |
| `src/input.rs` | Konversi event keyboard dan mouse |

## Lisensi

[MPL-2.0](LICENSE), sama dengan Servo.
