# 有界 worker 操作與恢復

此入口面向 Windows 本機的單一 coordinator／單一 worker，延續 [Loop 工作契約](loop-task-contract.md) 與 [AGENTS.md](../AGENTS.md)。它只啟動明確指定的外部 executable，不提供 Codex／Claude／Gemini／Grok 登入或 provider adapter；不會自動建立 PR、留言或合併。人工發布仍由 coordinator 依既有流程處理。

## 啟動與協定

先確認 GitHub issue 已核准且依賴關閉，從主要 checkout 建立 owner 相符的登記 worktree，並準備工作包。以下 issue 編號與 worker 路徑只是範例，不代表已領單或存在可用 provider：

```powershell
cargo xtask worktree create --issue 19 --owner Codex
cargo xtask task prepare --issue 19 --owner Codex
cargo xtask harness run-once --issue 19 --owner Codex --worker C:\tools\loop-worker.exe --timeout-secs 300 --max-attempts 2
```

worker 是絕對路徑的 executable，工作目錄為登記 worktree；執行器直接傳 argv，不經 shell，也不將 issue 原文拼成命令。worker 依序收到：

```text
--work-package <本機 JSON 絕對路徑>
--result <本次 attempt 的 JSON 結果路徑>
--run-id <已登記 run-id>
--issue <issue 編號>
--owner <owner>
--attempt <本次序號>
```

只有正常結束並寫出符合身分的結果，才視為 worker 完成；結果範例：

```json
{
  "protocol_version": 1,
  "issue": 19,
  "owner": "Codex",
  "run_id": "由工作包提供的實際 run-id",
  "status": "completed",
  "summary": "實際變更、驗證與人工驗收狀態"
}
```

遇到規格缺漏或需要人類回答時，worker 可將 `status` 寫為 `blocked`，在 `summary` 說明原因與恢復條件，並以 exit code 0 結束以表示結果協定有效；runner 保存紀錄、以非零狀態停止並交給 coordinator，不自動重試此結果。worker 的非零 exit code 則分類為執行失敗。

`summary` 最多 16 KiB，不應含 token、密碼或登入資訊；本機紀錄可能包含工作區絕對路徑，不要整份貼到公開 issue／PR。worker 完成不等於功能已通過 CI、PR 已合併或 GUI 已驗收，coordinator 仍須審查 diff 與證據。

## 停止與恢復

```powershell
cargo xtask harness cancel --issue 19 --owner Codex
cargo xtask harness resume --issue 19 --owner Codex --worker C:\tools\loop-worker.exe --timeout-secs 300 --max-attempts 2
```

`timeout-secs` 與 `max-attempts` 都必須明確提供；最多 10 次、所有 attempt 的 worker 時間預算合計最多 3600 秒，不在 resume 重設額度。程序清理另有短時限；逾時、取消、失敗、阻塞或額度耗盡會回傳非零，不把停止當成成功完成。

`resume` 必須使用原有 owner、worker 與限制，沿用原工作區與 run-id，已消耗的 attempt 不歸零。它先確認程序停止與本機／遠端狀態，不因 PID 不存在、label 逾時或網路錯誤就搶單；有未提交／未推送變更時保留資料，不執行 reset／clean。完成或額度耗盡的 run 不會再次啟動 worker。

`cancel` 是冪等請求，回報「已提出」不代表 worker 已停止。明確執行 `resume` 會確認取消標記移轉當下已存在的請求；coordinator 不應並行執行相反的 cancel／resume 操作。若要取消已恢復的執行，請在 resume 的確認階段之後重新提出取消，不能假設重疊的 cancel 一定優先。

執行紀錄位於主要 checkout 被忽略的 `.loop/harness/`；不要刪鎖檔、改寫 journal 或手動清除未確認停止的 run。紀錄損毀、owner／指派不一致、停止無法確認或 GitHub 無法核對時，保留工作區與證據，由 coordinator 判斷下一步。恢復還須位於原 Windows session；跨登入 session 不能以查不到 Local Job 就推定程序已停。

每個 Git／GitHub 探測另有 15 秒時限；worker 時間預算不包含這些核對與最長 30 秒的程序清理。此鎖只協調同機本機檔案系統，不是跨機排程服務；token／context 無法取得時記未知，不能用帳戶配額推算。

## 程序生命週期與驗收邊界

Windows 以 `PROC_THREAD_ATTRIBUTE_JOB_LIST` 在建立程序時加入具 `KILL_ON_JOB_CLOSE` 的 Job，避免先建立程序再指派 Job 的 crash 窗口；不允許 breakaway，也不繼承 Job handle。主程序結束後仍須等 Job 的 active process 數量歸零，整棵程序樹共用同一時限；若後代仍存活至逾時，結果是 timed-out，清理整棵樹後才允許重試／恢復。取消也會清理程序樹，不只停止主程序。

Job handle 不會因一般程序全部結束就自動成為 signaled，不能用等待 Job handle 代替停止確認；此處依主程序 handle 與 Job accounting 查詢判斷。Job 並不是惡意程式 sandbox，worker 不得透過外部服務或其他繞開 Job 的方式啟動工作。

macOS／其他平台目前僅執行 fake 狀態機測試，真實 worker 啟動會回報不支援；Windows 原生測試與兩平台編譯／測試由 CI 提供證據，實際使用者 provider 的登入、非互動呼叫與 Windows 人工閉環仍須另外記錄，不能假稱已驗收。

官方依據：[Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)、[建立程序時加入 Job](https://devblogs.microsoft.com/oldnewthing/20230209-00/?p=107812/)、[等待 Job 程序結束的差異](https://devblogs.microsoft.com/oldnewthing/20130405-00/?p=4743)。
