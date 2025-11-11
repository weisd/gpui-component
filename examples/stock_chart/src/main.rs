use chrono::Datelike;
use gpui::*;
use gpui_component::plot::label::{Text, TEXT_GAP, TEXT_SIZE};
use gpui_component::plot::{origin_point, scale::*, AxisText, Grid, Label, Plot};
use gpui_component::{button::*, *};
use serde::{Deserialize, Serialize};
use serde_json;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// 格式化成交量显示
fn format_volume(volume: f64) -> String {
    if volume >= 1_000_000_000.0 {
        format!("{:.2}亿", volume / 100_000_000.0)
    } else if volume >= 10_000.0 {
        format!("{:.2}万", volume / 10_000.0)
    } else {
        format!("{:.0}", volume)
    }
}

// 格式化成交额显示
fn format_amount(amount: f64) -> String {
    if amount >= 100_000_000.0 {
        format!("{:.2}亿", amount / 100_000_000.0)
    } else if amount >= 10_000.0 {
        format!("{:.2}万", amount / 10_000.0)
    } else {
        format!("{:.2}", amount)
    }
}

// 计算指数移动平均线（EMA）
fn calculate_ema<T, F>(data: &[T], period: usize, close_fn: F) -> Vec<Option<f64>>
where
    F: Fn(&T) -> f64,
{
    let mut ema_values = Vec::with_capacity(data.len());
    let multiplier = 2.0 / (period as f64 + 1.0);

    for i in 0..data.len() {
        if i == 0 {
            // 第一个值使用收盘价
            ema_values.push(Some(close_fn(&data[i])));
        } else if let Some(prev_ema) = ema_values[i - 1] {
            // EMA = (收盘价 - 前一日EMA) * 乘数 + 前一日EMA
            let close = close_fn(&data[i]);
            let ema = (close - prev_ema) * multiplier + prev_ema;
            ema_values.push(Some(ema));
        } else {
            ema_values.push(None);
        }
    }

    ema_values
}

// 计算MACD指标
struct MacdData {
    dif: Vec<Option<f64>>,  // DIF线：EMA12 - EMA26
    dea: Vec<Option<f64>>,  // DEA线：DIF的9日EMA
    macd: Vec<Option<f64>>, // MACD柱：2 * (DIF - DEA)
}

fn calculate_macd<T, F>(data: &[T], close_fn: F) -> MacdData
where
    F: Fn(&T) -> f64,
{
    // 计算EMA12和EMA26
    let ema12 = calculate_ema(data, 12, &close_fn);
    let ema26 = calculate_ema(data, 26, &close_fn);

    // 计算DIF = EMA12 - EMA26
    let dif: Vec<Option<f64>> = ema12
        .iter()
        .zip(ema26.iter())
        .map(|(e12, e26)| match (e12, e26) {
            (Some(e12_val), Some(e26_val)) => Some(e12_val - e26_val),
            _ => None,
        })
        .collect();

    // 计算DEA = DIF的9日EMA
    let dea = calculate_ema_from_values(&dif, 9);

    // 计算MACD = 2 * (DIF - DEA)
    let macd: Vec<Option<f64>> = dif
        .iter()
        .zip(dea.iter())
        .map(|(d, dea_val)| match (d, dea_val) {
            (Some(d_val), Some(dea_val)) => Some(2.0 * (d_val - dea_val)),
            _ => None,
        })
        .collect();

    MacdData { dif, dea, macd }
}

// 从Option<f64>值计算EMA（用于计算DEA）
fn calculate_ema_from_values(values: &[Option<f64>], period: usize) -> Vec<Option<f64>> {
    let mut ema_values = Vec::with_capacity(values.len());
    let multiplier = 2.0 / (period as f64 + 1.0);

    for i in 0..values.len() {
        if let Some(value) = values[i] {
            if i == 0 {
                ema_values.push(Some(value));
            } else if let Some(prev_ema) = ema_values[i - 1] {
                let ema = (value - prev_ema) * multiplier + prev_ema;
                ema_values.push(Some(ema));
            } else {
                ema_values.push(None);
            }
        } else {
            ema_values.push(None);
        }
    }

    ema_values
}

// 计算移动平均线（MA）
fn calculate_ma<T, F>(data: &[T], period: usize, close_fn: F) -> Vec<Option<f64>>
where
    F: Fn(&T) -> f64,
{
    let mut ma_values = Vec::with_capacity(data.len());

    for i in 0..data.len() {
        if i < period - 1 {
            // 数据不足，无法计算均线
            ma_values.push(None);
        } else {
            // 计算period天的收盘价平均值
            let sum: f64 = data[i.saturating_sub(period - 1)..=i]
                .iter()
                .map(|d| close_fn(d))
                .sum();
            let avg = sum / period as f64;
            ma_values.push(Some(avg));
        }
    }

    ma_values
}

#[derive(Clone, Serialize, Deserialize)]
struct StockData {
    date: String,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: u64,
}

// 蜡烛图组件
struct CandlestickChart<T, X>
where
    T: 'static,
    X: PartialEq + Into<SharedString> + 'static,
{
    data: Vec<T>,
    x: Option<Rc<dyn Fn(&T) -> X>>,
    open: Option<Rc<dyn Fn(&T) -> f64>>,
    high: Option<Rc<dyn Fn(&T) -> f64>>,
    low: Option<Rc<dyn Fn(&T) -> f64>>,
    close: Option<Rc<dyn Fn(&T) -> f64>>,
    tick_margin: usize,
    show_left_y_axis: bool, // 是否显示左侧Y轴标签区域
    // 十字光标状态（暂时不使用，但保留用于未来扩展）
    #[allow(dead_code)]
    mouse_x: Option<f32>,
    #[allow(dead_code)]
    mouse_y: Option<f32>,
    #[allow(dead_code)]
    selected_index: Option<usize>,
}

impl<T, X> CandlestickChart<T, X>
where
    T: 'static,
    X: PartialEq + Into<SharedString> + 'static,
{
    pub fn new<I>(data: I) -> Self
    where
        I: IntoIterator<Item = T>,
    {
        Self {
            data: data.into_iter().collect(),
            x: None,
            open: None,
            high: None,
            low: None,
            close: None,
            tick_margin: 1,
            show_left_y_axis: false, // 默认显示左侧Y轴标签区域
            mouse_x: None,
            mouse_y: None,
            selected_index: None,
        }
    }

    /// 设置是否显示左侧Y轴标签区域
    pub fn show_left_y_axis(mut self, show: bool) -> Self {
        self.show_left_y_axis = show;
        self
    }

    pub fn x(mut self, x: impl Fn(&T) -> X + 'static) -> Self {
        self.x = Some(Rc::new(x));
        self
    }

    pub fn open(mut self, open: impl Fn(&T) -> f64 + 'static) -> Self {
        self.open = Some(Rc::new(open));
        self
    }

    pub fn high(mut self, high: impl Fn(&T) -> f64 + 'static) -> Self {
        self.high = Some(Rc::new(high));
        self
    }

    pub fn low(mut self, low: impl Fn(&T) -> f64 + 'static) -> Self {
        self.low = Some(Rc::new(low));
        self
    }

    pub fn close(mut self, close: impl Fn(&T) -> f64 + 'static) -> Self {
        self.close = Some(Rc::new(close));
        self
    }

    pub fn tick_margin(mut self, tick_margin: usize) -> Self {
        self.tick_margin = tick_margin;
        self
    }
}

