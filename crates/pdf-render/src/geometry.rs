//! Matrices, points and paths in the engine's own types, so that the
//! rasteriser stays behind `raster.rs` (ADR-0003).

/// Affine matrix `[a b c d e f]` with the PDF convention (ISO 32000-2, 8.3.4):
/// `x' = a x + c y + e`, `y' = b x + d y + f`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Matrix {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Matrix {
    pub const IDENTITY: Self = Self::new(1.0, 0.0, 0.0, 1.0, 0.0, 0.0);

    pub const fn new(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> Self {
        Self { a, b, c, d, e, f }
    }

    /// The transform that applies `self`, then `next`. `cm` with operand `m`
    /// makes the new CTM `m.then(ctm)` (ISO 32000-2, 8.4.4).
    pub fn then(&self, next: &Self) -> Self {
        Self {
            a: self.a * next.a + self.b * next.c,
            b: self.a * next.b + self.b * next.d,
            c: self.c * next.a + self.d * next.c,
            d: self.c * next.b + self.d * next.d,
            e: self.e * next.a + self.f * next.c + next.e,
            f: self.e * next.b + self.f * next.d + next.f,
        }
    }

    pub fn apply(&self, point: Point) -> Point {
        Point {
            x: self.a * point.x + self.c * point.y + self.e,
            y: self.b * point.x + self.d * point.y + self.f,
        }
    }

    pub fn is_finite(&self) -> bool {
        [self.a, self.b, self.c, self.d, self.e, self.f]
            .iter()
            .all(|v| v.is_finite())
    }

    pub fn coefficients(&self) -> [f64; 6] {
        [self.a, self.b, self.c, self.d, self.e, self.f]
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    fn distance(self, other: Self) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }
}

/// One path element, in user space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Segment {
    MoveTo(Point),
    LineTo(Point),
    CurveTo(Point, Point, Point),
    Close,
}

/// A path under construction (ISO 32000-2, 8.5.2).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Path {
    segments: Vec<Segment>,
    /// Start of the current subpath, if any.
    start: Option<Point>,
    /// Current point, if any.
    current: Option<Point>,
}

impl Path {
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    pub fn current_point(&self) -> Option<Point> {
        self.current
    }

    pub fn clear(&mut self) {
        self.segments.clear();
        self.start = None;
        self.current = None;
    }

    pub fn move_to(&mut self, point: Point) {
        // Consecutive `m` replace the previous one (ISO 32000-2, 8.5.2.1).
        if let Some(Segment::MoveTo(last)) = self.segments.last_mut() {
            *last = point;
        } else {
            self.segments.push(Segment::MoveTo(point));
        }
        self.start = Some(point);
        self.current = Some(point);
    }

    /// Returns `false`, changing nothing, without a current point.
    pub fn line_to(&mut self, point: Point) -> bool {
        if self.current.is_none() {
            return false;
        }
        self.segments.push(Segment::LineTo(point));
        self.current = Some(point);
        true
    }

    /// Returns `false`, changing nothing, without a current point.
    pub fn curve_to(&mut self, c1: Point, c2: Point, end: Point) -> bool {
        if self.current.is_none() {
            return false;
        }
        self.segments.push(Segment::CurveTo(c1, c2, end));
        self.current = Some(end);
        true
    }

    /// Closes the current subpath; nothing to do without one.
    pub fn close(&mut self) {
        let Some(start) = self.start else {
            return;
        };
        if !matches!(self.segments.last(), Some(Segment::Close)) {
            self.segments.push(Segment::Close);
        }
        self.current = Some(start);
    }

    pub fn rectangle(&mut self, x: f64, y: f64, width: f64, height: f64) {
        self.move_to(Point::new(x, y));
        self.segments
            .push(Segment::LineTo(Point::new(x + width, y)));
        self.segments
            .push(Segment::LineTo(Point::new(x + width, y + height)));
        self.segments
            .push(Segment::LineTo(Point::new(x, y + height)));
        self.segments.push(Segment::Close);
        self.current = Some(Point::new(x, y));
    }

