use patient_safe_agent_errors::{
    appointment_loop::{record_failure, AppointmentStep, StepFailure},
    infrai_errors::InfraiErrors,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let failure = StepFailure {
        run_id: "run-2026-10-01-0042".to_owned(),
        step: AppointmentStep::ConfirmAppointment,
        attempt: 1,
        kind: "SchedulingConflict".to_owned(),
        detail: "slot changed before confirmation".to_owned(),
    };

    let action = record_failure(&InfraiErrors::from_env()?, &failure).await?;
    println!("captured run {} and selected: {action:?}", failure.run_id);
    Ok(())
}
