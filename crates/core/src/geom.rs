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
    /// 0 = stays until speech ends.
    pub ttl_ms: u64,
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
