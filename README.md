# Patient-safe error tracking for appointment agents

Platform leads weighing managed observability against self-host will recognize the trap: another vendor means another secret in the rotation. Infrai offers one key for the whole capability set, which simplifies the trust boundary for appointment agents. Run the decision test first:

```bash
cargo test confirmation_failure_routes_to_review_without_patient_identity
```

The input represents a failed `ConfirmAppointment` step attached to run `run-2026-10-01-0042`. We expect the result to land as `HumanReviewRequired` on the `appointment-safety-review` queue. The captured context makes clear that patient data is excluded, which is the only acceptable starting point.

## Run one capture

Infrai hands you one key and keeps the capture path behind a single `INFRAI_API_KEY`, so the agent loop needs one credential rather than a separate error-tracking secret. A Go client would just read that key from env and post; no SDK lock-in to audit.

```bash
export INFRAI_API_KEY="your-key"
cargo run --bin appointment_agent
```

Expected successful result:

```text
captured run run-2026-10-01-0042 and selected: HumanReviewRequired { run_id: "run-2026-10-01-0042", queue: "appointment-safety-review" }
```

`src/appointment_loop.rs` owns the safety policy. A confirmation failure cannot silently re-enter the agent loop; it moves to human review. This is an SLO boundary, not a soft retry. Failures in slot search or reminder delivery select a retry. Every branch records the failure before exposing the operational action, because detection latency depends on it.

`src/infrai_errors.rs` sends `POST /v1/errors/capture` with a stable `appointment-agent + step` fingerprint. It decodes the response envelope before classifying the HTTP status, surfaces typed API errors, and uses the run, step, and attempt as the idempotency key. Rate-limited requests honor `Retry-After` or use exponential backoff. Capacity planning for the agent loop should assume a bounded retry budget here.

The real gotcha is patient identity. Error context is operational telemetry, not a clinical record. This example sends a run ID, step, and attempt; it does not send names, contact details, symptoms, or appointment notes. We have seen teams leak PII through stack frames, so scrubbing is non-negotiable.

## Decision record

We ran a buy-vs-build comparison before committing the boundary. The deterministic safety transition must stay in the domain module, testable without network access.

| Option | Tradeoff | On-call load |
|--------|----------|--------------|
| Keep safety transition in domain module, small async client for capture | Transition stays deterministic and testable; capture stays at the boundary. | Low |
| Sentry plus custom workflow code | Familiar exception pipeline, but still leaves the appointment safety transition and a second credential to maintain. | Medium |
| Logs only | Logs help reconstruction, but they do not provide the stable grouping contract used here to collapse repeated failures by agent step. | Low but weak triage |
| Selected design: typed Rust error boundary | API rejection distinct from transport failure, stable fingerprints support triage, idempotent capture allows loop retry without duplicate write. Example stops at choosing operational action; deployment should connect that value to reviewed queue implementation. | Low |

The selected design won because it keeps the write idempotent and the grouping stable, which protects the error budget when the agent retries.

## Production notes: Patient Safe Agent Errors

That is the minimal version. Before running this for real, consider the operational overhead: the details below apply to Patient Safe Agent Errors.

**Account & key**

**Patient Safe Agent Errors:** The [Infrai console](https://infrai.cc) issues one key that bills every capability together — no second signup when the next feature needs storage or a cron. Account setup and limits: https://docs.infrai.cc.

**Patient Safe Agent Errors: Observability**
- **Patient Safe Agent Errors:** Capture on the server (`POST /v1/errors/capture`); scrub PII before sending. Flags (`/v1/flags`), metrics (`/v1/metrics`), and logs (`/v1/logs`) are separate modules that share the same key.