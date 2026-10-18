# SysRQUI：XQ 模組化重構專案

[![CI](https://github.com/bonaparta/SysJustRQUI/actions/workflows/ci.yml/badge.svg)](https://github.com/bonaparta/SysJustRQUI/actions/workflows/ci.yml)

本專案基於 **Rust / GPUI** 新架構，建立「產品 → 模組 → 相依」的自動化組裝與交付機制。核心目標是依據客戶訂閱的產品功能，在建置時期（Build-time）僅編入所需模組，讓開發團隊能快速組裝並發布專屬的客製化版本。

---

## 核心目標與要解決的問題

* **依訂閱組裝版本**：將功能拆分為獨立的技術單元（模組），依據後端授權的「產品」自動解析依賴關係並組裝專屬版本。

* **大幅縮短客製交付工期**：讓開發者能透過組裝既有模組與撰寫串接中介程式（Glue code），產出特定規格的可執行檔。

---

## 技術架構與選型背景 (FAQ)

### Q: Java AWT、Xamarin.Forms(包原生)、Java Swing、Flutter、Qt(自繪畫面)，為什麼跨平台 UI 最後都沒有「真正成功」？

**A:** 「原生體驗（Native UX）」的細節是動態移動的靶，一直改光是相容就會花很多心力
- 包原生如果沒有對到，自己刻就會花大量的 effort 保持多線共容
- 自繪畫面本質上是「全螢幕遊戲」，完全脫離原生的文字渲染、無障礙支援與系統主題切換，記憶體消耗大且手感總差了一截
- Web 技術 + Chromium（Electron）反而靠大基數殺出。雖然 Electron 被詬病是「吃記憶體怪獸」，但它依靠 Web 龐大的開發者基數、CSS 的靈活樣式與開箱即用的現代排版，把許多傳統原生跨平台框架活活擠出了市場
- **跨平台 UI 弱點**
  - 複雜文字排版與雙向文字（阿拉伯文、希伯來文）、複雜字形替換（Ligatures）。
  - 完善的各國語言輸入法（IME）支援（這是目前許多 Rust 自繪 UI 在中文/日文輸入時最常崩潰的地方）。
  - 螢幕閱讀器與無障礙 API（Accessibility / a11y）。
  - 多螢幕高 DPI 動態縮放、混合 GPU 渲染。

### Q: Longbridge 是什麼東西？ 和 GPUI 是什麼關係？

**A:** Zed 是一款程式碼編輯器，改良了 VS Code 用 electron 寫的龐大與效能問題，由 Atom 團隊等開發。GPUI 是 Zed 的 GUI 框架。Longbridge 是一家跨國互聯網券商，提供美股、港股等全球股票交易服務，將自己的 App 用 GPUI 重構叫 Longbridge Pro。以往會透過 OpenGL 或 DirectX 11 來繪製UI，現代系統改用更貼近硬體的「低階圖形 API」，Metal：Apple 原廠的專屬底層圖形 API / Vulkan：跨平台(Khronos 集團 - Linux、Windows、Android)的底層圖形 API。
- Zed 是一款程式碼編輯器，全部用 Rust 寫，以 Rust GPU 加速 UI 函式庫
- GPUI 是 Zed 的 GUI 框架
- Longbridge 則是把 GPUI 拿去做出第一款大規模非編輯器商業軟體
  - 補充了大量商業級 UI 元件（類似 shadcn/ui 的 Rust 版、虛擬滾動表格、Dock 佈局、分頁、圖表、Markdown/HTML 支援）
  - 針對頻繁更新的資料圖表做出增量渲染（Retained Mode）最佳化的 GPUI 分支
- Metal/Vulkan 直接在 GPU 上畫出 120fps 的程式碼編輯器

---

## 專案位置

- 程式碼：https://github.com/bonaparta/SysJustRQUI
- Issue 追蹤：https://github.com/bonaparta/SysJustRQUI/issues

## 快速開始

需要 Rust 工具鏈（[rustup](https://rustup.rs/)），Windows、macOS 都可以開發。

```bash
cargo build
cargo run
cargo test
```

`cargo run` 會啟動 `crates/app`（執行檔 `sysrqui`）；`cargo test`、`cargo clippy` 涵蓋所有 crate。

## 目錄結構

```
Cargo.toml        workspace 設定（成員、共用版本與依賴）
crates/
  app/            sysrqui：進入點，組裝視窗與模組（glue code）
  ui/             sysrqui-ui：GPUI 共用元件與配色
  quote/          sysrqui-quote：報價資料模型，純 Rust、不依賴 gpui
docs/             loop engineering 流程、交接檔
```

依賴方向：`app` → `ui`、`quote`。`quote` 不依賴 gpui 與 `ui`，`ui` 也不依賴 `quote`，資料轉畫面的程式放在 `app`。之後依訂閱組裝版本時，功能模組會以 crate 的形式加進 `crates/`。

## 開發方式

本專案由多個 AI agent（Claude、Codex、Gemini、Grok）與人類共同開發。

- 共用規則：[AGENTS.md](AGENTS.md)
- Loop engineering 操作步驟：[docs/loop-engineering.md](docs/loop-engineering.md)
- 最新進度與下一步：`docs/handoff/` 底下日期最新的交接檔

## 專案狀態

- 已完成：Hello 視窗；多 agent 讀 issue、開 PR、交接的流程打通；CI（Windows + macOS 跑 fmt/build/test/clippy）。
- 進行中：[#5 開發流程基礎建設](https://github.com/bonaparta/SysJustRQUI/issues/5)（分支保護、workspace 分層、loop 流程文件、debug 環境指令）。
- 之後：把待完成功能拆成 GitHub issue、每日包版發到 GitHub Releases。
