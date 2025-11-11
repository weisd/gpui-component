use gpui::*;
use gpui_component::plot::{scale::*, Plot};
use gpui_component::*;
use serde::{Deserialize, Serialize};

// 股票数据结构
#[derive(Clone, Serialize, Deserialize)]
struct StockData {
    date: String,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: u64,
}

// 生成示例数据
fn generate_sample_data(count: usize) -> Vec<StockData> {
    let mut data = Vec::new();
    let mut price = 100.0;
    let mut rng = 12345u64; // 简单的伪随机数生成器

    for i in 0..count {
        // 简单的伪随机数生成
        rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
        let change = ((rng % 100) as f64 / 100.0 - 0.5) * 5.0; // -2.5 到 2.5 的变化

        let open = price;
        let close = price + change;
        let high = open.max(close) + (rng % 10) as f64 / 10.0;
        let low = open.min(close) - (rng % 10) as f64 / 10.0;
        let volume = 1000000 + (rng % 5000000) as u64;

        price = close;

        let date = format!("2024-{:02}-{:02}", 1 + (i / 30), 1 + (i % 30));
        data.push(StockData {
            date,
            open,
            high,
            low,
            close,
            volume,
        });
    }

    data
}

// K线图组件
struct KLineChart {
    data: Vec<StockData>,
}

impl KLineChart {
    fn new(data: Vec<StockData>) -> Self {
        Self { data }
    }
}

impl IntoElement for KLineChart {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for KLineChart {
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
            size: gpui::Size::new(px(800.0), px(600.0)),
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
        let width = bounds.size.width.as_f32();
        let height = bounds.size.height.as_f32();

        // X轴scale
        let x = ScaleBand::new(
            self.data.iter().map(|d| d.date.clone()).collect(),
            vec![0., width],
        )
        .padding_inner(0.3)
        .padding_outer(0.1);

        // Y轴scale（价格）
        let all_prices: Vec<f64> = self
            .data
            .iter()
            .flat_map(|d| vec![d.high, d.low, d.open, d.close])
            .collect();
        let min_price = all_prices.iter().fold(f64::INFINITY, |a, &b| a.min(b));
        let max_price = all_prices.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
        let price_range = max_price - min_price;
        let margin = price_range * 0.1;
        let y = ScaleLinear::new(
            vec![min_price - margin, max_price + margin],
            vec![height - 50., 10.],
        );

        // 绘制蜡烛图
        let band_width = x.band_width();
        for d in &self.data {
            if let Some(x_tick) = x.tick(&d.date) {
                let center_x = x_tick + band_width / 2.0;

                if let (Some(open_y), Some(close_y), Some(high_y), Some(low_y)) = (
                    y.tick(&d.open),
                    y.tick(&d.close),
                    y.tick(&d.high),
                    y.tick(&d.low),
                ) {
                    let is_up = d.close >= d.open;
                    let body_top = open_y.min(close_y);
                    let body_bottom = open_y.max(close_y);

                    // 绘制上影线
                    if high_y < body_top {
                        let mut builder = PathBuilder::stroke(px(1.0));
                        builder.move_to(point(px(center_x), px(body_top)));
                        builder.line_to(point(px(center_x), px(high_y)));
                        if let Ok(path) = builder.build() {
                            window.paint_path(
                                path,
                                if is_up {
                                    cx.theme().danger
                                } else {
                                    cx.theme().success
                                },
                            );
                        }
                    }

                    // 绘制下影线
                    if low_y > body_bottom {
                        let mut builder = PathBuilder::stroke(px(1.0));
                        builder.move_to(point(px(center_x), px(body_bottom)));
                        builder.line_to(point(px(center_x), px(low_y)));
                        if let Ok(path) = builder.build() {
                            window.paint_path(
                                path,
                                if is_up {
                                    cx.theme().danger
                                } else {
                                    cx.theme().success
                                },
                            );
                        }
                    }

                    // 绘制实体（上涨红色空心，下跌绿色实心）
                    let body_color = if is_up {
                        cx.theme().danger
                    } else {
                        cx.theme().success
                    };

                    if is_up {
                        // 上涨：红色空心（只绘制边框）
                        let mut builder = PathBuilder::stroke(px(1.0));
                        builder.move_to(point(px(center_x - band_width * 0.3), px(body_top)));
                        builder.line_to(point(px(center_x + band_width * 0.3), px(body_top)));
                        builder.line_to(point(px(center_x + band_width * 0.3), px(body_bottom)));
                        builder.line_to(point(px(center_x - band_width * 0.3), px(body_bottom)));
                        builder.close();
                        if let Ok(path) = builder.build() {
                            window.paint_path(path, body_color);
                        }
                    } else {
                        // 下跌：绿色实心
                        let mut builder = PathBuilder::fill();
                        builder.move_to(point(px(center_x - band_width * 0.3), px(body_top)));
                        builder.line_to(point(px(center_x + band_width * 0.3), px(body_top)));
                        builder.line_to(point(px(center_x + band_width * 0.3), px(body_bottom)));
                        builder.line_to(point(px(center_x - band_width * 0.3), px(body_bottom)));
                        builder.close();
                        if let Ok(path) = builder.build() {
                            window.paint_path(path, body_color);
                        }
                    }
                }
            }
        }
    }
}

// 主应用
struct Example {
    data: Vec<StockData>,
}

impl Example {
    fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self {
            data: generate_sample_data(60),
        }
    }
}

impl Render for Example {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .child(KLineChart::new(self.data.clone()))
    }
}

fn main() {
    let app = Application::new();

    app.run(move |cx| {
        // This must be called before using any GPUI Component features.
        gpui_component::init(cx);

        cx.open_window(WindowOptions::default(), |window, cx| {
            cx.new(|cx| Example::new(window, cx))
        })
        .unwrap();
    });
}
