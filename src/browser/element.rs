#![allow(dead_code)]
use crate::browser::tab::Tab;
use crate::browser::transport::next_id;
use crate::browser::types::{CaptureOptions, ImageFormat};
use anyhow::{Context, Result};
use serde_json::json;
use simd_json::derived::{ValueObjectAccess, ValueObjectAccessAsArray};
use simd_json::prelude::ValueAsScalar;

pub struct Element<'a> {
    parent: &'a Tab,
    backend_node_id: u64,
}

impl<'a> Element<'a> {
    pub(crate) async fn new(parent: &'a Tab, node_id: u64) -> Result<Self> {
        let msg_id = next_id();
        let msg = json!({
            "id": msg_id,
            "method": "DOM.describeNode",
            "params": { "nodeId": node_id, "depth": 100 }
        })
        .to_string();

        let res = parent.send_and_get_msg(msg_id, msg).await?;
        let backend_node_id = res["result"]["node"]["backendNodeId"]
            .as_u64()
            .context("Missing backendNodeId")?;

        Ok(Self {
            parent,
            backend_node_id,
        })
    }

    pub async fn screenshot_with_options(&self, opts: CaptureOptions) -> Result<String> {
        if let Some(ref viewport) = opts.viewport {
            self.parent.set_viewport(viewport).await?;
        }

        // Get element bounding box
        let msg_id = next_id();
        let msg_box = json!({
            "id": msg_id,
            "method": "DOM.getBoxModel",
            "params": { "backendNodeId": self.backend_node_id }
        })
        .to_string();

        let res_box = self.parent.send_and_get_msg(msg_id, msg_box).await?;

        // SimdJson extraction
        let border = res_box
            .get("result")
            .and_then(|r| r.get("model"))
            .and_then(|m| m.get_array("border"))
            .context("Failed to get box model border")?;

        let (x, y, w, h) = (
            border[0].as_f64().unwrap_or(0.0),
            border[1].as_f64().unwrap_or(0.0),
            (border[2].as_f64().unwrap_or(0.0) - border[0].as_f64().unwrap_or(0.0)),
            (border[5].as_f64().unwrap_or(0.0) - border[1].as_f64().unwrap_or(0.0)),
        );

        let mut params = json!({
            "format": opts.format.as_str(),
            "clip": { "x": x, "y": y, "width": w, "height": h, "scale": 1.0 },
            "fromSurface": true,
            "captureBeyondViewport": opts.full_page,
        });

        if matches!(opts.format, ImageFormat::Jpeg | ImageFormat::WebP) {
            params["quality"] = json!(opts.quality.unwrap_or(90));
        }

        // Handle transparent background for PNG
        if opts.omit_background && matches!(opts.format, ImageFormat::Png) {
            let msg_id = next_id();
            let msg = json!({
                "id": msg_id,
                "method": "Emulation.setDefaultBackgroundColorOverride",
                "params": { "color": { "r": 0, "g": 0, "b": 0, "a": 0 } }
            })
            .to_string();
            self.parent.send_and_get_msg(msg_id, msg).await?;
        }

        let msg_id = next_id();
        let msg_cap = json!({
            "id": msg_id,
            "method": "Page.captureScreenshot",
            "params": params
        })
        .to_string();

        self.parent.activate().await?;
        let res_cap = self.parent.send_and_get_msg(msg_id, msg_cap).await?;

        // Reset background
        if opts.omit_background && matches!(opts.format, ImageFormat::Png) {
            let msg_id = next_id();
            let msg = json!({
                "id": msg_id,
                "method": "Emulation.setDefaultBackgroundColorOverride",
                "params": {}
            })
            .to_string();
            let _ = self.parent.send_and_get_msg(msg_id, msg).await;
        }

        res_cap["result"]["data"]
            .as_str()
            .map(|s| s.to_string())
            .context("No image data received")
    }
}
