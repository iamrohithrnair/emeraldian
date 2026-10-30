#!/bin/bash
# generate a list of symbols to export in libtexprintf
# parses texprintf.h to collect all relavant symbols

# get global variables
grep 'extern' texprintf.h |grep -o -E '[^ ;]+;'|grep -o -E '[^;]+' >texprintfsymbols
grep -o -E '[^ \*\(]+\(' texprintf.h |grep -o -E '^[^\(]+' >>texprintfsymbols
