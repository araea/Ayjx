#![allow(dead_code)]

use chrono::{DateTime, Local, TimeZone};
use std::collections::HashMap;
use std::future::Future;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::task::AbortHandle;

/// 全局定时任务管理器
pub struct Scheduler {
    tasks: Mutex<HashMap<u64, AbortHandle>>,
    next_id: AtomicU64,
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            tasks: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    /// 添加一个灵活调度任务
    ///
    /// # 参数
    /// - `next_run_calculator`: 一个闭包，接收当前时间，返回下一次执行时间。如果返回 None，任务停止。
    /// - `task_gen`: 任务生成闭包。
    ///
    /// # 示例：每天 0 点执行
    /// ```rust
    /// scheduler.add_schedule(
    ///     |now| {
    ///         // 获取明天的 0 点
    ///         let tomorrow = now.date_naive().succ_opt()?.and_hms_opt(0, 0, 0)?;
    ///         Local.from_local_datetime(&tomorrow).single()
    ///     },
    ///     || async { println!("Midnight!"); }
    /// );
    /// ```
    pub fn add_schedule<C, F, Fut>(&self, mut next_run_calculator: C, mut task_gen: F) -> u64
    where
        C: FnMut(DateTime<Local>) -> Option<DateTime<Local>> + Send + 'static,
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);

        // 首次计算执行时间
        let mut next_time = next_run_calculator(Local::now());

        let handle = tokio::spawn(async move {
            while let Some(target_time) = next_time {
                let now = Local::now();

                // 计算需要 sleep 多久
                if target_time > now {
                    let duration = (target_time - now)
                        .to_std()
                        .unwrap_or(Duration::from_millis(0));
                    tokio::time::sleep(duration).await;
                }

                // 执行任务
                task_gen().await;

                // 计算下一次
                next_time = next_run_calculator(Local::now());
            }
        });

        let abort_handle = handle.abort_handle();
        self.tasks.lock().unwrap().insert(id, abort_handle);
        id
    }

    /// 兼容旧接口：固定间隔执行
    pub fn add_interval<F, Fut>(&self, duration: Duration, task_gen: F) -> u64
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.add_schedule(
            move |now| Some(now + chrono::Duration::from_std(duration).unwrap()),
            task_gen,
        )
    }

    /// 辅助方法：每天特定时间执行 (HH:MM:SS)
    pub fn add_daily_at<F, Fut>(&self, hour: u32, minute: u32, second: u32, task_gen: F) -> u64
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.add_schedule(
            move |now| {
                let today = now.date_naive();
                // 构造今天的目标时间
                let target_today = today
                    .and_hms_opt(hour, minute, second)
                    .and_then(|t| Local.from_local_datetime(&t).single());

                if let Some(target) = target_today
                    && target > now
                {
                    return Some(target);
                }

                // 如果今天已经过了，或者是无效时间（如夏令时跳变），则定在明天
                let tomorrow = today.succ_opt()?;
                tomorrow
                    .and_hms_opt(hour, minute, second)
                    .and_then(|t| Local.from_local_datetime(&t).single())
            },
            task_gen,
        )
    }

    pub fn remove(&self, id: u64) {
        if let Some(handle) = self.tasks.lock().unwrap().remove(&id) {
            handle.abort();
        }
    }

    pub fn shutdown(&self) {
        println!("正在清理定时任务...");
        let mut tasks = self.tasks.lock().unwrap();
        for (_, handle) in tasks.drain() {
            handle.abort();
        }
    }
}
