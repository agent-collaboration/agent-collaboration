//! Real backend keyboard logger. Injected input proves decoding only.
#[path = "probe_support/key_logger.rs"]
mod key_logger;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    key_logger::run_keyboard_probe().await
}
