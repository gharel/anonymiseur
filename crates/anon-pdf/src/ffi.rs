//! Seul module `unsafe` du crate : appel de la cale C d'OCR (`csrc/ocr_shim.c`).
//!
//! La cale crée son propre `fz_context`, rattrape toute exception MuPDF sous
//! `fz_try` et ne rend la main qu'avec un code de retour : aucun `longjmp` ne
//! traverse de cadre Rust. Elle n'écrit aucun fichier.
#![allow(unsafe_code)]

use std::ffi::{CStr, CString, c_char, c_float, c_int, c_void};

/// Mot brut rendu par Tesseract, coordonnées en pixels de l'image fournie.
#[derive(Clone, PartialEq)]
pub(crate) struct RawWord {
    pub text: String,
    /// 0 à 100.
    pub confidence: f32,
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub block: i32,
    pub para: i32,
    pub line: i32,
}

impl std::fmt::Debug for RawWord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Jamais le texte reconnu dans les journaux.
        f.debug_struct("RawWord")
            .field("len", &self.text.len())
            .field("confidence", &self.confidence)
            .finish_non_exhaustive()
    }
}

/// Échec de la cale (sans détail : les messages MuPDF ne sont pas propagés).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShimError {
    InvalidArgument,
    Engine,
}

type WordCb = extern "C" fn(
    user: *mut c_void,
    utf8: *const c_char,
    confidence: c_float,
    left: c_int,
    top: c_int,
    right: c_int,
    bottom: c_int,
    block: c_int,
    para: c_int,
    line: c_int,
);

unsafe extern "C" {
    fn anon_ocr_gray(
        samples: *const u8,
        w: c_int,
        h: c_int,
        dpi: c_int,
        lang: *const c_char,
        datadir: *const c_char,
        options: *const c_char,
        cb: WordCb,
        user: *mut c_void,
        err: *mut c_char,
        err_len: usize,
    ) -> c_int;
}

#[allow(clippy::too_many_arguments)]
extern "C" fn collect_word(
    user: *mut c_void,
    utf8: *const c_char,
    confidence: c_float,
    left: c_int,
    top: c_int,
    right: c_int,
    bottom: c_int,
    block: c_int,
    para: c_int,
    line: c_int,
) {
    if user.is_null() || utf8.is_null() {
        return;
    }
    // SAFETY: `user` est le `&mut Vec<RawWord>` passé par `ocr_gray` ci-dessous,
    // vivant pendant tout l'appel C, et la cale ne l'utilise que depuis ce fil.
    let words = unsafe { &mut *user.cast::<Vec<RawWord>>() };
    // SAFETY: `utf8` est une chaîne terminée par NUL allouée par Tesseract,
    // valide jusqu'au retour de ce rappel (libérée ensuite par la cale).
    let text = unsafe { CStr::from_ptr(utf8) }
        .to_string_lossy()
        .into_owned();
    words.push(RawWord {
        text,
        confidence,
        left,
        top,
        right,
        bottom,
        block,
        para,
        line,
    });
}

/// OCR d'une image en niveaux de gris (`samples.len() == w * h`, `w % 4 == 0`).
/// L'appelant doit garantir qu'un seul OCR tourne à la fois dans le processus.
pub(crate) fn ocr_gray(
    samples: &[u8],
    width: u32,
    height: u32,
    dpi: u32,
    lang: &str,
    datadir: &str,
    options: &str,
) -> Result<Vec<RawWord>, ShimError> {
    let w = c_int::try_from(width).map_err(|_| ShimError::InvalidArgument)?;
    let h = c_int::try_from(height).map_err(|_| ShimError::InvalidArgument)?;
    let d = c_int::try_from(dpi).map_err(|_| ShimError::InvalidArgument)?;
    let expected = (width as usize)
        .checked_mul(height as usize)
        .ok_or(ShimError::InvalidArgument)?;
    if w <= 0 || h <= 0 || !width.is_multiple_of(4) || samples.len() != expected {
        return Err(ShimError::InvalidArgument);
    }
    let lang = CString::new(lang).map_err(|_| ShimError::InvalidArgument)?;
    let datadir = CString::new(datadir).map_err(|_| ShimError::InvalidArgument)?;
    let options = CString::new(options).map_err(|_| ShimError::InvalidArgument)?;
    let mut words: Vec<RawWord> = Vec::new();
    let mut err = [0 as c_char; 256];
    // SAFETY: tous les pointeurs sont valides pendant l'appel : `samples` couvre
    // `w * h` octets (vérifié ci-dessus), les chaînes sont terminées par NUL,
    // `words` est un `Vec<RawWord>` exclusif utilisé seulement par `collect_word`,
    // `err` fait `err.len()` octets. La cale rattrape toute exception MuPDF.
    let rc = unsafe {
        anon_ocr_gray(
            samples.as_ptr(),
            w,
            h,
            d,
            lang.as_ptr(),
            datadir.as_ptr(),
            options.as_ptr(),
            collect_word,
            (&raw mut words).cast::<c_void>(),
            err.as_mut_ptr(),
            err.len(),
        )
    };
    match rc {
        0 => Ok(words),
        -2 => Err(ShimError::InvalidArgument),
        _ => Err(ShimError::Engine),
    }
}
