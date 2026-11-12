"""Banc d'essai des modèles NER candidats.

Mesure, pour chaque candidat tel qu'il serait embarqué : rappel et précision par famille
(personnes, sociétés, lieux) et par langue, sur `exemples/` et sur le jeu de contrôle
indépendant `tools/model/controle/controle.jsonl` ; taille du fichier ; temps pour 1 000 mots.

Usage :
    uv run --project tools/model tools/model/download.py
    uv run --project tools/model tools/model/prepare_camembert.py
    uv run --project tools/model tools/model/bench.py [--only nom] [--threshold 0.5]

Écrit `tools/model/resultats/bench.md`, `bench.json` et les prédictions sur le jeu de
contrôle (`<candidat>_controle.json`, comparées par les tests Rust).
"""

from __future__ import annotations

import argparse
import json
import statistics
import time
from collections import defaultdict
from pathlib import Path

import pipeline
from extract import blocks
from pipeline import NerModel, is_word_char, load_id2label

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
CACHE = HERE / "cache"
OUT = HERE / "resultats"

CAM = CACHE / "cmarkea__distilcamembert-base-ner"
XEN = CACHE / "Xenova__distilbert-base-multilingual-cased-ner-hrl"


def nm(size: str, f: str = "model_q4e.onnx") -> dict:
    d = CACHE / f"newmean-ai__pii-{size}"
    return {"onnx": d / "onnx" / f, "tok": d / "tokenizer.json", "cfg": d / "config.json", "licence": "Apache-2.0"}


CANDIDATES: dict[str, dict] = {
    "pii-nano q4e": nm("nano"),
    "pii-small q4e": nm("small"),
    "pii-medium q4e": nm("medium"),
    "pii-medium q4": nm("medium", "model_q4.onnx"),
    "pii-large q4e": nm("large"),
    "distilcamembert fp32": {"onnx": CAM / "model.onnx", "tok": CAM / "fast" / "tokenizer.json", "cfg": CAM / "config.json", "licence": "MIT"},
    "distilcamembert int8 (HF)": {"onnx": CAM / "model_quantized.onnx", "tok": CAM / "fast" / "tokenizer.json", "cfg": CAM / "config.json", "licence": "MIT"},
    "distilcamembert int8+Gather": {"onnx": CAM / "model_int8_gather.onnx", "tok": CAM / "fast" / "tokenizer.json", "cfg": CAM / "config.json", "licence": "MIT"},
    "distilcamembert q4+Gather": {"onnx": CAM / "model_q4_gather.onnx", "tok": CAM / "fast" / "tokenizer.json", "cfg": CAM / "config.json", "licence": "MIT"},
    "distilcamembert int8 pc+rr+Gather": {"onnx": CAM / "model_int8_pc_gather.onnx", "tok": CAM / "fast" / "tokenizer.json", "cfg": CAM / "config.json", "licence": "MIT"},
    "distilcamembert n8+Gather": {"onnx": CAM / "model_n8_gather.onnx", "tok": CAM / "fast" / "tokenizer.json", "cfg": CAM / "config.json", "licence": "MIT"},
    "distilbert-multi-hrl int8": {"onnx": XEN / "onnx" / "model_quantized.onnx", "tok": XEN / "tokenizer.json", "cfg": XEN / "config.json", "licence": "AFL-3.0 (incompatible)"},
}

FAMILY_OF_EXPECTED = {
    "PERSONNE": "personnes", "NOM": "personnes", "PRENOM": "personnes",
    "SOCIETE": "societes", "LIEU": "lieux", "ADRESSE": "lieux",
}
FAMILY_OF_PRED = {"PERSONNE": "personnes", "SOCIETE": "societes", "LIEU": "lieux", "ADRESSE": "lieux"}
FAMILIES = ["personnes", "societes", "lieux"]


def load_examples() -> list[dict]:
    """Une unité par fichier : blocs + entités attendues non OCR."""
    data = json.loads((ROOT / "exemples" / "attendu.json").read_text(encoding="utf-8"))
    units = []
    for f in data["fichiers"]:
        path = ROOT / "exemples" / f["fichier"]
        bl = blocks(path)
        if not bl:
            continue
        ents = [(e["type"], e["valeur"]) for e in f["entites"] if e["type"] in FAMILY_OF_EXPECTED and not e.get("ocr")]
        units.append({"id": f["fichier"], "lang": f["langue"], "blocks": bl, "entites": ents})
    return units