impl<T, X> Plot for CandlestickChart<T, X>
where
    T: 'static,
    X: PartialEq + Into<SharedString> + 'static,
{
    fn paint(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let (Some(x_fn), Some(open_fn), Some(high_fn), Some(low_fn), Some(close_fn)) = (
            self.x.as_ref(),
            self.open.as_ref(),
            self.high.as_ref(),
            self.low.as_ref(),
            self.close.as_ref(),
        ) else {
            return;
        };

        let total_width = bounds.size.width.as_f32();
        let total_height = bounds.size.height.as_f32();
        // 为Y轴标签单独分配空间，避免与K线重叠（左右两侧各分配空间）
        let y_label_width = 50.0; // Y轴标签区域宽度（左右两侧各50）
                                  // 根据是否显示左侧Y轴标签区域调整宽度计算
        let left_y_axis_width = if self.show_left_y_axis {
            y_label_width
        } else {
            0.0
        };
        let width = total_width - left_y_axis_width - y_label_width; // K线图实际宽度（中间区域，不包含左右Y轴标签区域）
                                                                     // 为最低价标签预留底部空间，避免标签被裁剪c
        let bottom_margin = TEXT_SIZE + TEXT_GAP * 4.0; // 底部边距（文本高度 + 间距）
        let height = total_height - bottom_margin; // K线图实际高度（减去底部边距）

        // X scale - 使用 ScaleBand 以便蜡烛之间有间距，从左侧Y轴标签区域后开始
        let x = ScaleBand::new(
            self.data.iter().map(|v| x_fn(v)).collect(),
            vec![left_y_axis_width, left_y_axis_width + width],
        )
        .padding_inner(0.3)
        .padding_outer(0.1);
        let band_width = x.band_width();
        let candle_width = (band_width * 0.7).max(4.0); // 蜡烛宽度为 band 的 70%，最小4像素
        let candle_x_offset = (band_width - candle_width) / 2.0;

        // 计算均线值（用于Y轴scale计算）
        let ma5 = calculate_ma(&self.data, 5, |d| close_fn(d));
        let ma10 = calculate_ma(&self.data, 10, |d| close_fn(d));
        let ma20 = calculate_ma(&self.data, 20, |d| close_fn(d));
        let ma30 = calculate_ma(&self.data, 30, |d| close_fn(d));
        let ma60 = calculate_ma(&self.data, 60, |d| close_fn(d));
        let ma120 = calculate_ma(&self.data, 120, |d| close_fn(d));
        let ma250 = calculate_ma(&self.data, 250, |d| close_fn(d));

        // Y scale - 包含所有价格（high, low, open, close）和均线值
        let mut all_prices: Vec<f64> = self
            .data
            .iter()
            .flat_map(|d| vec![high_fn(d), low_fn(d), open_fn(d), close_fn(d)])
            .collect();

        // 添加均线值到价格范围计算中
        for ma_values in [&ma5, &ma10, &ma20, &ma30, &ma60, &ma120, &ma250] {
            for ma_value in ma_values.iter().flatten() {
                all_prices.push(*ma_value);
            }
        }

        // 找到最小和最大价格
        let min_price = all_prices.iter().copied().fold(f64::INFINITY, f64::min);
        let max_price = all_prices.iter().copied().fold(f64::NEG_INFINITY, f64::max);

        // 添加一些边距（5%）
        let price_range = max_price - min_price;
        let margin = price_range * 0.05;
        let domain_min = (min_price - margin).max(0.0);
        let domain_max = max_price + margin;

        let y = ScaleLinear::new(vec![domain_min, domain_max], vec![height, 10.]);

        // 获取鼠标位置并计算对应的数据点
        let mouse_pos = window.mouse_position();
        let mut selected_index = None;
        let mut cursor_x = None;
        let mut cursor_y = None;

        if bounds.contains(&mouse_pos) {
            let local_x = (mouse_pos.x - bounds.origin.x).as_f32();
            let local_y = (mouse_pos.y - bounds.origin.y).as_f32();

            // 计算对应的数据点索引（需要考虑左侧Y轴标签的偏移）
            for (i, d) in self.data.iter().enumerate() {
                if let Some(x_tick) = x.tick(&x_fn(d)) {
                    let band_start = x_tick;
                    let band_end = x_tick + band_width;
                    if local_x >= band_start && local_x <= band_end {
                        selected_index = Some(i);
                        cursor_x = Some(px(x_tick + band_width / 2.0));
                        break;
                    }
                }
            }

            // 计算Y坐标（根据鼠标Y位置对应的价格）
            if local_y >= 0.0 && local_y <= height {
                cursor_y = Some(px(local_y));
            }
        }

        // 不再绘制X轴和X轴标签（主图不显示X轴标签）

        // 绘制右侧Y轴价格标签
        // 根据价格范围计算合适的整数间隔和标签数量（5-10个）
        let price_range = domain_max - domain_min;

        // 根据价格范围选择合适的间隔和标签数量
        let interval = if price_range > 100.0 {
            // 价格范围大（>100），使用较大间隔
            let candidate_intervals = vec![1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0];
            candidate_intervals
                .iter()
                .find(|&&interval| {
                    let count = ((domain_max - domain_min) / interval).ceil() as usize + 1;
                    count >= 5 && count <= 10
                })
                .copied()
                .unwrap_or(10.0)
        } else if price_range > 10.0 {
            // 价格范围中等（10-100），使用中等间隔
            let candidate_intervals = vec![0.5, 1.0, 2.0, 5.0, 10.0];
            candidate_intervals
                .iter()
                .find(|&&interval| {
                    let count = ((domain_max - domain_min) / interval).ceil() as usize + 1;
                    count >= 5 && count <= 10
                })
                .copied()
                .unwrap_or(1.0)
        } else {
            // 价格范围小（<10），使用小间隔
            let candidate_intervals = vec![0.1, 0.2, 0.5, 1.0];
            candidate_intervals
                .iter()
                .find(|&&interval| {
                    let count = ((domain_max - domain_min) / interval).ceil() as usize + 1;
                    count >= 5 && count <= 10
                })
                .copied()
                .unwrap_or(0.1)
        };

        // 计算起始价格（向下取整到最近的间隔）
        let start_price = (domain_min / interval).floor() * interval;
        // 计算结束价格（向上取整到最近的间隔）
        let end_price = (domain_max / interval).ceil() * interval;

        // 生成价格标签（使用整数间隔）
        // Y轴标签可以比最大值和最小值多一格
        let mut y_labels: Vec<AxisText> = Vec::new();
        let mut current_price = start_price;
        while current_price <= end_price {
            // 检查价格是否在实际价格范围内，或者比最小值低一格，或者比最大值高一格
            if (current_price >= min_price && current_price <= max_price)
                || (current_price < min_price && current_price >= min_price - interval)
                || (current_price > max_price && current_price <= max_price + interval)
            {
                if let Some(y_tick) = y.tick(&current_price) {
                    // 格式化价格：如果是整数则显示整数，否则显示2位小数
                    let price_text = if current_price.fract() == 0.0 {
                        format!("{:.0}", current_price)
                    } else {
                        format!("{:.2}", current_price)
                    };
                    y_labels.push(
                        AxisText::new(price_text, y_tick, cx.theme().muted_foreground)
                            .align(TextAlign::Right),
                    );
                }
            }
            current_price += interval;
        }

        // 如果标签数量不在5-10个范围内，重新计算
        if y_labels.len() < 5 || y_labels.len() > 10 {
            // 根据实际价格范围重新计算间隔
            let ideal_count = 7; // 目标标签数量
            let ideal_interval = price_range / (ideal_count - 1) as f64;

            // 找到最接近的"友好"间隔
            let friendly_intervals = if price_range > 100.0 {
                vec![1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0]
            } else if price_range > 10.0 {
                vec![0.5, 1.0, 2.0, 5.0, 10.0]
            } else {
                vec![0.1, 0.2, 0.5, 1.0]
            };

            let final_interval = friendly_intervals
                .iter()
                .min_by(|&&a, &&b| {
                    (a - ideal_interval)
                        .abs()
                        .partial_cmp(&(b - ideal_interval).abs())
                        .unwrap()
                })
                .copied()
                .unwrap_or(ideal_interval);

            let start_price = (domain_min / final_interval).floor() * final_interval;
            let end_price = (domain_max / final_interval).ceil() * final_interval;

            y_labels.clear();
            let mut current_price = start_price;
            while current_price <= end_price {
                // 检查价格是否在实际价格范围内，或者比最小值低一格，或者比最大值高一格
                if (current_price >= min_price && current_price <= max_price)
                    || (current_price < min_price && current_price >= min_price - final_interval)
                    || (current_price > max_price && current_price <= max_price + final_interval)
                {
                    if let Some(y_tick) = y.tick(&current_price) {
                        let price_text = if current_price.fract() == 0.0 {
                            format!("{:.0}", current_price)
                        } else {
                            format!("{:.2}", current_price)
                        };
                        y_labels.push(
                            AxisText::new(price_text, y_tick, cx.theme().muted_foreground)
                                .align(TextAlign::Right),
                        );
                    }
                }
                current_price += final_interval;
            }
        }

        // 收集网格线的Y坐标（在绘制Y轴标签之前）
        let grid_y_positions: Vec<f32> = y_labels.iter().map(|label| label.tick.as_f32()).collect();

        // 在左右两侧绘制Y轴标签
        // Y轴标签的Y坐标应该与K线图的价格值对齐
        // 注意：文本的origin是基线位置（底部），需要调整Y坐标使文本中心与价格线对齐
        let y_label_items: Vec<Text> = y_labels
            .iter()
            .flat_map(|t| {
                // Y轴标签的Y坐标直接使用y.tick()返回的值，确保与K线图的价格值对齐
                let y_tick_f32 = t.tick.as_f32();
                // 确保Y坐标在图表绘制区域内（不超出bounds）
                let clamped_y = y_tick_f32.max(10.0).min(height);
                // 文本的origin是基线位置（底部），需要调整Y坐标使文本中心与价格线对齐
                let text_baseline_y = clamped_y - TEXT_SIZE / 2.0;

                let mut labels = Vec::new();

                // 左侧标签（仅在show_left_y_axis为true时添加）
                if self.show_left_y_axis {
                    let left_label = Text {
                        text: t.text.clone(),
                        origin: point(px(TEXT_GAP), px(text_baseline_y)), // 左侧，左对齐
                        color: t.color,
                        font_size: t.font_size,
                        font_weight: gpui::FontWeight::NORMAL,
                        align: TextAlign::Left,
                    };
                    labels.push(left_label);
                }

                // 右侧标签（右对齐）
                let right_label = Text {
                    text: t.text.clone(),
                    origin: point(px(total_width - TEXT_GAP), px(text_baseline_y)), // 右侧，右对齐
                    color: t.color,
                    font_size: t.font_size,
                    font_weight: gpui::FontWeight::NORMAL,
                    align: TextAlign::Right,
                };
                labels.push(right_label);

                labels
            })
            .collect();
        let y_label = Label::new(y_label_items);
        y_label.paint(&bounds, window, cx);

        // 绘制Y轴标签区域与内容区域之间的分隔线（左右两侧）
        let border_color = cx.theme().border;

        // 左侧分隔线（仅在show_left_y_axis为true时绘制）
        if self.show_left_y_axis {
            let left_divider_x = left_y_axis_width;
            let mut left_divider_builder = PathBuilder::stroke(px(1.0));
            let left_divider_start = origin_point(px(left_divider_x), px(0.0), bounds.origin);
            let left_divider_end =
                origin_point(px(left_divider_x), px(total_height), bounds.origin);
            left_divider_builder.move_to(left_divider_start);
            left_divider_builder.line_to(left_divider_end);
            if let Ok(left_divider_path) = left_divider_builder.build() {
                window.paint_path(left_divider_path, Background::from(border_color));
            }
        }

        // 右侧分隔线（Y轴标签区域左边缘）
        let right_divider_x = left_y_axis_width + width;
        let mut right_divider_builder = PathBuilder::stroke(px(1.0));
        let right_divider_start = origin_point(px(right_divider_x), px(0.0), bounds.origin);
        let right_divider_end = origin_point(px(right_divider_x), px(total_height), bounds.origin);
        right_divider_builder.move_to(right_divider_start);
        right_divider_builder.line_to(right_divider_end);
        if let Ok(right_divider_path) = right_divider_builder.build() {
            window.paint_path(right_divider_path, Background::from(border_color));
        }

        // 绘制网格 - 网格线对应Y轴标签值，不超出Y轴标签区域
        // 创建只包含图表区域的bounds（不包含左右Y轴标签区域）
        let chart_bounds = gpui::Bounds {
            origin: origin_point(px(left_y_axis_width), px(0.0), bounds.origin),
            size: gpui::Size {
                width: px(width), // 只使用图表宽度，不包含左右Y轴标签区域
                height: bounds.size.height,
            },
        };
        Grid::new()
            .y(grid_y_positions)
            .stroke(cx.theme().border)
            .dash_array(&[px(4.), px(2.)])
            .paint(&chart_bounds, window);

        // 绘制均线（在蜡烛之前绘制，这样蜡烛会覆盖均线）
        // 均线值已在Y轴scale计算时计算过了
        let origin = bounds.origin;
        // 使用指定的颜色
        use gpui_component::Colorize;
        let ma_colors = [
            Hsla::parse_hex("#575756").unwrap(), // MA5 （深灰色）
            Hsla::parse_hex("#e82bf6").unwrap(), // MA10 （紫色）
            Hsla::parse_hex("#0000FF").unwrap(), // MA20 （蓝色）
            Hsla::parse_hex("#aaa9a9").unwrap(), // MA30 （浅灰色）
            Hsla::parse_hex("#640000").unwrap(), // MA60 （深绿色）
            Hsla::parse_hex("#206f09").unwrap(), // MA120 （深绿色）
            Hsla::parse_hex("#f4b350").unwrap(), // MA250 (黄色)
        ];
        let ma_data = [&ma5, &ma10, &ma20, &ma30, &ma60, &ma120, &ma250];

        for (ma_values, &color) in ma_data.iter().zip(ma_colors.iter()) {
            let mut line_builder = PathBuilder::stroke(px(1.0));
            let mut has_points = false;

            for (i, d) in self.data.iter().enumerate() {
                if let Some(ma_value) = ma_values.get(i).and_then(|v| *v) {
                    if let Some(x_tick) = x.tick(&x_fn(d)) {
                        // 确保x_tick在图表区域内（不小于left_y_axis_width）
                        if x_tick >= left_y_axis_width {
                            if let Some(ma_y_f32) = y.tick(&ma_value) {
                                let ma_y = px(ma_y_f32);
                                let point =
                                    origin_point(px(x_tick + band_width / 2.0), ma_y, origin);

                                if !has_points {
                                    line_builder.move_to(point);
                                    has_points = true;
                                } else {
                                    line_builder.line_to(point);
                                }
                            }
                        }
                    }
                }
            }

            if has_points {
                if let Ok(line_path) = line_builder.build() {
                    window.paint_path(line_path, Background::from(color));
                }
            }
        }

        // 绘制蜡烛
        let success_color = cx.theme().success;
        let danger_color = cx.theme().danger;

        for d in &self.data {
            let x_tick = x.tick(&x_fn(d));
            if let Some(x_tick) = x_tick {
                // 确保x_tick在图表区域内（不小于left_y_axis_width）
                if x_tick < left_y_axis_width {
                    continue;
                }

                let open = open_fn(d);
                let high = high_fn(d);
                let low = low_fn(d);
                let close = close_fn(d);

                let open_y = y.tick(&open);
                let high_y = y.tick(&high);
                let low_y = y.tick(&low);
                let close_y = y.tick(&close);

                if let (Some(open_y), Some(high_y), Some(low_y), Some(close_y)) =
                    (open_y, high_y, low_y, close_y)
                {
                    let candle_x = x_tick + candle_x_offset;
                    let is_up = close > open;

                    // 计算实体的Y坐标
                    // 注意：在屏幕坐标系中，较小的Y值在上方（对应较高价格）
                    let body_top_y = close_y.min(open_y); // 较小的Y值（较高价格）
                    let body_bottom_y = close_y.max(open_y); // 较大的Y值（较低价格）

                    // 确保实体有最小高度（至少3像素）
                    let min_body_height = 3.0;
                    let body_height = body_bottom_y - body_top_y;
                    let final_body_top_y = if body_height < min_body_height {
                        body_top_y - (min_body_height - body_height) / 2.0
                    } else {
                        body_top_y
                    };
                    let final_body_bottom_y = if body_height < min_body_height {
                        body_bottom_y + (min_body_height - body_height) / 2.0
                    } else {
                        body_bottom_y
                    };

                    // 绘制实体（矩形）
                    let body_p1 = origin_point(px(candle_x), px(final_body_top_y), origin);
                    let body_p2 =
                        origin_point(px(candle_x + candle_width), px(final_body_bottom_y), origin);
                    let body_bounds = Bounds::from_corners(body_p1, body_p2);

                    if is_up {
                        // 红色空心蜡烛（上涨）：白色填充 + 红色边框
                        window.paint_quad(fill(body_bounds, cx.theme().background));
                        // 绘制红色边框
                        let mut border_builder = PathBuilder::stroke(px(1.5));
                        let border_top_left =
                            origin_point(px(candle_x), px(final_body_top_y), origin);
                        let border_top_right =
                            origin_point(px(candle_x + candle_width), px(final_body_top_y), origin);
                        let border_bottom_right = origin_point(
                            px(candle_x + candle_width),
                            px(final_body_bottom_y),
                            origin,
                        );
                        let border_bottom_left =
                            origin_point(px(candle_x), px(final_body_bottom_y), origin);

                        border_builder.move_to(border_top_left);
                        border_builder.line_to(border_top_right);
                        border_builder.line_to(border_bottom_right);
                        border_builder.line_to(border_bottom_left);
                        border_builder.line_to(border_top_left);

                        if let Ok(border_path) = border_builder.build() {
                            window.paint_path(border_path, Background::from(danger_color));
                        }
                    } else {
                        // 绿色实心蜡烛（下跌）：实心绿色矩形
                        window.paint_quad(fill(body_bounds, success_color));
                    }

                    // 绘制影线：从实体顶部延伸到最高价，从实体底部延伸到最低价
                    let wick_x = x_tick + band_width / 2.0;
                    let wick_color = if is_up { danger_color } else { success_color };

                    // 上影线：从实体顶部到最高价
                    let upper_wick_top_y = high_y.min(low_y); // 最高价的Y坐标（较小值）
                    if upper_wick_top_y < final_body_top_y {
                        let mut upper_wick_builder = PathBuilder::stroke(px(1.5));
                        let upper_wick_start =
                            origin_point(px(wick_x), px(final_body_top_y), origin);
                        let upper_wick_end = origin_point(px(wick_x), px(upper_wick_top_y), origin);
                        upper_wick_builder.move_to(upper_wick_start);
                        upper_wick_builder.line_to(upper_wick_end);
                        if let Ok(upper_wick_path) = upper_wick_builder.build() {
                            window.paint_path(upper_wick_path, Background::from(wick_color));
                        }
                    }

                    // 下影线：从实体底部到最低价
                    let lower_wick_bottom_y = high_y.max(low_y); // 最低价的Y坐标（较大值）
                    if lower_wick_bottom_y > final_body_bottom_y {
                        let mut lower_wick_builder = PathBuilder::stroke(px(1.5));
                        let lower_wick_start =
                            origin_point(px(wick_x), px(final_body_bottom_y), origin);
                        let lower_wick_end =
                            origin_point(px(wick_x), px(lower_wick_bottom_y), origin);
                        lower_wick_builder.move_to(lower_wick_start);
                        lower_wick_builder.line_to(lower_wick_end);
                        if let Ok(lower_wick_path) = lower_wick_builder.build() {
                            window.paint_path(lower_wick_path, Background::from(wick_color));
                        }
                    }
                }
            }
        }

        // 找到最高点和最低点
        let mut max_price_value = f64::NEG_INFINITY;
        let mut min_price_value = f64::INFINITY;
        let mut max_price_index = 0;
        let mut min_price_index = 0;

        for (i, d) in self.data.iter().enumerate() {
            let high = high_fn(d);
            let low = low_fn(d);
            if high > max_price_value {
                max_price_value = high;
                max_price_index = i;
            }
            if low < min_price_value {
                min_price_value = low;
                min_price_index = i;
            }
        }

        // 绘制最高点和最低点标记
        if let Some(max_data) = self.data.get(max_price_index) {
            // 确保使用正确的数据来获取x_tick
            let x_value = x_fn(max_data);
            if let Some(x_tick) = x.tick(&x_value) {
                // 确保x_tick在图表区域内（不小于left_y_axis_width）
                if x_tick >= left_y_axis_width {
                    if let Some(max_y) = y.tick(&max_price_value) {
                        // marker_x 是K线的中心位置
                        let marker_x = x_tick + band_width / 2.0;
                        let arrow_size = 8.0;
                        let arrow_offset = 15.0; // 箭头与价格点的距离

                        // 绘制向下指向的箭头（在最高点上方）
                        let arrow_top_y = max_y - arrow_offset; // 箭头顶部在最高点上方
                        let arrow_tip = origin_point(px(marker_x), px(max_y), origin); // 箭头尖端指向最高价
                        let arrow_left =
                            origin_point(px(marker_x - arrow_size / 2.0), px(arrow_top_y), origin);
                        let arrow_right =
                            origin_point(px(marker_x + arrow_size / 2.0), px(arrow_top_y), origin);

                        // 绘制箭头三角形（填充）
                        let mut arrow_builder = PathBuilder::fill();
                        arrow_builder.move_to(arrow_tip);
                        arrow_builder.line_to(arrow_left);
                        arrow_builder.line_to(arrow_right);
                        arrow_builder.line_to(arrow_tip);
                        if let Ok(arrow_path) = arrow_builder.build() {
                            window.paint_path(arrow_path, Background::from(danger_color));
                        }

                        // 绘制价格标签（在箭头上方）
                        let label_text = format!("最高: {:.2}", max_price_value);
                        let label_center_x = marker_x; // 使用K线中心
                        let label_center_y = arrow_top_y - 15.0; // 标签在箭头上方，合适的距离
                                                                 // 文本的origin.y是基线位置，为了垂直居中，需要稍微向下调整
                        let text_baseline_y = label_center_y + 3.0;
                        let price_label = Label::new(vec![Text {
                            text: label_text.into(),
                            origin: point(px(label_center_x), px(text_baseline_y)),
                            color: danger_color,
                            font_size: px(10.0),
                            font_weight: gpui::FontWeight::SEMIBOLD,
                            align: TextAlign::Center,
                        }]);
                        price_label.paint(&bounds, window, cx);
                    }
                }
            }
        }

        if let Some(min_data) = self.data.get(min_price_index) {
            // 确保使用正确的数据来获取x_tick
            let x_value = x_fn(min_data);
            if let Some(x_tick) = x.tick(&x_value) {
                // 确保x_tick在图表区域内（不小于left_y_axis_width）
                if x_tick >= left_y_axis_width {
                    if let Some(min_y) = y.tick(&min_price_value) {
                        // marker_x 是K线的中心位置
                        let marker_x = x_tick + band_width / 2.0;
                        let arrow_size = 8.0;

                        // 绘制向上指向的箭头（在最低点下方）
                        // 参考最高价的样式：箭头与价格点的距离为15像素
                        let arrow_offset = 15.0; // 箭头底部与最低价的距离（与最高价对称）
                        let arrow_bottom_y = min_y + arrow_offset; // 箭头底部位置（最低价下方15像素）
                        let arrow_tip = origin_point(px(marker_x), px(min_y), origin); // 箭头尖端指向最低价
                        let arrow_left = origin_point(
                            px(marker_x - arrow_size / 2.0),
                            px(arrow_bottom_y),
                            origin,
                        );
                        let arrow_right = origin_point(
                            px(marker_x + arrow_size / 2.0),
                            px(arrow_bottom_y),
                            origin,
                        );

                        // 绘制箭头三角形（填充）
                        let mut arrow_builder = PathBuilder::fill();
                        arrow_builder.move_to(arrow_tip);
                        arrow_builder.line_to(arrow_left);
                        arrow_builder.line_to(arrow_right);
                        arrow_builder.line_to(arrow_tip);
                        if let Ok(arrow_path) = arrow_builder.build() {
                            window.paint_path(arrow_path, Background::from(success_color));
                        }

                        // 绘制价格标签（在箭头下方）
                        // 确保标签在箭头下方，有足够的距离
                        let label_text = format!("最低: {:.2}", min_price_value);
                        let label_center_x = marker_x; // 使用K线中心
                        let text_baseline_y = arrow_bottom_y;
                        let price_label = Label::new(vec![Text {
                            text: label_text.into(),
                            origin: point(px(label_center_x), px(text_baseline_y)),
                            color: success_color,
                            font_size: px(10.0),
                            font_weight: gpui::FontWeight::SEMIBOLD,
                            align: TextAlign::Center,
                        }]);
                        price_label.paint(&bounds, window, cx);
                    }
                }
            }
        }

        // 绘制最新价格标记（最后一根K线的收盘价）- 显示在右侧Y轴标签区域内
        if let Some(latest_data) = self.data.last() {
            let latest_price = close_fn(latest_data);
            if let Some(latest_y) = y.tick(&latest_price) {
                let arrow_size = 8.0;
                let arrow_offset = 10.0; // 箭头与图表右边缘的距离

                // 绘制向左指向的箭头（在右侧Y轴标签区域内，指向最新价）
                let chart_right_edge = left_y_axis_width + width; // 图表右边缘位置
                let arrow_left_x = chart_right_edge + arrow_offset; // 箭头左侧位置（在右侧Y轴标签区域内）
                let arrow_tip = origin_point(px(chart_right_edge), px(latest_y), origin); // 箭头尖端指向图表右边缘（最新价位置）
                let arrow_top =
                    origin_point(px(arrow_left_x), px(latest_y - arrow_size / 2.0), origin);
                let arrow_bottom =
                    origin_point(px(arrow_left_x), px(latest_y + arrow_size / 2.0), origin);

                // 绘制箭头三角形（填充）
                // 使用主题颜色，根据涨跌决定颜色
                let arrow_color = if latest_price > open_fn(latest_data) {
                    danger_color // 上涨用红色
                } else {
                    success_color // 下跌用绿色
                };
                let mut arrow_builder = PathBuilder::fill();
                arrow_builder.move_to(arrow_tip);
                arrow_builder.line_to(arrow_top);
                arrow_builder.line_to(arrow_bottom);
                arrow_builder.line_to(arrow_tip);
                if let Ok(arrow_path) = arrow_builder.build() {
                    window.paint_path(arrow_path, Background::from(arrow_color));
                }

                // 绘制价格标签（在箭头左侧）
                let label_text = format!("{:.2}", latest_price);
                let label_center_x = arrow_left_x + 5.0; // 标签在箭头左侧
                let text_baseline_y = latest_y - TEXT_SIZE / 2.0; // 文本中心与价格对齐
                let price_label = Label::new(vec![Text {
                    text: label_text.into(),
                    origin: point(px(label_center_x), px(text_baseline_y)),
                    color: arrow_color,
                    font_size: px(10.0),
                    font_weight: gpui::FontWeight::SEMIBOLD,
                    align: TextAlign::Left,
                }]);
                price_label.paint(&bounds, window, cx);
            }
        }

        // 绘制十字光标
        if let (Some(cx_pos), Some(cy_pos)) = (cursor_x, cursor_y) {
            let cursor_color = cx.theme().foreground.opacity(0.6);

            // 绘制垂直线（从顶部到底部）- 使用虚线
            let mut vline_builder = PathBuilder::stroke(px(1.5)).dash_array(&[px(4.0), px(2.0)]); // 虚线样式：4像素实线，2像素空白
            let vline_start = origin_point(cx_pos, px(0.0), origin);
            let vline_end = origin_point(cx_pos, px(height), origin);
            vline_builder.move_to(vline_start);
            vline_builder.line_to(vline_end);
            if let Ok(vline_path) = vline_builder.build() {
                window.paint_path(vline_path, Background::from(cursor_color));
            }

            // 绘制水平线（从左到右）- 使用虚线
            let mut hline_builder = PathBuilder::stroke(px(1.5)).dash_array(&[px(4.0), px(2.0)]); // 虚线样式：4像素实线，2像素空白
            let chart_left_edge = left_y_axis_width; // 图表左边缘位置
            let chart_right_edge = left_y_axis_width + width; // 图表右边缘位置
            let hline_start = origin_point(px(chart_left_edge), cy_pos, origin);
            let hline_end = origin_point(px(chart_right_edge), cy_pos, origin);
            hline_builder.move_to(hline_start);
            hline_builder.line_to(hline_end);
            if let Ok(hline_path) = hline_builder.build() {
                window.paint_path(hline_path, Background::from(cursor_color));
            }

            // 在左右两侧显示当前价格
            let cy_pos_f32 = cy_pos.as_f32();
            let height_f32 = height;
            let ratio = 1.0 - cy_pos_f32 / height_f32;
            let current_price = domain_min + (domain_max - domain_min) * ratio as f64;
            let price_text = format!("{:.2}", current_price);

            // 左侧标签（仅在show_left_y_axis为true时显示）
            if self.show_left_y_axis {
                let left_price_label = Label::new(vec![Text {
                    text: price_text.clone().into(),
                    origin: point(px(TEXT_GAP), cy_pos), // 左侧，左对齐
                    color: cursor_color,
                    font_size: px(12.0),
                    font_weight: gpui::FontWeight::SEMIBOLD,
                    align: TextAlign::Left,
                }]);
                left_price_label.paint(&bounds, window, cx);
            }

            // 右侧标签（右对齐）
            let right_price_label = Label::new(vec![Text {
                text: price_text.into(),
                origin: point(px(total_width - TEXT_GAP), cy_pos), // 右侧，右对齐
                color: cursor_color,
                font_size: px(12.0),
                font_weight: gpui::FontWeight::SEMIBOLD,
                align: TextAlign::Right,
            }]);
            right_price_label.paint(&bounds, window, cx);

            // 显示当前K线的详细信息
            if let Some(idx) = selected_index {
                if let Some(d) = self.data.get(idx) {
                    // 将 T 转换为 StockData 以访问 volume 字段
                    let stock_data = unsafe { &*(d as *const T as *const StockData) };
                    let open = open_fn(d);
                    let high = high_fn(d);
                    let low = low_fn(d);
                    let close = close_fn(d);

                    // 在光标交叉点附近显示信息
                    let info_x = cx_pos.as_f32().min(width - 150.0).max(10.0);
                    let info_y = cy_pos.as_f32().min(height - 100.0).max(10.0);
                    let info_pos = origin_point(px(info_x), px(info_y), origin);

                    // 创建信息文本背景（增加高度以容纳更多信息）
                    let info_bg = cx.theme().background.opacity(0.9);
                    let info_bounds = Bounds::new(info_pos, gpui::Size::new(px(140.0), px(120.0))); // 增加高度到120
                    window.paint_quad(fill(info_bounds, info_bg));

                    // 绘制边框
                    let mut border_builder = PathBuilder::stroke(px(1.0));
                    let border_color = cx.theme().border;
                    let border_tl = info_bounds.origin;
                    let border_tr = point(info_bounds.right(), info_bounds.top());
                    let border_br = point(info_bounds.right(), info_bounds.bottom());
                    let border_bl = point(info_bounds.left(), info_bounds.bottom());
                    border_builder.move_to(border_tl);
                    border_builder.line_to(border_tr);
                    border_builder.line_to(border_br);
                    border_builder.line_to(border_bl);
                    border_builder.line_to(border_tl);
                    if let Ok(border_path) = border_builder.build() {
                        window.paint_path(border_path, Background::from(border_color));
                    }

                    // 使用 Label 绘制文本信息
                    let text_color = cx.theme().foreground;
                    let font_size = px(10.0);
                    let line_height = 14.0;
                    let mut y_offset = info_y + 5.0;

                    let mut label_texts = Vec::new();

                    // 日期
                    let date_str: SharedString = x_fn(d).into();
                    label_texts.push(
                        Text::new(
                            format!("日期: {}", date_str),
                            point(px(info_x + 5.0), px(y_offset)),
                            text_color,
                        )
                        .font_size(font_size),
                    );
                    y_offset += line_height;

                    // 开盘
                    label_texts.push(
                        Text::new(
                            format!("开盘: {:.2}", open),
                            point(px(info_x + 5.0), px(y_offset)),
                            text_color,
                        )
                        .font_size(font_size),
                    );
                    y_offset += line_height;

                    // 最高
                    label_texts.push(
                        Text::new(
                            format!("最高: {:.2}", high),
                            point(px(info_x + 5.0), px(y_offset)),
                            text_color,
                        )
                        .font_size(font_size),
                    );
                    y_offset += line_height;

                    // 最低
                    label_texts.push(
                        Text::new(
                            format!("最低: {:.2}", low),
                            point(px(info_x + 5.0), px(y_offset)),
                            text_color,
                        )
                        .font_size(font_size),
                    );
                    y_offset += line_height;

                    // 收盘
                    label_texts.push(
                        Text::new(
                            format!("收盘: {:.2}", close),
                            point(px(info_x + 5.0), px(y_offset)),
                            text_color,
                        )
                        .font_size(font_size),
                    );
                    y_offset += line_height;

                    // 成交量
                    let volume = stock_data.volume;
                    let volume_f64 = volume as f64;
                    let volume_str = format_volume(volume_f64);
                    label_texts.push(
                        Text::new(
                            format!("成交量: {}", volume_str),
                            point(px(info_x + 5.0), px(y_offset)),
                            text_color,
                        )
                        .font_size(font_size),
                    );
                    y_offset += line_height;

                    // 成交额（成交额 = 成交量 * 平均价，平均价使用(开盘+收盘)/2）
                    let avg_price = (open + close) / 2.0;
                    let amount = volume_f64 * avg_price;
                    let amount_str = format_amount(amount);
                    label_texts.push(
                        Text::new(
                            format!("成交额: {}", amount_str),
                            point(px(info_x + 5.0), px(y_offset)),
                            text_color,
                        )
                        .font_size(font_size),
                    );

                    // 绘制标签
                    let label = Label::new(label_texts);
                    label.paint(&bounds, window, cx);
                }
            }
        }
    }
}

