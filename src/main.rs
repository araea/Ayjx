mod adapter_console;
mod adapter_napcat;

mod plugin_blacklist;
mod plugin_echo;
mod plugin_logger;
mod plugin_survey;

use ayjx::Ayjx;

use adapter_console::ConsoleAdapter;
use adapter_napcat::NapCatAdapter;

use plugin_blacklist::BlacklistPlugin;
use plugin_echo::EchoPlugin;
use plugin_logger::ConsoleLoggerPlugin;
use plugin_survey::InteractivePlugin;

use ayjx::prelude::*;

#[tokio::main]
async fn main() -> AyjxResult<()> {
    let ayjx = Ayjx::builder()
        .config_path("config.toml")
        .data_dir("data")
        .adapter(ConsoleAdapter::default())
        .adapter(NapCatAdapter::new())
        .plugin(BlacklistPlugin)
        .plugin(ConsoleLoggerPlugin::new())
        .plugin(EchoPlugin)
        .plugin(InteractivePlugin)
        .with_default_middlewares()
        .build();

    ayjx.run().await?;

    Ok(())
}
