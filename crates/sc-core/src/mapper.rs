//! Turning stick events into key presses and mouse movement, without any
//! platform code, so it can be tested on its own. The platform side feeds it
//! events and a clock, and sends what comes out.

use crate::keymap::{Action, HatDir, Input, KeyProfile, wheel_percent};
use crate::keys::Key;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Duration;

/// Something to send to the virtual keyboard or mouse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    KeyDown(Key),
    KeyUp(Key),
    /// Relative mouse movement in pixels.
    Move {
        dx: i32,
        dy: i32,
    },
    /// Mouse wheel; positive scrolls up. `hi_res` is in 1/120ths of a click
    /// (Linux's high-resolution wheel units), for programs that scroll
    /// smoothly; `clicks` is how many whole clicks that completes, for
    /// programs that only understand whole clicks (and on Windows, where
    /// many programs drop anything smaller).
    Wheel {
        hi_res: i32,
        clicks: i32,
    },
}

/// One wheel click in high-resolution units.
pub const WHEEL_CLICK: i32 = 120;

/// A tap on a scroll mapping turns the wheel one click. Held longer than
/// this, it scrolls smoothly at the profile's scroll speed.
const WHEEL_DELAY: Duration = Duration::from_millis(200);

struct Held {
    action: Action,
    /// When smooth scrolling starts, for wheel mappings.
    wheel_from: Duration,
}

pub struct Mapper {
    /// (stick index in the profile, input) -> action
    actions: HashMap<(usize, Input), Action>,
    held: BTreeMap<(usize, Input), Held>,
    key_counts: HashMap<Key, u32>,
    hats: HashMap<(usize, u8), BTreeSet<Input>>,
    mouse_speed: f32,
    carry: (f32, f32),
    wheel_speed: f32,
    /// Fraction of a high-resolution wheel unit not yet sent.
    wheel_carry: f32,
    /// High-resolution units sent that do not yet make a whole click.
    wheel_part: i32,
    last_tick: Option<Duration>,
}

impl Mapper {
    pub fn new(profile: &KeyProfile) -> Self {
        let mut actions = HashMap::new();
        for (i, stick) in profile.sticks.iter().enumerate() {
            for m in &stick.mappings {
                actions.insert((i, m.input), m.action.clone());
            }
        }
        Mapper {
            actions,
            held: BTreeMap::new(),
            key_counts: HashMap::new(),
            hats: HashMap::new(),
            mouse_speed: profile.mouse_speed.max(0.0),
            carry: (0.0, 0.0),
            wheel_speed: profile.wheel_speed.max(0.0),
            wheel_carry: 0.0,
            wheel_part: 0,
            last_tick: None,
        }
    }

    /// A button on stick `stick` (index into the profile's sticks) went down or up.
    pub fn button(&mut self, stick: usize, number: u16, pressed: bool, now: Duration) -> Vec<Output> {
        let input = Input::Button { number };
        if pressed { self.press(stick, input, now) } else { self.release(stick, input) }
    }

    /// A hat now points `dir` (`None` when centred). A diagonal with no
    /// mapping of its own counts as both straight directions it is made of,
    /// as in JoyToKey's 4-way mode.
    pub fn hat(&mut self, stick: usize, hat: u8, dir: Option<HatDir>, now: Duration) -> Vec<Output> {
        let mut wanted = BTreeSet::new();
        if let Some(d) = dir {
            let exact = Input::Hat { hat, direction: d };
            if self.actions.contains_key(&(stick, exact)) {
                wanted.insert(exact);
            } else if let Some((a, b)) = d.parts() {
                for part in [a, b] {
                    let input = Input::Hat { hat, direction: part };
                    if self.actions.contains_key(&(stick, input)) {
                        wanted.insert(input);
                    }
                }
            }
        }
        let before = self.hats.insert((stick, hat), wanted.clone()).unwrap_or_default();
        let mut out = Vec::new();
        for input in before.difference(&wanted) {
            out.extend(self.release(stick, *input));
        }
        for input in wanted.difference(&before) {
            out.extend(self.press(stick, *input, now));
        }
        out
    }

    fn press(&mut self, stick: usize, input: Input, now: Duration) -> Vec<Output> {
        let Some(action) = self.actions.get(&(stick, input)).cloned() else { return Vec::new() };
        if self.held.contains_key(&(stick, input)) {
            return Vec::new();
        }
        let mut out = Vec::new();
        match &action {
            Action::Keys { keys } => {
                for k in keys {
                    let n = self.key_counts.entry(*k).or_default();
                    *n += 1;
                    if *n == 1 {
                        out.push(Output::KeyDown(*k));
                    }
                }
            }
            Action::Wheel { amount } => {
                // A tap is always exactly one click, whatever is left over.
                let dir = wheel_percent(*amount).signum();
                self.wheel_carry = 0.0;
                self.wheel_part = 0;
                out.push(Output::Wheel { hi_res: dir * WHEEL_CLICK, clicks: dir });
            }
            Action::Mouse { .. } => {}
        }
        self.held.insert((stick, input), Held { action, wheel_from: now + WHEEL_DELAY });
        out
    }

