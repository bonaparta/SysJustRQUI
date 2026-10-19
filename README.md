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

第一次在這台電腦開發，請先依 [新電腦開工：Windows／macOS](docs/new-computer.md) 安裝平台工具、設定 GitHub 認證，再完成檢查、建置、啟動與除錯。

需要 Rust 工具鏈（[rustup](https://rustup.rs/)），Windows、macOS 都可以開發。

```bash
cargo build
cargo run
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

`cargo run` 會啟動 `crates/app`（執行檔 `sysrqui`）。CI 的 build、test、clippy 會明確檢查整個 workspace，包含開發工具 crate `sysrqui-xtask`；fmt 使用 `cargo fmt --all --check`。根目錄一般 `cargo run` 仍只會選取 app，不會改成執行 xtask。

### 開發環境檢查

執行 `cargo xtask doctor` 檢查 `git`、`cargo`、`rustc`、`gh` 是否可找到並能回報版本，同時列出作業系統與架構。`codex`、`claude`、`gemini`、`grok` 只檢查命令是否存在；doctor 不會呼叫它們，也不檢查登入或非互動能力。必要工具缺少或版本檢查失敗時命令會以非零狀態結束，並顯示修復方向。doctor 不會安裝工具、登入 GitHub 或讀取認證資料。

### VS Code 除錯設定

先安裝 [Rust Analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer) 與 [CodeLLDB](https://marketplace.visualstudio.com/items?itemName=vadimcn.vscode-lldb)，並在 VS Code 開啟 repository 根目錄。於該目錄執行：

```bash
cargo xtask debug-setup
```

指令會在 `.vscode/` 產生 `launch.json`、`tasks.json` 與 `extensions.json`，供 VS Code 建置及以 CodeLLDB 除錯 `sysrqui`。Launch 設定由 CodeLLDB 的 [Cargo support](https://github.com/vadimcn/codelldb/blob/master/MANUAL.md#cargo-support) 選取執行檔，不依賴寫死的 `target/` 路徑；tasks 的 build 工作可單獨執行，launch 不會再呼叫該 task 以避免重複建置。

完成後在 `crates/app/src/main.rs` 設中斷點，於 Run and Debug 選取 **Debug sysrqui** 並按 F5。建置仍需要已安裝並設定好的平台 Rust/Cargo 工具鏈與 GPUI 編譯相依；macOS 需可用的 Xcode Metal Toolchain，不要以 `gpui/runtime_shaders` 繞過。

產生器只會建立不存在的檔案。若任一目標 JSON 已存在且內容不同，指令會列出衝突檔並停止，不改動任何目標檔；請先自行合併或移開既有設定再重跑。相同內容可安全重跑；`.vscode/` 是本機設定，不會納入版本控制。產生器不會建立 `settings.json`。

### Issue worktree

Loop worker 可用 `cargo xtask worktree` 在隔離目錄建立工作區。需從 repository 的主要 checkout 執行；預設工作區位於被 Git 忽略的 `.loop/worktrees/`，登記檔在 `.loop/runs/`，不會提交。`--owner` 是本次執行的短識別名稱，不是登入或鎖服務：初期採單一 coordinator 派單，跨電腦的工作狀態不會同步，接手前先查 GitHub issue、handoff 與已推送 branch。

```bash
cargo xtask worktree create --issue 18 --owner codex
cargo xtask worktree status --issue 18
cargo xtask worktree remove --issue 18 --owner codex
```

`create` 會先 `git fetch origin main`，以最新 `origin/main` 建立 `codex/issue-18-worktree`；同一 owner 重跑會驗證並復用既有工作區。`status` 只讀本機 registry 與 Git worktree 狀態，不會建立 registry。`remove` 僅接受登記工作區，且要求工作樹（含 ignored 檔）乾淨、HEAD 已在最新 `origin/main`，並使用非強制的 Git worktree／branch 移除；檢查不通過時保留資料供人工處理。移除後保留 registry 記錄供查詢。若中斷在工作區／branch 移除附近，先查看 `status`，確認資料與遠端合併狀態後可重試 `remove`；無法證明安全時會拒絕操作。初版 manifest 更新非 crash-atomic，不代表支援跨程序或跨機互斥。

### 固定驗證與任務工作包

在工作 checkout 執行：

```bash
cargo xtask verify
cargo xtask task prepare --issue 18 --owner codex
```

`verify` 依序執行 `cargo fmt --all --check`、workspace build、test 與 clippy；預設 fail-fast，未執行的步驟會標示 skipped，Cargo 輸出直接顯示在終端。每次執行都在主要 checkout 的 ignored `.loop/verify/` 留一份唯一 JSON，記錄工作區識別、branch、HEAD、dirty 狀態、固定參數、結果與耗時；失敗或 cargo 無法啟動也會記錄，舊結果不會被覆蓋。

Windows 的 `verify` 會將子 Cargo 的產物與中間建置檔放到主要 checkout 的 ignored `.loop/verify-build/`，避免 workspace build 覆寫正在執行的 xtask（[#27](https://github.com/bonaparta/SysJustRQUI/issues/27)）。若檢查器本身來自該目錄，改用 `.loop/verify-build-fallback/`；快取可重用，首次需額外編譯與磁碟空間。終端與 JSON 的 `cargo_target_dir` 會記錄本次目錄，不修改使用者全域設定；macOS 沿用原有 Cargo 設定。也可逐條執行 [四條 workspace 指令](docs/new-computer.md#4-建置與完整驗證)。

`task prepare` 需要本機已登記、owner 相符的 active worktree，並以 `gh issue view` 讀 issue 與依賴：issue 必須 open 且非 `blocked`／`needs-review`；AI 建立的 issue 必須有 `ready`，依賴 issue 必須已關閉。人類 issue 視同 ready。它會在 ignored `.loop/tasks/` 產生 Markdown 與 JSON 工作包，內容保留 issue 原文及工作區、owner、run-id、branch、base/head SHA，並連到共用規則與回報格式。issue 本文當作資料，不直接變成指令或 shell 參數；人為修改過的工作包不會被覆寫。此命令只準備工作包，不會呼叫或啟動 Codex、Claude、Gemini 或 Grok；worker 必須由 coordinator 手動指定並在包內 worktree 執行。

### 有界 Windows worker

`cargo xtask harness` 提供 Windows 本機的外部 executable worker 入口；它不會自動安裝、登入或呼叫任何 AI provider。先由 coordinator 建立 issue worktree 與工作包，再明確指定遵循工作包／結果檔協定的可執行檔。詳見 [有界 worker 操作與恢復](docs/loop-harness.md)。

Windows 使用 Job Object 控制 worker 程序樹，以本機檔案鎖避免同一 run 同時啟動，並保存執行次數與恢復紀錄；無法確認舊程序已停止時拒絕重新啟動。macOS／其他平台目前不提供相同強度的原生程序樹管理，真實啟動會明確回報 `UnsupportedContainment`，跨平台測試使用 fake worker。這不代表四種 AI 已接通，也不代表 Windows 人工操作已驗收。

### 每日試用包（#11）

Daily workflow 提供 Windows x64／ARM64、macOS Intel x64／Apple Silicon arm64 四份 ZIP；固定同一 main SHA，各架構完成 workspace fmt／build／test／clippy、release build 與相依稽核後才發布 GitHub prerelease。每日台北 07:00 開始排程，亦可在 Actions → Daily 手動觸發；排程可能延遲，公開 repo 閒置 60 天後 GitHub 可能停用排程。

以完整成功 daily Release 為基準；首次發布、有新 commit（包含文件）才建置，相同 SHA 跳過。失敗不推進基準；手動／排程互斥，失敗上傳保留 draft 供重試，不覆寫已發布版本。每份 ZIP 上限 50 MiB；成功發布後保留最近 30 個完整 daily prerelease，自動刪除更舊每日版及附件，正式版保留。無變更重跑也可重試清理；PR 先跑四架構建置與封裝但不發布或清理。

下載、版本核對、啟動、驗收與失敗恢復見 [每日包版操作](docs/daily-release.md)。Windows 使用靜態 CRT；Mac 僅 ad-hoc 簽署，未提供正式簽章／公證。產物建置成功不代表 GUI、最低 OS 或下載真機啟動已驗收。

## 目錄結構

```
Cargo.toml        workspace 設定（成員、共用版本與依賴）
crates/
  app/            sysrqui：進入點，組裝視窗與模組（glue code）
  ui/             sysrqui-ui：GPUI 共用元件與配色
  quote/          sysrqui-quote：報價資料模型，純 Rust、不依賴 gpui
  xtask/          sysrqui-xtask：專案開發輔助指令（cargo xtask）
docs/             loop engineering 流程、交接檔
```

依賴方向：`app` → `ui`、`quote`。`quote` 不依賴 gpui 與 `ui`，`ui` 也不依賴 `quote`，資料轉畫面的程式放在 `app`。之後依訂閱組裝版本時，功能模組會以 crate 的形式加進 `crates/`。

## 開發方式

本專案由多個 AI agent（Claude、Codex、Gemini、Grok）與人類共同開發。

- 共用規則：[AGENTS.md](AGENTS.md)
- Loop engineering 操作步驟：[docs/loop-engineering.md](docs/loop-engineering.md)
- Loop 工作契約、任務範本與驗收回報格式：[docs/loop-task-contract.md](docs/loop-task-contract.md)
- Loop Engineering 前置建置計畫：[docs/plan-loop-engineering-step.md](docs/plan-loop-engineering-step.md)
- 最新進度與下一步：`docs/handoff/` 底下日期最新的交接檔

## 專案狀態

- 已完成：Hello 視窗；多 agent 讀 issue、開 PR、交接的流程打通；雙平台 CI、分支保護、workspace 分層與 VS Code debug 設定產生指令。
- 待人工驗收：[#5 開發流程基礎建設](https://github.com/bonaparta/SysJustRQUI/issues/5)的 Windows／macOS VS Code 實際中斷點操作。
- 待審提案：[#9 Das 登入 UI 殼](https://github.com/bonaparta/SysJustRQUI/issues/9)、[#10 可拖拉分割欄框](https://github.com/bonaparta/SysJustRQUI/issues/10)，規格確認並標 `ready` 後開工。
- 已獲批次授權：[#13 Loop Engineering](https://github.com/bonaparta/SysJustRQUI/issues/13) 下的 #14–#19 與 [#11 每日包版](https://github.com/bonaparta/SysJustRQUI/issues/11)；依賴及實際進度以 GitHub issue／PR 與最新交接檔為準，不能跳過前置工作。
