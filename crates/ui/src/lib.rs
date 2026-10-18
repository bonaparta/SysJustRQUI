//! GPUI 共用元件與配色。不依賴 `sysrqui-quote`：只吃字串等通用資料，
//! 資料轉畫面的 glue code 放在 app。

use gpui::*;

/// 配色（Catppuccin Mocha）。
pub mod theme {
    use gpui::{Rgba, rgb};

    pub fn background() -> Rgba {
        rgb(0x181825)
    }
    pub fn surface() -> Rgba {
        rgb(0x313244)
    }
    pub fn text() -> Rgba {
        rgb(0xcdd6f4)
    }
    pub fn subtext() -> Rgba {
        rgb(0xa6adc8)
    }
    pub fn accent() -> Rgba {
        rgb(0x89b4fa)
    }
}

/// 圓角底色卡片。
pub fn card(content: impl Into<SharedString>) -> Div {
    div()
        .p_3()
        .rounded_md()
        .bg(theme::surface())
        .child(content.into())
}
