# AGENTS.md

本專案由多個 AI agent（Claude、Codex、Gemini、Grok）與人類共同開發。
這份檔案是**所有 agent 共用的唯一規則來源**；`CLAUDE.md`、`GEMINI.md` 只是指回這裡。

## 開工第一件事

工作來源有兩個：**交接檔**（上一手 AI 留的）與 **GitHub issue**（人類也會直接在上面發需求）。兩個都要看。

1. `git pull --ff-only`，確保讀到的是最新的交接檔與文件。
2. 讀 `docs/handoff/` 底下**日期最新**的 `handoff_YYYYMMDD.md`，那是上一手留下的進度與下一步。
3. 讀 `README.md`（專案目標、技術選型、開發方式）。
4. 撈人類發的 issue：
   ```bash
   gh issue list --state open --search "-label:ai-created"
   ```
   沒有 `ai-created` label 的 open issue 都是人類發的需求。交接檔沒提到的，就是上一手之後新發的。
5. 用 3 句話回報「現在做到哪、下一步是什麼」，並列出人類新發的 issue，**不要自己猜進度**。回報完**直接開工**，不必等確認：依「自主範圍」挑下一張可做的 issue，一路做到 merge。

目前交接檔：`docs/handoff/handoff_20261018.md`

## 專案概要

- Rust 券商報價軟體，UI 使用 Zed 的 GPUI 框架。
- Cargo workspace（edition 2024），程式放在 `crates/`：
  - `crates/app`（package `sysrqui`）：進入點 `crates/app/src/main.rs`，組裝視窗與模組（glue code），`cargo run` 即可啟動。
  - `crates/ui`（package `sysrqui-ui`）：GPUI 共用元件與配色。
  - `crates/quote`（package `sysrqui-quote`）：報價資料模型，純 Rust。
  - `crates/xtask`（package `sysrqui-xtask`）：開發輔助指令；`cargo xtask debug-setup` 產生 VS Code 除錯設定。
- 依賴方向：`app` → `ui`、`quote`。**`quote` 不可依賴 gpui 或 `ui`；`ui` 不依賴 `quote`**，資料轉畫面的程式放在 `app`。
- GitHub repo：https://github.com/bonaparta/SysJustRQUI （issue 追蹤也在這裡）。

## 常用指令

```bash
cargo build --workspace --all-targets
cargo run
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

## Loop engineering 工作流程

完整操作步驟見 `docs/loop-engineering.md`。簡述：

1. `gh issue list` / `gh issue view <N>` 選一張 issue。
2. 從 `main` 開分支（或 worktree），例如 `feat/issue-<N>-<簡述>`。
3. 開發、測試（使用上方完整 workspace 檢查指令，涵蓋 xtask）。
4. commit（訊息格式見下）→ push 分支 → `gh pr create`，PR 內文寫 `Closes #<N>`。
5. 開啟自動 merge：`gh pr merge --auto --merge --delete-branch`，CI（`windows-latest`、`macos-latest`）綠燈後 GitHub 自動 merge。
6. `gh issue comment <N>` 回報做了什麼、PR 與 merge commit。
7. 在 `docs/handoff/` 新增或更新 `handoff_YYYYMMDD.md`，並更新本檔「目前交接檔」那一行。

PR 規則：

- **所有改動都走 PR，包括文件與交接檔**。`main` 有分支保護（ruleset `protect-main`），直接 push 會被拒絕，也沒有人可以略過。
- 只改 `*.md` 或 `docs/` 時，CI 會跳過編譯、直接回報綠燈，PR 很快就會 merge。
- 文件盡量併進該 issue 的 PR，不要為了小改單獨開 PR。交接檔照同樣流程開 PR、auto-merge，不必先問。
- **一張 issue 盡量只開一個 PR**：issue 與 PR 共用編號，PR 太多 issue 編號會跳很快。不要開純驗證用的 PR。

## 自主範圍：什麼時候不必問

人類只在 issue 這一層把關：**issue 是 `ready`，就代表目標與驗收條件已同意**，agent 從開分支到 merge、回報、寫交接檔一路做完，中間不問。

| label | 意思 |
|---|---|
| `ready` | 可以開工。人類發的 issue 視同 `ready`；AI 開的 issue 要人類加上 `ready` 才能做 |
| `in-progress` | 有 agent 正在做，其他 agent 不要領。開工時加上，merge 後移除 |
| `blocked` | 在等人類回答，問題寫在 issue 留言裡 |
| `needs-review` | 已 merge，但有自動測試驗不到的部分（例如 UI 外觀），請人類看 |

相依關係在 issue 內文寫 `depends on #N`；前置 issue 沒關閉前不要開工。

**不必問、直接做**：在 `ready` issue 範圍內寫程式、開分支、push 分支、開 PR、`gh pr merge --auto`、在 issue 留言回報、關閉自己完成的 issue、開 `ai-created` issue 記錄新發現的工作、寫交接檔並開 PR。

**只有下列情況要停**：

- 改 repo 設定、分支保護、權限、secrets。
- 刪除資料（分支除外）、force push。
- 規格缺漏，猜了可能白做（例如 API 網址、參數還沒給）。
- 做到一半發現要改 issue 的範圍或驗收條件。

**停的方式是非同步的，不要原地等**：在 issue 留言寫清楚問題與選項（附上推薦），加 `blocked` label，接著去做下一張 `ready` issue。人類回答後把 label 改回 `ready`。沒有其他 `ready` issue 可做時，才寫交接檔結束這一輪。

選 issue 的順序：人類發的 issue 優先，再來是交接檔的下一步，同類依 issue 編號由小到大。

## 規則

- **Commit 訊息**：Conventional Commits（`feat:`、`fix:`、`docs:`、`chore:` …），內文說明原因；結尾加上自己的署名行，例如
  `Co-Authored-By: Codex <noreply@openai.com>`。
- 一次 commit 只做一件事；不要把無關變更混進來。
- **機密不進 repo**：token、`gitlab.token.config`、`.env` 等一律不 commit（已列在 `.gitignore`）。
- 不要 force push、不要改寫已 push 的歷史。
- 對外動作在「自主範圍」內不必再問；使用者當次另有指示時，以當次指示為準。
- 交接檔要寫清楚：做了什麼、驗證結果、卡住的地方、下一手要做什麼。
- **Token 用量低於 10% 就交接**：工作中定期檢查剩餘的 token／context 用量。剩不到 10% 時，停下手邊工作，先寫交接檔：正在做哪一項、做到哪一步、哪些改動還沒 commit、下一步要做什麼。目的是讓下一個 AI 直接接手，不必猜進度或重新掃整個 repo。
- **根目錄 `Readme.txt` 是使用者的個人筆記**：給人想事情、做紀錄用。AI 不要修改、不要 commit（已列在 `.gitignore`）。
- **新工作先開 issue**：基礎建設與功能開發，先在 GitHub 開 issue 寫清楚目標與驗收條件，再開始做。
- **AI 開的 issue 一律加 `ai-created` label**（`gh issue create --label ai-created ...`）。沒有這個 label 的 issue 視為人類發的需求。issue 作者都是同一個 GitHub 帳號，只能靠 label 區分。
- 文件使用繁體中文。
