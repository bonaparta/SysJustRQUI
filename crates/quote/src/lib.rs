//! 報價資料模型。純 Rust，不依賴 gpui 與 `sysrqui-ui`。

use std::fmt;

/// 單一商品的報價快照。
#[derive(Debug, Clone, PartialEq)]
pub struct Quote {
    /// 商品代號，例如 `2330`。
    pub symbol: String,
    /// 商品名稱，例如 `台積電`。
    pub name: String,
    /// 成交價。
    pub price: f64,
    /// 漲跌幅（百分比），例如 `1.55` 代表 +1.55%。
    pub change_pct: f64,
}

impl Quote {
    pub fn new(
        symbol: impl Into<String>,
        name: impl Into<String>,
        price: f64,
        change_pct: f64,
    ) -> Self {
        Self {
            symbol: symbol.into(),
            name: name.into(),
            price,
            change_pct,
        }
    }
}

/// 格式：`2330 台積電: 980.00 (+1.55%)`
impl fmt::Display for Quote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {}: {:.2} ({:+.2}%)",
            self.symbol, self.name, self.price, self.change_pct
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_rise() {
        let q = Quote::new("2330", "台積電", 980.0, 1.55);
        assert_eq!(q.to_string(), "2330 台積電: 980.00 (+1.55%)");
    }

    #[test]
    fn display_fall() {
        let q = Quote::new("2317", "鴻海", 172.5, -0.29);
        assert_eq!(q.to_string(), "2317 鴻海: 172.50 (-0.29%)");
    }
}