// 手动实现 IntoElement 和 Element trait
impl<T, X> IntoElement for CandlestickChart<T, X>
where
    T: 'static,
    X: PartialEq + Into<SharedString> + 'static,
{
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<T, X> Element for CandlestickChart<T, X>
where
    T: 'static,
    X: PartialEq + Into<SharedString> + 'static,
{
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let style = Style {
            size: gpui::Size::full(),
            ..Default::default()
        };

        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Window,
        _: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        // 在 paint 方法中监听鼠标移动事件，当鼠标在图表区域内移动时触发重绘
        let chart_bounds = bounds;
        window.on_mouse_event({
            let bounds = chart_bounds;
            move |event: &gpui::MouseMoveEvent, _, window, _cx| {
                // 只在鼠标在图表区域内时触发刷新
                if bounds.contains(&event.position) {
                    window.refresh();
                }
            }
        });

        // 绘制图表
        <Self as Plot>::paint(self, bounds, window, cx)
    }
}

// 成交量图组件（带十字光标联动）
struct VolumeChart {
    data: Vec<StockData>,
    show_left_y_axis: bool, // 是否显示左侧Y轴标签区域
}

impl VolumeChart {
    fn new(_data: Vec<StockData>, _success_color: Hsla, _danger_color: Hsla) -> Self {
        Self {
            data: _data,
            show_left_y_axis: false, // 默认与主图保持一致
        }
    }

