# 新電腦開工：Windows／macOS（#28）

目標是從一台尚未設定的電腦，取得 SysJustRQUI、完成 workspace 檢查，啟動 Hello App 並設定除錯。以下命令從 repository 根目錄執行；Windows 使用 PowerShell，macOS 使用終端機。先完成對應平台的安裝，再做共用步驟。

## 1. 安裝工具

兩平台都需要 [Git](https://git-scm.com/downloads)、[Rust／rustup](https://rust-lang.org/tools/install/) 與 [GitHub CLI（gh）](https://cli.github.com/)。依官方安裝程式操作，安裝後重開終端機，使 PATH 生效。Rust 安裝選 stable；本專案目前未固定 Rust 版本，CI 也使用 stable，本文件不另訂版本。

### Windows

1. 安裝 Visual Studio 或 [Build Tools](https://visualstudio.microsoft.com/downloads/#build-tools-for-visual-studio)，在 Visual Studio Installer 選「使用 C++ 的桌面開發」，包含 MSVC C++ 建置工具與 Windows SDK；已有 Visual Studio 時，檢查是否裝齊該工作負載。
2. 用 rustup 安裝符合本機 CPU 的 **MSVC** 工具鏈：x64 為 `x86_64-pc-windows-msvc`，ARM64 為 `aarch64-pc-windows-msvc`；ARM64 建置也須安裝對應的 MSVC ARM64 工具。先做本機架構的開發，不要求新電腦一次交叉建置四架構。
3. 開啟 Visual Studio 的 Developer PowerShell，執行下一節的版本檢查。一般 PowerShell 若找不到 linker／SDK，也先改用該開發者終端機核對安裝。

使用 Windows 原生工具鏈；這份流程不涵蓋 WSL。遇到 `link.exe` 錯誤時確認沒有選到 Git Bash 的同名 coreutils 程式，詳見疑難排解。

### macOS

1. 安裝完整 [Xcode](https://developer.apple.com/xcode/)，開啟一次完成初始元件與授權設定；只有 Command Line Tools 不保證具備 GPUI 編譯 shader 所需的 Metal 工具。
2. 若 Xcode 裝在預設位置，依序執行下列命令；若使用其他位置，將路徑改成實際的 Xcode Developer 目錄。

   ```bash
   sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer
   sudo xcodebuild -license accept
   xcodebuild -version
   xcrun -sdk macosx metal --version
   ```

3. 最後一條若回報缺少 Metal Toolchain，執行 `xcodebuild -downloadComponent MetalToolchain`，完成後再跑一次 `xcrun -sdk macosx metal --version`；仍失敗就先處理 Xcode／元件安裝，不啟用 `gpui/runtime_shaders` 繞過。
4. 用 rustup 安裝本機架構的 stable Rust；Apple Silicon 的 host 應為 `aarch64-apple-darwin`，Intel 為 `x86_64-apple-darwin`。Apple Silicon 若出現 x86_64，先確認終端機與工具鏈是否以 Rosetta 執行，再選定開發環境。

### 共用版本檢查

```text
git --version
rustup show active-toolchain
rustc -vV
cargo --version
gh --version
rustup component add rustfmt clippy
cargo fmt --version
cargo clippy --version
```

每條都應成功，`rustc -vV` 的 `host` 應符合上方平台／CPU。找不到命令時先重開終端機、檢查安裝及 PATH；Rust 工具通常位於 Windows 的 `%USERPROFILE%\.cargo\bin` 或 macOS 的 `~/.cargo/bin`。

VS Code、Rust Analyzer 與 CodeLLDB 是下方除錯流程需要的選用工具；Codex、Claude、Gemini、Grok CLI 都不是建置或啟動 Hello App 的必要工具。Python 是 daily 封裝腳本的工具，不是這份 Rust Hello 開發流程的前置需求。

## 2. 取得程式與設定 GitHub 認證

在自己選定的開發目錄執行，避免 clone 到已存在且含資料的同名目錄：

```text
git clone https://github.com/bonaparta/SysJustRQUI.git
cd SysJustRQUI
git status --short
git branch --show-current
```

公開 repo 的 HTTPS clone 不需要登入；預期狀態沒有輸出、分支為 `main`。已有 checkout 時先檢查修改，再用 `git pull --ff-only` 同步，不覆蓋未提交工作；若不能 fast-forward，保留現況並查明分支差異。

要讀寫 issue、建立 PR 或推送時，由使用者在自己的終端機完成 GitHub 登入：

```text
gh auth login --hostname github.com --git-protocol https --web
gh auth status --hostname github.com
gh auth setup-git --hostname github.com
```

依瀏覽器提示自行授權；最後一條設定 Git 使用 gh 的認證。確認登入的是預期帳號，並具有需要的 repository 存取權限。不要將 token、密碼或含認證的 URL 貼進 issue、文件或 commit；若環境已有組織指定的認證方式，沿用該方式，不另寫入憑證檔。GitHub CLI 登入與 Git 推送權限是不同的檢查，登入成功不等於有權 push。

準備開發前先讀 [AGENTS.md](../AGENTS.md)、`docs/handoff/` 日期最新的交接檔及 [README](../README.md)，再查人類需求：

```text
gh issue list --repo bonaparta/SysJustRQUI --state open --search "-label:ai-created"
```

依 issue 的授權、依賴及 owner 狀態選工作；所有改動都走分支／PR。隔離工作區與領單方式見 [Loop Engineering](loop-engineering.md)，不要直接在 main 寫功能或重複領取 `in-progress` issue。

## 3. 檢查開發環境

```text
cargo xtask doctor
```

首次會先編譯 xtask，因此 Rust 或原生建置工具壞掉時，可能還沒印出 doctor 報告就失敗。成功應列出 OS／架構及 `git`、`cargo`、`rustc`、`gh` 版本，最後顯示 `Doctor checks passed.`；選用 AI CLI 未找到不影響通過。

doctor 只檢查必要工具能否執行版本命令、AI CLI 是否存在，不會安裝或登入；它也不驗證 GitHub 權限、MSVC／SDK、Metal、GPU、GUI 或 AI provider 能否工作。必須繼續執行下一節，不能以 doctor 通過代替完整驗證。

## 4. 建置與完整驗證

在 repo 根目錄**逐條執行**，每條成功才繼續；PowerShell 可看 `$LASTEXITCODE`，macOS 可看 `$?`，應為 `0`：

```text
cargo fmt --all --check
cargo build --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

預期 fmt 沒有格式差異、build 成功、所有 workspace 測試無失敗、clippy 無警告。這些指令涵蓋 xtask；單跑 `cargo build` 只涵蓋 default-members，不能替代 workspace 檢查。首次下載相依與編譯可能較久，需能存取 Cargo registry／相依來源。

也可用 `cargo xtask verify` 依序執行同樣四步並留下 JSON；Windows 會使用獨立的建置快取，避免 [#27 的自身執行檔鎖衝突](https://github.com/bonaparta/SysJustRQUI/issues/27)，首次需額外編譯與磁碟空間。不要同時執行多個驗證或手動啟動驗證快取內的產物。若使用尚未包含 #27 修正的舊版本，Windows 先用上面四條命令；手動執行不會產生 verify JSON，請記錄各步結束碼及 commit SHA，失敗後未執行的項目明寫「未執行」。紀錄與快取位置見 [README](../README.md#固定驗證與任務工作包)。

## 5. 啟動與除錯

```text
cargo run
```

預期開啟標題為 `SysRQUI - Zed GPUI Hello World` 的視窗，顯示歡迎文字、台積電示範報價卡片，以及 Windows 的 `Platform: Win` 或 macOS 的 `Platform: Mac`。報價是固定範例，「即時撮合引擎已就緒」也是示範文案，不代表已連線行情或交易服務。操作完成後關閉本次啟動的視窗，讓終端機結束執行；不要停止其他人的程序。

若要在 VS Code 除錯，安裝 Rust Analyzer 與 CodeLLDB，以 repo 根目錄開啟，再執行 `cargo xtask debug-setup`；在 `crates/app/src/main.rs` 的 `main()` 內設定中斷點，選 **Debug sysrqui** 後按 F5。命中中斷點、繼續後開出視窗才算完成這項操作；設定檔生成成功不等於除錯已驗收。既有 JSON 衝突時產生器會停止，請人工合併，不直接覆蓋；細節見 [VS Code 除錯設定](../README.md#vs-code-除錯設定)。

## 疑難排解與驗收紀錄

| 現象 | 下一步 |
|---|---|
| 找不到 `gh`／`cargo` 等命令 | 確認安裝目錄在 PATH，重開終端機；不要只因工具裝過就略過版本檢查。 |
| Windows 缺 linker、SDK，或 `link.exe` 出現 coreutils 錯誤 | 核對 C++ 工作負載與本機架構工具；在 Developer PowerShell 執行 `where.exe link`，排除 Git Bash 的 `usr/bin/link.exe`。 |
| macOS 缺 Metal Toolchain | 回到平台安裝步驟，確認目前選用的完整 Xcode 與 `metal --version`；不要用 runtime_shaders 繞過。 |
| Windows verify 出現無法覆寫 xtask.exe／Access denied | 核對是否為包含 #27 修正的版本，及錯誤路徑是否有其他程序使用；只關閉自己啟動且已確認的程序，保留錯誤，必要時改跑第 4 節四條指令。 |
| build 通過，但沒有視窗或發生 GPU 錯誤 | 保存原始錯誤、OS／CPU／GPU、commit SHA 與啟動方式，回報 issue；不能以 CI 成功推定 GUI 已驗收。 |
| GitHub 認證或 push 失敗 | 由使用者檢查 gh 登入帳號、repo 權限與 Git 的認證設定；保留錯誤訊息，回報前移除憑證。 |

新機驗收時記錄 `git rev-parse HEAD`、OS 版本、CPU／Rust host、各檢查結果、視窗截圖與中斷點結果；未操作的步驟明寫未驗證。這份指引不宣稱每種乾淨新機或最低 OS 已驗收；[#5](https://github.com/bonaparta/SysJustRQUI/issues/5) 的實際除錯、[#18](https://github.com/bonaparta/SysJustRQUI/issues/18) 的平台畫面及 [#19](https://github.com/bonaparta/SysJustRQUI/issues/19) 的真實 worker 閉環仍按各 issue 追蹤。

若只要下載試用，請看 [每日包版與恢復](daily-release.md) 及 [#11 最新回報](https://github.com/bonaparta/SysJustRQUI/issues/11)。四份產物範圍是 Windows x64／ARM64、macOS Intel x64／Apple Silicon arm64，台北每日 07:00 開始、同 SHA 全數成功才發布；正式發布是否完成、GUI／最低 OS 驗收以該 issue 的證據為準，不把 PR artifacts 當正式下載包，也不把建置成功當實機已驗收。

## 官方參考

- [Rust 安裝與 PATH](https://rust-lang.org/tools/install/)、[Microsoft 的 Windows Rust 環境設定](https://learn.microsoft.com/en-us/windows/dev-environment/rust/setup)。
- [Zed 的 macOS 建置前置需求](https://zed.dev/docs/development/macos)、[Apple 的 Xcode 元件安裝說明](https://developer.apple.com/documentation/xcode/downloading-and-installing-additional-xcode-components)；此處只採用 GPUI 所需的平台設定，未將整套 Zed 的其他相依列為本專案必要工具。
- [GitHub CLI 登入](https://cli.github.com/manual/gh_auth_login)與 [Git 認證整合](https://cli.github.com/manual/gh_auth_setup-git)。
