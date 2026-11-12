#!/usr/bin/env python3
"""Génère les faux fichiers d'exemple de l'anonymiseur dans exemples/.

Toutes les données sont inventées (invariant 5 : aucune donnée réelle). La
génération est déterministe : aucune date courante, aucun aléa. Le script se
termine par un auto-contrôle qui relit chaque fichier et confronte son contenu
à exemples/attendu.json.

    uv run --project tools python tools/make_examples.py
"""

from __future__ import annotations

import base64
import hashlib
import html
import io
import json
import re
import sys
import textwrap
import zipfile
import zlib
from datetime import datetime
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

RACINE = Path(__file__).resolve().parent.parent
SORTIE = RACINE / "exemples"
MENTION = "DONNÉES FICTIVES — exemple de test"
DATE0 = datetime(2026, 3, 2, 9, 0, 0)
DATE_ZIP = (2026, 3, 2, 9, 0, 0)
RENFORCE = {"DATE", "AGE", "FONCTION", "MONTANT", "LIEU", "MATRICULE"}

# ---------------------------------------------------------------------------
# Clés de contrôle (valides)
# ---------------------------------------------------------------------------


def luhn_ok(num: str) -> bool:
    total = 0
    for i, ch in enumerate(reversed(num)):
        d = int(ch)
        if i % 2 == 1:
            d *= 2
            if d > 9:
                d -= 9
        total += d
    return total % 10 == 0


def luhn_cle(prefixe: str) -> str:
    return next(str(d) for d in range(10) if luhn_ok(prefixe + str(d)))


def iban_make(pays: str, bban: str) -> str:
    def num(s: str) -> str:
        return "".join(c if c.isdigit() else str(ord(c) - 55) for c in s)

    cle = 98 - int(num(bban + pays + "00")) % 97
    iban = f"{pays}{cle:02d}{bban}"
    assert int(num(iban[4:] + iban[:4])) % 97 == 1
    return " ".join(iban[i : i + 4] for i in range(0, len(iban), 4))


def _rib_fr() -> str:
    banque, guichet, compte = "30004", "00123", "12345678901"
    cle = 97 - (89 * int(banque) + 15 * int(guichet) + 3 * int(compte)) % 97
    return iban_make("FR", f"{banque}{guichet}{compte}{cle:02d}")


def _nir() -> str:
    base = "1850369123045"
    cle = 97 - int(base) % 97
    assert (int(base) + cle) % 97 == 0
    return f"{base[0]} {base[1:3]} {base[3:5]} {base[5:7]} {base[7:10]} {base[10:13]} {cle:02d}"


def _siret() -> str:
    siren8 = "41234567"
    siren = siren8 + luhn_cle(siren8)
    p13 = siren + "0001"
    siret = p13 + luhn_cle(p13)
    assert luhn_ok(siren) and luhn_ok(siret)
    return f"{siret[:3]} {siret[3:6]} {siret[6:9]} {siret[9:]}"


# ---------------------------------------------------------------------------
# Distribution : mêmes personnes et mêmes sociétés dans tous les fichiers
# ---------------------------------------------------------------------------

SOC_N = "Nautilus Pacifique SARL"
SOC_A = "Atelier Bougainville SAS"
SOC_H = "Harbour Analytics Pty Ltd"
JD, JD_MAJ, JD_INIT = "Jean Dupont", "DUPONT Jean", "J. Dupont"
MC = "Marie-Claire de La Fontaine"
HK = "Hélène Kaloï"
ML = "Martin Lefèvre"
PG = "Pierre Garnier"
AV = "Anaïs Vaitiare"
ON = "Olivier Nguyen"
EC = "Emily Carter"
CF = "Carlos Fernández Ruiz"
MJ = "María José Ortega Vidal"

EM = {
    JD: "jean.dupont@example.com",
    MC: "mc.delafontaine@example.org",
    HK: "helene.kaloi@example.com",
    ML: "martin.lefevre@example.com",
    PG: "pierre.garnier@example.org",
    AV: "anais.vaitiare@example.com",
    ON: "olivier.nguyen@example.org",
    EC: "emily.carter@example.com",
    CF: "carlos.fernandez@example.com",
    MJ: "maria.ortega@example.org",
}
TEL_JD_FR, TEL_JD_NC = "06 12 34 56 78", "+687 12 34 56"
TEL_MC = "+33 6 12 34 56 78"
TEL_HK = "+687 78 12 34"
TEL_ML = "07 11 22 33 44"
TEL_PG = "06 98 76 54 32"
TEL_AV = "+687 82 45 67"
TEL_ON = "+33 6 98 12 34 56"
TEL_N = "28 12 34"  # fixe Nautilus, écrit « Tél : 28 12 34 »

FONC = {
    JD: "gérant",
    MC: "directrice financière",
    HK: "chargée de projet",
    ML: "juriste d'entreprise",
    PG: "responsable technique",
    AV: "responsable commerciale",
    ON: "ingénieur réseau",
}
RIDET_N = "1 234 567.001"
ADR_N = "14 rue des Frangipaniers, 98800 Nouméa"
BP_N = "BP 1234, 98845 Nouméa Cedex"
ADR_A = "8 allée des Tilleuls, 69003 Lyon"
SIRET_A = _siret()
IBAN_A = _rib_fr()
NIR_JD = _nir()
CB = "4111 1111 1111 1111"
assert luhn_ok(CB.replace(" ", ""))
NON_SIREN = "123 456 789"
assert not luhn_ok(NON_SIREN.replace(" ", ""))
URL_PORTAIL = "https://portail.example.com/clients/nautilus"
IP = "192.0.2.45"
VERSION = "1.2.3.4"
IBAN_ES = iban_make("ES", "00491500051234567892")
IBAN_GB = iban_make("GB", "BARC20000012345678")

M_TTC, M_HT, M_TVA, M_ACOMPTE = "105 000,00 EUR", "87 500,00 EUR", "17 500,00 EUR", "31 500,00 EUR"

# ---------------------------------------------------------------------------
# Registre de la vérité terrain
# ---------------------------------------------------------------------------


class Fiche:
    def __init__(self, fichier: str, langue: str, notes: str):
        self.fichier, self.langue, self.notes = fichier, langue, notes
        self.entites: list[dict] = []
        self.pieges: list[str] = []

    def e(self, type_: str, valeur: str, *emplacements: str, ocr: bool = False) -> str:
        """Enregistre une entité et renvoie sa valeur (pour l'écrire dans le fichier)."""
        for ent in self.entites:
            if ent["type"] == type_ and ent["valeur"] == valeur and ent["ocr"] == ocr:
                for emp in emplacements:
                    if emp not in ent["emplacements"]:
                        ent["emplacements"].append(emp)
                return valeur
        self.entites.append(
            {
                "type": type_,
                "valeur": valeur,
                "profil": "renforce" if type_ in RENFORCE else "standard",
                "ocr": ocr,
                "emplacements": list(emplacements),
            }
        )
        return valeur

    def p(self, valeur: str) -> str:
        """Enregistre un piège à faux positif et renvoie sa valeur."""
        if valeur not in self.pieges:
            self.pieges.append(valeur)
        return valeur

    def json(self) -> dict:
        return {
            "fichier": self.fichier,
            "langue": self.langue,
            "notes": self.notes,
            "entites": self.entites,
            "ne_pas_detecter": self.pieges,
        }


# ---------------------------------------------------------------------------
# Utilitaires fichiers
# ---------------------------------------------------------------------------


def normaliser_zip(chemin: Path, modifs: dict | None = None) -> None:
    """Réécrit un zip avec des dates fixes (déterminisme) et des modifications ciblées."""
    modifs = modifs or {}
    with zipfile.ZipFile(chemin) as zin:
        items = [(zi, zin.read(zi.filename)) for zi in zin.infolist()]
    vus = set()
    with zipfile.ZipFile(chemin, "w") as zout:
        for zi, data in items:
            if zi.filename in modifs:
                data = modifs[zi.filename](data)
                vus.add(zi.filename)
            ni = zipfile.ZipInfo(zi.filename, DATE_ZIP)
            ni.compress_type = zi.compress_type
            ni.external_attr = zi.external_attr
            zout.writestr(ni, data)
    manquants = set(modifs) - vus
    if manquants:
        raise RuntimeError(f"parties absentes de {chemin.name} : {manquants}")


