# Loop Engineering 操作步驟

目標：agent 自己到 GitHub issue 抓工作 → 開分支開發 → 開 PR → CI 綠燈 merge → 回報 → 交接給下一手。
Repo：https://github.com/bonaparta/SysJustRQUI
Issue 追蹤：https://github.com/bonaparta/SysJustRQUI/issues
本專案的程式碼與 issue 均由 GitHub 管理；`docs/issue/` 是本地需求草稿，檔名不代表 GitHub issue 編號。

跨工具共用的 issue 欄位、worker 任務提示、blocked 提問、checkpoint、完成回報與範例見[Loop 工作契約](loop-task-contract.md)。本文件描述操作順序；共用規則以 `AGENTS.md` 為準。

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
   人類新發的 issue 要向使用者回報。挑 issue 的順序與什麼時候要停，見 `AGENTS.md`「自主範圍」。
   `ai-created` issue 預設須由人類確認為 `ready` 才能開工。人類可在當次對話明確授權 coordinator 將指定的一批 issue 標為 `ready`；授權只適用該批與該次安排，不改變其他 AI 建立 issue 的規則。coordinator 派工時先確認依賴、owner、run-id 與 branch/worktree，再記錄 `in-progress`：
   ```bash
   gh issue list --label ready
   gh issue edit 1 --add-label in-progress
   ```
   `in-progress` label 不是互斥鎖；只由已指派的 coordinator 啟動 worker。逾時先查 checkpoint、owner、worktree 與 PR，不能直接搶單或清除工作。遇到要停的情況，依[Blocked 提問格式](loop-task-contract.md#blocked-提問格式)記錄問題與選項並改成 `blocked`，然後依授權和依賴安排下一張：
   ```bash
   gh issue comment 1 --body "問題：...；選項 A / B；推薦 A"
   gh issue edit 1 --add-label blocked --remove-label in-progress
   ```
4. **開分支／worktree**（每個 issue 對應一個 owner、run-id、branch 與 worktree；位置由本機設定決定。所有改動都走 PR，包括文件與交接檔；`main` 有分支保護，直接 push 會被拒絕）：
   ```bash
   cargo xtask worktree create --issue 1 --owner codex
   ```
   指令只支援從主要 checkout 操作，先抓取 `origin/main`，預設在忽略的 `.loop/worktrees/` 建立 `codex/issue-1-worktree`，並在主要 checkout 的 `.loop/runs/` 留本機 registry。用 `cargo xtask worktree status --issue 1` 查看狀態；合併且確認工作樹乾淨後，用 `cargo xtask worktree remove --issue 1 --owner codex` 回收。移除會拒絕 dirty、untracked、ignored、尚未合併或 owner 不符的工作區，不會 force 清除。首次使用 worktree 命令前也可直接使用一般 Git 分支方式；跨機接手以 GitHub issue／PR／handoff 和已推送 branch 為準，這份本機 registry 不會跨機同步。
5. **開發與驗證**（和 CI 跑的一樣；只改 `*.md` 或 `docs/` 時 CI 會跳過編譯）：
   ```bash
   cargo fmt --all --check
   cargo build --workspace --all-targets
   cargo test --workspace
   cargo clippy --workspace --all-targets -- -D warnings
   ```
6. **Commit、push 分支、開 PR**（一張 issue 盡量只開一個 PR，不開純驗證用的 PR。全部驗收已可由 CI／合併完成時，PR 描述可用 `Closes #N`；尚待人工驗收時用 `Refs #N`，issue 保持開啟並標為 `needs-review`）：
   ```bash
   git add <檔案>
   git commit -m "feat: ..." -m "說明" -m "Co-Authored-By: <Agent> <email>"
   git push -u origin HEAD
   gh pr create --title "feat: ..." --body "Refs #1 ..."
   ```
7. **開啟自動 merge**（`windows-latest`、`macos-latest` 都綠燈後 GitHub 自動 merge）：
   ```bash
   gh pr merge --auto --merge --delete-branch
   gh pr checks --watch
   ```
8. **回報到 issue**（附上 PR、merge commit、驗證結果和人工驗收狀態；依[完成回報格式](loop-task-contract.md#完成回報格式)填寫）：
   ```bash
   gh issue comment 1 --body "完成了什麼；PR #<n>；merge commit <hash>；下一步 ..."
   ```
9. **交接**：新增 `docs/handoff/handoff_YYYYMMDD.md`，更新 `AGENTS.md` 的「目前交接檔」，同樣開分支、走 PR、auto-merge，不必先問。

## 注意

- 機密（token、`gitlab.token.config`）不進 repo。
- 不 force push。
- 初期只由單一 coordinator 派單；`in-progress` label 不是跨程序或跨機原子鎖。相同 issue 的重跑先檢查 owner、manifest、worktree 與 PR，再決定復用或人工恢復，不逾時搶單。初版 registry 寫入不是 crash-atomic；工作區或 registry 不一致時保留資料並人工檢查，不強制清除。
- AI 開 issue 一律加 `ai-created` label：
  ```bash
  gh issue create --label ai-created --title "..." --body-file <檔案>
  ```
  issue 作者都是同一個 GitHub 帳號，沒有這個 label 的 issue 視為人類發的需求。
- 長篇留言可以改用 `--body-file <檔案>`，避免 shell 跳脫問題。
