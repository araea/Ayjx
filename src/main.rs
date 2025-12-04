mod adapter_console;
mod adapter_napcat;

mod plugin_blacklist;
mod plugin_echo;
mod plugin_group_self_title;
mod plugin_logger;
mod plugin_recall;
mod plugin_repeater;

use adapter_console::ConsoleAdapter;
use adapter_napcat::NapCatAdapter;

use plugin_blacklist::BlacklistPlugin;
use plugin_echo::EchoPlugin;
use plugin_group_self_title::SelfTitlePlugin;
use plugin_logger::ConsoleLoggerPlugin;
use plugin_recall::RecallPlugin;
use plugin_repeater::RepeaterPlugin;

use ayjx::prelude::*;

#[tokio::main]
async fn main() -> AyjxResult<()> {
    let ayjx = Ayjx::builder()
        .adapter(ConsoleAdapter::default())
        .adapter(NapCatAdapter::new())
        .plugin(BlacklistPlugin)
        .plugin(ConsoleLoggerPlugin::new())
        .plugin(EchoPlugin)
        .plugin(RepeaterPlugin::new())
        .plugin(SelfTitlePlugin)
        .plugin(RecallPlugin)
        .build();

    ayjx.run().await?;

    Ok(())
}