def police(taille: int, gras: bool = False) -> ImageFont.FreeTypeFont:
    import reportlab

    vera = Path(reportlab.__file__).parent / "fonts" / ("VeraBd.ttf" if gras else "Vera.ttf")
    candidats = [
        "C:/Windows/Fonts/arialbd.ttf" if gras else "C:/Windows/Fonts/arial.ttf",
        "/Library/Fonts/Arial Bold.ttf" if gras else "/Library/Fonts/Arial.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"
        if gras
        else "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        str(vera),
    ]
    for c in candidats:
        if Path(c).exists():
            return ImageFont.truetype(c, taille)
    raise RuntimeError("aucune police TrueType disponible")


def rendre_texte(lignes, largeur: int, hauteur: int, marge: int) -> Image.Image:
    """lignes : liste de (texte, taille_px, gras, espace_apres_px). Noir sur blanc, niveaux de gris."""
    img = Image.new("L", (largeur, hauteur), 255)
    dr = ImageDraw.Draw(img)
    y = marge
    for texte, taille, gras, apres in lignes:
        if texte:
            dr.text((marge, y), texte, font=police(taille, gras), fill=0)
        y += int(taille * 1.35) + apres
    assert y < hauteur, "texte trop long pour l'image"
    return img


def png_octets(img: Image.Image, dpi: int = 300) -> bytes:
    b = io.BytesIO()
    img.save(b, "PNG", dpi=(dpi, dpi))
    return b.getvalue()


# ---------------------------------------------------------------------------
# DOCX
# ---------------------------------------------------------------------------


def construire_contrat(d: Fiche, chemin: Path) -> None:
    from docx import Document
    from docx.opc.constants import RELATIONSHIP_TYPE as RT
    from docx.oxml import OxmlElement, parse_xml
    from docx.oxml.ns import nsdecls, qn

    E = d.e
    doc = Document()
    cp = doc.core_properties
    cp.author = E("PERSONNE", JD, "metadonnee")
    cp.last_modified_by = E("PERSONNE", ML, "metadonnee")
    cp.title = "Contrat de prestation de services"
    cp.created = cp.modified = DATE0
    cp.language = "fr-FR"
    E("SOCIETE", SOC_N, "metadonnee")

    sec = doc.sections[0]
    sec.header.paragraphs[0].text = (
        f"{E('SOCIETE', SOC_N, 'en-tete')} | {E('ADRESSE', BP_N, 'en-tete')} | "
        f"Tél : {E('TEL', TEL_N, 'en-tete')}"
    )
    sec.footer.paragraphs[0].text = (
        f"{E('PERSONNE', JD_INIT, 'pied')} | {E('EMAIL', EM[JD], 'pied')} | "
        f"{E('TEL', TEL_JD_NC, 'pied')}"
    )

    doc.add_heading("Contrat de prestation de services", level=1)
    doc.add_paragraph(MENTION)
    doc.add_paragraph("Entre les soussignés :")

    # Nom coupé entre deux runs de mise en forme différente (prénom en gras).
    p = doc.add_paragraph(
        f"{E('SOCIETE', SOC_N, 'corps')}, RIDET {E('RIDET', RIDET_N, 'corps')}, dont le siège est situé "
        f"{E('ADRESSE', ADR_N, 'corps')} ({E('ADRESSE', BP_N, 'corps')}), représentée par "
    )
    prenom, nom = JD.split(" ")
    r = p.add_run(prenom + " ")
    r.bold = True
    p.add_run(nom)
    E("PERSONNE", JD, "corps")
    p.add_run(f", {E('FONCTION', FONC[JD], 'corps')}.")

    doc.add_paragraph(
        f"et {E('SOCIETE', SOC_A, 'corps')}, SIRET {E('SIRET', SIRET_A, 'corps')}, dont le siège est situé "
        f"{E('ADRESSE', ADR_A, 'corps')}, représentée par {E('PERSONNE', MC, 'corps')}, "
        f"{E('FONCTION', FONC[MC], 'corps')}, née le {E('DATE_NAISS', '12 mai 1981', 'corps')} "
        f"({E('AGE', '44 ans', 'corps')})."
    )

    doc.add_heading("Article 1 - Objet", level=2)
    doc.add_paragraph(
        f"Le prestataire réalise une mission de conseil à {E('LIEU', 'Païta', 'corps')} du "
        f"{E('DATE', '1er septembre 2026', 'corps')} au {E('DATE', '31 décembre 2026', 'corps')}. "
        f"Cette clause est la {d.p('pierre angulaire')} du contrat. Une extension est envisagée en "
        f"{d.p('Martinique')}. Les spécifications suivent la version {d.p(VERSION)} de l'annexe technique "
        f"(dossier réf. {d.p(NON_SIREN.replace(' ', ''))})."
    )

    doc.add_heading("Article 2 - Prix et paiement", level=2)
    p_iban = doc.add_paragraph(
        f"Le prix est de {E('MONTANT', M_TTC, 'corps')} TTC, dont un acompte de "
        f"{E('MONTANT', M_ACOMPTE, 'corps')} versé le {E('DATE', '2 mars 2026', 'corps')} sur le compte "
    )
    run_iban = p_iban.add_run(f"IBAN {E('IBAN', IBAN_A, 'corps')}")
    p_iban.add_run(".")
    doc.add_comment(
        runs=run_iban,
        text=(
            f"À valider avec {E('PERSONNE', PG, 'commentaire')} ({E('TEL', TEL_PG, 'commentaire')}) "
            f"avant le {E('DATE', '20 février 2026', 'commentaire')}."
        ),
        author=E("PERSONNE", HK, "commentaire"),
        initials="HK",
    )

    doc.add_heading("Article 3 - Interlocuteurs", level=2)
    p_suivi = doc.add_paragraph(
        f"Suivi juridique : {E('PERSONNE', ML, 'corps')}, {E('FONCTION', FONC[ML], 'corps')}, joignable au "
        f"{E('TEL', TEL_ML, 'corps')}. "
    )
    # Lien mailto: (relation externe + w:hyperlink).
    rid = p_suivi.part.relate_to(f"mailto:{E('EMAIL', EM[ML], 'lien')}", RT.HYPERLINK, is_external=True)
    h = OxmlElement("w:hyperlink")
    h.set(qn("r:id"), rid)
    hr = OxmlElement("w:r")
    rpr = OxmlElement("w:rPr")
    coul = OxmlElement("w:color")
    coul.set(qn("w:val"), "0563C1")
    souligne = OxmlElement("w:u")
    souligne.set(qn("w:val"), "single")
    rpr.append(coul)
    rpr.append(souligne)
    hr.append(rpr)
    ht = OxmlElement("w:t")
    ht.text = f"Écrire à {ML}"
    hr.append(ht)
    h.append(hr)
    p_suivi._p.append(h)

    # Révision suivie : suppression puis insertion, XML injecté à la main.
    p_rev = doc.add_paragraph("Contact technique : ")
    auteur = E("PERSONNE", ML, "revision")
    date_rev = "2026-02-25T14:30:00Z"
    p_rev._p.append(
        parse_xml(
            f'<w:del {nsdecls("w")} w:id="9001" w:author="{auteur}" w:date="{date_rev}">'
            f'<w:r><w:delText xml:space="preserve">{E("PERSONNE", PG, "revision")}</w:delText></w:r></w:del>'
        )
    )
    p_rev._p.append(
        parse_xml(
            f'<w:ins {nsdecls("w")} w:id="9002" w:author="{auteur}" w:date="{date_rev}">'
            f'<w:r><w:t xml:space="preserve">{E("PERSONNE", ON, "revision")}</w:t></w:r></w:ins>'
        )
    )

    doc.add_paragraph(f"Fait à {E('LIEU', 'Nouméa', 'corps')}, le {E('DATE', '2 mars 2026', 'corps')}.")
    doc.add_paragraph(f"Pour {SOC_N} : {E('PERSONNE', JD_MAJ, 'corps')}")
    doc.save(chemin)

    def set_company(data: bytes) -> bytes:
        s = data.decode("utf-8")
        balise = f"<Company>{SOC_N}</Company>"
        if re.search(r"<Company\s*/>|<Company>.*?</Company>", s):
            s = re.sub(r"<Company\s*/>|<Company>.*?</Company>", balise, s)
        else:
            s = s.replace("</Properties>", balise + "</Properties>")
        return s.encode("utf-8")

    normaliser_zip(
        chemin,
        {
            "word/comments.xml": lambda b: re.sub(rb'w:date="[^"]*"', b'w:date="2026-02-20T10:00:00Z"', b),
            "docProps/app.xml": set_company,
        },
    )


