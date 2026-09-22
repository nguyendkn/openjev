# Research: Jev (TypeSafe AI) Official API Contract

Scope: field-level API contract for `apps/server` design (switch-compat target). No code touched.

## 0. TL;DR — official docs DO exist (contra prior assumption)

Found official docs subdomain `docs.typesafe.ai` (API ref at `/api`, quickstart at
`/introduction/quickstart`) and OpenAPI 3.1.0 spec at `api.typesafe.ai/openapi.json` (confirmed:
"3.1.0 format, 2 operations, 16 schemas" per `github.com/api-evangelist/typesafe-ai` profiling repo).
**Caveat**: fetched via WebFetch's summarizing sub-model (HTML→markdown→small-model pass), not raw
bytes — code blocks below read as literal quotes (consistent JSON across 3 independent fetches:
`docs.typesafe.ai/api`, `openapi.json`, `quickstart`), prose descriptions are paraphrase-grade.
Treat JSON blocks as high-confidence, prose descriptions as medium-confidence. Raw `openapi.json`
fetch via GitHub raw mirror 404'd — did not find a byte-exact copy.

## 1. Endpoint

```
POST https://api.typesafe.ai/v1/systemone
Authorization: Bearer <API_KEY>
Content-Type: application/json
```

Secondary: `GET /v1/models` — lists available model names/aliases for the account.
Source: [docs.typesafe.ai/api](https://docs.typesafe.ai/api), [api.typesafe.ai/openapi.json](https://api.typesafe.ai/openapi.json).

## 2. Request schema (top-level, applies to ALL question types — single unified endpoint, not 3 separate ones)

| Field | Type | Required | Description | Example |
|---|---|---|---|---|
| `state` | string \| object \| array | Yes | "The content all questions in this request refer to." Plain string, or structured (chat log/record/app state). | `"I was charged twice. Please help."` or `{"subject":"Duplicate charge","message":"Please help."}` (openapi.json `examples`) |
| `model` | string | Yes | "Name or alias of the model to use." | `"jev-latest"` |
| `questions` | map<string, Question> | Yes, `minProperties: 1` | "A map of typed Question objects. You choose each key; answers come back under the same keys." | `{"urgency": {...}}` |

Source: [docs.typesafe.ai/api](https://docs.typesafe.ai/api) (table form), [api.typesafe.ai/openapi.json](https://api.typesafe.ai/openapi.json) (JSON schema, `required: ["model","questions","state"]`).

## 3. Question object — 3 primitive types (confirms prior `noul`/`choice`/`score` finding at field level)

Common to all 3: `type` (`"noul"|"choice"|"score"`, required), `instructions` (string\|object\|array, required — the question text/prompt).

| Type | Extra field | Type | Required | Constraint | Example (from quickstart curl) |
|---|---|---|---|---|---|
| `noul` | `criteria` | object `{true: string, false: string}` | optional | binary description pair | `{"type":"noul","instructions":"Does this message express urgency?"}` (criteria omitted — optional) |
| `choice` | `criteria` | map<string, string\|object\|array\|null> | **required** | up to 255 options (matches prior finding); value = option description | (not shown in fetched quickstart snippet, but matches JEV-CPU-Gemma4 `options:[{id,description}]` shape) |
| `score` | `criteria` | array<string\|object\|array> | **required** | 2–10 ordered levels — GitHub issue `typesafe-ai/skills#6` explicitly confirms "the API enforces at most 10 levels" (real user-filed bug against inconsistent OpenAPI max) | — |

Source: [docs.typesafe.ai/api](https://docs.typesafe.ai/api), corroborated by [github.com/typesafe-ai/skills issue #6](https://github.com/typesafe-ai/skills/issues/6) (score level cap = 10, real bug report — strong independent confirmation the `score` type is real and has this exact constraint).

**Verbatim quickstart request example** (docs.typesafe.ai/introduction/quickstart):
```json
{
  "state": "Hi, I've been trying to connect my Stripe account for 3 days...",
  "model": "jev-latest",
  "questions": {
    "urgency": {
      "type": "noul",
      "instructions": "Does this message express urgency?"
    }
  }
}
```

## 4. Response schema

| Field | Type | Required | Description |
|---|---|---|---|
| `model` | string | Yes | "Name of the model that answered the questions." |
| `answers` | map<string, Answer> | Yes, `minProperties: 1` | "Answers keyed by the question names supplied in the request." |
| `usage` | object `{input_tokens: int, output_tokens: int}` | Yes | token usage |

### Answer variants (discriminated by `type`, matches request question type)

| Type | Fields | Types | Example |
|---|---|---|---|
| `noul` | `type`, `noul` | `"noul"`, number 0–1 (0=no,1=yes) | `{"type":"noul","noul":1.0}` (verbatim, quickstart) |
| `choice` | `type`, `choice`, `probabilities`, `confidence` | `"choice"`, string (highest-prob option id), map<string,number> (sums to 1), number 0–1 | not directly quoted but field names confirmed by docs.typesafe.ai/api table |
| `score` | `type`, `score`, `legend`, `probabilities`, `confidence` | `"score"`, number (prob-weighted value), map<level-idx,description>, map<level-idx,number> (sums to 1), number 0–1 | — |

`confidence` field note: third-party sources (datacamp.com blog, pydantic.dev provider docs — NOT
official docs.typesafe.ai) describe it as "a margin, not a probability the answer is right," and
one (pydantic.dev) says it surfaces as `provider_details['confidence']` in their wrapper, not
necessarily the raw Jev field name — treat `confidence` as the field name per official docs table,
the "margin not probability" semantic as third-party interpretation, medium confidence.

Source: [docs.typesafe.ai/api](https://docs.typesafe.ai/api), [api.typesafe.ai/openapi.json](https://api.typesafe.ai/openapi.json).

## 5. Auth, errors, rate limits

- Auth: `Authorization: Bearer <API_KEY>` (Bearer token, static key from `console.typesafe.ai/settings/keys`).
- Error format found as a status-code table only (no error *body* JSON schema was surfaced by fetch):

| Status | Meaning (verbatim) |
|---|---|
| 401 | "Missing or invalid API key. Check the `Authorization` header." |
| 422 | "The request body failed validation — for example a missing required field or a malformed question." |
| 429 | "You have exceeded your rate limit. Back off and retry after a short delay." |
| 529 | "TypeSafe is temporarily overloaded. Retry after a short delay." |

- No explicit rate-limit *header names* (e.g. `X-RateLimit-Remaining`) were surfaced — guidance is
  prose only ("retry with exponential backoff"). **Unverified**: exact error body JSON shape (e.g.
  `{"error":{"type":...,"message":...}}`) was not captured by this pass.

Source: [docs.typesafe.ai/api](https://docs.typesafe.ai/api).

## 6. SDKs (strongest independent field-schema corroboration path, not deeply mined this pass)

- Python: `pip install typesafe-sdk` (`typesafe_sdk`), Python ≥3.10, reads `TYPESAFE_API_KEY` env, defaults to `jev-latest`.
- TS/JS: `npm install @typesafe-ai/sdk`.
- Both at v0.6.0 per one search-summarized source (medium confidence, not directly fetched).
- Claude Code plugin: `claude plugin marketplace add typesafe-ai/skills` → `claude plugin install typesafe@typesafe-ai` (quickstart page) — implies a `typesafe-ai/skills` GitHub org repo with MCP server + skill defs; not fetched this pass, likely the single richest ground-truth source (real TS/Python types) for a follow-up.

## 7. Confidence-gating / auto-act vs escalate (blog claim, field-level status)

Confirmed generically: every answer carries a `confidence` (0–1). Blog/marketing claim
("act autonomously when confidence high, escalate when uncertain") has **no single official
universal threshold** — one third-party (datacamp.com) cites example thresholds "under 0.45 → human,
auto floor 0.72, high-stakes 0.88" but these read as that author's own recommended policy, not a
TypeSafe-mandated constant or response field. No `threshold` or `escalate` field found in the
request/response schema itself — gating is caller-side logic on `confidence`.

## 8. Recommendation for `apps/server` field-naming (to maximize "drop-in switch" compatibility)

Given the schema above is real (2 independent official-domain fetches + 1 OpenAPI-labeled fetch,
mutually consistent, plus a real GitHub bug report corroborating the `score` 10-level cap), if
`openjev-rs`'s server wants a Jev user to swap the base URL only:

- Top-level request: use `state`, `model`, `questions` (not e.g. `input`/`prompt`/`items`).
- Question object: `type` (`noul`|`choice`|`score`), `instructions`, `criteria` — not `prompt`/`options`.
- Response: `model`, `answers`, `usage.input_tokens`/`usage.output_tokens`.
- Answer: `type` + type-named value field (`noul`/`choice`/`score`) + `probabilities` + `confidence`
  (choice/score only) + `legend` (score only) — not generic `value`/`result`.
- Endpoint path `/v1/systemone`, auth `Authorization: Bearer`, error statuses 401/422/429/529.

## Unresolved questions

1. WebFetch results here are AI-summarized page reads, not raw HTML/JSON bytes — could not get a
   byte-exact `openapi.json` (GitHub raw mirror attempt 404'd). If exact-to-the-character schema is
   needed before locking `apps/server`'s wire format, re-fetch with a raw HTTP client (curl) against
   `https://api.typesafe.ai/openapi.json` directly, or `gh repo clone typesafe-ai/skills` (not done —
   out of scope, this pass is research-only, no code/network-writes beyond WebFetch).
2. Error response *body* JSON shape not found (only status-code meanings).
3. Rate-limit header names not found (only 429 prose guidance).
4. `typesafe-ai/skills` GitHub org repo (real, referenced by 2 sources incl. a live issue #6) not
   directly read this pass — likely contains the actual OpenAPI YAML/JSON + MCP server + SDK type
   defs; best next lead for byte-exact field types if this research continues.
5. SDK version "0.6.0" and package existence (`typesafe-sdk`, `@typesafe-ai/sdk`) came from
   WebSearch's synthesized answer, not a direct PyPI/npm page fetch — not independently confirmed
   by visiting pypi.org/npmjs.com directly.
6. `console.typesafe.ai/settings/keys` and playground existence stated by search snippets only, not
   fetched (likely requires login, per prior research pass).
7. Confidence-threshold recommendations (0.45/0.72/0.88) are one blogger's opinion, not official —
   do not treat as a spec constant.
