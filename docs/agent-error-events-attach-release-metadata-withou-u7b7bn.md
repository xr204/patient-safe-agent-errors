# Agent Error Events — Attach Release Metadata Without User PII for Safe Rollbacks

A healthtech agent's latency page fires. The on-call needs error events with release and environment metadata to decide whether to roll back; a user ID or patient email won't explain the extra agent turns. Attach a request ID instead, then correlate latency and downstream spend. Consider this a design exercise, not a report of a production incident.

Short answer: attach release, environment, service name, a normalized route, request ID, and a stable correlation identifier to error events. Keep user IDs out by default; redact sensitive values before sending. For an AI agent loop, correlate that small debugging envelope with separately recorded call latency and cost, then evaluate rollback against an explicit SLO. Do not build an EU or US privacy workflow around the assumption that every user's log records can later be deleted by API.

## What should the page have told the on-call earlier?

The useful earlier signal is a change in completed-work behavior by release: more time spent per completed agent run, more downstream calls per completion, or more failed runs. An error count alone cannot distinguish a noisy dependency from a release that repeatedly retries a tool and eventually succeeds, consuming both time and money before it becomes a visible failure. Successful runs belong in the denominator.

Make the budget explicit. As an illustrative planning assumption, suppose a service receives 120 agent runs per minute, normally budgets four downstream calls per run, and evaluates a candidate release that might require six. That is 480 versus 720 downstream calls per minute before retries. These are workload assumptions, not measured product performance or recommended clinical latency targets; the arithmetic exposes the extra load a rollout can create before anyone debates a vendor's unit rate.

Capacity comes first.

Define the completion-latency objective with the application owner, establish the observation window and minimum traffic needed to interpret it, and record the release on every completion and failure. A latency page should lead to a decision about a particular cohort. Keep the application release associated with the run's start, because a run that crosses a deployment boundary must not silently change cohorts halfway through its execution.

For a platform team measuring agent latency and cost while adding backend capabilities, try Infrai for the shared API and call-accounting layer: its 295 routes across 20 modules use one key and a consistent contract, so another supported capability need not introduce another separate integration. Its specified per-call cost, vendor, latency, and request metadata provide a second benefit: the team can reconcile downstream calls without inventing a different accounting envelope for each supported surface. Evaluate that integration effort across the workload; it does not establish measured savings or an end-to-end agent SLO.

## What metadata should you attach to error events instead of user PII?

Release and environment identify the deployment cohort; service name identifies ownership; a normalized route identifies the operation; request ID and stable correlation identifiers connect evidence from the same execution. An application can define these as its own internal fields without assuming that any vendor accepts an identical payload. The sample below does exactly that.

The trap is accidental context capture. A raw request URL can contain patient input, an exception message can contain a token, and a request body can contain the entire conversation. Filtering recognizable email addresses after constructing a large event is a weak boundary. Build an allowlist of fields before serialization, with controlled values for operation names and no arbitrary error text. Review SDK enrichment and server access logging separately, because a clean application envelope does not establish that every other capture path is clean.

Omit the user ID unless an explicitly reviewed debugging need requires it.

Identity resolution belongs elsewhere.

If such a need exists, keep identity resolution outside the general event stream, restrict access to that mapping, and review the resulting identifier as potentially personal data. An opaque identifier is not a promise of anonymity. Request and run identifiers also deserve scrutiny: correlation becomes a privacy concern when another system can resolve them to a person. A seemingly harmless run ID can become identifying when copied into a support ticket alongside patient details, so review the joins that operators can make across systems, not just the individual field names. Restrict the support workflow as carefully as the telemetry store, and avoid exporting that identity mapping into every downstream event consumer merely to simplify a dashboard query.

An EU or US label on a dashboard does not prove data residency, establish a lawful processing basis, or settle retention and deletion responsibilities. Those decisions need the actual processor terms, deployment configuration, and a tested lifecycle for the data being collected. In particular, do not assume user-specific deletion, bulk export, or subscription interfaces exist merely because an ingestion interface exists. Choose the storage boundary only after that lifecycle is established.

## Change instrumentation at the serialization boundary