def construire_report_en(d: Fiche, chemin: Path) -> None:
    from docx import Document

    E = d.e
    doc = Document()
    cp = doc.core_properties
    cp.author = E("PERSONNE", EC, "metadonnee")
    cp.title = "Quarterly status report"
    cp.created = cp.modified = DATE0
    cp.language = "en-US"
    doc.add_heading("Quarterly status report", level=1)
    doc.add_paragraph(MENTION)
    doc.add_paragraph(
        f"Prepared by {E('PERSONNE', EC, 'corps')}, {E('FONCTION', 'Chief Financial Officer', 'corps')} of "
        f"{E('SOCIETE', SOC_H, 'corps')}, for {E('PERSONNE', JD, 'corps')} of {E('SOCIETE', SOC_N, 'corps')}."
    )
    doc.add_paragraph(
        f"Our office is located at {E('ADRESSE', '221 Harbour Street, Sydney NSW 2000', 'corps')}. "
        f"Ms Carter, {E('AGE', '41 years old', 'corps')}, can be reached at {E('EMAIL', EM[EC], 'corps')} or on "
        f"{E('TEL', '+44 20 7946 0958', 'corps')} (UK) and {E('TEL', '+1 202 555 0143', 'corps')} (US)."
    )
    doc.add_paragraph(
        f"The review meeting took place on {E('DATE', 'March 14, 2026', 'corps')}. Total invoiced: "
        f"{E('MONTANT', 'USD 48,500.00', 'corps')}, to be paid to IBAN {E('IBAN', IBAN_GB, 'corps')}."
    )
    doc.add_paragraph(
        f"For questions in New Caledonia, call {E('PERSONNE', JD, 'corps')} on {E('TEL', TEL_JD_NC, 'corps')}. "
        f"The software release is version {d.p(VERSION)}."
    )
    doc.save(chemin)
    normaliser_zip(chemin)


# ---------------------------------------------------------------------------
# XLSX
# ---------------------------------------------------------------------------

CLIENTS = [
    # nom, prénom, personne (pour l'email), téléphone (str ou int), adresse, RIDET, montant
    ("Dupont", "Jean", JD, TEL_JD_FR, ADR_N, RIDET_N, 1250000),
    ("de La Fontaine", "Marie-Claire", MC, 33612345678, ADR_A, "2 345 678.001", 840000),
    ("Kaloï", "Hélène", HK, 687781234, "22 avenue des Flamboyants, 98835 Dumbéa", "3 456 789.001", 2310000),
    ("Lefèvre", "Martin", ML, TEL_ML, "5 rue du Lagon, 98890 Païta", "4 567 890.001", 475000),
    ("Garnier", "Pierre", PG, 281234, "31 route de la Baie, 98860 Koné", "5 678 901.001", 1980000),
    ("Vaitiare", "Anaïs", AV, TEL_AV, "3 impasse des Orchidées, 98800 Nouméa", "6 789 012.001", 620000),
    ("Nguyen", "Olivier", ON, TEL_ON, "17 rue des Lilas, 75011 Paris", "7 890 123.001", 3150000),
]
TEL_NOMBRES = [c[3] for c in CLIENTS if isinstance(c[3], int)]


def construire_xlsx(d: Fiche, chemin: Path) -> None:
    from openpyxl import Workbook
    from openpyxl.chart import BarChart, Reference
    from openpyxl.comments import Comment
    from openpyxl.worksheet.table import Table, TableStyleInfo

    E = d.e
    wb = Workbook()
    wb.properties.creator = E("PERSONNE", AV, "metadonnee")
    wb.properties.lastModifiedBy = E("PERSONNE", JD, "metadonnee")
    wb.properties.title = "Fichier clients"
    wb.properties.created = wb.properties.modified = DATE0
    ws = wb.active
    ws.title = "Clients"
    ws.append(["Nom", "Prénom", "Email", "Téléphone", "Adresse", "RIDET", "Montant"])
    for nom, prenom, pers, tel, adr, ridet, montant in CLIENTS:
        ws.append(
            [
                E("NOM", nom, "cellule"),
                E("PRENOM", prenom, "cellule"),
                E("EMAIL", EM[pers], "cellule"),
                tel,
                E("ADRESSE", adr, "cellule"),
                E("RIDET", ridet, "cellule"),
                montant,
            ]
        )
        E("TEL", str(tel), "cellule")
        E("MONTANT", str(montant), "cellule")
    for row in ws.iter_rows(min_row=2, min_col=7, max_col=7):
        row[0].number_format = "#,##0"
    for row in ws.iter_rows(min_row=2, min_col=4, max_col=4):
        if isinstance(row[0].value, int):
            row[0].number_format = "0"
    tab = Table(displayName="TableClients", ref=f"A1:G{len(CLIENTS) + 1}")
    tab.tableStyleInfo = TableStyleInfo(name="TableStyleMedium2", showRowStripes=True)
    ws.add_table(tab)
    ws["A10"] = MENTION
    ws["A11"] = (
        f'=CONCATENATE("Dossier suivi par {E("PERSONNE", HK, "formule")}, total : ",TEXT(SUM(G2:G8),"0")," XPF")'
    )
    cm = Comment(
        f"Relancer avant le {E('DATE', '30 novembre 2026', 'commentaire')} : voir {E('PERSONNE', PG, 'commentaire')}.",
        E("PERSONNE", JD, "commentaire"),
    )
    cm.width, cm.height = 320, 90
    ws["A2"].comment = cm

    ch = BarChart()
    ch.type = "col"
    ch.title = "Montants par client"
    ch.y_axis.title = "XPF"
    ch.add_data(Reference(ws, min_col=7, min_row=1, max_row=len(CLIENTS) + 1), titles_from_data=True)
    ch.set_categories(Reference(ws, min_col=1, min_row=2, max_row=len(CLIENTS) + 1))
    ch.height, ch.width = 8, 16
    ws.add_chart(ch, "I2")
    for col, w in zip("ABCDEFG", (16, 14, 30, 18, 42, 16, 12)):
        ws.column_dimensions[col].width = w

    ws2 = wb.create_sheet("Notes libres")
    ws2["A1"] = MENTION
    ws2["A2"] = (
        f"Réunion du {E('DATE', '14 mars 2026', 'cellule')} à {E('LIEU', 'Dumbéa', 'cellule')} avec "
        f"{E('PERSONNE', MC, 'cellule')}, {E('FONCTION', FONC[MC], 'cellule')}."
    )
    ws2["A3"] = (
        f"Contact : {E('PERSONNE', ON, 'cellule')}, {E('EMAIL', EM[ON], 'cellule')}, "
        f"{E('TEL', TEL_ON, 'cellule')}."
    )
    ws2["A4"] = f"Modèle de relance version {d.p(VERSION)} ; extension prévue en {d.p('Martinique')}."
    ws2["A5"] = f"Le suivi qualité reste la {d.p('pierre angulaire')} de l'offre."
    ws2.column_dimensions["A"].width = 100
    wb.save(chemin)
    normaliser_zip(chemin)


# ---------------------------------------------------------------------------
# PPTX
# ---------------------------------------------------------------------------


def image_contact_pptx(d: Fiche) -> bytes:
    lignes = [
        (MENTION, 30, False, 14),
        (f"Contact sécurité : {d.e('PERSONNE', ON, 'image', ocr=True)}", 46, True, 12),
        (d.e("TEL", TEL_ON, "image", ocr=True), 46, False, 12),
        (d.e("EMAIL", EM[ON], "image", ocr=True), 46, False, 0),
    ]
    return png_octets(rendre_texte(lignes, 1500, 470, 40))


