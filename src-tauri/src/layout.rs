//! 子 webview 的版面計算。純函式,單位皆為邏輯像素。
//! 前端用 CSS flex 畫出同樣的格子(標題列、分隔線),兩邊依同一個公式切分才能對齊。

use serde::Deserialize;

/// 前端量測到的舞台位置與並排參數,數值都來自 CSS,Rust 端不另外寫死。
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
pub struct Geometry {
    /// 舞台距離視窗上、左、右、下緣的距離
    pub top: f64,
    pub left: f64,
    pub right: f64,
    pub bottom: f64,
    /// 並排時格與格之間的間隙(分隔線所在)
    pub gap: f64,
    /// 並排時每格上方服務名稱列的高度;只有一格時不顯示
    pub header: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// 把舞台依權重切成 n 格,回傳每格中官方頁面該放的位置(已扣掉名稱列)。
pub fn tiles(win_w: f64, win_h: f64, g: &Geometry, weights: &[f64], vertical: bool) -> Vec<Rect> {
    let n = weights.len();
    if n == 0 {
        return Vec::new();
    }
    let w = (win_w - g.left - g.right).max(0.0);
    let h = (win_h - g.top - g.bottom).max(0.0);
    let header = if n > 1 { g.header } else { 0.0 };
    let total: f64 = weights.iter().sum();
    let span = ((if vertical { h } else { w }) - g.gap * (n - 1) as f64).max(0.0);

    let mut offset = 0.0;
    weights
        .iter()
        .map(|weight| {
            let len = span * weight / total;
            let (x, y, tw, th) = if vertical {
                (g.left, g.top + offset, w, len)
            } else {
                (g.left + offset, g.top, len, h)
            };
            offset += len + g.gap;
            Rect {
                x,
                y: y + header,
                w: tw,
                h: (th - header).max(0.0),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: Geometry = Geometry {
        top: 36.0,
        left: 52.0,
        right: 6.0,
        bottom: 6.0,
        gap: 6.0,
        header: 26.0,
    };

    #[test]
    fn single_tile_fills_stage_without_header() {
        let r = tiles(500.0, 800.0, &G, &[1.0], false);
        assert_eq!(r, vec![Rect { x: 52.0, y: 36.0, w: 442.0, h: 758.0 }]);
    }

    #[test]
    fn weighted_columns() {
        // 舞台寬 442,扣掉一個間隙剩 436,依 1:3 切成 109 / 327
        let r = tiles(500.0, 800.0, &G, &[1.0, 3.0], false);
        assert_eq!(r[0], Rect { x: 52.0, y: 62.0, w: 109.0, h: 732.0 });
        assert_eq!(r[1], Rect { x: 167.0, y: 62.0, w: 327.0, h: 732.0 });
    }

    #[test]
    fn rows_put_header_on_each_tile() {
        // 舞台高 758,扣掉兩個間隙剩 746,三等分
        let r = tiles(500.0, 800.0, &G, &[1.0, 1.0, 1.0], true);
        let len = 746.0 / 3.0;
        assert_eq!(r[1].y, 36.0 + len + 6.0 + 26.0);
        assert_eq!(r[2].h, len - 26.0);
    }
}
