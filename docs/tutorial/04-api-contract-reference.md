# Jev API Contract Reference — Official Schema

This chapter documents the **official Jev REST API** as documented at docs.typesafe.ai (verified via fetches, see sources). This is the contract you must follow if you want to build a drop-in-compatible endpoint or integrate with TypeSafe's ecosystem.

**Caveat**: Fetched via AI summarization (not raw bytes), so prose descriptions are medium-confidence. JSON field names and structure are high-confidence (multiple independent fetches agreed). Verify exact field names and response shapes in your implementation against live API calls before shipping.

---

## Endpoint

```
POST https://api.typesafe.ai/v1/systemone
Authorization: Bearer <API_KEY>
Content-Type: application/json
```

Secondary endpoint:
```
GET https://api.typesafe.ai/v1/models
```
Returns: list of available model names/aliases (e.g., `["jev-latest", "jev-2024-q3", ...]`).

---

## Request Schema

### Top-Level Structure

```json
{
  "state": "...",
  "model": "jev-latest",
  "questions": {
    "question_name_1": { ... },
    "question_name_2": { ... }
  }
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `state` | string \| object \| array | **Yes** | The context all questions refer to. Can be plain text, structured JSON, or a chat log. |
| `model` | string | **Yes** | Model name/alias, e.g., `"jev-latest"`. |
| `questions` | object (map) | **Yes** | Dictionary of Question objects, keyed by names you choose. Answer keys match question keys. Min 1 question. |

### state Field (Examples)

```json
// Plain text
"state": "I was charged twice for order #4471 and need this reversed today."

// Structured object
"state": {
  "subject": "Duplicate charge",
  "message": "Please help, I was charged twice.",
  "order_id": "4471"
}

// Chat log (array)
"state": [
  {"role": "user", "content": "How do I reset my password?"},
  {"role": "assistant", "content": "You can reset it here: ..."},
  {"role": "user", "content": "That didn't work, still locked out."}
]
```

### Question Object — 3 Primitive Types

All questions have:
- `type` (required): `"noul"` | `"choice"` | `"score"`
- `instructions` (required): string or object describing the question

Then, one type-specific field:

#### Type: `noul` (Binary)

```json
{
  "type": "noul",
  "instructions": "Does this message express urgency?",
  "criteria": {
    "true": "The message indicates time sensitivity or pressure.",
    "false": "The message does not emphasize urgency."
  }
}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `criteria` | object `{true: string, false: string}` | Optional | Definitions of true/false; can be omitted. |

#### Type: `choice` (Multi-way, 2-255 options)

```json
{
  "type": "choice",
  "instructions": "Route to the correct support queue:",
  "criteria": {
    "billing": "Billing, payments, refunds",
    "technical": "Product bugs, feature issues",
    "account": "Login, password, account access",
    "spam": "Off-topic or spam"
  }
}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `criteria` | map<string, string \| object \| array> | **Yes** | 2-255 options. Keys are option IDs; values are descriptions. |

#### Type: `score` (Ordered Rubric, 2-10 levels)

```json
{
  "type": "score",
  "instructions": "Incident severity:",
  "criteria": [
    "sev4 — Low, internal tool, business hours OK",
    "sev3 — Medium, affects some users",
    "sev2 — High, critical path affected",
    "sev1 — Catastrophic, revenue loss, page on-call"
  ]
}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `criteria` | array<string \| object \| array> | **Yes** | 2-10 ordered levels. Index 0 = lowest, index (n-1) = highest. |

**Caveat from field research**: GitHub issue typesafe-ai/skills#6 filed by a user confirms the API enforces a **max of 10 levels** for `score` type. No hardcoded min/max length for `choice` options was found in the OpenAPI spec, but 2-255 is stated as the practical range.

---

## Request Example (Quickstart)

From docs.typesafe.ai/introduction/quickstart:

```json
{
  "state": "Hi, I've been trying to connect my Stripe account for 3 days, and it's not working. I'm losing sales.",
  "model": "jev-latest",
  "questions": {
    "urgency": {
      "type": "noul",
      "instructions": "Does this message express urgency?"
    },
    "sentiment": {
      "type": "choice",
      "instructions": "Customer sentiment:",
      "criteria": {
        "positive": "Happy, satisfied",
        "neutral": "Factual, no emotion",
        "negative": "Frustrated, angry"
      }
    }
  }
}
```

---

## Response Schema

### Top-Level Structure

