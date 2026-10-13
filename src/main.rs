use clap::{Args, Parser, Subcommand};

/// XQ Fantasy - 報價與交易指令列工具
#[derive(Parser, Debug)]
#[command(name = "xqfantasy")]
#[command(author = "XQ Developer")]
#[command(version = "0.1.0")]
#[command(about = "券商即時報價與市場監控 Console Application", long_about = None)]
struct Cli {
    /// 啟用詳細日誌輸出
    #[arg(short, long, global = true)]
    verbose: bool,

    /// 指定配置檔路徑
    #[arg(short, long, value_name = "FILE")]
    config: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// 查詢單檔或多檔標的即時報價
    Quote(QuoteArgs),

    /// 啟動即時盤面監控模式
    Monitor(MonitorArgs),

    /// 測試與報價主機之連線狀態
    Ping {
        /// 報價伺服器主機位置
        #[arg(default_value = "quote.twse.com.tw:443")]
        server: String,
    },
}

#[derive(Args, Debug)]
struct QuoteArgs {
    /// 股票或商品代碼 (例如: 2330, 0050)
    #[arg(default_value = "2330")]
    symbol: String,

    /// 市場別 (twse, otc, futures)
    #[arg(short, long, default_value = "twse")]
    market: String,
}

#[derive(Args, Debug)]
struct MonitorArgs {
    /// 監控的商品代碼清單 (多個以空格分隔)
    #[arg(required = true)]
    symbols: Vec<String>,

    /// 更新頻率 (秒)
    #[arg(short, long, default_value_t = 1)]
    interval: u64,
}

fn main() {
    let cli = Cli::parse();

    if cli.verbose {
        println!("[DEBUG] Verbose mode enabled");
        if let Some(cfg) = &cli.config {
            println!("[DEBUG] Using configuration file: {}", cfg);
        }
    }

    match &cli.command {
        Some(Commands::Quote(args)) => {
            handle_quote(args);
        }
        Some(Commands::Monitor(args)) => {
            handle_monitor(args);
        }
        Some(Commands::Ping { server }) => {
            handle_ping(server);
        }
        None => {
            // 未輸入任何子指令時，預設執行 2330 報價示範
            let default_quote = QuoteArgs {
                symbol: "2330".to_string(),
                market: "twse".to_string(),
            };
            handle_quote(&default_quote);
        }
    }
}

fn handle_quote(args: &QuoteArgs) {
    println!("========================================");
    println!("  市場報價查詢 (即時撮合快照)");
    println!("========================================");
    println!("代碼: {}", args.symbol);
    println!("市場: {}", args.market);
    println!("----------------------------------------");
    // 示範模擬報價資料
    println!("成交價: 980.00 | 漲跌: +15.00 (+1.55%)");
    println!("最高價: 985.00 | 最低價: 970.00");
    println!("成交量: 32,450 張");
    println!("========================================");
}

fn handle_monitor(args: &MonitorArgs) {
    println!("========================================");
    println!("  啟動市場監控模式 (更新頻率: {}s)", args.interval);
    println!("========================================");
    println!("監控股號清單: {:?}", args.symbols);
    println!("(按 Ctrl+C 可停止監控)");
    println!("----------------------------------------");
    for (idx, symbol) in args.symbols.iter().enumerate() {
        println!("[#{}] 訂閱 {} 即時報價流 (Tick/OrderBook) 成功", idx + 1, symbol);
    }
}

fn handle_ping(server: &str) {
    println!("連線至報價伺服器: {} ...", server);
    println!("連線成功! 延遲: 12ms");
}
