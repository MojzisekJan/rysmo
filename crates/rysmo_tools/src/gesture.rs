//! Touch gestures over the 3D view, independent of any windowing library.
//!
//! The app feeds raw touches in and gets gestures out:
//!
//! - a tap is a click; two taps close together are a double click;
//! - a long press (half a second without moving) is a right click, for the context menu;
//! - one finger dragging is a drag, which the app turns into orbiting or into tool input
//!   depending on the tool and the on-screen modifier toggles;
//! - two fingers pan and pinch-zoom. A second finger during a drag cancels the drag;
//! - two or three fingers tapping together, without moving, are a multi-finger tap
//!   (undo and redo in the app).
//!
//! The design follows the touch layer of Heron Brush (`crates/heron-brush/src/touch.rs`).

use std::collections::BTreeMap;

use rysmo_math::DVec2;

/// How far a finger may wander and still be a tap or a long press, in pixels.
pub const SLOP: f64 = 10.0;
/// How long a still finger takes to become a long press, in seconds.
pub const LONG_PRESS: f64 = 0.5;
/// The longest gap between two taps of a double tap, in seconds.
pub const DOUBLE_TAP: f64 = 0.35;
/// The longest a multi-finger tap may last, from the first finger down to the last up.
pub const MULTI_TAP: f64 = 0.35;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Start,
    Move,
    End,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gesture {
    /// A tap, with how many taps in a row it completes (1, 2, 3).
    Tap {
        pos: DVec2,
        count: u8,
    },
    LongPress(DVec2),
    DragStart(DVec2),
    DragMove {
        pos: DVec2,
        delta: DVec2,
    },
    DragEnd(DVec2),
    /// The drag ended in a way that should not commit (a second finger came down).
    DragCancel,
    /// Two fingers moved: pan by `pan` pixels and zoom by `zoom` (below 1 is closer)
    /// about `centre`.
    PanZoom {
        pan: DVec2,
        zoom: f64,
        centre: DVec2,
    },
    /// Several fingers tapped together without moving.
    MultiTap {
        fingers: u8,
    },
}

#[derive(Clone, Copy, Debug)]
struct Finger {
    start: DVec2,
    pos: DVec2,
    start_time: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    Idle,
    Pending,
    Dragging,
    LongPressed,
    Two {
        centre: DVec2,
        dist: f64,
    },
    /// Wait for every finger to lift.
    Done,
}

#[derive(Clone, Debug)]
pub struct Gestures {
    fingers: BTreeMap<u64, Finger>,
    state: State,
    last_tap: Option<(f64, DVec2, u8)>,
    /// The touch sequence since the first finger went down: when, how many fingers at
    /// most, and whether any finger left its slop.
    began: f64,
    most: usize,
    moved: bool,
}

impl Default for Gestures {
    fn default() -> Self {
        Gestures { fingers: BTreeMap::new(), state: State::Idle, last_tap: None, began: 0.0, most: 0, moved: false }
    }
}

impl Gestures {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a finger is down.
    pub fn active(&self) -> bool {
        !self.fingers.is_empty()
    }

