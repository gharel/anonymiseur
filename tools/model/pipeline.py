"""Chaîne d'inférence NER de référence (Python). `crates/anon-ner` en est la transcription
exacte : mêmes fenêtres, même moyenne des probabilités, même agrégation, même seuil.
Toute modification ici doit être reportée côté Rust (et inversement).
"""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path

import numpy as np
import onnxruntime as ort
from tokenizers import Tokenizer

# Fenêtre totale (jetons spéciaux compris) et chevauchement entre fenêtres, en jetons.
WINDOW_TOKENS = 256
WINDOW_OVERLAP = 64
# Seuil de confiance moyen d'une entité (moyenne des probabilités de ses jetons).
MIN_CONFIDENCE = 0.5

# Étiquettes des modèles -> types de l'application (préfixes de balise).
LABEL_MAP = {
    "PER": "PERSONNE",
    "PERSON": "PERSONNE",
    "ORG": "SOCIETE",
    "LOC": "LIEU",
    "ADDRESS": "ADRESSE",
    "DATE_OF_BIRTH": "DATE_NAISS",
}


@dataclass
class Entity:
    kind: str
    start: int  # offsets en caractères (Python) ; en octets côté Rust
    end: int
    score: float


def is_word_char(c: str) -> bool:
    return c.isalnum()


class NerModel:
    def __init__(self, onnx_path: Path, tokenizer_path: Path, id2label: dict[int, str], threads: int = 0):
        so = ort.SessionOptions()
        if threads:
            so.intra_op_num_threads = threads
        so.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
        self.session = ort.InferenceSession(str(onnx_path), so, providers=["CPUExecutionProvider"])
        self.tok = Tokenizer.from_file(str(tokenizer_path))
        specials = self.tok.encode("", add_special_tokens=True).ids
        if len(specials) != 2:
            raise ValueError(f"jetons spéciaux inattendus : {specials}")
        self.cls, self.sep = specials
        self.labels: list[tuple[str, str | None]] = []
        for i in range(len(id2label)):
            lab = id2label[i]
            if lab == "O":
                self.labels.append(("O", None))
            else:
                bio, typ = lab.split("-", 1)
                self.labels.append((bio, LABEL_MAP.get(typ)))

    def token_probs(self, ids: list[int]) -> np.ndarray:
        n = len(ids)
        content = WINDOW_TOKENS - 2
        step = content - WINDOW_OVERLAP
        acc = np.zeros((n, len(self.labels)), dtype=np.float64)
        cnt = np.zeros(n, dtype=np.float64)
        start = 0
        while True:
            end = min(start + content, n)
            win = [self.cls, *ids[start:end], self.sep]
            inp = np.array([win], dtype=np.int64)
            logits = self.session.run(None, {"input_ids": inp, "attention_mask": np.ones_like(inp)})[0][0]
            logits = logits[1:-1].astype(np.float64)
            logits -= logits.max(axis=1, keepdims=True)
            p = np.exp(logits)
            p /= p.sum(axis=1, keepdims=True)
            acc[start:end] += p
            cnt[start:end] += 1
            if end >= n:
                break
            start += step
        return acc / cnt[:, None]

    def predict(self, text: str) -> list[Entity]:
        if not text.strip():
            return []
        enc = self.tok.encode(text, add_special_tokens=False)
        if not enc.ids:
            return []
        probs = self.token_probs(enc.ids)
        ents: list[tuple[str, int, int, list[float]]] = []
        cur: list | None = None
        for i, (s, e) in enumerate(enc.offsets):
            k = int(probs[i].argmax())
            bio, kind = self.labels[k]
            score = float(probs[i][k])
            if e <= s:
                continue
            if kind is None:
                cur = None
                continue
            continues = cur is not None and cur[0] == kind and (bio != "B" or s <= cur[2])
            if continues:
                cur[2] = e
                cur[3].append(score)
            else:
                cur = [kind, s, e, [score]]
                ents.append(cur)
        out: list[Entity] = []
        for kind, s, e, scores in ents:
            # Étendre aux frontières de mot, puis retirer ponctuation et espaces aux bords.
            if is_word_char(text[s]):
                while s > 0 and is_word_char(text[s - 1]):
                    s -= 1
            if is_word_char(text[e - 1]):
                while e < len(text) and is_word_char(text[e]):
                    e += 1
            while s < e and not is_word_char(text[s]):
                s += 1
            while e > s and not is_word_char(text[e - 1]):
                e -= 1
            if s >= e:
                continue
            conf = sum(scores) / len(scores)
            if conf < MIN_CONFIDENCE:
                continue
            if out and out[-1].kind == kind and out[-1].end >= s:
                prev = out[-1]
                prev.end = max(prev.end, e)
                prev.score = min(prev.score, conf)
                continue
            out.append(Entity(kind, s, e, conf))
        return out


def load_id2label(config_path: Path) -> dict[int, str]:
    cfg = json.loads(config_path.read_text(encoding="utf-8"))
    return {int(k): v for k, v in cfg["id2label"].items()}
