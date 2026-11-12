"""Génère les faux fichiers de test du spike PDF (données fictives uniquement).

uv run --project ../../tools python gen_fixtures.py
"""

from pathlib import Path

from PIL import Image, ImageDraw, ImageFont
from reportlab.lib.pagesizes import A4
from reportlab.pdfgen import canvas

OUT = Path(__file__).parent / "fixtures"
OUT.mkdir(exist_ok=True)


def texte_pdf() -> None:
    c = canvas.Canvas(str(OUT / "texte.pdf"), pagesize=A4)
    c.setAuthor("Jean Dupont")
    c.setTitle("Contrat Hélène Müller-Lefèvre")
    c.setSubject("Dossier Jean Dupont")
    c.setKeywords("Dupont, Müller-Lefèvre")
    c.setFont("Helvetica-Bold", 16)
    c.drawString(72, 770, "Contrat de prestation")
    c.setFont("Helvetica", 12)
    lines = [
        "Entre Jean Dupont, demeurant 12 rue des Lilas, 75011 Paris,",
        "et Hélène Müller-Lefèvre, née le 3 mars 1980, il est convenu :",
        "Jean Dupont s'engage à verser la somme convenue avant le 1er juin.",
        "Fait à Paris, pour servir et valoir ce que de droit.",
    ]
    y = 730
    for line in lines:
        c.drawString(72, y, line)
        y -= 20
    c.showPage()
    c.save()


def piege_pdf() -> None:
    """PDF écrit à la main : opérateurs ' et ", TJ avec crénage, chaîne hexa,
    nom coupé sur deux lignes."""
    content = b"""BT
/F1 12 Tf
16 TL
72 720 Td
(Dossier suivi par) Tj
(Jean Dupont, conseiller.) '
(Le signataire, Jean) '
(Dupont, confirme.) '
T* [(Contact : Je) 40 (an Du) -20 (pont.)] TJ
T* <4A65616E2044> Tj (upont en hexa.) Tj
0 1 (Fin du dossier Jean Dupont.) "
ET
"""
    objs = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] "
        b"/Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        b"<< /Length %d >>\nstream\n" % len(content) + content + b"endstream",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
        b"<< /Author (Jean Dupont) /Title (Piege) >>",
    ]
    out = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
    offsets = []
    for i, body in enumerate(objs, start=1):
        offsets.append(len(out))
        out += b"%d 0 obj\n" % i + body + b"\nendobj\n"
    xref = len(out)
    out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objs) + 1)
    for off in offsets:
        out += b"%010d 00000 n \n" % off
    out += b"trailer\n<< /Size %d /Root 1 0 R /Info 6 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (
        len(objs) + 1,
        xref,
    )
    (OUT / "piege.pdf").write_bytes(bytes(out))


def scan() -> None:
    """Page « scannée » : image seule, 200 dpi, A4."""
    dpi = 200
    w, h = int(8.27 * dpi), int(11.69 * dpi)
    img = Image.new("L", (w, h), 255)
    d = ImageDraw.Draw(img)
    font = ImageFont.truetype("C:/Windows/Fonts/arial.ttf", 44)
    lines = [
        "Attestation sur l'honneur",
        "Je soussigné Jean Dupont, certifie que",
        "Hélène Müller-Lefèvre réside au 12 rue",
        "des Lilas à Paris depuis le 3 mars 2015.",
    ]
    y = 250
    for line in lines:
        d.text((180, y), line, fill=0, font=font)
        y += 90
    img.save(OUT / "scan.png", dpi=(dpi, dpi))
    img.convert("RGB").save(OUT / "scan.pdf", resolution=dpi)


if __name__ == "__main__":
    texte_pdf()
    piege_pdf()
    scan()
    for p in sorted(OUT.iterdir()):
        print(p.name, p.stat().st_size)
