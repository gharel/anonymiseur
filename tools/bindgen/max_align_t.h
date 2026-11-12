/* Le libclang du paquet pip n'embarque pas les en-tetes de ressources de clang
   (__stddef_max_align_t.h) ; les en-tetes MSVC ne definissent pas max_align_t en C.
   Meme definition que clang pour la cible MSVC. */
#ifndef ANON_MAX_ALIGN_T
#define ANON_MAX_ALIGN_T
typedef double max_align_t;
#endif