    /// 设置是否显示左侧Y轴标签区域
    pub fn show_left_y_axis(mut self, show: bool) -> Self {
        self.show_left_y_axis = show;
        self
    }
}

impl IntoElement for VolumeChart {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for VolumeChart {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let style = Style {
            size: gpui::Size::full(),
            ..Default::default()
        };
        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Window,
        _: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        // 与主图K线图保持一致：减去左右Y轴标签宽度，确保X轴对齐
        let total_width = bounds.size.width.as_f32();
        let y_label_width = 50.0; // Y轴标签区域宽度（左右两侧各50）
                                  // 根据是否显示左侧Y轴标签区域调整宽度计算
        let left_y_axis_width = if self.show_left_y_axis {
            y_label_width
        } else {
            0.0
        };
        let chart_width = total_width - left_y_axis_width - y_label_width; // 图表实际宽度（中间区域，不包含左右Y轴标签区域）

        let height = bounds.size.height.as_f32();
        let chart_height = height; // 使用全部高度，不预留X轴标签空间

        // X scale - 与主图一致，从左侧Y轴标签区域后开始
        let x_fn = |d: &StockData| d.date.clone();
        let x = ScaleBand::new(
            self.data.iter().map(|v| x_fn(v)).collect(),
            vec![left_y_axis_width, left_y_axis_width + chart_width],
        )
        .padding_inner(0.3)
        .padding_outer(0.1);
        let band_width = x.band_width();

