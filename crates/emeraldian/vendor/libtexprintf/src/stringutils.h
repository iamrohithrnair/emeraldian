#define PRINTABLE(C) iswprint((C))
extern int WCSPACES;
extern int FCSPACES;

int strspaces(char *str);
char * Unicode2Utf8(int U);
int NumByte(char *p); /* return the number of bytes of the multibyte char pointed to by p */
int UNumByte(int U); /* return the number of UTF-8 bytes for the unicode code point */
int IsInSet(char c, const char *set);
int Unicode(char *p, int *N);
int IsCombiningMark(int ch);
char * UnicodeMapper(char *in);

int MappableSuper(char *super);
char *MapSuperScript(char *super);
int MappableSub(char *sub);
char *MapSubScript(char *sub);
