# Loop Engineering 操作步驟

目標：agent 自己到 GitHub issue 抓工作 → 開分支開發 → 開 PR → CI 綠燈 merge → 回報 → 交接給下一手。
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

1. **同步程式碼**：
   ```bash
   git pull --ff-only
   ```
2. **讀交接**：打開 `docs/handoff/` 最新的 `handoff_YYYYMMDD.md`。
3. **讀 issue**：交接檔之外，人類也會直接在 GitHub 發 issue。
   ```bash
   # 人類發的需求：沒有 ai-created label 的 open issue
   gh issue list --state open --search "-label:ai-created"
   # 全部 open issue
   gh issue list
   gh issue view 1 --comments
   ```
   人類新發的 issue 要向使用者回報；和交接檔的下一步衝突時，先問使用者要先做哪個。
4. **開分支**（所有改動都走 PR，包括文件與交接檔；`main` 有分支保護，直接 push 會被拒絕）：
   ```bash
   git switch -c feat/issue-1-簡述
   ```
5. **開發與驗證**（和 CI 跑的一樣；只改 `*.md` 或 `docs/` 時 CI 會跳過編譯）：
   ```bash
   cargo fmt --check
   cargo build --all-targets
   cargo test
   cargo clippy --all-targets -- -D warnings
   ```
6. **Commit、push 分支、開 PR**（一張 issue 盡量只開一個 PR，不開純驗證用的 PR）：
   ```bash
   git add <檔案>
   git commit -m "feat: ..." -m "說明" -m "Co-Authored-By: <Agent> <email>"
   git push -u origin HEAD
   gh pr create --title "feat: ..." --body "Closes #1 ..."
   ```
7. **開啟自動 merge**（`windows-latest`、`macos-latest` 都綠燈後 GitHub 自動 merge）：
   ```bash
   gh pr merge --auto --merge --delete-branch
   gh pr checks --watch
   ```
8. **回報到 issue**（附上 PR 與 merge commit）：
   ```bash
   gh issue comment 1 --body "完成了什麼；PR #<n>；merge commit <hash>；下一步 ..."
   ```
9. **交接**：新增 `docs/handoff/handoff_YYYYMMDD.md`，更新 `AGENTS.md` 的「目前交接檔」，同樣開分支、走 PR。要不要開 PR 依使用者當次指示，沒說就先問。

## 注意

- 機密（token、`gitlab.token.config`）不進 repo。
- 不 force push。
- AI 開 issue 一律加 `ai-created` label：
  ```bash
  gh issue create --label ai-created --title "..." --body-file <檔案>
  ```
  issue 作者都是同一個 GitHub 帳號，沒有這個 label 的 issue 視為人類發的需求。
- 長篇留言可以改用 `--body-file <檔案>`，避免 shell 跳脫問題。
