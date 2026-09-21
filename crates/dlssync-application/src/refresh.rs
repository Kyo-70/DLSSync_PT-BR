use std::time::Duration;

pub const ACTIVE_CATALOG_INTERVAL: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshReason {
    Startup,
    Focus,
    VisibleTimer,
    HiddenTimer,
    ManualUser,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshPolicy {
    pub automatic_allowed: bool,
    pub visible: bool,
    pub background_enabled: bool,
    pub background_interval: Duration,
}

pub fn refresh_due(
    policy: RefreshPolicy,
    reason: RefreshReason,
    elapsed_since_attempt: Option<Duration>,
) -> bool {
    if reason == RefreshReason::ManualUser {
        return true;
    }
    if !policy.automatic_allowed {
        return false;
    }
    match reason {
        RefreshReason::Startup => true,
        RefreshReason::Focus | RefreshReason::VisibleTimer if policy.visible => {
            elapsed_since_attempt.is_none_or(|elapsed| elapsed >= ACTIVE_CATALOG_INTERVAL)
        }
        RefreshReason::HiddenTimer if !policy.visible && policy.background_enabled => {
            elapsed_since_attempt.is_none_or(|elapsed| elapsed >= policy.background_interval)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn standard(visible: bool) -> RefreshPolicy {
        RefreshPolicy {
            automatic_allowed: true,
            visible,
            background_enabled: true,
            background_interval: Duration::from_secs(6 * 60 * 60),
        }
    }

    #[test]
    fn phase4_visible_refresh_uses_exact_fifteen_minute_boundary() {
        assert!(!refresh_due(
            standard(true),
            RefreshReason::Focus,
            Some(Duration::from_secs(899))
        ));
        assert!(refresh_due(
            standard(true),
            RefreshReason::Focus,
            Some(Duration::from_secs(900))
        ));
    }

    #[test]
    fn phase4_hidden_refresh_keeps_six_hour_setting() {
        assert!(!refresh_due(
            standard(false),
            RefreshReason::HiddenTimer,
            Some(Duration::from_secs(6 * 60 * 60 - 1))
        ));
        assert!(refresh_due(
            standard(false),
            RefreshReason::HiddenTimer,
            Some(Duration::from_secs(6 * 60 * 60))
        ));
    }

    #[test]
    fn phase4_nexus_allows_only_manual_refresh() {
        let nexus = RefreshPolicy {
            automatic_allowed: false,
            ..standard(true)
        };
        assert!(!refresh_due(nexus, RefreshReason::Startup, None));
        assert!(!refresh_due(nexus, RefreshReason::Focus, None));
        assert!(refresh_due(nexus, RefreshReason::ManualUser, None));
    }
}
