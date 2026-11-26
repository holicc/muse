//! `muse mcp`：stdio 上的 MCP 服务，让 AI 通过统一接口控制正在运行的壁纸。
//!
//! 每个工具都转成一次接口调用（见 [`crate::api`]），所以只在 muse 运行时有效；
//! 写操作和设计窗口一样会备份原文件、可以用 `undo` 撤销。

use std::collections::BTreeMap;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ErrorData, ServerHandler, ServiceExt, schemars, tool, tool_handler, tool_router};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Clone)]
struct Muse {
    tool_router: ToolRouter<Self>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct Pack {
    /// 诗词包的目录名（list_poems 里的 name），如 "jing-ye-si"。
    pack: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct Pause {
    /// true 暂停，false 继续；不给时切换。
    paused: Option<bool>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct StylePatch {
    /// 诗词包的目录名。
    pack: String,
    /// 颜色：键为 color（整首）、title、line.N（第 N 列）、byline、seal（印章底色）、rule、masthead、issue、
    /// source、watermark，单个字在部件名后加 .N（如 line.2.3）。值为 "#rrggbb"、"#rrggbbaa"（带透明度）
    /// 或渐变数组 ["#ffffff", "#7aa8ff"]；null 表示删掉这一项、恢复默认。
    colors: Option<BTreeMap<String, Value>>,
    /// 这首诗的背景："auto"（当前桌面壁纸）、"pack"（诗词包自带）或图片绝对路径；null 恢复跟随全局。
    #[serde(default, deserialize_with = "present")]
    wallpaper: Option<Value>,
    /// 部件位移 [x, y]，单位是屏幕高度（0.1 即屏高的十分之一）；null 删掉。
    offsets: Option<BTreeMap<String, Value>>,
    /// 部件字号倍数（0.2–5，1 为跟随整首）；null 删掉。
    sizes: Option<BTreeMap<String, Value>>,
    /// 部件不透明度（0–1）；null 删掉。
    opacity: Option<BTreeMap<String, Value>>,
}

/// 给了这个字段就是 Some，包括 null（表示删掉），没给才是 None。
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

#[derive(Deserialize, schemars::JsonSchema)]
struct Settings {
    /// 全局设置：wallpaper（auto / pack / 图片路径）、font、text_color（auto / #rrggbb）、
    /// style（magazine / classic）、text_scale（0.3–3）、fps（1–120）、battery_fps（0–120，0 为用电池时暂停）、
    /// pause_on_fullscreen、hold（秒）、char_interval（秒）、tray。值为 null 表示删掉、回到 config.toml 或默认值。
    values: BTreeMap<String, Value>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct ArrangementArgs {
    /// 诗主体中心的横向位置，0 最左、1 最右；要和 y 一起给。
    x: Option<f32>,
    /// 诗主体中心的纵向位置，0 最上、1 最下。
    y: Option<f32>,
    /// 在全局字号上再乘的倍数。
    scale: Option<f32>,
    /// true 时恢复诗词包的默认位置和字号。
    reset: Option<bool>,
}

fn reply(r: Result<Value, String>) -> Result<CallToolResult, ErrorData> {
    Ok(match r {
        Ok(v) => CallToolResult::success(vec![ContentBlock::text(
            serde_json::to_string_pretty(&v).unwrap_or_default(),
        )]),
        Err(e) => CallToolResult::error(vec![ContentBlock::text(e)]),
    })
}

fn call(method: &str, params: Value) -> Result<CallToolResult, ErrorData> {
    reply(crate::control::call(method, params))
}

#[tool_router]
impl Muse {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "壁纸当前状态：正在显示哪首诗、第几首、是否暂停。")]
    async fn status(&self) -> Result<CallToolResult, ErrorData> {
        call("status", Value::Null)
    }

    #[tool(description = "列出所有诗：目录名、标题、作者、诗句、诗词包指定的版式等。")]
    async fn list_poems(&self) -> Result<CallToolResult, ErrorData> {
        call("poems", Value::Null)
    }

    #[tool(description = "切到下一首诗。")]
    async fn next_poem(&self) -> Result<CallToolResult, ErrorData> {
        call("next", Value::Null)
    }

    #[tool(description = "切到上一首诗。")]
    async fn previous_poem(&self) -> Result<CallToolResult, ErrorData> {
        call("prev", Value::Null)
    }

    #[tool(description = "切到指定的诗。")]
    async fn show_poem(
        &self,
        Parameters(p): Parameters<Pack>,
    ) -> Result<CallToolResult, ErrorData> {
        call("goto", json!({ "pack": p.pack }))
    }

    #[tool(description = "暂停或继续动画。")]
    async fn set_paused(
        &self,
        Parameters(p): Parameters<Pause>,
    ) -> Result<CallToolResult, ErrorData> {
        call("pause", json!({ "paused": p.paused }))
    }

    #[tool(description = "读取一首诗的样式：颜色、背景、部件位置 / 字号 / 不透明度。")]
    async fn get_style(
        &self,
        Parameters(p): Parameters<Pack>,
    ) -> Result<CallToolResult, ErrorData> {
        call("style.get", json!({ "pack": p.pack }))
    }

    #[tool(
        description = "修改一首诗的样式，只改给出的项，壁纸立即生效。可以改颜色（纯色、渐变、带透明度、单个字）、\
                       这首诗的背景、部件位置 / 字号 / 不透明度。返回修改后的完整样式。改错了可以用 undo 撤销。"
    )]
    async fn update_style(
        &self,
        Parameters(p): Parameters<StylePatch>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut params = json!({ "pack": p.pack });
        for (k, v) in [
            ("colors", p.colors.map(|m| json!(m))),
            ("offsets", p.offsets.map(|m| json!(m))),
            ("sizes", p.sizes.map(|m| json!(m))),
            ("opacity", p.opacity.map(|m| json!(m))),
        ] {
            if let Some(v) = v {
                params[k] = v;
            }
        }
        if let Some(w) = p.wallpaper {
            params["wallpaper"] = w;
        }
        call("style.patch", params)
    }