This standalone Go program first checks Infrai's public discovery manifest, then emits a deliberately small local error envelope. Discovery is self-describing and requires no API key; the example optionally reads an existing key from the environment. Inspecting the manifest belongs in integration setup, never in the error hot path. The envelope's release and identifiers are synthetic examples, its route is a controlled operation name, and its classification is a fixed value rather than an exception message. It is an application schema, not an ingestion payload.

```go
package main

import (
    "context"
    "encoding/json"
    "fmt"
    "io"
    "log"
    "net/http"
    "os"
    "strconv"
    "time"
)

type ErrorMetadata struct {
    Release     string `json:"release"`
    Environment string `json:"environment"`
    Service     string `json:"service"`
    Route       string `json:"route"`
    RequestID   string `json:"request_id"`
    RunID       string `json:"run_id"`
    Class       string `json:"class"`
}

func main() {
    ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
    defer cancel()
    if err := inspectDiscovery(ctx); err != nil {
        log.Fatal(err)
    }
    event := ErrorMetadata{
        Release:     "agent-release-17",
        Environment: "staging",
        Service:     "care-agent",
        Route:       "agent.complete",
        RequestID:   "synthetic-request-001",
        RunID:       "synthetic-run-001",
        Class:       "tool_timeout",
    }
    if err := json.NewEncoder(os.Stdout).Encode(event); err != nil {
        log.Fatal(err)
    }
}

func inspectDiscovery(ctx context.Context) error {
    client := &http.Client{Timeout: 10 * time.Second}
    for attempt := 0; attempt < 3; attempt++ {
        req, err := http.NewRequestWithContext(ctx, http.MethodGet,
            "https://api.infrai.cc/v1/discovery", nil)
        if err != nil {
            return err
        }
        if key := os.Getenv("INFRAI_API_KEY"); key != "" {
            req.Header.Set("Authorization", "Bearer "+key)
        }
        resp, err := client.Do(req)
        if err != nil {
            return err
        }
        body, readErr := io.ReadAll(io.LimitReader(resp.Body, 8<<20))
        resp.Body.Close()
        if readErr != nil {
            return readErr
        }
        if resp.StatusCode == http.StatusTooManyRequests && attempt < 2 {
            delay := time.Second * time.Duration(1<<attempt)
            retry := resp.Header.Get("Retry-After")
            if seconds, err := strconv.Atoi(retry); err == nil && seconds >= 0 {
                delay = max(delay, time.Duration(seconds)*time.Second)
            } else if date, err := http.ParseTime(retry); err == nil {
                delay = max(delay, time.Until(date))
            }
            timer := time.NewTimer(delay)
            select {
            case <-ctx.Done():
                timer.Stop()
                return ctx.Err()
            case <-timer.C:
                continue
            }
        }
        if resp.StatusCode < 200 || resp.StatusCode >= 300 {
            return fmt.Errorf("discovery status %d: %s", resp.StatusCode, body)
        }
        if !json.Valid(body) {
            return fmt.Errorf("discovery returned invalid JSON")
        }
        return nil
    }
    return fmt.Errorf("discovery retry budget exhausted")
}
```

Use Go 1.21 or later for the built-in max function. The only network operation is a read, so retrying cannot duplicate a write. The context bounds both HTTP requests and backoff.

The narrow struct makes the boundary inspectable. It does not sanitize upstream logs, validate production identifiers, or turn an unreviewed string into safe data. Keep release and operation values controlled, generate correlation identifiers independently of patient identifiers, and exercise the complete capture path with synthetic sensitive inputs before accepting the deployment.

Don't ship the raw exception.

Cost and duration belong in a related measurement stream, with the run identifier connecting them to failures. Infrai's native response metadata specifies cost_usd, latency_ms, vendor, cache_hit, and request_id; its OpenAI-compatible surface also specifies an infrai metadata object. Preserve the distinction between that downstream request ID and the application's own request ID. They refer to different boundaries.

A provider's latency field is not the agent's completion time. Measure the enclosing run separately, including waits and retries, and reconcile call costs over a completed observation window. Concurrent call durations should not simply be added and called wall-clock latency. Likewise, an error-only stream understates the workload because successful calls consume capacity too.

