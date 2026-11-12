"""Prépare `assets/ner/` avec les modèles retenus par le banc d'essai (voir assets/ner/LISEZMOI.md).

- `fr.*`    : DistilCamemBERT-NER (cmarkea), MatMul 4 bits + plongements int8 (prepare_camembert.py) ;
- `multi.*` : newmean-ai/pii-medium, `onnx/model_q4.onnx`.

Écrit aussi les références Python utilisées par les tests Rust :
- `tools/model/controle/reference_fr.json`, `reference_multi.json` (prédictions sur le jeu de contrôle) ;
- `tools/model/controle/exemples_blocs.jsonl` (blocs extraits de `exemples/` + entités attendues).

Usage : uv run --project tools/model tools/model/prepare_assets.py
(le réseau n'est utilisé que pour le téléchargement depuis Hugging Face, révisions figées).
"""

from __future__ import annotations

import hashlib
import json
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
ASSETS = ROOT / "assets" / "ner"
CACHE = HERE / "cache"
CAM = CACHE / "cmarkea__distilcamembert-base-ner"
PII = CACHE / "newmean-ai__pii-medium"

BUNDLES = {
    "fr": (CAM / "model_q4_gather.onnx", CAM / "fast" / "tokenizer.json", CAM / "config.json"),
    "multi": (PII / "onnx" / "model_q4.onnx", PII / "tokenizer.json", PII / "config.json"),
}


def sha256(p: Path) -> str:
    h = hashlib.sha256()
    with p.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def run(script: str, *args: str) -> None:
    subprocess.run([sys.executable, str(HERE / script), *args], check=True)


def main() -> None:
    run("download.py", "cmarkea/distilcamembert-base-ner", "newmean-ai/pii-medium")
    if not BUNDLES["fr"][0].exists() or not BUNDLES["fr"][1].exists():
        run("prepare_camembert.py")
    ASSETS.mkdir(parents=True, exist_ok=True)
    for name, (onnx, tok, cfg) in BUNDLES.items():
        for src, suffix in ((onnx, "onnx"), (tok, "tokenizer.json"), (cfg, "config.json")):
            dst = ASSETS / f"{name}.{suffix}"
            shutil.copyfile(src, dst)
            print(f"{dst.relative_to(ROOT)}  {dst.stat().st_size:>10}  {sha256(dst)}")

    from bench import FAMILY_OF_EXPECTED, load_control, load_examples
    from pipeline import NerModel, load_id2label

    ctl = load_control()
    for name in BUNDLES:
        m = NerModel(ASSETS / f"{name}.onnx", ASSETS / f"{name}.tokenizer.json",
                     load_id2label(ASSETS / f"{name}.config.json"))
        ref = []
        for u in ctl:
            text = u["blocks"][0]
            ref.append({"id": u["id"], "entites": [
                {"kind": e.kind, "valeur": text[e.start:e.end], "score": round(e.score, 4)} for e in m.predict(text)]})
        (HERE / "controle" / f"reference_{name}.json").write_text(
            json.dumps(ref, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    with (HERE / "controle" / "exemples_blocs.jsonl").open("w", encoding="utf-8", newline="\n") as f:
        for u in load_examples():
            ents = [{"type": t, "valeur": v} for t, v in u["entites"] if t in FAMILY_OF_EXPECTED]
            for b in u["blocks"]:
                f.write(json.dumps({"id": u["id"], "lang": u["lang"], "text": b, "entites": ents},
                                   ensure_ascii=False) + "\n")
    print("références écrites dans", HERE / "controle")


if __name__ == "__main__":
    main()
