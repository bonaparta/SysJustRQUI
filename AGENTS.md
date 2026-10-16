# AGENTS.md

本專案由多個 AI agent（Claude、Codex、Gemini、Grok）與人類共同開發。
這份檔案是**所有 agent 共用的唯一規則來源**；`CLAUDE.md`、`GEMINI.md` 只是指回這裡。

## 開工第一件事

1. 讀 `docs/handoff/` 底下**日期最新**的 `handoff_YYYYMMDD.md`，那是上一手留下的進度與下一步。
2. 讀 `docs/Readme.txt`（願景、許願池、已知限制）。
3. 先用 3 句話向使用者回報「現在做到哪、下一步是什麼」，**不要自己猜進度**；有疑問先問，等使用者確認後再動手。

目前交接檔：`docs/handoff/handoff_20261016.md`

## 專案概要

- Rust 券商報價軟體，UI 使用 Zed 的 GPUI 框架。
- Crate 名稱：`xqfantasy`（`Cargo.toml`，edition 2024）。
- 程式進入點：`src/main.rs`；範例：`src/bin/hello_gpui.rs`。
- GitHub repo：https://github.com/bonaparta/SysJustRQUI （issue 追蹤也在這裡）。

## 常用指令

```bash
cargo build
cargo run
cargo run --bin hello_gpui
cargo test
cargo fmt
cargo clippy
```

## Loop engineering 工作流程

完整操作步驟見 `docs/loop-engineering.md`。簡述：

1. `gh issue list` / `gh issue view <N>` 選一張 issue。
2. 開發、測試（`cargo build`、`cargo test`）。
3. commit（訊息格式見下）→ `git push`。
4. `gh issue comment <N>` 回報做了什麼、commit hash。
5. 在 `docs/handoff/` 新增或更新 `handoff_YYYYMMDD.md`，並更新本檔「目前交接檔」那一行。

## 規則

- **Commit 訊息**：Conventional Commits（`feat:`、`fix:`、`docs:`、`chore:` …），內文說明原因；結尾加上自己的署名行，例如
  `Co-Authored-By: Codex <noreply@openai.com>`。
- 一次 commit 只做一件事；不要把無關變更混進來。
- **機密不進 repo**：token、`gitlab.token.config`、`.env` 等一律不 commit（已列在 `.gitignore`）。
- 不要 force push、不要改寫已 push 的歷史。
- 對外動作（留言、push、開/關 issue）要符合使用者當次的指示。
- 交接檔要寫清楚：做了什麼、驗證結果、卡住的地方、下一手要做什麼。
- 文件使用繁體中文。
