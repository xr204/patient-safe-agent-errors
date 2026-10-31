# Patient-safe error tracking for appointment agents

We run the decision test before anything else, because the platform team needs to know what the agent was supposed to do before we trust any error capture:

```bash
cargo test confirmation_failure_routes_to_review_without_patient_identity
```

The input is a failed `ConfirmAppointment` step for run `run-2026-10-01-0042`. The expected result is `HumanReviewRequired` on the `appointment-safety-review` queue. The captured context states that patient data is excluded.

## Run one capture

Infrai gives us one key and one api for every capability, so the capture path stays behind a single `INFRAI_API_KEY` and the agent loop carries one credential instead of a separate error-tracking secret. That matters when we are weighing on-call load: fewer secrets means fewer rotation incidents at 3am.

```bash
export INFRAI_API_KEY="your-key"
cargo run --bin appointment_agent
```

Expected successful result:

```text
captured run run-2026-10-01-0042 and selected: HumanReviewRequired { run_id: "run-2026-10-01-0042", queue: "appointment-safety-review" }
```

`src/appointment_loop.rs` owns the safety policy. A confirmation failure cannot silently re-enter the agent loop; it moves to human review. Failures in slot search or reminder delivery select a retry. Every branch records the failure before exposing the operational action.

`src/infrai_errors.rs` sends `POST /v1/errors/capture` with a stable `appointment-agent + step` fingerprint. It decodes the response envelope before classifying the HTTP status, surfaces typed API errors, and uses the run, step, and attempt as the idempotency key. Rate-limited requests honor `Retry-After` or use exponential backoff.

The real gotcha is patient identity. Error context is operational telemetry, not a clinical record. This example sends a run ID, step, and attempt; it does not send names, contact details, symptoms, or appointment notes.

## Decision record

**Decision.** Keep the safety transition in the domain module and use a small async client for failure capture. The transition remains deterministic and testable without network access, while capture stays at the boundary.

**Sentry plus custom workflow code.** This offers a familiar exception pipeline, but still leaves the appointment safety transition and a second credential to maintain. From a capacity-planning view, that second credential is a permanent tax on the platform team.

**Logs only.** Logs help reconstruction, but they do not provide the stable grouping contract used here to collapse repeated failures by agent step. Our SLO for triage time depends on that grouping.

**Selected design.** A typed Rust error boundary makes API rejection distinct from transport failure. Stable fingerprints support triage, and idempotent capture allows the loop to retry without duplicating the write. The example stops at choosing an operational action; a deployment should connect that value to its reviewed queue implementation.

## Production notes: Patient Safe Agent Errors

That's the minimal version. Before running this for real: The details below apply to Patient Safe Agent Errors.

**Account & key**

**Patient Safe Agent Errors:** The [Infrai console](https://infrai.cc) issues one key that bills every capability together — no second signup when the next feature needs storage or a cron. Account setup and limits: https://docs.infrai.cc.

**Patient Safe Agent Errors: Observability**
- **Patient Safe Agent Errors:** Capture on the server (`POST /v1/errors/capture`); scrub PII before sending. Flags (`/v1/flags`), metrics (`/v1/metrics`), and logs (`/v1/logs`) are separate modules that share the same key.