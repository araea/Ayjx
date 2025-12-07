pub mod avatar;
pub mod data_loader;
pub mod renderer;
pub mod utils;

use crate::event::Context;
use crate::plugins::get_config;
use crate::plugins::stats_visualizer::{StatsConfig, default_config};

use self::avatar::prepare_avatars;
use self::data_loader::{BarData, SeriesData, fetch_bar_data, fetch_line_data};
use self::renderer::{draw_bar_chart, draw_line_chart};

#[allow(clippy::too_many_arguments)]
pub async fn generate(
    ctx: &Context,
    is_all_groups: bool,
    data_type: &str,
    chart_type: &str,
    query_group: Option<i64>,
    query_user: Option<i64>,
    sender_id: i64,
    start_time: i64,
    end_time: i64,
    title: &str,
) -> Result<String, String> {
    let db = &ctx.db;
    let config: StatsConfig = get_config(ctx, "stats_visualizer")
        .unwrap_or_else(|| serde::Deserialize::deserialize(default_config()).unwrap());

    // 1. 走势图
    if chart_type == "走势" {
        let chart_data: Vec<SeriesData> = fetch_line_data(
            db,
            is_all_groups,
            data_type,
            query_group,
            query_user,
            start_time,
            end_time,
        )
        .await?;

        return draw_line_chart(&config, title, chart_data);
    }

    // 2. 柱状图 / 排行榜
    let mut bar_data: Vec<BarData> = fetch_bar_data(
        db,
        is_all_groups,
        data_type,
        query_group,
        query_user,
        sender_id,
        start_time,
        end_time,
    )
    .await?;

    // 3. 准备头像
    prepare_avatars(&mut bar_data).await;

    // 4. 绘图
    draw_bar_chart(&config, title, bar_data)
}
