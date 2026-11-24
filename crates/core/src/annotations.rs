//! On-screen pointing and drawing.
//!
//! The model speaks plain text with inline tags. We parse the tags, convert
//! screenshot-pixel coordinates into global logical desktop coordinates, and
//! produce a render plan. Rendering itself happens in the UI shell; this module
//! only produces numbers, which is what makes "does it point at the right
//! place" testable.
//!
//! Grammar (see `assets/annotation_grammar.txt`):
//! ```text
//! [POINT:x,y:label:screenN]      [POINT:none]
//! [DRAW:screenN:circle(cx,cy,r)] [DRAW:screenN:rect(x,y,w,h)]
//! [DRAW:screenN:arrow(x1,y1,x2,y2)] [DRAW:screenN:line(x1,y1,x2,y2)]
//! [DRAW:screenN:poly(x1,y1;x2,y2;...)] [DRAW:screenN:text(x,y,body)]
//! [STEP:n:label:rect(x,y,w,h)]
//! ```

use crate::ports::DisplayInfo;

pub const MAX_LABEL_CHARS: usize = 40;
pub const MAX_ANNOTATIONS: usize = 40;

pub use crate::geom::{Point, PointerItem, PointerPlan, Rect, Stroke, StrokeKind};

#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Circle { cx: f64, cy: f64, r: f64 },
    Rect { x: f64, y: f64, w: f64, h: f64 },
    Arrow { x1: f64, y1: f64, x2: f64, y2: f64 },
    Line { x1: f64, y1: f64, x2: f64, y2: f64 },
    Poly { points: Vec<(f64, f64)> },
    Text { x: f64, y: f64, body: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum AnnotationKind {
    Point { x_px: f64, y_px: f64, label: String },
    Draw(Shape),
    Step { n: u8, label: String, shape: Shape },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    pub screen: usize,
    pub kind: AnnotationKind,
    /// Filled for Point/Step after conversion.
    pub global: Option<Point>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StepSpec {
    pub n: u8,
    pub label: String,
    pub target: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DroppedTag {
    pub raw: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedReply {
    pub plain_text: String,
    pub annotations: Vec<Annotation>,
    pub steps: Vec<StepSpec>,
    pub dropped: Vec<DroppedTag>,
    pub points_to_none: bool,
}

/// Map a screenshot pixel coordinate to the global logical desktop.
pub fn to_global(display: &DisplayInfo, x_px: f64, y_px: f64) -> Point {
    let scale = if display.scale > 0.0 {
        display.scale
    } else {
        1.0
    };
    Point::new(
        display.origin_x + x_px / scale,
        display.origin_y + y_px / scale,
    )
}

fn clamp_px(v: f64, max: f64) -> f64 {
    if v.is_nan() {
        return 0.0;
    }
    v.clamp(0.0, max.max(0.0))
}

fn sanitize_label(label: &str) -> String {
    let cleaned: String = label
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_string();
    cleaned.chars().take(MAX_LABEL_CHARS).collect()
}

fn parse_f64(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

fn split_shape(body: &str) -> Option<(&str, &str)> {
    let open = body.find('(')?;
    if !body.ends_with(')') {
        return None;
    }
    Some((body[..open].trim(), body[open + 1..body.len() - 1].trim()))
}

fn parse_shape(name: &str, args: &str) -> Option<Shape> {
    let nums: Vec<&str> = args.split(',').map(|s| s.trim()).collect();
    let n = |i: usize| -> Option<f64> { nums.get(i).and_then(|s| parse_f64(s)) };
    match name.to_ascii_lowercase().as_str() {
        "circle" => Some(Shape::Circle {
            cx: n(0)?,
            cy: n(1)?,
            r: n(2)?.abs(),
        }),
        "rect" => Some(Shape::Rect {
            x: n(0)?,
            y: n(1)?,
            w: n(2)?.abs(),
            h: n(3)?.abs(),
        }),
        "arrow" => Some(Shape::Arrow {
            x1: n(0)?,
            y1: n(1)?,
            x2: n(2)?,
            y2: n(3)?,
        }),
        "line" => Some(Shape::Line {
            x1: n(0)?,
            y1: n(1)?,
            x2: n(2)?,
            y2: n(3)?,
        }),
        "poly" => {
            let mut points = Vec::new();
            for pair in args.split(';') {
                let mut it = pair.split(',');
                let x = parse_f64(it.next()?)?;
                let y = parse_f64(it.next()?)?;
                points.push((x, y));
            }
            if points.len() < 2 {
                None
            } else {
                Some(Shape::Poly { points })
            }
        }
        "text" => {
            let open = args.find(',')?;
            let rest = &args[open + 1..];
            let comma = rest.find(',')?;
            let x = parse_f64(&args[..open])?;
            let y = parse_f64(&rest[..comma])?;
            Some(Shape::Text {
                x,
                y,
                body: sanitize_label(&rest[comma + 1..]),
            })
        }
        _ => None,
    }
}

fn shape_bounds_px(shape: &Shape) -> (f64, f64) {
    match shape {
        Shape::Circle { cx, cy, r } => (cx + r, cy + r),
        Shape::Rect { x, y, w, h } => (x + w, y + h),
        Shape::Arrow { x1, y1, x2, y2 } | Shape::Line { x1, y1, x2, y2 } => {
            (x1.max(*x2), y1.max(*y2))
        }
        Shape::Poly { points } => points
            .iter()
            .fold((0.0, 0.0), |acc, p| (acc.0.max(p.0), acc.1.max(p.1))),
        Shape::Text { x, y, .. } => (*x, *y),
    }
}

/// Split a raw model reply into speakable text and annotation tags.
pub fn parse_reply(raw: &str, displays: &[DisplayInfo], cursor_display: usize) -> ParsedReply {
    let mut out = ParsedReply::default();
    let mut plain = String::with_capacity(raw.len());
    let mut rest = raw;

    while let Some(start) = rest.find('[') {
        plain.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find(']') else {
            plain.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let tag = &after[..end];
        let consumed = &rest[start..start + 1 + end + 1];
        // Multi-line tags are never valid; drop the bracket to keep text.
        if tag.contains('\n') || tag.len() > 400 {
            plain.push_str(consumed);
            rest = &after[end + 1..];
            continue;
        }

        match parse_tag(tag, displays, cursor_display) {
            TagResult::Point(None) => {
                out.points_to_none = true;
            }
            TagResult::Point(Some(ann)) => {
                if out.annotations.len() < MAX_ANNOTATIONS {
                    out.annotations.push(ann);
                }
            }
            TagResult::Step(step) => {
                if out.steps.len() < 15 {
                    out.steps.push(step);
                }
            }
            TagResult::Unknown => plain.push_str(consumed),
            TagResult::Invalid(reason) => out.dropped.push(DroppedTag {
                raw: sanitize_label(tag),
                reason,
            }),
        }
        rest = &after[end + 1..];
    }
    plain.push_str(rest);

    out.plain_text = normalize_whitespace(&plain);
    out
}

enum TagResult {
    Point(Option<Annotation>),
    Step(StepSpec),
    Unknown,
    Invalid(String),
}

fn resolve_screen(token: Option<&str>, displays: &[DisplayInfo], cursor: usize) -> (usize, bool) {
    let Some(token) = token else {
        return (cursor, true);
    };
    let idx: Option<usize> = token
        .strip_prefix("screen")
        .and_then(|n| n.trim().parse::<usize>().ok())
        .or_else(|| token.parse::<usize>().ok());
    match idx {
        Some(i) if i < displays.len() => (i, false),
        _ => (cursor.min(displays.len().saturating_sub(1)), true),
    }
}

fn parse_tag(tag: &str, displays: &[DisplayInfo], cursor: usize) -> TagResult {
    let tag = tag.trim();
    let upper = tag.to_ascii_uppercase();

    if upper.starts_with("POINT:") {
        let body = &tag[6..];
        if body.trim().eq_ignore_ascii_case("none") {
            return TagResult::Point(None);
        }
        let parts: Vec<&str> = body.split(':').collect();
        let Some(coords) = parts.first() else {
            return TagResult::Invalid("missing coordinates".into());
        };
        let mut xy = coords.split(',');
        let (Some(x), Some(y)) = (xy.next().and_then(parse_f64), xy.next().and_then(parse_f64))
        else {
            return TagResult::Invalid("bad coordinates".into());
        };
        let label = parts.get(1).map(|s| sanitize_label(s)).unwrap_or_default();
        let (screen, _fell_back) = resolve_screen(parts.get(2).copied(), displays, cursor);
        if let Some(d) = displays.get(screen) {
            let x = clamp_px(x, d.width_px() as f64);
            let y = clamp_px(y, d.height_px() as f64);
            let ann = Annotation {
                screen,
                kind: AnnotationKind::Point {
                    x_px: x,
                    y_px: y,
                    label,
                },
                global: Some(to_global(d, x, y)),
            };
            return TagResult::Point(Some(ann));
        }
        return TagResult::Invalid("no display".into());
    }

    if upper.starts_with("DRAW:") {
        let body = &tag[5..];
        let parts: Vec<&str> = body.splitn(2, ':').collect();
        if parts.len() != 2 {
            return TagResult::Invalid("draw needs screen and shape".into());
        }
        let (screen, _) = resolve_screen(Some(parts[0]), displays, cursor);
        let Some((name, args)) = split_shape(parts[1]) else {
            return TagResult::Invalid("bad shape".into());
        };
        let Some(shape) = parse_shape(name, args) else {
            return TagResult::Invalid("unknown shape".into());
        };
        return TagResult::Point(Some(Annotation {
            screen,
            kind: AnnotationKind::Draw(shape),
            global: None,
        }));
    }

    if upper.starts_with("STEP:") {
        let body = &tag[5..];
        let parts: Vec<&str> = body.splitn(3, ':').collect();
        if parts.len() != 3 {
            return TagResult::Invalid("step needs n, label, shape".into());
        }
        let Ok(n) = parts[0].trim().parse::<u8>() else {
            return TagResult::Invalid("bad step number".into());
        };
        let label = sanitize_label(parts[1]);
        let Some((name, args)) = split_shape(parts[2]) else {
            return TagResult::Invalid("bad step shape".into());
        };
        let Some(shape) = parse_shape(name, args) else {
            return TagResult::Invalid("unknown step shape".into());
        };
        let (screen, _) = resolve_screen(None, displays, cursor);
        if let Some(d) = displays.get(screen) {
            let (mx, my) = shape_bounds_px(&shape);
            let origin = match &shape {
                Shape::Rect { x, y, .. } => (*x, *y),
                Shape::Circle { cx, cy, r } => (cx - r, cy - r),
                _ => (0.0, 0.0),
            };
            let tl = to_global(
                d,
                clamp_px(origin.0, d.width_px() as f64),
                clamp_px(origin.1, d.height_px() as f64),
            );
            let br = to_global(
                d,
                clamp_px(mx, d.width_px() as f64),
                clamp_px(my, d.height_px() as f64),
            );
            let target = Rect::new(
                tl.x,
                tl.y,
                (br.x - tl.x).abs().max(4.0),
                (br.y - tl.y).abs().max(4.0),
            );
            return TagResult::Step(StepSpec { n, label, target });
        }
        return TagResult::Invalid("no display".into());
    }

    TagResult::Unknown
}

fn normalize_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !last_space {
                out.push(' ');
            }
            last_space = true;
        } else {
            out.push(c);
            last_space = false;
        }
    }
    out.trim().to_string()
}

/// Plain speakable text: every known tag removed, whitespace normalised.
pub fn strip_tags(raw: &str) -> String {
    let mut plain = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find('[') {
        plain.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find(']') else {
            plain.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let tag = &after[..end];
        let up = tag.trim().to_ascii_uppercase();
        let known = up.starts_with("POINT:") || up.starts_with("DRAW:") || up.starts_with("STEP:");
        if !known {
            plain.push('[');
            plain.push_str(tag);
            plain.push(']');
        }
        rest = &after[end + 1..];
    }
    plain.push_str(rest);
    normalize_whitespace(&plain)
}

// ------------------------------------------------------------- render plan

struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }
    fn next_f(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as f64) / ((1u64 << 31) as f64)
    }
    fn jitter(&mut self, amount: f64) -> f64 {
        (self.next_f() - 0.5) * 2.0 * amount
    }
}

fn hash_seed(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

fn arc_points(
    center: Point,
    radius: f64,
    seed: &mut Lcg,
    steps: usize,
    overshoot: f64,
) -> Vec<Point> {
    let start = seed.next_f() * std::f64::consts::TAU;
    let sweep = std::f64::consts::TAU + overshoot;
    (0..=steps)
        .map(|i| {
            let t = i as f64 / steps as f64;
            let a = start + sweep * t;
            let r = radius + seed.jitter(radius * 0.06);
            Point::new(center.x + r * a.cos(), center.y + r * a.sin())
        })
        .collect()
}

fn rect_loop(r: Rect, jitter: f64, seed: &mut Lcg) -> Vec<Point> {
    let corners = [
        Point::new(r.x, r.y),
        Point::new(r.x + r.w, r.y),
        Point::new(r.x + r.w, r.y + r.h),
        Point::new(r.x, r.y + r.h),
        Point::new(r.x, r.y),
    ];
    let mut pts = Vec::new();
    for w in corners.windows(2) {
        let (a, b) = (w[0], w[1]);
        for i in 0..=6 {
            let t = i as f64 / 6.0;
            pts.push(Point::new(
                a.x + (b.x - a.x) * t + seed.jitter(jitter),
                a.y + (b.y - a.y) * t + seed.jitter(jitter),
            ));
        }
    }
    pts
}

fn arrow_points(from: Point, to: Point, seed: &mut Lcg) -> Vec<Point> {
    let mut pts = Vec::new();
    let ctrl = Point::new(
        (from.x + to.x) / 2.0 + seed.jitter(24.0),
        (from.y + to.y) / 2.0 + seed.jitter(24.0),
    );
    for i in 0..=16 {
        let t = i as f64 / 16.0;
        let it = 1.0 - t;
        pts.push(Point::new(
            it * it * from.x + 2.0 * it * t * ctrl.x + t * t * to.x,
            it * it * from.y + 2.0 * it * t * ctrl.y + t * t * to.y,
        ));
    }
    let ang = (to.y - pts[pts.len() - 2].y).atan2(to.x - pts[pts.len() - 2].x);
    for side in [-1.0f64, 1.0] {
        let a = ang + side * 0.5;
        pts.push(Point::new(to.x - 12.0 * a.cos(), to.y - 12.0 * a.sin()));
        pts.push(to);
    }
    pts
}

/// Convert parsed annotations into a render plan in global logical coordinates.
pub fn plan_from(
    parsed: &ParsedReply,
    displays: &[DisplayInfo],
    plan_id: &str,
) -> Option<PointerPlan> {
    if parsed.annotations.is_empty() && parsed.steps.is_empty() {
        return None;
    }
    let mut seed = Lcg::new(hash_seed(plan_id));
    let mut items = Vec::new();

    for ann in &parsed.annotations {
        let Some(d) = displays.get(ann.screen) else {
            continue;
        };
        match &ann.kind {
            AnnotationKind::Point { x_px, y_px, label } => {
                let target = to_global(d, *x_px, *y_px);
                let r = 18.0;
                let strokes = vec![Stroke {
                    kind: StrokeKind::Circle,
                    points: arc_points(target, r, &mut seed, 18, 0.35),
                    closed: false,
                }];
                items.push(PointerItem {
                    screen: ann.screen,
                    label: if label.is_empty() {
                        None
                    } else {
                        Some(label.clone())
                    },
                    target: Some(target),
                    strokes,
                    text: None,
                    ttl_ms: 2500,
                });
            }
            AnnotationKind::Draw(shape) => {
                let (strokes, text, ttl) = shape_to_strokes(shape, d, &mut seed);
                if strokes.is_empty() && text.is_none() {
                    continue;
                }
                items.push(PointerItem {
                    screen: ann.screen,
                    label: None,
                    target: None,
                    strokes,
                    text,
                    ttl_ms: ttl,
                });
            }
            AnnotationKind::Step { n: _, label, shape } => {
                let (strokes, text, ttl) = shape_to_strokes(shape, d, &mut seed);
                items.push(PointerItem {
                    screen: ann.screen,
                    label: Some(label.clone()),
                    target: Some(shape_focus(shape, d)),
                    strokes,
                    text,
                    ttl_ms: ttl,
                });
            }
        }
    }

    if items.is_empty() {
        None
    } else if items.len() > MAX_ANNOTATIONS {
        items.truncate(MAX_ANNOTATIONS);
        Some(PointerPlan {
            plan_id: plan_id.to_string(),
            items,
        })
    } else {
        Some(PointerPlan {
            plan_id: plan_id.to_string(),
            items,
        })
    }
}

fn shape_focus(shape: &Shape, d: &DisplayInfo) -> Point {
    match shape {
        Shape::Rect { x, y, w, h } => to_global(d, x + w / 2.0, y + h / 2.0),
        Shape::Circle { cx, cy, .. } => to_global(d, *cx, *cy),
        Shape::Arrow { x2, y2, .. } | Shape::Line { x2, y2, .. } => to_global(d, *x2, *y2),
        Shape::Poly { points } => {
            let p = points.first().copied().unwrap_or((0.0, 0.0));
            to_global(d, p.0, p.1)
        }
        Shape::Text { x, y, .. } => to_global(d, *x, *y),
    }
}

fn shape_to_strokes(
    shape: &Shape,
    d: &DisplayInfo,
    seed: &mut Lcg,
) -> (Vec<Stroke>, Option<String>, u64) {
    match shape {
        Shape::Circle { cx, cy, r } => {
            let c = to_global(d, *cx, *cy);
            let rr = r / d.scale.max(0.01);
            (
                vec![Stroke {
                    kind: StrokeKind::Circle,
                    points: arc_points(c, rr, seed, 22, 0.2),
                    closed: false,
                }],
                None,
                0,
            )
        }
        Shape::Rect { x, y, w, h } => {
            let tl = to_global(d, *x, *y);
            let br = to_global(d, x + w, y + h);
            let rect = Rect::new(tl.x, tl.y, (br.x - tl.x).abs(), (br.y - tl.y).abs());
            (
                vec![Stroke {
                    kind: StrokeKind::Highlight,
                    points: rect_loop(rect, 2.0, seed),
                    closed: true,
                }],
                None,
                2000,
            )
        }
        Shape::Arrow { x1, y1, x2, y2 } => {
            let a = to_global(d, *x1, *y1);
            let b = to_global(d, *x2, *y2);
            (
                vec![Stroke {
                    kind: StrokeKind::Arrow,
                    points: arrow_points(a, b, seed),
                    closed: false,
                }],
                None,
                0,
            )
        }
        Shape::Line { x1, y1, x2, y2 } => {
            let a = to_global(d, *x1, *y1);
            let b = to_global(d, *x2, *y2);
            let mut pts = Vec::new();
            for i in 0..=10 {
                let t = i as f64 / 10.0;
                pts.push(Point::new(
                    a.x + (b.x - a.x) * t + seed.jitter(1.5),
                    a.y + (b.y - a.y) * t + seed.jitter(1.5),
                ));
            }
            (
                vec![Stroke {
                    kind: StrokeKind::Line,
                    points: pts,
                    closed: false,
                }],
                None,
                0,
            )
        }
        Shape::Poly { points } => {
            let pts: Vec<Point> = points.iter().map(|(x, y)| to_global(d, *x, *y)).collect();
            (
                vec![Stroke {
                    kind: StrokeKind::Poly,
                    points: pts,
                    closed: true,
                }],
                None,
                0,
            )
        }
        Shape::Text { x, y, body } => {
            let _anchor = to_global(d, *x, *y);
            (Vec::new(), Some(body.clone()), 0)
        }
    }
}

/// Reposition already-drawn strokes after a page scroll.
/// `anchors` maps stroke index -> anchored shape; only strokes tied to page
/// content move, floating cards and text do not (SRC-PRD D4).
pub fn reproject(strokes: &mut [Stroke], delta: Point, anchored: &[bool]) {
    for (i, stroke) in strokes.iter_mut().enumerate() {
        if anchored.get(i).copied().unwrap_or(false) {
            for p in &mut stroke.points {
                p.x += delta.x;
                p.y += delta.y;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::DisplayInfo;

    fn displays() -> Vec<DisplayInfo> {
        vec![
            DisplayInfo {
                index: 0,
                id: 1,
                name: "built-in".into(),
                origin_x: 0.0,
                origin_y: 0.0,
                width: 1440.0,
                height: 900.0,
                scale: 2.0,
                is_primary: true,
                is_cursor_display: true,
            },
            DisplayInfo {
                index: 1,
                id: 2,
                name: "external".into(),
                origin_x: -1920.0,
                origin_y: 0.0,
                width: 1920.0,
                height: 1080.0,
                scale: 1.0,
                is_primary: false,
                is_cursor_display: false,
            },
        ]
    }

    #[test]
    fn parses_point_with_label_and_screen() {
        let d = displays();
        let p = parse_reply("看这里 [POINT:200,100:save:1]", &d, 0);
        assert_eq!(p.plain_text, "看这里");
        match &p.annotations[0].kind {
            AnnotationKind::Point { x_px, y_px, label } => {
                assert_eq!(*x_px, 200.0);
                assert_eq!(*y_px, 100.0);
                assert_eq!(label, "save");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(p.annotations[0].screen, 1);
    }

    #[test]
    fn point_none_is_recorded_and_silent() {
        let d = displays();
        let p = parse_reply("不确定 [POINT:none]", &d, 0);
        assert!(p.points_to_none);
        assert!(p.annotations.is_empty());
        assert_eq!(p.plain_text, "不确定");
    }

    #[test]
    fn malformed_tags_are_dropped_not_spoken() {
        let d = displays();
        let p = parse_reply("bad [POINT:abc,def:label:0] end", &d, 0);
        assert!(p.annotations.is_empty());
        assert_eq!(p.dropped.len(), 1);
        assert!(!p.plain_text.contains("POINT"));
    }

    #[test]
    fn out_of_range_coordinates_are_clipped() {
        let d = displays();
        let p = parse_reply("[POINT:99999,99999:edge:0]", &d, 0);
        let AnnotationKind::Point { x_px, y_px, .. } = &p.annotations[0].kind else {
            panic!("expected point");
        };
        assert_eq!(*x_px, d[0].width_px() as f64);
        assert_eq!(*y_px, d[0].height_px() as f64);
    }

    #[test]
    fn unknown_screen_falls_back_to_cursor_display() {
        let d = displays();
        let p = parse_reply("[POINT:100,100:here:9]", &d, 1);
        assert_eq!(p.annotations[0].screen, 1);
    }

    #[test]
    fn parses_every_draw_shape() {
        let d = displays();
        let raw = "[DRAW:0:circle(100,100,50)] [DRAW:0:rect(10,20,30,40)] [DRAW:0:arrow(1,2,3,4)] [DRAW:0:line(5,6,7,8)] [DRAW:0:poly(0,0;10,0;10,10)] [DRAW:0:text(20,30,note)]";
        let p = parse_reply(raw, &d, 0);
        assert_eq!(p.annotations.len(), 6);
        assert!(p
            .annotations
            .iter()
            .all(|a| matches!(a.kind, AnnotationKind::Draw(_))));
        assert_eq!(p.plain_text, "");
    }

    #[test]
    fn parses_step_with_target_rect() {
        let d = displays();
        let p = parse_reply("first [STEP:1:open menu:rect(100,200,80,40)]", &d, 0);
        assert_eq!(p.steps.len(), 1);
        let s = &p.steps[0];
        assert_eq!(s.n, 1);
        assert_eq!(s.label, "open menu");
        assert!((s.target.x - 50.0).abs() < 0.6, "x {}", s.target.x);
        assert!((s.target.w - 40.0).abs() < 0.6, "w {}", s.target.w);
    }

    #[test]
    fn strip_tags_keeps_unknown_brackets() {
        let out = strip_tags("see [note] and [POINT:1,2:x:0]");
        assert!(out.contains("[note]"), "{out}");
        assert!(!out.contains("POINT"), "{out}");
    }

    #[test]
    fn plan_from_builds_strokes_and_respects_cap() {
        let d = displays();
        let mut raw = String::with_capacity(0);
        for i in 0..MAX_ANNOTATIONS + 5 {
            raw.push_str(&format!("[POINT:{},{i}:p:0]", 10 + i));
        }
        let p = parse_reply(&raw, &d, 0);
        let plan = plan_from(&p, &d, "plan-1").unwrap();
        assert!(plan.items.len() <= MAX_ANNOTATIONS);
        assert!(!plan.items[0].strokes.is_empty());
    }

    #[test]
    fn reproject_only_moves_anchored_strokes() {
        let mut strokes = vec![
            Stroke {
                kind: StrokeKind::Rect,
                points: vec![Point { x: 0.0, y: 0.0 }],
                closed: true,
            },
            Stroke {
                kind: StrokeKind::Rect,
                points: vec![Point { x: 10.0, y: 10.0 }],
                closed: true,
            },
        ];
        reproject(&mut strokes, Point { x: 5.0, y: 7.0 }, &[true, false]);
        assert_eq!(strokes[0].points[0], Point { x: 5.0, y: 7.0 });
        assert_eq!(strokes[1].points[0], Point { x: 10.0, y: 10.0 });
    }
}