    /// Every point of the path through `matrix`, padded by `pad`, lies
    /// within `limit` of the device origin (and is therefore finite).
    pub fn is_within(&self, matrix: &Matrix, pad: f64, limit: f64) -> bool {
        self.points().all(|p| {
            let q = matrix.apply(p);
            q.x.abs() + pad <= limit && q.y.abs() + pad <= limit
        })
    }

    fn points(&self) -> impl Iterator<Item = Point> + '_ {
        self.segments.iter().flat_map(|segment| {
            let (points, count): ([Point; 3], usize) = match *segment {
                Segment::MoveTo(p) | Segment::LineTo(p) => ([p, p, p], 1),
                Segment::CurveTo(a, b, c) => ([a, b, c], 3),
                Segment::Close => ([Point::new(0.0, 0.0); 3], 0),
            };
            points.into_iter().take(count)
        })
    }

    /// Estimated rasteriser work to paint this path through `matrix` on a
    /// `width` x `height` canvas, each point widened by `pad` device pixels
    /// (half a stroke width, 0 for a fill or a clip). An upper-bound style
    /// estimate, in units of about one line segment or one tile:
    /// - flattening: one per segment, plus for a curve the square root of
    ///   its control polygon length in device pixels over the flattening
    ///   tolerance (0.25 px);
    /// - strips: each segment crosses about a quarter of its device extent
    ///   (clipped to the canvas) in 4-pixel tiles;
    /// - filling: the clipped bounding box area over 1024 pixels (one wide
    ///   tile command per 256 x 4 pixels), plus its compositing.
    pub fn work(&self, matrix: &Matrix, pad: f64, width: f64, height: f64) -> f64 {
        let clip = |low: f64, high: f64, size: f64| (high.min(size) - low.max(0.0)).max(0.0);
        let mut work = 0.0;
        let mut current = Point::new(0.0, 0.0);
        let mut bounds = Bounds::EMPTY;
        for segment in &self.segments {
            let (points, count): ([Point; 3], usize) = match *segment {
                Segment::MoveTo(p) => {
                    current = matrix.apply(p);
                    work += 1.0;
                    continue;
                }
                Segment::LineTo(p) => ([matrix.apply(p); 3], 1),
                Segment::CurveTo(a, b, c) => {
                    ([matrix.apply(a), matrix.apply(b), matrix.apply(c)], 3)
                }
                Segment::Close => {
                    work += 1.0;
                    continue;
                }
            };
            let mut segment_bounds = Bounds::EMPTY;
            segment_bounds.add(current);
            let mut length = 0.0;
            let mut previous = current;
            for point in points.iter().take(count) {
                segment_bounds.add(*point);
                length += previous.distance(*point);
                previous = *point;
            }
            if count == 3 {
                work += (length * 4.0).sqrt();
            }
            let extent = clip(segment_bounds.x0 - pad, segment_bounds.x1 + pad, width)
                + clip(segment_bounds.y0 - pad, segment_bounds.y1 + pad, height);
            work += 1.0 + extent / 4.0;
            bounds.merge(&segment_bounds);
            current = previous;
        }
        if bounds.x0 <= bounds.x1 {
            let area = clip(bounds.x0 - pad, bounds.x1 + pad, width)
                * clip(bounds.y0 - pad, bounds.y1 + pad, height);
            work += area / 1024.0;
        }
        work
    }

    /// Length of the control polygon in user space: an upper bound of the
    /// arc length, used to bound the number of dashes.
    pub fn polygon_length(&self) -> f64 {
        let mut length = 0.0;
        let mut start = Point::new(0.0, 0.0);
        let mut current = start;
        for segment in &self.segments {
            match *segment {
                Segment::MoveTo(p) => {
                    start = p;
                    current = p;
                }
                Segment::LineTo(p) => {
                    length += current.distance(p);
                    current = p;
                }
                Segment::CurveTo(a, b, c) => {
                    length += current.distance(a) + a.distance(b) + b.distance(c);
                    current = c;
                }
                Segment::Close => {
                    length += current.distance(start);
                    current = start;
                }
            }
        }
        length
    }
}

