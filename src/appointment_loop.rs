use crate::infrai_errors::{CaptureError, CaptureRequest, ExceptionPayload, InfraiErrors};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppointmentStep {
    FindSlot,
    ConfirmAppointment,
    SendReminder,
}

impl AppointmentStep {
    fn name(self) -> &'static str {
        match self {
            Self::FindSlot => "find-slot",
            Self::ConfirmAppointment => "confirm-appointment",
            Self::SendReminder => "send-reminder",
        }
    }

    fn requires_human_review(self) -> bool {
        matches!(self, Self::ConfirmAppointment)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepFailure {
    pub run_id: String,
    pub step: AppointmentStep,
    pub attempt: u32,
    pub kind: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationalAction {
    RetryQueued {
        run_id: String,
        step: AppointmentStep,
    },
    HumanReviewRequired {
        run_id: String,
        queue: &'static str,
    },
}

#[derive(Debug, Error)]
pub enum LoopError {
    #[error("failure capture did not complete: {0}")]
    Capture(#[from] CaptureError),
}

pub fn decide_action(failure: &StepFailure) -> OperationalAction {
    if failure.step.requires_human_review() {
        OperationalAction::HumanReviewRequired {
            run_id: failure.run_id.clone(),
            queue: "appointment-safety-review",
        }
    } else {
        OperationalAction::RetryQueued {
            run_id: failure.run_id.clone(),
            step: failure.step,
        }
    }
}

pub fn capture_request(failure: &StepFailure) -> CaptureRequest {
    let step = failure.step.name();
    CaptureRequest {
        title: format!("appointment-agent/{step} failed"),
        message: format!("{} at attempt {}", failure.kind, failure.attempt),
        exception: ExceptionPayload {
            r#type: failure.kind.clone(),
            value: failure.detail.clone(),
        },
        level: "error".to_owned(),
        fingerprint: vec!["appointment-agent".to_owned(), step.to_owned()],
        context: json!({
            "run_id": failure.run_id,
            "step": step,
            "attempt": failure.attempt,
            "patient_data_included": false
        }),
        service: "appointment-agent".to_owned(),
        idempotency_key: format!("{}:{step}:{}", failure.run_id, failure.attempt),
    }
}

pub async fn record_failure(
    infrai: &InfraiErrors,
    failure: &StepFailure,
) -> Result<OperationalAction, LoopError> {
    infrai.capture(&capture_request(failure)).await?;
    Ok(decide_action(failure))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_failure_routes_to_review_without_patient_identity() {
        let failure = StepFailure {
            run_id: "run-2026-10-01-0042".to_owned(),
            step: AppointmentStep::ConfirmAppointment,
            attempt: 1,
            kind: "SchedulingConflict".to_owned(),
            detail: "slot changed before confirmation".to_owned(),
        };

        assert_eq!(
            decide_action(&failure),
            OperationalAction::HumanReviewRequired {
                run_id: failure.run_id.clone(),
                queue: "appointment-safety-review",
            }
        );
        let request = capture_request(&failure);
        assert_eq!(
            request.fingerprint,
            ["appointment-agent", "confirm-appointment"]
        );
        assert_eq!(request.context["patient_data_included"], false);
        assert!(request.idempotency_key.ends_with(":confirm-appointment:1"));
    }
}