def construire_pptx(d: Fiche, chemin: Path) -> None:
    from pptx import Presentation
    from pptx.chart.data import CategoryChartData
    from pptx.enum.chart import XL_CHART_TYPE
    from pptx.util import Inches

    E = d.e
    prs = Presentation()
    cp = prs.core_properties
    cp.author = E("PERSONNE", AV, "metadonnee")
    cp.last_modified_by = E("PERSONNE", PG, "metadonnee")
    cp.title = "Projet Lagon"
    cp.created = cp.modified = DATE0
    cp.revision = 1

    s1 = prs.slides.add_slide(prs.slide_layouts[0])
    s1.shapes.title.text = "Projet Lagon : revue de lancement"
    s1.placeholders[1].text = f"{MENTION}\nRevue du {E('DATE', '3 décembre 2026', 'corps')}"

    s2 = prs.slides.add_slide(prs.slide_layouts[1])
    s2.shapes.title.text = "Équipe projet"
    tf = s2.placeholders[1].text_frame
    tf.text = f"Chef de projet : {E('PERSONNE', JD, 'corps')}, {E('FONCTION', FONC[JD], 'corps')}"
    for ligne in (
        f"Responsable technique : {E('PERSONNE', PG, 'corps')}",
        f"Finance : {E('PERSONNE', MC, 'corps')}, {E('FONCTION', FONC[MC], 'corps')}",
        f"{E('FONCTION', FONC[HK], 'corps')} : {E('PERSONNE', HK, 'corps')}",
        f"Budget global : {E('MONTANT', M_TTC, 'corps')}",
    ):
        tf.add_paragraph().text = ligne
    s2.notes_slide.notes_text_frame.text = (
        f"Rappeler {E('PERSONNE', PG, 'note')} au {E('TEL', TEL_PG, 'note')} avant la revue. "
        f"Prévoir le déplacement à {E('LIEU', 'Païta', 'note')}."
    )

    s3 = prs.slides.add_slide(prs.slide_layouts[5])
    s3.shapes.title.text = "Charge prévisionnelle (jours)"
    cd = CategoryChartData()
    cd.categories = [E("PERSONNE", JD_INIT, "corps"), E("PERSONNE", PG, "corps"), E("PERSONNE", HK, "corps")]
    cd.add_series("Jours", (42, 65, 30))
    s3.shapes.add_chart(XL_CHART_TYPE.COLUMN_CLUSTERED, Inches(1), Inches(1.8), Inches(8), Inches(5), cd)

    s4 = prs.slides.add_slide(prs.slide_layouts[5])
    s4.shapes.title.text = "Contact et divers"
    s4.shapes.add_picture(io.BytesIO(image_contact_pptx(d)), Inches(0.7), Inches(1.8), width=Inches(8.6))
    tb = s4.shapes.add_textbox(Inches(0.7), Inches(5.2), Inches(8.6), Inches(1.5))
    tb.text_frame.word_wrap = True
    tb.text_frame.text = (
        f"Version du modèle : {d.p(VERSION)}. La documentation est la {d.p('pierre angulaire')} du projet ; "
        f"une extension en {d.p('Martinique')} est à l'étude."
    )
    prs.save(chemin)
    normaliser_zip(chemin)


# ---------------------------------------------------------------------------
# ODF
# ---------------------------------------------------------------------------


def meta_odf(doc, d: Fiche, auteur: str, titre: str) -> None:
    from odf.dc import Creator, Title
    from odf.dc import Date as DcDate
    from odf.meta import CreationDate, InitialCreator

    doc.meta.addElement(InitialCreator(text=d.e("PERSONNE", auteur, "metadonnee")))
    doc.meta.addElement(Creator(text=auteur))
    doc.meta.addElement(Title(text=titre))
    doc.meta.addElement(CreationDate(text="2026-03-02T09:00:00"))
    doc.meta.addElement(DcDate(text="2026-03-02T09:00:00"))


def construire_odt(d: Fiche, chemin: Path) -> None:
    from odf.opendocument import OpenDocumentText
    from odf.text import H, P

    E = d.e
    doc = OpenDocumentText()
    meta_odf(doc, d, HK, "Compte rendu de réunion")
    doc.text.addElement(H(outlinelevel=1, text="Compte rendu de réunion"))
    doc.text.addElement(P(text=MENTION))
    lignes = [
        f"Réunion du {E('DATE', '14 mars 2026', 'corps')} à {E('LIEU', 'Païta', 'corps')}, chez {E('SOCIETE', SOC_N, 'corps')}.",
        f"Présents : {E('PERSONNE', JD, 'corps')} ({E('FONCTION', FONC[JD], 'corps')}), "
        f"{E('PERSONNE', PG, 'corps')} ({E('FONCTION', FONC[PG], 'corps')}), {E('PERSONNE', ON, 'corps')} "
        f"({E('AGE', '38 ans', 'corps')}).",
        f"Contacts : {E('EMAIL', EM[JD], 'corps')}, {E('TEL', TEL_JD_FR, 'corps')} ; "
        f"{E('EMAIL', EM[PG], 'corps')}, {E('TEL', TEL_PG, 'corps')} ; {E('EMAIL', EM[ON], 'corps')}.",
        f"Décision : budget de {E('MONTANT', M_HT, 'corps')} HT validé ; prochaine réunion le "
        f"{E('DATE', '18 avril 2026', 'corps')}.",
        f"Remarque : {d.p('Martinique')} n'est pas concernée ; la qualité est la {d.p('pierre angulaire')} ; "
        f"version {d.p(VERSION)}.",
    ]
    for ligne in lignes:
        doc.text.addElement(P(text=ligne))
    doc.save(str(chemin))
    normaliser_zip(chemin)


def construire_ods(d: Fiche, chemin: Path) -> None:
    from odf.opendocument import OpenDocumentSpreadsheet
    from odf.table import Table, TableCell, TableRow
    from odf.text import P

    E = d.e
    doc = OpenDocumentSpreadsheet()
    meta_odf(doc, d, ML, "Planning")
    t = Table(name="Planning")

    def txt(s: str) -> TableCell:
        c = TableCell(valuetype="string")
        c.addElement(P(text=s))
        return c

    def nombre(n: int) -> TableCell:
        c = TableCell(valuetype="float", value=n)
        c.addElement(P(text=str(n)))
        return c

    def ligne(cells) -> None:
        r = TableRow()
        for c in cells:
            r.addElement(c)
        t.addElement(r)

    ligne([txt(x) for x in ("Nom", "Prénom", "Poste", "Email", "Téléphone", "Date", "Montant")])
    lignes = [
        ("DUPONT", "Jean", FONC[JD], EM[JD], TEL_JD_FR, "3 décembre 2026", 1250000),
        ("Garnier", "Pierre", FONC[PG], EM[PG], TEL_PG, "4 décembre 2026", 1980000),
        ("de La Fontaine", "Marie-Claire", FONC[MC], EM[MC], TEL_MC, "7 décembre 2026", 840000),
        ("Kaloï", "Hélène", FONC[HK], EM[HK], TEL_HK, "8 décembre 2026", 2310000),
    ]
    for nom, prenom, poste, mail, tel, date, montant in lignes:
        ligne(
            [
                txt(E("NOM", nom, "corps")),
                txt(E("PRENOM", prenom, "corps")),
                txt(E("FONCTION", poste, "corps")),
                txt(E("EMAIL", mail, "corps")),
                txt(E("TEL", tel, "corps")),
                txt(E("DATE", date, "corps")),
                nombre(E("MONTANT", str(montant), "corps") and montant),
            ]
        )
    ligne([txt(MENTION)])
    ligne([txt(f"Lieu des interventions : {E('LIEU', 'Koné', 'corps')} ; {d.p('Martinique')} hors périmètre ; version {d.p(VERSION)}.")])
    doc.spreadsheet.addElement(t)
    doc.save(str(chemin))
    normaliser_zip(chemin)


