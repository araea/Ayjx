#![allow(dead_code)]
use crate::browser::element::Element;
use crate::browser::transport::{Transport, TransportResponse, next_id};
use crate::browser::types::Viewport;
use anyhow::{Context, Result, anyhow};
use serde_json::json;
use simd_json::OwnedValue;
use simd_json::derived::{ValueObjectAccess, ValueObjectAccessAsScalar};
use simd_json::prelude::ValueAsScalar;
use std::sync::Arc;

pub struct Tab {
    pub(crate) transport: Arc<Transport>,
    pub(crate) session_id: String,
    pub(crate) target_id: String,
}

impl Tab {
    pub async fn new(transport: Arc<Transport>) -> Result<Self> {
        let TransportResponse::Response(_, res_create) = transport
            .send(json!({ "id": next_id(), "method": "Target.createTarget", "params": { "url": "about:blank" } }))
            .await? else { return Err(anyhow!("Invalid response type")); };

        let target_id = res_create
            .get("result")
            .and_then(|r| r.get_str("targetId"))
            .context("No targetId")?
            .to_string();

        let TransportResponse::Response(_, res_attach) = transport
            .send(json!({ "id": next_id(), "method": "Target.attachToTarget", "params": { "targetId": target_id } }))
            .await? else { return Err(anyhow!("Invalid response type")); };

        let session_id = res_attach
            .get("result")
            .and_then(|r| r.get_str("sessionId"))
            .context("No sessionId")?
            .to_string();

        Ok(Self {
            transport,
            session_id,
            target_id,
        })
    }

    // Helper to send message to the specific target session
    pub(crate) async fn send_and_get_msg(&self, msg_id: usize, msg: String) -> Result<OwnedValue> {
        let send_fut = self.transport.send(json!({
            "id": next_id(),
            "method": "Target.sendMessageToTarget",
            "params": { "sessionId": self.session_id, "message": msg }
        }));
        let recv_fut = self.transport.get_target_msg(msg_id);

        let (_, target_msg) = futures_util::try_join!(send_fut, recv_fut)?;

        match target_msg {
            TransportResponse::Target(res) => Ok(res),
            other => Err(anyhow!("Unexpected response: {:?}", other)),
        }
    }

    pub async fn send_cmd(&self, method: &str, params: serde_json::Value) -> Result<OwnedValue> {
        let msg_id = next_id();
        let msg = json!({
            "id": msg_id,
            "method": method,
            "params": params
        })
        .to_string();
        self.send_and_get_msg(msg_id, msg).await
    }

    pub async fn set_viewport(&self, viewport: &Viewport) -> Result<&Self> {
        let screen_orientation = if viewport.is_landscape {
            json!({"type": "landscapePrimary", "angle": 90})
        } else {
            json!({"type": "portraitPrimary", "angle": 0})
        };

        self.send_cmd(
            "Emulation.setDeviceMetricsOverride",
            json!({
                "width": viewport.width,
                "height": viewport.height,
                "deviceScaleFactor": viewport.device_scale_factor,
                "mobile": viewport.is_mobile,
                "screenOrientation": screen_orientation
            }),
        )
        .await?;

        if viewport.has_touch {
            self.send_cmd(
                "Emulation.setTouchEmulationEnabled",
                json!({ "enabled": true, "maxTouchPoints": 5 }),
            )
            .await?;
        }

        Ok(self)
    }

    pub async fn set_content(&self, content: &str) -> Result<&Self> {
        self.send_cmd("Page.enable", json!({})).await?;
        let load_event_future = self
            .transport
            .wait_for_event(&self.session_id, "Page.loadEventFired");

        let js_write = format!(
            r#"document.open(); document.write({}); document.close();"#,
            serde_json::to_string(content)?
        );

        self.send_cmd(
            "Runtime.evaluate",
            json!({ "expression": js_write, "awaitPromise": true }),
        )
        .await?;

        load_event_future.await?;
        Ok(self)
    }

    pub async fn find_element(&self, selector: &str) -> Result<Element<'_>> {
        let res_doc = self.send_cmd("DOM.getDocument", json!({})).await?;
        let root_node_id = res_doc["result"]["root"]["nodeId"]
            .as_u64()
            .context("No root node")?;

        let res_sel = self
            .send_cmd(
                "DOM.querySelector",
                json!({ "nodeId": root_node_id, "selector": selector }),
            )
            .await?;

        let node_id = res_sel["result"]["nodeId"]
            .as_u64()
            .context("Element not found")?;

        Element::new(self, node_id).await
    }

    pub async fn activate(&self) -> Result<&Self> {
        let msg_id = next_id();
        let msg = json!({ "id": msg_id, "method": "Target.activateTarget", "params": { "targetId": self.target_id } }).to_string();
        self.send_and_get_msg(msg_id, msg).await?;
        Ok(self)
    }

    pub async fn close(&self) -> Result<()> {
        let msg_id = next_id();
        let msg = json!({ "id": msg_id, "method": "Target.closeTarget", "params": { "targetId": self.target_id } }).to_string();
        self.send_and_get_msg(msg_id, msg).await?;
        Ok(())
    }
}
