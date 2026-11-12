"""Télécharge les fichiers des modèles candidats dans tools/model/cache/ (révisions figées).

Usage : uv run --project tools/model tools/model/download.py [candidat ...]
"""

from __future__ import annotations

import hashlib
import sys
from pathlib import Path

from huggingface_hub import hf_hub_download

CACHE = Path(__file__).resolve().parent / "cache"

# dépôt -> (révision figée, fichiers)
CANDIDATES: dict[str, tuple[str, list[str]]] = {
    "cmarkea/distilcamembert-base-ner": (
        "e539d952f70088b450c300c28ba455da87f2dc4b",
        ["config.json", "model.onnx", "model_quantized.onnx", "sentencepiece.bpe.model",
         "tokenizer_config.json", "README.md"],
    ),
    "newmean-ai/pii-nano": (
        "101a9b76a96e3e360b9e6128ad45084931b5d50b",
        ["config.json", "onnx/model_q4e.onnx", "onnx/model_q4.onnx", "tokenizer.json", "LICENSE", "NOTICE", "README.md"],
    ),
    "newmean-ai/pii-small": (
        "4c5509a91e948b2b93ac74ecbf9b0ef1fb1f021a",
        ["config.json", "onnx/model_q4e.onnx", "onnx/model_q4.onnx", "tokenizer.json", "LICENSE", "NOTICE", "README.md"],
    ),
    "newmean-ai/pii-medium": (
        "aff3cd7f17a31a67a165569458f223fc3d1818cb",
        ["config.json", "onnx/model_q4e.onnx", "onnx/model_q4.onnx", "tokenizer.json", "LICENSE", "NOTICE", "README.md"],
    ),
    "newmean-ai/pii-large": (
        "19a761b64f0850e196d413f55a981c17ebc784a6",
        ["config.json", "onnx/model_q4e.onnx", "onnx/model_q4.onnx", "tokenizer.json", "LICENSE", "NOTICE", "README.md"],
    ),
    "Xenova/distilbert-base-multilingual-cased-ner-hrl": (
        "c2a4dbf593c57f47004c5bc2d3770d311aee9c43",
        ["config.json", "onnx/model_quantized.onnx", "tokenizer.json", "tokenizer_config.json", "README.md"],
    ),
}


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def local_dir(repo: str) -> Path:
    return CACHE / repo.replace("/", "__")


def main(argv: list[str]) -> None:
    wanted = argv or list(CANDIDATES)
    for repo in wanted:
        rev, files = CANDIDATES[repo]
        for f in files:
            p = Path(hf_hub_download(repo, f, revision=rev, local_dir=local_dir(repo)))
            print(f"{repo}/{f}  {p.stat().st_size:>11}  {sha256(p)}")


if __name__ == "__main__":
    main(sys.argv[1:])