        // Y scale
        let min_volume = self
            .data
            .iter()
            .map(|d| d.volume as f64)
            .fold(f64::INFINITY, f64::min);
        let max_volume = self
            .data
            .iter()
            .map(|d| d.volume as f64)
            .fold(f64::NEG_INFINITY, f64::max);
        let volume_range = max_volume - min_volume;
        let margin = volume_range * 0.05;
        // 确保domain_min从0开始
        let domain_min = 0.0;
        let domain_max = max_volume + margin;

        let y = ScaleLinear::new(vec![domain_min, domain_max], vec![chart_height, 10.]);

        // 绘制Y轴标签（成交量）- 只显示4个值，与网格线对齐，从0开始，尽量使用整数
        use gpui::point;
        use gpui::TextAlign;
        use gpui_component::plot::label::{Text, TEXT_GAP, TEXT_SIZE};
        use gpui_component::plot::{AxisText, Label};

        // 辅助函数：将值取整到合适的整数
        fn round_to_nice_integer(value: f64, max_value: f64) -> f64 {
            if value <= 0.0 {
                return 0.0;
            }
            if value >= max_value {
                return max_value;
            }
            // 根据最大值确定取整单位
            if max_value <= 0.0 {
                return value;
            }
            let magnitude = max_value.log10().floor();
            let base_unit = if magnitude <= 0.0 {
                1.0 // 如果最大值小于10，使用1作为取整单位
            } else {
                10_f64.powi(magnitude as i32 - 1) // 例如：1000000 -> 100000, 100000 -> 10000
            };
            // 取整到base_unit的倍数，但不超过max_value
            let rounded = (value / base_unit).ceil() * base_unit;
            rounded.min(max_value)
        }

        // 计算4个均匀分布的标签值，第一个值从0开始，其他值尽量使用整数
        let label_count = 4;
        let mut volume_labels: Vec<(AxisText, f32)> = Vec::new(); // 存储标签和对应的Y坐标

        // 先计算均匀分布的volume值（0, max/3, 2*max/3, max），然后取整
        for i in 0..label_count {
            let volume_value = if i == 0 {
                // 第一个标签（底部）始终为0
                0.0
            } else if i == label_count - 1 {
                // 最后一个标签显示最大值（取整后的最大值）
                round_to_nice_integer(domain_max, domain_max)
            } else {
                // 计算均匀分布的volume值
                let ratio = i as f64 / (label_count - 1) as f64;
                let raw_value = domain_min + (domain_max - domain_min) * ratio;
                // 将值取整到合适的整数
                round_to_nice_integer(raw_value, domain_max)
            };

            // 根据取整后的值计算对应的Y坐标
            let actual_grid_y = if volume_value <= domain_min {
                chart_height
            } else if volume_value >= domain_max {
                10.0
            } else {
                // 反向映射：value -> Y坐标
                // ScaleLinear映射：domain_min (0.0) -> chart_height（底部）, domain_max -> 10.0（顶部）
                // 所以：ratio = (volume_value - domain_min) / (domain_max - domain_min)
                // Y = chart_height - (chart_height - 10.0) * ratio
                let ratio = (volume_value - domain_min) / (domain_max - domain_min);
                chart_height - (chart_height - 10.0) * ratio as f32
            };

            // 优化成交量单位显示：统一使用合适的单位
            let volume_text = if volume_value >= 1_000_000_000.0 {
                format!("{:.0}亿", volume_value / 100_000_000.0)
            } else if volume_value >= 10_000.0 {
                format!("{:.0}万", volume_value / 10_000.0)
            } else if volume_value >= 1_000.0 {
                format!("{:.0}千", volume_value / 1_000.0)
            } else {
                format!("{:.0}", volume_value)
            };
            volume_labels.push((
                AxisText::new(volume_text, actual_grid_y, cx.theme().muted_foreground)
                    .align(TextAlign::Right),
                actual_grid_y,
            ));
        }

        // 收集网格线的Y坐标（使用取整后值对应的实际Y坐标）
        // 按Y坐标从大到小排序（从底部到顶部），确保标签值从下往上递增
        volume_labels.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let grid_y_positions: Vec<f32> = volume_labels.iter().map(|(_, grid_y)| *grid_y).collect();

        // 绘制成交量Y轴标签（左右两侧）
        let volume_label_items: Vec<Text> = volume_labels
            .iter()
            .flat_map(|(t, actual_grid_y)| {
                // Y轴标签的Y坐标使用取整后值对应的实际Y坐标，确保与网格线对齐
                let y_tick_f32 = *actual_grid_y;
                // 确保Y坐标在图表绘制区域内（不超出bounds）
                let clamped_y = y_tick_f32.max(10.0).min(chart_height);
                // 文本的origin是基线位置（底部），需要调整Y坐标使文本中心与网格线对齐
                let text_baseline_y = clamped_y - TEXT_SIZE / 2.0;

                let mut labels = Vec::new();

                // 左侧标签（仅在show_left_y_axis为true时添加）
                if self.show_left_y_axis {
                    let left_label = Text {
                        text: t.text.clone(),
                        origin: point(px(TEXT_GAP), px(text_baseline_y)), // 左侧，左对齐
                        color: t.color,
                        font_size: t.font_size,
                        font_weight: gpui::FontWeight::NORMAL,
                        align: TextAlign::Left,
                    };
                    labels.push(left_label);
                }

                // 右侧标签（右对齐）
                let right_label = Text {
                    text: t.text.clone(),
                    origin: point(px(total_width - TEXT_GAP), px(text_baseline_y)), // 右侧，右对齐
                    color: t.color,
                    font_size: t.font_size,
                    font_weight: gpui::FontWeight::NORMAL,
                    align: TextAlign::Right,
                };
                labels.push(right_label);

                labels
            })
            .collect();
        let volume_label = Label::new(volume_label_items);
        volume_label.paint(&bounds, window, cx);

        // 绘制Y轴标签区域与内容区域之间的分隔线（左右两侧）
        let origin = bounds.origin;
        let border_color = cx.theme().border;

        // 左侧分隔线（仅在show_left_y_axis为true时绘制）
        if self.show_left_y_axis {
            let left_divider_x = left_y_axis_width;
            let mut left_divider_builder = PathBuilder::stroke(px(1.0));
            let left_divider_start = origin_point(px(left_divider_x), px(0.0), origin);
            let left_divider_end = origin_point(px(left_divider_x), px(height), origin);
            left_divider_builder.move_to(left_divider_start);
            left_divider_builder.line_to(left_divider_end);
            if let Ok(left_divider_path) = left_divider_builder.build() {
                window.paint_path(left_divider_path, Background::from(border_color));
            }
        }

        // 右侧分隔线（Y轴标签区域左边缘）
        let right_divider_x = left_y_axis_width + chart_width;
        let mut right_divider_builder = PathBuilder::stroke(px(1.0));
        let right_divider_start = origin_point(px(right_divider_x), px(0.0), origin);
        let right_divider_end = origin_point(px(right_divider_x), px(height), origin);
        right_divider_builder.move_to(right_divider_start);
        right_divider_builder.line_to(right_divider_end);
        if let Ok(right_divider_path) = right_divider_builder.build() {
            window.paint_path(right_divider_path, Background::from(border_color));
        }

        // 绘制网格（不绘制X轴）- 网格线对应Y轴标签值，不超出Y轴标签区域
        // 创建只包含图表区域的bounds（不包含左右Y轴标签区域）
        let chart_bounds = gpui::Bounds {
            origin: origin_point(px(left_y_axis_width), px(0.0), bounds.origin),
            size: gpui::Size {
                width: px(chart_width), // 只使用图表宽度，不包含左右Y轴标签区域
                height: bounds.size.height,
            },
        };
        Grid::new()
            .y(grid_y_positions)
            .stroke(cx.theme().border)
            .dash_array(&[px(4.), px(2.)])
            .paint(&chart_bounds, window);

        // 手动绘制柱状图（不绘制X轴标签），确保不超出Y轴标签区域
        let success_color = cx.theme().success.opacity(0.7);
        let danger_color = cx.theme().danger.opacity(0.7);
        let origin = bounds.origin;

        for d in &self.data {
            if let Some(x_tick) = x.tick(&x_fn(d)) {
                // 确保x_tick在图表区域内（不小于left_y_axis_width）
                if x_tick >= left_y_axis_width {
                    if let Some(volume_y) = y.tick(&(d.volume as f64)) {
                        let bar_x = x_tick + band_width * 0.1;
                        let bar_width = band_width * 0.8;
                        let bar_top = volume_y.min(chart_height);
                        let bar_bottom = chart_height;
                        let bar_height = bar_bottom - bar_top;

                        let color = if d.close > d.open {
                            danger_color
                        } else {
                            success_color
                        };

                        let bar_bounds = gpui::Bounds {
                            origin: origin_point(px(bar_x), px(bar_top), origin),
                            size: gpui::Size {
                                width: px(bar_width),
                                height: px(bar_height),
                            },
                        };
                        window.paint_quad(gpui::quad(
                            bar_bounds,
                            0.0,
                            color,
                            px(0.0),
                            color,
                            gpui::BorderStyle::default(),
                        ));
                    }
                }
            }
        }

