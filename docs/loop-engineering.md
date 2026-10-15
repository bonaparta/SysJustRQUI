# Loop Engineering 操作步驟

目標：agent 自己到 GitHub issue 抓工作 → 開發 → push → 回報 → 交接給下一手。
Repo：https://github.com/bonaparta/SysJustRQUI
Issue 追蹤：https://github.com/bonaparta/SysJustRQUI/issues
本專案的程式碼與 issue 均由 GitHub 管理；`docs/issue/` 是本地需求草稿，檔名不代表 GitHub issue 編號。

## 一次性環境設定（每台電腦做一次）

1. 安裝 GitHub CLI：
   ```bash
   winget install --id GitHub.cli -e
   ```
   裝完要**重開終端機**，PATH 才會有 `gh`。
2. 登入 GitHub（會開瀏覽器，貼上畫面上的一次性代碼）：
   ```bash
   gh auth login --hostname github.com --git-protocol https --web
   ```
3. 讓 `git push` 使用 gh 的認證：
   ```bash
   gh auth setup-git
   ```
4. 驗證：
   ```bash
   gh auth status
   gh issue list --repo bonaparta/SysJustRQUI
   ```

## 每一輪 loop

1. **讀交接**：打開 `docs/handoff/` 最新的 `handoff_YYYYMMDD.md`。
2. **同步程式碼**：
   ```bash
   git pull --ff-only
   ```
3. **讀 issue**：
   ```bash
   gh issue list
   gh issue view 1 --comments
   ```
4. **開發與驗證**：
   ```bash
   cargo build
   cargo test
   ```
5. **Commit 與 push**：
   ```bash
   git add <檔案>
   git commit -m "feat: ..." -m "說明" -m "Co-Authored-By: <Agent> <email>"
   git push
   ```
6. **回報到 issue**（附上 commit hash）：
   ```bash
   git log -1 --format=%H
   gh issue comment 1 --body "完成了什麼；commit <hash>；下一步 ..."
   ```
7. **交接**：新增 `docs/handoff/handoff_YYYYMMDD.md`，更新 `AGENTS.md` 的「目前交接檔」，然後 commit + push。

## 注意

- 機密（token、`gitlab.token.config`）不進 repo。
- 不 force push。
- 長篇留言可以改用 `--body-file <檔案>`，避免 shell 跳脫問題。
