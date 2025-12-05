use crate::event::Event;
use simd_json::derived::ValueObjectAccessAsScalar;
use std::time::Duration;
use tokio::sync::{Mutex as AsyncMutex, oneshot};

/// 事件匹配器，用于处理交互式等待及 API 响应
pub struct Matcher {
    waiters: AsyncMutex<Vec<Waiter>>,
}

struct Waiter {
    // 消息匹配条件
    group_id: Option<i64>,
    user_id: Option<i64>,
    // API 响应匹配条件
    echo: Option<String>,

    sender: oneshot::Sender<Event>,
}

impl Matcher {
    pub fn new() -> Self {
        Self {
            waiters: AsyncMutex::new(Vec::new()),
        }
    }

    /// 注册一个消息等待者 (群号/用户)
    pub async fn wait(
        &self,
        group_id: Option<i64>,
        user_id: Option<i64>,
        timeout_duration: Duration,
    ) -> Option<Event> {
        self.wait_internal(group_id, user_id, None, timeout_duration)
            .await
    }

    /// 注册一个响应等待者 (Echo)
    pub async fn wait_resp(&self, echo: String, timeout_duration: Duration) -> Option<Event> {
        self.wait_internal(None, None, Some(echo), timeout_duration)
            .await
    }

    async fn wait_internal(
        &self,
        group_id: Option<i64>,
        user_id: Option<i64>,
        echo: Option<String>,
        timeout_duration: Duration,
    ) -> Option<Event> {
        let (tx, rx) = oneshot::channel();
        {
            let mut guard = self.waiters.lock().await;
            guard.push(Waiter {
                group_id,
                user_id,
                echo,
                sender: tx,
            });
        }

        match tokio::time::timeout(timeout_duration, rx).await {
            Ok(Ok(event)) => Some(event),
            _ => None,
        }
    }

    /// 尝试分发事件给等待者。如果事件被消费（匹配成功），返回 None；否则返回原事件。
    pub async fn dispatch(&self, event: Event) -> Option<Event> {
        let g_id = event
            .get_i64("group_id")
            .or_else(|| event.get_u64("group_id").map(|v| v as i64));
        let u_id = event
            .get_i64("user_id")
            .or_else(|| event.get_u64("user_id").map(|v| v as i64));
        let echo = event.get_str("echo").map(|s| s.to_string());

        // 如果既不是群消息/私聊消息，也不是 API 响应，直接放行
        if g_id.is_none() && u_id.is_none() && echo.is_none() {
            return Some(event);
        }

        let mut guard = self.waiters.lock().await;

        // 寻找匹配者
        let index = guard.iter().position(|w| {
            // 1. 优先匹配 echo (API 响应)
            if let Some(req_echo) = &w.echo {
                if let Some(resp_echo) = &echo {
                    return req_echo == resp_echo;
                }
                return false;
            }

            // 2. 匹配消息 (Group / User)
            // 如果等待者没有 echo 限制，且事件也没有 echo (即普通消息)，则进行 ID 匹配
            if echo.is_none() {
                let match_group = w.group_id.is_none() || w.group_id == g_id;
                let match_user = w.user_id.is_none() || w.user_id == u_id;
                return match_group && match_user;
            }

            false
        });

        if let Some(idx) = index {
            let waiter = guard.remove(idx);
            // 发送事件给等待者。忽略错误（如等待者已超时）
            let _ = waiter.sender.send(event);
            None // 事件被消费
        } else {
            Some(event) // 无匹配，返还事件
        }
    }
}
