#ifndef __TEXPRINTF_H__
#define __TEXPRINTF_H__

typedef struct {
	char *name;
	unsigned int unicode;
} Symbol;

extern Symbol * TEXPRINTF_SYMBOLS;
extern int TEXPRINTF_LW;								/* line width, if it is 0 the line width is infinite */
extern char * TEXPRINTF_FONT;							/* default font, one of:
														 *  "mathsfbfit"
														 *  "mathsfbf"
														 *  "mathfrak"
														 *  "mathbfit"
														 *  "mathsfit"
														 *  "mathcal"
														 *  "mathscr"
														 *  "mathbf"
														 *  "mathbb"
														 *  "mathsf"
														 *  "mathtt"
														 *  "mathnormal"
														 *  "text" */
extern int TEXPRINTF_FCW;								/* full width character width */
extern int TEXPRINTF_WCW;								/* wide character width */
extern int TEXPRINTF_ERR;
int texprintf(const char *format, ...);					/* prints to stdout */
char * stexprintf(const char *format, ...);				/* prints to string */
char * texstring(const char *str);                      /* stextprintf, but no printf formatting */
void texfree(void *ptr);								/* free memory from results */
int ftexprintf(FILE *f, const char *format, ...);		/* prints to file */
void texboxtree(const char *format, ...);				/* print the box-tree, for debugging purposes */
void texlistsymbols();									/* prints a list all known symbols */
char *texsymbols_str();									/* produces a ';' separated list symbols formatted as "<name>:<value>" */
void texerrors();										/* prints error messages of the encountered errors */
char *texerrors_str();									/* prints a ';' separated list of error messages */
int texerror_state();									/* returns 0 if no error is present */
void SetStyleASCII();									/* set style to ASCII, avoiding unicode where possible */
void SetStyleUNICODE();									/* standard style */
void ToggleMapSuperSub();								/* toggles wheteher super/sub scripts are mapped if possible (e.g. map 2 in a subscript to ₂)*/
void ToggleAvoidCombining();							/* combining diacritical marks do not always work well depending on your fonts I suppose */
void SetRootFont(const char *font);						/* function to set the default font to
														 *  "mathsfbfit"
														 *  "mathsfbf"
														 *  "mathfrak"
														 *  "mathbfit"
														 *  "mathsfit"
														 *  "mathcal"
														 *  "mathscr"
														 *  "mathbf"
														 *  "mathbb"
														 *  "mathsf"
														 *  "mathtt"
														 *  "mathnormal"
														 *  "text" 
														 */

#endif
