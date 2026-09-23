import json, subprocess, sys, time, os, urllib.request

SCEN = json.load(open("/work/laya_scen.json"))
PORT = 8090
LAYA = "/work/ggmlc/build/examples/laya/laya"
T = os.environ.get("T", "16")
REPS = int(os.environ.get("REPS", "3"))


def _post(payload, timeout):
    r = urllib.request.Request(
        "http://127.0.0.1:%d/v1/decide" % PORT,
        data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(r, timeout=timeout) as resp:
        return json.load(resp)


def wait(proc, timeout=600):
    t0 = time.time()
    probe = {"state": "hi", "model": "laya-english",
             "questions": {"answer": {"type": "choice",
                                      "instructions": "Pick the correct option.",
                                      "criteria": {"A": None, "B": None}}}}
    while time.time() - t0 < timeout:
        if proc.poll() is not None:
            raise RuntimeError("laya serve died rc=%s" % proc.returncode)
        try:
            _post(probe, 120)
            return
        except Exception:
            time.sleep(2)
    raise RuntimeError("laya health timeout")


def decide(sc):
    crit = dict((o, None) for o in sc["options"])
    payload = {"state": sc["state"], "model": "laya-english",
               "questions": {"answer": {"type": "choice",
                                        "instructions": "Pick the correct option.",
                                        "criteria": crit}}}
    t0 = time.time()
    d = _post(payload, 300)
    return (time.time() - t0) * 1000.0, d["answers"]["answer"]


def run(path, tag):
    log = open("/work/logs/laya-%s.log" % tag, "wb")
    p = subprocess.Popen([LAYA, "serve", path, "--port", str(PORT),
                          "--device", "cpu", "--threads", T],
                         stdout=log, stderr=log)
    try:
        wait(p)
        out = []
        for sc in SCEN:
            decide(sc)  # warmup
            lats = []
            a = None
            for _ in range(REPS):
                ms, a = decide(sc)
                lats.append(ms)
            probs = a.get("probabilities") or {}
            gtp = sc["gt_probs"]
            dP = [abs(probs.get(k, 0.0) - v) for k, v in gtp.items()]
            top = probs.get(sc["gt_choice"])
            rec = {"id": sc["id"], "choice": a.get("choice"), "gt_choice": sc["gt_choice"],
                   "match": a.get("choice") == sc["gt_choice"],
                   "top_prob": top, "gt_top": sc["gt_top"],
                   "d_top": round(abs((top or 0.0) - sc["gt_top"]), 6),
                   "max_dP": round(max(dP), 6),
                   "confidence": a.get("confidence"), "gt_conf": sc["gt_conf"],
                   "lat_ms_median": round(sorted(lats)[len(lats) // 2], 2),
                   "lat_ms_min": round(min(lats), 2),
                   "probabilities": probs}
            out.append(rec)
            print("  %s: %s (gt %s) top=%s d=%s %sms" % (
                sc["id"], rec["choice"], rec["gt_choice"], rec["top_prob"],
                rec["d_top"], rec["lat_ms_median"]), flush=True)
        return out
    finally:
        p.terminate()
        try:
            p.wait(20)
        except Exception:
            p.kill()
        log.close()


if __name__ == "__main__":
    os.makedirs("/work/logs", exist_ok=True)
    os.makedirs("/work/results", exist_ok=True)
    res = {}
    for spec in sys.argv[1:]:
        tag, path = spec.split("=", 1)
        print("=== laya %s (%.0fMB) ===" % (tag, os.path.getsize(path) / 1e6), flush=True)
        r = run(path, tag)
        n = len(r)
        agree = sum(1 for x in r if x["match"])
        res[tag] = {"path": path, "size_bytes": os.path.getsize(path),
                    "choice_agreement": "%d/%d" % (agree, n),
                    "mean_d_top": round(sum(x["d_top"] for x in r) / n, 6),
                    "max_d_top": round(max(x["d_top"] for x in r), 6),
                    "mean_max_dP": round(sum(x["max_dP"] for x in r) / n, 6),
                    "lat_ms_median_mean": round(sum(x["lat_ms_median"] for x in r) / n, 2),
                    "scenarios": r}
        summ = dict((k, dict((kk, vv) for kk, vv in v.items() if kk != "scenarios"))
                    for k, v in res.items())
        print(json.dumps(summ, indent=1), flush=True)
        json.dump(res, open("/work/results/laya.json", "w"), indent=1)
