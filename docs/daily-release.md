# 每日包版與恢復（#11）

## 已確認範圍

人類於 2026-10-19 確認四份產物：Windows x64、Windows ARM64、macOS Intel x64、macOS Apple Silicon arm64。先前 CPU 待決已解除；不得自行減為兩份。以同一 main SHA 完成全部 target 檢查與封裝才發布 prerelease；每天台北 07:00 開始，包含週末。沒有變更就跳過，文件 commit 也算變更。保留歷史，不新增 secrets。

## 操作

1. 開啟 GitHub Actions → Daily → Run workflow；正式手動／排程只選 main 快照。PR 只驗證，不發布。
2. plan 固定完整 source SHA 和台北日期，讀取所有已發布 daily prerelease；只認四份 ZIP、manifest、checksum 全齊的版本，不把 draft／缺檔版本當成功基準。
3. 四個原生 runner 分別驗證 Rust host、fmt、workspace build／test／clippy、release build。macOS 缺 Metal Toolchain 時下載，不能用 runtime_shaders 繞過。Windows 靜態 CRT 避免外部 VC runtime。
4. 封裝驗證實際 PE／Mach-O 架構，檢查 import dependencies。非系統 DLL／dylib 未封裝就拒絕產出。Mac 讀取 Mach-O 最低 deployment target，建立 .app 與 Info.plist，再 ad-hoc 簽署並驗證。
5. 產物上傳為 Actions artifacts，保留 14 天；正式 publisher 僅在全部 target 成功後下載，核對四份來源 SHA、target 與 SHA256。先建 draft、上傳全部檔案、核對遠端完整後才公開 prerelease。已公開相同 SHA 跳過，不受日期改變影響。

## 下載與回報

到 GitHub Releases 選 daily prerelease，下載相符 OS／CPU 的 ZIP，旁邊 `.sha256` 可核對完整性。完整解壓後 Windows 啟動 sysrqui.exe；macOS 開啟 SysRQUI.app。每包含 version.json，記錄 source SHA／target／相依稽核與簽章狀態。

Windows 未正式簽章；macOS 僅 ad-hoc，沒有 Developer ID 簽章或公證，系統可能阻擋。若無法開啟，回報實際系統訊息，不要求關閉平台安全機制。GUI 與最低 OS 真機驗收待完成；Mac metadata 的最低版本僅代表 binary deployment target，不能當成最低支援 OS 保證。

回報 issue 時提供 ZIP 檔名、source SHA、CPU、OS 版本、重現步驟與截圖；確認 Windows 畫面為 Win、Mac 為 Mac。#11、#18 的人工驗收不能以 CI 編譯替代。

## 失敗與重跑

- 任一 target 失敗：不發布、不推進成功基準；查看失敗 job，修復經 PR 合併後再手動執行。不得為了綠燈刪掉某個架構。
- 上傳中斷：保留 draft。相同日期與 SHA 重跑可補齊該 draft，不刪歷史；若變成隔日的新 tag，舊 draft 也保留。不完整的同名非 draft 版本拒絕覆寫，需先調查。
- 手動和排程共用 daily-release concurrency 群組，cancel-in-progress=false；GitHub 可能替換 pending run，但不會中止正在發布的 run。
- 手動開始後 main 更新：本次仍使用 plan 固定的 SHA，新 commit 留到下一輪。
- GitHub 排程可能延遲／被丟棄；公開 repo 閒置 60 天後可能停用。07:00 是開始時間，不保證可下載時間。

## 工具與驗證

`python -m unittest discover -s scripts -p 'test_*.py' -v` 測試首次發布、無變更／重複跳過、不完整發布不推進基準、架構不符、非系統相依拒絕、損毀 checksum、上傳失敗與 draft 恢復。測試用 fake 資料，不觸發真實 Release。

`scripts/daily.py package` 是 CI 在檢查通過後使用的入口，不會自行執行 Cargo；不得把手動呼叫封裝視為 workspace 已驗證。`publish` 需要標準 GITHUB_TOKEN 的 contents:write，其他 job 唯讀；不需新增 secret 或變更 repo 設定。

## 官方依據

- [GitHub 原生 runner 清單](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)：windows-2025、windows-11-arm、macos-15-intel、macos-15（arm64）；workflow 另核對 rustc host，不能靠標籤假定。
- [排程與時區](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#schedule)：使用 cron 0 7 * * * 與 Asia/Taipei。

- [Microsoft ICU 系統程式庫](https://learn.microsoft.com/en-us/windows/win32/intl/international-components-for-unicode--icu-)及 [ProcessPrng／BCryptPrimitives](https://learn.microsoft.com/en-us/windows/win32/seccng/processprng)：Windows release 實際 imports 包含 icuuc.dll、bcryptprimitives.dll，歸類為系統 DLL；這項元件依據不等於整個 app 的最低 OS 真機保證。
