//! Bounded in-memory occupancy series. Missing samples are gaps, never zero.
use std::{collections::VecDeque, time::Duration};
pub const WINDOW: Duration = Duration::from_secs(60);
const CAPACITY: usize = 121;
#[derive(Debug, Clone, Copy)]
pub struct Point {
    pub time: Duration,
    pub percent: Option<f64>,
}
#[derive(Debug, Default)]
pub struct PercentSeries {
    points: VecDeque<Point>,
}
impl PercentSeries {
    pub fn push(&mut self, time: Duration, percent: Option<f64>) {
        if self.points.back().is_some_and(|p| p.time >= time) {
            return;
        }
        let percent = percent.filter(|p| p.is_finite() && (0. ..=100.).contains(p));
        self.points.push_back(Point { time, percent });
        let cutoff = time.saturating_sub(WINDOW);
        while self.points.len() > CAPACITY || self.points.front().is_some_and(|p| p.time < cutoff) {
            self.points.pop_front();
        }
    }
    pub fn points(&self) -> impl Iterator<Item = &Point> {
        self.points.iter()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_gaps_and_rejects_invalid_values_and_reordered_times() {
        let mut series = PercentSeries::default();
        for (time, value) in [
            (0, Some(0.)),
            (1, None),
            (2, Some(f64::NAN)),
            (3, Some(101.)),
            (4, Some(80.)),
        ] {
            series.push(Duration::from_secs(time), value);
        }
        series.push(Duration::from_secs(3), Some(10.));
        let values = series.points().map(|p| p.percent).collect::<Vec<_>>();
        assert_eq!(values, [Some(0.), None, None, None, Some(80.)]);
    }
    #[test]
    fn bounds_memory_and_uses_elapsed_time_instead_of_sample_number() {
        let mut series = PercentSeries::default();
        for time in 0..1000 {
            series.push(Duration::from_millis(time), Some(50.));
        }
        assert_eq!(series.points().count(), CAPACITY);
        series.push(Duration::from_secs(70), None);
        assert_eq!(series.points().count(), 1);
    }
}
