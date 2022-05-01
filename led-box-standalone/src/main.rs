use std::sync::Arc;

use parking_lot::Mutex;

mod app;
mod box_status;

use box_status::handle_box;

fn to_device_name(spi: &tokio_serial::SerialPortInfo) -> String {
    let name = spi.port_name.clone();
    // This is necessary on linux:
    name.replace("/sys/class/tty/", "/dev/")
}

fn main() -> anyhow::Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "info");
    }
    env_tracing_logger::init();

    let (tx, cmd_rx) = tokio::sync::mpsc::channel(10);

    let available_ports = tokio_serial::available_ports()?
        .iter()
        .map(to_device_name)
        .filter(|x| x != "/dev/ttyS0")
        .collect();
    let box_manager = Arc::new(Mutex::new(box_status::BoxManager::new()));

    let _tokio_join_handle = {
        let box_manager = box_manager.clone();
        std::thread::Builder::new()
            .name("tokio-thread".to_string())
            .spawn(move || {
                // launch single-threaded tokio runner in this thread
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(async { handle_box(box_manager, cmd_rx).await })
            })
    };

    let app = app::TemplateApp::new(available_ports, box_manager, tx);
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(Box::new(app), native_options);
}
