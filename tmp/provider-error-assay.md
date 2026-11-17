# Provider Error-Surface Assay (read-only, no edits)

Scope: `crates/pa-ai/src/providers/*` + `crates/pa-core/src/session_engine/`
(auto_retry / provider_retry / provider_failover / provider_park).
Repo root: `/data/projects/explorations/nursery/prime-agent`.
Date: 2026-11-17.

## How the error pipeline works (short)

1. Each provider turns a non-OK HTTP response into `ProviderError::Http`
   (`pa-ai/src/utils_inner/stream_failure.rs:412` `from_http_status_body`,
   called by every provider at its `status >= 400` gate).
2. `classify_stream_failure` (`stream_failure.rs:527`) maps `error.type` /
   `error.code` text + status → `StreamFailureKind` (refusal, safety,
   payment_required 402, overloaded (incl. 529), rate_limit (incl. 429), auth
   (401), permission (403), invalid_request (400/404), malformed, server_error
   (>=500), unknown).
3. The provider records a `provider_stream_failure` diagnostic with
   `{kind, providerErrorType, status, retryAfterMs, requestId, raw}`
   (`stream_failure.rs:697-712` `extract_parts_from_http`, `record_stream_failure`).
4. The engine's retry policy reads only the diagnostic's
   kind/status/retryAfterMs (`pa-core/src/session_engine/provider_retry.rs:100-128`),
   never the message text (except the tool-router 404 marker, `provider_retry.rs:84-96`).
5. Two retry drivers share one policy: the turn loop
   `run_turn_with_auto_retry` (`auto_retry.rs:64`, called via
   `provider_failover.rs:119`) and the one-shot helper
   `complete_with_provider_retry` (`provider_retry.rs:218`) used by
   compaction/refine/side-question arms.

## Table: provider x error class x current handling x recommended handling

| Provider | HTTP statuses surfaced | Body shapes recognized | Current retry handling | Recommended |
|---|---|---|---|---|
| openai-completions (incl. cf/workers-ai, openrouter, groq, deepseek, zai, moonshot, prime-inference) | any 4xx/5xx → `openai_http_error` (`openai_completions/errors.rs:40-66`, message composed SDK-style at `:14-37`) | `error.message` / `error.type` / `error.code` (nested `error.error` too, `stream_failure.rs:695-710`); OpenRouter `error.metadata.raw` appended (`errors.rs:71-95`) | status-based: 429/5xx transient, 400/404 permanent, 401 one retry (`provider_retry.rs:142-157`); Retry-After header honored generically | OK. GAP: cloudflare returns PLAIN-TEXT bodies ("rate limit exceeded" style, no JSON `error` object) → body_type None, but status 429 still classifies via `stream_failure.rs:552`; fine. No fix needed beyond keeping status primary. |
| openai-responses | any 4xx/5xx via `from_http_status_body` (`openai_responses.rs:330-337`) | same shared shapes; x-request-id kept (`openai_responses.rs:328`) | shared policy | OK |
| azure-openai-responses | any 4xx/5xx (`azure_openai_responses.rs:370-377`) | shared shapes; x-request-id (`:368`) | shared policy | OK. Azure throttling (429, `error.code=429` numeric) falls through to status → RateLimit. OK |
| openai-codex-responses (WS + HTTP) | status in `CodexApiError` (`openai_codex_responses/errors.rs:16-22`) | `error.code`/`error.type`/`plan_type`/`resets_at` (`errors.rs:248-257`); usage-limit friendly message + `resets_at`→retryAfterMs (`errors.rs:259-300`); max(Retry-After, resets_at) rule (`errors.rs:337-340`); stale-continuation `previous_response_not_found` resent in full (`errors.rs:226-238`); WS close codes 1002/1005/1006/1009 (`errors.rs:98-108`) | shared policy; usage_limit → RateLimit → transient + quota-park eligible | OK. GAP: `resets_at` can exceed `maxRetryDelayMs` (60s cap, `provider_retry.rs:34`) → `ExceedsCap` give-up instead of park, UNLESS kind is exactly `rate_limit` (park seam `provider_park.rs:125-127`). Codex usage limits DO map to `rate_limit` via regex (`errors.rs:302-306`), so they park. OK — verified chain. |
| anthropic | any 4xx/5xx (`anthropic/stream.rs:264-271`); 529 overloaded via status (`stream_failure.rs:547`) | `error.type` (e.g. `overloaded_error`, `rate_limit_error`, `invalid_request_error`, `authentication_error`), request-id header (`stream.rs:258-262`); mid-stream `refusal` stop_reason → failure (`stream.rs:680`) | Retry-After (seconds) parsed generically (`utils_inner/stream_failure/http_retry.rs:17-40`); overloaded/rate_limit transient; auth one retry | OK |
| google / google-vertex | any 4xx/5xx (`google.rs:304-317`, `google_vertex.rs:361-374`); body then BLANKED (`sdk_name="ApiError"`, `body=None`) to mirror the TS genai SDK numeric `code` | ONLY status + class name `ApiError`; the real `error.status` string ("RESOURCE_EXHAUSTED", "UNAVAILABLE") is deliberately discarded (`google.rs:306-315`) | status-decided only: 429/5xx transient, 400/404 permanent | QUIRK: google text-keyed classes (safety/recitation regex, `stream_failure.rs:540-543`) never fire on the HTTP path (body dropped); mid-stream SAFETY still caught via stop_reason (`google.rs:364`). Acceptable; do NOT "restore" the body — TS parity is deliberate. |
| bedrock (http1 + h2c) | status deliberately DROPPED: `status: None` in `bedrock_http_error` (`bedrock/mod.rs:161-165`) to mirror the AWS SDK | `__type` / `errorType` / `x-amzn-errortype` → exception name; `message`/`Message`; known prefixes (`mod.rs:93-99`): ThrottlingException, ServiceUnavailableException, InternalServerException, ValidationException, ModelStreamErrorException | classification by exception NAME only (sdk_name): `ThrottlingException`→rate_limit (test `mod.rs:872-881`), `ServiceUnavailableException`→server_error; h2 GOAWAY/RST_STREAM → connection errors (`h2.rs`, `goaway.rs`) | QUIRK/RISK: `InternalServerException` matches NO pattern in `classify_stream_failure` (contains "server" but not "server_error") → kind=Unknown → still retried (default non-permanent) but NOT via the labeled server_error path, and status=None means `is_permanent_provider_failure_kind` status rules (404 etc.) are inert. Also Retry-After headers are discarded for bedrock (`headers: HashMap::default()` at `mod.rs:167`). Lowest-hanging fix. |
| mistral | any 4xx/5xx (`mistral/stream.rs:194-222`), body truncated to a cap | `SDKError` class name; body text as detail; mid-stream SAFETY stop (`stream.rs:354`) | shared policy; message "Mistral API error (429): ..." → status decides | OK |
| faux (test provider) | n/a | "No more faux responses queued" excluded from retry + disclosure (`provider_retry.rs:58-64`) | never retried | OK |

