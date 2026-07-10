//! Exponential backoff with a total-wait budget. No Win32, no I/O, no clock.

#[derive(Debug, Clone)]
pub struct Backoff {
    base_ms: u64,
    cap_ms: u64,
    give_up_after_ms: u64,
    next_ms: u64,
    elapsed_ms: u64,
}

impl Backoff {
    pub fn new(base_ms: u64, cap_ms: u64, give_up_after_ms: u64) -> Self {
        Self { base_ms, cap_ms, give_up_after_ms, next_ms: base_ms, elapsed_ms: 0 }
    }

    /// `None` means the caller should stop retrying.
    pub fn next_delay_ms(&mut self) -> Option<u64> {
        let delay = self.next_ms.min(self.cap_ms);
        if self.elapsed_ms + delay > self.give_up_after_ms {
            return None;
        }
        self.elapsed_ms += delay;
        self.next_ms = (self.next_ms * 2).min(self.cap_ms);
        Some(delay)
    }

    pub fn reset(&mut self) {
        self.next_ms = self.base_ms;
        self.elapsed_ms = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delays_double_until_the_cap() {
        let mut b = Backoff::new(500, 4_000, 60_000);
        assert_eq!(b.next_delay_ms(), Some(500));
        assert_eq!(b.next_delay_ms(), Some(1_000));
        assert_eq!(b.next_delay_ms(), Some(2_000));
        assert_eq!(b.next_delay_ms(), Some(4_000));
        assert_eq!(b.next_delay_ms(), Some(4_000));
    }

    #[test]
    fn gives_up_once_the_total_wait_exceeds_the_budget() {
        let mut b = Backoff::new(1_000, 1_000, 2_500);
        assert_eq!(b.next_delay_ms(), Some(1_000)); // total 1000
        assert_eq!(b.next_delay_ms(), Some(1_000)); // total 2000
        assert_eq!(b.next_delay_ms(), None); // would exceed 2500
    }

    #[test]
    fn reset_starts_over() {
        let mut b = Backoff::new(500, 4_000, 60_000);
        b.next_delay_ms();
        b.next_delay_ms();
        b.reset();
        assert_eq!(b.next_delay_ms(), Some(500));
    }

    #[test]
    fn a_reset_backoff_can_give_up_again() {
        let mut b = Backoff::new(1_000, 1_000, 1_000);
        assert_eq!(b.next_delay_ms(), Some(1_000));
        assert_eq!(b.next_delay_ms(), None);
        b.reset();
        assert_eq!(b.next_delay_ms(), Some(1_000));
    }
}
