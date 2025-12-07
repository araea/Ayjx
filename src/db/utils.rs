use chrono::{Datelike, Duration, Local};

/// 根据自然语言时间描述获取时间戳范围 (start, end)
///
/// 适用场景：生成词云、查询统计数据等
/// 支持：今日, 昨日, 本周, 上周, 近7天, 近30天, 本月, 上月, 今年, 去年, 总
/// 默认返回今日范围
pub fn get_time_range(time_str: &str) -> (i64, i64) {
    let now = Local::now();
    let today_start = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_local_timezone(Local)
        .unwrap();

    match time_str {
        "今日" => (today_start.timestamp(), now.timestamp()),
        "昨日" => {
            let yest_start = today_start - Duration::days(1);
            (yest_start.timestamp(), today_start.timestamp())
        }
        "本周" => {
            let weekday = now.weekday().num_days_from_monday();
            let week_start = today_start - Duration::days(weekday as i64);
            (week_start.timestamp(), now.timestamp())
        }
        "上周" => {
            let weekday = now.weekday().num_days_from_monday();
            let this_week_start = today_start - Duration::days(weekday as i64);
            let last_week_start = this_week_start - Duration::days(7);
            (last_week_start.timestamp(), this_week_start.timestamp())
        }
        "近7天" => {
            let start = now - Duration::days(7);
            (start.timestamp(), now.timestamp())
        }
        "近30天" => {
            let start = now - Duration::days(30);
            (start.timestamp(), now.timestamp())
        }
        "本月" => {
            let month_start = now
                .date_naive()
                .with_day(1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_local_timezone(Local)
                .unwrap();
            (month_start.timestamp(), now.timestamp())
        }
        "上月" => {
            let this_month_start = now
                .date_naive()
                .with_day(1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_local_timezone(Local)
                .unwrap();

            let (prev_year, prev_month) = if this_month_start.month() == 1 {
                (this_month_start.year() - 1, 12)
            } else {
                (this_month_start.year(), this_month_start.month() - 1)
            };

            let prev_month_start = this_month_start
                .with_year(prev_year)
                .unwrap()
                .with_month(prev_month)
                .unwrap();

            (prev_month_start.timestamp(), this_month_start.timestamp())
        }
        "今年" => {
            let year_start = now
                .date_naive()
                .with_month(1)
                .unwrap()
                .with_day(1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_local_timezone(Local)
                .unwrap();
            (year_start.timestamp(), now.timestamp())
        }
        "去年" => {
            let this_year_start = now
                .date_naive()
                .with_month(1)
                .unwrap()
                .with_day(1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_local_timezone(Local)
                .unwrap();

            let last_year_start = this_year_start
                .with_year(this_year_start.year() - 1)
                .unwrap();
            (last_year_start.timestamp(), this_year_start.timestamp())
        }
        "总" => (0, now.timestamp()),
        _ => (today_start.timestamp(), now.timestamp()),
    }
}
