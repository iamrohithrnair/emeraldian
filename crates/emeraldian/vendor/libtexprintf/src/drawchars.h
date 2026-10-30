#ifndef DRAWCHAR_H
#define DRAWCHAR_H
/* The style struct contains all "drawing characters"
 * This way one can change libtexprintf style for some things
 * Currently we have a "UNI" style, which will use unicode for 
 * everything, and an "ASC" style, which preferrs ASCII when possible.
 */
typedef struct Style {	
	int mapsupersub; 
	int avoidcombining; // option to avoid combining diacritical marks if possible
	int LBRACKCHAR[4]; // single, lower, extender, upper
	int RBRACKCHAR[4]; 
	int LSQUARECHAR[4];
	int RSQUARECHAR[4];
	int VBARCHAR[4];
	int DVBARCHAR[4];
	/* floor/ceil and up/down arrows */
	int LFLOORCHAR[4]; // single, lower, extender, upper
	int RFLOORCHAR[4];
	int LCEILCHAR[4];
	int RCEILCHAR[4];
	int UPARROWCHAR[4]; // single, lower, extender, extender
	int DUPARROWCHAR[4];
	int DOWNARROWCHAR[4]; // single, extender, extender, upper
	int DDOWNARROWCHAR[4];
	int UPDOWNARROWCHAR[4]; // single, lower, extender, upper
	int DUPDOWNARROWCHAR[4];
	/* symmetrical brackets (for use with SymBrac)*/
	int LCURLYCHAR[5]; // single, lower, center, extender, upper
	int RCURLYCHAR[5]; // single, lower, center, extender, upper
	/* angle brackets (for use with AngleBrac) */
	int ANGLECHAR[4]; // single left, single right, downward, upward
	int FSLASH;    // forward slash
	int BSLASH;	   // backward slash
	/* fraction, overline, underline */ 
	int FRACLINE;
	int OVERLINE;
	int UNDERLINE;
	/* array characters 
	*                     0      1      2      3      4      5      6      7      8      9      10
	*                     │      ─      ┌      ┬      ┐      ├      ┼      ┤      └      ┴      ┘       */
	int ARRAYCHAR[11];
	/* integrals and sqrt */
	int SQRTCHAR[5]; // downward diagonal, vertical, left top corner, horizontal, right top corner
	int INTCHAR[3]; // bottom, extender, top
	int IINTCHAR[4]; // single, double, tripple, dots
	int OINTCHAR[2]; // quasi-circle
	int OIINTCHAR[4]; // single, double, tripple, dots	
	/* row mayor arrays of characters to draw symbols*/
	int SUMCHAR[10]; // width, height, start lower left, end upper right
	int PRODCHAR[8]; // width, height, start lower left, end upper right
	int prime[3]; // prime width, height, start lower left, end upper right
	int dprime[4]; 
	int tprime[5]; 	
	int qprime[6];
} Style;

#endif //#ifndef DRAWCHAR_H
