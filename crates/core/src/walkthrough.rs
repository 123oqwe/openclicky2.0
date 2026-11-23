//! Step-by-step guidance: advance on hit, release on miss, never exceed 15.

use crate::annotations::{Point, Rect};
use crate::error::{Error, Result};

pub const MAX_STEPS: usize = 15;
pub const HIT_TOLERANCE: f64 = 8.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub n: u8,
    pub label: String,
    pub target: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Advance {
    Next(Step),
    Completed,
    Released,
    Missed,
}

#[derive(Debug, Clone)]
pub struct Walkthrough {
    steps: Vec<Step>,
    current: usize,
    done: Vec<u8>,
    released: bool,
}

impl Walkthrough {
    pub fn new(mut steps: Vec<Step>) -> Result<Self> {
        if steps.is_empty() {
            return Err(Error::BadRequest("带做没有任何步骤".into()));
        }
        if steps.len() > MAX_STEPS {
            steps.truncate(MAX_STEPS);
        }
        steps.sort_by_key(|s| s.n);
        Ok(Self {
            steps,
            current: 0,
            done: Vec::new(),
            released: false,
        })
    }

    pub fn current(&self) -> Option<&Step> {
        self.steps.get(self.current)
    }

    pub fn is_done(&self) -> bool {
        self.current >= self.steps.len()
    }

    pub fn completed(&self) -> &[u8] {
        &self.done
    }

    pub fn on_click(&mut self, p: Point) -> Advance {
        let Some(step) = self.steps.get(self.current) else {
            return Advance::Completed;
        };
        if step.target.contains(p, HIT_TOLERANCE) {
            self.done.push(step.n);
            self.current += 1;
            self.released = false;
            match self.steps.get(self.current) {
                Some(next) => Advance::Next(next.clone()),
                None => Advance::Completed,
            }
        } else {
            self.released = true;
            Advance::Missed
        }
    }

    /// Moving the mouse after a miss releases the overlay; returns true if a
    /// release was actually needed.
    pub fn on_mouse_move(&mut self) -> bool {
        if self.released {
            self.released = false;
            return true;
        }
        false
    }

    /// A step the user must do by hand; `continue` moves on without a click.
    pub fn on_continue(&mut self) -> Result<Advance> {
        let Some(step) = self.steps.get(self.current) else {
            return Err(Error::State("带做已经结束，没有更多步骤".into()));
        };
        // Never re-enter a step that was already completed.
        if self.done.contains(&step.n) {
            return Err(Error::State("该步骤已完成".into()));
        }
        self.done.push(step.n);
        self.current += 1;
        Ok(match self.steps.get(self.current) {
            Some(next) => Advance::Next(next.clone()),
            None => Advance::Completed,
        })
    }

    pub fn release(&mut self) -> Advance {
        self.released = true;
        Advance::Released
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps() -> Vec<Step> {
        vec![
            Step {
                n: 1,
                label: "open menu".into(),
                target: Rect::new(10.0, 10.0, 40.0, 20.0),
            },
            Step {
                n: 2,
                label: "pick export".into(),
                target: Rect::new(100.0, 60.0, 80.0, 30.0),
            },
        ]
    }

    #[test]
    fn hit_advances_and_miss_releases() {
        let mut w = Walkthrough::new(steps()).unwrap();
        assert_eq!(w.on_click(Point::new(500.0, 500.0)), Advance::Missed);
        assert!(w.on_mouse_move());
        match w.on_click(Point::new(30.0, 20.0)) {
            Advance::Next(s) => assert_eq!(s.n, 2),
            other => panic!("expected next, got {other:?}"),
        }
        assert_eq!(w.on_click(Point::new(120.0, 70.0)), Advance::Completed);
        assert_eq!(w.completed(), &[1, 2]);
    }

    #[test]
    fn click_near_the_edge_still_hits() {
        let mut w = Walkthrough::new(steps()).unwrap();
        // 6px outside the rect but inside the tolerance.
        assert!(matches!(
            w.on_click(Point::new(6.0, 16.0)),
            Advance::Next(_)
        ));
    }

    #[test]
    fn at_most_fifteen_steps() {
        let many: Vec<Step> = (1..=30)
            .map(|n| Step {
                n,
                label: format!("s{n}"),
                target: Rect::new(0.0, 0.0, 10.0, 10.0),
            })
            .collect();
        let w = Walkthrough::new(many).unwrap();
        assert_eq!(w.steps.len(), MAX_STEPS);
    }

    #[test]
    fn continue_moves_on_and_cannot_replay() {
        let mut w = Walkthrough::new(steps()).unwrap();
        assert!(matches!(w.on_continue().unwrap(), Advance::Next(_)));
        assert!(w.on_continue().unwrap() == Advance::Completed);
        assert!(w.on_continue().is_err(), "已结束的带做不能再推进");
    }

    #[test]
    fn empty_walkthrough_is_rejected() {
        assert!(Walkthrough::new(vec![]).is_err());
    }
}