        // 扩展检测范围：检测鼠标是否在K线图或成交量图的X坐标范围内
        // 不仅检测成交量图bounds，还要检测K线图的X坐标范围，以便联动
        let mouse_pos = window.mouse_position();

        let mut cursor_x = None;

        // 计算对应的数据点索引（与K线图使用相同的逻辑）
        // 扩展检测：不仅检测成交量图bounds，还要检测整个窗口的X坐标范围
        let global_x = mouse_pos.x.as_f32();
        let bounds_x_start = bounds.origin.x.as_f32();
        let bounds_x_end = (bounds.origin.x + bounds.size.width).as_f32();

        if global_x >= bounds_x_start && global_x <= bounds_x_end {
            let local_x = global_x - bounds_x_start;
            for d in &self.data {
                if let Some(x_tick) = x.tick(&x_fn(d)) {
                    let band_start = x_tick;
                    let band_end = x_tick + band_width;
                    if local_x >= band_start && local_x <= band_end {
                        cursor_x = Some(px(x_tick + band_width / 2.0));
                        break;
                    }
                }
            }
        }

        // 绘制竖线（与K线图的十字光标联动）
        // 竖线延伸到图表区域底部
        if let Some(cx_pos) = cursor_x {
            let cursor_color = cx.theme().foreground.opacity(0.6);
            let mut vline_builder = PathBuilder::stroke(px(1.5)).dash_array(&[px(4.0), px(2.0)]); // 虚线样式：4像素实线，2像素空白
            let vline_start = origin_point(cx_pos, px(0.0), origin);
            let vline_end = origin_point(cx_pos, px(chart_height), origin); // 延伸到图表区域底部
            vline_builder.move_to(vline_start);
            vline_builder.line_to(vline_end);
            if let Ok(vline_path) = vline_builder.build() {
                window.paint_path(vline_path, Background::from(cursor_color));
            }
        }

        // 监听鼠标移动事件，触发重绘（与K线图联动）
        let chart_bounds = bounds;
        window.on_mouse_event({
            let bounds = chart_bounds;
            move |event: &gpui::MouseMoveEvent, _, window, _cx| {
                // 扩展检测范围：检测鼠标是否在图表bounds内，或者在整个窗口的X坐标范围内
                // 这样即使鼠标在K线图区域，成交量图的竖线也会更新
                let mouse_x = event.position.x;
                let bounds_x_start = bounds.origin.x;
                let bounds_x_end = bounds.origin.x + bounds.size.width;
                if mouse_x >= bounds_x_start && mouse_x <= bounds_x_end {
                    window.refresh();
                }
            }
        });
    }
}

// MACD图表组件
struct MacdChart {
    data: Vec<StockData>,
    macd_data: MacdData,
    show_left_y_axis: bool, // 是否显示左侧Y轴标签区域
}

impl MacdChart {
    fn new(data: Vec<StockData>) -> Self {
        let macd_data = calculate_macd(&data, |d| d.close);
        Self {
            data,
            macd_data,
            show_left_y_axis: false, // 默认与主图保持一致
        }
    }

    /// 设置是否显示左侧Y轴标签区域
    pub fn show_left_y_axis(mut self, show: bool) -> Self {
        self.show_left_y_axis = show;
        self
    }
}

impl IntoElement for MacdChart {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for MacdChart {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let style = Style {
            size: gpui::Size::full(),
            ..Default::default()
        };
        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Window,
        _: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        // 与主图K线图保持一致：减去左右Y轴标签宽度，确保X轴对齐
        let total_width = bounds.size.width.as_f32();
        let y_label_width = 50.0; // Y轴标签区域宽度（左右两侧各50）
                                  // 根据是否显示左侧Y轴标签区域调整宽度计算
        let left_y_axis_width = if self.show_left_y_axis {
            y_label_width
        } else {
            0.0
        };
        let chart_width = total_width - left_y_axis_width - y_label_width; // 图表实际宽度（中间区域，不包含左右Y轴标签区域）

        let origin = bounds.origin;
        let height = bounds.size.height.as_f32();
        let chart_height = height; // 使用全部高度，不预留X轴标签空间

        // X scale - 与主图一致，从左侧Y轴标签区域后开始
        let x_fn = |d: &StockData| d.date.clone();
        let x = ScaleBand::new(
            self.data.iter().map(|v| x_fn(v)).collect(),
            vec![left_y_axis_width, left_y_axis_width + chart_width],
        )
        .padding_inner(0.3)
        .padding_outer(0.1);
        let band_width = x.band_width();

        // 计算Y scale - 包含DIF、DEA和MACD的所有值
        let mut all_values: Vec<f64> = Vec::new();
        for v in self.macd_data.dif.iter().flatten() {
            all_values.push(*v);
        }
        for v in self.macd_data.dea.iter().flatten() {
            all_values.push(*v);
        }
        for v in self.macd_data.macd.iter().flatten() {
            all_values.push(*v);
        }

        if all_values.is_empty() {
            return;
        }

        let min_val = all_values.iter().copied().fold(f64::INFINITY, f64::min);
        let max_val = all_values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let range = max_val - min_val;
        let margin = range * 0.1;
        let domain_min = min_val - margin;
        let domain_max = max_val + margin;

        let y = ScaleLinear::new(vec![domain_min, domain_max], vec![chart_height, 10.]);

        // 绘制MACD柱状图（正值和负值用不同颜色）
        let success_color = cx.theme().success.opacity(0.7);
        let danger_color = cx.theme().danger.opacity(0.7);
        let zero_y = y.tick(&0.0).unwrap_or(chart_height / 2.0);

        for (i, d) in self.data.iter().enumerate() {
            if let Some(macd_val) = self.macd_data.macd.get(i).and_then(|v| *v) {
                if let Some(x_tick) = x.tick(&x_fn(d)) {
                    // 确保x_tick在图表区域内（不小于left_y_axis_width）
                    if x_tick >= left_y_axis_width {
                        if let Some(macd_y) = y.tick(&macd_val) {
                            let bar_x = x_tick + band_width * 0.1;
                            let bar_width = band_width * 0.8;
                            let bar_top = macd_y.min(zero_y);
                            let bar_bottom = macd_y.max(zero_y);
                            let bar_height = (bar_bottom - bar_top).abs();

                            let color = if macd_val >= 0.0 {
                                danger_color // 正值用红色
                            } else {
                                success_color // 负值用绿色
                            };

                            let bar_bounds = gpui::Bounds {
                                origin: origin_point(px(bar_x), px(bar_top), origin),
                                size: gpui::Size {
                                    width: px(bar_width),
                                    height: px(bar_height),
                                },
                            };
                            window.paint_quad(gpui::quad(
                                bar_bounds,
                                0.0,
                                color,
                                px(0.0),
                                color,
                                gpui::BorderStyle::default(),
                            ));
                        }
                    }
                }
            }
        }

        // 绘制DIF线（深灰色）
        use gpui_component::Colorize;
        let dif_color = Hsla::parse_hex("#575756").unwrap(); // 深灰色
        let mut dif_builder = PathBuilder::stroke(px(1.0));
        let mut has_dif_points = false;
        for (i, d) in self.data.iter().enumerate() {
            if let Some(dif_val) = self.macd_data.dif.get(i).and_then(|v| *v) {
                if let Some(x_tick) = x.tick(&x_fn(d)) {
                    // 确保x_tick在图表区域内（不小于left_y_axis_width）
                    if x_tick >= left_y_axis_width {
                        if let Some(dif_y) = y.tick(&dif_val) {
                            let point =
                                origin_point(px(x_tick + band_width / 2.0), px(dif_y), origin);
                            if !has_dif_points {
                                dif_builder.move_to(point);
                                has_dif_points = true;
                            } else {
                                dif_builder.line_to(point);
                            }
                        }
                    }
                }
            }
        }
        if has_dif_points {
            if let Ok(dif_path) = dif_builder.build() {
                window.paint_path(dif_path, Background::from(dif_color));
            }
        }

        // 绘制DEA线（紫色）
        let dea_color = Hsla::parse_hex("#e82bf6").unwrap(); // 紫色
        let mut dea_builder = PathBuilder::stroke(px(1.0));
        let mut has_dea_points = false;
        for (i, d) in self.data.iter().enumerate() {
            if let Some(dea_val) = self.macd_data.dea.get(i).and_then(|v| *v) {
                if let Some(x_tick) = x.tick(&x_fn(d)) {
                    // 确保x_tick在图表区域内（不小于left_y_axis_width）
                    if x_tick >= left_y_axis_width {
                        if let Some(dea_y) = y.tick(&dea_val) {
                            let point =
                                origin_point(px(x_tick + band_width / 2.0), px(dea_y), origin);
                            if !has_dea_points {
                                dea_builder.move_to(point);
                                has_dea_points = true;
                            } else {
                                dea_builder.line_to(point);
                            }
                        }
                    }
                }
            }
        }
        if has_dea_points {
            if let Ok(dea_path) = dea_builder.build() {
                window.paint_path(dea_path, Background::from(dea_color));
            }
        }

        // 绘制零轴线
        let zero_line_color = cx.theme().border;
        let mut zero_builder = PathBuilder::stroke(px(1.0));
        let chart_left_edge = left_y_axis_width; // 图表左边缘位置
        let chart_right_edge = left_y_axis_width + chart_width; // 图表右边缘位置
        let zero_start = origin_point(px(chart_left_edge), px(zero_y), origin);
        let zero_end = origin_point(px(chart_right_edge), px(zero_y), origin);
        zero_builder.move_to(zero_start);
        zero_builder.line_to(zero_end);
        if let Ok(zero_path) = zero_builder.build() {
            window.paint_path(zero_path, Background::from(zero_line_color));
        }

        // 绘制MACD Y轴标签 - 只显示4个值，与网格线对齐
        use gpui::point;
        use gpui::TextAlign;
        use gpui_component::plot::label::{Text, TEXT_GAP, TEXT_SIZE};
        use gpui_component::plot::{AxisText, Label};

