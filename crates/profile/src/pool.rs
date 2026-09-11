//! Account eligibility, quota ranking and cancellation-safe in-flight reservations.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::{ProfileModel, ProfileView};

pub fn compatible_model<'a>(
    profile: &'a ProfileView,
    model: &str,
    thinking: &str,
) -> Option<&'a ProfileModel> {
    profile.auth_configured.then_some(())?;
    profile.models.iter().find(|m| {
        m.enabled && m.id == model && m.thinking.iter().any(|t| t == thinking) && m.limits.is_some()
    })
}

#[derive(Clone, Copy, Debug)]
struct Quota {
    // Unknown is a separate class, never fabricated as 100% or unlimited.
    known: bool,
    score: f64,
    weekly: f64,
    short: f64,
}

fn number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|n| n.is_finite())
}

fn remaining(window: &Value) -> Option<f64> {
    number(&window["usedPercent"]).map(|used| (100.0 - used).clamp(0.0, 100.0))
}

fn quota(profile: &ProfileView, model: &str, now: i64) -> Option<Quota> {
    let failed = |snapshot: &Value| {
        snapshot["ok"] == false
            && !matches!(
                snapshot["error"].as_str(),
                Some("not_probed" | "not_reported_by_provider")
            )
    };
    if failed(&profile.account) || failed(&profile.rate_limits) {
        return None;
    }
    let limits = &profile.rate_limits["rateLimits"];
    let mut windows = vec![&limits["primary"], &limits["secondary"]];
    // Claude exposes model-family limits separately. Unrelated limits (e.g.
    // Codex code reviews) must not disqualify ordinary model calls.
    if profile.provider == "anthropic" {
        for family in ["sonnet", "opus", "haiku"] {
            if model.to_ascii_lowercase().contains(family) {
                if let Some(extra) =
                    profile.rate_limits["rateLimitsByLimitId"].get(format!("seven_day_{family}"))
                {
                    windows.push(&extra["secondary"]);
                }
            }
        }
    }
    if windows.iter().any(|w| remaining(w) == Some(0.0)) {
        // A passed reset timestamp is not proof that the provider replenished
        // quota. Wait for the next probe instead of inventing available credit.
        return None;
    }
    let credits = &limits["credits"];
    if profile.billing == "usage"
        && (credits["hasCredits"] == false || number(&credits["balance"]).is_some_and(|n| n <= 0.0))
    {
        return None;
    }
    let mut weekly: Option<(f64, f64)> = None;
    let mut short: Option<f64> = None;
    for (index, window) in windows.iter().enumerate() {
        let Some(left) = remaining(window) else {
            continue;
        };
        let duration = number(&window["windowDurationMins"]).unwrap_or(if index == 0 {
            300.0
        } else {
            10080.0
        });
        if duration >= 10080.0 {
            let days = number(&window["resetsAt"])
                .map(|reset| ((reset - now as f64) / 86400.0).max(1.0 / 1440.0))
                .unwrap_or(7.0);
            let score = (left / 100.0) / (days / 7.0);
            if weekly.is_none_or(|(_, old)| score < old) {
                weekly = Some((left, score));
            }
        } else {
            short = Some(short.map_or(left, |old| old.min(left)));
        }
    }
    let balance_known = number(&credits["balance"]).is_some_and(|n| n > 0.0);
    Some(Quota {
        known: weekly.is_some() || short.is_some() || balance_known,
        score: weekly
            .map(|(_, score)| score)
            .or(short.map(|n| n / 100.0))
            .unwrap_or(1.0),
        weekly: weekly.map(|(left, _)| left).unwrap_or(0.0),
        short: short.unwrap_or(0.0),
    })
}

#[derive(Default)]
struct Occupancy {
    active: usize,
    cooldown_until: i64,
    last_selected: u64,
}

#[derive(Default)]
struct State {
    profiles: HashMap<String, Occupancy>,
    sequence: u64,
}

#[derive(Clone, Default)]
pub struct AccountPool(Arc<Mutex<State>>);

