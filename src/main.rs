//! Lumen Nightly: browser portabel ringan berbasis Servo, UI ditulis dengan Rust.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod input;
mod paths;
mod privacy;
mod settings;
mod ui;

use winit::event_loop::EventLoop;

const HELP: &str = "\
Pemakaian: lumen [OPSI] [ALAMAT...]

  --private    Mode privat: cookie dan data situs tidak ditulis ke disk
  --version    Tampilkan versi
  --help       Tampilkan bantuan ini

Data disimpan di folder `Data` di sebelah program (atau di folder LUMEN_DATA).";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Proses konten (hanya bila dibuild dengan fitur `multiprocess`): Servo menjalankan ulang
    // exe ini dengan `--content-process <token>`.
    #[cfg(feature = "multiprocess")]
    if let Some(pos) = args.iter().position(|a| a == "--content-process") {
        if let Some(token) = args.get(pos + 1) {
            servo::run_content_process(token.clone());
            return Ok(());
        }
    }

    let mut private = false;
    let mut urls = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--private" => private = true,
            "--version" | "-V" => {
                println!("{}", app::version_line());
                return Ok(());
            }
            "--help" | "-h" => {
                println!("{}\n\n{HELP}", app::version_line());
                return Ok(());
            }
            other if other.starts_with("--") => {
                eprintln!("Opsi tidak dikenal: {other}\n\n{HELP}");
                std::process::exit(2);
            }
            _ => urls.push(arg),
        }
    }

    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Gagal memasang penyedia kripto");

    let launch = app::Launch { paths: paths::Paths::init(), private, urls };

    let event_loop = EventLoop::with_user_event().build()?;
    let mut app = app::App::new(&event_loop, launch);
    event_loop.run_app(&mut app)?;
    app.finish();
    Ok(())
}