    /// Feeds one touch event. `time` is in seconds.
    pub fn touch(&mut self, id: u64, phase: Phase, pos: DVec2, time: f64) -> Vec<Gesture> {
        let mut out = Vec::new();
        match phase {
            Phase::Start => {
                if self.fingers.is_empty() {
                    (self.began, self.most, self.moved) = (time, 0, false);
                }
                self.fingers.insert(id, Finger { start: pos, pos, start_time: time });
                self.most = self.most.max(self.fingers.len());
                match (self.fingers.len(), self.state) {
                    (1, State::Idle) => self.state = State::Pending,
                    (2, State::Pending | State::Dragging | State::LongPressed) => {
                        if self.state == State::Dragging {
                            out.push(Gesture::DragCancel);
                        }
                        let (centre, dist) = self.pair();
                        self.state = State::Two { centre, dist };
                    }
                    _ => {}
                }
            }
            Phase::Move => {
                let Some(f) = self.fingers.get_mut(&id) else { return out };
                let prev = f.pos;
                f.pos = pos;
                let start = f.start;
                self.moved |= pos.distance(start) > SLOP;
                match self.state {
                    State::Pending if pos.distance(start) > SLOP => {
                        self.state = State::Dragging;
                        out.push(Gesture::DragStart(start));
                        out.push(Gesture::DragMove { pos, delta: pos - start });
                    }
                    State::Dragging => out.push(Gesture::DragMove { pos, delta: pos - prev }),
                    State::Two { centre, dist } if self.fingers.len() >= 2 => {
                        let (c, d) = self.pair();
                        let zoom = if d > 1.0 && dist > 1.0 { dist / d } else { 1.0 };
                        out.push(Gesture::PanZoom { pan: c - centre, zoom, centre: c });
                        self.state = State::Two { centre: c, dist: d };
                    }
                    _ => {}
                }
            }
            Phase::End | Phase::Cancel => {
                let Some(mut f) = self.fingers.remove(&id) else { return out };
                f.pos = pos;
                match self.state {
                    State::Pending if phase == Phase::End => {
                        let count = match self.last_tap {
                            Some((t, p, n)) if time - t < DOUBLE_TAP && p.distance(f.pos) < SLOP * 2.0 => {
                                (n + 1).min(3)
                            }
                            _ => 1,
                        };
                        self.last_tap = Some((time, f.pos, count));
                        out.push(Gesture::Tap { pos: f.pos, count });
                    }
                    State::Dragging if phase == Phase::End => out.push(Gesture::DragEnd(f.pos)),
                    State::Dragging => out.push(Gesture::DragCancel),
                    _ => {}
                }
                self.moved |= f.pos.distance(f.start) > SLOP;
                if self.fingers.is_empty()
                    && phase == Phase::End
                    && self.most >= 2
                    && !self.moved
                    && time - self.began <= MULTI_TAP
                {
                    out.push(Gesture::MultiTap { fingers: self.most.min(u8::MAX as usize) as u8 });
                }
                self.state = if self.fingers.is_empty() { State::Idle } else { State::Done };
            }
        }
        out
    }

    /// Call every frame: turns a finger that stayed still long enough into a long press.
    pub fn update(&mut self, time: f64) -> Vec<Gesture> {
        if self.state != State::Pending {
            return Vec::new();
        }
        let Some(f) = self.fingers.values().next() else { return Vec::new() };
        if time - f.start_time >= LONG_PRESS && f.pos.distance(f.start) <= SLOP {
            self.state = State::LongPressed;
            return vec![Gesture::LongPress(f.pos)];
        }
        Vec::new()
    }