```json
{
  "model": "jev-latest",
  "answers": {
    "question_name_1": { ... },
    "question_name_2": { ... }
  },
  "usage": {
    "input_tokens": 125,
    "output_tokens": 30
  }
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `model` | string | **Yes** | Name of the model that processed the request. |
| `answers` | object (map) | **Yes** | Answers keyed by question names from the request. |
| `usage` | object | **Yes** | Token counts (for billing). |

### Answer Objects — Per-Type

All answers have `type` field matching the question. Then:

#### Answer: `noul` (Binary)

```json
{
  "type": "noul",
  "noul": 0.87
}
```

| Field | Type | Value range |
|-------|------|-------------|
| `noul` | number | [0.0, 1.0] — 0=no, 1=yes. |

No `confidence` field for binary (the single number is the confidence level directly).

#### Answer: `choice` (Multi-way)

```json
{
  "type": "choice",
  "choice": "billing",
  "probabilities": {
    "billing": 0.71,
    "technical": 0.18,
    "account": 0.09,
    "spam": 0.02
  },
  "confidence": 0.71
}
```

| Field | Type | Notes |
|-------|------|-------|
| `choice` | string | Highest-probability option ID. |
| `probabilities` | map<string, number> | Distribution over all options, sums to 1.0 (within float precision). |
| `confidence` | number, [0, 1] | Confidence score. Per third-party sources: a "margin" between top choice and alternatives, not a true probability that the answer is *correct*. |

#### Answer: `score` (Ordered Rubric)

```json
{
  "type": "score",
  "score": 3.2,
  "legend": {
    "0": "sev4 — Low, business hours OK",
    "1": "sev3 — Medium, some users",
    "2": "sev2 — High, critical path",
    "3": "sev1 — Catastrophic"
  },
  "probabilities": {
    "0": 0.05,
    "1": 0.15,
    "2": 0.40,
    "3": 0.40
  },
  "confidence": 0.40
}
```

| Field | Type | Notes |
|-------|------|-------|
| `score` | number | Probability-weighted expected value (e.g., 3.2 = expected sev between sev1 and sev2). |
| `legend` | map<index, string> | Maps numeric indices (0, 1, 2, ...) to level descriptions. |
| `probabilities` | map<index, number> | Distribution over levels (0 = lowest, n-1 = highest). |
| `confidence` | number, [0, 1] | Confidence (same semantic as `choice`). |

---

## Response Example (Quickstart)

For the request above:

```json
{
  "model": "jev-latest",
  "answers": {
    "urgency": {
      "type": "noul",
      "noul": 0.95
    },
    "sentiment": {
      "type": "choice",
      "choice": "negative",
      "probabilities": {
        "positive": 0.05,
        "neutral": 0.10,
        "negative": 0.85
      },
      "confidence": 0.85
    }
  },
  "usage": {
    "input_tokens": 78,
    "output_tokens": 12
  }
}
```

---

## Authentication & Errors

### Authentication

```bash
curl -X POST https://api.typesafe.ai/v1/systemone \
  -H "Authorization: Bearer $TYPESAFE_API_KEY" \
  -H "Content-Type: application/json" \
  -d @request.json
```

API key: static token from `console.typesafe.ai/settings/keys` (requires login).

### Error Responses

| Status | Meaning |
|--------|---------|
| **401** | Missing or invalid API key. Check the `Authorization` header. |
| **422** | Request body failed validation (missing required field, malformed question, etc.). |
| **429** | Rate limit exceeded. Back off and retry with exponential backoff. |
| **529** | TypeSafe is overloaded. Retry after a short delay. |

**Caveat**: Error response *body* JSON shape was not captured in this research (docs.typesafe.ai only showed status-code meanings, not error JSON structure). Implement retry logic for 429/529; for 422, validate your request locally before retrying.

---

## Confidence & Auto-Gating

The `confidence` field enables autonomous decision-making with graceful fallback:

```
confidence ≥ 0.88: auto-execute (high certainty)
0.45 ≤ confidence < 0.88: escalate (uncertain, flag for review)
confidence < 0.45: reject (very uncertain, always escalate)
```

These thresholds are **not official Jev constants** — they're from one blogger's published policy (datacamp.com). Best practice:

1. **Measure confidence distribution on your own test set.**
2. **Pick thresholds that match your risk tolerance** (higher threshold = more escalations but fewer mistakes).
3. **Monitor calibration**: does 85% confidence actually mean 85% accuracy? If not, recalibrate.

---

## Key Design Properties (Implied by the Contract)

1. **Single unified endpoint** (`/v1/systemone`) for all question types — no separate endpoints for noul/choice/score.
2. **Batch multi-question in one request** — efficient for parallel decisions.
3. **Options supplied at request time** — no retraining needed for new decision spaces.
4. **Probabilities sum to 1.0** — proper probability distribution, can be used directly for calibration/gating.
5. **No streaming** — request/response are atomic; all answers returned at once.
6. **Token counting included** — for billing; treat like standard LLM APIs.

---

## Implementing a Drop-In-Compatible Endpoint

If you're building an openjev-rs server or any other typed-decision inference system and want Jev users to swap endpoints with minimal code changes:

1. **Implement `/v1/systemone` as your primary endpoint.**
2. **Accept the exact request schema above** (state, model, questions).
3. **Return the exact answer schema** (type, noul/choice/score fields, probabilities, confidence).
4. **Return a 422 for malformed requests** (missing required fields, score >10 levels, etc.).
5. **Support `/v1/models` or document which model names you support** (helps clients know what to request).

openjev-rs's implementation lives in `apps/server/src/app.rs` (see run.md), but its `/bench` endpoint is a superset (returns timing details and all 3 methods — constrained-readout, generation, Laya). A future `/v1/systemone` compatibility endpoint could route requests to one of these methods under the hood.

---

## Sources

- `plans/20260922-2146-openjev-rust-implementation/research/researcher-06-jev-api-contract.md` § 0-8 — complete OpenAPI fetch + official docs + schema tables + example requests/responses.
- `docs.typesafe.ai/api` — official API reference (fetched, summarized).
- `docs.typesafe.ai/introduction/quickstart` — official quickstart example.
- `api.typesafe.ai/openapi.json` — OpenAPI 3.1.0 spec (fetch summarized, not raw bytes).
- `github.com/typesafe-ai/skills/issues/6` — real user-filed bug confirming score 10-level limit (external evidence of API constraint).

