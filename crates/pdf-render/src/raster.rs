//! The only module that touches `vello_cpu` (ADR-0003): replacing the
//! rasteriser, for example by `tiny-skia`, means rewriting this file.
//!
//! The adapter only uses fills, strokes and the clip stack. It never pushes
//! a layer: `vello_cpu` asserts that every layer is popped before rendering,
//! whereas pending clip paths are allowed.

use vello_cpu::color::{AlphaColor, Srgb};
use vello_cpu::kurbo::{Affine, BezPath, Cap, Join, PathEl, Point as KPoint, Rect, Stroke};
use vello_cpu::peniko::{Fill, ImageAlphaType};
use vello_cpu::{Pixmap, RenderContext, Resources};

use crate::geometry::{Matrix, Path, Segment};

/// Colour with straight alpha, components in `0.0..=1.0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineCap {
    Butt,
    Round,
    Square,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineJoin {
    Miter,
    Round,
    Bevel,
}

/// Stroke parameters in user space (ISO 32000-2, 8.4.3).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StrokeStyle {
    // Dashes, when present, are an even number of non-negative lengths
    // with a positive finite sum, and `dash_phase` lies in one period.
    pub width: f64,
    pub cap: LineCap,
    pub join: LineJoin,
    pub miter_limit: f64,
    /// Dash lengths (empty for a solid line) and phase.
    pub dashes: Vec<f64>,
    pub dash_phase: f64,
}

/// Width of a zero-width line, in device pixels: "the thinnest line that can
/// be rendered" (ISO 32000-2, 8.4.3.2).
pub(crate) const HAIRLINE: f64 = 1.0;

pub(crate) struct Canvas {
    context: RenderContext,
    width: u16,
    height: u16,
}

impl Canvas {
    /// A canvas filled with opaque white, the paper colour.
    pub fn new(width: u16, height: u16) -> Self {
        let mut context = RenderContext::new(width, height);
        context.set_paint(AlphaColor::<Srgb>::WHITE);
        context.fill_rect(&Rect::new(0.0, 0.0, f64::from(width), f64::from(height)));
        Self {
            context,
            width,
            height,
        }
    }

    /// Width and height in pixels.
    pub fn size(&self) -> (f64, f64) {
        (f64::from(self.width), f64::from(self.height))
    }

    pub fn fill(&mut self, path: &Path, matrix: &Matrix, even_odd: bool, color: Rgba) {
        self.context.set_transform(affine(matrix));
        self.context.set_fill_rule(fill_rule(even_odd));
        self.context.set_paint(paint(color));
        self.context.fill_path(&bez_path(path));
    }

    /// Strokes `path` (ISO 32000-2, 8.5.3.2). Dashes are always cut in
    /// user space along the untransformed path, as PDF defines them. The
    /// outline is then expanded in device space when the CTM is conformal
    /// (rotation, reflection and uniform scale) or the line is a hairline,
    /// so that the flattening tolerance is in device pixels; otherwise it is
    /// expanded in user space, where a non-uniform pen is correct.
    pub fn stroke(&mut self, path: &Path, matrix: &Matrix, style: &StrokeStyle, color: Rgba) {
        self.context.set_paint(paint(color));
        let mut user = bez_path(path);
        if !style.dashes.is_empty() {
            user = path_from(vello_cpu::kurbo::dash(
                user.into_iter(),
                style.dash_phase,
                &style.dashes,
            ));
        }
        let device_width = if style.width > 0.0 {
            conformal_scale(matrix).map(|scale| style.width * scale)
        } else {
            Some(HAIRLINE)
        };
        match device_width {
            Some(width) => {
                let mut device = user;
                device.apply_affine(affine(matrix));
                self.context.set_transform(Affine::IDENTITY);
                self.context.set_stroke(stroke(style, width));
                self.context.stroke_path(&device);
            }
            None => {
                self.context.set_transform(affine(matrix));
                self.context.set_stroke(stroke(style, style.width));
                self.context.stroke_path(&user);
            }
        }
    }

