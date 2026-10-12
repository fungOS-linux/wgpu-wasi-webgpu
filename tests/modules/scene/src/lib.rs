//! The scene vello-scenes renders through wgpu-wasi-webgpu, and the crate's
//! test renders natively with vello_cpu to compare: on a white background,
//! a filled circle, a stroked rectangle laid over it with some alpha, and a
//! curved path filled with a linear gradient. 512x512.

pub use vello_common;

use vello_common::kurbo::{BezPath, Circle, Join, Rect, Shape, Stroke};
use vello_common::paint::PaintType;
use vello_common::peniko::{Color, Gradient};

pub const WIDTH: u16 = 512;
pub const HEIGHT: u16 = 512;

/// One drawing call, in order.
pub enum Draw {
    Fill(BezPath, PaintType),
    Stroke(BezPath, Stroke, PaintType),
}

pub fn draws() -> Vec<Draw> {
    let background = Rect::new(0.0, 0.0, f64::from(WIDTH), f64::from(HEIGHT));
    let circle = Circle::new((180.0, 180.0), 120.0);
    let frame = Rect::new(250.0, 70.0, 460.0, 260.0);

    let mut wave = BezPath::new();
    wave.move_to((60.0, 340.0));
    wave.curve_to((160.0, 270.0), (260.0, 420.0), (360.0, 330.0));
    wave.curve_to((420.0, 280.0), (480.0, 360.0), (460.0, 430.0));
    wave.quad_to((300.0, 520.0), (120.0, 470.0));
    wave.close_path();
    let gradient = Gradient::new_linear((60.0, 300.0), (460.0, 480.0)).with_stops([
        Color::from_rgb8(220, 40, 40),
        Color::from_rgb8(250, 200, 30),
        Color::from_rgb8(120, 40, 180),
    ]);

    vec![
        Draw::Fill(background.to_path(0.1), Color::WHITE.into()),
        Draw::Fill(circle.to_path(0.1), Color::from_rgb8(40, 100, 220).into()),
        Draw::Stroke(
            frame.to_path(0.1),
            Stroke::new(14.0).with_join(Join::Round),
            Color::from_rgba8(20, 140, 60, 200).into(),
        ),
        Draw::Fill(wave, gradient.into()),
    ]
}

/// Draws the scene on anything with vello's `set_paint`, `set_stroke`,
/// `fill_path` and `stroke_path`: vello_hybrid's `Scene` and vello_cpu's
/// `RenderContext` both have them, under no shared trait.
#[macro_export]
macro_rules! draw {
    ($target:expr) => {{
        let target = &mut $target;
        for draw in $crate::draws() {
            match draw {
                $crate::Draw::Fill(path, paint) => {
                    target.set_paint(paint);
                    target.fill_path(&path);
                }
                $crate::Draw::Stroke(path, stroke, paint) => {
                    target.set_paint(paint);
                    target.set_stroke(stroke);
                    target.stroke_path(&path);
                }
            }
        }
    }};
}
