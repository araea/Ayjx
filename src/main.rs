mod adapter_console;
mod adapter_napcat;

mod plugin_blacklist;
mod plugin_echo;
mod plugin_survey;

use ayjx::Ayjx;

use adapter_console::ConsoleAdapter;
use adapter_napcat::NapCatAdapter;

use plugin_blacklist::BlacklistPlugin;
use plugin_echo::EchoPlugin;
use plugin_survey::InteractivePlugin;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ayjx = Ayjx::builder()
        .config_path("config.toml")
        .data_dir("data")
        .adapter(ConsoleAdapter::default())
        .adapter(NapCatAdapter::new())
        .plugin(BlacklistPlugin)
        .plugin(EchoPlugin)
        .plugin(InteractivePlugin)
        .with_default_middlewares()
        .build();

    ayjx.run().await?;
    Ok(())
}
