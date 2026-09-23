import json, subprocess, sys, time, os, urllib.request

SCEN = json.load(open("/work/scen.json"))
T = os.environ.get("T", "16")
PORT = int(os.environ.get("PORT", "8081"))
OUT = os.environ.get("OUT", "/work/results/llm.json")


def wait_health(proc, timeout=900):
    t0 = time.time()
    while time.time() - t0 < timeout:
        if proc.poll() is not None:
            raise RuntimeError("server died rc=%s" % proc.returncode)
        try:
            with urllib.request.urlopen("http://127.0.0.1:%d/health" % PORT, timeout=3) as r:
                if r.status == 200:
                    return
        except Exception:
            time.sleep(2)
    raise RuntimeError("health timeout")


def strip_think(t):
    out = ""
    rest = t
    while True:
        i = rest.find("<think>")
        if i < 0:
            out += rest
            break
        out += rest[:i]
        after = rest[i + 7:]
        j = after.find("</think>")
        if j < 0:
            break
        rest = after[j + 8:]
    return out.strip()


def parse_answer(text, options):
    s = text.find("{")
    e = text.rfind("}")
    if s < 0 or e < s:
        return None
    try:
        v = json.loads(text[s:e + 1])
    except Exception:
        return None
    if not isinstance(v, dict):
        return None
    a = v.get("answer")
    if not isinstance(a, str):
        return None
    a = a.strip()
    for o in options:
        if o.strip() == a:
            return a
    return None


def run_model(path, tag):
    log = open("/work/logs/%s.server.log" % tag, "wb")
    proc = subprocess.Popen(
        ["/app/llama-server", "-m", path, "--port", str(PORT), "--host", "127.0.0.1",
         "-t", T, "-tb", T, "-c", "4096", "-np", "1"],
        stdout=log, stderr=log, env=dict(os.environ, LD_LIBRARY_PATH="/app"))
    try:
        wait_health(proc)
        res = []
        for sc in SCEN:
            full = (sc["prompt"] + "\nOptions: " + ", ".join(sc["options"]) +
                    "\nRespond with ONLY a JSON object of the exact form "
                    '{"answer": "<one of the options above>"}.')
            body = json.dumps({"messages": [{"role": "user", "content": full}],
                               "temperature": 0, "top_k": 1, "seed": 0,
                               "n_predict": 512, "cache_prompt": False}).encode()
            req = urllib.request.Request("http://127.0.0.1:%d/v1/chat/completions" % PORT,
                                         data=body,
                                         headers={"Content-Type": "application/json"})
            t0 = time.time()
            with urllib.request.urlopen(req, timeout=3600) as r:
                d = json.load(r)
            wall = (time.time() - t0) * 1000.0
            txt = d["choices"][0]["message"]["content"]
            tm = d.get("timings", {})
            clean = strip_think(txt)
            ans = parse_answer(clean, sc["options"])
            rec = {"id": sc["id"], "answer": ans, "expected": sc["expected"],
                   "conf": sc["conf"], "valid_json": ans is not None,
                   "wall_ms": round(wall, 1),
                   "predicted_n": tm.get("predicted_n"),
                   "predicted_ms": tm.get("predicted_ms"),
                   "predicted_per_second": tm.get("predicted_per_second"),
                   "prompt_n": tm.get("prompt_n"),
                   "prompt_per_second": tm.get("prompt_per_second"),
                   "raw_tail": clean[-160:]}
            res.append(rec)
            print("  %s: %s (%.0fms, %s tok, %.2f tok/s)" % (
                sc["id"], ans, wall, tm.get("predicted_n"),
                tm.get("predicted_per_second") or 0.0), flush=True)
        return res
    finally:
        proc.terminate()
        try:
            proc.wait(30)
        except Exception:
            proc.kill()
        log.close()


if __name__ == "__main__":
    os.makedirs("/work/logs", exist_ok=True)
    os.makedirs("/work/results", exist_ok=True)
    out = {}
    if os.path.exists(OUT):
        out = json.load(open(OUT))
    for spec in sys.argv[1:]:
        tag, path = spec.split("=", 1)
        print("=== %s (%.2fGB) ===" % (tag, os.path.getsize(path) / 1e9), flush=True)
        t0 = time.time()
        sc = run_model(path, tag)
        n = len(sc)
        tps = [x["predicted_per_second"] for x in sc if x["predicted_per_second"]]
        out[tag] = {"path": path, "size_bytes": os.path.getsize(path),
                    "total_s": round(time.time() - t0, 1),
                    "valid_json": "%d/%d" % (sum(1 for x in sc if x["valid_json"]), n),
                    "gen_tok_s_mean": round(sum(tps) / len(tps), 2) if tps else None,
                    "gen_ms_mean": round(sum(x["predicted_ms"] or 0 for x in sc) / n, 1),
                    "wall_ms_mean": round(sum(x["wall_ms"] for x in sc) / n, 1),
                    "scenarios": sc}
        json.dump(out, open(OUT, "w"), indent=1)
        print(json.dumps({k: {kk: vv for kk, vv in v.items() if kk != "scenarios"}
                          for k, v in out.items()}, indent=1), flush=True)
    print("ALLDONE", flush=True)
