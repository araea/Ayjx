mod adapter_console;
mod adapter_napcat;

mod plugin_blacklist;
mod plugin_echo;
mod plugin_logger;

use ayjx::Ayjx;

use adapter_console::ConsoleAdapter;
use adapter_napcat::NapCatAdapter;

use plugin_blacklist::BlacklistPlugin;
use plugin_echo::EchoPlugin;
use plugin_logger::ConsoleLoggerPlugin;

use ayjx::prelude::*;

#[tokio::main]
async fn main() -> AyjxResult<()> {
    let ayjx = Ayjx::builder()
        .adapter(ConsoleAdapter::default())
        .adapter(NapCatAdapter::new())
        .plugin(BlacklistPlugin)
        .plugin(ConsoleLoggerPlugin::new())
        .plugin(EchoPlugin)
        .build();

    ayjx.run().await?;

    Ok(())
}
