use chrono::Datelike;
use gpui::*;
use gpui_component::chart::BarChart;
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
            mouse_x: None,
            mouse_y: None,
            selected_index: None,
        }
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
        // 为Y轴标签单独分配空间，避免与K线重叠
        let y_label_width = 50.0; // Y轴标签区域宽度（右侧）
        let width = total_width - y_label_width; // K线图实际宽度（左侧，不包含Y轴标签区域）
                                                 // 为X轴标签单独分配空间，避免与最低价标签重叠
        let x_label_height = TEXT_SIZE + TEXT_GAP * 3.0; // X轴标签区域高度（文本高度 + 上下间距）
                                                         // X轴位置：在底部预留X轴标签空间
        let x_axis_y = total_height - x_label_height; // X轴位置
        let height = x_axis_y; // K线图实际高度（到X轴为止）

        // X scale - 使用 ScaleBand 以便蜡烛之间有间距
        let x = ScaleBand::new(self.data.iter().map(|v| x_fn(v)).collect(), vec![0., width])
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

            // 计算对应的数据点索引
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

        // 绘制 X 轴
        let data_len = self.data.len();
        let x_label = self.data.iter().enumerate().filter_map(|(i, d)| {
            if (i + 1) % self.tick_margin == 0 {
                x.tick(&x_fn(d)).map(|x_tick| {
                    let align = match i {
                        0 => {
                            if data_len == 1 {
                                TextAlign::Center
                            } else {
                                TextAlign::Left
                            }
                        }
                        i if i == data_len - 1 => TextAlign::Right,
                        _ => TextAlign::Center,
                    };
                    AxisText::new(
                        x_fn(d).into(),
                        x_tick + band_width / 2.,
                        cx.theme().muted_foreground,
                    )
                    .align(align)
                })
            } else {
                None
            }
        });

        // 绘制X轴和标签
        // 先绘制X轴线
        let origin = bounds.origin;
        let mut x_axis_builder = PathBuilder::stroke(px(1.0));
        let x_axis_start = origin_point(px(0.0), px(x_axis_y), origin);
        let x_axis_end = origin_point(px(width), px(x_axis_y), origin);
        x_axis_builder.move_to(x_axis_start);
        x_axis_builder.line_to(x_axis_end);
        if let Ok(x_axis_path) = x_axis_builder.build() {
            window.paint_path(x_axis_path, Background::from(cx.theme().border));
        }

        // 绘制X轴标签，在X轴下方，使用point而不是origin_point，确保不超出bounds
        let x_label_y = x_axis_y + TEXT_GAP; // X轴标签位置（X轴下方一个TEXT_GAP）
                                             // 确保标签不超出bounds底部（考虑文本高度）
        let max_label_y = total_height - TEXT_SIZE - TEXT_GAP;
        let clamped_x_label_y = x_label_y.min(max_label_y);
        let x_label_items: Vec<Text> = x_label
            .into_iter()
            .map(|t| {
                // 确保X坐标在bounds范围内
                let x_tick_f32 = t.tick.as_f32();
                let clamped_x = x_tick_f32.max(0.0).min(width);
                Text {
                    text: t.text,
                    origin: point(px(clamped_x), px(clamped_x_label_y)), // 使用point，传递相对坐标
                    color: t.color,
                    font_size: t.font_size,
                    font_weight: gpui::FontWeight::NORMAL,
                    align: t.align,
                }
            })
            .collect();
        let x_label_comp = Label::new(x_label_items);
        x_label_comp.paint(&bounds, window, cx);

        // 绘制右侧Y轴价格标签
        let y_label_count = 5;
        let y_labels: Vec<AxisText> = (0..y_label_count)
            .map(|i| {
                let ratio = i as f64 / (y_label_count - 1) as f64;
                let price = domain_min + (domain_max - domain_min) * (1.0 - ratio);
                let y_tick = height * (i as f32 / (y_label_count - 1) as f32);
                AxisText::new(format!("{:.2}", price), y_tick, cx.theme().muted_foreground)
                    .align(TextAlign::Right)
            })
            .collect();

        // 在右侧绘制Y轴标签，使用point传递相对坐标（Label::paint会自动加上bounds.origin）
        let y_label_items: Vec<Text> = y_labels
            .into_iter()
            .map(|t| {
                // 确保Y坐标在bounds范围内
                let y_tick_f32 = t.tick.as_f32();
                let clamped_y = y_tick_f32.max(0.0).min(height);
                // Y轴标签显示在右侧预留区域（K线图右侧）
                // 使用point传递相对坐标，Label::paint会自动加上bounds.origin
                // 由于是右对齐，origin.x是文本的右边缘位置，所以应该设置为total_width - TEXT_GAP
                let y_label_x = total_width - TEXT_GAP; // Y轴标签X位置（预留区域右侧，右对齐）
                Text {
                    text: t.text,
                    origin: point(px(y_label_x), px(clamped_y)), // 使用point，传递相对坐标
                    color: t.color,
                    font_size: t.font_size,
                    font_weight: gpui::FontWeight::NORMAL,
                    align: TextAlign::Right,
                }
            })
            .collect();
        let y_label = Label::new(y_label_items);
        y_label.paint(&bounds, window, cx);

        // 绘制网格
        Grid::new()
            .y((0..=3).map(|i| height * i as f32 / 4.0).collect())
            .stroke(cx.theme().border)
            .dash_array(&[px(4.), px(2.)])
            .paint(&bounds, window);

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
            Hsla::parse_hex("#FFA500").unwrap(), // MA250 (橙色)
        ];
        let ma_data = [&ma5, &ma10, &ma20, &ma30, &ma60, &ma120, &ma250];

        for (ma_values, &color) in ma_data.iter().zip(ma_colors.iter()) {
            let mut line_builder = PathBuilder::stroke(px(1.0));
            let mut has_points = false;

            for (i, d) in self.data.iter().enumerate() {
                if let Some(ma_value) = ma_values.get(i).and_then(|v| *v) {
                    if let Some(x_tick) = x.tick(&x_fn(d)) {
                        if let Some(ma_y_f32) = y.tick(&ma_value) {
                            let ma_y = px(ma_y_f32);
                            let point = origin_point(px(x_tick + band_width / 2.0), ma_y, origin);

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

        if let Some(min_data) = self.data.get(min_price_index) {
            // 确保使用正确的数据来获取x_tick
            let x_value = x_fn(min_data);
            if let Some(x_tick) = x.tick(&x_value) {
                if let Some(min_y) = y.tick(&min_price_value) {
                    // marker_x 是K线的中心位置
                    let marker_x = x_tick + band_width / 2.0;
                    let arrow_size = 8.0;

                    // 绘制向上指向的箭头（在最低点下方）
                    // 确保箭头和标签在X轴标签上方，不会重叠
                    let arrow_bottom_y = min_y + 20.0; // 减少箭头与最低点的距离，避免超出K线图区域
                    let arrow_tip = origin_point(px(marker_x), px(min_y), origin);
                    let arrow_left =
                        origin_point(px(marker_x - arrow_size / 2.0), px(arrow_bottom_y), origin);
                    let arrow_right =
                        origin_point(px(marker_x + arrow_size / 2.0), px(arrow_bottom_y), origin);

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
                    // 确保标签在X轴标签上方，不会重叠
                    let label_text = format!("最低: {:.2}", min_price_value);
                    let label_center_x = marker_x; // 使用K线中心
                    let label_center_y = arrow_bottom_y + 15.0; // 标签在箭头下方，合适的距离
                                                                // 检查标签是否会超出K线图区域（X轴上方），如果会则调整位置
                                                                // 确保标签在X轴上方至少保留TEXT_GAP的距离
                    let max_label_y = height - TEXT_GAP - TEXT_SIZE;
                    let label_center_y = label_center_y.min(max_label_y);
                    // 文本的origin.y是基线位置，为了垂直居中，需要稍微向下调整
                    let text_baseline_y = label_center_y + 3.0;
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
            let hline_start = origin_point(px(0.0), cy_pos, origin);
            let hline_end = origin_point(px(width), cy_pos, origin);
            hline_builder.move_to(hline_start);
            hline_builder.line_to(hline_end);
            if let Ok(hline_path) = hline_builder.build() {
                window.paint_path(hline_path, Background::from(cursor_color));
            }

            // 在右侧显示当前价格（显示在Y轴标签区域内，右对齐）
            let cy_pos_f32 = cy_pos.as_f32();
            let height_f32 = height;
            let ratio = 1.0 - cy_pos_f32 / height_f32;
            let current_price = domain_min + (domain_max - domain_min) * ratio as f64;
            let price_text = format!("{:.2}", current_price);
            let price_label = Label::new(vec![Text {
                text: price_text.into(),
                origin: point(px(total_width - TEXT_GAP), cy_pos), // 使用point，右对齐，显示在Y轴标签区域内
                color: cursor_color,
                font_size: px(12.0),
                font_weight: gpui::FontWeight::SEMIBOLD,
                align: TextAlign::Right,
            }]);
            price_label.paint(&bounds, window, cx);

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
    bar_chart: BarChart<StockData, String, f64>,
    data: Vec<StockData>,
}

impl VolumeChart {
    fn new(data: Vec<StockData>, success_color: Hsla, danger_color: Hsla) -> Self {
        Self {
            bar_chart: BarChart::new(data.clone())
                .x(|d| d.date.clone())
                .y(|d| d.volume as f64)
                .fill(move |d| {
                    // 与K线图颜色一致：上涨红色，下跌绿色
                    if d.close > d.open {
                        danger_color
                    } else {
                        success_color
                    }
                })
                .tick_margin(3),
            data,
        }
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
        // 与主图K线图保持一致：减去Y轴标签宽度，确保X轴对齐
        let total_width = bounds.size.width.as_f32();
        let y_label_width = 50.0; // 与主图K线图一致
        let chart_width = total_width - y_label_width; // 图表实际宽度（不包含Y轴标签区域）

        // 创建一个调整后的bounds，使BarChart使用与主图相同的宽度
        let adjusted_bounds = Bounds {
            origin: bounds.origin,
            size: gpui::Size {
                width: px(chart_width),
                height: bounds.size.height,
            },
        };

        // 先绘制BarChart（使用调整后的bounds，确保X轴与主图对齐）
        <BarChart<StockData, String, f64> as Plot>::paint(
            &mut self.bar_chart,
            adjusted_bounds,
            window,
            cx,
        );

        // 扩展检测范围：检测鼠标是否在K线图或成交量图的X坐标范围内
        // 不仅检测成交量图bounds，还要检测K线图的X坐标范围，以便联动
        let mouse_pos = window.mouse_position();
        let origin = bounds.origin;
        let width = chart_width; // 使用与主图相同的宽度
        let height = bounds.size.height.as_f32();

        // 使用与K线图相同的X scale计算（确保竖线对齐）
        let x_fn = |d: &StockData| d.date.clone();
        let x = ScaleBand::new(self.data.iter().map(|v| x_fn(v)).collect(), vec![0., width])
            .padding_inner(0.3) // 与K线图一致
            .padding_outer(0.1); // 与K线图一致
        let band_width = x.band_width();

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
        // 竖线只延伸到图表区域底部（X轴位置），不超出X轴标签区域
        if let Some(cx_pos) = cursor_x {
            let cursor_color = cx.theme().foreground.opacity(0.6);
            // BarChart使用AXIS_GAP预留X轴标签空间，图表区域高度是 height - AXIS_GAP
            // 竖线应该只延伸到图表区域底部（X轴位置）
            use gpui_component::plot::AXIS_GAP;
            let chart_height = height - AXIS_GAP; // 图表区域高度（不包括X轴标签）
            let mut vline_builder = PathBuilder::stroke(px(1.5)).dash_array(&[px(4.0), px(2.0)]); // 虚线样式：4像素实线，2像素空白
            let vline_start = origin_point(cx_pos, px(0.0), origin);
            let vline_end = origin_point(cx_pos, px(chart_height), origin); // 只延伸到图表区域底部
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

fn stock_chart(data: Vec<StockData>, cx: &mut Context<Example>) -> impl IntoElement {
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
                        .tick_margin(3),
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
                .child(VolumeChart::new(data, success_color, danger_color))
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