def construire_odp(d: Fiche, chemin: Path) -> None:
    from odf.draw import Frame, Page, TextBox
    from odf.opendocument import OpenDocumentPresentation
    from odf.namespaces import PRESENTATIONNS
    from odf.presentation import Notes
    from odf.style import MasterPage, PageLayout, PageLayoutProperties
    from odf.text import P

    E = d.e
    doc = OpenDocumentPresentation()
    meta_odf(doc, d, AV, "Support de présentation")
    pl = PageLayout(name="pm1")
    pl.addElement(PageLayoutProperties(pagewidth="28cm", pageheight="21cm", printorientation="landscape"))
    doc.automaticstyles.addElement(pl)
    mp = MasterPage(name="Standard", pagelayoutname=pl)
    doc.masterstyles.addElement(mp)

    def page(nom: str, lignes: list[str], note: str | None = None) -> None:
        pg = Page(masterpagename=mp, name=nom)
        fr = Frame(width="24cm", height="14cm", x="2cm", y="3cm")
        tb = TextBox()
        for ligne in lignes:
            tb.addElement(P(text=ligne))
        fr.addElement(tb)
        pg.addElement(fr)
        if note:
            n = Notes()
            nf = Frame(width="20cm", height="10cm", x="2cm", y="2cm")
            nf.setAttrNS(PRESENTATIONNS, "class", "notes")
            nt = TextBox()
            nt.addElement(P(text=note))
            nf.addElement(nt)
            n.addElement(nf)
            pg.addElement(n)
        doc.presentation.addElement(pg)

    page("page1", [MENTION, f"Support de formation de {E('SOCIETE', SOC_A, 'corps')}", f"Session du {E('DATE', '9 février 2026', 'corps')}"])
    page(
        "page2",
        [
            "Intervenants",
            f"{E('PERSONNE', MC, 'corps')}, {E('FONCTION', FONC[MC], 'corps')} : {E('EMAIL', EM[MC], 'corps')}",
            f"{E('PERSONNE', JD_MAJ, 'corps')}, {E('FONCTION', FONC[JD], 'corps')} : {E('TEL', TEL_JD_NC, 'corps')}",
            f"{E('PERSONNE', HK, 'corps')} : {E('TEL', TEL_HK, 'corps')}",
            f"Lieu : {E('LIEU', 'Dumbéa', 'corps')} ; {d.p('Martinique')} en visioconférence ; version {d.p(VERSION)}",
        ],
        note=f"Prévenir {E('PERSONNE', PG, 'note')} au {E('TEL', TEL_PG, 'note')}.",
    )
    doc.save(str(chemin))
    normaliser_zip(chemin)


# ---------------------------------------------------------------------------
# PDF
# ---------------------------------------------------------------------------


def construire_facture(d: Fiche, chemin: Path) -> None:
    from reportlab.lib.pagesizes import A4
    from reportlab.pdfgen import canvas

    E = d.e
    c = canvas.Canvas(str(chemin), pagesize=A4, invariant=1, pageCompression=0)
    c.setAuthor(E("PERSONNE", AV, "metadonnee"))
    c.setTitle("Facture n° 2026-0457")
    c.setSubject("Facture de prestation")
    c.setCreator("make_examples")
    lignes = [
        (MENTION, True),
        ("FACTURE n° 2026-0457", True),
        (f"Date : {E('DATE', '12 novembre 2026', 'corps')} - échéance : {E('DATE', '12 décembre 2026', 'corps')}", False),
        ("", False),
        ("Émetteur", True),
        (E("SOCIETE", SOC_A, "corps"), False),
        (E("ADRESSE", ADR_A, "corps"), False),
        (f"SIRET {E('SIRET', SIRET_A, 'corps')}", False),
        (f"Contact : {E('PERSONNE', MC, 'corps')} - {E('EMAIL', EM[MC], 'corps')} - {E('TEL', TEL_MC, 'corps')}", False),
        ("", False),
        ("Client", True),
        (E("SOCIETE", SOC_N, "corps"), False),
        (f"À l'attention de {E('PERSONNE', JD, 'corps')}, {E('FONCTION', FONC[JD], 'corps')}", False),
        (E("ADRESSE", ADR_N, "corps"), False),
        (f"RIDET {E('RIDET', RIDET_N, 'corps')} - {E('EMAIL', EM[JD], 'corps')}", False),
        ("", False),
        (f"Prestation de conseil à {E('LIEU', 'Païta', 'corps')} (septembre à décembre 2026) : {E('MONTANT', M_HT, 'corps')} HT", False),
        (f"TVA 20 % : {E('MONTANT', M_TVA, 'corps')}", False),
        (f"Total TTC : {E('MONTANT', M_TTC, 'corps')}", True),
        ("", False),
        (f"Règlement par virement : IBAN {E('IBAN', IBAN_A, 'corps')}", False),
        (f"Règlement par carte : {E('CB', CB, 'corps')}", False),
        ("", False),
        (f"Réf. dossier {d.p(NON_SIREN.replace(' ', ''))} - logiciel de facturation version {d.p(VERSION)}", False),
        (f"Livraison hors {d.p('Martinique')}. La qualité de service est la {d.p('pierre angulaire')} de notre offre.", False),
    ]
    y = 800
    for texte, gras in lignes:
        c.setFont("Helvetica-Bold" if gras else "Helvetica", 10.5)
        if texte:
            c.drawString(56, y, texte)
        y -= 17
    c.showPage()
    c.save()


def construire_courrier(d: Fiche, chemin: Path) -> None:
    from reportlab.lib.pagesizes import A4
    from reportlab.lib.utils import ImageReader
    from reportlab.pdfgen import canvas

    E = lambda t, v: d.e(t, v, "image", ocr=True)  # noqa: E731
    corps = textwrap.wrap(
        f"Suite à notre entretien du {E('DATE', '14 mars 2026')} à {E('LIEU', 'Païta')}, je vous confirme le "
        f"virement de {E('MONTANT', M_TTC)} sur le compte IBAN {E('IBAN', IBAN_A)}. En cas de question, "
        f"merci de contacter {E('PERSONNE', PG)} au {E('TEL', TEL_PG)}.",
        width=78,
    )
    corps2 = textwrap.wrap(
        f"Le dossier {d.p(NON_SIREN.replace(' ', ''))} reste ouvert jusqu'à la livraison, y compris pour "
        f"{d.p('Martinique')}.",
        width=78,
    )
    T, G, S = 44, True, 0
    lignes = [
        (MENTION, 36, G, 40),
        (E("SOCIETE", SOC_A), T, G, S),
        (E("ADRESSE", ADR_A), T, False, S),
        (f"Tél : {E('TEL', TEL_MC)} - {E('EMAIL', EM[MC])}", T, False, 60),
        (f"À l'attention de {E('PERSONNE', JD)}", T, False, S),
        (E("SOCIETE", SOC_N), T, False, S),
        (E("ADRESSE", BP_N), T, False, 60),
        (f"{E('LIEU', 'Lyon')}, le {E('DATE', '12 novembre 2026')}", T, False, 60),
        ("Objet : règlement de la facture n° 2026-0457", T, G, 40),
        ("Monsieur,", T, False, 20),
    ]
    lignes += [(x, T, False, S) for x in corps] + [("", T, False, 10)]
    lignes += [(x, T, False, S) for x in corps2]
    lignes += [
        ("", T, False, 30),
        ("Cordialement,", T, False, 40),
        (E("PERSONNE", MC), T, G, S),
        (E("FONCTION", FONC[MC]), T, False, S),
    ]
    img = rendre_texte(lignes, 2480, 3508, 230)
    c = canvas.Canvas(str(chemin), pagesize=A4, invariant=1, pageCompression=0)
    c.setTitle("Courrier numérisé")
    c.drawImage(ImageReader(img), 0, 0, width=A4[0], height=A4[1])
    c.showPage()
    c.save()


def construire_carte(d: Fiche, chemin: Path) -> None:
    E = lambda t, v: d.e(t, v, "image", ocr=True)  # noqa: E731
    lignes = [
        (E("PERSONNE", AV), 72, True, 8),
        (E("FONCTION", FONC[AV]), 46, False, 30),
        (E("SOCIETE", SOC_A), 50, True, 30),
        (f"Tél : {E('TEL', TEL_AV)}", 44, False, 4),
        (E("EMAIL", EM[AV]), 44, False, 30),
        (MENTION, 30, False, 0),
    ]
    chemin.write_bytes(png_octets(rendre_texte(lignes, 1500, 860, 60)))


# ---------------------------------------------------------------------------
# Texte brut
# ---------------------------------------------------------------------------


