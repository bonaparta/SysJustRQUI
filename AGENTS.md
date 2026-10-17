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
5. 先用 3 句話向使用者回報「現在做到哪、下一步是什麼」，並列出人類新發的 issue，**不要自己猜進度**。人類的 issue 和交接檔的下一步衝突時，先問使用者要先做哪個。有疑問先問，等使用者確認後再動手。

目前交接檔：`docs/handoff/handoff_20261018.md`

## 專案概要

- Rust 券商報價軟體，UI 使用 Zed 的 GPUI 框架。
- Crate 名稱：`sysrqui`（`Cargo.toml`，edition 2024）。
- 程式進入點：`src/main.rs`（SysRQUI 的 GPUI 範例視窗，`cargo run` 即可啟動）。
- GitHub repo：https://github.com/bonaparta/SysJustRQUI （issue 追蹤也在這裡）。

## 常用指令

```bash
cargo build
cargo run
cargo test
cargo fmt
cargo clippy
```

## Loop engineering 工作流程

完整操作步驟見 `docs/loop-engineering.md`。簡述：

1. `gh issue list` / `gh issue view <N>` 選一張 issue。
2. 從 `main` 開分支（或 worktree），例如 `feat/issue-<N>-<簡述>`。
3. 開發、測試（`cargo build`、`cargo test`、`cargo fmt`、`cargo clippy`）。
4. commit（訊息格式見下）→ push 分支 → `gh pr create`，PR 內文寫 `Closes #<N>`。
5. CI（`windows-latest`、`macos-latest`）綠燈後，由 agent 自己 merge：`gh pr merge --merge --delete-branch`。
6. `gh issue comment <N>` 回報做了什麼、PR 與 merge commit。
7. 在 `docs/handoff/` 新增或更新 `handoff_YYYYMMDD.md`，並更新本檔「目前交接檔」那一行。

PR 規則：

- **程式碼一律走 PR**，CI 綠燈才 merge。
- **只改文件**（README、`docs/`、本檔等）可以直接 push `main`，不必開 PR。交接檔要不要 push，依使用者當次指示，沒說就先問。
- **一張 issue 盡量只開一個 PR**：issue 與 PR 共用編號，PR 太多 issue 編號會跳很快。不要開純驗證用的 PR。

## 規則

- **Commit 訊息**：Conventional Commits（`feat:`、`fix:`、`docs:`、`chore:` …），內文說明原因；結尾加上自己的署名行，例如
  `Co-Authored-By: Codex <noreply@openai.com>`。
- 一次 commit 只做一件事；不要把無關變更混進來。
- **機密不進 repo**：token、`gitlab.token.config`、`.env` 等一律不 commit（已列在 `.gitignore`）。
- 不要 force push、不要改寫已 push 的歷史。
- 對外動作（留言、push、開/關 issue）要符合使用者當次的指示。
- 交接檔要寫清楚：做了什麼、驗證結果、卡住的地方、下一手要做什麼。
- **Token 用量低於 10% 就交接**：工作中定期檢查剩餘的 token／context 用量。剩不到 10% 時，停下手邊工作，先寫交接檔：正在做哪一項、做到哪一步、哪些改動還沒 commit、下一步要做什麼。目的是讓下一個 AI 直接接手，不必猜進度或重新掃整個 repo。
- **根目錄 `Readme.txt` 是使用者的個人筆記**：給人想事情、做紀錄用。AI 不要修改、不要 commit（已列在 `.gitignore`）。
- **新工作先開 issue**：基礎建設與功能開發，先在 GitHub 開 issue 寫清楚目標與驗收條件，再開始做。
- **AI 開的 issue 一律加 `ai-created` label**（`gh issue create --label ai-created ...`）。沒有這個 label 的 issue 視為人類發的需求。issue 作者都是同一個 GitHub 帳號，只能靠 label 區分。
- 文件使用繁體中文。