        // 计算4个与网格线对齐的标签值
        // 网格线的Y坐标是 chart_height * i / 4.0 (i从0到3)
        // 需要找到对应的MACD值，使得y.tick(&macd_value)与网格线Y坐标对齐
        let label_count = 4;
        let mut macd_labels: Vec<AxisText> = Vec::new();
        for i in 0..label_count {
            // 网格线的Y坐标（从底部到顶部）
            let grid_y = chart_height * i as f32 / (label_count - 1) as f32;
            // 从Y坐标反推对应的MACD值
            // ScaleLinear映射：domain_min -> chart_height, domain_max -> 10.0
            // 所以：value = domain_min + (domain_max - domain_min) * (1.0 - (y - 10.0) / (chart_height - 10.0))
            let ratio = if chart_height > 10.0 {
                (grid_y - 10.0) / (chart_height - 10.0)
            } else {
                0.0
            };
            let macd_value = domain_min + (domain_max - domain_min) * (1.0 - ratio as f64);

            // 根据值的大小自动格式化
            let macd_text = if macd_value.abs() >= 10.0 {
                format!("{:.0}", macd_value)
            } else if macd_value.abs() >= 1.0 {
                format!("{:.1}", macd_value)
            } else {
                format!("{:.2}", macd_value)
            };
            macd_labels.push(
                AxisText::new(macd_text, grid_y, cx.theme().muted_foreground)
                    .align(TextAlign::Right),
            );
        }

        // 收集网格线的Y坐标（在绘制Y轴标签之前）
        let grid_y_positions: Vec<f32> = macd_labels
            .iter()
            .map(|label| label.tick.as_f32())
            .collect();

        // 绘制MACD Y轴标签（左右两侧）
        let macd_label_items: Vec<Text> = macd_labels
            .iter()
            .flat_map(|t| {
                // Y轴标签的Y坐标直接使用网格线的Y坐标，确保与网格线对齐
                let y_tick_f32 = t.tick.as_f32();
                // 确保Y坐标在图表绘制区域内（不超出bounds）
                let clamped_y = y_tick_f32.max(10.0).min(chart_height);
                // 文本的origin是基线位置（底部），需要调整Y坐标使文本中心与网格线对齐
                let text_baseline_y = clamped_y - TEXT_SIZE / 2.0;

                let mut labels = Vec::new();

                // 左侧标签（仅在show_left_y_axis为true时添加）
                if self.show_left_y_axis {
                    let left_label = Text {
                        text: t.text.clone(),
                        origin: point(px(TEXT_GAP), px(text_baseline_y)), // 左侧，左对齐
                        color: t.color,
                        font_size: t.font_size,
                        font_weight: gpui::FontWeight::NORMAL,
                        align: TextAlign::Left,
                    };
                    labels.push(left_label);
                }

                // 右侧标签（右对齐）
                let right_label = Text {
                    text: t.text.clone(),
                    origin: point(px(total_width - TEXT_GAP), px(text_baseline_y)), // 右侧，右对齐
                    color: t.color,
                    font_size: t.font_size,
                    font_weight: gpui::FontWeight::NORMAL,
                    align: TextAlign::Right,
                };
                labels.push(right_label);

                labels
            })
            .collect();
        let macd_label = Label::new(macd_label_items);
        macd_label.paint(&bounds, window, cx);

        // 绘制Y轴标签区域与内容区域之间的分隔线（左右两侧）
        let origin = bounds.origin;
        let border_color = cx.theme().border;

        // 左侧分隔线（仅在show_left_y_axis为true时绘制）
        if self.show_left_y_axis {
            let left_divider_x = left_y_axis_width;
            let mut left_divider_builder = PathBuilder::stroke(px(1.0));
            let left_divider_start = origin_point(px(left_divider_x), px(0.0), origin);
            let left_divider_end = origin_point(px(left_divider_x), px(height), origin);
            left_divider_builder.move_to(left_divider_start);
            left_divider_builder.line_to(left_divider_end);
            if let Ok(left_divider_path) = left_divider_builder.build() {
                window.paint_path(left_divider_path, Background::from(border_color));
            }
        }

        // 右侧分隔线（Y轴标签区域左边缘）
        let right_divider_x = left_y_axis_width + chart_width;
        let mut right_divider_builder = PathBuilder::stroke(px(1.0));
        let right_divider_start = origin_point(px(right_divider_x), px(0.0), origin);
        let right_divider_end = origin_point(px(right_divider_x), px(height), origin);
        right_divider_builder.move_to(right_divider_start);
        right_divider_builder.line_to(right_divider_end);
        if let Ok(right_divider_path) = right_divider_builder.build() {
            window.paint_path(right_divider_path, Background::from(border_color));
        }

        // 绘制网格 - 网格线对应Y轴标签值，不超出Y轴标签区域
        // 创建只包含图表区域的bounds（不包含左右Y轴标签区域）
        let chart_bounds = gpui::Bounds {
            origin: origin_point(px(left_y_axis_width), px(0.0), bounds.origin),
            size: gpui::Size {
                width: px(chart_width), // 只使用图表宽度，不包含左右Y轴标签区域
                height: bounds.size.height,
            },
        };
        Grid::new()
            .y(grid_y_positions)
            .stroke(cx.theme().border)
            .dash_array(&[px(4.), px(2.)])
            .paint(&chart_bounds, window);

        // 不再绘制X轴标签（取消X轴标签占位）

        // 绘制竖线（与K线图的十字光标联动）
        let mouse_pos = window.mouse_position();
        let mut cursor_x = None;

        let global_x = mouse_pos.x.as_f32();
        let bounds_x_start = bounds.origin.x.as_f32();
        let bounds_x_end = (bounds.origin.x + bounds.size.width).as_f32();

        if global_x >= bounds_x_start && global_x <= bounds_x_end {
            let local_x = global_x - bounds_x_start;
            for d in &self.data {
                if let Some(x_tick) = x.tick(&x_fn(d)) {
                    let band_start = x_tick;
                    let band_end = x_tick + band_width;
                    if local_x >= band_start && local_x <= band_end {
                        cursor_x = Some(px(x_tick + band_width / 2.0));
                        break;
                    }
                }
            }
        }

        if let Some(cx_pos) = cursor_x {
            let cursor_color = cx.theme().foreground.opacity(0.6);
            let mut vline_builder = PathBuilder::stroke(px(1.5)).dash_array(&[px(4.0), px(2.0)]);
            let vline_start = origin_point(cx_pos, px(0.0), origin);
            let vline_end = origin_point(cx_pos, px(chart_height), origin);
            vline_builder.move_to(vline_start);
            vline_builder.line_to(vline_end);
            if let Ok(vline_path) = vline_builder.build() {
                window.paint_path(vline_path, Background::from(cursor_color));
            }
        }

        // 监听鼠标移动事件，触发重绘（与K线图联动）
        let chart_bounds = bounds;
        window.on_mouse_event({
            let bounds = chart_bounds;
            move |event: &gpui::MouseMoveEvent, _, window, _cx| {
                let mouse_x = event.position.x;
                let bounds_x_start = bounds.origin.x;
                let bounds_x_end = bounds.origin.x + bounds.size.width;
                if mouse_x >= bounds_x_start && mouse_x <= bounds_x_end {
                    window.refresh();
                }
            }
        });
    }
}

// 统一的股票图表组件（包含主图和幅图）
struct StockChart {
    data: Vec<StockData>,
    show_left_y_axis: bool, // 是否显示左侧Y轴标签区域（主图和幅图联动）
}

impl StockChart {
    fn new(data: Vec<StockData>) -> Self {
        Self {
            data,
            show_left_y_axis: false, // 默认不显示左侧Y轴标签区域
        }
    }

    /// 设置是否显示左侧Y轴标签区域（主图和幅图联动）
    pub fn show_left_y_axis(mut self, show: bool) -> Self {
        self.show_left_y_axis = show;
        self
    }
}

impl IntoElement for StockChart {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for StockChart {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let style = Style {
            size: gpui::Size::full(),
            ..Default::default()
        };
        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Window,
        _: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        // 这个组件不直接绘制，而是通过子组件绘制
        // 实际的绘制逻辑在子组件中
    }
}

fn stock_chart(data: Vec<StockData>, cx: &mut Context<Example>) -> impl IntoElement {
    // 统一的左侧Y轴显示开关（主图和幅图联动）
    let show_left_y_axis = false;

    v_flex()
        .gap_4()
        .size_full()
        .p_4()
        .child(div().font_semibold().text_lg().child("股票 K 线图"))
        .child(
            div()
                .flex_1()
                .min_h(px(300.))
                .border_1()
                .border_color(cx.theme().border)
                .rounded_lg()
                .p_4()
                .child(
                    CandlestickChart::new(data.clone())
                        .x(|d| d.date.clone())
                        .open(|d| d.open)
                        .high(|d| d.high)
                        .low(|d| d.low)
                        .close(|d| d.close)
                        .tick_margin(3)
                        .show_left_y_axis(show_left_y_axis), // 使用统一的开关
                ),
        )
        .child({
            let success_color = cx.theme().success.opacity(0.7);
            let danger_color = cx.theme().danger.opacity(0.7);
            div()
                .h(px(150.))
                .border_1()
                .border_color(cx.theme().border)
                .rounded_lg()
                .p_4()
                .child(
                    VolumeChart::new(data.clone(), success_color, danger_color)
                        .show_left_y_axis(show_left_y_axis), // 使用统一的开关
                )
        })
        .child({
            div()
                .h(px(150.))
                .border_1()
                .border_color(cx.theme().border)
                .rounded_lg()
                .p_4()
                .child(
                    MacdChart::new(data).show_left_y_axis(show_left_y_axis), // 使用统一的开关
                )
        })
}

// 生成示例股票数据（使用伪随机数生成器，不依赖外部库）
fn generate_stock_data(days: usize) -> Vec<StockData> {
    let mut data = Vec::with_capacity(days);
    let mut price = 100.0; // 起始价格
    let mut trend = 0.0; // 趋势因子
    let mut seed = 12345u64; // 伪随机数种子

    // 简单的线性同余生成器
    let mut random = || {
        seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        (seed >> 16) as f64 / 65536.0 // 0.0 到 1.0
    };

    // 月份天数
    let month_days = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

    let mut month = 1;
    let mut day = 1;

    for i in 0..days {
        // 格式化日期
        let date_str = format!("{:02}-{:02}", month, day);

        // 更新日期
        day += 1;
        if day > month_days[month as usize - 1] {
            day = 1;
            month += 1;
            if month > 12 {
                month = 1;
            }
        }

        // 更新趋势（使用正弦波和伪随机数模拟趋势）
        trend += (i as f64 * 0.01).sin() * 0.5 + (random() - 0.5) * 0.3;
        trend = trend.clamp(-2.0, 2.0);

        // 计算开盘价（基于前一天的收盘价，加上小幅波动）
        let open = price + (random() - 0.5) * 2.0;

        // 计算价格变化（包含趋势和随机波动）
        let change = trend + (random() - 0.5) * 3.0;
        let close = open + change;

        // 确保价格合理（不会偏离起始价格太远）
        let close = close.max(price * 0.5).min(price * 2.0);

        // 计算最高价和最低价
        let volatility = (random() * 0.03 + 0.01) * price;
        let high = close.max(open) + volatility * random();
        let low = close.min(open) - volatility * random();

        // 确保 high >= max(open, close) 和 low <= min(open, close)
        let high = high.max(open.max(close) * 1.01);
        let low = low.min(open.min(close) * 0.99);

        // 生成成交量（与价格波动相关）
        let volume_base = 1000000.0;
        let change_ratio = change.abs() / price.max(1.0);
        let volume_variation = (change_ratio * 10.0 + 1.0) * volume_base;
        let volume = (volume_variation * (0.8 + random() * 0.4)) as u64;

        data.push(StockData {
            date: date_str,
            open,
            high,
            low,
            close,
            volume,
        });

        price = close; // 下一天的开盘价基于今天的收盘价
    }

    data
}