def construire_notes(d: Fiche, chemin: Path) -> None:
    E = d.e
    t = f"""{MENTION}

Notes de réunion du {E('DATE', '14 mars 2026', 'corps')} - {E('LIEU', 'Dumbéa', 'corps')}

Présents : {E('PERSONNE', JD_MAJ, 'corps')} ({E('FONCTION', FONC[JD], 'corps')} de {E('SOCIETE', SOC_N, 'corps')}),
{E('PERSONNE', PG, 'corps')} ({E('AGE', '62 ans', 'corps')}, {E('FONCTION', FONC[PG], 'corps')}) et {E('PERSONNE', HK, 'corps')}.
Le dossier sera repris ensuite par {E('PERSONNE', JD_INIT, 'corps')}.

Coordonnées :
- {E('PERSONNE', JD, 'corps')} : {E('EMAIL', EM[JD], 'corps')}, {E('TEL', TEL_JD_FR, 'corps')} ou {E('TEL', TEL_JD_NC, 'corps')}
- {E('PERSONNE', MC, 'corps')} : {E('EMAIL', EM[MC], 'corps')}, {E('TEL', TEL_MC, 'corps')}
- Standard Nautilus - Tél : {E('TEL', TEL_N, 'corps')}

Données administratives (fictives) :
- NIR de {E('PERSONNE', JD_MAJ, 'corps')} : {E('NIR', NIR_JD, 'corps')}
- Carte de paiement de test : {E('CB', CB, 'corps')}
- Véhicule de service : {E('PLAQUE', '123 456 NC', 'corps')} ; matricule agent {E('MATRICULE', 'MAT-004517', 'corps')}
- Serveur de test : {E('IP', IP, 'corps')} - portail : {E('URL', URL_PORTAIL, 'corps')}
- Budget : {E('MONTANT', '12 500 000 XPF', 'corps')}, livraison à {E('LIEU', 'Koné', 'corps')} avant le {E('DATE', '31 décembre 2026', 'corps')}.

Pièges à faux positifs :
- Une antenne est prévue en {d.p('Martinique')}.
- Cette règle est la {d.p('pierre angulaire')} de la méthode.
- Le module est en version {d.p(VERSION)}.
- La référence {d.p(NON_SIREN.replace(' ', ''))} n'est pas un SIREN valide.
"""
    chemin.write_text(t, encoding="utf-8", newline="\n")


def construire_csv(d: Fiche, chemin: Path) -> None:
    E = d.e
    lignes = ["Nom;Prénom;Email;Téléphone;Ville;Date d'entrée;Montant"]
    data = [
        ("DUPONT", "Jean", JD, TEL_JD_FR, "Nouméa", "2024-03-14", 1250000),
        ("de La Fontaine", "Marie-Claire", MC, TEL_MC, "Lyon", "2023-11-02", 840000),
        ("KALOÏ", "Hélène", HK, TEL_HK, "Dumbéa", "2025-01-20", 2310000),
        ("Lefèvre", "Martin", ML, TEL_ML, "Païta", "2022-06-09", 475000),
        ("Garnier", "Pierre", PG, TEL_PG, "Koné", "2024-09-30", 1980000),
        ("Vaitiare", "Anaïs", AV, TEL_AV, "Nouméa", "2025-05-12", 620000),
    ]
    for nom, prenom, pers, tel, ville, date, montant in data:
        lignes.append(
            ";".join(
                [
                    E("NOM", nom, "cellule"),
                    E("PRENOM", prenom, "cellule"),
                    E("EMAIL", EM[pers], "cellule"),
                    E("TEL", tel, "cellule"),
                    E("LIEU", ville, "cellule"),
                    E("DATE", date, "cellule"),
                    E("MONTANT", str(montant), "cellule"),
                ]
            )
        )
    lignes.append(f"{MENTION};;;;;;")
    chemin.write_text("\n".join(lignes) + "\n", encoding="utf-8", newline="\n")


def construire_brief(d: Fiche, chemin: Path) -> None:
    E = d.e
    t = f"""# Brief de projet

> {MENTION}

## Contexte

Le projet est porté par **{E('SOCIETE', SOC_N, 'corps')}** ({E('ADRESSE', ADR_N, 'corps')}). Le responsable est
**{E('PERSONNE', JD, 'corps')}**, {E('FONCTION', FONC[JD], 'corps')}, joignable au {E('TEL', TEL_JD_FR, 'corps')}.

## Équipe

| Rôle | Nom | Contact |
|---|---|---|
| {FONC[PG].capitalize()} | {E('PERSONNE', PG, 'corps')} | [{E('EMAIL', EM[PG], 'corps')}](mailto:{E('EMAIL', EM[PG], 'lien')}) |
| {FONC[HK].capitalize()} | {E('PERSONNE', HK, 'corps')} | {E('TEL', TEL_HK, 'corps')} |

## Planning

- Lancement le {E('DATE', '3 décembre 2026', 'corps')} à {E('LIEU', 'Koné', 'corps')}.
- Enveloppe : {E('MONTANT', M_TTC, 'corps')}.
- Suivi sur le [portail client]({E('URL', URL_PORTAIL, 'lien')}), serveur de recette `{E('IP', IP, 'corps')}`.

## Remarques

L'extension en {d.p('Martinique')} est reportée. La relecture reste la {d.p('pierre angulaire')} du projet.
Version du gabarit : {d.p(VERSION)}.
"""
    chemin.write_text(t, encoding="utf-8", newline="\n")


def construire_es(d: Fiche, chemin: Path) -> None:
    E = d.e
    t = f"""{MENTION}

Informe de situación del {E('DATE', '14 de marzo de 2026', 'corps')}

Redactado por {E('PERSONNE', CF, 'corps')}, {E('FONCTION', 'director financiero', 'corps')}, de {E('SOCIETE', SOC_N, 'corps')}.
Revisado por {E('PERSONNE', MJ, 'corps')} ({E('AGE', '52 años', 'corps')}).

Dirección de la oficina: {E('ADRESSE', 'Calle de las Acacias 12, 28013 Madrid', 'corps')}.
Contacto: {E('EMAIL', EM[CF], 'corps')} y {E('EMAIL', EM[MJ], 'corps')}.
Teléfonos: {E('TEL', '+34 612 34 56 78', 'corps')} (España), {E('TEL', '+52 55 1234 5678', 'corps')} (México),
{E('TEL', TEL_JD_NC, 'corps')} ({E('PERSONNE', JD, 'corps')}, Nueva Caledonia).

Importe facturado: {E('MONTANT', '48.500,00 EUR', 'corps')}, a pagar en la cuenta {E('IBAN', IBAN_ES, 'corps')}.
Próxima reunión en {E('LIEU', 'Valencia', 'corps')} el {E('DATE', '2 de abril de 2026', 'corps')}.
Versión del informe: {d.p(VERSION)}.
"""
    chemin.write_text(t, encoding="utf-8", newline="\n")


def construire_reponse_ia(d: Fiche, chemin: Path) -> None:
    d.p("[PERSONNE_1]")
    d.p("[SOCIETE_1]")
    d.p("[PERSONNE_99]")
    t = f"""{MENTION}

# Synthèse du contrat (réponse simulée d'une IA)

Le contrat lie [SOCIETE_1] et [SOCIETE_2]. [PERSONNE_1] représente le premier prestataire et [PERSONNE_2] la
seconde société ; le paiement se fait sur le compte [IBAN_1] pour un montant de [MONTANT_1].

## Balises altérées par l'IA

- Casse et espace modifiées : [Personne 1] doit être reconnue comme [PERSONNE_1].
- Soulignement échappé (Markdown) : [PERSONNE\\_1] reste la même personne.
- Balise mise en gras : **[SOCIETE_1]** désigne toujours la première société.
- Minuscules : [personne_2] correspond à [PERSONNE_2].

## Balise inventée

L'IA mentionne aussi [PERSONNE_99], qui n'existe pas dans la session : elle doit être signalée et laissée telle quelle.

## Coordonnées à utiliser

Écrire à [EMAIL_1] ou appeler le [TEL_1] ; adresse du siège : [ADRESSE_1].
"""
    chemin.write_text(t, encoding="utf-8", newline="\n")


# ---------------------------------------------------------------------------
# LISEZMOI
# ---------------------------------------------------------------------------