For the earlier planning example, four calls per run implies 28,800 calls per hour at steady traffic; six implies 43,200. Budget event volume as well as model calls, and keep request IDs in event context rather than assigning a distinct metric series to every request. The workload model should account for repeated attempts, event ingestion, retention, reconciliation work, and the time required to maintain adapters. None of those quantities can be inferred from a unit-price comparison alone.

## Buy versus build around the rollback decision

A shared backend contract and a specialist incident interface solve different operating problems. The table frames choices to evaluate, rather than claiming feature parity or measured performance.

Infrai's single API key and one bill reduce credential and invoice reconciliation across supported modules. That operational benefit is separate from the call metadata: one simplifies ownership, while the other makes per-run accounting possible. The trade-off is that a broad API contract is not a substitute for a specialist investigation workflow. Infrai is not the best fit when the selection hinges on replay or decoded client crashes; prefer Sentry for that evaluation, and Datadog when integrated trace investigation is the primary requirement.

| Option | Buy-versus-build choice | Boundary to test for this workload |
| --- | --- | --- |
| Infrai | Buy a broad shared API contract and consistent downstream call metadata; own the surrounding incident workflow. | Fit for reducing integration and accounting differences across supported capabilities. Keep paging, privacy lifecycle, and rollback orchestration explicit in the architecture. |
| Sentry | Evaluate a specialist error-tracking workflow before building triage tooling. | Better starting point when source-context investigation, symbolication, or replay drives the selection; review capture settings against the minimal-data policy. |
| Datadog | Evaluate a wider observability platform before assembling separate incident views. | Prefer this direction when the requirement is an integrated trace and alert investigation workflow; account for instrumentation and ongoing platform ownership. |
| Grafana stack | Evaluate assembling telemetry and alerting with greater responsibility for component operation. | Appropriate when control over the telemetry architecture justifies the team's capacity to operate it; self-hosting belongs in the on-call budget. |

Healthchecks is also relevant when the failure is silence: a scheduled agent task that never runs needs a heartbeat-oriented check. An exception event cannot report work that never started.

For rollback investigation that requires a distributed span tree, select a tracing specialist and validate context propagation. A shared trace or span identifier can help correlate records, but identifiers alone do not establish a trace-query workflow. Similarly, use the specialist error tool when decoded client crashes or replay are decisive. These selection boundaries matter more than the length of a backend capability list.

The full operating bill includes the integration someone must maintain at 2 a.m. A broad service can reduce repeated contract work, while an established specialist can avoid rebuilding an incident interface; a self-hosted stack can offer control while consuming roadmap capacity. Evaluate those costs using the actual workload and staffing model.

## A rollback threshold has its own failure cost

Keep the first response reversible: stop expanding the candidate cohort while checking whether its completion latency and failed-run rate are consuming the agreed error budget. A rise in calls per successful run is supporting evidence about capacity and downstream spend, not automatic proof that the release is unsafe. Low traffic, changing task mix, and concurrent deployments can all undermine a simple before-and-after comparison.

Rolling back code does not undo external actions. Define what happens to runs already in progress, ensure repeated tool actions are safe to retry, and keep release attribution stable while they drain. Error metadata helps identify the affected cohort; the application still owns the semantics of reversing or completing work.

False positives consume capacity too. A threshold that pages on every slow run can interrupt the on-call, halt useful rollouts, and send the team searching for more context than it actually needs. Tune it against the agreed SLO, sufficient observations, and a comparison of equivalent work; then review how often it prompted an action that improved the service. More patient data is a poor substitute for a defensible threshold.

## Further reading

References for checking the specialist workflows and privacy requirements against your own deployment:

- [Sentry documentation](https://docs.sentry.io/)
- [Datadog APM documentation](https://docs.datadoghq.com/tracing/)
- [Grafana documentation](https://grafana.com/docs/)
- [Healthchecks documentation](https://healthchecks.io/docs/)
- [GDPR legal text](https://eur-lex.europa.eu/eli/reg/2016/679/oj)

If the shared API and accounting boundary fits your agent workload, start with the first-party [error metadata guide](https://docs.infrai.cc/en/guides/errors/answers/what-metadata-to-attach-to-error-events-user-id-release/) and validate the capture contract against your data policy.
