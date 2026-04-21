/// Hybrid Logical Clock (HLC)
///
/// Fixed-width, lex-sortable string format:
/// `${13-digit-unix-ms}-${6-digit-logical-counter}-${instance_id}`
///
/// Example: `1713570123456-000042-node-sg-1`
///
/// Invariants:
/// - Strictly monotonically increasing across all local events.
/// - Survives restarts and NTP steps (restored from `node_state` on boot).
/// - Remote HLCs whose physical component is >60s in the future are rejected.
use log::{debug, warn};
use std::sync::{Arc, Mutex};

const MAX_FUTURE_SKEW_MS: u64 = 60_000;

fn wall_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Parse the physical-time component (milliseconds) out of an HLC string.
pub fn physical_ms(hlc: &str) -> Option<u64> {
    hlc.splitn(3, '-').next()?.parse().ok()
}

/// Parse the logical-counter component out of an HLC string.
pub fn logical(hlc: &str) -> Option<u32> {
    let mut parts = hlc.splitn(3, '-');
    parts.next()?;
    parts.next()?.parse().ok()
}

#[derive(Clone)]
pub struct HlcState {
    /// Current max physical time seen (milliseconds since epoch).
    pub pt: u64,
    /// Logical counter for the current physical-time tick.
    pub l: u32,
    /// Stable identifier for this node, embedded in every HLC.
    pub instance_id: String,
}

impl HlcState {
    pub fn new(instance_id: &str) -> Self {
        Self {
            pt: wall_ms(),
            l: 0,
            instance_id: instance_id.to_string(),
        }
    }

    /// Restore from a persisted HLC string (e.g. loaded from node_state on boot).
    /// Physical time is set to `max(persisted_pt, wall_clock)`.
    /// Logical counter is bumped by 1 if pt did not advance.
    pub fn restore(persisted_hlc: &str, instance_id: &str) -> Self {
        let persisted_pt = physical_ms(persisted_hlc).unwrap_or(0);
        let wall = wall_ms();
        let pt = persisted_pt.max(wall);
        let l = if pt == persisted_pt {
            logical(persisted_hlc).unwrap_or(0).saturating_add(1)
        } else {
            0
        };
        debug!(
            "[hlc] restored: persisted_pt={} wall={} -> pt={} l={}",
            persisted_pt, wall, pt, l
        );
        Self {
            pt,
            l,
            instance_id: instance_id.to_string(),
        }
    }

    /// Format the current state as a lex-sortable HLC string.
    pub fn to_string(&self) -> String {
        format!("{:013}-{:06}-{}", self.pt, self.l, self.instance_id)
    }
}

/// Thread-safe HLC shared across the application.
#[derive(Clone)]
pub struct Hlc(Arc<Mutex<HlcState>>);

impl Hlc {
    pub fn new(instance_id: &str) -> Self {
        Self(Arc::new(Mutex::new(HlcState::new(instance_id))))
    }

    pub fn restore(persisted_hlc: &str, instance_id: &str) -> Self {
        Self(Arc::new(Mutex::new(HlcState::restore(
            persisted_hlc,
            instance_id,
        ))))
    }

    /// Advance the HLC and return the new value.
    ///
    /// ```text
    /// l = max(local.pt, wall) unchanged? → l + 1 : 0
    /// pt = max(local.pt, wall)
    /// ```
    pub fn now(&self) -> String {
        let mut state = self.0.lock().unwrap();
        let wall = wall_ms();
        let new_pt = state.pt.max(wall);
        let new_l = if new_pt == state.pt {
            state.l.saturating_add(1)
        } else {
            0
        };
        state.pt = new_pt;
        state.l = new_l;
        state.to_string()
    }