// 股票数据缓存（用于文件持久化）
#[derive(Serialize, Deserialize)]
struct StockDataCacheFile {
    data: Vec<StockData>,
    timestamp: u64, // Unix timestamp (秒)
    symbol: String,
}

impl StockDataCacheFile {
    fn new(data: Vec<StockData>, symbol: String) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Self {
            data,
            timestamp,
            symbol,
        }
    }

    fn is_valid(&self, symbol: &str, max_age: Duration) -> bool {
        if self.symbol != symbol {
            return false;
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let age = Duration::from_secs(now.saturating_sub(self.timestamp));
        age < max_age
    }
}

// 内存缓存（用于运行时）
struct StockDataCache {
    data: Vec<StockData>,
    timestamp: Instant,
    symbol: String,
}

impl StockDataCache {
    fn new(data: Vec<StockData>, symbol: String) -> Self {
        Self {
            data,
            timestamp: Instant::now(),
            symbol,
        }
    }

    fn is_valid(&self, symbol: &str, max_age: Duration) -> bool {
        self.symbol == symbol && self.timestamp.elapsed() < max_age
    }
}

// 获取缓存文件路径
fn cache_file_path(symbol: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push("gpui_stock_cache");
    std::fs::create_dir_all(&path).ok();
    path.push(format!("{}.json", symbol));
    path
}

// 从文件加载缓存
fn load_cache_from_file(symbol: &str) -> Option<StockDataCacheFile> {
    let path = cache_file_path(symbol);
    if !path.exists() {
        return None;
    }

    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<StockDataCacheFile>(&content) {
            Ok(cache) => Some(cache),
            Err(e) => {
                eprintln!("解析缓存文件失败: {}", e);
                None
            }
        },
        Err(e) => {
            eprintln!("读取缓存文件失败: {}", e);
            None
        }
    }
}

// 保存缓存到文件
fn save_cache_to_file(cache: &StockDataCacheFile) -> anyhow::Result<()> {
    let path = cache_file_path(&cache.symbol);
    let content = serde_json::to_string_pretty(cache)?;
    std::fs::write(&path, content)?;
    Ok(())
}

pub struct Example {
    stock_data: Vec<StockData>,
    loading: bool,
    cache: Option<StockDataCache>,
}

impl Example {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut example = Self {
            stock_data: Vec::new(),
            loading: true,
            cache: None,
        };

        // 先尝试从文件加载缓存
        let symbol = "AAPL";
        const CACHE_DURATION: Duration = Duration::from_secs(50 * 60);
        if let Some(file_cache) = load_cache_from_file(symbol) {
            if file_cache.is_valid(symbol, CACHE_DURATION) {
                // 使用文件缓存数据，立即显示，无需等待网络请求
                example.stock_data = file_cache.data.clone();
                example.loading = false;
                // 同时更新内存缓存
                example.cache = Some(StockDataCache::new(file_cache.data, symbol.to_string()));
                cx.notify();
                return example;
            }
        }

        // 如果文件缓存无效或不存在，异步加载股票数据
        example.load_stock_data(window, cx);

        example
    }

    fn load_stock_data(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view_handle = cx.entity().downgrade();
        let http_client = cx.http_client().clone();
        let symbol = "AAPL";
        let url = format!(
            "https://query1.finance.yahoo.com/v8/finance/chart/{}?interval=1d&range=1y",
            symbol
        );

        // 检查缓存（缓存有效期5分钟）
        const CACHE_DURATION: Duration = Duration::from_secs(5 * 60);
        if let Some(cache) = &self.cache {
            if cache.is_valid(symbol, CACHE_DURATION) {
                // 使用缓存数据
                self.stock_data = cache.data.clone();
                self.loading = false;
                cx.notify();
                return;
            }
        }

        // 使用 spawn_in 来执行 HTTP 请求并更新 UI
        cx.spawn_in(window, async move |_, cx| {
            // 使用 gpui 的 HTTP 客户端接口
            let result = fetch_stock_data_with_client(&url, http_client.as_ref()).await;

            cx.update(|_window, cx| {
                if let Some(view) = view_handle.upgrade() {
                    view.update(cx, |view, _| {
                        match result {
                            Ok(data) => {
                                // 更新内存缓存
                                view.cache =
                                    Some(StockDataCache::new(data.clone(), symbol.to_string()));

                                // 保存到文件缓存
                                let file_cache =
                                    StockDataCacheFile::new(data.clone(), symbol.to_string());
                                if let Err(e) = save_cache_to_file(&file_cache) {
                                    eprintln!("保存缓存文件失败: {}", e);
                                }

                                view.stock_data = data;
                                view.loading = false;
                            }
                            Err(e) => {
                                eprintln!("获取股票数据失败: {}, 使用模拟数据", e);
                                // 如果获取失败，使用模拟数据
                                view.stock_data = generate_stock_data(120);
                                view.loading = false;
                            }
                        }
                    });
                }
            })?;

            Ok::<_, anyhow::Error>(())
        })
        .detach();
    }
}

impl Render for Example {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.loading {
            return div()
                .v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child("正在加载股票数据...");
        }

        div()
            .v_flex()
            .gap_2()
            .size_full()
            .child(stock_chart(self.stock_data.clone(), cx))
            .child(Button::new("ok").primary().label("刷新数据").on_click({
                let view_handle = cx.entity().downgrade();
                move |_, window, cx| {
                    if let Some(view) = view_handle.upgrade() {
                        view.update(cx, |view, cx| {
                            view.loading = true;
                            // 清除内存缓存和文件缓存，强制重新获取数据
                            view.cache = None;
                            // 删除文件缓存
                            let symbol = "AAPL";
                            let cache_path = cache_file_path(symbol);
                            let _ = std::fs::remove_file(&cache_path);
                            view.load_stock_data(window, cx);
                        });
                    }
                }
            }))
    }
}

// 从网络获取股票数据 - 使用 gpui 的 HTTP 客户端接口
async fn fetch_stock_data_with_client(
    url: &str,
    http_client: &dyn gpui::http_client::HttpClient,
) -> anyhow::Result<Vec<StockData>> {
    use futures::AsyncReadExt;
    use gpui::http_client::{http, AsyncBody};

    let url = gpui::http_client::Url::parse(url)?;
    let request = http::Request::builder()
        .uri(url.as_str())
        .method("GET")
        .body(AsyncBody::empty())?;

    let response = http_client.send(request).await?;
    let (parts, body) = response.into_parts();

    if !parts.status.is_success() {
        anyhow::bail!("HTTP请求失败: {}", parts.status);
    }

    // 读取响应体
    let mut bytes = Vec::new();
    match body.0 {
        gpui::http_client::Inner::Bytes(cursor) => {
            bytes = cursor.into_inner().to_vec();
        }
        gpui::http_client::Inner::AsyncReader(mut reader) => {
            use std::pin::Pin;
            let mut pinned_reader = Pin::new(&mut reader);
            pinned_reader.read_to_end(&mut bytes).await?;
        }
        gpui::http_client::Inner::Empty => {}
    }

    let json: serde_json::Value = serde_json::from_slice(&bytes)?;

    // 解析Yahoo Finance API响应
    let result = json
        .get("chart")
        .and_then(|c| c.get("result"))
        .and_then(|r| r.as_array())
        .and_then(|arr| arr.first());

    if let Some(result) = result {
        let timestamps = result
            .get("timestamp")
            .and_then(|t| t.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_u64()).collect::<Vec<_>>())
            .unwrap_or_default();

        let indicators = result
            .get("indicators")
            .ok_or_else(|| anyhow::anyhow!("无法找到 indicators 字段"))?;
        let quote = indicators
            .get("quote")
            .and_then(|q| q.as_array())
            .and_then(|arr| arr.first())
            .ok_or_else(|| anyhow::anyhow!("无法找到 quote 数据"))?;

        let opens = quote
            .get("open")
            .and_then(|o| o.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_f64()).collect::<Vec<_>>())
            .unwrap_or_default();

        let highs = quote
            .get("high")
            .and_then(|h| h.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_f64()).collect::<Vec<_>>())
            .unwrap_or_default();

        let lows = quote
            .get("low")
            .and_then(|l| l.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_f64()).collect::<Vec<_>>())
            .unwrap_or_default();

        let closes = quote
            .get("close")
            .and_then(|c| c.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_f64()).collect::<Vec<_>>())
            .unwrap_or_default();

        let volumes = quote
            .get("volume")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_u64()).collect::<Vec<_>>())
            .unwrap_or_default();

        let mut data = Vec::new();
        let len = timestamps
            .len()
            .min(opens.len())
            .min(highs.len())
            .min(lows.len())
            .min(closes.len());

        // 只取最新的120根K线
        let start_idx = len.saturating_sub(120);

        for i in start_idx..len {
            let timestamp = timestamps[i];
            let date = chrono::DateTime::from_timestamp(timestamp as i64, 0)
                .unwrap_or_else(|| chrono::Utc::now());
            let date_naive = date.date_naive();
            let month = date_naive.month();
            let day = date_naive.day();
            let date_str = format!("{:02}-{:02}", month, day);

            data.push(StockData {
                date: date_str,
                open: opens.get(i).copied().unwrap_or(0.0),
                high: highs.get(i).copied().unwrap_or(0.0),
                low: lows.get(i).copied().unwrap_or(0.0),
                close: closes.get(i).copied().unwrap_or(0.0),
                volume: volumes.get(i).copied().unwrap_or(0),
            });
        }

        Ok(data)
    } else {
        anyhow::bail!("无法解析股票数据")
    }
}

fn main() {
    let app = Application::new();

    app.run(move |cx| {
        // This must be called before using any GPUI Component features.
        gpui_component::init(cx);

        // 设置 HTTP 客户端
        let http_client = std::sync::Arc::new(
            reqwest_client::ReqwestClient::user_agent("gpui-component/hello_world").unwrap(),
        );
        cx.set_http_client(http_client);

        cx.spawn(async move |cx| {
            cx.open_window(WindowOptions::default(), |window, cx| {
                let view = cx.new(|cx| Example::new(window, cx));
                // This first level on the window, should be a Root.
                cx.new(|cx| Root::new(view.into(), window, cx))
            })?;

            Ok::<_, anyhow::Error>(())
        })
        .detach();
    });
}