/// Axis-aligned bounds; empty while `x0 > x1`.
#[derive(Debug, Clone, Copy)]
struct Bounds {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

impl Bounds {
    const EMPTY: Self = Self {
        x0: f64::INFINITY,
        y0: f64::INFINITY,
        x1: f64::NEG_INFINITY,
        y1: f64::NEG_INFINITY,
    };

    fn add(&mut self, p: Point) {
        self.x0 = self.x0.min(p.x);
        self.y0 = self.y0.min(p.y);
        self.x1 = self.x1.max(p.x);
        self.y1 = self.y1.max(p.y);
    }

    fn merge(&mut self, other: &Self) {
        self.x0 = self.x0.min(other.x0);
        self.y0 = self.y0.min(other.y0);
        self.x1 = self.x1.max(other.x1);
        self.y1 = self.y1.max(other.y1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn then_applies_left_first() {
        let scale = Matrix::new(2.0, 0.0, 0.0, 3.0, 0.0, 0.0);
        let shift = Matrix::new(1.0, 0.0, 0.0, 1.0, 10.0, 20.0);
        let p = scale.then(&shift).apply(Point::new(1.0, 1.0));
        assert_eq!(p, Point::new(12.0, 23.0));
        let q = shift.then(&scale).apply(Point::new(1.0, 1.0));
        assert_eq!(q, Point::new(22.0, 63.0));
    }

    #[test]
    fn path_construction_rules() {
        let mut path = Path::default();
        assert!(!path.line_to(Point::new(1.0, 1.0)));
        path.close();
        assert!(path.is_empty());
        path.move_to(Point::new(0.0, 0.0));
        path.move_to(Point::new(5.0, 5.0));
        assert_eq!(path.segments(), &[Segment::MoveTo(Point::new(5.0, 5.0))]);
        assert!(path.line_to(Point::new(6.0, 5.0)));
        path.close();
        path.close();
        assert_eq!(path.segments().len(), 3);
        assert_eq!(path.current_point(), Some(Point::new(5.0, 5.0)));
        path.rectangle(0.0, 0.0, 3.0, 4.0);
        assert_eq!(path.current_point(), Some(Point::new(0.0, 0.0)));
        assert_eq!(path.polygon_length(), 2.0 + 14.0);
    }

    #[test]
    fn work_counts_curves_crossings_and_area() {
        let canvas = (1000.0, 1000.0);
        let work = |path: &Path, pad| path.work(&Matrix::IDENTITY, pad, canvas.0, canvas.1);
        let mut path = Path::default();
        // A line outside the canvas: one move, one segment, nothing covered.
        path.move_to(Point::new(-5000.0, -10.0));
        path.line_to(Point::new(-10.0, -5000.0));
        assert_eq!(work(&path, 0.0), 2.0);
        // A long line across the canvas: its clipped extent in 4-pixel tiles.
        path.clear();
        path.move_to(Point::new(0.0, 0.0));
        path.line_to(Point::new(1e12, 1e12));
        assert_eq!(work(&path, 0.0), 1.0 + 1.0 + 2000.0 / 4.0 + 1e6 / 1024.0);
        // A padded stroke covers more.
        path.clear();
        path.move_to(Point::new(100.0, 100.0));
        path.line_to(Point::new(200.0, 100.0));
        assert!(work(&path, 10.0) > work(&path, 0.0));
        // A huge curve costs the square root of its size.
        path.clear();
        path.move_to(Point::new(0.0, 0.0));
        path.curve_to(
            Point::new(1e12, 1e12),
            Point::new(0.0, 1e12),
            Point::new(0.0, 0.0),
        );
        assert!(work(&path, 0.0) > 1e6);
    }

    #[test]
    fn bounds_are_checked_with_padding() {
        let mut path = Path::default();
        path.move_to(Point::new(0.0, 90.0));
        assert!(path.is_within(&Matrix::IDENTITY, 10.0, 100.0));
        assert!(!path.is_within(&Matrix::IDENTITY, 10.1, 100.0));
        path.line_to(Point::new(f64::NAN, 0.0));
        assert!(!path.is_within(&Matrix::IDENTITY, 0.0, 100.0));
    }
}
