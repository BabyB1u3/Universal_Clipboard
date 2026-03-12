mod app;
mod clipboard;
mod pairing_service;

fn main() -> anyhow::Result<()> {
    app::run()
}