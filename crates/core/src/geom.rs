//! Geometry and render-plan types shared by the ports and the annotation parser.
//!
//! These used to live in `annotations`, which made `ports` depend on
//! `annotations` while `annotations` depended on `ports` — a module cycle.
//! They now live here, below both.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    pub fn center(&self) -> Point {
        Point::new(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    pub fn contains(&self, p: Point, tolerance: f64) -> bool {
        p.x >= self.x - tolerance
            && p.x <= self.x + self.w + tolerance
            && p.y >= self.y - tolerance
            && p.y <= self.y + self.h + tolerance
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StrokeKind {
    Circle,
    Rect,
    Arrow,
    Line,
    Poly,
    Highlight,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub kind: StrokeKind,
    /// Global logical desktop coordinates, already "hand drawn".
    pub points: Vec<Point>,
    pub closed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointerItem {
    pub screen: usize,
    pub label: Option<String>,
    /// Where the companion flies to (global logical).
    pub target: Option<Point>,
    pub strokes: Vec<Stroke>,
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointerPlan {
    pub plan_id: String,
    pub items: Vec<PointerItem>,
}

impl PointerPlan {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Points recorded while the talk key is held: the user is pointing at
/// something on screen with the mouse (SRC-PRD B4).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Trail {
    pub points: Vec<Point>,
}

impl Trail {
    pub const MIN_POINTS: usize = 3;
    pub const MIN_SIZE: f64 = 8.0;

    pub fn push(&mut self, p: Point) {
        if let Some(last) = self.points.last() {
            if (last.x - p.x).abs() < 2.0 && (last.y - p.y).abs() < 2.0 {
                return;
            }
        }
        self.points.push(p);
    }

    pub fn bbox(&self) -> Option<Rect> {
        let first = *self.points.first()?;
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (first.x, first.y, first.x, first.y);
        for p in &self.points {
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
        }
        Some(Rect {
            x: min_x,
            y: min_y,
            w: max_x - min_x,
            h: max_y - min_y,
        })
    }

    /// A dot or a jitter is not a gesture; require a few points and some extent.
    pub fn is_meaningful(&self) -> bool {
        if self.points.len() < Self::MIN_POINTS {
            return false;
        }
        match self.bbox() {
            Some(r) => r.w.max(r.h) >= Self::MIN_SIZE,
            None => false,
        }
    }
}

#[cfg(test)]
mod trail_tests {
    use super::*;

    fn trail(pts: &[(f64, f64)]) -> Trail {
        let mut t = Trail::default();
        for (x, y) in pts {
            t.push(Point { x: *x, y: *y });
        }
        t
    }

    #[test]
    fn bbox_covers_all_points() {
        let t = trail(&[(10.0, 20.0), (60.0, 20.0), (60.0, 90.0)]);
        let r = t.bbox().unwrap();
        assert_eq!(
            r,
            Rect {
                x: 10.0,
                y: 20.0,
                w: 50.0,
                h: 70.0
            }
        );
    }

    #[test]
    fn tiny_movements_are_ignored() {
        let t = trail(&[(0.0, 0.0), (0.5, 0.5), (1.0, 1.0)]);
        assert_eq!(t.points.len(), 1, "dead zone collapses jitter");
        assert!(!t.is_meaningful());
    }

    #[test]
    fn a_circle_is_meaningful() {
        let mut t = Trail::default();
        for i in 0..20 {
            let a = i as f64 / 20.0 * std::f64::consts::TAU;
            t.push(Point {
                x: 100.0 + 40.0 * a.cos(),
                y: 100.0 + 40.0 * a.sin(),
            });
        }
        assert!(t.is_meaningful());
        let r = t.bbox().unwrap();
        assert!(r.w > 70.0 && r.h > 70.0);
    }
}
