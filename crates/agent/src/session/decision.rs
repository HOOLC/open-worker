//! Pure next-action selection from durable state plus explicit world facts.

use std::collections::BTreeSet;

use super::events::{
    AutoWaitEndReason, DeadlineKind, OutstandingItem, Purpose, StepInterruptionReason, TurnOutcome,
    HANDOFF_TOOL_NAME,
};
use super::state::{SessionState, ToolDeliveryState};
use super::tools::ToolChange;

#[derive(Clone, Debug)]
pub struct DecisionWorld {
    pub now_ms: i64,
    pub live_tools: BTreeSet<String>,
    pub tool_changes: Vec<ToolChange>,
    pub outstanding: Vec<OutstandingItem>,
    pub estimated_input_tokens: Option<u64>,
    pub input_budget: Option<u64>,
    pub provider_retry_limit: u32,
    pub handoff_timeout_ms: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Decision {
    InterruptStep {
        step_id: String,
        reason: StepInterruptionReason,
    },
    InterruptTools {
        invocation_ids: Vec<String>,
    },
    StartTurn,
    EndAutoWait {
        step_id: String,
        reason: AutoWaitEndReason,
    },
    StartStep {
        purpose: Purpose,
        include_pending_tools: bool,
        outstanding: Vec<OutstandingItem>,
    },
    ApplyHandoff {
        document: Option<String>,
        failure: Option<String>,
    },
    FinishTurn {
        outcome: TurnOutcome,
        outstanding: Vec<OutstandingItem>,
    },
    ReachDeadline {
        deadline: DeadlineKind,
    },
    Park {
        until_ms: Option<i64>,
    },
}

pub fn decide(state: &SessionState, world: &DecisionWorld) -> Decision {
    if let Some(step) = &state.active_step {
        return Decision::InterruptStep {
            step_id: step.step_id.clone(),
            reason: StepInterruptionReason::Recovery,
        };
    }

    let interrupted = state
        .pending_tools
        .values()
        .filter(|pending| {
            pending.result.is_none()
                && !world.live_tools.contains(&pending.invocation.invocation_id)
        })
        .map(|pending| pending.invocation.invocation_id.clone())
        .collect::<Vec<_>>();
    if !interrupted.is_empty() {
        return Decision::InterruptTools {
            invocation_ids: interrupted,
        };
    }

    if state
        .active_turn
        .as_ref()
        .is_some_and(|turn| turn.cancel_requested)
    {
        return Decision::FinishTurn {
            outcome: TurnOutcome::Cancelled,
            outstanding: world.outstanding.clone(),
        };
    }

    if let Some(wait) = &state.auto_wait {
        let effective_deadline_ms = handoff_deadline_ms(state, world)
            .map_or(wait.deadline_ms, |deadline| deadline.min(wait.deadline_ms));
        let all_finished = wait.invocation_ids.iter().all(|id| {
            state
                .pending(id)
                .is_none_or(|pending| pending.result.is_some())
        });
        let reason = if all_finished {
            Some(AutoWaitEndReason::BatchCompleted)
        } else if !state.unconsumed_inputs.is_empty() {
            Some(AutoWaitEndReason::NewInput)
        } else if world.now_ms >= effective_deadline_ms {
            Some(AutoWaitEndReason::TimedOut)
        } else {
            None
        };
        return reason.map_or(
            Decision::Park {
                until_ms: Some(effective_deadline_ms),
            },
            |reason| Decision::EndAutoWait {
                step_id: wait.step_id.clone(),
                reason,
            },
        );
    }

    if let Some(turn) = &state.active_turn {
        if turn.consecutive_provider_failures > 0 {
            let failure = state.last_step_failure.as_ref();
            if failure.is_some_and(|failure| failure.error.is_context_overflow()) {
                return Decision::ApplyHandoff {
                    document: None,
                    failure: failure.map(|failure| failure.error.message.clone()),
                };
            }
            if failure.is_some_and(|failure| failure.purpose == Purpose::Handoff) {
                if let Some(reason) = handoff_limit_failure(state, world) {
                    return Decision::ApplyHandoff {
                        document: None,
                        failure: Some(reason),
                    };
                }
            }
            if !turn.provider_retry_allowed
                || turn.consecutive_provider_failures >= world.provider_retry_limit.max(1)
            {
                if failure.is_some_and(|failure| failure.purpose == Purpose::Handoff) {
                    return Decision::ApplyHandoff {
                        document: None,
                        failure: failure.map(|failure| failure.error.message.clone()),
                    };
                }
                return Decision::FinishTurn {
                    outcome: TurnOutcome::Failed,
                    outstanding: world.outstanding.clone(),
                };
            }
            return Decision::StartStep {
                purpose: failure.map_or(Purpose::Conversation, |failure| failure.purpose),
                include_pending_tools: true,
                outstanding: Vec::new(),
            };
        }
    }

    if let Some(wait) = &state.wait_deadline {
        if world.now_ms >= wait.deadline_ms {
            return Decision::ReachDeadline {
                deadline: DeadlineKind::Wait {
                    invocation_id: wait.invocation_id.clone(),
                },
            };
        }
        if state.unconsumed_inputs.is_empty() && state.pending_notices.is_empty() {
            return Decision::Park {
                until_ms: Some(wait.deadline_ms),
            };
        }
    }

    if state.active_turn.is_none() {
        let completed_tool_result = state
            .pending_tools
            .values()
            .any(|pending| pending.result.is_some())
            && state.last_turn_outcome != Some(TurnOutcome::Cancelled);
        let proactive_notice = !state.pending_notices.is_empty()
            && state.last_turn_outcome != Some(TurnOutcome::Failed);
        if !state.unconsumed_inputs.is_empty() || proactive_notice || completed_tool_result {
            return Decision::StartTurn;
        }
        return Decision::Park {
            until_ms: state.wait_deadline.as_ref().map(|wait| wait.deadline_ms),
        };
    }

    if let Some(decision) = control_decision(state, world) {
        return decision;
    }

    if state
        .active_turn
        .as_ref()
        .and_then(|turn| turn.handoff.as_ref())
        .is_some()
    {
        if let Some(reason) = handoff_limit_failure(state, world) {
            return Decision::ApplyHandoff {
                document: None,
                failure: Some(reason),
            };
        }
        return Decision::StartStep {
            purpose: Purpose::Handoff,
            include_pending_tools: true,
            outstanding: Vec::new(),
        };
    }

    if should_handoff(state, world) {
        return Decision::StartStep {
            purpose: Purpose::Handoff,
            include_pending_tools: true,
            outstanding: Vec::new(),
        };
    }

    let include_pending_tools = state
        .pending_tools
        .values()
        .any(|pending| pending.delivery == ToolDeliveryState::Fresh);
    Decision::StartStep {
        purpose: Purpose::Conversation,
        include_pending_tools,
        outstanding: if state
            .latest_assistant()
            .is_some_and(|(_, invocations)| invocations.is_empty())
        {
            world.outstanding.clone()
        } else {
            Vec::new()
        },
    }
}

fn handoff_deadline_ms(state: &SessionState, world: &DecisionWorld) -> Option<i64> {
    state
        .active_turn
        .as_ref()?
        .handoff
        .as_ref()
        .map(|progress| {
            progress
                .started_at_ms
                .saturating_add(world.handoff_timeout_ms.max(1))
        })
}

fn handoff_limit_failure(state: &SessionState, world: &DecisionWorld) -> Option<String> {
    let progress = state.active_turn.as_ref()?.handoff.as_ref()?;
    if world.now_ms >= handoff_deadline_ms(state, world)? {
        return Some("context handoff exceeded its total timeout".into());
    }
    (progress.attempts >= world.provider_retry_limit.max(1)).then(|| {
        format!(
            "the model did not produce a valid handoff document in {} handoff steps",
            progress.attempts
        )
    })
}

fn should_handoff(state: &SessionState, world: &DecisionWorld) -> bool {
    state.anchor().is_some()
        && !state.generation.entries.is_empty()
        && matches!(
            (world.estimated_input_tokens, world.input_budget),
            (Some(estimate), Some(budget)) if estimate > budget
        )
}

fn control_decision(state: &SessionState, world: &DecisionWorld) -> Option<Decision> {
    let (_, invocations) = state.latest_assistant()?;

    for invocation in invocations {
        let Some(result) = state
            .pending(&invocation.invocation_id)
            .and_then(|pending| pending.result.as_ref())
        else {
            continue;
        };
        if invocation.tool == HANDOFF_TOOL_NAME {
            let document = result
                .data
                .get("document")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
                .or_else(|| {
                    invocation
                        .arguments
                        .get("document")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                });
            return Some(Decision::ApplyHandoff {
                failure: (result.outcome != super::events::ToolOutcome::Succeeded)
                    .then(|| result.message.clone()),
                document,
            });
        }
    }

    if let Some(end) = invocations
        .iter()
        .find(|invocation| invocation.tool == super::events::END_TOOL_NAME)
    {
        let pending = state.pending(&end.invocation_id)?;
        pending.result.as_ref()?;
        let acknowledge = end
            .arguments
            .get("acknowledge_outstanding")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        if world.outstanding.is_empty() || acknowledge {
            return Some(Decision::FinishTurn {
                outcome: TurnOutcome::Finished,
                outstanding: world.outstanding.clone(),
            });
        }
        return Some(Decision::StartStep {
            purpose: Purpose::Conversation,
            include_pending_tools: true,
            outstanding: world.outstanding.clone(),
        });
    }

    None
}