    pub fn push_clip(&mut self, path: &Path, matrix: &Matrix, even_odd: bool) {
        self.context.set_transform(affine(matrix));
        self.context.set_fill_rule(fill_rule(even_odd));
        self.context.push_clip_path(&bez_path(path));
    }

    pub fn pop_clip(&mut self) {
        self.context.pop_clip();
    }

    /// Rasterises the scene: RGBA8 with straight alpha, rows top to bottom.
    pub fn finish(mut self) -> Vec<u8> {
        let mut pixmap = Pixmap::new(self.width, self.height);
        let mut resources = Resources::new();
        self.context.flush();
        self.context.render(&mut pixmap, &mut resources);
        pixmap.take_rgba8(ImageAlphaType::Alpha)
    }
}

fn affine(matrix: &Matrix) -> Affine {
    Affine::new(matrix.coefficients())
}

fn fill_rule(even_odd: bool) -> Fill {
    if even_odd {
        Fill::EvenOdd
    } else {
        Fill::NonZero
    }
}

fn paint(color: Rgba) -> AlphaColor<Srgb> {
    AlphaColor::new([color.r, color.g, color.b, color.a])
}

/// The uniform scale of a conformal matrix, `None` for any other.
fn conformal_scale(matrix: &Matrix) -> Option<f64> {
    let scale = (matrix.a * matrix.d - matrix.b * matrix.c).abs().sqrt();
    let tolerance = 1e-9 * scale;
    let rotation =
        (matrix.a - matrix.d).abs() <= tolerance && (matrix.b + matrix.c).abs() <= tolerance;
    let reflection =
        (matrix.a + matrix.d).abs() <= tolerance && (matrix.b - matrix.c).abs() <= tolerance;
    (scale > 0.0 && (rotation || reflection)).then_some(scale)
}

/// A solid pen: dashes are applied to the path beforehand.
fn stroke(style: &StrokeStyle, width: f64) -> Stroke {
    let cap = match style.cap {
        LineCap::Butt => Cap::Butt,
        LineCap::Round => Cap::Round,
        LineCap::Square => Cap::Square,
    };
    let join = match style.join {
        LineJoin::Miter => Join::Miter,
        LineJoin::Round => Join::Round,
        LineJoin::Bevel => Join::Bevel,
    };
    Stroke::new(width)
        .with_caps(cap)
        .with_join(join)
        .with_miter_limit(style.miter_limit)
}

/// Builds a path from `elements`, dropping drawing elements that come before
/// any move: they have no start point, so nothing to draw (`kurbo`'s dash
/// output for a degenerate closed subpath starts so, and `BezPath` panics on
/// it).
fn path_from(elements: impl Iterator<Item = PathEl>) -> BezPath {
    let mut out = BezPath::new();
    for element in elements {
        if out.elements().is_empty() && !matches!(element, PathEl::MoveTo(_)) {
            continue;
        }
        out.push(element);
    }
    out
}

fn bez_path(path: &Path) -> BezPath {
    let point = |p: crate::geometry::Point| KPoint::new(p.x, p.y);
    let mut out = BezPath::new();
    for segment in path.segments() {
        match *segment {
            Segment::MoveTo(p) => out.move_to(point(p)),
            Segment::LineTo(p) => out.line_to(point(p)),
            Segment::CurveTo(a, b, c) => out.curve_to(point(a), point(b), point(c)),
            Segment::Close => out.close_path(),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conformal_matrices_are_recognised() {
        let rotation = Matrix::new(0.0, 2.0, -2.0, 0.0, 5.0, 5.0);
        assert_eq!(conformal_scale(&rotation), Some(2.0));
        let page_90 = Matrix::new(0.0, 3.0, 3.0, 0.0, 0.0, 0.0);
        assert_eq!(conformal_scale(&page_90), Some(3.0));
        assert_eq!(
            conformal_scale(&Matrix::new(1.0, 0.0, 0.0, 2.0, 0.0, 0.0)),
            None
        );
        assert_eq!(
            conformal_scale(&Matrix::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0)),
            None
        );
    }
}
