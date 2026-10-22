use std::time::Duration;
use tokio::time::{Instant, Interval, MissedTickBehavior};

pub fn period(seconds: u32) -> Duration {
    Duration::from_secs(u64::from(if seconds == 0 { 30 } else { seconds }))
}

pub struct Timer {
    interval: Interval,
    next: Instant,
}

impl Timer {
    pub async fn tick(&mut self) {
        let scheduled = self.interval.tick().await;
        let period = self.interval.period();
        let now = Instant::now();
        self.next = scheduled + period;
        if self.next <= now {
            // Match Interval's Skip behavior so the countdown follows the actual
            // next tick even after the event loop has been delayed.
            let remainder = now.duration_since(scheduled).as_nanos() % period.as_nanos();
            self.next = now + period - Duration::from_nanos(remainder as u64);
        }
    }

    pub fn reset(&mut self) {
        self.next = Instant::now() + self.interval.period();
        self.interval.reset_at(self.next);
    }

    pub fn remaining_secs(&self) -> u64 {
        let remaining = self.next.saturating_duration_since(Instant::now());
        remaining.as_secs() + u64::from(remaining.subsec_nanos() != 0)
    }
}

pub fn timer(seconds: u32) -> Timer {
    let period = period(seconds);
    let next = Instant::now() + period;
    let mut timer = tokio::time::interval_at(next, period);
    timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
    Timer {
        interval: timer,
        next,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::FutureExt;

    #[tokio::test(start_paused = true)]
    async fn waits_a_full_interval_and_skips_missed_ticks_without_bursts() {
        let mut timer = timer(30);
        assert_eq!(timer.remaining_secs(), 30);
        assert!(timer.tick().now_or_never().is_none());
        tokio::time::advance(Duration::from_secs(29)).await;
        assert_eq!(timer.remaining_secs(), 1);
        assert!(timer.tick().now_or_never().is_none());
        tokio::time::advance(Duration::from_secs(1)).await;
        assert_eq!(timer.remaining_secs(), 0);
        assert!(timer.tick().now_or_never().is_some());
        assert_eq!(timer.remaining_secs(), 30);
        tokio::time::advance(Duration::from_secs(300)).await;
        assert!(timer.tick().now_or_never().is_some());
        assert!(timer.tick().now_or_never().is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn countdown_rounds_up_and_tracks_skipped_tick_deadlines() {
        let mut timer = timer(30);
        tokio::time::advance(Duration::from_millis(1250)).await;
        assert_eq!(timer.remaining_secs(), 29);
        assert!(timer.tick().now_or_never().is_none());
        tokio::time::advance(Duration::from_secs(100)).await;
        assert!(timer.tick().now_or_never().is_some());
        assert_eq!(timer.remaining_secs(), 19);
        tokio::time::advance(Duration::from_millis(18750)).await;
        assert_eq!(timer.remaining_secs(), 0);
        assert!(timer.tick().now_or_never().is_some());
        timer.reset();
        assert_eq!(timer.remaining_secs(), 30);
    }

    #[tokio::test(start_paused = true)]
    async fn reset_on_resume_or_profile_switch_waits_a_new_interval() {
        let mut timer = timer(0);
        tokio::time::advance(Duration::from_secs(60)).await;
        timer.reset();
        assert!(timer.tick().now_or_never().is_none());
        tokio::time::advance(Duration::from_secs(29)).await;
        assert!(timer.tick().now_or_never().is_none());
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(timer.tick().now_or_never().is_some());
    }
}