impl AccountPool {
    /// Selection and reservation share one lock; concurrent callers cannot all
    /// observe the same unreserved snapshot. Preferred is the last durable call.
    pub fn acquire(
        &self,
        profiles: &[ProfileView],
        model: &str,
        thinking: &str,
        preferred: Option<&str>,
        now: i64,
    ) -> Option<AccountLease> {
        let mut state = self.0.lock().expect("account pool");
        let mut candidates: Vec<_> = profiles
            .iter()
            .filter_map(|p| {
                compatible_model(p, model, thinking)?;
                let q = quota(p, model, now)?;
                let occupancy = state.profiles.get(&p.profile_id);
                if occupancy.is_some_and(|o| o.cooldown_until > now) {
                    return None;
                }
                let active = occupancy.map_or(0, |o| o.active);
                let last = occupancy.map_or(0, |o| o.last_selected);
                Some((p, q, active, last))
            })
            .collect();
        candidates.sort_by(|(a, aq, aa, al), (b, bq, ba, bl)| {
            // Healthy subscriptions precede metered accounts to preserve the
            // existing preference. Within each class, known quota precedes unknown.
            (b.billing == "subscription")
                .cmp(&(a.billing == "subscription"))
                .then_with(|| bq.known.cmp(&aq.known))
                .then_with(|| (bq.score / (1 + ba) as f64).total_cmp(&(aq.score / (1 + aa) as f64)))
                .then_with(|| bq.weekly.total_cmp(&aq.weekly))
                .then_with(|| bq.short.total_cmp(&aq.short))
                .then_with(|| al.cmp(bl))
                .then_with(|| a.profile_id.cmp(&b.profile_id))
        });
        let chosen = candidates
            .iter()
            .find(|(p, _, _, _)| Some(p.profile_id.as_str()) == preferred)
            .or_else(|| candidates.first())?
            .0
            .profile_id
            .clone();
        state.sequence += 1;
        let sequence = state.sequence;
        let occupancy = state.profiles.entry(chosen.clone()).or_default();
        occupancy.active += 1;
        occupancy.last_selected = sequence;
        Some(AccountLease {
            pool: self.clone(),
            profile_id: chosen,
        })
    }

    /// Explicit selections bypass eligibility but still consume shared capacity.
    pub fn reserve_explicit(&self, profile_id: &str) -> AccountLease {
        let mut state = self.0.lock().expect("account pool");
        state
            .profiles
            .entry(profile_id.to_owned())
            .or_default()
            .active += 1;
        AccountLease {
            pool: self.clone(),
            profile_id: profile_id.to_owned(),
        }
    }

    pub fn clear_failure(&self, profile_id: &str) {
        if let Some(state) = self
            .0
            .lock()
            .expect("account pool")
            .profiles
            .get_mut(profile_id)
        {
            state.cooldown_until = 0;
        }
    }
}

/// Held across credential refresh and the provider call; Drop also runs on cancellation.
pub struct AccountLease {
    pool: AccountPool,
    pub profile_id: String,
}

impl AccountLease {
    pub fn reject(&self, now: i64) {
        self.pool
            .0
            .lock()
            .expect("account pool")
            .profiles
            .entry(self.profile_id.clone())
            .or_default()
            .cooldown_until = now + 60;
    }
}