LISEZMOI = f"""# Jeu de fichiers d'exemple

{MENTION}

Tous les fichiers de ce dossier sont **entièrement fictifs** et générés par `tools/make_examples.py`
(`uv run --project tools python tools/make_examples.py`). Les emails sont en `example.com` ou `example.org`.
La vérité terrain lisible par les tests est dans `attendu.json` : toute valeur sensible d'un fichier y figure.

## Contenu

| Fichier | Ce qu'il permet de vérifier |
|---|---|
| `contrat-prestation.docx` | nom coupé entre deux mises en forme, en-tête, pied de page, commentaire, révision suivie, lien `mailto:`, propriétés |
| `fichier-clients.xlsx` | tableau Excel, téléphones et montants saisis en nombres, formule, commentaire de cellule, graphique, 2e feuille |
| `presentation-projet.pptx` | diapositives, notes du présentateur, graphique, image contenant du texte |
| `facture.pdf` | PDF avec texte, métadonnées |
| `courrier-scanne.pdf`, `carte-visite.png` | images seules : aucun texte extractible, OCR nécessaire |
| `compte-rendu.odt`, `planning.ods`, `support.odp` | formats OpenDocument |
| `notes.txt`, `export.csv`, `brief.md` | texte brut, CSV à séparateur `;`, Markdown |
| `report-en.docx`, `informe-es.txt` | anglais et espagnol, téléphones internationaux |
| `reponse-ia-exemple.md` | réponse d'IA avec balises correctes, altérées et inventée |

Les mêmes sociétés et les mêmes sept personnes reviennent d'un fichier à l'autre : Jean Dupont (écrit aussi
`DUPONT Jean` et `J. Dupont`), Marie-Claire de La Fontaine, Hélène Kaloï, Martin Lefèvre, Pierre Garnier,
Anaïs Vaitiare, Olivier Nguyen.

## Scénario de test guidé

### 1. Anonymiser le contrat

1. Ouvrez l'application et déposez `contrat-prestation.docx`.
2. Vérifiez à l'écran que la liste des détections contient : les deux sociétés, les personnes, le RIDET, le
   SIRET, l'IBAN, les adresses (dont la boîte postale), les emails, les téléphones, les dates et les montants.
3. Contrôlez les emplacements difficiles : le nom « Jean Dupont » écrit en deux mises en forme (prénom en gras),
   l'en-tête, le pied de page, le commentaire de Hélène Kaloï, la révision suivie (Pierre Garnier supprimé,
   Olivier Nguyen inséré), le lien `mailto:` et les propriétés du document (auteur, société).
4. Contrôlez aussi que **ne sont pas détectés** : « Martinique », « pierre angulaire », « 1.2.3.4 » et
   « 123456789 ».
5. Validez, anonymisez, puis ouvrez le fichier produit : plus aucune valeur sensible ne doit y subsister, y compris
   dans le commentaire, la révision, le lien et les propriétés. Le contrôle de fuite doit passer sans alerte.

### 2. Cohérence des balises

Dans la même session, ajoutez `facture.pdf`, `notes.txt` et `brief.md`. Une même personne ou société doit
recevoir la même balise partout, y compris sous ses variantes (`Jean Dupont`, `DUPONT Jean`, `J. Dupont`).

### 3. Tableur

Déposez `fichier-clients.xlsx` : les colonnes Nom, Prénom, Email, Téléphone, Adresse, RIDET et Montant doivent être
reconnues. Vérifiez les téléphones saisis en nombres (par exemple `33612345678`), la formule (le nom dans le texte
entre guillemets), le commentaire de la cellule A2 et la feuille « Notes libres ». Le graphique doit survivre à
l'export. Aucun tableau croisé dynamique n'est fourni : Excel n'était pas pilotable par COM sur la machine de
génération.

### 4. Images et OCR

Déposez `courrier-scanne.pdf` et `carte-visite.png` : aucun texte n'est extractible, l'application doit passer par
l'OCR et proposer les valeurs (marquées `ocr: true` dans `attendu.json`). Dans `presentation-projet.pptx`, l'image de
la quatrième diapositive contient un contact (Olivier Nguyen) que seul l'OCR peut trouver ; les notes du
présentateur contiennent un nom et un téléphone.

### 5. Profil renforcé

Relancez l'analyse d'un fichier en profil renforcé : les dates, âges, fonctions (« directrice financière »),
montants et communes (Païta, Dumbéa, Koné) apparaissent en plus. Ils sont marqués `renforce` dans `attendu.json`.

### 6. Restauration

Après avoir anonymisé le contrat, collez `reponse-ia-exemple.md` dans la zone de restauration. Les balises
exactes sont remplacées par les vraies valeurs ; les formes altérées (`[Personne 1]`, `[PERSONNE\\_1]`,
`**[SOCIETE_1]**`, `[personne_2]`) sont reconnues ; `[PERSONNE_99]` est signalée comme inconnue et reste intacte.
Si votre numérotation diffère de celle du fichier (ordre de détection), adaptez les numéros avant de comparer.

### 7. Autres formats et langues

Déposez `compte-rendu.odt`, `planning.ods`, `support.odp` (auteur en métadonnée, notes du présentateur dans
l'ODP), `export.csv` (séparateur `;`), `report-en.docx` et `informe-es.txt` (téléphones et IBAN internationaux).

## Contrôle automatique

`cargo test -p anon-engine` rejoue ces fichiers contre `attendu.json`. Chaque nouveau cas piège doit recevoir son
fichier ici et son entrée dans `attendu.json`.
"""

# ---------------------------------------------------------------------------
# Auto-contrôle
# ---------------------------------------------------------------------------


def _zip_txt(z: zipfile.ZipFile, motifs: list[str], sep: bool, brut: bool = True) -> str:
    out = []
    for n in z.namelist():
        if any(re.fullmatch(m, n) for m in motifs):
            x = z.read(n).decode("utf-8", "replace")
            s = re.sub(r"</(?:w:p|a:p)>", "\n", x)
            s = re.sub(r"<[^>]+>", "\n" if sep else "", s)
            out.append(html.unescape(s))
            if brut:
                out.append(html.unescape(x))
    return "\n".join(out)


def _pdf_unescape(b: bytes) -> bytes:
    def rep(m):
        g = m.group(1)
        if g[:1].isdigit():
            return bytes([int(g, 8) & 0xFF])
        return {b"n": b"\n", b"r": b"\r", b"t": b"\t", b"b": b"\b", b"f": b"\f"}.get(g, g)

    return re.sub(rb"\\([0-7]{1,3}|.)", rep, b, flags=re.S)


def pdf_texte(data: bytes) -> tuple[str, str]:
    """Extraction minimale (flux de contenu, opérateurs Tj/TJ) et auteur des métadonnées."""
    morceaux = []
    for m in re.finditer(rb"stream\r?\n(.*?)\r?\nendstream", data, re.S):
        s = m.group(1).strip()
        try:
            if s.endswith(b"~>"):
                s = base64.a85decode(s[2:-2] if s.startswith(b"<~") else s[:-2])
        except Exception:
            pass
        try:
            s = zlib.decompress(s)
        except Exception:
            pass
        for t in re.finditer(rb"\(((?:\\.|[^\\()])*)\)\s*Tj", s, re.S):
            morceaux.append(_pdf_unescape(t.group(1)).decode("cp1252", "replace"))
        for t in re.finditer(rb"\[(.*?)\]\s*TJ", s, re.S):
            for u in re.finditer(rb"\(((?:\\.|[^\\()])*)\)", t.group(1), re.S):
                morceaux.append(_pdf_unescape(u.group(1)).decode("cp1252", "replace"))
    meta = ""
    for champ in (b"Author", b"Title"):
        m = re.search(rb"/" + champ + rb"\s*(\((?:\\.|[^\\)])*\)|<[0-9A-Fa-f]+>)", data)
        if m:
            v = m.group(1)
            if v.startswith(b"<"):
                raw = bytes.fromhex(v[1:-1].decode())
            else:
                raw = _pdf_unescape(v[1:-1])
            meta += "\n" + (raw[2:].decode("utf-16-be") if raw[:2] == b"\xfe\xff" else raw.decode("cp1252", "replace"))
    return "\n".join(morceaux), meta


def seaux(chemin: Path) -> dict[str, str] | None:
    """Extrait le texte par emplacement. None pour les images pures."""
    ext = chemin.suffix.lower()
    if ext in (".txt", ".csv", ".md"):
        t = chemin.read_text(encoding="utf-8")
        return {k: t for k in ("corps", "cellule", "lien", "note", "commentaire")}
    if ext == ".pdf":
        corps, meta = pdf_texte(chemin.read_bytes())
        return {"corps": corps, "metadonnee": meta}
    if ext == ".png":
        return None
    z = zipfile.ZipFile(chemin)
    if ext == ".docx":
        doc = _zip_txt(z, [r"word/document\.xml", r"word/footnotes\.xml"], False)
        return {
            "corps": doc,
            "revision": doc,
            "en-tete": _zip_txt(z, [r"word/header\d*\.xml"], False),
            "pied": _zip_txt(z, [r"word/footer\d*\.xml"], False),
            "commentaire": _zip_txt(z, [r"word/comments\.xml"], False),
            "lien": _zip_txt(z, [r"word/_rels/document\.xml\.rels"], False),
            "metadonnee": _zip_txt(z, [r"docProps/(core|app)\.xml"], True),
        }
    if ext == ".xlsx":
        cell = _zip_txt(z, [r"xl/sharedStrings\.xml", r"xl/worksheets/sheet\d+\.xml", r"xl/tables/.*"], True)
        return {
            "cellule": cell,
            "formule": _zip_txt(z, [r"xl/worksheets/sheet\d+\.xml"], True),
            "commentaire": _zip_txt(z, [r"xl/comments.*\.xml"], True),
            "metadonnee": _zip_txt(z, [r"docProps/(core|app)\.xml"], True),
        }
    if ext == ".pptx":
        return {
            "corps": _zip_txt(z, [r"ppt/slides/slide\d+\.xml", r"ppt/charts/chart\d+\.xml"], False),
            "note": _zip_txt(z, [r"ppt/notesSlides/notesSlide\d+\.xml"], False),
            "metadonnee": _zip_txt(z, [r"docProps/(core|app)\.xml"], True),
            "image": "\n".join(n for n in z.namelist() if n.startswith("ppt/media/")),
        }
    if ext in (".odt", ".ods", ".odp"):
        c = _zip_txt(z, [r"content\.xml"], True, brut=False)
        return {"corps": c, "cellule": c, "note": c, "metadonnee": _zip_txt(z, [r"meta\.xml"], True)}
    raise RuntimeError(f"format inconnu : {chemin.name}")


