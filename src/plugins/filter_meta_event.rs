use crate::bot::WsWriter;
use crate::event::{Context, EventType};
use crate::plugins::{PluginError, build_config};
use futures_util::future::BoxFuture;
use serde::Serialize;
use simd_json::derived::ValueObjectAccessAsScalar;
use toml::Value;

#[derive(Serialize)]
struct FilterConfig {
    enabled: bool,
}

pub fn default_config() -> Value {
    build_config(FilterConfig { enabled: true })
}

pub fn handle<'a>(
    ctx: Context,
    _writer: &'a mut WsWriter,
) -> BoxFuture<'a, Result<Option<Context>, PluginError>> {
    Box::pin(async move {
        if let EventType::Onebot(event) = &ctx.event {
            if let Some(post_type) = event.get_str("post_type")
                && post_type == "meta_event"
            {
                return Ok(None);
            }
        }
        Ok(Some(ctx))
    })
}
