//! Context pressure gauge.
//!
//! The governor does not pick a percentage of the model window. It measures
//! how many more calls fit at the observed growth rate, so a fast coding turn
//! compacts earlier in absolute tokens and a slow chat runs longer.

/// How close the context is to the working-set ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PressureLevel {
    Green,
    Amber,
    Orange,
    Red,
}

impl PressureLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Green => "green",
            Self::Amber => "amber",
            Self::Orange => "orange",
            Self::Red => "red",
        }
    }

    /// Share of the history budget kept verbatim at this level.
    pub fn detail_share(self) -> f32 {
        match self {
            Self::Green => 0.8,
            Self::Amber => 0.6,
            Self::Orange => 0.4,
            Self::Red => 0.2,
        }
    }
}

/// One pressure reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pressure {
    pub level: PressureLevel,
    pub used: u32,
    pub ceiling: u32,
    pub growth: u32,
    pub headroom_calls: u32,
}

/// Decision source for context pressure.
#[derive(Debug, Clone, Copy)]
pub struct ContextGovernor {
    ceiling: u32,
    capacity: u32,
}

impl ContextGovernor {
    /// `working_set` is the per-call input target; `context_window` is the
    /// hard capacity. The ceiling is the smaller of the two after the output
    /// reserve.
    pub fn new(context_window: u32, working_set: u32, output_reserve: u32) -> Self {
        let capacity = context_window.saturating_sub(output_reserve);
        Self {
            ceiling: capacity.min(working_set),
            capacity,
        }
    }

    pub fn ceiling(&self) -> u32 {
        self.ceiling
    }

    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    /// Reads pressure for the current call.
    ///
    /// `growth` is the tokens added per model call; zero is treated as no
    /// measurable growth, which cannot exhaust the budget on its own.
    pub fn pressure(&self, used: u32, growth: u32) -> Pressure {
        let remaining = self.ceiling.saturating_sub(used);
        let headroom_calls = remaining.checked_div(growth).unwrap_or(u32::MAX);
        let near_ceiling = used.saturating_mul(100) >= self.ceiling.saturating_mul(90);
        let level = if near_ceiling || headroom_calls <= 1 {
            PressureLevel::Red
        } else if headroom_calls <= 2 {
            PressureLevel::Orange
        } else if headroom_calls <= 4 {
            PressureLevel::Amber
        } else {
            PressureLevel::Green
        };
        Pressure {
            level,
            used,
            ceiling: self.ceiling,
            growth,
            headroom_calls,
        }
    }

    /// Whether the request no longer fits the model at all.
    pub fn over_capacity(&self, used: u32) -> bool {
        used > self.capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn governor() -> ContextGovernor {
        // A 1M window with a 200k working set, matching the observed plateau.
        ContextGovernor::new(1_000_000, 200_000, 4_096)
    }

    #[test]
    fn a_large_window_with_a_small_working_set_uses_the_working_set() {
        let pressure = governor().pressure(10_000, 1_000);
        assert_eq!(pressure.ceiling, 200_000);
        assert_eq!(pressure.level, PressureLevel::Green);
    }

    #[test]
    fn growth_moves_the_trigger_point() {
        let governor = ContextGovernor::new(200_000, 200_000, 4_096);
        // Ceiling is 195,904, so about 95k remains at 100k used.
        assert_eq!(
            governor.pressure(100_000, 10_000).level,
            PressureLevel::Green
        );
        assert_eq!(
            governor.pressure(100_000, 30_000).level,
            PressureLevel::Amber
        );
        assert_eq!(
            governor.pressure(100_000, 40_000).level,
            PressureLevel::Orange
        );
        assert_eq!(governor.pressure(100_000, 60_000).level, PressureLevel::Red);
    }

    #[test]
    fn the_ninety_percent_guard_wins_over_a_stale_growth_estimate() {
        let governor = ContextGovernor::new(200_000, 200_000, 4_096);
        let pressure = governor.pressure(190_000, 1);
        assert_eq!(pressure.level, PressureLevel::Red);
    }

    #[test]
    fn zero_growth_is_not_pressure() {
        let pressure = governor().pressure(10_000, 0);
        assert_eq!(pressure.level, PressureLevel::Green);
        assert_eq!(pressure.headroom_calls, u32::MAX);
    }

    #[test]
    fn capacity_only_counts_the_hard_window() {
        let governor = ContextGovernor::new(20_000, 12_000, 4_096);
        assert_eq!(governor.ceiling(), 12_000);
        assert_eq!(governor.capacity(), 15_904);
        assert!(!governor.over_capacity(12_500));
        assert!(governor.over_capacity(16_000));
    }

    #[test]
    fn detail_share_shrinks_with_pressure() {
        let shares = [
            PressureLevel::Green.detail_share(),
            PressureLevel::Amber.detail_share(),
            PressureLevel::Orange.detail_share(),
            PressureLevel::Red.detail_share(),
        ];
        assert_eq!(shares, [0.8, 0.6, 0.4, 0.2]);
    }
}
