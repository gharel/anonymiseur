"""Extraction de blocs de texte des fichiers d'exemple (approximation des `Handler` Rust :
paragraphes, cellules, notes, commentaires, propriétés). Les fichiers image ou scannés
(OCR) sont ignorés : le banc mesure le modèle, pas l'OCR.
"""

from __future__ import annotations

import csv
import io
import re
import zipfile
from pathlib import Path
from xml.etree import ElementTree as ET

NS_W = "{http://schemas.openxmlformats.org/wordprocessingml/2006/main}"
NS_A = "{http://schemas.openxmlformats.org/drawingml/2006/main}"
NS_TEXT = "{urn:oasis:names:tc:opendocument:xmlns:text:1.0}"
NS_DC = "{http://purl.org/dc/elements/1.1/}"
NS_CP = "{http://schemas.openxmlformats.org/package/2006/metadata/core-properties}"
NS_META = "{urn:oasis:names:tc:opendocument:xmlns:meta:1.0}"


def _xml_paragraphs(data: bytes, para_tag: str, text_tags: set[str]) -> list[str]:
    root = ET.fromstring(data)
    out = []
    for p in root.iter(para_tag):
        parts = [el.text or "" for el in p.iter() if el.tag in text_tags]
        s = "".join(parts).strip()
        if s:
            out.append(s)
    return out


def _odf_paragraphs(data: bytes) -> list[str]:
    root = ET.fromstring(data)
    out = []
    for p in root.iter():
        if p.tag in (NS_TEXT + "p", NS_TEXT + "h"):
            buf = []

            def walk(el):
                if el.text:
                    buf.append(el.text)
                for ch in el:
                    if ch.tag == NS_TEXT + "s":
                        buf.append(" " * int(ch.get(NS_TEXT + "c", "1")))
                    elif ch.tag in (NS_TEXT + "tab",):
                        buf.append("\t")
                    elif ch.tag in (NS_TEXT + "line-break",):
                        buf.append("\n")
                    elif ch.tag == NS_TEXT + "note":
                        pass
                    else:
                        walk(ch)
                    if ch.tail:
                        buf.append(ch.tail)

            walk(p)
            s = "".join(buf).strip()
            if s:
                out.append(s)
    return out


def _meta(data: bytes, tags: list[str]) -> list[str]:
    root = ET.fromstring(data)
    return [el.text.strip() for el in root.iter() if el.tag in tags and el.text and el.text.strip()]


def blocks(path: Path) -> list[str]:
    ext = path.suffix.lower()
    if ext in (".txt", ".md"):
        return [l.strip() for l in path.read_text(encoding="utf-8").splitlines() if l.strip()]
    if ext == ".csv":
        text = path.read_text(encoding="utf-8-sig")
        dialect = csv.Sniffer().sniff(text.splitlines()[0])
        return [c.strip() for row in csv.reader(io.StringIO(text), dialect) for c in row if c.strip()]
    if ext in (".docx", ".pptx"):
        out: list[str] = []
        with zipfile.ZipFile(path) as z:
            for name in z.namelist():
                if not name.endswith(".xml"):
                    continue
                data = z.read(name)
                if name == "docProps/core.xml":
                    out += _meta(data, [NS_DC + "creator", NS_CP + "lastModifiedBy", NS_DC + "title"])
                elif ext == ".docx" and name.startswith("word/"):
                    out += _xml_paragraphs(data, NS_W + "p", {NS_W + "t", NS_W + "delText"})
                elif ext == ".pptx" and name.startswith("ppt/") and ("slides/" in name or "notesSlides/" in name or "comments" in name):
                    out += _xml_paragraphs(data, NS_A + "p", {NS_A + "t"})
                    if "comments" in name:
                        out += [t for t in re.findall(r"<p:text>(.*?)</p:text>", data.decode("utf-8"))]
        return out
    if ext == ".xlsx":
        import openpyxl

        out = []
        wb = openpyxl.load_workbook(path, data_only=False)
        for ws in wb.worksheets:
            for row in ws.iter_rows():
                for c in row:
                    if c.value is not None and str(c.value).strip():
                        out.append(str(c.value).strip())
                    if c.comment:
                        out.append(c.comment.text.strip())
        props = wb.properties
        out += [v for v in (props.creator, props.lastModifiedBy, props.title) if v]
        return out
    if ext in (".odt", ".ods", ".odp"):
        out = []
        with zipfile.ZipFile(path) as z:
            for name in ("content.xml", "styles.xml"):
                if name in z.namelist():
                    out += _odf_paragraphs(z.read(name))
            if "meta.xml" in z.namelist():
                out += _meta(z.read("meta.xml"), [NS_DC + "creator", NS_META + "initial-creator", NS_DC + "title"])
        return out
    if ext == ".pdf":
        from pypdf import PdfReader

        r = PdfReader(str(path))
        out = []
        for page in r.pages:
            out += [l.strip() for l in (page.extract_text() or "").splitlines() if l.strip()]
        if r.metadata:
            out += [v for v in (r.metadata.author, r.metadata.title) if v]
        return out
    return []
