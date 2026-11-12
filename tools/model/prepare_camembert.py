"""DistilCamemBERT-NER : génère tokenizer.json (depuis sentencepiece.bpe.model) et deux
quantifications supplémentaires du modèle fp32 :

- int8 dynamique MatMul + Gather (plongements compris) -> model_int8_gather.onnx (~70 Mo visé) ;
- int8 dynamique par canal, plage réduite, MatMul + Gather -> model_int8_pc_gather.onnx ;
- MatMul en 8 bits (MatMulNBits, bloc 32) + Gather int8 -> model_n8_gather.onnx ;
- MatMul en 4 bits (MatMulNBits, bloc 32) + Gather int8 -> model_q4_gather.onnx (retenu).

Usage : uv run --project tools/model tools/model/prepare_camembert.py
"""

from __future__ import annotations

from pathlib import Path

from onnxruntime.quantization import QuantType, quantize_dynamic
from transformers import AutoTokenizer

DIR = Path(__file__).resolve().parent / "cache" / "cmarkea__distilcamembert-base-ner"


def tokenizer() -> None:
    # Le constructeur direct produit un vocabulaire vide avec transformers 5 : passer
    # par from_pretrained, puis vérifier contre sentencepiece (décalage fairseq de 4).
    import sentencepiece as spm
    from tokenizers import Tokenizer

    tok = AutoTokenizer.from_pretrained(str(DIR))
    tok.save_pretrained(str(DIR / "fast"))
    fast = Tokenizer.from_file(str(DIR / "fast" / "tokenizer.json"))
    sp = spm.SentencePieceProcessor(model_file=str(DIR / "sentencepiece.bpe.model"))
    sample = "Mme Hinatea Teriitahi, née à Dumbéa, gérante de Lagon Bleu SAS."
    got = fast.encode(sample, add_special_tokens=False).ids
    want = [i + 4 if i != 0 else 3 for i in sp.encode(sample)]
    if got != want:
        raise SystemExit(f"tokenizer.json incohérent avec sentencepiece : {got} != {want}")
    print("tokenizer.json ->", DIR / "fast" / "tokenizer.json")


def int8_gather() -> None:
    out = DIR / "model_int8_gather.onnx"
    quantize_dynamic(
        str(DIR / "model.onnx"),
        str(out),
        weight_type=QuantType.QInt8,
        op_types_to_quantize=["MatMul", "Gather"],
        per_channel=False,
        extra_options={"WeightSymmetric": True},
    )
    print(out, out.stat().st_size)


def int8_pc_gather() -> None:
    out = DIR / "model_int8_pc_gather.onnx"
    quantize_dynamic(str(DIR / "model.onnx"), str(out), weight_type=QuantType.QInt8,
                     op_types_to_quantize=["MatMul", "Gather"], per_channel=True, reduce_range=True)
    print(out, out.stat().st_size)


def nbits_gather(bits: int, name: str) -> None:
    from onnxruntime.quantization.matmul_nbits_quantizer import (
        DefaultWeightOnlyQuantConfig,
        MatMulNBitsQuantizer,
    )
    import onnx

    tmp = DIR / f"{name}_tmp.onnx"
    model = onnx.load(str(DIR / "model.onnx"))
    cfg = DefaultWeightOnlyQuantConfig(block_size=32, is_symmetric=True, bits=bits)
    q = MatMulNBitsQuantizer(model, algo_config=cfg)
    q.process()
    q.model.save_model_to_file(str(tmp), use_external_data_format=False)
    out = DIR / f"{name}.onnx"
    quantize_dynamic(str(tmp), str(out), weight_type=QuantType.QInt8, op_types_to_quantize=["Gather"])
    tmp.unlink()
    print(out, out.stat().st_size)


if __name__ == "__main__":
    tokenizer()
    int8_gather()
    int8_pc_gather()
    nbits_gather(8, "model_n8_gather")
    nbits_gather(4, "model_q4_gather")
