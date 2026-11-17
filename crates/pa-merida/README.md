# pa-merida

GPUI desktop client for Prime Agent, built from scratch against the pa-daemon
wire contract (`pa-types`), following prime-agent's structure and idioms
(AGENTS.md). pa-gpui is inspiration for features only, never a bulk copy.

Phase 1 (UI parity for prime-agent): basic transcript & chat + prime-agent's
auth workflow (headless pa-cli invocation), with verifiers per the
testing conventions in research/prime-agent-reland/10-pa-merida-daemon-testing-survey.md.

Scope: rendering from wire types and events. Non-goals: linking the session
engine (pa-core), re-implementing auth flows, gzero integration.