impl Drop for AccountLease {
    fn drop(&mut self) {
        if let Some(state) = self
            .pool
            .0
            .lock()
            .expect("account pool")
            .profiles
            .get_mut(&self.profile_id)
        {
            state.active = state.active.saturating_sub(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn profile(id: &str, used: Option<f64>, days: f64) -> ProfileView {
        serde_json::from_value(json!({
            "profile_id":id,"provider":"openai","billing":"subscription","auth_configured":true,
            "account":{"ok":true},"rateLimits":{"ok":true,"rateLimits":{"secondary":{
                "usedPercent":used,"windowDurationMins":10080,"resetsAt":1_000_000.0+days*86400.0
            }}},"models":[{"id":"model","api":"openai-responses","thinking":["high"],
                "default_thinking":"high","capabilities":{"input":["text"]},
                "limits":{"context_window_tokens":32000,"max_output_tokens":4096}}]
        }))
        .unwrap()
    }
    fn acquire(
        pool: &AccountPool,
        profiles: &[ProfileView],
        preferred: Option<&str>,
    ) -> Option<AccountLease> {
        pool.acquire(profiles, "model", "high", preferred, 1_000_000)
    }

    #[test]
    fn weekly_reset_score_and_primary_weekly_window() {
        let pool = AccountPool::default();
        let mut profiles = vec![
            profile("more", Some(20.0), 7.0),
            profile("soon", Some(50.0), 1.0),
        ];
        let weekly = profiles[1].rate_limits["rateLimits"]["secondary"].take();
        profiles[1].rate_limits["rateLimits"]["primary"] = weekly;
        assert_eq!(acquire(&pool, &profiles, None).unwrap().profile_id, "soon");
    }

    #[test]
    fn unknown_is_fallback_and_never_beats_known_quota() {
        let pool = AccountPool::default();
        let mut profiles = vec![
            profile("unknown", None, 7.0),
            profile("known", Some(99.0), 7.0),
        ];
        assert_eq!(acquire(&pool, &profiles, None).unwrap().profile_id, "known");
        profiles[1].rate_limits["rateLimits"]["secondary"]["usedPercent"] = json!(100);
        assert_eq!(
            acquire(&pool, &profiles, None).unwrap().profile_id,
            "unknown"
        );
        profiles[0].account["ok"] = json!(false);
        assert!(acquire(&pool, &profiles, None).is_none());
    }

    #[test]
    fn reservations_affinity_cooldown_and_cancellation() {
        let pool = AccountPool::default();
        let profiles = vec![profile("a", Some(50.0), 7.0), profile("b", Some(50.0), 7.0)];
        let first = acquire(&pool, &profiles, None).unwrap();
        let second = acquire(&pool, &profiles, None).unwrap();
        assert_ne!(first.profile_id, second.profile_id);
        assert_eq!(
            acquire(&pool, &profiles, Some("a")).unwrap().profile_id,
            "a"
        );
        first.reject(1_000_000);
        assert_eq!(
            acquire(&pool, &profiles, Some("a")).unwrap().profile_id,
            "b"
        );
        second.reject(1_000_000);
        assert!(acquire(&pool, &profiles, None).is_none());
        drop((first, second));
        assert!(pool
            .0
            .lock()
            .unwrap()
            .profiles
            .values()
            .all(|p| p.active == 0));
        assert!(pool
            .acquire(&profiles, "model", "high", None, 1_000_061)
            .is_some());
    }

    #[test]
    fn model_effort_credentials_disabled_and_exhausted_are_hard_filters() {
        let pool = AccountPool::default();
        let mut p = profile("a", Some(50.0), 7.0);
        assert!(pool
            .acquire(&[p.clone()], "model", "low", None, 1_000_000)
            .is_none());
        p.models[0].enabled = false;
        assert!(acquire(&pool, &[p.clone()], None).is_none());
        p.models[0].enabled = true;
        p.auth_configured = false;
        assert!(acquire(&pool, &[p.clone()], None).is_none());
        p.auth_configured = true;
        p.rate_limits["rateLimits"]["primary"] = json!({"usedPercent":100,"resetsAt":1});
        assert!(acquire(&pool, &[p], None).is_none());
    }

    #[test]
    fn unrelated_limits_do_not_block_but_claude_model_limits_do() {
        let pool = AccountPool::default();
        let mut p = profile("a", Some(50.0), 7.0);
        p.rate_limits["rateLimitsByLimitId"] = json!({"reviews":{"primary":{"usedPercent":100}}});
        assert!(acquire(&pool, &[p.clone()], None).is_some());
        p.provider = "anthropic".into();
        p.models[0].id = "claude-sonnet-4".into();
        p.rate_limits["rateLimitsByLimitId"]["seven_day_sonnet"] =
            json!({"secondary":{"usedPercent":100}});
        assert!(pool
            .acquire(&[p], "claude-sonnet-4", "high", None, 1_000_000)
            .is_none());
    }

    #[test]
    fn usage_zero_balance_excluded_and_unlimited_marker_is_unknown() {
        let pool = AccountPool::default();
        let mut p = profile("a", None, 7.0);
        p.billing = "usage".into();
        p.rate_limits["rateLimits"]["credits"] = json!({"balance":"0","unlimited":true});
        assert!(acquire(&pool, &[p.clone()], None).is_none());
        p.rate_limits["rateLimits"]["credits"] = json!({"unlimited":true,"balance":null});
        assert!(!quota(&p, "model", 1_000_000).unwrap().known);
    }
}
