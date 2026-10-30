# libtexprintf, vendored

This directory is [libtexprintf](https://github.com/bartp5/libtexprintf), the C
library behind the `utftex` command line tool. It typesets TeX into Unicode
boxes and is what the reading pane uses for maths.

- Upstream: <https://github.com/bartp5/libtexprintf>
- Tag: `v.1.31` (the tag's spelling)
- Commit: `18977837b20649d56a651eb6bf846f1c914db77a`
- Licence: GPL-3.0-only. The full text is in `COPYING`; the authors are in
  `AUTHORS` and the changes between versions in `ChangeLog`.
- Copyright: Bart Pasteuning and the libtexprintf contributors.

## What is here

The sources `src/Makefile.am` builds into the library, and the headers they
include:

    src/boxes.c      src/boxes.h        src/drawbox.c    src/drawbox.h
    src/drawchars.h  src/texprintf.c    src/texprintf.h  src/parser.c
    src/parser.h     src/lexer.c        src/lexer.h      src/error.c
    src/error.h      src/stringutils.c  src/stringutils.h
    src/parsedef.h   src/mapunicode.h   src/unicodeblocks.h

Also kept for provenance: `AUTHORS`, `ChangeLog`, `COPYING`, `NEWS`,
`README.md`, and the two generator scripts `src/gen_errorflags.sh` and
`src/gentexprintfsymbols.sh`.

Left out, because they are the command line front end, its tests or its
autotools build and are not needed to link the library: `src/main.c`,
`src/testfonts.c`, `src/testfonts.h`, `src/utf2unicode.c`, the `Makefile.am`s,
`configure.ac`, `autogen.sh`, `INSTALL`, and the test scripts and fixtures.

## Changes from upstream

None. The sources are byte-for-byte the files at the commit above.

The two headers upstream generates while building, `errorflags.h` and
`errormessages.h`, are not checked in here either: `crates/emeraldian/build.rs`
generates them into Cargo's `OUT_DIR` by scanning the same
`// ERRORFLAG NAME "message"` comments in the same file order that
`src/gen_errorflags.sh` uses. The library is compiled by `cc` from that build
script; nothing is fetched at build or run time.

## Known upstream bugs

- `\over` or `\choose` as the **first** token of an input segfaults version
  1.31: the lexer slices the numerator from one byte before the string. The
  same commands after any other token (`a \over b`), and longer commands that
  start with one (`\overline`), are safe. `crates/emeraldian/src/ui/utftex.rs`
  refuses the crashing spelling before calling the library and lets the
  built-in renderer set that formula; the vendored source is not patched.

## Windows

The sources compile with MSVC as well as with GCC and Clang; each translation
unit was checked with MSVC 19.43 (VS 17.13, `/W3`) and needed no changes. The
only platform-specific detail is in `build.rs`: `lexer.c` calls `roundf`, so
the maths library is linked on every target except MSVC, where the CRT already
has it.

## Licence

libtexprintf is GPL-3.0-only. Emeraldian is GPL-3.0-or-later; the two are
compatible, and the combined work is distributed under GPL-3.0. See the
"Credits" and "License" sections of the repository's `README.md`.
