use crate::adapters::onebot::LockedWriter;
use crate::command::match_command;
use crate::config::build_config;
use crate::event::Context;
use crate::plugins::{PluginError, get_config};
use futures_util::future::BoxFuture;
use shindan_maker::ShindanDomain;
use std::sync::OnceLock;
use toml::Value;

pub mod config;
pub mod entity;
pub mod executor;
pub mod manager;
pub mod stats;
pub mod storage;
pub mod utils;

use config::PluginConfig;
use storage::Storage;
use utils::extract_args;

// 全局单例存储
static STORAGE: OnceLock<Storage> = OnceLock::new();

fn get_storage() -> &'static Storage {
    STORAGE.get_or_init(Storage::new)
}

pub fn default_config() -> Value {
    build_config(PluginConfig::default())
}

pub fn init(ctx: Context) -> BoxFuture<'static, std::result::Result<(), PluginError>> {
    Box::pin(async move {
        let storage = get_storage();
        storage.init(&ctx.db).await;
        Ok(())
    })
}

pub fn handle(
    ctx: Context,
    writer: LockedWriter,
) -> BoxFuture<'static, std::result::Result<Option<Context>, PluginError>> {
    Box::pin(async move {
        let task = async move {
            let config: PluginConfig = get_config(&ctx, "shindan").unwrap_or_default();
            let storage = get_storage();
            let domain = config
                .domain
                .parse::<ShindanDomain>()
                .unwrap_or(ShindanDomain::Jp);

            // 1. 优先匹配系统管理指令
            macro_rules! get_params {
                ($match:expr) => {{
                    let args = extract_args(&$match.args);
                    args.into_iter().collect::<Vec<String>>()
                }};
            }

            if let Some(m) = match_command(&ctx, "添加神断") {
                let p = get_params!(m);
                let params: Vec<&str> = p.iter().map(|s| s.as_str()).collect();
                manager::handle_add(&ctx, writer, &params, domain, storage).await?;
                return Ok::<Option<Context>, PluginError>(Some(ctx));
            }
            if let Some(m) = match_command(&ctx, "删除神断") {
                let p = get_params!(m);
                let params: Vec<&str> = p.iter().map(|s| s.as_str()).collect();
                manager::handle_del(&ctx, writer, &params, storage).await?;
                return Ok::<Option<Context>, PluginError>(Some(ctx));
            }
            if match_command(&ctx, "神断列表").is_some() {
                manager::handle_list(&ctx, writer, storage).await?;
                return Ok::<Option<Context>, PluginError>(Some(ctx));
            }
            if let Some(m) = match_command(&ctx, "随机神断") {
                let p = get_params!(m);
                let params: Vec<&str> = p.iter().map(|s| s.as_str()).collect();
                executor::handle_shindan_exec(
                    &ctx, writer, &params, &m.args, domain, storage, &config, true,
                )
                .await?;
                return Ok::<Option<Context>, PluginError>(Some(ctx));
            }
            if let Some(m) = match_command(&ctx, "设置神断") {
                let p = get_params!(m);
                let params: Vec<&str> = p.iter().map(|s| s.as_str()).collect();
                manager::handle_set_mode(&ctx, writer, &params, storage).await?;
                return Ok::<Option<Context>, PluginError>(Some(ctx));
            }
            if let Some(m) = match_command(&ctx, "修改神断") {
                let p = get_params!(m);
                let params: Vec<&str> = p.iter().map(|s| s.as_str()).collect();
                manager::handle_modify(&ctx, writer, &params, storage).await?;
                return Ok::<Option<Context>, PluginError>(Some(ctx));
            }
            if let Some(m) = match_command(&ctx, "用户次数") {
                let p = get_params!(m);
                let params: Vec<&str> = p.iter().map(|s| s.as_str()).collect();
                stats::handle_user_count(&ctx, writer, &params, &m.args, storage).await?;
                return Ok::<Option<Context>, PluginError>(Some(ctx));
            }
            if let Some(m) = match_command(&ctx, "用户排行榜") {
                let p = get_params!(m);
                let params: Vec<&str> = p.iter().map(|s| s.as_str()).collect();
                stats::handle_user_rank(&ctx, writer, &params, storage, config.rank_max).await?;
                return Ok::<Option<Context>, PluginError>(Some(ctx));
            }
            if let Some(m) = match_command(&ctx, "查看神断") {
                let p = get_params!(m);
                let params: Vec<&str> = p.iter().map(|s| s.as_str()).collect();
                manager::handle_view_info(&ctx, writer, &params, storage).await?;
                return Ok::<Option<Context>, PluginError>(Some(ctx));
            }
            if let Some(m) = match_command(&ctx, "神断次数") {
                let p = get_params!(m);
                let params: Vec<&str> = p.iter().map(|s| s.as_str()).collect();
                stats::handle_item_rank(&ctx, writer, &params, storage, config.rank_max).await?;
                return Ok::<Option<Context>, PluginError>(Some(ctx));
            }
            if let Some(m) = match_command(&ctx, "查找神断") {
                let p = get_params!(m);
                let params: Vec<&str> = p.iter().map(|s| s.as_str()).collect();
                manager::handle_search(&ctx, writer, &params, storage, config.rank_max).await?;
                return Ok::<Option<Context>, PluginError>(Some(ctx));
            }
            if match_command(&ctx, "神断帮助").is_some()
                || match_command(&ctx, "插件指令列表").is_some()
            {
                manager::handle_help(&ctx, writer).await?;
                return Ok::<Option<Context>, PluginError>(Some(ctx));
            }

            // 2. 匹配已保存的神断指令
            // 由于指令是动态的，遍历列表进行匹配
            let shindan_list = storage.get_shindans();
            for s in shindan_list {
                if let Some(m) = match_command(&ctx, &s.command) {
                    let p = get_params!(m);
                    let params: Vec<&str> = p.iter().map(|s| s.as_str()).collect();
                    executor::run_specific_shindan(
                        &ctx, writer, &s.command, &params, &m.args, domain, storage, &config,
                    )
                    .await?;
                    return Ok::<Option<Context>, PluginError>(Some(ctx));
                }
            }

            Ok::<Option<Context>, PluginError>(Some(ctx))
        };

        match task.await {
            Ok(res) => Ok(res),
            Err(e) => Err(e),
        }
    })
}
