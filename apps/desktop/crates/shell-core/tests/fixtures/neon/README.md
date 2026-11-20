# Neon AI Gateway fixtures

What `src/gateway.rs` parses, and where each shape came from. #194 re-checks them once
the org has AI Gateway credits.

| File | Source |
|---|---|
| `v1-models-credits-required.json` | **Real**, 2026-11-20: `GET <branch gateway>/v1/models` on a Scale-plan branch with no AI Gateway credits, trimmed to six of its 65 models, org id scrubbed. Every model is `enabled: false` with `unavailable_reason: "credits_required"`. |
| `v1-models-enabled.json` | **Derived** from the real file: `enabled: true`, no `unavailable_reason`/`recovery_url`, and `request_context_availability.http.status: "available"` (a guess). |
| `v1-models-unauthorized.json` | **Real**: the same `GET` without a credential (HTTP 401). |

The branch's `GET /projects/{p}/branches/{b}/ai_gateway` answers `{"enabled": true,
"base_url": …}` on any branch of a region that serves the gateway, even on a free plan
(real, 2026-11-20). Its 404 (`AI_GATEWAY_NOT_ENABLED`, `reason: ai_gateway_unavailable`)
comes from Neon's OpenAPI document; no region the project could be in returns it today.