    #[tool(description = "把一首诗的样式全部恢复默认（颜色、背景、部件位置、字号、不透明度）。")]
    async fn reset_style(
        &self,
        Parameters(p): Parameters<Pack>,
    ) -> Result<CallToolResult, ErrorData> {
        call("style.set", json!({ "pack": p.pack, "style": {} }))
    }

    #[tool(description = "读取全局设置：settings.toml 里写的值、实际生效的值、每一项来自哪里。")]
    async fn get_settings(&self) -> Result<CallToolResult, ErrorData> {
        call("settings.get", Value::Null)
    }

    #[tool(
        description = "修改全局设置（写进 settings.toml，覆盖 config.toml 的同名项；config.toml 不会被改）。"
    )]
    async fn update_settings(
        &self,
        Parameters(p): Parameters<Settings>,
    ) -> Result<CallToolResult, ErrorData> {
        call("settings.set", json!({ "values": p.values }))
    }

    #[tool(description = "读取整首诗的位置和字号倍数（所有诗共用）。")]
    async fn get_arrangement(&self) -> Result<CallToolResult, ErrorData> {
        call("arrangement.get", Value::Null)
    }

    #[tool(description = "设置整首诗的位置和字号倍数（所有诗共用）。")]
    async fn set_arrangement(
        &self,
        Parameters(p): Parameters<ArrangementArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        // 只发给出的项：没给 x、y 时保留原来的位置。
        let mut params = json!({});
        for (k, v) in [("x", p.x), ("y", p.y), ("scale", p.scale)] {
            if let Some(v) = v {
                params[k] = json!(v);
            }
        }
        if p.reset == Some(true) {
            params["reset"] = json!(true);
        }
        call("arrangement.set", params)
    }

    #[tool(description = "撤销最近一次修改（样式、设置或位置）。")]
    async fn undo(&self) -> Result<CallToolResult, ErrorData> {
        call("undo", Value::Null)
    }

    #[tool(description = "最近的修改记录，最新的在前。")]
    async fn history(&self) -> Result<CallToolResult, ErrorData> {
        call("history", Value::Null)
    }

    #[tool(description = "让壁纸重新读取配置和样式文件（手动改了文件之后用）。")]
    async fn reload(&self) -> Result<CallToolResult, ErrorData> {
        call("reload", Value::Null)
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Muse {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("muse", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "muse 是古诗词动态桌面壁纸。这些工具控制正在运行的壁纸：切换诗、改每首诗的颜色和部件位置、改全局设置。\
                 只有 muse 在运行时有效。修改都可以用 undo 撤销。",
            )
    }
}

/// 在 stdio 上运行 MCP 服务，直到客户端断开。
pub fn run() -> Result<(), String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("启动异步运行时失败: {e}"))?;
    rt.block_on(async {
        let service = Muse::new()
            .serve(rmcp::transport::stdio())
            .await
            .map_err(|e| format!("MCP 初始化失败: {e}"))?;
        service
            .waiting()
            .await
            .map_err(|e| format!("MCP 服务出错: {e}"))?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wallpaper_null_means_reset() {
        let p: StylePatch =
            serde_json::from_value(json!({ "pack": "a", "wallpaper": null })).unwrap();
        assert_eq!(p.wallpaper, Some(Value::Null));
        let p: StylePatch = serde_json::from_value(json!({ "pack": "a" })).unwrap();
        assert_eq!(p.wallpaper, None);
    }
}