## What auto_retry already handles / swallows / panics

- HANDLES: exponential backoff base 2s, retries=3, unbounded ceiling
  (`provider_retry.rs:26-34`); ±20% jitter (`:159-189`); Retry-After /
  Retry-After-Ms / HTTP-date (seconds + date forms, `http_retry.rs:17-40`);
  server-wait cap 60s with give-up (`provider_retry.rs:191-206`) and quota-park
  seam for rate_limit waits over the cap (`auto_retry.rs:141-170`,
  `provider_park.rs:125-127`); failover to backup provider
  (`provider_failover.rs:80-119`); abort at every rung.
- PERMANENT (fail fast, no retry): `invalid_request` (except 404),
  `refusal`, `permission`, `safety`, `payment_required` (402) — first attempt;
  `auth` — one retry, then stale-markable (`provider_retry.rs:142-157`).
- NEVER RETRIED (not provider errors): `agent_lifecycle_failure` diagnostic,
  faux queue exhaustion, unsupported-tool 404+marker, context overflow
  (owned by compact-and-retry; `provider_retry.rs:51-96`,
  `auto_retry.rs:87-96`).
- SWALLOWS (no retry, no disclosure unless a provider_stream_failure
  diagnostic exists): the SANCTIONED DIVERGENCE at `auto_retry.rs:100-120` —
  attempt-0 failures without the diagnostic settle silently by design.
- PANICS: none in the retry path. Providers only `expect("static regex")` on
  compile-time literals (`stream_failure.rs:542,551,558`).
- Telemetry-only classifier: `error_classify.rs` maps message text →
  (category, subtype, retryable) for upload sanitization; it is NOT the retry
  decision-maker (retry uses the diagnostic). `subtype_retryable`
  (`error_classify.rs:144-157`) counts `malformed_response` and
  `insufficient_balance` retryable — inconsistent with
  `is_permanent_provider_failure_kind` (both permanent there). Cosmetic only.

## Transient vs PERMANENT (verified verdicts)

- TRANSIENT (retry w/ backoff): 429 rate_limit, 5xx server_error, 529
  overloaded, 408-timeout/connection errors (kind=None → retried by default),
  stream_drop (test-pinned retryable at every rung, `provider_retry.rs:803-812`),
  invalid_request+404 (routing blip, `provider_retry.rs:144-146`).
- PERMANENT: 400 invalid_request, 402 payment_required, 403 permission,
  refusal, safety, unsupported-tool 404, auth after 1 retry, context overflow
  (compact-and-retry owns it).

## Priority order (what to implement after 429)

1. Bedrock Retry-After + status preservation: `bedrock_http_error`
   (`bedrock/mod.rs:161-172`) discards headers and status; pass
   `retry_after_ms` from `parse_retry_after_ms(&headers)` and consider keeping
   status for status-based verdicts.
2. `InternalServerException` unrecognized: widen `classify_stream_failure`'s
   server_error pattern (e.g. `internal.?server`) or map bedrock exception
   names to kinds directly.
3. Anthropic 529 `overloaded_error`: covered, but confirm overload responses
   carry Retry-After (they usually don't) — jittered ladder already applies.
4. Codex `resets_at` > cap: verified safe (parks as rate_limit), but add a
   test pinning kind=rate_limit for `usage_limit_reached` so the park seam
   can't regress.
5. Malformed/SSE-parse errors (e.g. `google.rs:333-335` "Could not parse
   Gemini SSE chunk") become `ProviderError::Message` → classified Unknown →
   retried silently 3x with no diagnostic; consider a `malformed_response`
   kind for parity with `error_classify.rs` (`malformed_response` is
   retryable there).
6. Error-classifier/ retry-policy verdict mismatch (`insufficient_balance`,
   `malformed_response`) in `error_classify.rs:144-157` — align or document.
