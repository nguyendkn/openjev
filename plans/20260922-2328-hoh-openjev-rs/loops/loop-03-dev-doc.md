---
loop: 3
status: pending
preservation_constraints:
  - "cargo build --workspace (debug AND release) PASSES on Linux server"
  - "cargo test --workspace PASSES on Linux server"
  - "apps/cli continues to produce correct BenchReport JSON for Qwen3-0.6B (readout + generate)"
  - "reset_context still called between readout and generate"
  - "6-crate workspace structure, Timings 7 fields, 4 cached models, ggmlc-run binary all intact"
  - "G3/G4/G5 stay closed — do not reintroduce guessed API usage"
---

## Objective
`apps/server` (axum HTTP) reachable via real `curl` on `103.146.166.46` — DoD's core requirement.

This is the single highest-priority remaining DoD item: the user's Definition of Done requires
`curl http://103.146.166.46:<port>/health` and `POST .../bench` to work from outside. Nothing
in Loops 1-2 touched `apps/server` yet. Close that gap now, reusing Loop 2's proven
`engine`/`models`/`pipeline` for Qwen3-0.6B (Laya and the other 2 LLM models stay out of scope
this loop — narrow to "one model works end-to-end over HTTP" before widening).

## Tasks
1. **Implement `apps/server`**: axum app per `plan.md` § Architecture — `AppState` (lazy-load +
   cache loaded `Engine`s per model, `Arc<Mutex<...>>`), `POST /bench` handler wrapping
   inference in `tokio::task::spawn_blocking` (never block inline in an async handler), `GET
   /health` returning 200. Reuse `crates/pipeline`'s `run_readout`/`run_generate` exactly as
   `apps/cli` does (same `reset_context` ordering between them). Request/response DTOs should
   mirror `apps/cli`'s JSON output shape (same field names) — the two surfaces must agree.
2. **Bind for real external reachability**: bind `0.0.0.0:<port>` (NOT `127.0.0.1` — a
   localhost-only bind would make the DoD's external-curl requirement impossible to satisfy).
   Pick a concrete port (e.g. 8080) and confirm nothing else on the server already uses it
   (`ss -ltnp | grep <port>` before binding).
3. **Start the server as a persistent background process on the server** (not just a
   foreground SSH session that dies when you disconnect) — use `nohup`/`systemd`/`tmux`/
   `screen`, whichever is simplest and reliable; document which one and how to check
   status/logs/restart it (this matters for Loop 4+ and the tuning loop, which will need the
   server running repeatedly).
4. **Verify external curl reachability FROM THIS MACHINE (the orchestrator/dev machine, not
   just from inside the SSH session)** — this is the actual DoD bar ("call curl successfully
   ... via IP 103.146.166.46"), not merely "works when SSH'd in". Run the `curl` commands from
   the Windows dev machine's own shell against `http://103.146.166.46:<port>/...`. If this
   fails (firewall/security-group blocking inbound), diagnose (check `ufw`/`iptables` on the
   server, check if a cloud firewall/security group exists) and open the port — do not declare
   success based on a curl run from inside the SSH session alone.
5. **No auth** — confirmed in-scope per user's explicit instruction, do not add any auth
   middleware.

## Preservation
See frontmatter. Also: do not modify `crates/engine`/`crates/models`/`crates/pipeline`'s public
API in a way that breaks `apps/cli` — if a shared type needs to change (e.g. DTOs), change it
in a way both `apps/cli` and `apps/server` can use identically (avoid duplicating the
`BenchReport` shape in two places with subtle differences).

## Validation Requirements
- Given `openjev-server` running on the Linux server bound to `0.0.0.0:<port>`, When `curl
  http://103.146.166.46:<port>/health` is run from the Windows dev machine (external network
  path, not from inside the SSH session), Then it returns HTTP 200.
- Given the same server, When `curl -X POST http://103.146.166.46:<port>/bench -H
  "Content-Type: application/json" -d '{"model":"qwen3-0.6b","prompt":"The capital of France
  is: A) London B) Paris","options":["A","B"]}'` is run from the Windows dev machine, Then it
  returns a valid `BenchReport` JSON with the same shape/semantics as `apps/cli`'s output
  (readout + generate populated, correct answer B, all 7 Timings fields present).
- Given 2 near-simultaneous requests to `/bench`, When both complete, Then neither crashes the
  server nor corrupts the other's result (Mutex serializes correctly).
- Given an invalid model name in the request body, When `/bench` handles it, Then it returns a
  4xx JSON error, not a 500/panic.

## Out-of-scope
- Laya (`pipeline::laya`, `models::laya`) — Loop 4+ (G6/G7/G8 remain open, untouched this loop).
- MiniCPM-2B / Qwen-4B actual inference — Loop 4+.
- Performance tuning — after the full 3-method/4-model surface works once.
- G9 (stale docs), G10 (missing generate.rs tests), G11 (unsafe pattern) — tracked, can be
  picked up opportunistically if trivial, but not the loop's focus; do not let them block
  `apps/server` delivery.
- Windows build fix (G1) — still descoped.