def load_control() -> list[dict]:
    units = []
    for line in (HERE / "controle" / "controle.jsonl").read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        d = json.loads(line)
        for e in d["entites"]:
            if e["valeur"] not in d["text"]:
                raise SystemExit(f"{d['id']} : valeur absente du texte : {e['valeur']}")
        units.append({"id": d["id"], "lang": d["lang"], "blocks": [d["text"]],
                      "entites": [(e["type"], e["valeur"]) for e in d["entites"]]})
    return units


def occurrences(text: str, value: str) -> list[tuple[int, int]]:
    out, i = [], text.find(value)
    while i >= 0:
        out.append((i, i + len(value)))
        i = text.find(value, i + len(value))
    return out


def score_unit(unit: dict, preds_per_block: list[list[pipeline.Entity]], stats: dict, lang_key: str) -> None:
    """Rappel strict : tous les caractères alphanumériques de l'occurrence sont couverts par
    des prédictions de la même famille. Rappel partiel : au moins un. Précision : une
    prédiction est juste si elle chevauche une occurrence attendue de sa famille, ou si son
    texte est une partie d'une valeur attendue de sa famille (« Carter » pour « Emily Carter »)."""
    for text, preds in zip(unit["blocks"], preds_per_block):
        occ_by_fam = defaultdict(list)
        for typ, val in unit["entites"]:
            for s, e in occurrences(text, val):
                occ_by_fam[FAMILY_OF_EXPECTED[typ]].append((s, e))
        for fam in FAMILIES:
            covered = [False] * len(text)
            for p in preds:
                if FAMILY_OF_PRED.get(p.kind) == fam:
                    for i in range(p.start, p.end):
                        covered[i] = True
            for s, e in occ_by_fam[fam]:
                idx = [i for i in range(s, e) if is_word_char(text[i])]
                st = stats[(lang_key, fam)]
                st["occ"] += 1
                st["strict"] += all(covered[i] for i in idx)
                st["partial"] += any(covered[i] for i in idx)
        for p in preds:
            fam = FAMILY_OF_PRED.get(p.kind)
            if fam is None:
                continue
            st = stats[(lang_key, fam)]
            st["pred"] += 1
            ptxt = text[p.start:p.end]
            ok = any(s < p.end and p.start < e for s, e in occ_by_fam[fam]) or any(
                ptxt in val for typ, val in unit["entites"] if FAMILY_OF_EXPECTED[typ] == fam)
            st["tp_pred"] += ok
            if not ok:
                st.setdefault("fp", []).append(ptxt)
        for typ, val in unit["entites"]:
            pass


def new_stats():
    return defaultdict(lambda: {"occ": 0, "strict": 0, "partial": 0, "pred": 0, "tp_pred": 0})


def evaluate(model: NerModel, units: list[dict], dataset: str, misses: list) -> tuple[dict, list]:
    stats = new_stats()
    dump = []
    for u in units:
        preds = [model.predict(b) for b in u["blocks"]]
        lang = u["lang"] if u["lang"] in ("fr", "en") else "autres"
        score_unit(u, preds, stats, lang)
        for b, ps in zip(u["blocks"], preds):
            dump.append({"id": u["id"], "text": b, "entites": [{"kind": p.kind, "start": p.start, "end": p.end,
                                                                  "valeur": b[p.start:p.end], "score": round(p.score, 4)} for p in ps]})
            for typ, val in u["entites"]:
                if FAMILY_OF_EXPECTED[typ] == "personnes" and val in b:
                    cov = "".join(b[p.start:p.end] + "|" for p in ps if p.kind == "PERSONNE")
                    if not all(any(p.kind == "PERSONNE" and p.start <= i < p.end for p in ps)
                               for s, e in occurrences(b, val) for i in range(s, e) if is_word_char(b[i])):
                        misses.append(f"{dataset}/{u['id']}: « {val} » (trouvé : {cov or '-'})")
    return stats, dump


def speed(model: NerModel, control: list[dict]) -> float:
    words: list[str] = []
    fr = [u["blocks"][0] for u in control if u["lang"] == "fr"]
    while len(words) < 1000:
        for s in fr:
            words += s.split()
    text = " ".join(words[:1000])
    model.predict(text)
    times = []
    for _ in range(5):
        t0 = time.perf_counter()
        model.predict(text)
        times.append(time.perf_counter() - t0)
    return statistics.median(times) * 1000


