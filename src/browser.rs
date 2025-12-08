#![allow(dead_code)]
pub mod element;
pub mod process;
pub mod tab;
pub mod transport;
pub mod types;

use crate::{error, info, warn};
use anyhow::Result;
use serde_json::json;
use simd_json::derived::{ValueObjectAccessAsArray, ValueObjectAccessAsScalar};
use std::sync::Arc;
use tokio::sync::Mutex;

pub use process::BrowserProcess;
pub use tab::Tab;
pub use transport::{Transport, TransportResponse, next_id};
pub use types::{CaptureOptions, Viewport};

#[derive(Clone)]
pub struct Browser {
    transport: Arc<Transport>,
    process: Arc<Mutex<Option<BrowserProcess>>>,
}

static GLOBAL_BROWSER: Mutex<Option<Browser>> = Mutex::const_new(None);

impl Browser {
    pub async fn new() -> Result<Self> {
        let (proc, ws_url) = process::launch(true).await?;
        Ok(Self {
            transport: Arc::new(Transport::new(&ws_url).await?),
            process: Arc::new(Mutex::new(Some(proc))),
        })
    }

    pub async fn new_tab(&self) -> Result<Tab> {
        Tab::new(self.transport.clone()).await
    }

    pub async fn capture_html_with_options(
        &self,
        html: &str,
        selector: &str,
        opts: CaptureOptions,
    ) -> Result<String> {
        let tab = self.new_tab().await?;
        if let Some(ref viewport) = opts.viewport {
            tab.set_viewport(viewport).await?;
        }
        tab.set_content(html).await?;
        let el = tab.find_element(selector).await?;
        let shot = el.screenshot_with_options(opts).await?;
        let _ = tab.close().await;
        Ok(shot)
    }

    /// Captures a high-DPI screenshot with the specified scale factor.
    pub async fn capture_html_hidpi(
        &self,
        html: &str,
        selector: &str,
        scale: f64,
    ) -> Result<String> {
        let opts = CaptureOptions::new()
            .with_viewport(Viewport::default().with_device_scale_factor(scale));
        self.capture_html_with_options(html, selector, opts).await
    }

    /// Gracefully shuts down the global browser instance.
    pub async fn shutdown_global() {
        let mut lock = GLOBAL_BROWSER.lock().await;
        if let Some(browser) = lock.take() {
            info!(target: "Browser", "正在关闭全局浏览器实例...");
            let _ = browser.close_async().await;
        }
    }

    pub async fn close_async(&self) -> Result<()> {
        self.transport.shutdown().await;
        let mut lock = self.process.lock().await;
        if let Some(_proc) = lock.take() {
            // Drop triggers process kill
        }
        Ok(())
    }

    async fn is_alive(&self) -> bool {
        self.transport
            .send(json!({ "id": next_id(), "method": "Target.getTargets", "params": {} }))
            .await
            .is_ok()
    }

    /// Returns a shared singleton browser instance, launching if necessary.
    pub async fn instance() -> Self {
        let mut lock = GLOBAL_BROWSER.lock().await;

        if let Some(b) = &*lock {
            if b.is_alive().await {
                return b.clone();
            }
            warn!(target: "Browser", "实例已失效，正在重启...");
            let _ = b.close_async().await;
        }

        match Self::new().await {
            Ok(b) => {
                // Close initial blank page
                if let Ok(TransportResponse::Response(_, res)) = b
                    .transport
                    .send(json!({"id": next_id(), "method":"Target.getTargets", "params":{}}))
                    .await
                    && let Some(list) = res["result"].get_array("targetInfos")
                    && let Some(id) = list
                        .iter()
                        .find(|t| t["type"] == "page")
                        .and_then(|t| t.get_str("targetId"))
                {
                    let _ = b.transport.send(json!({"id":next_id(), "method":"Target.closeTarget", "params":{"targetId":id}})).await;
                }
                *lock = Some(b.clone());
                b
            }
            Err(e) => {
                error!(target: "Browser", "启动失败: {}", e);
                panic!("Browser launch failed");
            }
        }
    }
}
