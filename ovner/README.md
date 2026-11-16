# Ovner

Browser ringan untuk Windows 10/11, ditulis dengan Rust.

- **Mesin**: Microsoft Edge WebView2, mesin Chromium yang sudah ada di Windows 11. Semua situs
  (YouTube, Office online, Outlook web, situs bank) tampil seperti di Edge.
- **Pemblokir iklan**: [adblock-rust](https://github.com/brave/adblock-rust), mesin yang sama
  dengan Brave Shields, memakai daftar EasyList, EasyPrivacy, ABPindo (iklan situs Indonesia),
  dan uBlock. Permintaan iklan dibatalkan sebelum terkirim, dan kotak iklan yang kosong
  disembunyikan.
- **Hemat RAM**: tab yang tidak dibuka selama 15 menit ditidurkan (halamannya dibuang dari
  RAM dan dimuat ulang saat diklik). Tab yang memutar suara tidak ditidurkan. Tab di
  belakang memakai mode memori rendah WebView2, dan fitur Chromium yang tidak perlu
  (proses cadangan, terjemahan, cast) dimatikan.
- **Tanpa hak admin**: terpasang per pengguna, runtime Visual C++ sudah tertanam di exe.

## Pasang

1. Unduh **Ovner-Setup.exe** dari halaman [Releases](https://github.com/profrendi/ovner/releases/latest).
2. Jalankan. Ovner menyalin dirinya ke `%LOCALAPPDATA%\Programs\Ovner`, membuat ikon di
   **Start Menu** dan **Desktop**, lalu terbuka.
3. Kalau Windows SmartScreen muncul: **More info → Run anyway** (exe belum ditandatangani).

Ovner memeriksa versi baru saat dibuka. Kalau ada, tombol hijau **Update** muncul di
toolbar; klik untuk membuka ulang dengan versi baru.

Hapus lewat **Settings → Apps → Installed apps → Ovner → Uninstall**.

## Versi portabel

Unduh **Ovner-Portable.zip**, ekstrak ke folder mana saja (termasuk flashdisk), jalankan
`ovner.exe`. File `portable.txt` di sebelahnya membuat semua data (cookie, login, riwayat,
daftar filter) disimpan di folder `Data` di sebelah exe, dan tidak ada yang dipasang.

## Pintasan

| Tombol | Fungsi |
|---|---|
| Ctrl+T / Ctrl+W | Tab baru / tutup tab |
| Ctrl+Shift+T | Buka lagi tab yang ditutup |
| Ctrl+Tab, Ctrl+Shift+Tab | Tab berikut / sebelumnya |
| Ctrl+1 … Ctrl+8, Ctrl+9 | Pindah ke tab ke-N / tab terakhir |
| Ctrl+L, Alt+D, F6 | Ke kolom alamat |
| Alt+← / Alt+→, F5 | Kembali / maju, muat ulang |
| Ctrl+F, Ctrl+P, Ctrl+J | Cari di halaman, cetak, unduhan |
| Ctrl+ +/−/0 | Perbesar / perkecil / normal |
| Klik tengah pada tab | Tutup tab |

## Tameng (pemblokir iklan)

Angka di ikon tameng oranye adalah jumlah iklan dan pelacak yang diblokir di halaman itu.
Kalau ada situs yang rusak karena pemblokir, klik tameng untuk mematikannya **di situs itu
saja**; klik lagi untuk menyalakan.

Daftar filter diunduh saat Ovner pertama dibuka, lalu diperbarui seminggu sekali
(menu ☰ → *Perbarui daftar filter iklan* untuk memperbarui sekarang).

## Pengaturan

Menu ☰ → *Pengaturan* membuka `settings.ini` di Notepad. Tutup Ovner dulu sebelum menyimpan.

| Kunci | Bawaan | Arti |
|---|---|---|
| `mesin_cari` | `google` | `google`, `duckduckgo`, `bing`, `brave`, atau URL dengan `%s` |
| `tidurkan_tab_menit` | `15` | Menit sebelum tab di belakang ditidurkan (`0` = tidak pernah) |
| `blokir_iklan` | `ya` | Pemblokir iklan dan pelacak |
| `pengecualian_iklan` | | Situs tanpa pemblokir, pisahkan dengan koma |
| `hemat_ram` | `ya` | Mesin JavaScript mengutamakan memori kecil |
| `pulihkan_tab` | `ya` | Buka lagi tab terakhir (semuanya tidur sampai diklik) |
| `periksa_update` | `ya` | Periksa versi baru saat dibuka |

## Batasan

- RAM per tab yang **aktif** kurang lebih sama dengan Edge, karena mesinnya sama.
  Penghematan datang dari iklan yang tidak dimuat dan tab yang ditidurkan.
- Belum ada riwayat, bookmark, atau sinkronisasi; pengisi sandi bawaan WebView2 tetap bisa dipakai.
- Ekstensi Chrome belum didukung.
- Ovner belum bisa dijadikan browser default Windows.

## Build sendiri

```
cd ovner
cargo build --release
```

Proyek ini juga bisa dikompilasi dan diuji di Linux (WebKitGTK) untuk pengembangan;
pemblokir jaringan, tidur tab yang memutar suara, dan menu ☰ hanya aktif di Windows.