    /// Merge a remote HLC into the local clock (called when applying an incoming log entry).
    ///
    /// Rejects remote HLCs whose physical component is >60s ahead of the local wall clock.
    /// Returns `Err` with the reason if the remote HLC is rejected; otherwise returns the
    /// new local HLC string (which is strictly greater than the remote HLC).
    pub fn update(&self, remote_hlc: &str) -> Result<String, String> {
        let remote_pt = physical_ms(remote_hlc)
            .ok_or_else(|| format!("invalid remote HLC: '{}'", remote_hlc))?;
        let remote_l = logical(remote_hlc)
            .ok_or_else(|| format!("invalid remote HLC logical part: '{}'", remote_hlc))?;

        let wall = wall_ms();
        if remote_pt > wall + MAX_FUTURE_SKEW_MS {
            warn!(
                "[hlc] remote HLC '{}' is {}ms ahead of wall clock — rejected",
                remote_hlc,
                remote_pt - wall
            );
            return Err(format!(
                "remote HLC too far in future: {}ms skew",
                remote_pt - wall
            ));
        }

        let mut state = self.0.lock().unwrap();
        let max_pt = state.pt.max(remote_pt).max(wall);
        let new_l = if max_pt == state.pt && max_pt == remote_pt {
            state.l.max(remote_l).saturating_add(1)
        } else if max_pt == state.pt {
            state.l.saturating_add(1)
        } else if max_pt == remote_pt {
            remote_l.saturating_add(1)
        } else {
            0
        };
        state.pt = max_pt;
        state.l = new_l;
        Ok(state.to_string())
    }

    /// Return the current HLC string without advancing the clock.
    pub fn peek(&self) -> String {
        self.0.lock().unwrap().to_string()
    }

    /// Return the current physical timestamp in milliseconds.
    pub fn physical_ms(&self) -> u64 {
        self.0.lock().unwrap().pt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_ms_parses_correctly() {
        assert_eq!(physical_ms("1713570123456-000042-node-1"), Some(1713570123456));
    }

    #[test]
    fn logical_parses_correctly() {
        assert_eq!(logical("1713570123456-000042-node-1"), Some(42));
    }

    #[test]
    fn hlc_now_is_lex_sortable() {
        let hlc = Hlc::new("test-node");
        let t1 = hlc.now();
        let t2 = hlc.now();
        assert!(t2 > t1, "t2={} should be > t1={}", t2, t1);
    }

    #[test]
    fn hlc_now_monotonic_across_calls() {
        let hlc = Hlc::new("node-a");
        let mut prev = hlc.now();
        for _ in 0..100 {
            let next = hlc.now();
            assert!(next > prev, "next={} must be > prev={}", next, prev);
            prev = next;
        }
    }

    #[test]
    fn hlc_update_rejects_far_future() {
        let hlc = Hlc::new("local");
        // Use an HLC 2 minutes in the future
        let future_ms = wall_ms() + 120_001;
        let remote = format!("{:013}-000000-remote", future_ms);
        assert!(hlc.update(&remote).is_err());
    }

    #[test]
    fn hlc_update_advances_local_clock() {
        let hlc = Hlc::new("local");
        // Use an HLC in the near past (still valid)
        let past_ms = wall_ms() - 1000;
        let remote = format!("{:013}-000000-remote", past_ms);
        let result = hlc.update(&remote);
        assert!(result.is_ok());
        // After update, a new now() must be greater
        let after = hlc.now();
        assert!(after > result.unwrap());
    }

    #[test]
    fn hlc_restore_never_goes_back() {
        let past_ms = wall_ms() - 5000;
        let past_hlc = format!("{:013}-000099-old-node", past_ms);
        let state = HlcState::restore(&past_hlc, "new-node");
        // Should end up at wall time since wall > past
        assert!(state.pt >= past_ms);
    }

    #[test]
    fn hlc_restore_from_future_persisted_advances_l() {
        // Simulate a persisted HLC exactly equal to wall clock (unlikely in practice
        // but we test the branch where pt == persisted_pt)
        let wall = wall_ms();
        let hlc_str = format!("{:013}-000010-node", wall);
        let state = HlcState::restore(&hlc_str, "node");
        // If pt == persisted_pt, l should be bumped
        if state.pt == wall {
            assert!(state.l >= 11);
        }
    }

    #[test]
    fn hlc_string_format_is_fixed_width_prefix() {
        let hlc = Hlc::new("n");
        let s = hlc.now();
        let parts: Vec<&str> = s.splitn(3, '-').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].len(), 13, "physical part must be 13 digits");
        assert_eq!(parts[1].len(), 6, "logical part must be 6 digits");
    }

    #[test]
    fn hlc_lex_order_matches_causal_order() {
        // Two HLCs with the same physical time but different logical counters
        let a = "1713570123456-000001-node-a";
        let b = "1713570123456-000002-node-b";
        // b should sort after a lexicographically
        assert!(b > a);
    }
}
