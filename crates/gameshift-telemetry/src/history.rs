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
        self.push_value(time, percent, true);
    }
    pub fn push_rate(&mut self, time: Duration, value: Option<f64>) {
        self.push_value(time, value, false);
    }
    fn push_value(&mut self, time: Duration, percent: Option<f64>, bounded: bool) {
        if self.points.back().is_some_and(|p| p.time >= time) {
            return;
        }
        let percent = percent.filter(|p| p.is_finite() && *p >= 0. && (!bounded || *p <= 100.));
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

#[cfg(test)]
mod rate_tests {
    use super::*;
    #[test]
    fn observed_rates_are_unbounded_with_real_zero_and_invalid_gaps() {
        let mut series = PercentSeries::default();
        for (t, v) in [Some(0.), Some(2500.), Some(-1.), Some(f64::INFINITY), None]
            .into_iter()
            .enumerate()
        {
            series.push_rate(Duration::from_secs(t as u64), v);
        }
        assert_eq!(
            series.points().map(|p| p.percent).collect::<Vec<_>>(),
            [Some(0.), Some(2500.), None, None, None]
        );
        series.push_rate(Duration::from_secs(70), Some(5000.));
        assert_eq!(series.points().count(), 1);
    }
}

#[cfg(test)]
mod percentage_history_tests {
    use super::*;
    #[test]
    fn invalid_percentage_is_a_gap_without_erasing_the_sixty_second_history() {
        let mut series = PercentSeries::default();
        for (time, value) in [
            (0, Some(0.)),
            (10, Some(8.2)),
            (20, Some(101.)),
            (30, None),
            (60, Some(100.)),
        ] {
            series.push(Duration::from_secs(time), value);
        }
        assert_eq!(
            series
                .points()
                .map(|p| (p.time.as_secs(), p.percent))
                .collect::<Vec<_>>(),
            [
                (0, Some(0.)),
                (10, Some(8.2)),
                (20, None),
                (30, None),
                (60, Some(100.))
            ]
        );
        series.push(Duration::from_secs(61), Some(20.));
        assert_eq!(
            series.points().next().unwrap().time,
            Duration::from_secs(10)
        );
    }
}
