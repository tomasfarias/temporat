use std::time::Duration;
use tokio::time::{Instant, Interval, MissedTickBehavior};

pub fn period(seconds: u32) -> Duration {
    Duration::from_secs(u64::from(if seconds == 0 { 30 } else { seconds }))
}

pub fn timer(seconds: u32) -> Interval {
    let period = period(seconds);
    let mut timer = tokio::time::interval_at(Instant::now() + period, period);
    timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
    timer
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::FutureExt;

    #[tokio::test(start_paused = true)]
    async fn waits_a_full_interval_and_skips_missed_ticks_without_bursts() {
        let mut timer = timer(30);
        assert!(timer.tick().now_or_never().is_none());
        tokio::time::advance(Duration::from_secs(29)).await;
        assert!(timer.tick().now_or_never().is_none());
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(timer.tick().now_or_never().is_some());
        tokio::time::advance(Duration::from_secs(300)).await;
        assert!(timer.tick().now_or_never().is_some());
        assert!(timer.tick().now_or_never().is_none());
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