    fn pair(&self) -> (DVec2, f64) {
        let mut it = self.fingers.values();
        let a = it.next().map(|f| f.pos).unwrap_or_default();
        let b = it.next().map(|f| f.pos).unwrap_or(a);
        ((a + b) * 0.5, a.distance(b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    #[test]
    fn tap_and_double_tap() {
        let mut g = Gestures::new();
        assert!(g.touch(1, Phase::Start, p(100.0, 100.0), 0.0).is_empty());
        assert_eq!(g.touch(1, Phase::End, p(102.0, 101.0), 0.1), vec![Gesture::Tap { pos: p(102.0, 101.0), count: 1 }]);
        g.touch(2, Phase::Start, p(103.0, 100.0), 0.25);
        assert_eq!(g.touch(2, Phase::End, p(103.0, 100.0), 0.3), vec![Gesture::Tap { pos: p(103.0, 100.0), count: 2 }]);
        // Too late for a third.
        g.touch(3, Phase::Start, p(103.0, 100.0), 1.0);
        assert_eq!(
            g.touch(3, Phase::End, p(103.0, 100.0), 1.05),
            vec![Gesture::Tap { pos: p(103.0, 100.0), count: 1 }]
        );
    }

    #[test]
    fn two_and_three_finger_taps() {
        let mut g = Gestures::new();
        g.touch(1, Phase::Start, p(100.0, 100.0), 0.0);
        g.touch(2, Phase::Start, p(200.0, 100.0), 0.02);
        assert!(g.touch(1, Phase::End, p(101.0, 100.0), 0.12).is_empty());
        assert_eq!(g.touch(2, Phase::End, p(200.0, 101.0), 0.15), vec![Gesture::MultiTap { fingers: 2 }]);
        for (id, x) in [(1, 100.0), (2, 200.0), (3, 300.0)] {
            g.touch(id, Phase::Start, p(x, 100.0), 1.0);
        }
        for id in 1..=2 {
            assert!(g.touch(id, Phase::End, p(100.0 * id as f64, 100.0), 1.1).is_empty());
        }
        assert_eq!(g.touch(3, Phase::End, p(300.0, 100.0), 1.12), vec![Gesture::MultiTap { fingers: 3 }]);
    }

    #[test]
    fn a_pinch_or_a_slow_touch_is_not_a_multi_finger_tap() {
        let mut g = Gestures::new();
        g.touch(1, Phase::Start, p(100.0, 100.0), 0.0);
        g.touch(2, Phase::Start, p(200.0, 100.0), 0.0);
        g.touch(2, Phase::Move, p(260.0, 100.0), 0.05);
        g.touch(1, Phase::End, p(100.0, 100.0), 0.1);
        assert!(!g.touch(2, Phase::End, p(260.0, 100.0), 0.12).contains(&Gesture::MultiTap { fingers: 2 }));
        g.touch(1, Phase::Start, p(100.0, 100.0), 1.0);
        g.touch(2, Phase::Start, p(200.0, 100.0), 1.0);
        g.touch(1, Phase::End, p(100.0, 100.0), 1.6);
        assert!(g.touch(2, Phase::End, p(200.0, 100.0), 1.6).is_empty());
    }

    #[test]
    fn long_press() {
        let mut g = Gestures::new();
        g.touch(1, Phase::Start, p(50.0, 50.0), 0.0);
        assert!(g.update(0.3).is_empty());
        assert_eq!(g.update(0.6), vec![Gesture::LongPress(p(50.0, 50.0))]);
        // Lifting after a long press is not a tap.
        assert!(g.touch(1, Phase::End, p(50.0, 50.0), 0.8).is_empty());
    }

    #[test]
    fn one_finger_drag() {
        let mut g = Gestures::new();
        g.touch(1, Phase::Start, p(0.0, 0.0), 0.0);
        assert!(g.touch(1, Phase::Move, p(5.0, 0.0), 0.05).is_empty());
        let out = g.touch(1, Phase::Move, p(20.0, 0.0), 0.1);
        assert_eq!(
            out,
            vec![Gesture::DragStart(p(0.0, 0.0)), Gesture::DragMove { pos: p(20.0, 0.0), delta: p(20.0, 0.0) }]
        );
        assert_eq!(
            g.touch(1, Phase::Move, p(30.0, 5.0), 0.15),
            vec![Gesture::DragMove { pos: p(30.0, 5.0), delta: p(10.0, 5.0) }]
        );
        // A moving finger never becomes a long press.
        assert!(g.update(2.0).is_empty());
        assert_eq!(g.touch(1, Phase::End, p(30.0, 5.0), 0.2), vec![Gesture::DragEnd(p(30.0, 5.0))]);
    }

    #[test]
    fn pinch_and_pan() {
        let mut g = Gestures::new();
        g.touch(1, Phase::Start, p(100.0, 100.0), 0.0);
        g.touch(2, Phase::Start, p(200.0, 100.0), 0.01);
        // Spread the fingers to twice the distance: zoom in by half.
        let out = g.touch(2, Phase::Move, p(300.0, 100.0), 0.05);
        match out[0] {
            Gesture::PanZoom { pan, zoom, .. } => {
                assert!((zoom - 0.5).abs() < 1e-9);
                assert!((pan - p(50.0, 0.0)).length() < 1e-9);
            }
            other => panic!("{other:?}"),
        }
        // Lifting one finger ends the gesture without a tap.
        assert!(g.touch(1, Phase::End, p(100.0, 100.0), 0.1).is_empty());
        assert!(g.touch(2, Phase::End, p(300.0, 100.0), 0.1).is_empty());
        assert!(!g.active());
    }

    #[test]
    fn second_finger_cancels_a_drag() {
        let mut g = Gestures::new();
        g.touch(1, Phase::Start, p(0.0, 0.0), 0.0);
        g.touch(1, Phase::Move, p(30.0, 0.0), 0.1);
        assert_eq!(g.touch(2, Phase::Start, p(100.0, 0.0), 0.2), vec![Gesture::DragCancel]);
    }
}
