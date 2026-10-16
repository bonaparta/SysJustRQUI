use gpui::*;

struct HelloWorld {
    text: SharedString,
}

impl Render for HelloWorld {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .size_full()
            .bg(rgb(0x181825))
            .text_color(rgb(0xcdd6f4))
            .child(
                div()
                    .text_2xl()
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(0x89b4fa))
                    .child(self.text.clone()),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(0xa6adc8))
                    .mt_3()
                    .child("Zed GPUI - GPU Accelerated Window"),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_4()
                    .mt_6()
                    .child(
                        div()
                            .p_3()
                            .rounded_md()
                            .bg(rgb(0x313244))
                            .child("📈 2330 台積電: 980.00 (+1.55%)"),
                    )
                    .child(
                        div()
                            .p_3()
                            .rounded_md()
                            .bg(rgb(0x313244))
                            .child("⚡ 即時撮合引擎已就緒"),
                    ),
            )
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(700.0), px(450.0)), cx);
        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("SysRQUI - Zed GPUI Hello World".into()),
                appears_transparent: false,
                traffic_light_position: None,
            }),
            ..Default::default()
        };

        cx.open_window(window_options, |_window, cx| {
            cx.new(|_cx| HelloWorld {
                text: "Hello, World! 歡迎來到 SysRQUI".into(),
            })
        })
        .unwrap();

        // 從終端機或背景啟動時，把視窗帶到最前面，否則可能被其他視窗蓋住
        cx.activate(true);
    });
}
