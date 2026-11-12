/*
 * Cale C : OCR Tesseract (embarqué dans MuPDF) sur une image en niveaux de gris
 * fournie en mémoire. Aucun fichier n'est écrit.
 *
 * Toutes les exceptions MuPDF sont rattrapées ici, sous fz_try, avec un contexte
 * fz_context propre à l'appel : aucun longjmp ne traverse de cadre Rust.
 *
 * Leptonica (utilisé par Tesseract) a un allocateur global dans MuPDF : un seul
 * OCR à la fois dans le processus. L'appelant Rust sérialise les appels.
 */

#include <string.h>
#include "mupdf/fitz.h"

/* Fonctions internes de MuPDF (source/fitz/tessocr.h), exportées par libmupdf. */
void *ocr_init(fz_context *ctx, const char *lang, const char *datadir, const char *options);
void ocr_fin(fz_context *ctx, void *api);
void ocr_recognise(fz_context *ctx, void *api, fz_pixmap *pix,
		void (*callback)(fz_context *ctx, void *arg, int unicode, const char *font_name,
			const int *line_bbox, const int *word_bbox, const int *char_bbox, int pointsize),
		int (*progress)(fz_context *ctx, void *arg, int progress),
		void *arg);

/* API C de Tesseract (thirdparty/tesseract/include/tesseract/capi.h), compilée dans MuPDF. */
typedef struct TessBaseAPI TessBaseAPI;
typedef struct TessResultIterator TessResultIterator;
typedef struct TessPageIterator TessPageIterator;
enum { ANON_RIL_BLOCK = 0, ANON_RIL_PARA = 1, ANON_RIL_TEXTLINE = 2, ANON_RIL_WORD = 3 };
TessResultIterator *TessBaseAPIGetIterator(TessBaseAPI *handle);
void TessResultIteratorDelete(TessResultIterator *handle);
const TessPageIterator *TessResultIteratorGetPageIteratorConst(const TessResultIterator *handle);
int TessResultIteratorNext(TessResultIterator *handle, int level);
char *TessResultIteratorGetUTF8Text(const TessResultIterator *handle, int level);
float TessResultIteratorConfidence(const TessResultIterator *handle, int level);
int TessPageIteratorBoundingBox(const TessPageIterator *handle, int level, int *left, int *top, int *right, int *bottom);
int TessPageIteratorIsAtBeginningOf(const TessPageIterator *handle, int level);
void TessDeleteText(const char *text);

/* Rappel Rust : un mot reconnu. Ne doit ni paniquer ni dérouler. */
typedef void (*anon_word_cb)(void *user, const char *utf8, float confidence,
		int left, int top, int right, int bottom, int block, int para, int line);

static void
ignore_char(fz_context *ctx, void *arg, int unicode, const char *font_name,
	const int *line_bbox, const int *word_bbox, const int *char_bbox, int pointsize)
{
	(void)ctx; (void)arg; (void)unicode; (void)font_name;
	(void)line_bbox; (void)word_bbox; (void)char_bbox; (void)pointsize;
}

static void
emit_words(TessBaseAPI *api, anon_word_cb cb, void *user)
{
	TessResultIterator *it = TessBaseAPIGetIterator(api);
	const TessPageIterator *pi;
	int block = -1, para = -1, line = -1;

	if (it == NULL)
		return;
	pi = TessResultIteratorGetPageIteratorConst(it);
	do
	{
		char *text;
		int l = 0, t = 0, r = 0, b = 0;
		if (pi == NULL)
			break;
		if (TessPageIteratorIsAtBeginningOf(pi, ANON_RIL_BLOCK))
			block++;
		if (TessPageIteratorIsAtBeginningOf(pi, ANON_RIL_PARA))
			para++;
		if (TessPageIteratorIsAtBeginningOf(pi, ANON_RIL_TEXTLINE))
			line++;
		text = TessResultIteratorGetUTF8Text(it, ANON_RIL_WORD);
		if (text == NULL)
			continue;
		if (text[0] != 0 && TessPageIteratorBoundingBox(pi, ANON_RIL_WORD, &l, &t, &r, &b))
			cb(user, text, TessResultIteratorConfidence(it, ANON_RIL_WORD), l, t, r, b,
				block < 0 ? 0 : block, para < 0 ? 0 : para, line < 0 ? 0 : line);
		TessDeleteText(text);
	}
	while (TessResultIteratorNext(it, ANON_RIL_WORD));
	TessResultIteratorDelete(it);
}

/*
 * OCR d'une image 8 bits en niveaux de gris, `stride` == `w`, `w` multiple de 4.
 * Renvoie 0 si tout va bien, -1 en cas d'erreur (message générique de MuPDF
 * recopié dans `err`, jamais de contenu utilisateur), -2 si les arguments sont invalides.
 */
int
anon_ocr_gray(const unsigned char *samples, int w, int h, int dpi,
	const char *lang, const char *datadir, const char *options,
	anon_word_cb cb, void *user, char *err, size_t err_len)
{
	fz_context *ctx;
	fz_pixmap *pix = NULL;
	void *api = NULL;
	int rc = 0;

	if (err != NULL && err_len > 0)
		err[0] = 0;
	if (samples == NULL || cb == NULL || w <= 0 || h <= 0 || (w & 3) != 0 || dpi <= 0)
		return -2;

	ctx = fz_new_context(NULL, NULL, FZ_STORE_DEFAULT);
	if (ctx == NULL)
		return -1;
	fz_set_warning_callback(ctx, NULL, NULL);
	fz_set_error_callback(ctx, NULL, NULL);

	fz_var(pix);
	fz_var(api);
	fz_try(ctx)
	{
		unsigned char *dst;
		int y;
		pix = fz_new_pixmap(ctx, fz_device_gray(ctx), w, h, NULL, 0);
		dst = fz_pixmap_samples(ctx, pix);
		if (fz_pixmap_stride(ctx, pix) != w)
			fz_throw(ctx, FZ_ERROR_ARGUMENT, "unexpected stride");
		for (y = 0; y < h; y++)
			memcpy(dst + (size_t)y * (size_t)w, samples + (size_t)y * (size_t)w, (size_t)w);
		fz_set_pixmap_resolution(ctx, pix, dpi, dpi);
		api = ocr_init(ctx, lang, datadir, options);
		ocr_recognise(ctx, api, pix, ignore_char, NULL, NULL);
		emit_words((TessBaseAPI *)api, cb, user);
	}
	fz_always(ctx)
	{
		ocr_fin(ctx, api);
		fz_drop_pixmap(ctx, pix);
	}
	fz_catch(ctx)
	{
		rc = -1;
		if (err != NULL && err_len > 0)
		{
			const char *msg = fz_caught_message(ctx);
			if (msg != NULL)
			{
				strncpy(err, msg, err_len - 1);
				err[err_len - 1] = 0;
			}
		}
	}
	fz_drop_context(ctx);
	return rc;
}