def pct(a: int, b: int) -> str:
    return "—" if b == 0 else f"{100 * a / b:.1f}"


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--only", action="append")
    ap.add_argument("--threshold", type=float, default=pipeline.MIN_CONFIDENCE)
    args = ap.parse_args()
    pipeline.MIN_CONFIDENCE = args.threshold
    OUT.mkdir(exist_ok=True)
    examples, control = load_examples(), load_control()
    results = {}
    for name, c in CANDIDATES.items():
        if args.only and name not in args.only:
            continue
        if not c["onnx"].exists():
            print(f"absent : {name} ({c['onnx']}) — lancer download.py / prepare_camembert.py")
            continue
        model = NerModel(c["onnx"], c["tok"], load_id2label(c["cfg"]))
        misses: list[str] = []
        st_ex, _ = evaluate(model, examples, "exemples", misses)
        st_ct, dump = evaluate(model, control, "controle", misses)
        (OUT / f"{name.replace(' ', '_').replace('+', '_')}_controle.json").write_text(
            json.dumps(dump, ensure_ascii=False, indent=1), encoding="utf-8")
        ms = speed(model, control)
        ms1 = speed(NerModel(c["onnx"], c["tok"], load_id2label(c["cfg"]), threads=1), control)
        r = {"taille_mo": round(c["onnx"].stat().st_size / 1e6, 1), "licence": c["licence"], "ms_1000_mots": round(ms),
             "ms_1000_mots_1fil": round(ms1), "exemples": {}, "controle": {}, "rates": misses}
        for key, st in (("exemples", st_ex), ("controle", st_ct)):
            for (lang, fam), v in sorted(st.items()):
                r[key][f"{lang}/{fam}"] = {k: v[k] for k in ("occ", "strict", "partial", "pred", "tp_pred")} | {
                    "rappel": pct(v["strict"], v["occ"]), "rappel_partiel": pct(v["partial"], v["occ"]),
                    "precision": pct(v["tp_pred"], v["pred"]), "faux_positifs": v.get("fp", [])}
        results[name] = r
        print(f"{name:30} {r['taille_mo']:>6} Mo  {ms:>6.0f} ms  "
              + "  ".join(f"{k}:{v['rappel']}/{v['precision']}" for k, v in r["controle"].items() if k.endswith("personnes")))
    prev = {}
    if (OUT / "bench.json").exists() and args.only:
        prev = json.loads((OUT / "bench.json").read_text(encoding="utf-8"))
    prev.update(results)
    (OUT / "bench.json").write_text(json.dumps(prev, ensure_ascii=False, indent=1), encoding="utf-8")
    write_markdown(prev)


def write_markdown(results: dict) -> None:
    lines = ["| Candidat | Licence | Taille (Mo) | ms / 1 000 mots (tous fils / 1 fil) | "
             "Pers. FR contrôle R / P | Pers. FR exemples R / P | Pers. EN (ctl+ex) R / P | Pers. autres (ctl+ex) R / P | "
             "Sociétés FR ctl R / P | Lieux FR ctl R / P |",
             "|---|---|---|---|---|---|---|---|---|---|"]

    def rp(r: dict, keys: list[tuple[str, str]]) -> str:
        occ = strict = pred = tp = 0
        for ds, k in keys:
            v = r[ds].get(k)
            if v:
                occ += v["occ"]; strict += v["strict"]; pred += v["pred"]; tp += v["tp_pred"]
        return f"{pct(strict, occ)} / {pct(tp, pred)}"

    for name, r in results.items():
        es = [("controle", "autres/personnes"), ("exemples", "autres/personnes")]
        lines.append(
            f"| {name} | {r['licence']} | {r['taille_mo']} | {r['ms_1000_mots']} / {r['ms_1000_mots_1fil']} | "
            f"{rp(r, [('controle', 'fr/personnes')])} | {rp(r, [('exemples', 'fr/personnes')])} | "
            f"{rp(r, [('controle', 'en/personnes'), ('exemples', 'en/personnes')])} | {rp(r, es)} | "
            f"{rp(r, [('controle', 'fr/societes')])} | {rp(r, [('controle', 'fr/lieux')])} |")
    (OUT / "bench.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print("\n".join(lines))


if __name__ == "__main__":
    main()
