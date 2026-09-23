import json, glob, os

print("################ LLM SWEEP (10 project benchmark scenarios) ################")
bench = {}
if os.path.exists("/work/llama-bench.json"):
    for r in json.load(open("/work/llama-bench.json")):
        key = (r["model_filename"], "pp" if r["n_prompt"] else "tg")
        bench[key] = r["avg_ts"]

for f in ["qwen3-0.6b", "minicpm5-2b", "qwen3-4b"]:
    p = "/work/results/%s.json" % f
    if not os.path.exists(p):
        continue
    d = json.load(open(p))
    # reference = highest fidelity variant present
    ref = None
    for cand in ["f16", "q8_0_official", "q8_0_local", "q6_k_official", "q6_k"]:
        if cand in d:
            ref = cand
            break
    refans = {s["id"]: s["answer"] for s in d[ref]["scenarios"]}
    print("\n=== %s ===  (fidelity reference for agreement column: %s)" % (f, ref))
    print("%-18s %7s %7s %9s %9s %9s %9s %9s" % (
        "variant", "sizeGB", "validJS", "vs_ref", "vs_exp", "tg_tok/s", "pp_tok/s", "wall_ms"))
    for k, v in d.items():
        sc = v["scenarios"]
        n = len(sc)
        agree = sum(1 for s in sc if s["answer"] is not None and s["answer"] == refans.get(s["id"]))
        exp_tot = sum(1 for s in sc if s["expected"])
        exp_ok = sum(1 for s in sc if s["expected"] and s["answer"] == s["expected"])
        fn = os.path.basename(v["path"])
        print("%-18s %7.2f %7s %9s %9s %9s %9s %9.0f" % (
            k, v["size_bytes"] / 1e9, v["valid_json"],
            "%d/%d" % (agree, n), "%d/%d" % (exp_ok, exp_tot),
            ("%.2f" % bench[(fn, "tg")]) if (fn, "tg") in bench else "-",
            ("%.0f" % bench[(fn, "pp")]) if (fn, "pp") in bench else "-",
            v["wall_ms_mean"]))
    # per-scenario answer matrix
    print("  per-scenario answers:")
    ids = [s["id"] for s in d[ref]["scenarios"]]
    for i in ids:
        row = []
        for k, v in d.items():
            a = [s for s in v["scenarios"] if s["id"] == i][0]["answer"]
            row.append("%s=%s" % (k, a))
        print("   ", i, "|", "  ".join(row))

print("\n\n################ LAYA (15 PyTorch ground-truth scenarios) ################")
for p in ["/work/results/laya-hf.json", "/work/results/laya.json"]:
    if not os.path.exists(p):
        continue
    d = json.load(open(p))
    print("\n--- %s ---" % os.path.basename(p))
    print("%-16s %7s %10s %11s %10s %10s" % (
        "variant", "sizeMB", "gt_choice", "mean_d_top", "max_d_top", "lat_ms"))
    for k, v in d.items():
        print("%-16s %7.0f %10s %11.6f %10.6f %10.2f" % (
            k, v["size_bytes"] / 1e6, v["choice_agreement"],
            v["mean_d_top"], v["max_d_top"], v["lat_ms_median_mean"]))
        for s in v["scenarios"]:
            if not s["match"]:
                print("      FLIP %s: got %s, gt %s" % (s["id"], s["choice"], s["gt_choice"]))
    # intra-set agreement vs the set's own q8_0
    base = None
    for cand in ["hf_f16", "comp_q8_0"]:
        if cand in d:
            base = cand
            break
    if base:
        b = {s["id"]: s["choice"] for s in d[base]["scenarios"]}
        print("  intra-set agreement vs %s:" % base)
        for k, v in d.items():
            ag = sum(1 for s in v["scenarios"] if s["choice"] == b[s["id"]])
            print("    %-16s %d/%d" % (k, ag, len(v["scenarios"])))
