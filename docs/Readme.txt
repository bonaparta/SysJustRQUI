我要用 rust 寫一支券商報價軟體
使用 Zed UI 框架產生 UI

[專案位置]
程式碼：https://github.com/bonaparta/SysJustRQUI
Issue 追蹤：https://github.com/bonaparta/SysJustRQUI/issues

[許願池]
1. 使用 loop engineering 去爬 issue，自己抓回來開發
2. 跨平台(Windows、Linux、MacOS、Android、iOS)
3. 自己產生 UI
4. 可以量測 UI 效能
5. 雲端自選股
6. 雲端頁面存檔
7. AI XS 策略、回測、選股、下單
8. 可以跑測試，讓開發可以快速迭代
9. 要有測試工具，類似 zed，可以測試單元測試、測試介面、測試api
10. 要有自動部署功能，讓開發可以快速迭代
11. 要包版，讓我們每天有版本，可以玩，再上去許願
12. 可以用喊的，加上技術分析工具

[功能測試]
1. 可以做一個rust外框，裡面放 web，像xq的軟體嗎？
2. 我想像xq有分割欄框
  2.1 我拖不了分割線
3. 我想在這台電腦上看
4. 技術分析可以有tool嗎？
  4.1 切換週期、技術指標、畫線工具、十字游標(使用者視角加入)
  4.2 有黃金分割嗎？(開發者視角加入)

[目前的困擾]
1. 我不會 loop engineering，要架構在 GitHub 上，可以產出 issue 供自己開發（設定步驟見 docs/loop-engineering.md）

[已知限制]
1. 跨平台 UI 弱點
  - 使用繪圖引擎，包裝原生 UI 跨平台不同容 <=> 自己刻傷神
  - 複雜文字排版與雙向文字（阿拉伯文、希伯來文）、複雜字形替換（Ligatures）。
  - 完善的各國語言輸入法（IME）支援（這是目前許多 Rust 自繪 UI 在中文/日文輸入時最常崩潰的地方）。
  - 螢幕閱讀器與無障礙 API（Accessibility / a11y）。
  - 多螢幕高 DPI 動態縮放、混合 GPU 渲染。
  - 完全脫離原生的文字渲染、無障礙支援與系統主題切換，記憶體消耗大且手感總差了一截
2. UI 測試自動化
  - 頁面的繪製要保持壓在 20 ms 以下 => 不會測

[已克服限制]

[已完成功能]
- Hello 視窗

[小工具]
VS Code - web base，要裝 rust-analyzer(語言分析、提示)、CodeLLDB(除錯)、Even Better TOML(toml檔語法檢查)、Error Lens(錯誤提示)
Zed - rust 原生開發環境，小又瘦，就是要裝