    fn wheel_output(&mut self, hi_res: i32) -> Output {
        self.wheel_part += hi_res;
        let clicks = self.wheel_part / WHEEL_CLICK;
        self.wheel_part -= clicks * WHEEL_CLICK;
        Output::Wheel { hi_res, clicks }
    }

    fn release(&mut self, stick: usize, input: Input) -> Vec<Output> {
        let Some(held) = self.held.remove(&(stick, input)) else { return Vec::new() };
        let mut out = Vec::new();
        if let Action::Keys { keys } = &held.action {
            for k in keys.iter().rev() {
                if let Some(n) = self.key_counts.get_mut(k) {
                    *n = n.saturating_sub(1);
                    if *n == 0 {
                        self.key_counts.remove(k);
                        out.push(Output::KeyUp(*k));
                    }
                }
            }
        }
        out
    }

    /// Call regularly (every 10 ms or so): moves the mouse for held mouse
    /// mappings and scrolls for held wheel mappings.
    pub fn tick(&mut self, now: Duration) -> Vec<Output> {
        let last = self.last_tick.unwrap_or(now).max(now.saturating_sub(Duration::from_millis(100)));
        let dt = now.saturating_sub(last);
        self.last_tick = Some(now);
        let mut out = Vec::new();

        let (mut vx, mut vy) = (0.0f32, 0.0f32);
        let mut wheel = 0.0f32; // high-resolution units to add
        let mut wheel_held = false;
        for held in self.held.values() {
            match held.action {
                Action::Mouse { dx, dy } => {
                    vx += dx as f32 / 100.0;
                    vy += dy as f32 / 100.0;
                }
                Action::Wheel { amount } => {
                    wheel_held = true;
                    let active = now.saturating_sub(last.max(held.wheel_from));
                    let rate = self.wheel_speed * wheel_percent(amount) as f32 / 100.0 * WHEEL_CLICK as f32;
                    wheel += rate * active.as_secs_f32();
                }
                Action::Keys { .. } => {}
            }
        }
        if wheel_held {
            self.wheel_carry += wheel;
            let hi_res = self.wheel_carry.trunc();
            self.wheel_carry -= hi_res;
            if hi_res != 0.0 {
                out.push(self.wheel_output(hi_res as i32));
            }
        } else {
            self.wheel_carry = 0.0;
        }
        if vx != 0.0 || vy != 0.0 {
            let secs = dt.as_secs_f32();
            self.carry.0 += vx * self.mouse_speed * secs;
            self.carry.1 += vy * self.mouse_speed * secs;
            let (dx, dy) = (self.carry.0.trunc(), self.carry.1.trunc());
            self.carry.0 -= dx;
            self.carry.1 -= dy;
            if dx != 0.0 || dy != 0.0 {
                out.push(Output::Move { dx: dx as i32, dy: dy as i32 });
            }
        } else {
            self.carry = (0.0, 0.0);
        }
        out
    }

    /// Let go of everything, e.g. when stopping or when a stick is unplugged.
    pub fn release_all(&mut self) -> Vec<Output> {
        let held: Vec<(usize, Input)> = self.held.keys().copied().collect();
        let mut out = Vec::new();
        for (stick, input) in held {
            out.extend(self.release(stick, input));
        }
        self.hats.clear();
        out
    }