def controle(fiches: list[Fiche]) -> bool:
    print("=== Auto-contrôle ===")
    ok_global = True
    for f in fiches:
        chemin = SORTIE / f.fichier
        erreurs = []
        try:
            b = seaux(chemin)
        except Exception as exc:  # noqa: BLE001
            print(f"ECHEC  {f.fichier} : lecture impossible ({exc})")
            ok_global = False
            continue
        n = 0
        if b is None:  # image pure
            with Image.open(chemin) as im:
                if im.width < 1000:
                    erreurs.append("image trop petite")
            if any(not e["ocr"] for e in f.entites):
                erreurs.append("entité non OCR dans une image")
        else:
            tout = "\n".join(b.values())
            if chemin.suffix == ".pdf" and all(e["ocr"] for e in f.entites) and f.entites:
                if b["corps"].strip():
                    erreurs.append("texte extractible dans un PDF image")
                if "/Font" in chemin.read_bytes().decode("latin-1") and re.search(rb"\) ?Tj", chemin.read_bytes()):
                    erreurs.append("opérateur de texte dans un PDF image")
            for e in f.entites:
                if e["ocr"]:
                    if e["valeur"] in tout:
                        erreurs.append(f"valeur OCR présente en texte : {e['valeur']!r}")
                    n += 1
                    continue
                for emp in e["emplacements"]:
                    seau = b.get(emp)
                    if seau is None or e["valeur"] not in seau:
                        erreurs.append(f"{e['type']} {e['valeur']!r} absent de [{emp}]")
                    n += 1
            if f.entites and any(e["ocr"] for e in f.entites) and "image" in b and not b["image"]:
                erreurs.append("aucune image embarquée")
            for v in f.pieges:
                if v not in tout and not (f.entites and all(e["ocr"] for e in f.entites)):
                    erreurs.append(f"piège absent : {v!r}")
        # Contrôles propres à certains fichiers
        if f.fichier == "contrat-prestation.docx":
            z = zipfile.ZipFile(chemin)
            xml = z.read("word/document.xml").decode("utf-8")
            if re.search(r"<w:t[^>]*>[^<]*Jean Dupont", xml):
                erreurs.append("Jean Dupont n'est pas coupé entre deux runs")
            if "<w:ins " not in xml or "<w:del " not in xml:
                erreurs.append("révision suivie absente")
            if "mailto:" not in z.read("word/_rels/document.xml.rels").decode():
                erreurs.append("lien mailto absent")
        if f.fichier == "fichier-clients.xlsx":
            from openpyxl import load_workbook

            wb = load_workbook(chemin)
            ws = wb["Clients"]
            nombres = [ws.cell(r, 4).value for r in range(2, 9) if isinstance(ws.cell(r, 4).value, int)]
            if nombres != TEL_NOMBRES:
                erreurs.append("téléphones numériques manquants")
            if "TableClients" not in ws.tables:
                erreurs.append("objet Table absent")
            z = zipfile.ZipFile(chemin)
            if not any(x.startswith("xl/charts/chart") for x in z.namelist()):
                erreurs.append("graphique absent")
            if not str(ws["A11"].value).startswith("="):
                erreurs.append("formule absente")
        if f.fichier == "presentation-projet.pptx":
            z = zipfile.ZipFile(chemin)
            noms = z.namelist()
            if not any(x.startswith("ppt/charts/chart") for x in noms) or not any(
                x.startswith("ppt/embeddings/") for x in noms
            ):
                erreurs.append("graphique ou classeur embarqué absent")
        if erreurs:
            ok_global = False
            print(f"ECHEC  {f.fichier}")
            for x in erreurs:
                print(f"         - {x}")
        else:
            print(f"OK     {f.fichier} ({n} vérifications, {len(f.entites)} entités)")
    return ok_global


# ---------------------------------------------------------------------------
# Programme principal
# ---------------------------------------------------------------------------


def main() -> int:
    SORTIE.mkdir(exist_ok=True)
    plan = [
        ("contrat-prestation.docx", "fr", construire_contrat,
         "Nom coupé entre deux runs, en-tête, pied, commentaire, révision suivie, lien mailto, propriétés, RIDET, IBAN, SIRET, BP."),
        ("fichier-clients.xlsx", "fr", construire_xlsx,
         "Objet Table, téléphones et montants numériques, formule avec littéral nominatif, commentaire de cellule, graphique, feuille libre ; pas de TCD."),
        ("presentation-projet.pptx", "fr", construire_pptx,
         "Noms d'équipe, notes du présentateur, graphique avec noms en catégories (emplacement corps), image contenant du texte (OCR)."),
        ("facture.pdf", "fr", construire_facture,
         "PDF texte : émetteur, client, SIRET, IBAN, carte bancaire, email, auteur en métadonnée, pièges Martinique et pierre angulaire."),
        ("courrier-scanne.pdf", "fr", construire_courrier,
         "PDF image seule sans texte extractible : toutes les valeurs ne sont trouvables que par OCR."),
        ("carte-visite.png", "fr", construire_carte, "Image seule : nom, fonction, société, téléphone, email (OCR)."),
        ("compte-rendu.odt", "fr", construire_odt, "Texte ODF avec noms, emails, téléphones, auteur en métadonnée."),
        ("planning.ods", "fr", construire_ods, "Tableur ODF avec colonnes nominatives, montants numériques, auteur en métadonnée."),
        ("support.odp", "fr", construire_odp, "Présentation ODF avec noms, notes du présentateur, auteur en métadonnée."),
        ("notes.txt", "fr", construire_notes,
         "Texte brut : NIR, carte, plaque, matricule, IP, URL, formes DUPONT Jean et J. Dupont, pièges."),
        ("export.csv", "fr", construire_csv, "CSV UTF-8 séparateur point-virgule : noms en majuscules, à particule, accentués."),
        ("brief.md", "fr", construire_brief, "Markdown avec tableau, lien mailto, URL, IP, pièges."),
        ("report-en.docx", "en", construire_report_en, "Document anglais : noms, emails, téléphones UK et US, IBAN GB."),
        ("informe-es.txt", "es", construire_es, "Texte espagnol : noms, emails, téléphones ES et MX, IBAN ES."),
        ("reponse-ia-exemple.md", "fr", construire_reponse_ia,
         "Aucune donnée sensible : sert à tester la restauration (balises correctes, altérées [Personne 1], [PERSONNE\\_1], **[SOCIETE_1]**, [personne_2], inventée [PERSONNE_99])."),
    ]
    fiches = []
    for nom, langue, fn, notes in plan:
        f = Fiche(nom, langue, notes)
        fn(f, SORTIE / nom)
        fiches.append(f)

    attendu = json.dumps({"version": 1, "fichiers": [f.json() for f in fiches]}, ensure_ascii=False, indent=2) + "\n"
    (SORTIE / "attendu.json").write_text(attendu, encoding="utf-8", newline="\n")
    (SORTIE / "LISEZMOI.md").write_text(LISEZMOI, encoding="utf-8", newline="\n")

    ok = controle(fiches)
    print("\n=== Fichiers ===")
    for p in sorted(SORTIE.iterdir()):
        print(f"{p.stat().st_size:>9} octets  {p.name}")
    print("\nSHA-256 attendu.json :", hashlib.sha256(attendu.encode("utf-8")).hexdigest())
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