    /// Let go of everything held on one stick.
    pub fn release_stick(&mut self, stick: usize) -> Vec<Output> {
        let held: Vec<(usize, Input)> = self.held.keys().copied().filter(|(s, _)| *s == stick).collect();
        let mut out = Vec::new();
        for (s, input) in held {
            out.extend(self.release(s, input));
        }
        self.hats.retain(|(s, _), _| *s != stick);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::{Mapping, Stick};
    use crate::keys::parse_combo;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn profile() -> KeyProfile {
        let mut p = KeyProfile::new("t");
        p.mouse_speed = 1000.0;
        let keys = |s: &str| Action::Keys { keys: parse_combo(s).unwrap() };
        p.sticks.push(Stick {
            mappings: vec![
                Mapping { input: Input::Button { number: 5 }, action: keys("RCtrl+R") },
                Mapping { input: Input::Button { number: 6 }, action: keys("RCtrl") },
                Mapping { input: Input::Button { number: 21 }, action: Action::Wheel { amount: 1 } },
                Mapping { input: Input::Button { number: 23 }, action: Action::Wheel { amount: -50 } },
                Mapping {
                    input: Input::Hat { hat: 1, direction: HatDir::Up },
                    action: Action::Mouse { dx: 0, dy: -100 },
                },
                Mapping {
                    input: Input::Hat { hat: 1, direction: HatDir::Right },
                    action: Action::Mouse { dx: 100, dy: 0 },
                },
            ],
            ..Default::default()
        });
        p
    }

    fn k(s: &str) -> Key {
        Key::from_name(s).unwrap()
    }

    #[test]
    fn keys_are_held_while_the_button_is() {
        let mut m = Mapper::new(&profile());
        assert_eq!(m.button(0, 5, true, ms(0)), vec![Output::KeyDown(k("RCtrl")), Output::KeyDown(k("R"))]);
        assert_eq!(m.button(0, 5, true, ms(5)), vec![], "repeated press events change nothing");
        assert_eq!(m.button(0, 5, false, ms(10)), vec![Output::KeyUp(k("R")), Output::KeyUp(k("RCtrl"))]);
        assert_eq!(m.button(0, 7, true, ms(10)), vec![], "unmapped buttons do nothing");
    }

    #[test]
    fn a_shared_key_stays_down_until_both_buttons_are_released() {
        let mut m = Mapper::new(&profile());
        m.button(0, 5, true, ms(0));
        assert_eq!(m.button(0, 6, true, ms(1)), vec![], "RCtrl is already down");
        assert_eq!(m.button(0, 5, false, ms(2)), vec![Output::KeyUp(k("R"))]);
        assert_eq!(m.button(0, 6, false, ms(3)), vec![Output::KeyUp(k("RCtrl"))]);
    }

    #[test]
    fn mouse_moves_while_the_hat_is_held() {
        let mut m = Mapper::new(&profile());
        m.tick(ms(0));
        assert_eq!(m.hat(0, 1, Some(HatDir::Up), ms(0)), vec![]);
        assert_eq!(m.tick(ms(10)), vec![Output::Move { dx: 0, dy: -10 }], "1000 px/s for 10 ms");
        assert_eq!(m.tick(ms(15)), vec![Output::Move { dx: 0, dy: -5 }]);
        // A diagonal with no mapping of its own is up and right together.
        m.hat(0, 1, Some(HatDir::UpRight), ms(15));
        assert_eq!(m.tick(ms(25)), vec![Output::Move { dx: 10, dy: -10 }]);
        m.hat(0, 1, None, ms(25));
        assert_eq!(m.tick(ms(35)), vec![]);
    }

    /// Total (hi_res, clicks) sent while a wheel button is held for `hold`,
    /// ticking every 8 ms as the platforms do.
    fn scroll(m: &mut Mapper, button: u16, hold: u64) -> (i32, i32) {
        let mut out = m.button(0, button, true, ms(0));
        m.tick(ms(0));
        for t in (8..=hold).step_by(8) {
            out.extend(m.tick(ms(t)));
        }
        out.extend(m.button(0, button, false, ms(hold)));
        out.iter().fold((0, 0), |(h, c), o| match o {
            Output::Wheel { hi_res, clicks } => (h + hi_res, c + clicks),
            _ => (h, c),
        })
    }

    #[test]
    fn a_tap_is_one_wheel_click() {
        let mut m = Mapper::new(&profile());
        assert_eq!(m.button(0, 21, true, ms(0)), vec![Output::Wheel { hi_res: 120, clicks: 1 }]);
        m.tick(ms(0));
        assert_eq!(m.tick(ms(100)), vec![], "nothing more before the delay");
        m.button(0, 21, false, ms(150));
        assert_eq!(m.tick(ms(1000)), vec![]);
        assert_eq!(scroll(&mut m, 21, 150), (120, 1));
    }

    #[test]
    fn holding_scrolls_smoothly_at_the_set_speed() {
        // 20 clicks a second (the default) for the 0.8 s after the delay,
        // plus the first click: 17 clicks, sent as small steps.
        let mut m = Mapper::new(&profile());
        let (hi_res, clicks) = scroll(&mut m, 21, 1000);
        assert!((2030..=2040).contains(&hi_res), "{hi_res}");
        assert!((16..=17).contains(&clicks), "{clicks}");

        let mut m = Mapper::new(&profile());
        m.button(0, 21, true, ms(0));
        m.tick(ms(200));
        let step = m.tick(ms(208));
        assert!(
            matches!(step[..], [Output::Wheel { hi_res: 18..=19, clicks: 0 }]),
            "steps of a fraction of a click every tick: {step:?}"
        );
    }

    #[test]
    fn scroll_speed_and_share_apply() {
        let mut p = profile();
        p.wheel_speed = 40.0;
        let mut m = Mapper::new(&p);
        let (_, clicks) = scroll(&mut m, 21, 1000);
        assert!((32..=33).contains(&clicks), "twice as fast: {clicks}");
        // Button 23 scrolls down at 50% of 40: one click, then 20 a second.
        let (hi_res, clicks) = scroll(&mut m, 23, 1000);
        assert!(hi_res < 0 && (-17..=-16).contains(&clicks), "{hi_res} {clicks}");
    }

    #[test]
    fn stopping_lets_go_of_everything() {
        let mut m = Mapper::new(&profile());
        m.button(0, 5, true, ms(0));
        m.hat(0, 1, Some(HatDir::Up), ms(0));
        let out = m.release_all();
        assert_eq!(out.len(), 2);
        assert!(out.contains(&Output::KeyUp(k("RCtrl"))) && out.contains(&Output::KeyUp(k("R"))));
        m.tick(ms(0));
        assert_eq!(m.tick(ms(10)), vec![]);
    }
}
