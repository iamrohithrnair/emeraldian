
typedef struct {
	char           *name;
	PRSDEF          P; /* the PRSDEF enummerator type is declared in lexer.h */
	int 			Nargs;
	int 			Nopt;
} KEYWORD;
/* base context keywords */
const KEYWORD   Keys[] = {
	{"\\raisebox",  PD_RAISEBOX   	, 2, 0},
	{"\\int",  		PD_INT     	, 0, 1},
	{"\\iint",  	PD_IINT     	, 0, 1},
	{"\\iiint",  	PD_IIINT     	, 0, 1},
	{"\\iiiint",  	PD_IIIINT     	, 0, 1},
	{"\\idotsint",  PD_IDOTSINT   	, 0, 1},
	{"\\oint",  	PD_OINT     	, 0, 1},
	{"\\oiint",  	PD_OIINT     	, 0, 1},
	{"\\oiiint",  	PD_OIIINT     	, 0, 1},
	{"\\oiiiint",  	PD_OIIIINT    	, 0, 1},
	{"\\oidotsint", PD_OIDOTSINT  	, 0, 1},
	{"\\mathsfbfit",PD_MATHSFBFIT	, 1, 0},
	{"\\mathsfbf",  PD_MATHSFBF	, 1, 0},
	{"\\mathfrak",  PD_MATHFRAK	, 1, 0},
	{"\\mathbfit",  PD_MATHBFIT	, 1, 0},
	{"\\mathsfit",  PD_MATHSFIT	, 1, 0},
	{"\\mathcal",  	PD_MATHCAL		, 1, 0},
	{"\\mathscr",  	PD_MATHSCR		, 1, 0},
	{"\\mathbf",  	PD_MATHBF		, 1, 0},
	{"\\mathbb",  	PD_MATHBB		, 1, 0},
	{"\\mathsf",  	PD_MATHSF		, 1, 0},
	{"\\mathtt",  	PD_MATHTT		, 1, 0},
	{"\\mathnormal",PD_MATHNORMAL	, 1, 0},
	{"\\mbox",		PD_TEXT		, 1, 0},
	{"\\text",		PD_TEXT		, 1, 0},
	{"\\textrm",	PD_TEXT		, 1, 0},
	{"\\mathrm",	PD_TEXT		, 1, 0},
	{"\\operatorname",	PD_TEXT		, 1, 0},
	{"\\textbf",  	PD_MATHBF		, 1, 0}, // closest, or?
	{"\\backslash", PD_BACKSLASH	, 0, 0},
	{"\\begin", 	PD_BEGIN   	, 1, 0},
	{"\\frac", 		PD_FRAC    	, 2, 0},
	{"\\tfrac", 	PD_FRAC    	, 2, 0},
	{"\\dfrac", 	PD_FRAC    	, 2, 0},
	{"\\stackrel",	PD_STACK   	, 2, 1},
	{"\\stackbin",	PD_STACK   	, 2, 1},
	{"\\binom", 	PD_BINOM   	, 2, 0},
	{"\\sqrt", 		PD_SQRT    	, 1, 1},
	{"\\pmod", 		PD_PMOD    	, 1, 0},
	{"\\bmod", 		PD_BMOD    	, 1, 0},
	{"\\mod", 		PD_MOD    	, 1, 0},
	{"\\pod", 		PD_POD    	, 1, 0},
	{"\\left", 		PD_LEFTRIGHT   , 0, 0},
	{"\\big", 		PD_BIG1  , 0, 0},
	{"\\Big", 		PD_BIG2  , 0, 0},
	{"\\bigg", 		PD_BIG3  , 0, 0},
	{"\\Bigg", 		PD_BIG4  , 0, 0},
	{"\\bigl", 		PD_BIG1  , 0, 0}, // left and right versions are simply aliases of the same (in tex it seems only spacing varies a little)
	{"\\Bigl", 		PD_BIG2  , 0, 0},
	{"\\biggl", 	PD_BIG3  , 0, 0},
	{"\\Biggl", 	PD_BIG4  , 0, 0},
	{"\\bigr", 		PD_BIG1  , 0, 0},
	{"\\Bigr", 		PD_BIG2  , 0, 0},
	{"\\biggr", 	PD_BIG3  , 0, 0},
	{"\\Biggr", 	PD_BIG4  , 0, 0},
	{"\\prod",  	PD_PROD    	, 0, 0},
	{"\\sum",  		PD_SUM     	, 0, 0},
	{"\\box",  		PD_BOX     	, 2, 0},
	{"\\kern", 		PD_KERN    	, 1, 0},
	{"\\phantom",  	PD_PHANTOM     	, 1, 0},
	{"\\vphantom",  PD_VPHANTOM   	, 1, 0},
	{"\\hphantom",  PD_HPHANTOM    	, 1, 0},
	{"\\limits",  	PD_LIMITS      , 1, 0},
	{"\\nolimits",  PD_NOLIMITS    , 1, 0},
	{"\\lim",  		PD_LIM   		, 0, 0},
	{"\\over", 		PD_OVER   		, 0, 0},
	{"\\choose", 	PD_CHOOSE  		, 0, 0},
	{"\\exp",  		PD_FUNCTION   	, 0, 0},
	{"\\ln",  		PD_FUNCTION   	, 0, 0},
	{"\\log",  		PD_FUNCTION   	, 0, 0},
	{"\\min",  		PD_FUNCTION   	, 0, 0},
	{"\\max",  		PD_FUNCTION   	, 0, 0},
	{"\\sin",  		PD_FUNCTION   	, 0, 0},
	{"\\cos",  		PD_FUNCTION   	, 0, 0},
	{"\\tan",  		PD_FUNCTION   	, 0, 0},
	{"\\cot",  		PD_FUNCTION   	, 0, 0},
	{"\\arcsin",	PD_FUNCTION   	, 0, 0},
	{"\\arccos",	PD_FUNCTION   	, 0, 0},
	{"\\arctan",	PD_FUNCTION   	, 0, 0},
	{"\\arccot",	PD_FUNCTION   	, 0, 0},
	{"\\sinh", 		PD_FUNCTION   	, 0, 0},
	{"\\cosh", 		PD_FUNCTION   	, 0, 0},
	{"\\tanh", 		PD_FUNCTION   	, 0, 0},
	{"\\coth", 		PD_FUNCTION   	, 0, 0},
	{"\\sec",  		PD_FUNCTION   	, 0, 0},
	{"\\csc",  		PD_FUNCTION   	, 0, 0},
	{"\\deg",  		PD_FUNCTION   	, 0, 0},
	{"\\det",  		PD_FUNCTION   	, 0, 0},
	{"\\arg",  		PD_FUNCTION   	, 0, 0},
	{"\\it",		PD_SETITALIC	, 0, 0},
	{"\\bf",  		PD_SETBOLD		, 0, 0},
	{"\\rm",  		PD_SETROMAN	, 0, 0},
	{"\\mathrm",	PD_TEXT		, 1, 0},
	{"\\bm",		PD_BOLD		, 1, 0},
	{"\\boldsymbol",PD_BOLD		, 1, 0},
	{"\\pmb",		PD_BOLD		, 1, 0},
	{"^",      		PD_SUPER   	, 2, 0},
	{"_",      		PD_SUB     	, 2, 0},
	{"\\\\",      	PD_ENDLINE    	, 0, 0},
	{"\\ ",      	PD_SPACE    	, 0, 0}, /* in monospace fonts small spacing is a minimal one space wide */
	{"\\,",      	PD_NSPACE    	, 0, 0}, /* dummy for no space */
	{"\\-",      	PD_NSPACE    	, 0, 0}, /* what is \- in latex math mode. with me it does not through an arror and I see no change */
	{"\\;",      	PD_SPACE    	, 0, 0},
	{"\\:",      	PD_SPACE    	, 0, 0},
	{"\\quad",      PD_DSPACE    	, 0, 0},
	{"\\qquad",     PD_TSPACE    	, 0, 0},
	// combining stuff
	{"\\overline",  PD_COMB_OVERLINE    , 1, 0},
	{"\\underline", PD_COMB_UNDERLINE   , 1, 0},
	{"\\bar",  		PD_COMB_OVERLINE    , 1, 0},
	{"\\grave",     PD_COMB_GRAVE        , 1, 0},
	{"\\`",         PD_COMB_GRAVE      	, 1, 0},
	{"\\acute",     PD_COMB_ACUTE        , 1, 0},
	{"\\'",         PD_COMB_ACUTE       	, 1, 0},
	{"\\hat",       PD_COMB_HAT          , 1, 0},
	{"\\^",         PD_COMB_HAT          , 1, 0},
	{"\\widehat",   PD_COMB_HAT          , 1, 0}, // we cannot scale so it is just the same as hat
	{"\\tilde",     PD_COMB_TILDE        , 1, 0},
	{"\\widetilde", PD_COMB_TILDE        , 1, 0}, //cannot scale
	{"\\~",         PD_COMB_TILDE        , 1, 0},
	{"\\breve",     PD_COMB_BREVE        , 1, 0},
	{"\\dot",       PD_COMB_DOT          , 1, 0},	
	{"\\ddot",      PD_COMB_DIAERESIS    , 1, 0}, 
	{"\\\"",        PD_COMB_DIAERESIS    , 1, 0}, 
	{"\\mathring",  PD_COMB_MRING        , 1, 0}, 
	{"\\H",         PD_COMB_DACUTE       , 1, 0},
	{"\\check",     PD_COMB_CARON        , 1, 0},
	{"\\c", 		PD_COMB_CEDILLA      , 1, 0},
	{"\\k", 		PD_COMB_OGONEK       , 1, 0},
	{"\\utilde",    PD_COMB_UTILDE       , 1, 0},
	{"\\l",         PD_COMB_SSOLIDUS     , 1, 0}, 
	{"\\not",       PD_COMB_LSOLIDUS     , 1, 0},
	// auto generated entries	
	{"\\lvec" , PD_COMB_LVEC ,1,0},
	{"\\Lvec" , PD_COMB_LLVEC ,1,0},
	{"\\overleftarrow" , PD_COMB_LLVEC ,1,0},
	{"\\vec" , PD_COMB_VEC ,1,0},
	{"\\overrightarrow" , PD_COMB_VEC ,1,0},
	{"\\overleftarrow" , PD_COMB_LVEC ,1,0},
	{"\\dddot" , PD_COMB_DDDOT ,1,0},
	{"\\ddddot" , PD_COMB_DDDDOT ,1,0},
	{"\\overleftrightarrow" , PD_COMB_OVERLEFTRIGHTARROW ,1,0},
	{"\\ocirc" , PD_COMB_MRING ,1,0},
	{"\\ovhook" , PD_COMB_OVHOOK ,1,0},
	{"\\oturnedcomma" , PD_COMB_OTURNEDCOMMA ,1,0},
	{"\\ocommatopright" , PD_COMB_OCOMMATOPRIGHT ,1,0},
	{"\\droang" , PD_COMB_DROANG ,1,0},
	{"\\leftharpoonaccent" , PD_COMB_LVEC ,1,0},
	{"\\rightharpoonaccent" , PD_COMB_RIGHTHARPOONACCENT ,1,0},
	{"\\widebridgeabove" , PD_COMB_WIDEBRIDGEABOVE ,1,0},
	{"\\asteraccent" , PD_COMB_ASTERACCENT ,1,0},
	{"\\candra" , PD_COMB_CANDRA ,1,0},
	{"\\wideutilde" , PD_COMB_WIDEUTILDE ,1,0},
	{"\\threeunderdot" , PD_COMB_THREEUNDERDOT ,1,0},
	{"\\underleftarrow" , PD_COMB_UNDERLEFTARROW ,1,0},
	{"\\underrightarrow" , PD_COMB_UNDERRIGHTARROW ,1,0},
	{"\\underbar" , PD_COMB_UNDERBAR ,1,0},
	{"\\underleftrightarrow" , PD_COMB_UNDERLEFTRIGHTARROW ,1,0},
	{"\\underrightharpoondown" , PD_COMB_UNDERRIGHTHARPOONDOWN ,1,0},
	{"\\underleftharpoondown" , PD_COMB_UNDERLEFTHARPOONDOWN ,1,0},
	{"\\palh" , PD_COMB_PALH ,1,0},
	{"\\rh" , PD_COMB_RH ,1,0},
	{"\\sbbrg" , PD_COMB_SBBRG ,1,0},
	{"\\sout" , PD_COMB_SOUT ,1,0},
	{"\\strike" , PD_COMB_STRIKE ,1,0},
	{"\\annuity" , PD_COMB_ANNUITY ,1,0},
	{"\\enclosecircle" , PD_COMB_ENCLOSECIRCLE ,1,0},
	{"\\enclosesquare" , PD_COMB_ENCLOSESQUARE ,1,0},
	{"\\enclosediamond" , PD_COMB_ENCLOSEDIAMOND ,1,0},
	{"\\enclosetriangle" , PD_COMB_ENCLOSETRIANGLE ,1,0},
	{"\\vertoverlay" , PD_COMB_VERTOVERLAY ,1,0},
	/* commands to ignore */
	{"\\mathop",      	PD_NSPACE    	, 0, 0}, /* with libtexprintf the \limits command always works, so we ignore this */
	{"\\mathord",      	PD_NSPACE    	, 0, 0}, /* with libtexprintf everything is ordinary! (not sure what mathord actually does...) */
	{"\\kern",      	PD_NSPACE    	, 0, 0}, /* this I suppose I should implement at least for positive space */
	{"\\displaystyle", 	PD_NSPACE    	, 0, 0}, /* cannot change font size */
	// end
	{NULL, 			PD_NONE 		, 0, 0},
};

/* context dependent keywords */
const KEYWORD   Envs[] = {
	{"aligned",  	PD_ALIGN     	, 0, 0},
	{"align*",  	PD_ALIGN     	, 0, 0},
	{"align",  		PD_ALIGN     	, 0, 0},
	{"array",  		PD_ARRAY     	, 1, 1},
	{"cases*",  	PD_CASES    	, 0, 1},
	{"cases",  		PD_CASES    	, 0, 1},
	{"dcases",  	PD_CASES    	, 0, 1}, //mathtools display style version of cases
	{"pmatrix*",  	PD_PMATRIX    	, 0, 1},
	{"pmatrix",  	PD_PMATRIX    	, 0, 1},
	{"bmatrix*",  	PD_BMATRIX    	, 0, 1},
	{"bmatrix",  	PD_BMATRIX    	, 0, 1},
	{"Bmatrix*",  	PD_BBMATRIX    , 0, 1},
	{"Bmatrix",  	PD_BBMATRIX    , 0, 1},
	{"vmatrix*",  	PD_VMATRIX    	, 0, 1},
	{"vmatrix",  	PD_VMATRIX    	, 0, 1},
	{"Vmatrix*",  	PD_VVMATRIX    	, 0, 1},
	{"Vmatrix",  	PD_VVMATRIX    	, 0, 1},
	{"matrix*",  	PD_MATRIX    	, 0, 1},
	{"matrix",  	PD_MATRIX    	, 0, 1},
	{"\\begin", 	PD_BEGIN   	, 0, 0},
	{"\\end", 		PD_END   		, 0, 0},
	{"\\hline",  	PD_HLINE     	, 0, 0},
	{NULL, 			PD_NONE		, 0, 0},
};


typedef struct {
	char *name;
	SCALABLE_DELIMITER D;
} DELIMITER;


const DELIMITER   DelTable[] = {
	{"(", DEL_L},
	{")", DEL_R},
	{"[", DEL_LSQ},
	{"]", DEL_RSQ},
	{"{", DEL_LCURL}, // these work somewhat without escaping (unlike in tex) 
	{"}", DEL_RCURL},
	{"\\{", DEL_LCURL}, // add the escaped version 
	{"\\}", DEL_RCURL},
	{"<", DEL_LANGLE},
	{">", DEL_RANGLE},
	{"⟨", DEL_LANGLE}, // aliasses for the unicode ones
	{"⟩", DEL_RANGLE},
	{"⌈", DEL_LCEIL}, /* note some strings in this table are UTF-8 encoded */
	{"⌉", DEL_RCEIL},
	{"⌊", DEL_LFLOOR},
	{"⌋", DEL_RFLOOR},	
	{"|", DEL_VBAR},
	{"‖", DEL_DVBAR},
	{".", DEL_DOT},
	{"↑", DEL_UPARROW},
	{"↓", DEL_DOWNARROW},
	{"↕", DEL_UPDOWNARROW},
	{"⇑", DEL_DUPARROW},
	{"⇓", DEL_DDOWNARROW},
	{"⇕", DEL_DUPDOWNARROW},
	{"/", DEL_SLASH},
	{"\\backslash", DEL_BACKSLASH}, /* backslash is not substituted by the preprocessor as would collide with the use as an escape character
									 * thus the whole string \backslash remains and needs to be matched */
	{NULL, DEL_NONE}
};

typedef struct {
	PRSDEF          P;
	unsigned int comb;		// combining diacritical mark
	unsigned int alt;		// alternative character to put above or below (0 if no suitable character exists)
	unsigned int altascii;	// ascii alternative to put above/below
} CombiningMarks;

const CombiningMarks  Combining[] = {
	{PD_COMB_GRAVE,     0x00300, 0x0060, 0x0060}, // alt `
	{PD_COMB_ACUTE,     0x00301, 0x00B4, 0x00B4}, // alt ´
	{PD_COMB_HAT,       0x00302, 0x005E, 0x005E}, // alt ^
	{PD_COMB_TILDE,     0x00303, 0x007E, 0x007E}, // alt ~
	{PD_COMB_BREVE,     0x00306, 0x25E1, 0}, // alt unicode lower half circle
	{PD_COMB_OVERLINE,  0x00305, 0x02581, 0x005F}, // alt 
	{PD_COMB_UNDERLINE, 0x00332, 0x02500, 0x002D}, // alt 
	{PD_COMB_UNDERBAR , 0x00332,0x02500,0x002D}, // underline==underbar
	{PD_COMB_DOT,       0x00307, 0x002E, 0x002E}, // alt 
	{PD_COMB_DIAERESIS, 0x00308, 0, 0},
	{PD_COMB_MRING,     0x0030A, 0x02218, 0},  
	{PD_COMB_DACUTE,    0x0030B, 0, 0},
	{PD_COMB_CARON,     0x0030C, 0, 0}, // v or check mark 0x2713 ?
	{PD_COMB_CEDILLA,   0x00327, 0, 0},
	{PD_COMB_OGONEK,    0x00328, 0, 0},
	{PD_COMB_UTILDE,    0x00330, 0x007E, 0x007E},
	{PD_COMB_SSOLIDUS,  0x00337, 0, 0}, 
	{PD_COMB_LSOLIDUS,  0x00338, 0, 0}, 
	
	// auto generated
	{PD_COMB_LVEC , 0x020D0,0x021BC,0},
	{PD_COMB_LLVEC , 0x020D6,0x02190,0},
	{PD_COMB_VEC , 0x020D7,0x02192,0},
	{PD_COMB_DDDOT , 0x020DB,0x02026,0},
	{PD_COMB_DDDDOT , 0x020DC,0,0},
	{PD_COMB_OVERLEFTRIGHTARROW , 0x020E1,0x02194,0},
	{PD_COMB_OVHOOK , 0x00309,0x00242,0},
	{PD_COMB_OTURNEDCOMMA , 0x00312,0x02E32,0},
	{PD_COMB_OCOMMATOPRIGHT , 0x00315,0x2C,0},
	{PD_COMB_DROANG , 0x0031A,0x2510,0},
	{PD_COMB_RIGHTHARPOONACCENT , 0x020D1,0x21C0,0},
	{PD_COMB_WIDEBRIDGEABOVE , 0x020E9,0x23B4,0},
	{PD_COMB_ASTERACCENT , 0x20F0,0x2A,0},
	{PD_COMB_CANDRA , 0x00310,0x1D111,0}, // abuse a Musical symbol fermata below
	// accents below the character
	{PD_COMB_WIDEUTILDE , 0x00330,0x007E, 0x007E},
	{PD_COMB_THREEUNDERDOT , 0x020E8,0x02026,0},
	{PD_COMB_UNDERLEFTARROW , 0x20EE,0x021BC,0},
	{PD_COMB_UNDERRIGHTARROW , 0x20EF,0x02192,0},
	{PD_COMB_UNDERLEFTRIGHTARROW , 0x0034D,0x02194,0},
	{PD_COMB_UNDERRIGHTHARPOONDOWN , 0x20EC,0x021C1,0},
	{PD_COMB_UNDERLEFTHARPOONDOWN , 0x20ED,0x021BD,0},
	{PD_COMB_PALH , 0x00321,0x025DE,0},
	{PD_COMB_RH , 0x00322,0x025DF,0},
	{PD_COMB_SBBRG , 0x0032A,0x23B4,0},
	// accents right through the middle
	// no alternatives possible
	{PD_COMB_SOUT , 0x00336,0,0},
	{PD_COMB_STRIKE , 0x00336,0,0},
	{PD_COMB_ANNUITY , 0x020E7,0,0},
	{PD_COMB_ENCLOSECIRCLE , 0x020DD,0,0},
	{PD_COMB_ENCLOSESQUARE , 0x020DE,0,0},
	{PD_COMB_ENCLOSEDIAMOND , 0x020DF,0,0},
	{PD_COMB_ENCLOSETRIANGLE , 0x020E4,0,0},
	{PD_COMB_VERTOVERLAY , 0x020D2,0,0}, 
	
	{PD_NONE,    0, 0, 0},
};

typedef struct {
	char *name;
	unsigned int unicode;
} Symbol;

/* note symbols take presedence over keywords */

const Symbol  Symbols[] = {
/* basic latin */
	{"\\_",                    0x0005F},
//	{"\\{",                    0x0007B},
	{"\\}",                    0x0007D},
	{"\\lbrac",                0x0005B},
//	{"\\backslash",            0x0005C}, /* this cannot work as this table is for the pre-processor, it is defined as an argument-less command */
	{"\\rbrac",                0x0005D},
	{"\\sphat",                0x0005E},
	{"\\sptilde",              0x0007E},
	{"\\vert",                 0x0007C},
	{"\\mid",                  0x0007C},
	{"\\lvert",                0x0007C},
	{"\\rvert",                0x0007C},
	{"\\divides",              0x0007C},
/* latin supplement*/
	{"\\cent",                 0x000A2},
	{"\\pounds",               0x000A3},
	{"\\yen",                  0x000A5},
	{"\\neg",                  0x000AC},
	{"\\circledR",             0x000AE},
	{"\\pm",                   0x000B1},
	{"\\Micro",                0x000B5},
	{"\\times",                0x000D7},
	{"\\eth",                  0x000F0},
	{"\\div",                  0x000F7},
/* latin extended A & B*/
	{"\\imath",                0x00131},
	{"\\jmath",                0x00237},
/* greek symbols */
	/* upper case */
	{"\\Gamma",                0x00393},
	{"\\Delta",                0x00394},
	{"\\Theta",                0x00398},
	{"\\Lambda",               0x0039B},	
	{"\\Xi",                   0x0039E},
	{"\\Pi",                   0x003A0},
	{"\\Sigma",                0x003A3},
	{"\\Upsilon",              0x003A5},
	{"\\Phi",                  0x003A6},
	{"\\Psi",                  0x003A8},
	{"\\Omega",                0x003A9},
	
	/* lower case */
	{"\\alpha",                0x003B1},
	{"\\beta",                 0x003B2},
	{"\\gamma",                0x003B3},
	{"\\delta",                0x003B4},
	{"\\varepsilon",           0x003B5},
	{"\\epsilon",              0x003F5},
	{"\\backepsilon",          0x003F6},
	{"\\zeta",                 0x003B6},
	{"\\eta",                  0x003B7},
	{"\\theta",                0x003B8},
	{"\\iota",                 0x003B9},
	{"\\kappa",                0x003BA},
	{"\\lambda",               0x003BB},
	{"\\mu",                   0x003BC},
	{"\\nu",                   0x003BD},
	{"\\Nu",                   0x0039D},
	{"\\xi",                   0x003BE},
	{"\\pi",                   0x003C0},
	{"\\rho",                  0x003C1},
	{"\\varrho",               0x003F1},
	{"\\sigma",                0x003C3},
	{"\\varsigma",             0x003C2},
	{"\\tau",                  0x003C4},
	{"\\upsilon",              0x003C5},
	{"\\varphi",               0x1D711},
	{"\\chi",                  0x003C7},
	{"\\psi",                  0x003C8},
	{"\\omega",                0x003C9},
	{"\\varbeta",              0x003D0},
	{"\\vartheta",             0x003D1},
	{"\\phi",                  0x003D5},
	{"\\varpi",                0x003D6},
	{"\\Qoppa",                0x003D8},
	{"\\qoppa",                0x003D9},
	{"\\Stigma",               0x003DA},
	{"\\stigma",               0x003DB},
	{"\\Digamma",              0x003DC},
	{"\\digamma",              0x003DD},
	{"\\Koppa",                0x003DE},
	{"\\koppa",                0x003DF},
	{"\\Sampi",                0x003E0},
	{"\\sampi",                0x003E1},
/* general punktuation */
	{"\\|",                    0x02016},
	{"\\dagger",               0x02020},
	{"\\ddagger",              0x02021},
	{"\\ldots",                0x02026},
	{"\\prime",                0x02032},
	{"\\pprime",               0x02033},
	{"\\second",               0x02033},
	{"\\third",                0x02034},
	{"\\ppprime",               0x02034},
	{"\\backprime",            0x02035},
	{"\\backpprime",           0x02036},
	{"\\backppprime",          0x02037},
	{"\\cat",                  0x02040},
	{"\\fourth",               0x02057},
	{"\\pppprime",             0x02057}, 
/* Letterlike Symbols */
/*some symbols require a latex command structure, those I leave out here */
	{"\\Euler",   			   0x02107}, 
	{"\\hslash",   			   0x0210F}, 
	{"\\Im",  	 			   0x02111}, 
	{"\\ell",  	 			   0x02113}, 
	{"\\wp",  	 			   0x02118}, 
	{"\\Re",  	 			   0x0211C}, 
	{"\\tcohm",  	 		   0x02126}, 
	{"\\mho",	  	 		   0x02127}, 
	{"\\Angstroem",	 		   0x0212B}, 
	{"\\Finv",	  	 		   0x02132},  
	{"\\aleph",  	 		   0x02135},  
	{"\\beth",  	 		   0x02136},  
	{"\\gimel",  	 		   0x02137},  
	{"\\daleth",  	 		   0x02138},  
	{"\\Yup",  	 			   0x02144},  
	{"\\CapitalDifferentialD", 0x02145}, 
	{"\\DifferentialD",		   0x02146},  
	{"\\ExponetialE",		   0x02147},  
	{"\\ComplexI",			   0x02148},  
	{"\\ComplexJ",			   0x02149}, 
	{"\\invamp",			   0x0214B}, 
	
/* arrows */
	{"\\leftarrow",            0x02190},
	{"\\uparrow",              0x02191},
	{"\\to",           		   0x02192}, 
	{"\\rightarrow",           0x02192},
	{"\\downarrow",            0x02193},
	{"\\leftrightarrow",       0x02194},
	{"\\updownarrow",          0x02195},
	{"\\nwarrow",              0x02196},
	{"\\nearrow",              0x02197},
	{"\\searrow",              0x02198},
	{"\\swarrow",              0x02199},
	{"\\nleftarrow",           0x0219A},
	{"\\nrightarrow",          0x0219B},
	{"\\twoheadleftarrow",     0x0219E},
	{"\\twoheadrightarrow",    0x021A0},
	{"\\leftarrowtail",        0x021A2},
	{"\\rightarrowtail",       0x021A3},
	{"\\mapsfrom",             0x021A4},
	{"\\MapsUp",               0x021A5},
	{"\\mapsto",               0x021A6},
	{"\\MapsDown",             0x021A7},
	{"\\hookleftarrow", 0x021A9},
	{"\\hookrightarrow", 0x021AA},
	{"\\looparrowleft", 0x021AB},
	{"\\looparrowright", 0x021AC},
	{"\\leftrightsquigarrow", 0x021AD},
	{"\\nleftrightarrow", 0x021AE},
	{"\\lightning", 0x021AF},
	{"\\Lsh", 0x021B0},
	{"\\Rsh", 0x021B1},
	{"\\dlsh", 0x021B2},
	{"\\drsh", 0x021B3},
	{"\\curvearrowleft", 0x021B6},
	{"\\curvearrowright", 0x021B7},
	{"\\circlearrowleft", 0x021BA},
	{"\\circlearrowright", 0x021BB},
	{"\\leftharpoonup", 0x021BC},
	{"\\leftharpoondown", 0x021BD},
	{"\\upharpoonright", 0x021BE},
	{"\\upharpoonleft", 0x021BF},
	{"\\rightharpoonup", 0x021C0},
	{"\\rightharpoondown", 0x021C1},
	{"\\downharpoonright", 0x021C2},
	{"\\downharpoonleft", 0x021C3},
	{"\\rightleftarrows", 0x021C4},
	{"\\updownarrows", 0x021C5},
	{"\\leftrightarrows", 0x021C6},
	{"\\leftleftarrows", 0x021C7},
	{"\\upuparrows", 0x021C8},
	{"\\rightrightarrows", 0x021C9},
	{"\\downdownarrows", 0x021CA},
	{"\\leftrightharpoons", 0x021CB},
	{"\\rightleftharpoons", 0x021CC},
	{"\\nLeftarrow", 0x021CD},
	{"\\nLeftrightarrow", 0x021CE},
	{"\\nRightarrow", 0x021CF},
	{"\\Leftarrow", 0x021D0},
	{"\\Uparrow", 0x021D1},
	{"\\Rightarrow", 0x021D2},
	{"\\Downarrow", 0x021D3},
	{"\\Leftrightarrow", 0x021D4},
	{"\\Updownarrow", 0x021D5},
	{"\\Nwarrow", 0x021D6},
	{"\\Nearrow", 0x021D7},
	{"\\Searrow", 0x021D8},
	{"\\Swarrow", 0x021D9},
	{"\\Lleftarrow", 0x021DA},
	{"\\Rrightarrow", 0x021DB},
	{"\\leftsquigarrow", 0x021DC},
	{"\\rightsquigarrow", 0x021DD},
	{"\\dashleftarrow", 0x021E0},
	{"\\dashrightarrow", 0x021E2},
	{"\\LeftArrowBar", 0x021E4},
	{"\\RightArrowBar", 0x021E5},
	{"\\downuparrows", 0x021F5},
	{"\\pfun", 0x021F8},
	{"\\ffun", 0x021FB},
	{"\\leftarrowtriangle", 0x021FD},
	{"\\rightarrowtriangle", 0x021FE},
	{"\\leftrightarrowtriangle", 0x021FF},	/* to be continued */
	
	/* mathematical operators */
	{"\\forall", 0x02200},
	{"\\complement", 0x02201},
	{"\\partialup", 0x02202},
	{"\\exists", 0x02203},
	{"\\nexists", 0x02204},
	{"\\varnothing", 0x02205},
	{"\\nabla", 0x02207},
	{"\\in", 0x02208},
	{"\\notin", 0x02209},
	{"\\ni", 0x0220B},
	{"\\nni", 0x0220C},
	{"\\utfprod", 0x0220F},
	{"\\coprod", 0x02210},
	{"\\utfsum", 0x02211},
	{"\\mp", 0x02213},
	{"\\dotplus", 0x02214},
	{"\\slash", 0x02215},
	{"\\smallsetminus", 0x02216},
	{"\\ast", 0x02217},
	{"\\circ", 0x02218},
	{"\\bullet", 0x02219},	
	{"\\utfsqrt", 0x0221A},
	{"\\utfsqrt[3]", 0x0221B},
	{"\\utfsqrt[4]", 0x0221C},
	{"\\propto", 0x0221D},
	{"\\infty", 0x0221E},
	{"\\rightangle", 0x0221F},
	{"\\angle", 0x02220},
	{"\\measuredangle", 0x02221},
	{"\\sphericalangle", 0x02222},
	{"\\nmid", 0x02224},
	{"\\parallel", 0x02225},
	{"\\nparallel", 0x02226},
	{"\\wedge", 0x02227},
	{"\\vee", 0x02228},
	{"\\cap", 0x02229},
	{"\\cup", 0x0222A},
	{"\\utfint", 0x0222B},
	{"\\utfiint", 0x0222C},
	{"\\utfiiint", 0x0222D},
	{"\\utfoint", 0x0222E},
	{"\\utfoiint", 0x0222F},
	{"\\utfoiiint", 0x02230},
	{"\\varointclockwise", 0x02232},
	{"\\ointctrclockwise", 0x02233},
	{"\\therefore", 0x02234},
	{"\\because", 0x02235},
	{"\\Proportion", 0x02237},
	{"\\eqcolon", 0x02239},
	{"\\sim", 0x0223C},
	{"\\backsim", 0x0223D},
	{"\\AC", 0x0223F},
	{"\\wr", 0x02240},
	{"\\nsim", 0x02241},
	{"\\eqsim", 0x02242},
	{"\\simeq", 0x02243},
	{"\\nsimeq", 0x02244},
	{"\\cong", 0x02245},
	{"\\ncong", 0x02247},
	{"\\approx", 0x02248},
	{"\\napprox", 0x02249},
	{"\\approxeq", 0x0224A},
	{"\\asymp", 0x0224D},
	{"\\Bumpeq", 0x0224E},
	{"\\bumpeq", 0x0224F},
	{"\\doteq", 0x02250},
	{"\\Doteq", 0x02251},
	{"\\fallingdotseq", 0x02252},
	{"\\risingdotseq", 0x02253},
	{"\\coloneq", 0x02254},
	{"\\eqcirc", 0x02256},
	{"\\circeq", 0x02257},
	{"\\corresponds", 0x02259},
	{"\\triangleq", 0x0225C},
	{"\\neq", 0x02260},
	{"\\equiv", 0x02261},
	{"\\nequiv", 0x02262},
	{"\\leq", 0x02264},
	{"\\geq", 0x02265},
	{"\\leqq", 0x02266},
	{"\\geqq", 0x02267},
	{"\\lneqq", 0x02268},
	{"\\gneqq", 0x02269},
	{"\\ll", 0x0226A},
	{"\\gg", 0x0226B},
	{"\\between", 0x0226C},
	{"\\notasymp", 0x0226D},
	{"\\nless", 0x0226E},
	{"\\ngtr", 0x0226F},
	{"\\nleq", 0x02270},
	{"\\ngeq", 0x02271},
	{"\\lesssim", 0x02272},
	{"\\gtrsim", 0x02273},
	{"\\NotLessTilde", 0x02274},
	{"\\NotGreaterTilde", 0x02275},
	{"\\lessgtr", 0x02276},
	{"\\gtrless", 0x02277},
	{"\\NotGreaterLess", 0x02279},
	{"\\prec", 0x0227A},
	{"\\succ", 0x0227B},
	{"\\preccurlyeq", 0x0227C},
	{"\\succcurlyeq", 0x0227D},
	{"\\precsim", 0x0227E},
	{"\\succsim", 0x0227F},
	{"\\nprec", 0x02280},
	{"\\nsucc", 0x02281},
	{"\\subset", 0x02282},
	{"\\supset", 0x02283},
	{"\\nsubset", 0x02284},
	{"\\nsupset", 0x02285},
	{"\\subseteq", 0x02286},
	{"\\supseteq", 0x02287},
	{"\\nsubseteq", 0x02288},
	{"\\nsupseteq", 0x02289},
	{"\\subsetneq", 0x0228A},
	{"\\supsetneq", 0x0228B},
	{"\\uplus", 0x0228E},
	{"\\sqsubset", 0x0228F},
	{"\\sqsupset", 0x02290},
	{"\\sqsubseteq", 0x02291},
	{"\\sqsupseteq", 0x02292},
	{"\\sqcap", 0x02293},
	{"\\sqcup", 0x02294},
	{"\\oplus", 0x02295},
	{"\\ominus", 0x02296},
	{"\\otimes", 0x02297},
	{"\\oslash", 0x02298},
	{"\\odot", 0x02299},
	{"\\circledcirc", 0x0229A},
	{"\\circledast", 0x0229B},
	{"\\circleddash", 0x0229D},
	{"\\boxplus", 0x0229E},
	{"\\boxminus", 0x0229F},
	{"\\boxtimes", 0x022A0},
	{"\\boxdot", 0x022A1},
	{"\\vdash", 0x022A2},
	{"\\dashv", 0x022A3},
	{"\\top", 0x022A4},
	{"\\bot", 0x022A5},
	{"\\models", 0x022A7},
	{"\\vDash", 0x022A8},
	{"\\Vdash", 0x022A9},
	{"\\Vvdash", 0x022AA},
	{"\\VDash", 0x022AB},
	{"\\nvdash", 0x022AC},
	{"\\nvDash", 0x022AD},
	{"\\nVdash", 0x022AE},
	{"\\nVDash", 0x022AF},
	{"\\vartriangleleft", 0x022B2},
	{"\\vartriangleright", 0x022B3},
	{"\\trianglelefteq", 0x022B4},
	{"\\trianglerighteq", 0x022B5},
	{"\\multimapdotbothA", 0x022B6},
	{"\\multimapdotbothB", 0x022B7},
	{"\\multimap", 0x022B8},
	{"\\intercal", 0x022BA},
	{"\\veebar", 0x022BB},
	{"\\barwedge", 0x022BC},
	{"\\bigwedge", 0x022C0},
	{"\\bigvee", 0x022C1},
	{"\\bigcap", 0x022C2},
	{"\\bigcup", 0x022C3},
	{"\\diamond", 0x022C4},
	{"\\cdot", 0x022C5},
	{"\\star", 0x022C6},
	{"\\divideontimes", 0x022C7},
	{"\\bowtie", 0x022C8},
	{"\\ltimes", 0x022C9},
	{"\\rtimes", 0x022CA},
	{"\\leftthreetimes", 0x022CB},
	{"\\rightthreetimes", 0x022CC},
	{"\\backsimeq", 0x022CD},
	{"\\curlyvee", 0x022CE},
	{"\\curlywedge", 0x022CF},
	{"\\Subset", 0x022D0},
	{"\\Supset", 0x022D1},
	{"\\Cap", 0x022D2},
	{"\\Cup", 0x022D3},
	{"\\pitchfork", 0x022D4},
	{"\\hash", 0x022D5},
	{"\\lessdot", 0x022D6},
	{"\\gtrdot", 0x022D7},
	{"\\lll", 0x022D8},
	{"\\ggg", 0x022D9},
	{"\\lesseqgtr", 0x022DA},
	{"\\gtreqless", 0x022DB},
	{"\\curlyeqprec", 0x022DE},
	{"\\curlyeqsucc", 0x022DF},
	{"\\npreceq", 0x022E0},
	{"\\nsucceq", 0x022E1},
	{"\\nsqsubseteq", 0x022E2},
	{"\\nsqsupseteq", 0x022E3},
	{"\\lnsim", 0x022E6},
	{"\\gnsim", 0x022E7},
	{"\\precnsim", 0x022E8},
	{"\\succnsim", 0x022E9},
	{"\\ntriangleleft", 0x022EA},
	{"\\ntriangleright", 0x022EB},
	{"\\ntrianglelefteq", 0x022EC},
	{"\\ntrianglerighteq", 0x022ED},
	{"\\vdots", 0x022EE},
	{"\\cdots", 0x022EF},
	{"\\iddots", 0x022F0},
	{"\\ddots", 0x022F1},
	{"\\barin", 0x022F6},	
	/* misc technical */
	{"\\diameter", 0x02300},
	{"\\lceil", 0x02308},
	{"\\rceil", 0x02309},
	{"\\lfloor", 0x0230A},
	{"\\rfloor", 0x0230B},
	{"\\invneg", 0x02310},
	{"\\wasylozenge", 0x02311},
	{"\\ulcorner", 0x0231C},
	{"\\urcorner", 0x0231D},
	{"\\llcorner", 0x0231E},
	{"\\lrcorner", 0x0231F},
	{"\\frown", 0x02322},
	{"\\smile", 0x02323},
	{"\\APLinv", 0x02339},
	{"\\notslash", 0x0233F},
	{"\\notbackslash", 0x02340},
	{"\\APLleftarrowbox", 0x02347},
	{"\\APLrightarrowbox", 0x02348},
	{"\\APLuparrowbox", 0x02350},
	{"\\APLdownarrowbox", 0x02357},
	{"\\APLcomment", 0x0235D},
	{"\\APLinput", 0x0235E},
	{"\\APLlog", 0x0235F},
	{"\\overparen", 0x023DC},
	{"\\underparen", 0x023DD},
	{"\\overbrace", 0x023DE},
	{"\\underbrace", 0x023DF},	
	/* geometric shapes */
	{"\\triangle", 0x025B3},
	{"\\bigtriangleup", 0x025B3},
	{"\\blacktriangleup", 0x025B4},
	{"\\smalltriangleup", 0x025B5},
	{"\\RHD", 0x025B6},
	{"\\rhd", 0x025B7},
	{"\\blacktriangleright", 0x025B8},
	{"\\smalltriangleright", 0x025B9},
	{"\\bigtriangledown", 0x025BD},
	{"\\blacktriangledown", 0x025BE},
	{"\\smalltriangledown", 0x025BF},
	{"\\LHD", 0x025C0},
	{"\\lhd", 0x025C1},
	{"\\blacktriangleleft", 0x025C2},
	{"\\smalltriangleleft", 0x025C3},
	{"\\Diamondblack", 0x025C6},
	{"\\Diamond", 0x025C7},
	{"\\lozenge", 0x025CA},
	{"\\Box", 0x025A1},
	{"\\Circle", 0x025CB},
	{"\\CIRCLE", 0x025CF},
	{"\\LEFTcircle", 0x025D0},
	{"\\RIGHTcircle", 0x025D1},
	{"\\LEFTCIRCLE", 0x025D6},
	{"\\RIGHTCIRCLE", 0x025D7},
	{"\\boxbar", 0x025EB},
	{"\\square", 0x025FB},
	{"\\blacksquare", 0x025FC},
	/* Misc. Symbols */
	{"\\bigstar", 0x02605},
	{"\\Sun", 0x02609},
	{"\\Square", 0x02610},
	{"\\CheckedBox", 0x02611},
	{"\\XBox", 0x02612},
	{"\\steaming", 0x02615},
	{"\\pointright", 0x0261E},
	{"\\skull", 0x02620},
	{"\\radiation", 0x02622},
	{"\\biohazard", 0x02623},
	{"\\yinyang", 0x0262F},
	{"\\frownie", 0x02639},
	{"\\smiley", 0x0263A},
	{"\\blacksmiley", 0x0263B},
	{"\\sun", 0x0263C},
	{"\\rightmoon", 0x0263D},
	{"\\leftmoon", 0x0263E},
	{"\\mercury", 0x0263F},
	{"\\female", 0x02640},
	{"\\earth", 0x02641},
	{"\\male", 0x02642},
	{"\\jupiter", 0x02643},
	{"\\saturn", 0x02644},
	{"\\uranus", 0x02645},
	{"\\neptune", 0x02646},
	{"\\pluto", 0x02647},
	{"\\aries", 0x02648},
	{"\\taurus", 0x02649},
	{"\\gemini", 0x0264A},
	{"\\cancer", 0x0264B},
	{"\\leo", 0x0264C},
	{"\\virgo", 0x0264D},
	{"\\libra", 0x0264E},
	{"\\scorpio", 0x0264F},
	{"\\sagittarius", 0x02650},
	{"\\capricornus", 0x02651},
	{"\\aquarius", 0x02652},
	{"\\pisces", 0x02653},
	{"\\spadesuit", 0x02660},
	{"\\heartsuit", 0x02661},
	{"\\diamondsuit", 0x02662},
	{"\\clubsuit", 0x02663},
	{"\\varspadesuit", 0x02664},
	{"\\varheartsuit", 0x02665},
	{"\\vardiamondsuit", 0x02666},
	{"\\varclubsuit", 0x02667},
	{"\\quarternote", 0x02669},
	{"\\eighthnote", 0x0266A},
	{"\\twonotes", 0x0266B},
	{"\\sixteenthnote", 0x0266C},
	{"\\flat", 0x0266D},
	{"\\natural", 0x0266E},
	{"\\sharp", 0x0266F},
	{"\\recycle", 0x0267B},
	{"\\anchor", 0x02693},
	{"\\swords", 0x02694},
	{"\\warning", 0x026A0},
	{"\\medcirc", 0x026AA},
	{"\\medbullet", 0x026AB},
	/* dingbats */
	{"\\pencil", 0x0270E},
	{"\\checkmark", 0x02713},
	{"\\ballotx", 0x02717},
	{"\\maltese", 0x02720},
	{"\\arrowbullet", 0x027A2},
	/* Misc. Mathematical symbols-A */
	{"\\perp", 0x027C2},
	{"\\Lbag", 0x027C5},
	{"\\Rbag", 0x027C6},
	{"\\Diamonddot", 0x027D0},
	{"\\multimapinv", 0x027DC},
	{"\\llbracket", 0x027E6},
	{"\\rrbracket", 0x027E7},
	{"\\langle", 0x027E8},
	{"\\rangle", 0x027E9},
	{"\\lang", 0x027EA},
	{"\\rang", 0x027EB},
	{"\\lgroup", 0x027EE},
	{"\\rgroup", 0x027EF},
	/* Suppl. Arrows-A*/
	{"\\longleftarrow", 0x027F5},
	{"\\longrightarrow", 0x027F6},
	{"\\longleftrightarrow", 0x027F7},
	{"\\Longleftarrow", 0x027F8},
	{"\\Longrightarrow", 0x027F9},
	{"\\Longleftrightarrow", 0x027FA},
	{"\\longmapsfrom", 0x027FB},
	{"\\longmapsto", 0x027FC},
	{"\\Longmapsfrom", 0x027FD},
	{"\\Longmapsto", 0x027FE},
	/* Suppl. Arrows-B*/
	{"\\psur", 0x02900},
	{"\\Mapsfrom", 0x02906},
	{"\\Mapsto", 0x02907},
	{"\\UpArrowBar", 0x02912},
	{"\\DownArrowBar", 0x02913},
	{"\\pinj", 0x02914},
	{"\\finj", 0x02915},
	{"\\bij", 0x02916},
	{"\\leadsto", 0x02933},
	{"\\leftrightharpoon", 0x0294A},
	{"\\rightleftharpoon", 0x0294B},
	{"\\leftrightharpoonup", 0x0294E},
	{"\\rightupdownharpoon", 0x0294F},
	{"\\leftrightharpoondown", 0x02950},
	{"\\leftupdownharpoon", 0x02951},
	{"\\LeftVectorBar", 0x02952},
	{"\\RightVectorBar", 0x02953},
	{"\\RightUpVectorBar", 0x02954},
	{"\\RightDownVectorBar", 0x02955},
	{"\\DownLeftVectorBar", 0x02956},
	{"\\DownRightVectorBar", 0x02957},
	{"\\LeftUpVectorBar", 0x02958},
	{"\\LeftDownVectorBar", 0x02959},
	{"\\LeftTeeVector", 0x0295A},
	{"\\RightTeeVector", 0x0295B},
	{"\\RightUpTeeVector", 0x0295C},
	{"\\RightDownTeeVector", 0x0295D},
	{"\\DownLeftTeeVector", 0x0295E},
	{"\\DownRightTeeVector", 0x0295F},
	{"\\LeftUpTeeVector", 0x02960},
	{"\\LeftDownTeeVector", 0x02961},
	{"\\leftleftharpoons", 0x02962},
	{"\\upupharpoons", 0x02963},
	{"\\rightrightharpoons", 0x02964},
	{"\\downdownharpoons", 0x02965},
	{"\\leftbarharpoon", 0x0296A},
	{"\\barleftharpoon", 0x0296B},
	{"\\rightbarharpoon", 0x0296C},
	{"\\barrightharpoon", 0x0296D},
	{"\\updownharpoons", 0x0296E},
	{"\\downupharpoons", 0x0296F},
	{"\\strictfi", 0x0297C},
	{"\\strictif", 0x0297D},
	/* Misc. Mathematical symbols-B */
	{"\\VERT", 0x02980},
	{"\\spot", 0x02981},
	{"\\Lparen", 0x02985},
	{"\\Rparen", 0x02986},
	{"\\limg", 0x02987},
	{"\\rimg", 0x02988},
	{"\\lblot", 0x02989},
	{"\\rblot", 0x0298A},
	{"\\circledbslash", 0x029B8},
	{"\\circledless", 0x029C0},
	{"\\circledgtr", 0x029C1},
	{"\\boxslash", 0x029C4},
	{"\\boxbslash", 0x029C5},
	{"\\boxast", 0x029C6},
	{"\\boxcircle", 0x029C7},
	{"\\boxbox", 0x029C8},
	{"\\LeftTriangleBar", 0x029CF},
	{"\\RightTriangleBar", 0x029D0},
	{"\\multimapboth", 0x029DF},
	{"\\blacklozenge", 0x029EB},
	{"\\setminus", 0x029F5},
	{"\\zhide", 0x029F9},
	/* Suppl. Mathematical operators */{"\\bigodot", 0x02A00},
	{"\\bigoplus", 0x02A01},
	{"\\bigotimes", 0x02A02},
	{"\\biguplus", 0x02A04},
	{"\\bigsqcap", 0x02A05},
	{"\\bigsqcup", 0x02A06},
	{"\\varprod", 0x02A09},
	{"\\utfiiiint", 0x02A0C},
	{"\\utffint", 0x02A0F},
	{"\\utfsqint", 0x02A16},
	{"\\Join", 0x02A1D},
	{"\\zcmp", 0x02A1F},
	{"\\zpipe", 0x02A20},
	{"\\zproject", 0x02A21},
	{"\\fcmp", 0x02A3E},
	{"\\amalg", 0x02A3F},
	{"\\doublebarwedge", 0x02A5E},
	{"\\dsub", 0x02A64},
	{"\\rsub", 0x02A65},
	{"\\Coloneqq", 0x02A74},
	{"\\Equal", 0x02A75},
	{"\\Same", 0x02A76},
	{"\\leqslant", 0x02A7D},
	{"\\geqslant", 0x02A7E},
	{"\\lessapprox", 0x02A85},
	{"\\gtrapprox", 0x02A86},
	{"\\lneq", 0x02A87},
	{"\\gneq", 0x02A88},
	{"\\lnapprox", 0x02A89},
	{"\\gnapprox", 0x02A8A},
	{"\\lesseqqgtr", 0x02A8B},
	{"\\gtreqqless", 0x02A8C},
	{"\\eqslantless", 0x02A95},
	{"\\eqslantgtr", 0x02A96},
	{"\\NestedLessLess", 0x02AA1},
	{"\\NestedGreaterGreater", 0x02AA2},
	{"\\leftslice", 0x02AA6},
	{"\\rightslice", 0x02AA7},
	{"\\preceq", 0x02AAF},
	{"\\succeq", 0x02AB0},
	{"\\preceqq", 0x02AB3},
	{"\\succeqq", 0x02AB4},
	{"\\precapprox", 0x02AB7},
	{"\\succapprox", 0x02AB8},
	{"\\precnapprox", 0x02AB9},
	{"\\succnapprox", 0x02ABA},
	{"\\llcurly", 0x02ABB},
	{"\\ggcurly", 0x02ABC},
	{"\\subseteqq", 0x02AC5},
	{"\\supseteqq", 0x02AC6},
	{"\\subsetneqq", 0x02ACB},
	{"\\supsetneqq", 0x02ACC},
	{"\\Top", 0x02AEA},
	{"\\Bot", 0x02AEB},
	{"\\interleave", 0x02AF4},
	{"\\biginterleave", 0x02AFC},
	{"\\sslash", 0x02AFD},
	{"\\talloblong", 0x02AFE},
	/* misc symbols & arrows */
	/* mathematical alphanumeric symbols */
	{"\\partial", 0x1D715},
	/* terminate the table */
	/* modify some emojic characters to full blown emojis , e.g. ☠ → ☠️ (\skull  \rightarrow  \emojify \skull)*/
	/* this is a combining diacritiocal mark but not like a tex accent so it is not a function*/
	{"\\emojify", 0x0FE0F},	
	
	/* whatever symbols that were not there but were in https://github.com/JuliaLang/julia/blob/master/stdlib/REPL/src/latex_symbols.jl */
	{"\\cbrt",0x221B}, //  0x221B
	{"\\mars",0x02642}, //  ♂
	{"\\xor",0x022BB}, //  ⊻
	{"\\nand",0x022BC}, //  ⊼
	{"\\nor",0x022BD}, //  ⊽
	{"\\iff",0x027FA}, //  ⟺
	{"\\implies",0x027F9}, //  ⟹
	{"\\impliedby",0x027F8}, //  ⟸
	{"\\euler",0x0212F}, //  ℯ
	{"\\ohm",0x02126}, //  Ω
	{"\\hbar",0x00127}, //  ħ
	{"\\del",0x02207}, //  ∇
	{"\\euro",0x020AC}, //  €
	{"\\exclamdown",0x000A1}, //  ¡
	{"\\sterling",0x000A3}, //  £
	{"\\brokenbar",0x000A6}, //  ¦
	{"\\S",0x000A7}, //  §
	{"\\copyright",0x000A9}, //  ©
	{"\\ordfeminine",0x000AA}, //  ª
	{"\\highminus",0x000AF}, //  ¯ # APL high minus or non-combining macron above
	{"\\P",0x000B6}, //  ¶
	{"\\cdotp",0x000B7}, //  ·
	{"\\ordmasculine",0x000BA}, //  º
	{"\\questiondown",0x000BF}, //  ¿
	{"\\AA",0x000C5}, //  Å
	{"\\AE",0x000C6}, //  Æ
	{"\\DH",0x000D0}, //  Ð
	{"\\O",0x000D8}, //  Ø
	{"\\TH",0x000DE}, //  Þ
	{"\\ss",0x000DF}, //  ß
	{"\\aa",0x000E5}, //  å
	{"\\ae",0x000E6}, //  æ
	{"\\dh",0x000F0}, //  ð
	{"\\o",0x000F8}, //  ø
	{"\\th",0x000FE}, //  þ
	{"\\DJ",0x00110}, //  Đ
	{"\\dj",0x00111}, //  đ
	{"\\L",0x00141}, //  Ł
	{"\\NG",0x0014A}, //  Ŋ
	{"\\ng",0x0014B}, //  ŋ
	{"\\OE",0x00152}, //  Œ
	{"\\oe",0x00153}, //  œ
	{"\\hvlig",0x00195}, //  ƕ
	{"\\nrleg",0x0019E}, //  ƞ
	{"\\doublepipe",0x001C2}, //  ǂ
	{"\\trna",0x00250}, //  ɐ
	{"\\trnsa",0x00252}, //  ɒ
	{"\\openo",0x00254}, //  ɔ
	{"\\rtld",0x00256}, //  ɖ
	{"\\schwa",0x00259}, //  ə
	{"\\pgamma",0x00263}, //  ɣ
	{"\\pbgam",0x00264}, //  ɤ
	{"\\trnh",0x00265}, //  ɥ
	{"\\btdl",0x0026C}, //  ɬ
	{"\\rtll",0x0026D}, //  ɭ
	{"\\trnm",0x0026F}, //  ɯ
	{"\\trnmlr",0x00270}, //  ɰ
	{"\\ltlmr",0x00271}, //  ɱ
	{"\\ltln",0x00272}, //  ɲ
	{"\\rtln",0x00273}, //  ɳ
	{"\\clomeg",0x00277}, //  ɷ
	{"\\ltphi",0x00278}, //  ɸ # latin ϕ
	{"\\trnr",0x00279}, //  ɹ
	{"\\trnrl",0x0027A}, //  ɺ
	{"\\rttrnr",0x0027B}, //  ɻ
	{"\\rl",0x0027C}, //  ɼ
	{"\\rtlr",0x0027D}, //  ɽ
	{"\\fhr",0x0027E}, //  ɾ
	{"\\rtls",0x00282}, //  ʂ
	{"\\esh",0x00283}, //  ʃ
	{"\\trnt",0x00287}, //  ʇ
	{"\\rtlt",0x00288}, //  ʈ
	{"\\pupsil",0x0028A}, //  ʊ
	{"\\pscrv",0x0028B}, //  ʋ
	{"\\invv",0x0028C}, //  ʌ
	{"\\invw",0x0028D}, //  ʍ
	{"\\trny",0x0028E}, //  ʎ
	{"\\rtlz",0x00290}, //  ʐ
	{"\\yogh",0x00292}, //  ʒ
	{"\\glst",0x00294}, //  ʔ
	{"\\reglst",0x00295}, //  ʕ
	{"\\inglst",0x00296}, //  ʖ
	{"\\turnk",0x0029E}, //  ʞ
	{"\\dyogh",0x002A4}, //  ʤ
	{"\\tesh",0x002A7}, //  ʧ
	{"\\rasp",0x002BC}, //  ʼ
	{"\\verts",0x002C8}, //  ˈ
	{"\\verti",0x002CC}, //  ˌ
	{"\\lmrk",0x002D0}, //  ː
	{"\\hlmrk",0x002D1}, //  ˑ
	{"\\sbrhr",0x002D2}, //  ˒
	{"\\sblhr",0x002D3}, //  ˓
	{"\\rais",0x002D4}, //  ˔
	{"\\low",0x002D5}, //  ˕
	{"\\u",0x002D8}, //  ˘
	{"\\tildelow",0x002DC}, //  ˜
	{"\\Alpha",0x00391}, //  Α
	{"\\Beta",0x00392}, //  Β
	{"\\Epsilon",0x00395}, //  Ε
	{"\\Zeta",0x00396}, //  Ζ
	{"\\Eta",0x00397}, //  Η
	{"\\Iota",0x00399}, //  Ι
	{"\\Kappa",0x0039A}, //  Κ
	{"\\Rho",0x003A1}, //  Ρ
	{"\\Tau",0x003A4}, //  Τ
	{"\\Chi",0x003A7}, //  Χ
	{"\\varkappa",0x003F0}, //  ϰ
	{"\\varTheta",0x003F4}, //  ϴ
	{"\\enspace",0x02002}, //   
	{"\\thickspace",0x02005}, //   
	{"\\thinspace",0x02009}, //   
	{"\\hspace",0x0200A}, //   
	{"\\endash",0x02013}, //  –
	{"\\emdash",0x02014}, //  —
	{"\\Vert",0x02016}, //  ‖
	{"\\lVert",0x02016}, //  ‖
	{"\\rVert",0x02016}, //  ‖
	{"\\lq",0x02018}, //  ‘
	{"\\rq",0x02019}, //  ’
	{"\\reapos",0x0201B}, //  ‛
	{"\\ldq",0x0201C}, //  “
	{"\\rdq",0x0201D}, //  ”
	{"\\dots",0x02026}, //  …
	{"\\perthousand",0x02030}, //  ‰
	{"\\pertenthousand",0x02031}, //  ‱
	{"\\guilsinglleft",0x02039}, //  ‹
	{"\\guilsinglright",0x0203A}, //  ›
	{"\\nolinebreak",0x2060}, //  0x2060
	{"\\pes",0x020A7}, //  ₧
	{"\\numero",0x02116}, //  №
	{"\\xrat",0x0211E}, //  ℞
	{"\\trademark",0x02122}, //  ™
	{"\\bbsum",0x02140}, //  ⅀
	{"\\Game",0x02141}, //  ⅁
	{"\\dblarrowupdown",0x021C5}, //  ⇅
	{"\\DownArrowUpArrow",0x021F5}, //  ⇵
	{"\\emptyset",0x02205}, //  ∅
	{"\\surd",0x0221A}, //  √
	{"\\clwintegral",0x02231}, //  ∱
	{"\\Colon",0x02237}, //  ∷
	{"\\dotminus",0x02238}, //  ∸
	{"\\kernelcontraction",0x0223B}, //  ∻
	{"\\lazysinv",0x0223E}, //  ∾
	{"\\neqsim",0x02242}, //  ≂̸
	{"\\nsime",0x02244}, //  ≄
	{"\\approxnotequal",0x02246}, //  ≆
	{"\\tildetrpl",0x0224B}, //  ≋
	{"\\allequal",0x0224C}, //  ≌
	{"\\nBumpeq",0x0224E}, //  ≎̸
	{"\\nbumpeq",0x0224F}, //  ≏̸
	{"\\wedgeq",0x02259}, //  ≙
	{"\\starequal",0x0225B}, //  ≛
	{"\\questeq",0x0225F}, //  ≟
	{"\\ne",0x02260}, //  ≠
	{"\\le",0x02264}, //  ≤
	{"\\ge",0x02265}, //  ≥
	{"\\lvertneqq",0x02268}, //  ≨︀
	{"\\gvertneqq",0x02269}, //  ≩︀
	{"\\NotLessLess",0x0226A}, //  ≪̸
	{"\\NotGreaterGreater",0x0226B}, //  ≫̸
	{"\\notlessgreater",0x02278}, //  ≸
	{"\\notgreaterless",0x02279}, //  ≹
	{"\\nprecsim",0x0227E}, //  ≾̸
	{"\\nsuccsim",0x0227F}, //  ≿̸
	{"\\varsubsetneqq",0x0228A}, //  ⊊︀
	{"\\varsupsetneq",0x0228B}, //  ⊋︀
	{"\\cupdot",0x0228D}, //  ⊍
	{"\\NotSquareSubset",0x0228F}, //  ⊏̸
	{"\\NotSquareSuperset",0x02290}, //  ⊐̸
	{"\\indep",0x02AEB}, //  ⫫
	{"\\downvDash",0x02AEA}, //  ⫪
	{"\\upvDash",0x02AEB}, //  ⫫
	{"\\original",0x022B6}, //  ⊶
	{"\\image",0x022B7}, //  ⊷
	{"\\hermitconjmatrix",0x022B9}, //  ⊹
	{"\\rightanglearc",0x022BE}, //  ⊾
	{"\\verymuchless",0x022D8}, //  ⋘
	{"\\adots",0x022F0}, //  ⋰
	{"\\recorder",0x02315}, //  ⌕
	{"\\obar",0x0233D}, //  ⌽
	{"\\dlcorn",0x023A3}, //  ⎣
	{"\\lmoustache",0x023B0}, //  ⎰
	{"\\visiblespace",0x02423}, //  ␣
	{"\\circledS",0x024C8}, //  Ⓢ
	{"\\dshfnc",0x02506}, //  ┆
	{"\\sqfnw",0x02519}, //  ┙
	{"\\diagup",0x02571}, //  ╱
	{"\\diagdown",0x02572}, //  ╲
	{"\\vrecto",0x025AF}, //  ▯
	{"\\blacktriangle",0x025B4}, //  ▴
	{"\\vartriangle",0x025B5}, //  ▵
	{"\\triangledown",0x025BF}, //  ▿
	{"\\bigcirc",0x025CB}, //  ○
	{"\\cirfl",0x025D0}, //  ◐
	{"\\cirfr",0x025D1}, //  ◑
	{"\\cirfb",0x025D2}, //  ◒
	{"\\rvbull",0x025D8}, //  ◘
	{"\\sqfl",0x025E7}, //  ◧
	{"\\sqfr",0x025E8}, //  ◨
	{"\\sqfse",0x025EA}, //  ◪
	{"\\venus",0x02640}, //  ♀
	{"\\Uuparrow",0x0290A}, //  ⤊
	{"\\Ddownarrow",0x0290B}, //  ⤋
	{"\\bkarow",0x0290D}, //  ⤍
	{"\\dbkarow",0x0290F}, //  ⤏
	{"\\drbkarrow",0x02910}, //  ⤐
	{"\\twoheadrightarrowtail",0x02916}, //  ⤖
	{"\\hksearow",0x02925}, //  ⤥
	{"\\hkswarow",0x02926}, //  ⤦
	{"\\tona",0x02927}, //  ⤧
	{"\\toea",0x02928}, //  ⤨
	{"\\tosa",0x02929}, //  ⤩
	{"\\towa",0x0292A}, //  ⤪
	{"\\rdiagovfdiag",0x0292B}, //  ⤫
	{"\\fdiagovrdiag",0x0292C}, //  ⤬
	{"\\seovnearrow",0x0292D}, //  ⤭
	{"\\neovsearrow",0x0292E}, //  ⤮
	{"\\fdiagovnearrow",0x0292F}, //  ⤯
	{"\\rdiagovsearrow",0x02930}, //  ⤰
	{"\\neovnwarrow",0x02931}, //  ⤱
	{"\\nwovnearrow",0x02932}, //  ⤲
	{"\\Rlarr",0x02942}, //  ⥂
	{"\\rLarr",0x02944}, //  ⥄
	{"\\rarrx",0x02947}, //  ⥇
	{"\\LeftRightVector",0x0294E}, //  ⥎
	{"\\RightUpDownVector",0x0294F}, //  ⥏
	{"\\DownLeftRightVector",0x02950}, //  ⥐
	{"\\LeftUpDownVector",0x02951}, //  ⥑
	{"\\UpEquilibrium",0x0296E}, //  ⥮
	{"\\ReverseUpEquilibrium",0x0296F}, //  ⥯
	{"\\RoundImplies",0x02970}, //  ⥰
	{"\\Vvert",0x02980}, //  ⦀
	{"\\Elroang",0x02986}, //  ⦆
	{"\\ddfnc",0x02999}, //  ⦙
	{"\\Angle",0x0299C}, //  ⦜
	{"\\lpargt",0x029A0}, //  ⦠
	{"\\obslash",0x029B8}, //  ⦸
	{"\\Lap",0x029CA}, //  ⧊
	{"\\defas",0x029CB}, //  ⧋
	{"\\NotLeftTriangleBar",0x029CF}, //  ⧏̸
	{"\\NotRightTriangleBar",0x029D0}, //  ⧐̸
	{"\\dualmap",0x029DF}, //  ⧟
	{"\\shuffle",0x029E2}, //  ⧢
	{"\\RuleDelayed",0x029F4}, //  ⧴
	{"\\bigcupdot",0x02A03}, //  ⨃
	{"\\conjquant",0x02A07}, //  ⨇
	{"\\disjquant",0x02A08}, //  ⨈
	{"\\bigtimes",0x02A09}, //  ⨉
	{"\\clockoint",0x02A0F}, //  ⨏
	{"\\sqrint",0x02A16}, //  ⨖
	{"\\upint",0x02A1B}, //  ⨛
	{"\\lowint",0x02A1C}, //  ⨜
	{"\\plusdot",0x02A25}, //  ⨥
	{"\\Times",0x02A2F}, //  ⨯
	{"\\btimes",0x02A32}, //  ⨲
	{"\\And",0x02A53}, //  ⩓
	{"\\Or",0x02A54}, //  ⩔
	{"\\ElOr",0x02A56}, //  ⩖
	{"\\perspcorrespond",0x02A5E}, //  ⩞
	{"\\ddotseq",0x02A77}, //  ⩷
	{"\\nleqslant",0x02A7D}, //  ⩽̸
	{"\\ngeqslant",0x02A7E}, //  ⩾̸
	{"\\NotNestedLessLess",0x02AA1}, //  ⪡̸
	{"\\NotNestedGreaterGreater",0x02AA2}, //  ⪢̸
	{"\\partialmeetcontraction",0x02AA3}, //  ⪣
	{"\\bumpeqq",0x02AAE}, //  ⪮
	{"\\precneqq",0x02AB5}, //  ⪵
	{"\\succneqq",0x02AB6}, //  ⪶
	{"\\nsubseteqq",0x02AC5}, //  ⫅̸
	{"\\nsupseteqq",0x02AC6}, //  ⫆̸
	{"\\mlcp",0x02ADB}, //  ⫛
	{"\\forks",0x02ADC}, //  ⫝̸
	{"\\forksnot",0x02ADD}, //  ⫝
	{"\\dashV",0x02AE3}, //  ⫣
	{"\\Dashv",0x02AE4}, //  ⫤
	{"\\tdcol",0x02AF6}, //  ⫶
	{"\\openbracketleft",0x027E6}, //  ⟦
	{"\\openbracketright",0x027E7}, //  ⟧
	{"\\Zbar",0x001B5}, //  Ƶ # impedance (latin capital letter z with stroke)

	{"\\upMu",0x0039C}, //  Μ # capital mu greek
	{"\\upNu",0x0039D}, //  Ν # capital nu greek
	{"\\upOmicron",0x0039F}, //  Ο # capital omicron greek
	{"\\upepsilon",0x003B5}, //  ε # rounded small epsilon greek
	{"\\upomicron",0x003BF}, //  ο # small omicron greek
	{"\\upvarbeta",0x003D0}, //  ϐ # rounded small beta greek
	{"\\upoldKoppa",0x003D8}, //  Ϙ # greek letter archaic koppa
	{"\\upoldkoppa",0x003D9}, //  ϙ # greek small letter archaic koppa
	{"\\upstigma",0x003DB}, //  ϛ # greek small letter stigma
	{"\\upkoppa",0x003DF}, //  ϟ # greek small letter koppa
	{"\\upsampi",0x003E1}, //  ϡ # greek small letter sampi
	{"\\tieconcat",0x02040}, //  ⁀ # character tie z notation sequence concatenation
	
	{"\\eulermascheroni",0x02107}, //  ℇ # euler-mascheroni constant U+2107
	{"\\planck",0x0210E}, //  ℎ # planck constant
	{"\\turnediota",0x02129}, //  ℩ # turned iota
	{"\\Angstrom",0x0212B}, //  Å # angstrom capital a ring
	{"\\sansLturned",0x02142}, //  ⅂ # turned sans-serif capital l
	{"\\sansLmirrored",0x02143}, //  ⅃ # reversed sans-serif capital l
	{"\\PropertyLine",0x0214A}, //  ⅊ # property line
	{"\\upand",0x0214B}, //  ⅋ # turned ampersand
	{"\\twoheaduparrow",0x0219F}, //  ↟ # up two-headed arrow
	{"\\twoheaddownarrow",0x021A1}, //  ↡ # down two-headed arrow
	{"\\mapsup",0x021A5}, //  ↥ # maps to upward
	{"\\mapsdown",0x021A7}, //  ↧ # maps to downward
	{"\\updownarrowbar",0x021A8}, //  ↨ # up down arrow with base (perpendicular)
	{"\\downzigzagarrow",0x021AF}, //  ↯ # downwards zigzag arrow
	{"\\Ldsh",0x021B2}, //  ↲ # left down angled arrow
	{"\\Rdsh",0x021B3}, //  ↳ # right down angled arrow
	{"\\linefeed",0x021B4}, //  ↴ # rightwards arrow with corner downwards
	{"\\carriagereturn",0x021B5}, //  ↵ # downwards arrow with corner leftward = carriage return
	{"\\nHuparrow",0x021DE}, //  ⇞ # upwards arrow with double stroke
	{"\\nHdownarrow",0x021DF}, //  ⇟ # downwards arrow with double stroke
	{"\\leftdasharrow",0x021E0}, //  ⇠ # leftwards dashed arrow
	{"\\updasharrow",0x021E1}, //  ⇡ # upwards dashed arrow
	{"\\rightdasharrow",0x021E2}, //  ⇢ # rightwards dashed arrow
	{"\\downdasharrow",0x021E3}, //  ⇣ # downwards dashed arrow
	{"\\rightarrowbar",0x021E5}, //  ⇥ # rightwards arrow to bar
	{"\\leftwhitearrow",0x021E6}, //  ⇦ # leftwards white arrow
	{"\\upwhitearrow",0x021E7}, //  ⇧ # upwards white arrow
	{"\\rightwhitearrow",0x021E8}, //  ⇨ # rightwards white arrow
	{"\\downwhitearrow",0x021E9}, //  ⇩ # downwards white arrow
	{"\\whitearrowupfrombar",0x021EA}, //  ⇪ # upwards white arrow from bar
	{"\\circleonrightarrow",0x021F4}, //  ⇴ # right arrow with small circle
	{"\\rightthreearrows",0x021F6}, //  ⇶ # three rightwards arrows
	{"\\nvleftarrow",0x021F7}, //  ⇷ # leftwards arrow with vertical stroke
	{"\\nvrightarrow",0x021F8}, //  ⇸ # rightwards arrow with vertical stroke
	{"\\nvleftrightarrow",0x021F9}, //  ⇹ # left right arrow with vertical stroke
	{"\\nVleftarrow",0x021FA}, //  ⇺ # leftwards arrow with double vertical stroke
	{"\\nVrightarrow",0x021FB}, //  ⇻ # rightwards arrow with double vertical stroke
	{"\\nVleftrightarrow",0x021FC}, //  ⇼ # left right arrow with double vertical stroke
	{"\\increment",0x02206}, //  ∆ # laplacian (delta; nabla\string^2)
	{"\\smallin",0x0220A}, //  ∊ # set membership (small set membership)
	{"\\smallni",0x0220D}, //  ∍ # /ni /owns r: contains (small contains as member)
	{"\\QED",0x0220E}, //  ∎ # end of proof
	{"\\vysmblkcircle",0x02219}, //  ∙ # bullet operator
	{"\\fourthroot",0x0221C}, //  ∜ # fourth root
	{"\\dotsminusdots",0x0223A}, //  ∺ # minus with four dots geometric properties
	{"\\arceq",0x02258}, //  ≘ # arc equals; corresponds to
	{"\\veeeq",0x0225A}, //  ≚ # logical or equals
	{"\\eqdef",0x0225D}, //  ≝ # equals by definition
	{"\\measeq",0x0225E}, //  ≞ # measured by (m over equals)
	{"\\Equiv",0x02263}, //  ≣ # strict equivalence (4 lines)
	{"\\nasymp",0x0226D}, //  ≭ # not asymptotically equal to
	{"\\nlesssim",0x02274}, //  ≴ # not less similar
	{"\\ngtrsim",0x02275}, //  ≵ # not greater similar
	{"\\circledequal",0x0229C}, //  ⊜ # equal in circle
	{"\\prurel",0x022B0}, //  ⊰ # element precedes under relation
	{"\\scurel",0x022B1}, //  ⊱ # succeeds under relation
	{"\\varlrtriangle",0x022BF}, //  ⊿ # right triangle
	{"\\equalparallel",0x022D5}, //  ⋕ # parallel equal; equal or parallel
	{"\\eqless",0x022DC}, //  ⋜ # equal-or-less
	{"\\eqgtr",0x022DD}, //  ⋝ # equal-or-greater
	{"\\npreccurlyeq",0x022E0}, //  ⋠ # not precedes curly equals
	{"\\nsucccurlyeq",0x022E1}, //  ⋡ # not succeeds curly equals
	{"\\sqsubsetneq",0x022E4}, //  ⋤ # square subset not equals
	{"\\sqsupsetneq",0x022E5}, //  ⋥ # square superset not equals
	{"\\disin",0x022F2}, //  ⋲ # element of with long horizontal stroke
	{"\\varisins",0x022F3}, //  ⋳ # element of with vertical bar at end of horizontal stroke
	{"\\isins",0x022F4}, //  ⋴ # small element of with vertical bar at end of horizontal stroke
	{"\\isindot",0x022F5}, //  ⋵ # element of with dot above
	{"\\varisinobar",0x022F6}, //  ⋶ # element of with overbar
	{"\\isinobar",0x022F7}, //  ⋷ # small element of with overbar
	{"\\isinvb",0x022F8}, //  ⋸ # element of with underbar
	{"\\isinE",0x022F9}, //  ⋹ # element of with two horizontal strokes
	{"\\nisd",0x022FA}, //  ⋺ # contains with long horizontal stroke
	{"\\varnis",0x022FB}, //  ⋻ # contains with vertical bar at end of horizontal stroke
	{"\\nis",0x022FC}, //  ⋼ # small contains with vertical bar at end of horizontal stroke
	{"\\varniobar",0x022FD}, //  ⋽ # contains with overbar
	{"\\niobar",0x022FE}, //  ⋾ # small contains with overbar
	{"\\bagmember",0x022FF}, //  ⋿ # z notation bag membership
	{"\\house",0x02302}, //  ⌂ # house
	{"\\vardoublebarwedge",0x02306}, //  ⌆ # /doublebarwedge b: logical and double bar above [perspective (double bar over small wedge)]
	{"\\invnot",0x02310}, //  ⌐ # reverse not
	{"\\sqlozenge",0x02311}, //  ⌑ # square lozenge
	{"\\profline",0x02312}, //  ⌒ # profile of a line
	{"\\profsurf",0x02313}, //  ⌓ # profile of a surface
	{"\\viewdata",0x02317}, //  ⌗ # viewdata square
	{"\\turnednot",0x02319}, //  ⌙ # turned not sign
	{"\\varhexagonlrbonds",0x0232C}, //  ⌬ # six carbon ring corner down double bonds lower right etc
	{"\\conictaper",0x02332}, //  ⌲ # conical taper
	{"\\topbot",0x02336}, //  ⌶ # top and bottom
	{"\\hexagon",0x02394}, //  ⎔ # horizontal benzene ring [hexagon flat open]
	{"\\underbracket",0x023B5}, //  ⎵ # bottom square bracket
	{"\\bbrktbrk",0x023B6}, //  ⎶ # bottom square bracket over top square bracket
	{"\\lvboxline",0x023B8}, //  ⎸ # left vertical box line
	{"\\rvboxline",0x023B9}, //  ⎹ # right vertical box line
	{"\\varcarriagereturn",0x023CE}, //  ⏎ # return symbol
	{"\\trapezium",0x23E2}, //  0x23E2 # white trapezium
	{"\\benzenr",0x23E3}, //  0x23E3 # benzene ring with circle
	{"\\strns",0x23E4}, //  0x23E4 # straightness
	{"\\fltns",0x23E5}, //  0x23E5 # flatness
	{"\\accurrent",0x23E6}, //  0x23E6 # ac current
	{"\\elinters",0x23E7}, //  0x23E7 # electrical intersection
	{"\\blanksymbol",0x02422}, //  ␢ # blank symbol
	{"\\blockuphalf",0x02580}, //  ▀ # upper half block
	{"\\blocklowhalf",0x02584}, //  ▄ # lower half block
	{"\\blockfull",0x02588}, //  █ # full block
	{"\\blocklefthalf",0x0258C}, //  ▌ # left half block
	{"\\blockrighthalf",0x02590}, //  ▐ # right half block
	{"\\blockqtrshaded",0x02591}, //  ░ # 25\% shaded block
	{"\\blockhalfshaded",0x02592}, //  ▒ # 50\% shaded block
	{"\\blockthreeqtrshaded",0x02593}, //  ▓ # 75\% shaded block
	{"\\squoval",0x025A2}, //  ▢ # white square with rounded corners
	{"\\blackinwhitesquare",0x025A3}, //  ▣ # white square containing black small square
	{"\\squarehfill",0x025A4}, //  ▤ # square horizontal rule filled
	{"\\squarevfill",0x025A5}, //  ▥ # square vertical rule filled
	{"\\squarehvfill",0x025A6}, //  ▦ # square with orthogonal crosshatch fill
	{"\\squarenwsefill",0x025A7}, //  ▧ # square nw-to-se rule filled
	{"\\squareneswfill",0x025A8}, //  ▨ # square ne-to-sw rule filled
	{"\\squarecrossfill",0x025A9}, //  ▩ # square with diagonal crosshatch fill
	{"\\smblksquare",0x025AA}, //  ▪ # /blacksquare - sq bullet filled
	{"\\smwhtsquare",0x025AB}, //  ▫ # white small square
	{"\\hrectangleblack",0x025AC}, //  ▬ # black rectangle
	{"\\hrectangle",0x025AD}, //  ▭ # horizontal rectangle open
	{"\\vrectangleblack",0x025AE}, //  ▮ # black vertical rectangle
	{"\\parallelogramblack",0x025B0}, //  ▰ # black parallelogram
	{"\\parallelogram",0x025B1}, //  ▱ # parallelogram open
	{"\\bigblacktriangleup",0x025B2}, //  ▲ # 0x25b2 6 6d black up-pointing triangle
	{"\\blackpointerright",0x025BA}, //  ► # black right-pointing pointer
	{"\\whitepointerright",0x025BB}, //  ▻ # white right-pointing pointer
	{"\\bigblacktriangledown",0x025BC}, //  ▼ # big down triangle filled
	{"\\blackpointerleft",0x025C4}, //  ◄ # black left-pointing pointer
	{"\\whitepointerleft",0x025C5}, //  ◅ # white left-pointing pointer
	{"\\mdlgblkdiamond",0x025C6}, //  ◆ # black diamond
	{"\\mdlgwhtdiamond",0x025C7}, //  ◇ # white diamond; diamond open
	{"\\blackinwhitediamond",0x025C8}, //  ◈ # white diamond containing black small diamond
	{"\\fisheye",0x025C9}, //  ◉ # fisheye
	{"\\dottedcircle",0x025CC}, //  ◌ # dotted circle
	{"\\circlevertfill",0x025CD}, //  ◍ # circle with vertical fill
	{"\\bullseye",0x025CE}, //  ◎ # bullseye
	{"\\mdlgblkcircle",0x025CF}, //  ● # circle filled
	{"\\circletophalfblack",0x025D3}, //  ◓ # circle filled top half
	{"\\circleurquadblack",0x025D4}, //  ◔ # circle with upper right quadrant black
	{"\\blackcircleulquadwhite",0x025D5}, //  ◕ # circle with all but upper left quadrant black
	{"\\blacklefthalfcircle",0x025D6}, //  ◖ # left half black circle
	{"\\blackrighthalfcircle",0x025D7}, //  ◗ # right half black circle
	{"\\inversewhitecircle",0x025D9}, //  ◙ # inverse white circle
	{"\\invwhiteupperhalfcircle",0x025DA}, //  ◚ # upper half inverse white circle
	{"\\invwhitelowerhalfcircle",0x025DB}, //  ◛ # lower half inverse white circle
	{"\\ularc",0x025DC}, //  ◜ # upper left quadrant circular arc
	{"\\urarc",0x025DD}, //  ◝ # upper right quadrant circular arc
	{"\\lrarc",0x025DE}, //  ◞ # lower right quadrant circular arc
	{"\\llarc",0x025DF}, //  ◟ # lower left quadrant circular arc
	{"\\topsemicircle",0x025E0}, //  ◠ # upper half circle
	{"\\botsemicircle",0x025E1}, //  ◡ # lower half circle
	{"\\lrblacktriangle",0x025E2}, //  ◢ # lower right triangle filled
	{"\\llblacktriangle",0x025E3}, //  ◣ # lower left triangle filled
	{"\\ulblacktriangle",0x025E4}, //  ◤ # upper left triangle filled
	{"\\urblacktriangle",0x025E5}, //  ◥ # upper right triangle filled
	{"\\smwhtcircle",0x025E6}, //  ◦ # white bullet
	{"\\squareulblack",0x025E9}, //  ◩ # square filled top left corner
	{"\\trianglecdot",0x025EC}, //  ◬ # triangle with centered dot
	{"\\triangleleftblack",0x025ED}, //  ◭ # up-pointing triangle with left half black
	{"\\trianglerightblack",0x025EE}, //  ◮ # up-pointing triangle with right half black
	{"\\lgwhtcircle",0x025EF}, //  ◯ # large circle
	{"\\squareulquad",0x025F0}, //  ◰ # white square with upper left quadrant
	{"\\squarellquad",0x025F1}, //  ◱ # white square with lower left quadrant
	{"\\squarelrquad",0x025F2}, //  ◲ # white square with lower right quadrant
	{"\\squareurquad",0x025F3}, //  ◳ # white square with upper right quadrant
	{"\\circleulquad",0x025F4}, //  ◴ # white circle with upper left quadrant
	{"\\circlellquad",0x025F5}, //  ◵ # white circle with lower left quadrant
	{"\\circlelrquad",0x025F6}, //  ◶ # white circle with lower right quadrant
	{"\\circleurquad",0x025F7}, //  ◷ # white circle with upper right quadrant
	{"\\ultriangle",0x025F8}, //  ◸ # upper left triangle
	{"\\urtriangle",0x025F9}, //  ◹ # upper right triangle
	{"\\lltriangle",0x025FA}, //  ◺ # lower left triangle
	{"\\mdwhtsquare",0x025FB}, //  ◻ # white medium square
	{"\\mdblksquare",0x025FC}, //  ◼ # black medium square
	{"\\mdsmwhtsquare",0x025FD}, //  ◽ # white medium small square
	{"\\mdsmblksquare",0x025FE}, //  ◾ # black medium small square
	{"\\lrtriangle",0x025FF}, //  ◿ # lower right triangle
	{"\\bigwhitestar",0x02606}, //  ☆ # star open
	{"\\astrosun",0x02609}, //  ☉ # sun
	{"\\danger",0x02621}, //  ☡ # dangerous bend (caution sign)
	{"\\acidfree",0x267E}, //  0x267E # permanent paper sign
	{"\\dicei",0x02680}, //  ⚀ # die face-1
	{"\\diceii",0x02681}, //  ⚁ # die face-2
	{"\\diceiii",0x02682}, //  ⚂ # die face-3
	{"\\diceiv",0x02683}, //  ⚃ # die face-4
	{"\\dicev",0x02684}, //  ⚄ # die face-5
	{"\\dicevi",0x02685}, //  ⚅ # die face-6
	{"\\circledrightdot",0x02686}, //  ⚆ # white circle with dot right
	{"\\circledtwodots",0x02687}, //  ⚇ # white circle with two dots
	{"\\blackcircledrightdot",0x02688}, //  ⚈ # black circle with white dot right
	{"\\blackcircledtwodots",0x02689}, //  ⚉ # black circle with two white dots
	{"\\hermaphrodite",0x26A5}, //  0x26A5 # male and female sign
	{"\\mdwhtcircle",0x26AA}, //  0x26AA # medium white circle
	{"\\mdblkcircle",0x26AB}, //  0x26AB # medium black circle
	{"\\mdsmwhtcircle",0x26AC}, //  0x26AC # medium small white circle
	{"\\neuter",0x26B2}, //  0x26B2 # neuter
	{"\\circledstar",0x0272A}, //  ✪ # circled white star
	{"\\varstar",0x02736}, //  ✶ # six pointed black star
	{"\\dingasterisk",0x0273D}, //  ✽ # heavy teardrop-spoked asterisk
	{"\\draftingarrow",0x0279B}, //  ➛ # right arrow with bold head (drafting)
	{"\\threedangle",0x27C0}, //  0x27C0 # three dimensional angle
	{"\\whiteinwhitetriangle",0x27C1}, //  0x27C1 # white triangle containing small white triangle
	{"\\bsolhsub",0x27C8}, //  0x27C8 # reverse solidus preceding subset
	{"\\suphsol",0x27C9}, //  0x27C9 # superset preceding solidus
	{"\\wedgedot",0x027D1}, //  ⟑ # and with dot
	{"\\veedot",0x027C7}, //  ⟇ # or with dot
	{"\\upin",0x027D2}, //  ⟒ # element of opening upwards
	{"\\bigbot",0x027D8}, //  ⟘ # large up tack
	{"\\bigtop",0x027D9}, //  ⟙ # large down tack
	{"\\UUparrow",0x027F0}, //  ⟰ # upwards quadruple arrow
	{"\\DDownarrow",0x027F1}, //  ⟱ # downwards quadruple arrow
	{"\\longrightsquigarrow",0x027FF}, //  ⟿ # long rightwards squiggle arrow
	{"\\nvtwoheadrightarrow",0x02900}, //  ⤀ # rightwards two-headed arrow with vertical stroke
	{"\\nVtwoheadrightarrow",0x02901}, //  ⤁ # rightwards two-headed arrow with double vertical stroke
	{"\\nvLeftarrow",0x02902}, //  ⤂ # leftwards double arrow with vertical stroke
	{"\\nvRightarrow",0x02903}, //  ⤃ # rightwards double arrow with vertical stroke
	{"\\nvLeftrightarrow",0x02904}, //  ⤄ # left right double arrow with vertical stroke
	{"\\twoheadmapsto",0x02905}, //  ⤅ # rightwards two-headed arrow from bar
	{"\\downarrowbarred",0x02908}, //  ⤈ # downwards arrow with horizontal stroke
	{"\\uparrowbarred",0x02909}, //  ⤉ # upwards arrow with horizontal stroke
	{"\\leftbkarrow",0x0290C}, //  ⤌ # leftwards double dash arrow
	{"\\leftdbkarrow",0x0290E}, //  ⤎ # leftwards triple dash arrow
	{"\\rightdotarrow",0x02911}, //  ⤑ # rightwards arrow with dotted stem
	{"\\nvrightarrowtail",0x02914}, //  ⤔ # rightwards arrow with tail with vertical stroke
	{"\\nVrightarrowtail",0x02915}, //  ⤕ # rightwards arrow with tail with double vertical stroke
	{"\\nvtwoheadrightarrowtail",0x02917}, //  ⤗ # rightwards two-headed arrow with tail with vertical stroke
	{"\\nVtwoheadrightarrowtail",0x02918}, //  ⤘ # rightwards two-headed arrow with tail with double vertical stroke
	{"\\diamondleftarrow",0x0291D}, //  ⤝ # leftwards arrow to black diamond
	{"\\rightarrowdiamond",0x0291E}, //  ⤞ # rightwards arrow to black diamond
	{"\\diamondleftarrowbar",0x0291F}, //  ⤟ # leftwards arrow from bar to black diamond
	{"\\rightarrowplus",0x02945}, //  ⥅ # rightwards arrow with plus below
	{"\\leftarrowplus",0x02946}, //  ⥆ # leftwards arrow with plus below
	{"\\leftrightarrowcircle",0x02948}, //  ⥈ # left right arrow through small circle
	{"\\twoheaduparrowcircle",0x02949}, //  ⥉ # upwards two-headed arrow from small circle
	{"\\leftrightharpoonupdown",0x0294A}, //  ⥊ # left barb up right barb down harpoon
	{"\\leftrightharpoondownup",0x0294B}, //  ⥋ # left barb down right barb up harpoon
	{"\\updownharpoonrightleft",0x0294C}, //  ⥌ # up barb right down barb left harpoon
	{"\\updownharpoonleftright",0x0294D}, //  ⥍ # up barb left down barb right harpoon
	{"\\leftharpoonsupdown",0x02962}, //  ⥢ # leftwards harpoon with barb up above leftwards harpoon with barb down
	{"\\upharpoonsleftright",0x02963}, //  ⥣ # upwards harpoon with barb left beside upwards harpoon with barb right
	{"\\rightharpoonsupdown",0x02964}, //  ⥤ # rightwards harpoon with barb up above rightwards harpoon with barb down
	{"\\downharpoonsleftright",0x02965}, //  ⥥ # downwards harpoon with barb left beside downwards harpoon with barb right
	{"\\leftrightharpoonsup",0x02966}, //  ⥦ # leftwards harpoon with barb up above rightwards harpoon with barb up
	{"\\leftrightharpoonsdown",0x02967}, //  ⥧ # leftwards harpoon with barb down above rightwards harpoon with barb down
	{"\\rightleftharpoonsup",0x02968}, //  ⥨ # rightwards harpoon with barb up above leftwards harpoon with barb up
	{"\\rightleftharpoonsdown",0x02969}, //  ⥩ # rightwards harpoon with barb down above leftwards harpoon with barb down
	{"\\leftharpoonupdash",0x0296A}, //  ⥪ # leftwards harpoon with barb up above long dash
	{"\\dashleftharpoondown",0x0296B}, //  ⥫ # leftwards harpoon with barb down below long dash
	{"\\rightharpoonupdash",0x0296C}, //  ⥬ # rightwards harpoon with barb up above long dash
	{"\\dashrightharpoondown",0x0296D}, //  ⥭ # rightwards harpoon with barb down below long dash
	{"\\measuredangleleft",0x0299B}, //  ⦛ # measured angle opening left
	{"\\rightanglemdot",0x0299D}, //  ⦝ # measured right angle with dot
	{"\\angles",0x0299E}, //  ⦞ # angle with s inside
	{"\\angdnr",0x0299F}, //  ⦟ # acute angle
	{"\\sphericalangleup",0x029A1}, //  ⦡ # spherical angle opening up
	{"\\turnangle",0x029A2}, //  ⦢ # turned angle
	{"\\revangle",0x029A3}, //  ⦣ # reversed angle
	{"\\angleubar",0x029A4}, //  ⦤ # angle with underbar
	{"\\revangleubar",0x029A5}, //  ⦥ # reversed angle with underbar
	{"\\wideangledown",0x029A6}, //  ⦦ # oblique angle opening up
	{"\\wideangleup",0x029A7}, //  ⦧ # oblique angle opening down
	{"\\measanglerutone",0x029A8}, //  ⦨ # measured angle with open arm ending in arrow pointing up and right
	{"\\measanglelutonw",0x029A9}, //  ⦩ # measured angle with open arm ending in arrow pointing up and left
	{"\\measanglerdtose",0x029AA}, //  ⦪ # measured angle with open arm ending in arrow pointing down and right
	{"\\measangleldtosw",0x029AB}, //  ⦫ # measured angle with open arm ending in arrow pointing down and left
	{"\\measangleurtone",0x029AC}, //  ⦬ # measured angle with open arm ending in arrow pointing right and up
	{"\\measangleultonw",0x029AD}, //  ⦭ # measured angle with open arm ending in arrow pointing left and up
	{"\\measangledrtose",0x029AE}, //  ⦮ # measured angle with open arm ending in arrow pointing right and down
	{"\\measangledltosw",0x029AF}, //  ⦯ # measured angle with open arm ending in arrow pointing left and down
	{"\\revemptyset",0x029B0}, //  ⦰ # reversed empty set
	{"\\emptysetobar",0x029B1}, //  ⦱ # empty set with overbar
	{"\\emptysetocirc",0x029B2}, //  ⦲ # empty set with small circle above
	{"\\emptysetoarr",0x029B3}, //  ⦳ # empty set with right arrow above
	{"\\emptysetoarrl",0x029B4}, //  ⦴ # empty set with left arrow above
	{"\\circledparallel",0x029B7}, //  ⦷ # circled parallel
	{"\\odotslashdot",0x029BC}, //  ⦼ # circled anticlockwise-rotated division sign
	{"\\circledwhitebullet",0x029BE}, //  ⦾ # circled white bullet
	{"\\circledbullet",0x029BF}, //  ⦿ # circled bullet
	{"\\olessthan",0x029C0}, //  ⧀ # circled less-than
	{"\\ogreaterthan",0x029C1}, //  ⧁ # circled greater-than
	{"\\lrtriangleeq",0x029E1}, //  ⧡ # increases as
	{"\\eparsl",0x029E3}, //  ⧣ # equals sign and slanted parallel
	{"\\smeparsl",0x029E4}, //  ⧤ # equals sign and slanted parallel with tilde above
	{"\\eqvparsl",0x029E5}, //  ⧥ # identical to and slanted parallel
	{"\\dsol",0x029F6}, //  ⧶ # solidus with overbar
	{"\\rsolbar",0x029F7}, //  ⧷ # reverse solidus with horizontal stroke
	{"\\doubleplus",0x029FA}, //  ⧺ # double plus
	{"\\tripleplus",0x029FB}, //  ⧻ # triple plus
	{"\\modtwosum",0x02A0A}, //  ⨊ # modulo two sum
	{"\\cirfnint",0x02A10}, //  ⨐ # circulation function
	{"\\awint",0x02A11}, //  ⨑ # anticlockwise integration
	{"\\rppolint",0x02A12}, //  ⨒ # line integration with rectangular path around pole
	{"\\scpolint",0x02A13}, //  ⨓ # line integration with semicircular path around pole
	{"\\npolint",0x02A14}, //  ⨔ # line integration not including the pole
	{"\\pointint",0x02A15}, //  ⨕ # integral around a point operator
	{"\\ringplus",0x02A22}, //  ⨢ # plus sign with small circle above
	{"\\plushat",0x02A23}, //  ⨣ # plus sign with circumflex accent above
	{"\\simplus",0x02A24}, //  ⨤ # plus sign with tilde above
	{"\\plussim",0x02A26}, //  ⨦ # plus sign with tilde below
	{"\\plussubtwo",0x02A27}, //  ⨧ # plus sign with subscript two
	{"\\plustrif",0x02A28}, //  ⨨ # plus sign with black triangle
	{"\\commaminus",0x02A29}, //  ⨩ # minus sign with comma above
	{"\\opluslhrim",0x02A2D}, //  ⨭ # plus sign in left half circle
	{"\\oplusrhrim",0x02A2E}, //  ⨮ # plus sign in right half circle
	{"\\dottimes",0x02A30}, //  ⨰ # multiplication sign with dot above
	{"\\timesbar",0x02A31}, //  ⨱ # multiplication sign with underbar
	{"\\smashtimes",0x02A33}, //  ⨳ # smash product
	{"\\otimeslhrim",0x02A34}, //  ⨴ # multiplication sign in left half circle
	{"\\otimesrhrim",0x02A35}, //  ⨵ # multiplication sign in right half circle
	{"\\otimeshat",0x02A36}, //  ⨶ # circled multiplication sign with circumflex accent
	{"\\Otimes",0x02A37}, //  ⨷ # multiplication sign in double circle
	{"\\odiv",0x02A38}, //  ⨸ # circled division sign
	{"\\triangleplus",0x02A39}, //  ⨹ # plus sign in triangle
	{"\\triangleminus",0x02A3A}, //  ⨺ # minus sign in triangle
	{"\\triangletimes",0x02A3B}, //  ⨻ # multiplication sign in triangle
	{"\\capdot",0x02A40}, //  ⩀ # intersection with dot
	{"\\uminus",0x02A41}, //  ⩁ # union with minus sign
	{"\\capwedge",0x02A44}, //  ⩄ # intersection with logical and
	{"\\cupvee",0x02A45}, //  ⩅ # union with logical or
	{"\\twocups",0x02A4A}, //  ⩊ # union beside and joined with union
	{"\\twocaps",0x02A4B}, //  ⩋ # intersection beside and joined with intersection
	{"\\closedvarcup",0x02A4C}, //  ⩌ # closed union with serifs
	{"\\closedvarcap",0x02A4D}, //  ⩍ # closed intersection with serifs
	{"\\Sqcap",0x02A4E}, //  ⩎ # double square intersection
	{"\\Sqcup",0x02A4F}, //  ⩏ # double square union
	{"\\closedvarcupsmashprod",0x02A50}, //  ⩐ # closed union with serifs and smash product
	{"\\wedgeodot",0x02A51}, //  ⩑ # logical and with dot above
	{"\\veeodot",0x02A52}, //  ⩒ # logical or with dot above
	{"\\wedgeonwedge",0x02A55}, //  ⩕ # two intersecting logical and
	{"\\bigslopedvee",0x02A57}, //  ⩗ # sloping large or
	{"\\bigslopedwedge",0x02A58}, //  ⩘ # sloping large and
	{"\\wedgemidvert",0x02A5A}, //  ⩚ # logical and with middle stem
	{"\\veemidvert",0x02A5B}, //  ⩛ # logical or with middle stem
	{"\\midbarwedge",0x02A5C}, //  ⩜ # ogical and with horizontal dash
	{"\\midbarvee",0x02A5D}, //  ⩝ # logical or with horizontal dash
	{"\\wedgedoublebar",0x02A60}, //  ⩠ # logical and with double underbar
	{"\\varveebar",0x02A61}, //  ⩡ # small vee with underbar
	{"\\doublebarvee",0x02A62}, //  ⩢ # logical or with double overbar
	{"\\veedoublebar",0x02A63}, //  ⩣ # logical or with double underbar
	{"\\eqdot",0x02A66}, //  ⩦ # equals sign with dot below
	{"\\dotequiv",0x02A67}, //  ⩧ # identical with dot above
	{"\\dotsim",0x02A6A}, //  ⩪ # tilde operator with dot above
	{"\\simrdots",0x02A6B}, //  ⩫ # tilde operator with rising dots
	{"\\simminussim",0x02A6C}, //  ⩬ # similar minus similar
	{"\\congdot",0x02A6D}, //  ⩭ # congruent with dot above
	{"\\asteq",0x02A6E}, //  ⩮ # equals with asterisk
	{"\\hatapprox",0x02A6F}, //  ⩯ # almost equal to with circumflex accent
	{"\\approxeqq",0x02A70}, //  ⩰ # approximately equal or equal to
	{"\\eqqplus",0x02A71}, //  ⩱ # equals sign above plus sign
	{"\\pluseqq",0x02A72}, //  ⩲ # plus sign above equals sign
	{"\\eqqsim",0x02A73}, //  ⩳ # equals sign above tilde operator
	{"\\Coloneq",0x02A74}, //  ⩴ # double colon equal
	{"\\eqeqeq",0x02A76}, //  ⩶ # three consecutive equals signs
	{"\\equivDD",0x02A78}, //  ⩸ # equivalent with four dots above
	{"\\ltcir",0x02A79}, //  ⩹ # less-than with circle inside
	{"\\gtcir",0x02A7A}, //  ⩺ # greater-than with circle inside
	{"\\ltquest",0x02A7B}, //  ⩻ # less-than with question mark above
	{"\\gtquest",0x02A7C}, //  ⩼ # greater-than with question mark above
	{"\\lesdot",0x02A7F}, //  ⩿ # less-than or slanted equal to with dot inside
	{"\\gesdot",0x02A80}, //  ⪀ # greater-than or slanted equal to with dot inside
	{"\\lesdoto",0x02A81}, //  ⪁ # less-than or slanted equal to with dot above
	{"\\gesdoto",0x02A82}, //  ⪂ # greater-than or slanted equal to with dot above
	{"\\lesdotor",0x02A83}, //  ⪃ # less-than or slanted equal to with dot above right
	{"\\gesdotol",0x02A84}, //  ⪄ # greater-than or slanted equal to with dot above left
	{"\\lsime",0x02A8D}, //  ⪍ # less-than above similar or equal
	{"\\gsime",0x02A8E}, //  ⪎ # greater-than above similar or equal
	{"\\lsimg",0x02A8F}, //  ⪏ # less-than above similar above greater-than
	{"\\gsiml",0x02A90}, //  ⪐ # greater-than above similar above less-than
	{"\\lgE",0x02A91}, //  ⪑ # less-than above greater-than above double-line equal
	{"\\glE",0x02A92}, //  ⪒ # greater-than above less-than above double-line equal
	{"\\lesges",0x02A93}, //  ⪓ # less-than above slanted equal above greater-than above slanted equal
	{"\\gesles",0x02A94}, //  ⪔ # greater-than above slanted equal above less-than above slanted equal
	{"\\elsdot",0x02A97}, //  ⪗ # slanted equal to or less-than with dot inside
	{"\\egsdot",0x02A98}, //  ⪘ # slanted equal to or greater-than with dot inside
	{"\\eqqless",0x02A99}, //  ⪙ # double-line equal to or less-than
	{"\\eqqgtr",0x02A9A}, //  ⪚ # double-line equal to or greater-than
	{"\\eqqslantless",0x02A9B}, //  ⪛ # double-line slanted equal to or less-than
	{"\\eqqslantgtr",0x02A9C}, //  ⪜ # double-line slanted equal to or greater-than
	{"\\simless",0x02A9D}, //  ⪝ # similar or less-than
	{"\\simgtr",0x02A9E}, //  ⪞ # similar or greater-than
	{"\\simlE",0x02A9F}, //  ⪟ # similar above less-than above equals sign
	{"\\simgE",0x02AA0}, //  ⪠ # similar above greater-than above equals sign
	{"\\glj",0x02AA4}, //  ⪤ # greater-than overlapping less-than
	{"\\gla",0x02AA5}, //  ⪥ # greater-than beside less-than
	{"\\ltcc",0x02AA6}, //  ⪦ # less-than closed by curve
	{"\\gtcc",0x02AA7}, //  ⪧ # greater-than closed by curve
	{"\\lescc",0x02AA8}, //  ⪨ # less-than closed by curve above slanted equal
	{"\\gescc",0x02AA9}, //  ⪩ # greater-than closed by curve above slanted equal
	{"\\smt",0x02AAA}, //  ⪪ # smaller than
	{"\\lat",0x02AAB}, //  ⪫ # larger than
	{"\\smte",0x02AAC}, //  ⪬ # smaller than or equal to
	{"\\late",0x02AAD}, //  ⪭ # larger than or equal to
	{"\\precneq",0x02AB1}, //  ⪱ # precedes above single-line not equal to
	{"\\succneq",0x02AB2}, //  ⪲ # succeeds above single-line not equal to
	{"\\Prec",0x02ABB}, //  ⪻ # double precedes
	{"\\Succ",0x02ABC}, //  ⪼ # double succeeds
	{"\\subsetdot",0x02ABD}, //  ⪽ # subset with dot
	{"\\supsetdot",0x02ABE}, //  ⪾ # superset with dot
	{"\\subsetplus",0x02ABF}, //  ⪿ # subset with plus sign below
	{"\\supsetplus",0x02AC0}, //  ⫀ # superset with plus sign below
	{"\\submult",0x02AC1}, //  ⫁ # subset with multiplication sign below
	{"\\supmult",0x02AC2}, //  ⫂ # superset with multiplication sign below
	{"\\subedot",0x02AC3}, //  ⫃ # subset of or equal to with dot above
	{"\\supedot",0x02AC4}, //  ⫄ # superset of or equal to with dot above
	{"\\subsim",0x02AC7}, //  ⫇ # subset of above tilde operator
	{"\\supsim",0x02AC8}, //  ⫈ # superset of above tilde operator
	{"\\subsetapprox",0x02AC9}, //  ⫉ # subset of above almost equal to
	{"\\supsetapprox",0x02ACA}, //  ⫊ # superset of above almost equal to
	{"\\lsqhook",0x02ACD}, //  ⫍ # square left open box operator
	{"\\rsqhook",0x02ACE}, //  ⫎ # square right open box operator
	{"\\csub",0x02ACF}, //  ⫏ # closed subset
	{"\\csup",0x02AD0}, //  ⫐ # closed superset
	{"\\csube",0x02AD1}, //  ⫑ # closed subset or equal to
	{"\\csupe",0x02AD2}, //  ⫒ # closed superset or equal to
	{"\\subsup",0x02AD3}, //  ⫓ # subset above superset
	{"\\supsub",0x02AD4}, //  ⫔ # superset above subset
	{"\\subsub",0x02AD5}, //  ⫕ # subset above subset
	{"\\supsup",0x02AD6}, //  ⫖ # superset above superset
	{"\\suphsub",0x02AD7}, //  ⫗ # superset beside subset
	{"\\supdsub",0x02AD8}, //  ⫘ # superset beside and joined by dash with subset
	{"\\forkv",0x02AD9}, //  ⫙ # element of opening downwards
	{"\\lllnest",0x02AF7}, //  ⫷ # stacked very much less-than
	{"\\gggnest",0x02AF8}, //  ⫸ # stacked very much greater-than
	{"\\leqqslant",0x02AF9}, //  ⫹ # double-line slanted less-than or equal to
	{"\\geqqslant",0x02AFA}, //  ⫺ # double-line slanted greater-than or equal to
	{"\\squaretopblack",0x2B12}, //  0x2B12 # square with top half black
	{"\\squarebotblack",0x2B13}, //  0x2B13 # square with bottom half black
	{"\\squareurblack",0x2B14}, //  0x2B14 # square with upper right diagonal half black
	{"\\squarellblack",0x2B15}, //  0x2B15 # square with lower left diagonal half black
	{"\\diamondleftblack",0x2B16}, //  0x2B16 # diamond with left half black
	{"\\diamondrightblack",0x2B17}, //  0x2B17 # diamond with right half black
	{"\\diamondtopblack",0x2B18}, //  0x2B18 # diamond with top half black
	{"\\diamondbotblack",0x2B19}, //  0x2B19 # diamond with bottom half black
	{"\\dottedsquare",0x2B1A}, //  0x2B1A # dotted square
	{"\\lgblksquare",0x2B1B}, //  0x2B1B # black large square
	{"\\lgwhtsquare",0x2B1C}, //  0x2B1C # white large square
	{"\\vysmblksquare",0x2B1D}, //  0x2B1D # black very small square
	{"\\vysmwhtsquare",0x2B1E}, //  0x2B1E # white very small square
	{"\\pentagonblack",0x2B1F}, //  0x2B1F # black pentagon
	{"\\pentagon",0x2B20}, //  0x2B20 # white pentagon
	{"\\varhexagon",0x2B21}, //  0x2B21 # white hexagon
	{"\\varhexagonblack",0x2B22}, //  0x2B22 # black hexagon
	{"\\hexagonblack",0x2B23}, //  0x2B23 # horizontal black hexagon
	{"\\lgblkcircle",0x2B24}, //  0x2B24 # black large circle
	{"\\mdblkdiamond",0x2B25}, //  0x2B25 # black medium diamond
	{"\\mdwhtdiamond",0x2B26}, //  0x2B26 # white medium diamond
	{"\\mdblklozenge",0x2B27}, //  0x2B27 # black medium lozenge
	{"\\mdwhtlozenge",0x2B28}, //  0x2B28 # white medium lozenge
	{"\\smblkdiamond",0x2B29}, //  0x2B29 # black small diamond
	{"\\smblklozenge",0x2B2A}, //  0x2B2A # black small lozenge
	{"\\smwhtlozenge",0x2B2B}, //  0x2B2B # white small lozenge
	{"\\blkhorzoval",0x2B2C}, //  0x2B2C # black horizontal ellipse
	{"\\whthorzoval",0x2B2D}, //  0x2B2D # white horizontal ellipse
	{"\\blkvertoval",0x2B2E}, //  0x2B2E # black vertical ellipse
	{"\\whtvertoval",0x2B2F}, //  0x2B2F # white vertical ellipse
	{"\\circleonleftarrow",0x2B30}, //  0x2B30 # left arrow with small circle
	{"\\leftthreearrows",0x2B31}, //  0x2B31 # three leftwards arrows
	{"\\leftarrowonoplus",0x2B32}, //  0x2B32 # left arrow with circled plus
	{"\\longleftsquigarrow",0x2B33}, //  0x2B33 # long leftwards squiggle arrow
	{"\\nvtwoheadleftarrow",0x2B34}, //  0x2B34 # leftwards two-headed arrow with vertical stroke
	{"\\nVtwoheadleftarrow",0x2B35}, //  0x2B35 # leftwards two-headed arrow with double vertical stroke
	{"\\twoheadmapsfrom",0x2B36}, //  0x2B36 # leftwards two-headed arrow from bar
	{"\\twoheadleftdbkarrow",0x2B37}, //  0x2B37 # leftwards two-headed triple-dash arrow
	{"\\leftdotarrow",0x2B38}, //  0x2B38 # leftwards arrow with dotted stem
	{"\\nvleftarrowtail",0x2B39}, //  0x2B39 # leftwards arrow with tail with vertical stroke
	{"\\nVleftarrowtail",0x2B3A}, //  0x2B3A # leftwards arrow with tail with double vertical stroke
	{"\\twoheadleftarrowtail",0x2B3B}, //  0x2B3B # leftwards two-headed arrow with tail
	{"\\nvtwoheadleftarrowtail",0x2B3C}, //  0x2B3C # leftwards two-headed arrow with tail with vertical stroke
	{"\\nVtwoheadleftarrowtail",0x2B3D}, //  0x2B3D # leftwards two-headed arrow with tail with double vertical stroke
	{"\\leftarrowx",0x2B3E}, //  0x2B3E # leftwards arrow through x
	{"\\leftcurvedarrow",0x2B3F}, //  0x2B3F # wave arrow pointing directly left
	{"\\equalleftarrow",0x2B40}, //  0x2B40 # equals sign above leftwards arrow
	{"\\bsimilarleftarrow",0x2B41}, //  0x2B41 # reverse tilde operator above leftwards arrow
	{"\\leftarrowbackapprox",0x2B42}, //  0x2B42 # leftwards arrow above reverse almost equal to
	{"\\rightarrowgtr",0x2B43}, //  0x2B43 # rightwards arrow through greater-than
	{"\\leftarrowless",0x2977}, //  0x2977 # leftwards arrow through less-than
	{"\\rightarrowsupset",0x2B44}, //  0x2B44 # rightwards arrow through superset
	{"\\leftarrowsubset",0x297A}, //  0x297A # leftwards arrow through subset
	{"\\LLeftarrow",0x2B45}, //  0x2B45 # leftwards quadruple arrow
	{"\\RRightarrow",0x2B46}, //  0x2B46 # rightwards quadruple arrow
	{"\\bsimilarrightarrow",0x2B47}, //  0x2B47 # reverse tilde operator above rightwards arrow
	{"\\rightarrowbackapprox",0x2B48}, //  0x2B48 # rightwards arrow above reverse almost equal to
	{"\\similarleftarrow",0x2B49}, //  0x2B49 # tilde operator above leftwards arrow
	{"\\leftarrowapprox",0x2B4A}, //  0x2B4A # leftwards arrow above almost equal to
	{"\\leftarrowbsimilar",0x2B4B}, //  0x2B4B # leftwards arrow above reverse tilde operator
	{"\\rightarrowbsimilar",0x2B4C}, //  0x2B4C # righttwards arrow above reverse tilde operator
	{"\\medwhitestar",0x2B50}, //  0x2B50 # white medium star
	{"\\medblackstar",0x2B51}, //  0x2B51 # black medium star
	{"\\smwhitestar",0x2B52}, //  0x2B52 # white small star
	{"\\rightpentagonblack",0x2B53}, //  0x2B53 # black right-pointing pentagon
	{"\\rightpentagon",0x2B54}, //  0x2B54 # white right-pointing pentagon
	{"\\postalmark",0x03012}, //  〒 # postal mark
	{"\\triangleright",0x025B7}, //  ▷ # (large) right triangle, open; z notation range restriction
	{"\\triangleleft",0x025C1}, //  ◁ # (large) left triangle, open; z notation domain restriction
	{"\\leftouterjoin",0x027D5}, //  ⟕ # left outer join
	{"\\rightouterjoin",0x027D6}, //  ⟖ # right outer join
	{"\\fullouterjoin",0x027D7}, //  ⟗ # full outer join
	{"\\join",0x02A1D}, //  ⨝ # join
	{"\\leftwavearrow",0x0219C}, //  ↜ # left arrow-wavy
	{"\\rightwavearrow",0x0219D}, //  ↝ # right arrow-wavy
	{"\\varbarwedge",0x02305}, //  ⌅ # /barwedge b: logical and, bar above [projective (bar over small wedge)]
	{"\\smallblacktriangleright",0x025B8}, //  ▸ # right triangle, filled
	{"\\smallblacktriangleleft",0x025C2}, //  ◂ # left triangle, filled
	{"\\tricolon",0x0205D}, //  ⁝ # tricolon
	
	
	// anotherlist: https://github.com/latex3/unicode-math/blob/master/unicode-math-table.tex
	// I deduplicated things and removed blocks with combining diacritical marks and commands
	// (e.g. \sum)
	{"\\mathexclam",	0x00021}, //exclamation mark%
	{"\\mathoctothorpe",	0x00023}, //number sign%
	{"\\mathdollar",	0x00024}, //dollar sign%
	{"\\mathpercent",	0x00025}, //percent sign%
	{"\\mathampersand",	0x00026}, //ampersand%
	{"\\lparen",	0x00028}, //left parenthesis%
	{"\\rparen",	0x00029}, //right parenthesis%
	{"\\mathplus",	0x0002B}, //plus sign b:%
	{"\\mathcomma",	0x0002C}, //comma%
	{"\\mathperiod",	0x0002E}, //full stop, period%
	{"\\mathslash",	0x0002F}, //solidus%
	{"\\mathcolon",	0x0003A}, //colon%
	{"\\mathsemicolon",	0x0003B}, //semicolon p:%
	{"\\less",	0x0003C}, //less-than sign r:%
	{"\\equal",	0x0003D}, //equals sign r:%
	{"\\greater",	0x0003E}, //greater-than sign r:%
	{"\\mathquestion",	0x0003F}, //question mark%
	{"\\mathatsign",	0x00040}, //commercial at%
	{"\\lbrack",	0x0005B}, //left square bracket%
	{"\\rbrack",	0x0005D}, //right square bracket%
	{"\\lbrace",	0x0007B}, //left curly bracket%
	{"\\rbrace",	0x0007D}, //right curly bracket%
	{"\\mathsterling",	0x000A3}, //pound sign%
	{"\\mathyen",	0x000A5}, //yen sign%
	{"\\mathsection",	0x000A7}, //section symbol%
	{"\\mathparagraph",	0x000B6}, //paragraph symbol%
	{"\\matheth",	0x000F0}, //eth%
	{"\\mupAlpha",	0x00391}, //capital alpha, greek%
	{"\\mupBeta",	0x00392}, //capital beta, greek%
	{"\\mupGamma",	0x00393}, //capital gamma, greek%
	{"\\mupDelta",	0x00394}, //capital delta, greek%
	{"\\mupEpsilon",	0x00395}, //capital epsilon, greek%
	{"\\mupZeta",	0x00396}, //capital zeta, greek%
	{"\\mupEta",	0x00397}, //capital eta, greek%
	{"\\mupTheta",	0x00398}, //capital theta, greek%
	{"\\mupIota",	0x00399}, //capital iota, greek%
	{"\\mupKappa",	0x0039A}, //capital kappa, greek%
	{"\\mupLambda",	0x0039B}, //capital lambda, greek%
	{"\\mupMu",	0x0039C}, //capital mu, greek%
	{"\\mupNu",	0x0039D}, //capital nu, greek%
	{"\\mupXi",	0x0039E}, //capital xi, greek%
	{"\\mupOmicron",	0x0039F}, //capital omicron, greek%
	{"\\mupPi",	0x003A0}, //capital pi, greek%
	{"\\mupRho",	0x003A1}, //capital rho, greek%
	{"\\mupSigma",	0x003A3}, //capital sigma, greek%
	{"\\mupTau",	0x003A4}, //capital tau, greek%
	{"\\mupUpsilon",	0x003A5}, //capital upsilon, greek%
	{"\\mupPhi",	0x003A6}, //capital phi, greek%
	{"\\mupChi",	0x003A7}, //capital chi, greek%
	{"\\mupPsi",	0x003A8}, //capital psi, greek%
	{"\\mupOmega",	0x003A9}, //capital omega, greek%
	{"\\mupalpha",	0x003B1}, //small alpha, greek%
	{"\\mupbeta",	0x003B2}, //small beta, greek%
	{"\\mupgamma",	0x003B3}, //small gamma, greek%
	{"\\mupdelta",	0x003B4}, //small delta, greek%
	{"\\mupvarepsilon",	0x003B5}, //rounded small varepsilon, greek%
	{"\\mupzeta",	0x003B6}, //small zeta, greek%
	{"\\mupeta",	0x003B7}, //small eta, greek%
	{"\\muptheta",	0x003B8}, //straight theta, small theta, greek%
	{"\\mupiota",	0x003B9}, //small iota, greek%
	{"\\mupkappa",	0x003BA}, //small kappa, greek%
	{"\\muplambda",	0x003BB}, //small lambda, greek%
	{"\\mupmu",	0x003BC}, //small mu, greek%
	{"\\mupnu",	0x003BD}, //small nu, greek%
	{"\\mupxi",	0x003BE}, //small xi, greek%
	{"\\mupomicron",	0x003BF}, //small omicron, greek%
	{"\\muppi",	0x003C0}, //small pi, greek%
	{"\\muprho",	0x003C1}, //small rho, greek%
	{"\\mupvarsigma",	0x003C2}, //terminal sigma, greek%
	{"\\mupsigma",	0x003C3}, //small sigma, greek%
	{"\\muptau",	0x003C4}, //small tau, greek%
	{"\\mupupsilon",	0x003C5}, //small upsilon, greek%
	{"\\mupvarphi",	0x003C6}, //curly or open small phi, greek%
	{"\\mupchi",	0x003C7}, //small chi, greek%
	{"\\muppsi",	0x003C8}, //small psi, greek%
	{"\\mupomega",	0x003C9}, //small omega, greek%
	{"\\mupvartheta",	0x003D1}, // /vartheta - curly or open theta%
	{"\\mupphi",	0x003D5}, // /straightphi - small phi, greek%
	{"\\mupvarpi",	0x003D6}, //rounded small pi (pomega), greek%
	{"\\upDigamma",	0x003DC}, //capital digamma%
	{"\\updigamma",	0x003DD}, //old greek small letter digamma%
	{"\\mupvarkappa",	0x003F0}, //rounded small kappa, greek%
	{"\\mupvarrho",	0x003F1}, //rounded small rho, greek%
	{"\\mupvarTheta",	0x003F4}, //greek capital theta symbol%
	{"\\mupepsilon",	0x003F5}, //greek lunate varepsilon symbol%
	{"\\upbackepsilon",	0x003F6}, //greek reversed lunate epsilon symbol%
	{"\\mathhyphen",	0x02010}, //hyphen%
	{"\\horizbar",	0x02015}, //horizontal bar%
	{"\\twolowline",	0x02017}, //double low line (spacing)%
	{"\\smblkcircle",	0x02022}, // /bullet b: round bullet, filled%
	{"\\enleadertwodots",	0x02025}, //double baseline dot (en leader)%
	{"\\unicodeellipsis",	0x02026}, //ellipsis (horizontal)%
	{"\\dprime",	0x02033}, //double prime or second, not superscripted%
	{"\\trprime",	0x02034}, //triple prime (not superscripted)%
	{"\\backdprime",	0x02036}, //double reverse prime, not superscripted%
	{"\\backtrprime",	0x02037}, //triple reverse prime, not superscripted%
	{"\\caretinsert",	0x02038}, //caret (insertion mark)%
	{"\\Exclam",	0x0203C}, //double exclamation mark%
	{"\\hyphenbullet",	0x02043}, //rectangle, filled (hyphen bullet)%
	{"\\fracslash",	0x02044}, //fraction slash%
	{"\\Question",	0x02047}, //double question mark%
	{"\\closure",	0x02050}, //close up%
	{"\\qprime",	0x02057}, //quadruple prime, not superscripted%
	{"\\BbbC",	0x02102}, // /bbb c, open face c%
	{"\\Eulerconst",	0x02107}, //euler constant%
	{"\\mscrg",	0x0210A}, // /scr g, script letter g%
	{"\\mscrH",	0x0210B}, //hamiltonian (script capital h)%
	{"\\mfrakH",	0x0210C}, // /frak h, upper case h%
	{"\\BbbH",	0x0210D}, // /bbb h, open face h%
	{"\\Planckconst",	0x0210E}, //planck constant%
	{"\\mscrI",	0x02110}, // /scr i, script letter i%
	{"\\mscrL",	0x02112}, //lagrangian (script capital l)%
	{"\\BbbN",	0x02115}, // /bbb n, open face n%
	{"\\BbbP",	0x02119}, // /bbb p, open face p%
	{"\\BbbQ",	0x0211A}, // /bbb q, open face q%
	{"\\mscrR",	0x0211B}, // /scr r, script letter r%
	{"\\BbbR",	0x0211D}, // /bbb r, open face r%
	{"\\BbbZ",	0x02124}, // /bbb z, open face z%
	{"\\mfrakZ",	0x02128}, // /frak z, upper case z%
	{"\\mscrB",	0x0212C}, //bernoulli function (script capital b)%
	{"\\mfrakC",	0x0212D}, //black-letter capital c%
	{"\\mscre",	0x0212F}, // /scr e, script letter e%
	{"\\mscrE",	0x02130}, // /scr e, script letter e%
	{"\\mscrF",	0x02131}, // /scr f, script letter f%
	{"\\mscrM",	0x02133}, //physics m-matrix (script capital m)%
	{"\\mscro",	0x02134}, //order of (script small o)%
	{"\\Bbbpi",	0x0213C}, //double-struck small pi%
	{"\\Bbbgamma",	0x0213D}, //double-struck small gamma%
	{"\\BbbGamma",	0x0213E}, //double-struck capital gamma%
	{"\\BbbPi",	0x0213F}, //double-struck capital pi%
	{"\\Bbbsum",	0x02140}, //double-struck n-ary summation%
	{"\\mitBbbD",	0x02145}, //double-struck italic capital d%
	{"\\mitBbbd",	0x02146}, //double-struck italic small d%
	{"\\mitBbbe",	0x02147}, //double-struck italic small e%
	{"\\mitBbbi",	0x02148}, //double-struck italic small i%
	{"\\mitBbbj",	0x02149}, //double-struck italic small j%
	{"\\barovernorthwestarrow",	0x021B8}, //north west arrow to long bar%
	{"\\barleftarrowrightarrowbar",	0x021B9}, //leftwards arrow to bar over rightwards arrow to bar%
	{"\\acwopencirclearrow",	0x021BA}, //anticlockwise open circle arrow%
	{"\\cwopencirclearrow",	0x021BB}, //clockwise open circle arrow%
	{"\\barleftarrow",	0x021E4}, //leftwards arrow to bar%
	{"\\minus",	0x02212}, //minus sign%
	{"\\divslash",	0x02215}, //division slash%
	{"\\vysmwhtcircle",	0x02218}, //composite function (small circle)%
	{"\\cuberoot",	0x0221B}, //cube root%
	{"\\intclockwise",	0x02231}, //clockwise integral%
	{"\\mathratio",	0x02236}, //ratio%
	{"\\dashcolon",	0x02239}, //excess (-:)%
	{"\\invlazys",	0x0223E}, //most positive [inverted lazy s]%
	{"\\sinewave",	0x0223F}, //sine wave%
	{"\\sime",	0x02243}, //similar, equals (alias)%
	{"\\simneqq",	0x02246}, //similar, not equals [vert only for 9573 entity]%
	{"\\approxident",	0x0224B}, //approximately identical to%
	{"\\backcong",	0x0224C}, //all equal to%
	{"\\stareq",	0x0225B}, //star equals%
	{"\\nlessgtr",	0x02278}, //not less, greater%
	{"\\ngtrless",	0x02279}, //not greater, less%
	{"\\cupleftarrow",	0x0228C}, //multiset%
	{"\\assert",	0x022A6}, //assertion (vertical, short dash)%
	{"\\origof",	0x022B6}, //original of%
	{"\\imageof",	0x022B7}, //image of%
	{"\\hermitmatrix",	0x022B9}, //hermitian conjugate matrix%
	{"\\barvee",	0x022BD}, //bar, vee (large vee)%
	{"\\measuredrightangle",	0x022BE}, //right angle-measured [with arc]%
	{"\\smwhtdiamond",	0x022C4}, //white diamond%
	{"\\nvartriangleleft",	0x022EA}, //not left triangle%
	{"\\nvartriangleright",	0x022EB}, //not right triangle%
	{"\\unicodecdots",	0x022EF}, //three dots, centered%
	{"\\inttop",	0x02320}, //top half integral%
	{"\\intbottom",	0x02321}, //bottom half integral%
	{"\\APLnotslash",	0x0233F}, //solidus, bar through (apl functional symbol slash bar)%
	{"\\APLnotbackslash",	0x02340}, //apl functional symbol backslash bar%
	{"\\APLboxupcaret",	0x02353}, //boxed up caret%
	{"\\APLboxquestion",	0x02370}, //boxed question mark%
	{"\\rangledownzigzagarrow",	0x0237C}, //right angle with downwards zigzag arrow%
	{"\\lparenuend",	0x0239B}, //left parenthesis upper hook%
	{"\\lparenextender",	0x0239C}, //left parenthesis extension%
	{"\\lparenlend",	0x0239D}, //left parenthesis lower hook%
	{"\\rparenuend",	0x0239E}, //right parenthesis upper hook%
	{"\\rparenextender",	0x0239F}, //right parenthesis extension%
	{"\\rparenlend",	0x023A0}, //right parenthesis lower hook%
	{"\\lbrackuend",	0x023A1}, //left square bracket upper corner%
	{"\\lbrackextender",	0x023A2}, //left square bracket extension%
	{"\\lbracklend",	0x023A3}, //left square bracket lower corner%
	{"\\rbrackuend",	0x023A4}, //right square bracket upper corner%
	{"\\rbrackextender",	0x023A5}, //right square bracket extension%
	{"\\rbracklend",	0x023A6}, //right square bracket lower corner%
	{"\\lbraceuend",	0x023A7}, //left curly bracket upper hook%
	{"\\lbracemid",	0x023A8}, //left curly bracket middle piece%
	{"\\lbracelend",	0x023A9}, //left curly bracket lower hook%
	{"\\vbraceextender",	0x023AA}, //curly bracket extension%
	{"\\rbraceuend",	0x023AB}, //right curly bracket upper hook%
	{"\\rbracemid",	0x023AC}, //right curly bracket middle piece%
	{"\\rbracelend",	0x023AD}, //right curly bracket lower hook%
	{"\\intextender",	0x023AE}, //integral extension%
	{"\\harrowextender",	0x023AF}, //horizontal line extension (used to extend arrows)%
	{"\\rmoustache",	0x023B1}, //upper right or lower left curly bracket section%
	{"\\sumtop",	0x023B2}, //summation top%
	{"\\sumbottom",	0x023B3}, //summation bottom%
	{"\\overbracket",	0x023B4}, //top square bracket%
	{"\\sqrtbottom",	0x023B7}, //radical symbol bottom%
	{"\\obrbrak",	0x023E0}, //top tortoise shell bracket (mathematical use)%
	{"\\ubrbrak",	0x023E1}, //bottom tortoise shell bracket (mathematical use)%
	{"\\mathvisiblespace",	0x02423}, //open box%
	{"\\bdtriplevdash",	0x02506}, //doubly broken vert%
	{"\\mdlgblksquare",	0x025A0}, //square, filled%
	{"\\mdlgwhtsquare",	0x025A1}, //square, open%
	{"\\vrectangle",	0x025AF}, //rectangle, white (vertical)%
	{"\\mdlgwhtlozenge",	0x025CA}, //lozenge or total mark%
	{"\\mdlgwhtcircle",	0x025CB}, //medium large circle%
	{"\\circlelefthalfblack",	0x025D0}, //circle, filled left half [harvey ball]%
	{"\\circlerighthalfblack",	0x025D1}, //circle, filled right half%
	{"\\circlebottomhalfblack",	0x025D2}, //circle, filled bottom half%
	{"\\inversebullet",	0x025D8}, //inverse bullet %
	{"\\squareleftblack",	0x025E7}, //square, filled left half%
	{"\\squarerightblack",	0x025E8}, //square, filled right half%
	{"\\squarelrblack",	0x025EA}, //square, filled bottom right corner%
	{"\\Hermaphrodite",	0x026A5}, //male and female sign%
	{"\\lbrbrak",	0x02772}, //light left tortoise shell bracket ornament%
	{"\\rbrbrak",	0x02773}, //light right tortoise shell bracket ornament%
	{"\\subsetcirc",	0x027C3}, //open subset%
	{"\\supsetcirc",	0x027C4}, //open superset%
	{"\\lbag",	0x027C5}, //left s-shaped bag delimiter%
	{"\\rbag",	0x027C6}, //right s-shaped bag delimiter%
	{"\\longdivision",	0x027CC}, //long division%
	{"\\diamondcdot",	0x027D0}, //white diamond with centred dot%
	{"\\pullback",	0x027D3}, //lower right corner with dot%
	{"\\pushout",	0x027D4}, //upper left corner with dot%
	{"\\DashVDash",	0x027DA}, //left and right double turnstile%
	{"\\dashVdash",	0x027DB}, //left and right tack%
	{"\\vlongdash",	0x027DD}, //long left tack%
	{"\\longdashv",	0x027DE}, //long right tack%
	{"\\cirbot",	0x027DF}, //up tack with circle above%
	{"\\lozengeminus",	0x027E0}, //lozenge divided by horizontal rule%
	{"\\concavediamond",	0x027E1}, //white concave-sided diamond%
	{"\\concavediamondtickleft",	0x027E2}, //white concave-sided diamond with leftwards tick%
	{"\\concavediamondtickright",	0x027E3}, //white concave-sided diamond with rightwards tick%
	{"\\whitesquaretickleft",	0x027E4}, //white square with leftwards tick%
	{"\\whitesquaretickright",	0x027E5}, //white square with rightwards tick%
	{"\\lBrack",	0x027E6}, //mathematical left white square bracket%
	{"\\rBrack",	0x027E7}, //mathematical right white square bracket%
	{"\\lAngle",	0x027EA}, //mathematical left double angle bracket%
	{"\\rAngle",	0x027EB}, //mathematical right double angle bracket%
	{"\\Lbrbrak",	0x027EC}, //mathematical left white tortoise shell bracket%
	{"\\Rbrbrak",	0x027ED}, //mathematical right white tortoise shell bracket%
	{"\\acwgapcirclearrow",	0x027F2}, //anticlockwise gapped circle arrow%
	{"\\cwgapcirclearrow",	0x027F3}, //clockwise gapped circle arrow%
	{"\\rightarrowonoplus",	0x027F4}, //right arrow with circled plus%
	{"\\rightbkarrow",	0x0290D}, //rightwards double dash arrow%
	{"\\dbkarrow",	0x0290F}, //rightwards triple dash arrow%
	{"\\baruparrow",	0x02912}, //upwards arrow to bar%
	{"\\downarrowbar",	0x02913}, //downwards arrow to bar%
	{"\\lefttail",	0x02919}, //leftwards arrow-tail%
	{"\\righttail",	0x0291A}, //rightwards arrow-tail%
	{"\\leftdbltail",	0x0291B}, //leftwards double arrow-tail%
	{"\\rightdbltail",	0x0291C}, //rightwards double arrow-tail%
	{"\\barrightarrowdiamond",	0x02920}, //rightwards arrow from bar to black diamond%
	{"\\nwsearrow",	0x02921}, //north west and south east arrow%
	{"\\neswarrow",	0x02922}, //north east and south west arrow%
	{"\\hknwarrow",	0x02923}, //north west arrow with hook%
	{"\\hknearrow",	0x02924}, //north east arrow with hook%
	{"\\hksearrow",	0x02925}, //south east arrow with hook%
	{"\\hkswarrow",	0x02926}, //south west arrow with hook%
	{"\\rightcurvedarrow",	0x02933}, //wave arrow pointing directly right%
	{"\\uprightcurvearrow",	0x02934}, //arrow pointing rightwards then curving upwards%
	{"\\downrightcurvedarrow",	0x02935}, //arrow pointing rightwards then curving downwards%
	{"\\leftdowncurvedarrow",	0x02936}, //arrow pointing downwards then curving leftwards%
	{"\\rightdowncurvedarrow",	0x02937}, //arrow pointing downwards then curving rightwards%
	{"\\cwrightarcarrow",	0x02938}, //right-side arc clockwise arrow%
	{"\\acwleftarcarrow",	0x02939}, //left-side arc anticlockwise arrow%
	{"\\acwoverarcarrow",	0x0293A}, //top arc anticlockwise arrow%
	{"\\acwunderarcarrow",	0x0293B}, //bottom arc anticlockwise arrow%
	{"\\curvearrowrightminus",	0x0293C}, //top arc clockwise arrow with minus%
	{"\\curvearrowleftplus",	0x0293D}, //top arc anticlockwise arrow with plus%
	{"\\cwundercurvearrow",	0x0293E}, //lower right semicircular clockwise arrow%
	{"\\ccwundercurvearrow",	0x0293F}, //lower left semicircular anticlockwise arrow%
	{"\\acwcirclearrow",	0x02940}, //anticlockwise closed circle arrow%
	{"\\cwcirclearrow",	0x02941}, //clockwise closed circle arrow%
	{"\\rightarrowshortleftarrow",	0x02942}, //rightwards arrow above short leftwards arrow%
	{"\\leftarrowshortrightarrow",	0x02943}, //leftwards arrow above short rightwards arrow%
	{"\\shortrightarrowleftarrow",	0x02944}, //short rightwards arrow above leftwards arrow%
	{"\\rightarrowx",	0x02947}, //rightwards arrow through x%
	{"\\leftrightharpoonupup",	0x0294E}, //left barb up right barb up harpoon%
	{"\\updownharpoonrightright",	0x0294F}, //up barb right down barb right harpoon%
	{"\\leftrightharpoondowndown",	0x02950}, //left barb down right barb down harpoon%
	{"\\updownharpoonleftleft",	0x02951}, //up barb left down barb left harpoon%
	{"\\barleftharpoonup",	0x02952}, //leftwards harpoon with barb up to bar%
	{"\\rightharpoonupbar",	0x02953}, //rightwards harpoon with barb up to bar%
	{"\\barupharpoonright",	0x02954}, //upwards harpoon with barb right to bar%
	{"\\downharpoonrightbar",	0x02955}, //downwards harpoon with barb right to bar%
	{"\\barleftharpoondown",	0x02956}, //leftwards harpoon with barb down to bar%
	{"\\rightharpoondownbar",	0x02957}, //rightwards harpoon with barb down to bar%
	{"\\barupharpoonleft",	0x02958}, //upwards harpoon with barb left to bar%
	{"\\downharpoonleftbar",	0x02959}, //downwards harpoon with barb left to bar%
	{"\\leftharpoonupbar",	0x0295A}, //leftwards harpoon with barb up from bar%
	{"\\barrightharpoonup",	0x0295B}, //rightwards harpoon with barb up from bar%
	{"\\upharpoonrightbar",	0x0295C}, //upwards harpoon with barb right from bar%
	{"\\bardownharpoonright",	0x0295D}, //downwards harpoon with barb right from bar%
	{"\\leftharpoondownbar",	0x0295E}, //leftwards harpoon with barb down from bar%
	{"\\barrightharpoondown",	0x0295F}, //rightwards harpoon with barb down from bar%
	{"\\upharpoonleftbar",	0x02960}, //upwards harpoon with barb left from bar%
	{"\\bardownharpoonleft",	0x02961}, //downwards harpoon with barb left from bar%
	{"\\updownharpoonsleftright",	0x0296E}, //upwards harpoon with barb left beside downwards harpoon with barb right%
	{"\\downupharpoonsleftright",	0x0296F}, //downwards harpoon with barb left beside upwards harpoon with barb right%
	{"\\rightimply",	0x02970}, //right double arrow with rounded head%
	{"\\equalrightarrow",	0x02971}, //equals sign above rightwards arrow%
	{"\\similarrightarrow",	0x02972}, //tilde operator above rightwards arrow%
	{"\\leftarrowsimilar",	0x02973}, //leftwards arrow above tilde operator%
	{"\\rightarrowsimilar",	0x02974}, //rightwards arrow above tilde operator%
	{"\\rightarrowapprox",	0x02975}, //rightwards arrow above almost equal to%
	{"\\ltlarr",	0x02976}, //less-than above leftwards arrow%
	{"\\gtrarr",	0x02978}, //greater-than above rightwards arrow%
	{"\\subrarr",	0x02979}, //subset above rightwards arrow%
	{"\\suplarr",	0x0297B}, //superset above leftwards arrow%
	{"\\leftfishtail",	0x0297C}, //left fish tail%
	{"\\rightfishtail",	0x0297D}, //right fish tail%
	{"\\upfishtail",	0x0297E}, //up fish tail%
	{"\\downfishtail",	0x0297F}, //down fish tail%
	{"\\mdsmblkcircle",	0x02981}, //z notation spot%
	{"\\typecolon",	0x02982}, //z notation type colon%
	{"\\lBrace",	0x02983}, //left white curly bracket%
	{"\\rBrace",	0x02984}, //right white curly bracket%
	{"\\lParen",	0x02985}, //left white parenthesis%
	{"\\rParen",	0x02986}, //right white parenthesis%
	{"\\llparenthesis",	0x02987}, //z notation left image bracket%
	{"\\rrparenthesis",	0x02988}, //z notation right image bracket%
	{"\\llangle",	0x02989}, //z notation left binding bracket%
	{"\\rrangle",	0x0298A}, //z notation right binding bracket%
	{"\\lbrackubar",	0x0298B}, //left square bracket with underbar%
	{"\\rbrackubar",	0x0298C}, //right square bracket with underbar%
	{"\\lbrackultick",	0x0298D}, //left square bracket with tick in top corner%
	{"\\rbracklrtick",	0x0298E}, //right square bracket with tick in bottom corner%
	{"\\lbracklltick",	0x0298F}, //left square bracket with tick in bottom corner%
	{"\\rbrackurtick",	0x02990}, //right square bracket with tick in top corner%
	{"\\langledot",	0x02991}, //left angle bracket with dot%
	{"\\rangledot",	0x02992}, //right angle bracket with dot%
	{"\\lparenless",	0x02993}, //left arc less-than bracket%
	{"\\rparengtr",	0x02994}, //right arc greater-than bracket%
	{"\\Lparengtr",	0x02995}, //double left arc greater-than bracket%
	{"\\Rparenless",	0x02996}, //double right arc less-than bracket%
	{"\\lblkbrbrak",	0x02997}, //left black tortoise shell bracket%
	{"\\rblkbrbrak",	0x02998}, //right black tortoise shell bracket%
	{"\\fourvdots",	0x02999}, //dotted fence%
	{"\\vzigzag",	0x0299A}, //vertical zigzag line%
	{"\\rightanglesqr",	0x0299C}, //right angle variant with square%
	{"\\gtlpar",	0x029A0}, //spherical angle opening left%
	{"\\circlehbar",	0x029B5}, //circle with horizontal bar%
	{"\\circledvert",	0x029B6}, //circled vertical bar%
	{"\\operp",	0x029B9}, //circled perpendicular%
	{"\\obot",	0x029BA}, //circle divided by horizontal bar and top half divided by vertical bar%
	{"\\olcross",	0x029BB}, //circle with superimposed x%
	{"\\uparrowoncircle",	0x029BD}, //up arrow through circle%
	{"\\cirscir",	0x029C2}, //circle with small circle to the right%
	{"\\cirE",	0x029C3}, //circle with two horizontal strokes to the right%
	{"\\boxdiag",	0x029C4}, //squared rising diagonal slash%
	{"\\boxonbox",	0x029C9}, //two joined squares%
	{"\\triangleodot",	0x029CA}, //triangle with dot above%
	{"\\triangleubar",	0x029CB}, //triangle with underbar%
	{"\\triangles",	0x029CC}, //s in triangle%
	{"\\triangleserifs",	0x029CD}, //triangle with serifs at bottom%
	{"\\rtriltri",	0x029CE}, //right triangle above left triangle%
	{"\\ltrivb",	0x029CF}, //left triangle beside vertical bar%
	{"\\vbrtri",	0x029D0}, //vertical bar beside right triangle%
	{"\\lfbowtie",	0x029D1}, //left black bowtie%
	{"\\rfbowtie",	0x029D2}, //right black bowtie%
	{"\\fbowtie",	0x029D3}, //black bowtie%
	{"\\lftimes",	0x029D4}, //left black times%
	{"\\rftimes",	0x029D5}, //right black times%
	{"\\hourglass",	0x029D6}, //white hourglass%
	{"\\blackhourglass",	0x029D7}, //black hourglass%
	{"\\lvzigzag",	0x029D8}, //left wiggly fence%
	{"\\rvzigzag",	0x029D9}, //right wiggly fence%
	{"\\Lvzigzag",	0x029DA}, //left double wiggly fence%
	{"\\Rvzigzag",	0x029DB}, //right double wiggly fence%
	{"\\iinfin",	0x029DC}, //incomplete infinity%
	{"\\tieinfty",	0x029DD}, //tie over infinity%
	{"\\nvinfty",	0x029DE}, //infinity negated with vertical bar%
	{"\\laplac",	0x029E0}, //square with contoured outline%
	{"\\gleichstark",	0x029E6}, //gleich stark%
	{"\\thermod",	0x029E7}, //thermodynamic%
	{"\\downtriangleleftblack",	0x029E8}, //down-pointing triangle with left half black%
	{"\\downtrianglerightblack",	0x029E9}, //down-pointing triangle with right half black%
	{"\\blackdiamonddownarrow",	0x029EA}, //black diamond with down arrow%
	{"\\mdlgblklozenge",	0x029EB}, //black lozenge%
	{"\\circledownarrow",	0x029EC}, //white circle with down arrow%
	{"\\blackcircledownarrow",	0x029ED}, //black circle with down arrow%
	{"\\errbarsquare",	0x029EE}, //error-barred white square%
	{"\\errbarblacksquare",	0x029EF}, //error-barred black square%
	{"\\errbardiamond",	0x029F0}, //error-barred white diamond%
	{"\\errbarblackdiamond",	0x029F1}, //error-barred black diamond%
	{"\\errbarcircle",	0x029F2}, //error-barred white circle%
	{"\\errbarblackcircle",	0x029F3}, //error-barred black circle%
	{"\\ruledelayed",	0x029F4}, //rule-delayed%
	{"\\reversesolidus",	0x029F5}, //reverse solidus%
	{"\\xsol",	0x029F8}, //big solidus%
	{"\\xbsol",	0x029F9}, //big reverse solidus%
	{"\\lcurvyangle",	0x029FC}, //left pointing curved angle bracket%
	{"\\rcurvyangle",	0x029FD}, //right pointing curved angle bracket%
	{"\\tplus",	0x029FE}, //tiny%
	{"\\tminus",	0x029FF}, //miny%
	{"\\sumint",	0x02A0B}, //summation with integral%
	{"\\iiiint",	0x02A0C}, //quadruple integral operator%
	{"\\intbar",	0x02A0D}, //finite part integral%
	{"\\intBar",	0x02A0E}, //integral with double stroke%
	{"\\fint",	0x02A0F}, //integral average with slash%
	{"\\sqint",	0x02A16}, //quaternion integral operator%
	{"\\intlarhk",	0x02A17}, //integral with leftwards arrow with hook%
	{"\\intx",	0x02A18}, //integral with times sign%
	{"\\intcap",	0x02A19}, //integral with intersection%
	{"\\intcup",	0x02A1A}, //integral with union%
	{"\\bigtriangleleft",	0x02A1E}, //large left triangle operator%
	{"\\minusdot",	0x02A2A}, //minus sign with dot below%
	{"\\minusfdots",	0x02A2B}, //minus sign with falling dots%
	{"\\minusrdots",	0x02A2C}, //minus sign with rising dots%
	{"\\vectimes",	0x02A2F}, //vector or cross product%
	{"\\intprod",	0x02A3C}, //interior product%
	{"\\intprodr",	0x02A3D}, //righthand interior product%
	{"\\barcup",	0x02A42}, //union with overbar%
	{"\\barcap",	0x02A43}, //intersection with overbar%
	{"\\cupovercap",	0x02A46}, //union above intersection%
	{"\\capovercup",	0x02A47}, //intersection above union%
	{"\\cupbarcap",	0x02A48}, //union above bar above intersection%
	{"\\capbarcup",	0x02A49}, //intersection above bar above union%
	{"\\Wedge",	0x02A53}, //double logical and%
	{"\\Vee",	0x02A54}, //double logical or%
	{"\\veeonvee",	0x02A56}, //two intersecting logical or%
	{"\\veeonwedge",	0x02A59}, //logical or overlapping logical and%
	{"\\wedgebar",	0x02A5F}, //logical and with underbar%
	{"\\equivVert",	0x02A68}, //triple horizontal bar with double vertical stroke%
	{"\\equivVvert",	0x02A69}, //triple horizontal bar with triple vertical stroke%
	{"\\eqeq",	0x02A75}, //two consecutive equals signs%
	{"\\Lt",	0x02AA1}, //double nested less-than%
	{"\\Gt",	0x02AA2}, //double nested greater-than%
	{"\\topfork",	0x02ADA}, //pitchfork with tee top%
	{"\\shortlefttack",	0x02ADE}, //short left tack%
	{"\\shortdowntack",	0x02ADF}, //short down tack%
	{"\\shortuptack",	0x02AE0}, //short up tack%
	{"\\perps",	0x02AE1}, //perpendicular with s%
	{"\\vDdash",	0x02AE2}, //vertical bar triple right turnstile%
	{"\\DashV",	0x02AE5}, //double vertical bar double left turnstile%
	{"\\varVdash",	0x02AE6}, //long dash from left member of double vertical%
	{"\\Barv",	0x02AE7}, //short down tack with overbar%
	{"\\vBar",	0x02AE8}, //short up tack with underbar%
	{"\\vBarv",	0x02AE9}, //short up tack above short down tack%
	{"\\barV",	0x02AEA}, //double down tack%
	{"\\Vbar",	0x02AEB}, //double up tack%
	{"\\Not",	0x02AEC}, //double stroke not sign%
	{"\\bNot",	0x02AED}, //reversed double stroke not sign%
	{"\\revnmid",	0x02AEE}, //does not divide with reversed negation slash%
	{"\\cirmid",	0x02AEF}, //vertical line with circle above%
	{"\\midcir",	0x02AF0}, //vertical line with circle below%
	{"\\topcir",	0x02AF1}, //down tack with circle below%
	{"\\nhpar",	0x02AF2}, //parallel with horizontal stroke%
	{"\\parsim",	0x02AF3}, //parallel with tilde operator%
	{"\\nhVvert",	0x02AF5}, //triple vertical bar with horizontal stroke%
	{"\\threedotcolon",	0x02AF6}, //triple colon operator%
	{"\\trslash",	0x02AFB}, //triple solidus binary relation%
	{"\\bigtalloblong",	0x02AFF}, //n-ary white vertical bar%
	{"\\hzigzag",	0x03030}, //zigzag%
	{"\\mbfA",	0x1D400}, //mathematical bold capital a%
	{"\\mbfB",	0x1D401}, //mathematical bold capital b%
	{"\\mbfC",	0x1D402}, //mathematical bold capital c%
	{"\\mbfD",	0x1D403}, //mathematical bold capital d%
	{"\\mbfE",	0x1D404}, //mathematical bold capital e%
	{"\\mbfF",	0x1D405}, //mathematical bold capital f%
	{"\\mbfG",	0x1D406}, //mathematical bold capital g%
	{"\\mbfH",	0x1D407}, //mathematical bold capital h%
	{"\\mbfI",	0x1D408}, //mathematical bold capital i%
	{"\\mbfJ",	0x1D409}, //mathematical bold capital j%
	{"\\mbfK",	0x1D40A}, //mathematical bold capital k%
	{"\\mbfL",	0x1D40B}, //mathematical bold capital l%
	{"\\mbfM",	0x1D40C}, //mathematical bold capital m%
	{"\\mbfN",	0x1D40D}, //mathematical bold capital n%
	{"\\mbfO",	0x1D40E}, //mathematical bold capital o%
	{"\\mbfP",	0x1D40F}, //mathematical bold capital p%
	{"\\mbfQ",	0x1D410}, //mathematical bold capital q%
	{"\\mbfR",	0x1D411}, //mathematical bold capital r%
	{"\\mbfS",	0x1D412}, //mathematical bold capital s%
	{"\\mbfT",	0x1D413}, //mathematical bold capital t%
	{"\\mbfU",	0x1D414}, //mathematical bold capital u%
	{"\\mbfV",	0x1D415}, //mathematical bold capital v%
	{"\\mbfW",	0x1D416}, //mathematical bold capital w%
	{"\\mbfX",	0x1D417}, //mathematical bold capital x%
	{"\\mbfY",	0x1D418}, //mathematical bold capital y%
	{"\\mbfZ",	0x1D419}, //mathematical bold capital z%
	{"\\mbfa",	0x1D41A}, //mathematical bold small a%
	{"\\mbfb",	0x1D41B}, //mathematical bold small b%
	{"\\mbfc",	0x1D41C}, //mathematical bold small c%
	{"\\mbfd",	0x1D41D}, //mathematical bold small d%
	{"\\mbfe",	0x1D41E}, //mathematical bold small e%
	{"\\mbff",	0x1D41F}, //mathematical bold small f%
	{"\\mbfg",	0x1D420}, //mathematical bold small g%
	{"\\mbfh",	0x1D421}, //mathematical bold small h%
	{"\\mbfi",	0x1D422}, //mathematical bold small i%
	{"\\mbfj",	0x1D423}, //mathematical bold small j%
	{"\\mbfk",	0x1D424}, //mathematical bold small k%
	{"\\mbfl",	0x1D425}, //mathematical bold small l%
	{"\\mbfm",	0x1D426}, //mathematical bold small m%
	{"\\mbfn",	0x1D427}, //mathematical bold small n%
	{"\\mbfo",	0x1D428}, //mathematical bold small o%
	{"\\mbfp",	0x1D429}, //mathematical bold small p%
	{"\\mbfq",	0x1D42A}, //mathematical bold small q%
	{"\\mbfr",	0x1D42B}, //mathematical bold small r%
	{"\\mbfs",	0x1D42C}, //mathematical bold small s%
	{"\\mbft",	0x1D42D}, //mathematical bold small t%
	{"\\mbfu",	0x1D42E}, //mathematical bold small u%
	{"\\mbfv",	0x1D42F}, //mathematical bold small v%
	{"\\mbfw",	0x1D430}, //mathematical bold small w%
	{"\\mbfx",	0x1D431}, //mathematical bold small x%
	{"\\mbfy",	0x1D432}, //mathematical bold small y%
	{"\\mbfz",	0x1D433}, //mathematical bold small z%
	{"\\mitA",	0x1D434}, //mathematical italic capital a%
	{"\\mitB",	0x1D435}, //mathematical italic capital b%
	{"\\mitC",	0x1D436}, //mathematical italic capital c%
	{"\\mitD",	0x1D437}, //mathematical italic capital d%
	{"\\mitE",	0x1D438}, //mathematical italic capital e%
	{"\\mitF",	0x1D439}, //mathematical italic capital f%
	{"\\mitG",	0x1D43A}, //mathematical italic capital g%
	{"\\mitH",	0x1D43B}, //mathematical italic capital h%
	{"\\mitI",	0x1D43C}, //mathematical italic capital i%
	{"\\mitJ",	0x1D43D}, //mathematical italic capital j%
	{"\\mitK",	0x1D43E}, //mathematical italic capital k%
	{"\\mitL",	0x1D43F}, //mathematical italic capital l%
	{"\\mitM",	0x1D440}, //mathematical italic capital m%
	{"\\mitN",	0x1D441}, //mathematical italic capital n%
	{"\\mitO",	0x1D442}, //mathematical italic capital o%
	{"\\mitP",	0x1D443}, //mathematical italic capital p%
	{"\\mitQ",	0x1D444}, //mathematical italic capital q%
	{"\\mitR",	0x1D445}, //mathematical italic capital r%
	{"\\mitS",	0x1D446}, //mathematical italic capital s%
	{"\\mitT",	0x1D447}, //mathematical italic capital t%
	{"\\mitU",	0x1D448}, //mathematical italic capital u%
	{"\\mitV",	0x1D449}, //mathematical italic capital v%
	{"\\mitW",	0x1D44A}, //mathematical italic capital w%
	{"\\mitX",	0x1D44B}, //mathematical italic capital x%
	{"\\mitY",	0x1D44C}, //mathematical italic capital y%
	{"\\mitZ",	0x1D44D}, //mathematical italic capital z%
	{"\\mita",	0x1D44E}, //mathematical italic small a%
	{"\\mitb",	0x1D44F}, //mathematical italic small b%
	{"\\mitc",	0x1D450}, //mathematical italic small c%
	{"\\mitd",	0x1D451}, //mathematical italic small d%
	{"\\mite",	0x1D452}, //mathematical italic small e%
	{"\\mitf",	0x1D453}, //mathematical italic small f%
	{"\\mitg",	0x1D454}, //mathematical italic small g%
	{"\\miti",	0x1D456}, //mathematical italic small i%
	{"\\mitj",	0x1D457}, //mathematical italic small j%
	{"\\mitk",	0x1D458}, //mathematical italic small k%
	{"\\mitl",	0x1D459}, //mathematical italic small l%
	{"\\mitm",	0x1D45A}, //mathematical italic small m%
	{"\\mitn",	0x1D45B}, //mathematical italic small n%
	{"\\mito",	0x1D45C}, //mathematical italic small o%
	{"\\mitp",	0x1D45D}, //mathematical italic small p%
	{"\\mitq",	0x1D45E}, //mathematical italic small q%
	{"\\mitr",	0x1D45F}, //mathematical italic small r%
	{"\\mits",	0x1D460}, //mathematical italic small s%
	{"\\mitt",	0x1D461}, //mathematical italic small t%
	{"\\mitu",	0x1D462}, //mathematical italic small u%
	{"\\mitv",	0x1D463}, //mathematical italic small v%
	{"\\mitw",	0x1D464}, //mathematical italic small w%
	{"\\mitx",	0x1D465}, //mathematical italic small x%
	{"\\mity",	0x1D466}, //mathematical italic small y%
	{"\\mitz",	0x1D467}, //mathematical italic small z%
	{"\\mbfitA",	0x1D468}, //mathematical bold italic capital a%
	{"\\mbfitB",	0x1D469}, //mathematical bold italic capital b%
	{"\\mbfitC",	0x1D46A}, //mathematical bold italic capital c%
	{"\\mbfitD",	0x1D46B}, //mathematical bold italic capital d%
	{"\\mbfitE",	0x1D46C}, //mathematical bold italic capital e%
	{"\\mbfitF",	0x1D46D}, //mathematical bold italic capital f%
	{"\\mbfitG",	0x1D46E}, //mathematical bold italic capital g%
	{"\\mbfitH",	0x1D46F}, //mathematical bold italic capital h%
	{"\\mbfitI",	0x1D470}, //mathematical bold italic capital i%
	{"\\mbfitJ",	0x1D471}, //mathematical bold italic capital j%
	{"\\mbfitK",	0x1D472}, //mathematical bold italic capital k%
	{"\\mbfitL",	0x1D473}, //mathematical bold italic capital l%
	{"\\mbfitM",	0x1D474}, //mathematical bold italic capital m%
	{"\\mbfitN",	0x1D475}, //mathematical bold italic capital n%
	{"\\mbfitO",	0x1D476}, //mathematical bold italic capital o%
	{"\\mbfitP",	0x1D477}, //mathematical bold italic capital p%
	{"\\mbfitQ",	0x1D478}, //mathematical bold italic capital q%
	{"\\mbfitR",	0x1D479}, //mathematical bold italic capital r%
	{"\\mbfitS",	0x1D47A}, //mathematical bold italic capital s%
	{"\\mbfitT",	0x1D47B}, //mathematical bold italic capital t%
	{"\\mbfitU",	0x1D47C}, //mathematical bold italic capital u%
	{"\\mbfitV",	0x1D47D}, //mathematical bold italic capital v%
	{"\\mbfitW",	0x1D47E}, //mathematical bold italic capital w%
	{"\\mbfitX",	0x1D47F}, //mathematical bold italic capital x%
	{"\\mbfitY",	0x1D480}, //mathematical bold italic capital y%
	{"\\mbfitZ",	0x1D481}, //mathematical bold italic capital z%
	{"\\mbfita",	0x1D482}, //mathematical bold italic small a%
	{"\\mbfitb",	0x1D483}, //mathematical bold italic small b%
	{"\\mbfitc",	0x1D484}, //mathematical bold italic small c%
	{"\\mbfitd",	0x1D485}, //mathematical bold italic small d%
	{"\\mbfite",	0x1D486}, //mathematical bold italic small e%
	{"\\mbfitf",	0x1D487}, //mathematical bold italic small f%
	{"\\mbfitg",	0x1D488}, //mathematical bold italic small g%
	{"\\mbfith",	0x1D489}, //mathematical bold italic small h%
	{"\\mbfiti",	0x1D48A}, //mathematical bold italic small i%
	{"\\mbfitj",	0x1D48B}, //mathematical bold italic small j%
	{"\\mbfitk",	0x1D48C}, //mathematical bold italic small k%
	{"\\mbfitl",	0x1D48D}, //mathematical bold italic small l%
	{"\\mbfitm",	0x1D48E}, //mathematical bold italic small m%
	{"\\mbfitn",	0x1D48F}, //mathematical bold italic small n%
	{"\\mbfito",	0x1D490}, //mathematical bold italic small o%
	{"\\mbfitp",	0x1D491}, //mathematical bold italic small p%
	{"\\mbfitq",	0x1D492}, //mathematical bold italic small q%
	{"\\mbfitr",	0x1D493}, //mathematical bold italic small r%
	{"\\mbfits",	0x1D494}, //mathematical bold italic small s%
	{"\\mbfitt",	0x1D495}, //mathematical bold italic small t%
	{"\\mbfitu",	0x1D496}, //mathematical bold italic small u%
	{"\\mbfitv",	0x1D497}, //mathematical bold italic small v%
	{"\\mbfitw",	0x1D498}, //mathematical bold italic small w%
	{"\\mbfitx",	0x1D499}, //mathematical bold italic small x%
	{"\\mbfity",	0x1D49A}, //mathematical bold italic small y%
	{"\\mbfitz",	0x1D49B}, //mathematical bold italic small z%
	{"\\mscrA",	0x1D49C}, //mathematical script capital a%
	{"\\mscrC",	0x1D49E}, //mathematical script capital c%
	{"\\mscrD",	0x1D49F}, //mathematical script capital d%
	{"\\mscrG",	0x1D4A2}, //mathematical script capital g%
	{"\\mscrJ",	0x1D4A5}, //mathematical script capital j%
	{"\\mscrK",	0x1D4A6}, //mathematical script capital k%
	{"\\mscrN",	0x1D4A9}, //mathematical script capital n%
	{"\\mscrO",	0x1D4AA}, //mathematical script capital o%
	{"\\mscrP",	0x1D4AB}, //mathematical script capital p%
	{"\\mscrQ",	0x1D4AC}, //mathematical script capital q%
	{"\\mscrS",	0x1D4AE}, //mathematical script capital s%
	{"\\mscrT",	0x1D4AF}, //mathematical script capital t%
	{"\\mscrU",	0x1D4B0}, //mathematical script capital u%
	{"\\mscrV",	0x1D4B1}, //mathematical script capital v%
	{"\\mscrW",	0x1D4B2}, //mathematical script capital w%
	{"\\mscrX",	0x1D4B3}, //mathematical script capital x%
	{"\\mscrY",	0x1D4B4}, //mathematical script capital y%
	{"\\mscrZ",	0x1D4B5}, //mathematical script capital z%
	{"\\mscra",	0x1D4B6}, //mathematical script small a%
	{"\\mscrb",	0x1D4B7}, //mathematical script small b%
	{"\\mscrc",	0x1D4B8}, //mathematical script small c%
	{"\\mscrd",	0x1D4B9}, //mathematical script small d%
	{"\\mscrf",	0x1D4BB}, //mathematical script small f%
	{"\\mscrh",	0x1D4BD}, //mathematical script small h%
	{"\\mscri",	0x1D4BE}, //mathematical script small i%
	{"\\mscrj",	0x1D4BF}, //mathematical script small j%
	{"\\mscrk",	0x1D4C0}, //mathematical script small k%
	{"\\mscrl",	0x1D4C1}, //mathematical script small l%
	{"\\mscrm",	0x1D4C2}, //mathematical script small m%
	{"\\mscrn",	0x1D4C3}, //mathematical script small n%
	{"\\mscrp",	0x1D4C5}, //mathematical script small p%
	{"\\mscrq",	0x1D4C6}, //mathematical script small q%
	{"\\mscrr",	0x1D4C7}, //mathematical script small r%
	{"\\mscrs",	0x1D4C8}, //mathematical script small s%
	{"\\mscrt",	0x1D4C9}, //mathematical script small t%
	{"\\mscru",	0x1D4CA}, //mathematical script small u%
	{"\\mscrv",	0x1D4CB}, //mathematical script small v%
	{"\\mscrw",	0x1D4CC}, //mathematical script small w%
	{"\\mscrx",	0x1D4CD}, //mathematical script small x%
	{"\\mscry",	0x1D4CE}, //mathematical script small y%
	{"\\mscrz",	0x1D4CF}, //mathematical script small z%
	{"\\mbfscrA",	0x1D4D0}, //mathematical bold script capital a%
	{"\\mbfscrB",	0x1D4D1}, //mathematical bold script capital b%
	{"\\mbfscrC",	0x1D4D2}, //mathematical bold script capital c%
	{"\\mbfscrD",	0x1D4D3}, //mathematical bold script capital d%
	{"\\mbfscrE",	0x1D4D4}, //mathematical bold script capital e%
	{"\\mbfscrF",	0x1D4D5}, //mathematical bold script capital f%
	{"\\mbfscrG",	0x1D4D6}, //mathematical bold script capital g%
	{"\\mbfscrH",	0x1D4D7}, //mathematical bold script capital h%
	{"\\mbfscrI",	0x1D4D8}, //mathematical bold script capital i%
	{"\\mbfscrJ",	0x1D4D9}, //mathematical bold script capital j%
	{"\\mbfscrK",	0x1D4DA}, //mathematical bold script capital k%
	{"\\mbfscrL",	0x1D4DB}, //mathematical bold script capital l%
	{"\\mbfscrM",	0x1D4DC}, //mathematical bold script capital m%
	{"\\mbfscrN",	0x1D4DD}, //mathematical bold script capital n%
	{"\\mbfscrO",	0x1D4DE}, //mathematical bold script capital o%
	{"\\mbfscrP",	0x1D4DF}, //mathematical bold script capital p%
	{"\\mbfscrQ",	0x1D4E0}, //mathematical bold script capital q%
	{"\\mbfscrR",	0x1D4E1}, //mathematical bold script capital r%
	{"\\mbfscrS",	0x1D4E2}, //mathematical bold script capital s%
	{"\\mbfscrT",	0x1D4E3}, //mathematical bold script capital t%
	{"\\mbfscrU",	0x1D4E4}, //mathematical bold script capital u%
	{"\\mbfscrV",	0x1D4E5}, //mathematical bold script capital v%
	{"\\mbfscrW",	0x1D4E6}, //mathematical bold script capital w%
	{"\\mbfscrX",	0x1D4E7}, //mathematical bold script capital x%
	{"\\mbfscrY",	0x1D4E8}, //mathematical bold script capital y%
	{"\\mbfscrZ",	0x1D4E9}, //mathematical bold script capital z%
	{"\\mbfscra",	0x1D4EA}, //mathematical bold script small a%
	{"\\mbfscrb",	0x1D4EB}, //mathematical bold script small b%
	{"\\mbfscrc",	0x1D4EC}, //mathematical bold script small c%
	{"\\mbfscrd",	0x1D4ED}, //mathematical bold script small d%
	{"\\mbfscre",	0x1D4EE}, //mathematical bold script small e%
	{"\\mbfscrf",	0x1D4EF}, //mathematical bold script small f%
	{"\\mbfscrg",	0x1D4F0}, //mathematical bold script small g%
	{"\\mbfscrh",	0x1D4F1}, //mathematical bold script small h%
	{"\\mbfscri",	0x1D4F2}, //mathematical bold script small i%
	{"\\mbfscrj",	0x1D4F3}, //mathematical bold script small j%
	{"\\mbfscrk",	0x1D4F4}, //mathematical bold script small k%
	{"\\mbfscrl",	0x1D4F5}, //mathematical bold script small l%
	{"\\mbfscrm",	0x1D4F6}, //mathematical bold script small m%
	{"\\mbfscrn",	0x1D4F7}, //mathematical bold script small n%
	{"\\mbfscro",	0x1D4F8}, //mathematical bold script small o%
	{"\\mbfscrp",	0x1D4F9}, //mathematical bold script small p%
	{"\\mbfscrq",	0x1D4FA}, //mathematical bold script small q%
	{"\\mbfscrr",	0x1D4FB}, //mathematical bold script small r%
	{"\\mbfscrs",	0x1D4FC}, //mathematical bold script small s%
	{"\\mbfscrt",	0x1D4FD}, //mathematical bold script small t%
	{"\\mbfscru",	0x1D4FE}, //mathematical bold script small u%
	{"\\mbfscrv",	0x1D4FF}, //mathematical bold script small v%
	{"\\mbfscrw",	0x1D500}, //mathematical bold script small w%
	{"\\mbfscrx",	0x1D501}, //mathematical bold script small x%
	{"\\mbfscry",	0x1D502}, //mathematical bold script small y%
	{"\\mbfscrz",	0x1D503}, //mathematical bold script small z%
	{"\\mfrakA",	0x1D504}, //mathematical fraktur capital a%
	{"\\mfrakB",	0x1D505}, //mathematical fraktur capital b%
	{"\\mfrakD",	0x1D507}, //mathematical fraktur capital d%
	{"\\mfrakE",	0x1D508}, //mathematical fraktur capital e%
	{"\\mfrakF",	0x1D509}, //mathematical fraktur capital f%
	{"\\mfrakG",	0x1D50A}, //mathematical fraktur capital g%
	{"\\mfrakJ",	0x1D50D}, //mathematical fraktur capital j%
	{"\\mfrakK",	0x1D50E}, //mathematical fraktur capital k%
	{"\\mfrakL",	0x1D50F}, //mathematical fraktur capital l%
	{"\\mfrakM",	0x1D510}, //mathematical fraktur capital m%
	{"\\mfrakN",	0x1D511}, //mathematical fraktur capital n%
	{"\\mfrakO",	0x1D512}, //mathematical fraktur capital o%
	{"\\mfrakP",	0x1D513}, //mathematical fraktur capital p%
	{"\\mfrakQ",	0x1D514}, //mathematical fraktur capital q%
	{"\\mfrakS",	0x1D516}, //mathematical fraktur capital s%
	{"\\mfrakT",	0x1D517}, //mathematical fraktur capital t%
	{"\\mfrakU",	0x1D518}, //mathematical fraktur capital u%
	{"\\mfrakV",	0x1D519}, //mathematical fraktur capital v%
	{"\\mfrakW",	0x1D51A}, //mathematical fraktur capital w%
	{"\\mfrakX",	0x1D51B}, //mathematical fraktur capital x%
	{"\\mfrakY",	0x1D51C}, //mathematical fraktur capital y%
	{"\\mfraka",	0x1D51E}, //mathematical fraktur small a%
	{"\\mfrakb",	0x1D51F}, //mathematical fraktur small b%
	{"\\mfrakc",	0x1D520}, //mathematical fraktur small c%
	{"\\mfrakd",	0x1D521}, //mathematical fraktur small d%
	{"\\mfrake",	0x1D522}, //mathematical fraktur small e%
	{"\\mfrakf",	0x1D523}, //mathematical fraktur small f%
	{"\\mfrakg",	0x1D524}, //mathematical fraktur small g%
	{"\\mfrakh",	0x1D525}, //mathematical fraktur small h%
	{"\\mfraki",	0x1D526}, //mathematical fraktur small i%
	{"\\mfrakj",	0x1D527}, //mathematical fraktur small j%
	{"\\mfrakk",	0x1D528}, //mathematical fraktur small k%
	{"\\mfrakl",	0x1D529}, //mathematical fraktur small l%
	{"\\mfrakm",	0x1D52A}, //mathematical fraktur small m%
	{"\\mfrakn",	0x1D52B}, //mathematical fraktur small n%
	{"\\mfrako",	0x1D52C}, //mathematical fraktur small o%
	{"\\mfrakp",	0x1D52D}, //mathematical fraktur small p%
	{"\\mfrakq",	0x1D52E}, //mathematical fraktur small q%
	{"\\mfrakr",	0x1D52F}, //mathematical fraktur small r%
	{"\\mfraks",	0x1D530}, //mathematical fraktur small s%
	{"\\mfrakt",	0x1D531}, //mathematical fraktur small t%
	{"\\mfraku",	0x1D532}, //mathematical fraktur small u%
	{"\\mfrakv",	0x1D533}, //mathematical fraktur small v%
	{"\\mfrakw",	0x1D534}, //mathematical fraktur small w%
	{"\\mfrakx",	0x1D535}, //mathematical fraktur small x%
	{"\\mfraky",	0x1D536}, //mathematical fraktur small y%
	{"\\mfrakz",	0x1D537}, //mathematical fraktur small z%
	{"\\BbbA",	0x1D538}, //mathematical double-struck capital a%
	{"\\BbbB",	0x1D539}, //mathematical double-struck capital b%
	{"\\BbbD",	0x1D53B}, //mathematical double-struck capital d%
	{"\\BbbE",	0x1D53C}, //mathematical double-struck capital e%
	{"\\BbbF",	0x1D53D}, //mathematical double-struck capital f%
	{"\\BbbG",	0x1D53E}, //mathematical double-struck capital g%
	{"\\BbbI",	0x1D540}, //mathematical double-struck capital i%
	{"\\BbbJ",	0x1D541}, //mathematical double-struck capital j%
	{"\\BbbK",	0x1D542}, //mathematical double-struck capital k%
	{"\\BbbL",	0x1D543}, //mathematical double-struck capital l%
	{"\\BbbM",	0x1D544}, //mathematical double-struck capital m%
	{"\\BbbO",	0x1D546}, //mathematical double-struck capital o%
	{"\\BbbS",	0x1D54A}, //mathematical double-struck capital s%
	{"\\BbbT",	0x1D54B}, //mathematical double-struck capital t%
	{"\\BbbU",	0x1D54C}, //mathematical double-struck capital u%
	{"\\BbbV",	0x1D54D}, //mathematical double-struck capital v%
	{"\\BbbW",	0x1D54E}, //mathematical double-struck capital w%
	{"\\BbbX",	0x1D54F}, //mathematical double-struck capital x%
	{"\\BbbY",	0x1D550}, //mathematical double-struck capital y%
	{"\\Bbba",	0x1D552}, //mathematical double-struck small a%
	{"\\Bbbb",	0x1D553}, //mathematical double-struck small b%
	{"\\Bbbc",	0x1D554}, //mathematical double-struck small c%
	{"\\Bbbd",	0x1D555}, //mathematical double-struck small d%
	{"\\Bbbe",	0x1D556}, //mathematical double-struck small e%
	{"\\Bbbf",	0x1D557}, //mathematical double-struck small f%
	{"\\Bbbg",	0x1D558}, //mathematical double-struck small g%
	{"\\Bbbh",	0x1D559}, //mathematical double-struck small h%
	{"\\Bbbi",	0x1D55A}, //mathematical double-struck small i%
	{"\\Bbbj",	0x1D55B}, //mathematical double-struck small j%
	{"\\Bbbk",	0x1D55C}, //mathematical double-struck small k%
	{"\\Bbbl",	0x1D55D}, //mathematical double-struck small l%
	{"\\Bbbm",	0x1D55E}, //mathematical double-struck small m%
	{"\\Bbbn",	0x1D55F}, //mathematical double-struck small n%
	{"\\Bbbo",	0x1D560}, //mathematical double-struck small o%
	{"\\Bbbp",	0x1D561}, //mathematical double-struck small p%
	{"\\Bbbq",	0x1D562}, //mathematical double-struck small q%
	{"\\Bbbr",	0x1D563}, //mathematical double-struck small r%
	{"\\Bbbs",	0x1D564}, //mathematical double-struck small s%
	{"\\Bbbt",	0x1D565}, //mathematical double-struck small t%
	{"\\Bbbu",	0x1D566}, //mathematical double-struck small u%
	{"\\Bbbv",	0x1D567}, //mathematical double-struck small v%
	{"\\Bbbw",	0x1D568}, //mathematical double-struck small w%
	{"\\Bbbx",	0x1D569}, //mathematical double-struck small x%
	{"\\Bbby",	0x1D56A}, //mathematical double-struck small y%
	{"\\Bbbz",	0x1D56B}, //mathematical double-struck small z%
	{"\\mbffrakA",	0x1D56C}, //mathematical bold fraktur capital a%
	{"\\mbffrakB",	0x1D56D}, //mathematical bold fraktur capital b%
	{"\\mbffrakC",	0x1D56E}, //mathematical bold fraktur capital c%
	{"\\mbffrakD",	0x1D56F}, //mathematical bold fraktur capital d%
	{"\\mbffrakE",	0x1D570}, //mathematical bold fraktur capital e%
	{"\\mbffrakF",	0x1D571}, //mathematical bold fraktur capital f%
	{"\\mbffrakG",	0x1D572}, //mathematical bold fraktur capital g%
	{"\\mbffrakH",	0x1D573}, //mathematical bold fraktur capital h%
	{"\\mbffrakI",	0x1D574}, //mathematical bold fraktur capital i%
	{"\\mbffrakJ",	0x1D575}, //mathematical bold fraktur capital j%
	{"\\mbffrakK",	0x1D576}, //mathematical bold fraktur capital k%
	{"\\mbffrakL",	0x1D577}, //mathematical bold fraktur capital l%
	{"\\mbffrakM",	0x1D578}, //mathematical bold fraktur capital m%
	{"\\mbffrakN",	0x1D579}, //mathematical bold fraktur capital n%
	{"\\mbffrakO",	0x1D57A}, //mathematical bold fraktur capital o%
	{"\\mbffrakP",	0x1D57B}, //mathematical bold fraktur capital p%
	{"\\mbffrakQ",	0x1D57C}, //mathematical bold fraktur capital q%
	{"\\mbffrakR",	0x1D57D}, //mathematical bold fraktur capital r%
	{"\\mbffrakS",	0x1D57E}, //mathematical bold fraktur capital s%
	{"\\mbffrakT",	0x1D57F}, //mathematical bold fraktur capital t%
	{"\\mbffrakU",	0x1D580}, //mathematical bold fraktur capital u%
	{"\\mbffrakV",	0x1D581}, //mathematical bold fraktur capital v%
	{"\\mbffrakW",	0x1D582}, //mathematical bold fraktur capital w%
	{"\\mbffrakX",	0x1D583}, //mathematical bold fraktur capital x%
	{"\\mbffrakY",	0x1D584}, //mathematical bold fraktur capital y%
	{"\\mbffrakZ",	0x1D585}, //mathematical bold fraktur capital z%
	{"\\mbffraka",	0x1D586}, //mathematical bold fraktur small a%
	{"\\mbffrakb",	0x1D587}, //mathematical bold fraktur small b%
	{"\\mbffrakc",	0x1D588}, //mathematical bold fraktur small c%
	{"\\mbffrakd",	0x1D589}, //mathematical bold fraktur small d%
	{"\\mbffrake",	0x1D58A}, //mathematical bold fraktur small e%
	{"\\mbffrakf",	0x1D58B}, //mathematical bold fraktur small f%
	{"\\mbffrakg",	0x1D58C}, //mathematical bold fraktur small g%
	{"\\mbffrakh",	0x1D58D}, //mathematical bold fraktur small h%
	{"\\mbffraki",	0x1D58E}, //mathematical bold fraktur small i%
	{"\\mbffrakj",	0x1D58F}, //mathematical bold fraktur small j%
	{"\\mbffrakk",	0x1D590}, //mathematical bold fraktur small k%
	{"\\mbffrakl",	0x1D591}, //mathematical bold fraktur small l%
	{"\\mbffrakm",	0x1D592}, //mathematical bold fraktur small m%
	{"\\mbffrakn",	0x1D593}, //mathematical bold fraktur small n%
	{"\\mbffrako",	0x1D594}, //mathematical bold fraktur small o%
	{"\\mbffrakp",	0x1D595}, //mathematical bold fraktur small p%
	{"\\mbffrakq",	0x1D596}, //mathematical bold fraktur small q%
	{"\\mbffrakr",	0x1D597}, //mathematical bold fraktur small r%
	{"\\mbffraks",	0x1D598}, //mathematical bold fraktur small s%
	{"\\mbffrakt",	0x1D599}, //mathematical bold fraktur small t%
	{"\\mbffraku",	0x1D59A}, //mathematical bold fraktur small u%
	{"\\mbffrakv",	0x1D59B}, //mathematical bold fraktur small v%
	{"\\mbffrakw",	0x1D59C}, //mathematical bold fraktur small w%
	{"\\mbffrakx",	0x1D59D}, //mathematical bold fraktur small x%
	{"\\mbffraky",	0x1D59E}, //mathematical bold fraktur small y%
	{"\\mbffrakz",	0x1D59F}, //mathematical bold fraktur small z%
	{"\\msansA",	0x1D5A0}, //mathematical sans-serif capital a%
	{"\\msansB",	0x1D5A1}, //mathematical sans-serif capital b%
	{"\\msansC",	0x1D5A2}, //mathematical sans-serif capital c%
	{"\\msansD",	0x1D5A3}, //mathematical sans-serif capital d%
	{"\\msansE",	0x1D5A4}, //mathematical sans-serif capital e%
	{"\\msansF",	0x1D5A5}, //mathematical sans-serif capital f%
	{"\\msansG",	0x1D5A6}, //mathematical sans-serif capital g%
	{"\\msansH",	0x1D5A7}, //mathematical sans-serif capital h%
	{"\\msansI",	0x1D5A8}, //mathematical sans-serif capital i%
	{"\\msansJ",	0x1D5A9}, //mathematical sans-serif capital j%
	{"\\msansK",	0x1D5AA}, //mathematical sans-serif capital k%
	{"\\msansL",	0x1D5AB}, //mathematical sans-serif capital l%
	{"\\msansM",	0x1D5AC}, //mathematical sans-serif capital m%
	{"\\msansN",	0x1D5AD}, //mathematical sans-serif capital n%
	{"\\msansO",	0x1D5AE}, //mathematical sans-serif capital o%
	{"\\msansP",	0x1D5AF}, //mathematical sans-serif capital p%
	{"\\msansQ",	0x1D5B0}, //mathematical sans-serif capital q%
	{"\\msansR",	0x1D5B1}, //mathematical sans-serif capital r%
	{"\\msansS",	0x1D5B2}, //mathematical sans-serif capital s%
	{"\\msansT",	0x1D5B3}, //mathematical sans-serif capital t%
	{"\\msansU",	0x1D5B4}, //mathematical sans-serif capital u%
	{"\\msansV",	0x1D5B5}, //mathematical sans-serif capital v%
	{"\\msansW",	0x1D5B6}, //mathematical sans-serif capital w%
	{"\\msansX",	0x1D5B7}, //mathematical sans-serif capital x%
	{"\\msansY",	0x1D5B8}, //mathematical sans-serif capital y%
	{"\\msansZ",	0x1D5B9}, //mathematical sans-serif capital z%
	{"\\msansa",	0x1D5BA}, //mathematical sans-serif small a%
	{"\\msansb",	0x1D5BB}, //mathematical sans-serif small b%
	{"\\msansc",	0x1D5BC}, //mathematical sans-serif small c%
	{"\\msansd",	0x1D5BD}, //mathematical sans-serif small d%
	{"\\msanse",	0x1D5BE}, //mathematical sans-serif small e%
	{"\\msansf",	0x1D5BF}, //mathematical sans-serif small f%
	{"\\msansg",	0x1D5C0}, //mathematical sans-serif small g%
	{"\\msansh",	0x1D5C1}, //mathematical sans-serif small h%
	{"\\msansi",	0x1D5C2}, //mathematical sans-serif small i%
	{"\\msansj",	0x1D5C3}, //mathematical sans-serif small j%
	{"\\msansk",	0x1D5C4}, //mathematical sans-serif small k%
	{"\\msansl",	0x1D5C5}, //mathematical sans-serif small l%
	{"\\msansm",	0x1D5C6}, //mathematical sans-serif small m%
	{"\\msansn",	0x1D5C7}, //mathematical sans-serif small n%
	{"\\msanso",	0x1D5C8}, //mathematical sans-serif small o%
	{"\\msansp",	0x1D5C9}, //mathematical sans-serif small p%
	{"\\msansq",	0x1D5CA}, //mathematical sans-serif small q%
	{"\\msansr",	0x1D5CB}, //mathematical sans-serif small r%
	{"\\msanss",	0x1D5CC}, //mathematical sans-serif small s%
	{"\\msanst",	0x1D5CD}, //mathematical sans-serif small t%
	{"\\msansu",	0x1D5CE}, //mathematical sans-serif small u%
	{"\\msansv",	0x1D5CF}, //mathematical sans-serif small v%
	{"\\msansw",	0x1D5D0}, //mathematical sans-serif small w%
	{"\\msansx",	0x1D5D1}, //mathematical sans-serif small x%
	{"\\msansy",	0x1D5D2}, //mathematical sans-serif small y%
	{"\\msansz",	0x1D5D3}, //mathematical sans-serif small z%
	{"\\mbfsansA",	0x1D5D4}, //mathematical sans-serif bold capital a%
	{"\\mbfsansB",	0x1D5D5}, //mathematical sans-serif bold capital b%
	{"\\mbfsansC",	0x1D5D6}, //mathematical sans-serif bold capital c%
	{"\\mbfsansD",	0x1D5D7}, //mathematical sans-serif bold capital d%
	{"\\mbfsansE",	0x1D5D8}, //mathematical sans-serif bold capital e%
	{"\\mbfsansF",	0x1D5D9}, //mathematical sans-serif bold capital f%
	{"\\mbfsansG",	0x1D5DA}, //mathematical sans-serif bold capital g%
	{"\\mbfsansH",	0x1D5DB}, //mathematical sans-serif bold capital h%
	{"\\mbfsansI",	0x1D5DC}, //mathematical sans-serif bold capital i%
	{"\\mbfsansJ",	0x1D5DD}, //mathematical sans-serif bold capital j%
	{"\\mbfsansK",	0x1D5DE}, //mathematical sans-serif bold capital k%
	{"\\mbfsansL",	0x1D5DF}, //mathematical sans-serif bold capital l%
	{"\\mbfsansM",	0x1D5E0}, //mathematical sans-serif bold capital m%
	{"\\mbfsansN",	0x1D5E1}, //mathematical sans-serif bold capital n%
	{"\\mbfsansO",	0x1D5E2}, //mathematical sans-serif bold capital o%
	{"\\mbfsansP",	0x1D5E3}, //mathematical sans-serif bold capital p%
	{"\\mbfsansQ",	0x1D5E4}, //mathematical sans-serif bold capital q%
	{"\\mbfsansR",	0x1D5E5}, //mathematical sans-serif bold capital r%
	{"\\mbfsansS",	0x1D5E6}, //mathematical sans-serif bold capital s%
	{"\\mbfsansT",	0x1D5E7}, //mathematical sans-serif bold capital t%
	{"\\mbfsansU",	0x1D5E8}, //mathematical sans-serif bold capital u%
	{"\\mbfsansV",	0x1D5E9}, //mathematical sans-serif bold capital v%
	{"\\mbfsansW",	0x1D5EA}, //mathematical sans-serif bold capital w%
	{"\\mbfsansX",	0x1D5EB}, //mathematical sans-serif bold capital x%
	{"\\mbfsansY",	0x1D5EC}, //mathematical sans-serif bold capital y%
	{"\\mbfsansZ",	0x1D5ED}, //mathematical sans-serif bold capital z%
	{"\\mbfsansa",	0x1D5EE}, //mathematical sans-serif bold small a%
	{"\\mbfsansb",	0x1D5EF}, //mathematical sans-serif bold small b%
	{"\\mbfsansc",	0x1D5F0}, //mathematical sans-serif bold small c%
	{"\\mbfsansd",	0x1D5F1}, //mathematical sans-serif bold small d%
	{"\\mbfsanse",	0x1D5F2}, //mathematical sans-serif bold small e%
	{"\\mbfsansf",	0x1D5F3}, //mathematical sans-serif bold small f%
	{"\\mbfsansg",	0x1D5F4}, //mathematical sans-serif bold small g%
	{"\\mbfsansh",	0x1D5F5}, //mathematical sans-serif bold small h%
	{"\\mbfsansi",	0x1D5F6}, //mathematical sans-serif bold small i%
	{"\\mbfsansj",	0x1D5F7}, //mathematical sans-serif bold small j%
	{"\\mbfsansk",	0x1D5F8}, //mathematical sans-serif bold small k%
	{"\\mbfsansl",	0x1D5F9}, //mathematical sans-serif bold small l%
	{"\\mbfsansm",	0x1D5FA}, //mathematical sans-serif bold small m%
	{"\\mbfsansn",	0x1D5FB}, //mathematical sans-serif bold small n%
	{"\\mbfsanso",	0x1D5FC}, //mathematical sans-serif bold small o%
	{"\\mbfsansp",	0x1D5FD}, //mathematical sans-serif bold small p%
	{"\\mbfsansq",	0x1D5FE}, //mathematical sans-serif bold small q%
	{"\\mbfsansr",	0x1D5FF}, //mathematical sans-serif bold small r%
	{"\\mbfsanss",	0x1D600}, //mathematical sans-serif bold small s%
	{"\\mbfsanst",	0x1D601}, //mathematical sans-serif bold small t%
	{"\\mbfsansu",	0x1D602}, //mathematical sans-serif bold small u%
	{"\\mbfsansv",	0x1D603}, //mathematical sans-serif bold small v%
	{"\\mbfsansw",	0x1D604}, //mathematical sans-serif bold small w%
	{"\\mbfsansx",	0x1D605}, //mathematical sans-serif bold small x%
	{"\\mbfsansy",	0x1D606}, //mathematical sans-serif bold small y%
	{"\\mbfsansz",	0x1D607}, //mathematical sans-serif bold small z%
	{"\\mitsansA",	0x1D608}, //mathematical sans-serif italic capital a%
	{"\\mitsansB",	0x1D609}, //mathematical sans-serif italic capital b%
	{"\\mitsansC",	0x1D60A}, //mathematical sans-serif italic capital c%
	{"\\mitsansD",	0x1D60B}, //mathematical sans-serif italic capital d%
	{"\\mitsansE",	0x1D60C}, //mathematical sans-serif italic capital e%
	{"\\mitsansF",	0x1D60D}, //mathematical sans-serif italic capital f%
	{"\\mitsansG",	0x1D60E}, //mathematical sans-serif italic capital g%
	{"\\mitsansH",	0x1D60F}, //mathematical sans-serif italic capital h%
	{"\\mitsansI",	0x1D610}, //mathematical sans-serif italic capital i%
	{"\\mitsansJ",	0x1D611}, //mathematical sans-serif italic capital j%
	{"\\mitsansK",	0x1D612}, //mathematical sans-serif italic capital k%
	{"\\mitsansL",	0x1D613}, //mathematical sans-serif italic capital l%
	{"\\mitsansM",	0x1D614}, //mathematical sans-serif italic capital m%
	{"\\mitsansN",	0x1D615}, //mathematical sans-serif italic capital n%
	{"\\mitsansO",	0x1D616}, //mathematical sans-serif italic capital o%
	{"\\mitsansP",	0x1D617}, //mathematical sans-serif italic capital p%
	{"\\mitsansQ",	0x1D618}, //mathematical sans-serif italic capital q%
	{"\\mitsansR",	0x1D619}, //mathematical sans-serif italic capital r%
	{"\\mitsansS",	0x1D61A}, //mathematical sans-serif italic capital s%
	{"\\mitsansT",	0x1D61B}, //mathematical sans-serif italic capital t%
	{"\\mitsansU",	0x1D61C}, //mathematical sans-serif italic capital u%
	{"\\mitsansV",	0x1D61D}, //mathematical sans-serif italic capital v%
	{"\\mitsansW",	0x1D61E}, //mathematical sans-serif italic capital w%
	{"\\mitsansX",	0x1D61F}, //mathematical sans-serif italic capital x%
	{"\\mitsansY",	0x1D620}, //mathematical sans-serif italic capital y%
	{"\\mitsansZ",	0x1D621}, //mathematical sans-serif italic capital z%
	{"\\mitsansa",	0x1D622}, //mathematical sans-serif italic small a%
	{"\\mitsansb",	0x1D623}, //mathematical sans-serif italic small b%
	{"\\mitsansc",	0x1D624}, //mathematical sans-serif italic small c%
	{"\\mitsansd",	0x1D625}, //mathematical sans-serif italic small d%
	{"\\mitsanse",	0x1D626}, //mathematical sans-serif italic small e%
	{"\\mitsansf",	0x1D627}, //mathematical sans-serif italic small f%
	{"\\mitsansg",	0x1D628}, //mathematical sans-serif italic small g%
	{"\\mitsansh",	0x1D629}, //mathematical sans-serif italic small h%
	{"\\mitsansi",	0x1D62A}, //mathematical sans-serif italic small i%
	{"\\mitsansj",	0x1D62B}, //mathematical sans-serif italic small j%
	{"\\mitsansk",	0x1D62C}, //mathematical sans-serif italic small k%
	{"\\mitsansl",	0x1D62D}, //mathematical sans-serif italic small l%
	{"\\mitsansm",	0x1D62E}, //mathematical sans-serif italic small m%
	{"\\mitsansn",	0x1D62F}, //mathematical sans-serif italic small n%
	{"\\mitsanso",	0x1D630}, //mathematical sans-serif italic small o%
	{"\\mitsansp",	0x1D631}, //mathematical sans-serif italic small p%
	{"\\mitsansq",	0x1D632}, //mathematical sans-serif italic small q%
	{"\\mitsansr",	0x1D633}, //mathematical sans-serif italic small r%
	{"\\mitsanss",	0x1D634}, //mathematical sans-serif italic small s%
	{"\\mitsanst",	0x1D635}, //mathematical sans-serif italic small t%
	{"\\mitsansu",	0x1D636}, //mathematical sans-serif italic small u%
	{"\\mitsansv",	0x1D637}, //mathematical sans-serif italic small v%
	{"\\mitsansw",	0x1D638}, //mathematical sans-serif italic small w%
	{"\\mitsansx",	0x1D639}, //mathematical sans-serif italic small x%
	{"\\mitsansy",	0x1D63A}, //mathematical sans-serif italic small y%
	{"\\mitsansz",	0x1D63B}, //mathematical sans-serif italic small z%
	{"\\mbfitsansA",	0x1D63C}, //mathematical sans-serif bold italic capital a%
	{"\\mbfitsansB",	0x1D63D}, //mathematical sans-serif bold italic capital b%
	{"\\mbfitsansC",	0x1D63E}, //mathematical sans-serif bold italic capital c%
	{"\\mbfitsansD",	0x1D63F}, //mathematical sans-serif bold italic capital d%
	{"\\mbfitsansE",	0x1D640}, //mathematical sans-serif bold italic capital e%
	{"\\mbfitsansF",	0x1D641}, //mathematical sans-serif bold italic capital f%
	{"\\mbfitsansG",	0x1D642}, //mathematical sans-serif bold italic capital g%
	{"\\mbfitsansH",	0x1D643}, //mathematical sans-serif bold italic capital h%
	{"\\mbfitsansI",	0x1D644}, //mathematical sans-serif bold italic capital i%
	{"\\mbfitsansJ",	0x1D645}, //mathematical sans-serif bold italic capital j%
	{"\\mbfitsansK",	0x1D646}, //mathematical sans-serif bold italic capital k%
	{"\\mbfitsansL",	0x1D647}, //mathematical sans-serif bold italic capital l%
	{"\\mbfitsansM",	0x1D648}, //mathematical sans-serif bold italic capital m%
	{"\\mbfitsansN",	0x1D649}, //mathematical sans-serif bold italic capital n%
	{"\\mbfitsansO",	0x1D64A}, //mathematical sans-serif bold italic capital o%
	{"\\mbfitsansP",	0x1D64B}, //mathematical sans-serif bold italic capital p%
	{"\\mbfitsansQ",	0x1D64C}, //mathematical sans-serif bold italic capital q%
	{"\\mbfitsansR",	0x1D64D}, //mathematical sans-serif bold italic capital r%
	{"\\mbfitsansS",	0x1D64E}, //mathematical sans-serif bold italic capital s%
	{"\\mbfitsansT",	0x1D64F}, //mathematical sans-serif bold italic capital t%
	{"\\mbfitsansU",	0x1D650}, //mathematical sans-serif bold italic capital u%
	{"\\mbfitsansV",	0x1D651}, //mathematical sans-serif bold italic capital v%
	{"\\mbfitsansW",	0x1D652}, //mathematical sans-serif bold italic capital w%
	{"\\mbfitsansX",	0x1D653}, //mathematical sans-serif bold italic capital x%
	{"\\mbfitsansY",	0x1D654}, //mathematical sans-serif bold italic capital y%
	{"\\mbfitsansZ",	0x1D655}, //mathematical sans-serif bold italic capital z%
	{"\\mbfitsansa",	0x1D656}, //mathematical sans-serif bold italic small a%
	{"\\mbfitsansb",	0x1D657}, //mathematical sans-serif bold italic small b%
	{"\\mbfitsansc",	0x1D658}, //mathematical sans-serif bold italic small c%
	{"\\mbfitsansd",	0x1D659}, //mathematical sans-serif bold italic small d%
	{"\\mbfitsanse",	0x1D65A}, //mathematical sans-serif bold italic small e%
	{"\\mbfitsansf",	0x1D65B}, //mathematical sans-serif bold italic small f%
	{"\\mbfitsansg",	0x1D65C}, //mathematical sans-serif bold italic small g%
	{"\\mbfitsansh",	0x1D65D}, //mathematical sans-serif bold italic small h%
	{"\\mbfitsansi",	0x1D65E}, //mathematical sans-serif bold italic small i%
	{"\\mbfitsansj",	0x1D65F}, //mathematical sans-serif bold italic small j%
	{"\\mbfitsansk",	0x1D660}, //mathematical sans-serif bold italic small k%
	{"\\mbfitsansl",	0x1D661}, //mathematical sans-serif bold italic small l%
	{"\\mbfitsansm",	0x1D662}, //mathematical sans-serif bold italic small m%
	{"\\mbfitsansn",	0x1D663}, //mathematical sans-serif bold italic small n%
	{"\\mbfitsanso",	0x1D664}, //mathematical sans-serif bold italic small o%
	{"\\mbfitsansp",	0x1D665}, //mathematical sans-serif bold italic small p%
	{"\\mbfitsansq",	0x1D666}, //mathematical sans-serif bold italic small q%
	{"\\mbfitsansr",	0x1D667}, //mathematical sans-serif bold italic small r%
	{"\\mbfitsanss",	0x1D668}, //mathematical sans-serif bold italic small s%
	{"\\mbfitsanst",	0x1D669}, //mathematical sans-serif bold italic small t%
	{"\\mbfitsansu",	0x1D66A}, //mathematical sans-serif bold italic small u%
	{"\\mbfitsansv",	0x1D66B}, //mathematical sans-serif bold italic small v%
	{"\\mbfitsansw",	0x1D66C}, //mathematical sans-serif bold italic small w%
	{"\\mbfitsansx",	0x1D66D}, //mathematical sans-serif bold italic small x%
	{"\\mbfitsansy",	0x1D66E}, //mathematical sans-serif bold italic small y%
	{"\\mbfitsansz",	0x1D66F}, //mathematical sans-serif bold italic small z%
	{"\\mttA",	0x1D670}, //mathematical monospace capital a%
	{"\\mttB",	0x1D671}, //mathematical monospace capital b%
	{"\\mttC",	0x1D672}, //mathematical monospace capital c%
	{"\\mttD",	0x1D673}, //mathematical monospace capital d%
	{"\\mttE",	0x1D674}, //mathematical monospace capital e%
	{"\\mttF",	0x1D675}, //mathematical monospace capital f%
	{"\\mttG",	0x1D676}, //mathematical monospace capital g%
	{"\\mttH",	0x1D677}, //mathematical monospace capital h%
	{"\\mttI",	0x1D678}, //mathematical monospace capital i%
	{"\\mttJ",	0x1D679}, //mathematical monospace capital j%
	{"\\mttK",	0x1D67A}, //mathematical monospace capital k%
	{"\\mttL",	0x1D67B}, //mathematical monospace capital l%
	{"\\mttM",	0x1D67C}, //mathematical monospace capital m%
	{"\\mttN",	0x1D67D}, //mathematical monospace capital n%
	{"\\mttO",	0x1D67E}, //mathematical monospace capital o%
	{"\\mttP",	0x1D67F}, //mathematical monospace capital p%
	{"\\mttQ",	0x1D680}, //mathematical monospace capital q%
	{"\\mttR",	0x1D681}, //mathematical monospace capital r%
	{"\\mttS",	0x1D682}, //mathematical monospace capital s%
	{"\\mttT",	0x1D683}, //mathematical monospace capital t%
	{"\\mttU",	0x1D684}, //mathematical monospace capital u%
	{"\\mttV",	0x1D685}, //mathematical monospace capital v%
	{"\\mttW",	0x1D686}, //mathematical monospace capital w%
	{"\\mttX",	0x1D687}, //mathematical monospace capital x%
	{"\\mttY",	0x1D688}, //mathematical monospace capital y%
	{"\\mttZ",	0x1D689}, //mathematical monospace capital z%
	{"\\mtta",	0x1D68A}, //mathematical monospace small a%
	{"\\mttb",	0x1D68B}, //mathematical monospace small b%
	{"\\mttc",	0x1D68C}, //mathematical monospace small c%
	{"\\mttd",	0x1D68D}, //mathematical monospace small d%
	{"\\mtte",	0x1D68E}, //mathematical monospace small e%
	{"\\mttf",	0x1D68F}, //mathematical monospace small f%
	{"\\mttg",	0x1D690}, //mathematical monospace small g%
	{"\\mtth",	0x1D691}, //mathematical monospace small h%
	{"\\mtti",	0x1D692}, //mathematical monospace small i%
	{"\\mttj",	0x1D693}, //mathematical monospace small j%
	{"\\mttk",	0x1D694}, //mathematical monospace small k%
	{"\\mttl",	0x1D695}, //mathematical monospace small l%
	{"\\mttm",	0x1D696}, //mathematical monospace small m%
	{"\\mttn",	0x1D697}, //mathematical monospace small n%
	{"\\mtto",	0x1D698}, //mathematical monospace small o%
	{"\\mttp",	0x1D699}, //mathematical monospace small p%
	{"\\mttq",	0x1D69A}, //mathematical monospace small q%
	{"\\mttr",	0x1D69B}, //mathematical monospace small r%
	{"\\mtts",	0x1D69C}, //mathematical monospace small s%
	{"\\mttt",	0x1D69D}, //mathematical monospace small t%
	{"\\mttu",	0x1D69E}, //mathematical monospace small u%
	{"\\mttv",	0x1D69F}, //mathematical monospace small v%
	{"\\mttw",	0x1D6A0}, //mathematical monospace small w%
	{"\\mttx",	0x1D6A1}, //mathematical monospace small x%
	{"\\mtty",	0x1D6A2}, //mathematical monospace small y%
	{"\\mttz",	0x1D6A3}, //mathematical monospace small z%
	{"\\mbfAlpha",	0x1D6A8}, //mathematical bold capital alpha%
	{"\\mbfBeta",	0x1D6A9}, //mathematical bold capital beta%
	{"\\mbfGamma",	0x1D6AA}, //mathematical bold capital gamma%
	{"\\mbfDelta",	0x1D6AB}, //mathematical bold capital delta%
	{"\\mbfEpsilon",	0x1D6AC}, //mathematical bold capital epsilon%
	{"\\mbfZeta",	0x1D6AD}, //mathematical bold capital zeta%
	{"\\mbfEta",	0x1D6AE}, //mathematical bold capital eta%
	{"\\mbfTheta",	0x1D6AF}, //mathematical bold capital theta%
	{"\\mbfIota",	0x1D6B0}, //mathematical bold capital iota%
	{"\\mbfKappa",	0x1D6B1}, //mathematical bold capital kappa%
	{"\\mbfLambda",	0x1D6B2}, //mathematical bold capital lambda%
	{"\\mbfMu",	0x1D6B3}, //mathematical bold capital mu%
	{"\\mbfNu",	0x1D6B4}, //mathematical bold capital nu%
	{"\\mbfXi",	0x1D6B5}, //mathematical bold capital xi%
	{"\\mbfOmicron",	0x1D6B6}, //mathematical bold capital omicron%
	{"\\mbfPi",	0x1D6B7}, //mathematical bold capital pi%
	{"\\mbfRho",	0x1D6B8}, //mathematical bold capital rho%
	{"\\mbfvarTheta",	0x1D6B9}, //mathematical bold capital theta symbol%
	{"\\mbfSigma",	0x1D6BA}, //mathematical bold capital sigma%
	{"\\mbfTau",	0x1D6BB}, //mathematical bold capital tau%
	{"\\mbfUpsilon",	0x1D6BC}, //mathematical bold capital upsilon%
	{"\\mbfPhi",	0x1D6BD}, //mathematical bold capital phi%
	{"\\mbfChi",	0x1D6BE}, //mathematical bold capital chi%
	{"\\mbfPsi",	0x1D6BF}, //mathematical bold capital psi%
	{"\\mbfOmega",	0x1D6C0}, //mathematical bold capital omega%
	{"\\mbfnabla",	0x1D6C1}, //mathematical bold nabla%
	{"\\mbfalpha",	0x1D6C2}, //mathematical bold small alpha%
	{"\\mbfbeta",	0x1D6C3}, //mathematical bold small beta%
	{"\\mbfgamma",	0x1D6C4}, //mathematical bold small gamma%
	{"\\mbfdelta",	0x1D6C5}, //mathematical bold small delta%
	{"\\mbfvarepsilon",	0x1D6C6}, //mathematical bold small varepsilon%
	{"\\mbfzeta",	0x1D6C7}, //mathematical bold small zeta%
	{"\\mbfeta",	0x1D6C8}, //mathematical bold small eta%
	{"\\mbftheta",	0x1D6C9}, //mathematical bold small theta%
	{"\\mbfiota",	0x1D6CA}, //mathematical bold small iota%
	{"\\mbfkappa",	0x1D6CB}, //mathematical bold small kappa%
	{"\\mbflambda",	0x1D6CC}, //mathematical bold small lambda%
	{"\\mbfmu",	0x1D6CD}, //mathematical bold small mu%
	{"\\mbfnu",	0x1D6CE}, //mathematical bold small nu%
	{"\\mbfxi",	0x1D6CF}, //mathematical bold small xi%
	{"\\mbfomicron",	0x1D6D0}, //mathematical bold small omicron%
	{"\\mbfpi",	0x1D6D1}, //mathematical bold small pi%
	{"\\mbfrho",	0x1D6D2}, //mathematical bold small rho%
	{"\\mbfvarsigma",	0x1D6D3}, //mathematical bold small final sigma%
	{"\\mbfsigma",	0x1D6D4}, //mathematical bold small sigma%
	{"\\mbftau",	0x1D6D5}, //mathematical bold small tau%
	{"\\mbfupsilon",	0x1D6D6}, //mathematical bold small upsilon%
	{"\\mbfvarphi",	0x1D6D7}, //mathematical bold small phi%
	{"\\mbfchi",	0x1D6D8}, //mathematical bold small chi%
	{"\\mbfpsi",	0x1D6D9}, //mathematical bold small psi%
	{"\\mbfomega",	0x1D6DA}, //mathematical bold small omega%
	{"\\mbfpartial",	0x1D6DB}, //mathematical bold partial differential%
	{"\\mbfepsilon",	0x1D6DC}, //mathematical bold varepsilon symbol%
	{"\\mbfvartheta",	0x1D6DD}, //mathematical bold theta symbol%
	{"\\mbfvarkappa",	0x1D6DE}, //mathematical bold kappa symbol%
	{"\\mbfphi",	0x1D6DF}, //mathematical bold phi symbol%
	{"\\mbfvarrho",	0x1D6E0}, //mathematical bold rho symbol%
	{"\\mbfvarpi",	0x1D6E1}, //mathematical bold pi symbol%
	{"\\mitAlpha",	0x1D6E2}, //mathematical italic capital alpha%
	{"\\mitBeta",	0x1D6E3}, //mathematical italic capital beta%
	{"\\mitGamma",	0x1D6E4}, //mathematical italic capital gamma%
	{"\\mitDelta",	0x1D6E5}, //mathematical italic capital delta%
	{"\\mitEpsilon",	0x1D6E6}, //mathematical italic capital epsilon%
	{"\\mitZeta",	0x1D6E7}, //mathematical italic capital zeta%
	{"\\mitEta",	0x1D6E8}, //mathematical italic capital eta%
	{"\\mitTheta",	0x1D6E9}, //mathematical italic capital theta%
	{"\\mitIota",	0x1D6EA}, //mathematical italic capital iota%
	{"\\mitKappa",	0x1D6EB}, //mathematical italic capital kappa%
	{"\\mitLambda",	0x1D6EC}, //mathematical italic capital lambda%
	{"\\mitMu",	0x1D6ED}, //mathematical italic capital mu%
	{"\\mitNu",	0x1D6EE}, //mathematical italic capital nu%
	{"\\mitXi",	0x1D6EF}, //mathematical italic capital xi%
	{"\\mitOmicron",	0x1D6F0}, //mathematical italic capital omicron%
	{"\\mitPi",	0x1D6F1}, //mathematical italic capital pi%
	{"\\mitRho",	0x1D6F2}, //mathematical italic capital rho%
	{"\\mitvarTheta",	0x1D6F3}, //mathematical italic capital theta symbol%
	{"\\mitSigma",	0x1D6F4}, //mathematical italic capital sigma%
	{"\\mitTau",	0x1D6F5}, //mathematical italic capital tau%
	{"\\mitUpsilon",	0x1D6F6}, //mathematical italic capital upsilon%
	{"\\mitPhi",	0x1D6F7}, //mathematical italic capital phi%
	{"\\mitChi",	0x1D6F8}, //mathematical italic capital chi%
	{"\\mitPsi",	0x1D6F9}, //mathematical italic capital psi%
	{"\\mitOmega",	0x1D6FA}, //mathematical italic capital omega%
	{"\\mitnabla",	0x1D6FB}, //mathematical italic nabla%
	{"\\mitalpha",	0x1D6FC}, //mathematical italic small alpha%
	{"\\mitbeta",	0x1D6FD}, //mathematical italic small beta%
	{"\\mitgamma",	0x1D6FE}, //mathematical italic small gamma%
	{"\\mitdelta",	0x1D6FF}, //mathematical italic small delta%
	{"\\mitvarepsilon",	0x1D700}, //mathematical italic small varepsilon%
	{"\\mitzeta",	0x1D701}, //mathematical italic small zeta%
	{"\\miteta",	0x1D702}, //mathematical italic small eta%
	{"\\mittheta",	0x1D703}, //mathematical italic small theta%
	{"\\mitiota",	0x1D704}, //mathematical italic small iota%
	{"\\mitkappa",	0x1D705}, //mathematical italic small kappa%
	{"\\mitlambda",	0x1D706}, //mathematical italic small lambda%
	{"\\mitmu",	0x1D707}, //mathematical italic small mu%
	{"\\mitnu",	0x1D708}, //mathematical italic small nu%
	{"\\mitxi",	0x1D709}, //mathematical italic small xi%
	{"\\mitomicron",	0x1D70A}, //mathematical italic small omicron%
	{"\\mitpi",	0x1D70B}, //mathematical italic small pi%
	{"\\mitrho",	0x1D70C}, //mathematical italic small rho%
	{"\\mitvarsigma",	0x1D70D}, //mathematical italic small final sigma%
	{"\\mitsigma",	0x1D70E}, //mathematical italic small sigma%
	{"\\mittau",	0x1D70F}, //mathematical italic small tau%
	{"\\mitupsilon",	0x1D710}, //mathematical italic small upsilon%
	{"\\mitvarphi",	0x1D711}, //mathematical italic small phi%
	{"\\mitchi",	0x1D712}, //mathematical italic small chi%
	{"\\mitpsi",	0x1D713}, //mathematical italic small psi%
	{"\\mitomega",	0x1D714}, //mathematical italic small omega%
	{"\\mitpartial",	0x1D715}, //mathematical italic partial differential%
	{"\\mitepsilon",	0x1D716}, //mathematical italic varepsilon symbol%
	{"\\mitvartheta",	0x1D717}, //mathematical italic theta symbol%
	{"\\mitvarkappa",	0x1D718}, //mathematical italic kappa symbol%
	{"\\mitphi",	0x1D719}, //mathematical italic phi symbol%
	{"\\mitvarrho",	0x1D71A}, //mathematical italic rho symbol%
	{"\\mitvarpi",	0x1D71B}, //mathematical italic pi symbol%
	{"\\mbfitAlpha",	0x1D71C}, //mathematical bold italic capital alpha%
	{"\\mbfitBeta",	0x1D71D}, //mathematical bold italic capital beta%
	{"\\mbfitGamma",	0x1D71E}, //mathematical bold italic capital gamma%
	{"\\mbfitDelta",	0x1D71F}, //mathematical bold italic capital delta%
	{"\\mbfitEpsilon",	0x1D720}, //mathematical bold italic capital epsilon%
	{"\\mbfitZeta",	0x1D721}, //mathematical bold italic capital zeta%
	{"\\mbfitEta",	0x1D722}, //mathematical bold italic capital eta%
	{"\\mbfitTheta",	0x1D723}, //mathematical bold italic capital theta%
	{"\\mbfitIota",	0x1D724}, //mathematical bold italic capital iota%
	{"\\mbfitKappa",	0x1D725}, //mathematical bold italic capital kappa%
	{"\\mbfitLambda",	0x1D726}, //mathematical bold italic capital lambda%
	{"\\mbfitMu",	0x1D727}, //mathematical bold italic capital mu%
	{"\\mbfitNu",	0x1D728}, //mathematical bold italic capital nu%
	{"\\mbfitXi",	0x1D729}, //mathematical bold italic capital xi%
	{"\\mbfitOmicron",	0x1D72A}, //mathematical bold italic capital omicron%
	{"\\mbfitPi",	0x1D72B}, //mathematical bold italic capital pi%
	{"\\mbfitRho",	0x1D72C}, //mathematical bold italic capital rho%
	{"\\mbfitvarTheta",	0x1D72D}, //mathematical bold italic capital theta symbol%
	{"\\mbfitSigma",	0x1D72E}, //mathematical bold italic capital sigma%
	{"\\mbfitTau",	0x1D72F}, //mathematical bold italic capital tau%
	{"\\mbfitUpsilon",	0x1D730}, //mathematical bold italic capital upsilon%
	{"\\mbfitPhi",	0x1D731}, //mathematical bold italic capital phi%
	{"\\mbfitChi",	0x1D732}, //mathematical bold italic capital chi%
	{"\\mbfitPsi",	0x1D733}, //mathematical bold italic capital psi%
	{"\\mbfitOmega",	0x1D734}, //mathematical bold italic capital omega%
	{"\\mbfitnabla",	0x1D735}, //mathematical bold italic nabla%
	{"\\mbfitalpha",	0x1D736}, //mathematical bold italic small alpha%
	{"\\mbfitbeta",	0x1D737}, //mathematical bold italic small beta%
	{"\\mbfitgamma",	0x1D738}, //mathematical bold italic small gamma%
	{"\\mbfitdelta",	0x1D739}, //mathematical bold italic small delta%
	{"\\mbfitvarepsilon",	0x1D73A}, //mathematical bold italic small varepsilon%
	{"\\mbfitzeta",	0x1D73B}, //mathematical bold italic small zeta%
	{"\\mbfiteta",	0x1D73C}, //mathematical bold italic small eta%
	{"\\mbfittheta",	0x1D73D}, //mathematical bold italic small theta%
	{"\\mbfitiota",	0x1D73E}, //mathematical bold italic small iota%
	{"\\mbfitkappa",	0x1D73F}, //mathematical bold italic small kappa%
	{"\\mbfitlambda",	0x1D740}, //mathematical bold italic small lambda%
	{"\\mbfitmu",	0x1D741}, //mathematical bold italic small mu%
	{"\\mbfitnu",	0x1D742}, //mathematical bold italic small nu%
	{"\\mbfitxi",	0x1D743}, //mathematical bold italic small xi%
	{"\\mbfitomicron",	0x1D744}, //mathematical bold italic small omicron%
	{"\\mbfitpi",	0x1D745}, //mathematical bold italic small pi%
	{"\\mbfitrho",	0x1D746}, //mathematical bold italic small rho%
	{"\\mbfitvarsigma",	0x1D747}, //mathematical bold italic small final sigma%
	{"\\mbfitsigma",	0x1D748}, //mathematical bold italic small sigma%
	{"\\mbfittau",	0x1D749}, //mathematical bold italic small tau%
	{"\\mbfitupsilon",	0x1D74A}, //mathematical bold italic small upsilon%
	{"\\mbfitvarphi",	0x1D74B}, //mathematical bold italic small phi%
	{"\\mbfitchi",	0x1D74C}, //mathematical bold italic small chi%
	{"\\mbfitpsi",	0x1D74D}, //mathematical bold italic small psi%
	{"\\mbfitomega",	0x1D74E}, //mathematical bold italic small omega%
	{"\\mbfitpartial",	0x1D74F}, //mathematical bold italic partial differential%
	{"\\mbfitepsilon",	0x1D750}, //mathematical bold italic varepsilon symbol%
	{"\\mbfitvartheta",	0x1D751}, //mathematical bold italic theta symbol%
	{"\\mbfitvarkappa",	0x1D752}, //mathematical bold italic kappa symbol%
	{"\\mbfitphi",	0x1D753}, //mathematical bold italic phi symbol%
	{"\\mbfitvarrho",	0x1D754}, //mathematical bold italic rho symbol%
	{"\\mbfitvarpi",	0x1D755}, //mathematical bold italic pi symbol%
	{"\\mbfsansAlpha",	0x1D756}, //mathematical sans-serif bold capital alpha%
	{"\\mbfsansBeta",	0x1D757}, //mathematical sans-serif bold capital beta%
	{"\\mbfsansGamma",	0x1D758}, //mathematical sans-serif bold capital gamma%
	{"\\mbfsansDelta",	0x1D759}, //mathematical sans-serif bold capital delta%
	{"\\mbfsansEpsilon",	0x1D75A}, //mathematical sans-serif bold capital epsilon%
	{"\\mbfsansZeta",	0x1D75B}, //mathematical sans-serif bold capital zeta%
	{"\\mbfsansEta",	0x1D75C}, //mathematical sans-serif bold capital eta%
	{"\\mbfsansTheta",	0x1D75D}, //mathematical sans-serif bold capital theta%
	{"\\mbfsansIota",	0x1D75E}, //mathematical sans-serif bold capital iota%
	{"\\mbfsansKappa",	0x1D75F}, //mathematical sans-serif bold capital kappa%
	{"\\mbfsansLambda",	0x1D760}, //mathematical sans-serif bold capital lambda%
	{"\\mbfsansMu",	0x1D761}, //mathematical sans-serif bold capital mu%
	{"\\mbfsansNu",	0x1D762}, //mathematical sans-serif bold capital nu%
	{"\\mbfsansXi",	0x1D763}, //mathematical sans-serif bold capital xi%
	{"\\mbfsansOmicron",	0x1D764}, //mathematical sans-serif bold capital omicron%
	{"\\mbfsansPi",	0x1D765}, //mathematical sans-serif bold capital pi%
	{"\\mbfsansRho",	0x1D766}, //mathematical sans-serif bold capital rho%
	{"\\mbfsansvarTheta",	0x1D767}, //mathematical sans-serif bold capital theta symbol%
	{"\\mbfsansSigma",	0x1D768}, //mathematical sans-serif bold capital sigma%
	{"\\mbfsansTau",	0x1D769}, //mathematical sans-serif bold capital tau%
	{"\\mbfsansUpsilon",	0x1D76A}, //mathematical sans-serif bold capital upsilon%
	{"\\mbfsansPhi",	0x1D76B}, //mathematical sans-serif bold capital phi%
	{"\\mbfsansChi",	0x1D76C}, //mathematical sans-serif bold capital chi%
	{"\\mbfsansPsi",	0x1D76D}, //mathematical sans-serif bold capital psi%
	{"\\mbfsansOmega",	0x1D76E}, //mathematical sans-serif bold capital omega%
	{"\\mbfsansnabla",	0x1D76F}, //mathematical sans-serif bold nabla%
	{"\\mbfsansalpha",	0x1D770}, //mathematical sans-serif bold small alpha%
	{"\\mbfsansbeta",	0x1D771}, //mathematical sans-serif bold small beta%
	{"\\mbfsansgamma",	0x1D772}, //mathematical sans-serif bold small gamma%
	{"\\mbfsansdelta",	0x1D773}, //mathematical sans-serif bold small delta%
	{"\\mbfsansvarepsilon",	0x1D774}, //mathematical sans-serif bold small varepsilon%
	{"\\mbfsanszeta",	0x1D775}, //mathematical sans-serif bold small zeta%
	{"\\mbfsanseta",	0x1D776}, //mathematical sans-serif bold small eta%
	{"\\mbfsanstheta",	0x1D777}, //mathematical sans-serif bold small theta%
	{"\\mbfsansiota",	0x1D778}, //mathematical sans-serif bold small iota%
	{"\\mbfsanskappa",	0x1D779}, //mathematical sans-serif bold small kappa%
	{"\\mbfsanslambda",	0x1D77A}, //mathematical sans-serif bold small lambda%
	{"\\mbfsansmu",	0x1D77B}, //mathematical sans-serif bold small mu%
	{"\\mbfsansnu",	0x1D77C}, //mathematical sans-serif bold small nu%
	{"\\mbfsansxi",	0x1D77D}, //mathematical sans-serif bold small xi%
	{"\\mbfsansomicron",	0x1D77E}, //mathematical sans-serif bold small omicron%
	{"\\mbfsanspi",	0x1D77F}, //mathematical sans-serif bold small pi%
	{"\\mbfsansrho",	0x1D780}, //mathematical sans-serif bold small rho%
	{"\\mbfsansvarsigma",	0x1D781}, //mathematical sans-serif bold small final sigma%
	{"\\mbfsanssigma",	0x1D782}, //mathematical sans-serif bold small sigma%
	{"\\mbfsanstau",	0x1D783}, //mathematical sans-serif bold small tau%
	{"\\mbfsansupsilon",	0x1D784}, //mathematical sans-serif bold small upsilon%
	{"\\mbfsansvarphi",	0x1D785}, //mathematical sans-serif bold small phi%
	{"\\mbfsanschi",	0x1D786}, //mathematical sans-serif bold small chi%
	{"\\mbfsanspsi",	0x1D787}, //mathematical sans-serif bold small psi%
	{"\\mbfsansomega",	0x1D788}, //mathematical sans-serif bold small omega%
	{"\\mbfsanspartial",	0x1D789}, //mathematical sans-serif bold partial differential%
	{"\\mbfsansepsilon",	0x1D78A}, //mathematical sans-serif bold varepsilon symbol%
	{"\\mbfsansvartheta",	0x1D78B}, //mathematical sans-serif bold theta symbol%
	{"\\mbfsansvarkappa",	0x1D78C}, //mathematical sans-serif bold kappa symbol%
	{"\\mbfsansphi",	0x1D78D}, //mathematical sans-serif bold phi symbol%
	{"\\mbfsansvarrho",	0x1D78E}, //mathematical sans-serif bold rho symbol%
	{"\\mbfsansvarpi",	0x1D78F}, //mathematical sans-serif bold pi symbol%
	{"\\mbfitsansAlpha",	0x1D790}, //mathematical sans-serif bold italic capital alpha%
	{"\\mbfitsansBeta",	0x1D791}, //mathematical sans-serif bold italic capital beta%
	{"\\mbfitsansGamma",	0x1D792}, //mathematical sans-serif bold italic capital gamma%
	{"\\mbfitsansDelta",	0x1D793}, //mathematical sans-serif bold italic capital delta%
	{"\\mbfitsansEpsilon",	0x1D794}, //mathematical sans-serif bold italic capital epsilon%
	{"\\mbfitsansZeta",	0x1D795}, //mathematical sans-serif bold italic capital zeta%
	{"\\mbfitsansEta",	0x1D796}, //mathematical sans-serif bold italic capital eta%
	{"\\mbfitsansTheta",	0x1D797}, //mathematical sans-serif bold italic capital theta%
	{"\\mbfitsansIota",	0x1D798}, //mathematical sans-serif bold italic capital iota%
	{"\\mbfitsansKappa",	0x1D799}, //mathematical sans-serif bold italic capital kappa%
	{"\\mbfitsansLambda",	0x1D79A}, //mathematical sans-serif bold italic capital lambda%
	{"\\mbfitsansMu",	0x1D79B}, //mathematical sans-serif bold italic capital mu%
	{"\\mbfitsansNu",	0x1D79C}, //mathematical sans-serif bold italic capital nu%
	{"\\mbfitsansXi",	0x1D79D}, //mathematical sans-serif bold italic capital xi%
	{"\\mbfitsansOmicron",	0x1D79E}, //mathematical sans-serif bold italic capital omicron%
	{"\\mbfitsansPi",	0x1D79F}, //mathematical sans-serif bold italic capital pi%
	{"\\mbfitsansRho",	0x1D7A0}, //mathematical sans-serif bold italic capital rho%
	{"\\mbfitsansvarTheta",	0x1D7A1}, //mathematical sans-serif bold italic capital theta symbol%
	{"\\mbfitsansSigma",	0x1D7A2}, //mathematical sans-serif bold italic capital sigma%
	{"\\mbfitsansTau",	0x1D7A3}, //mathematical sans-serif bold italic capital tau%
	{"\\mbfitsansUpsilon",	0x1D7A4}, //mathematical sans-serif bold italic capital upsilon%
	{"\\mbfitsansPhi",	0x1D7A5}, //mathematical sans-serif bold italic capital phi%
	{"\\mbfitsansChi",	0x1D7A6}, //mathematical sans-serif bold italic capital chi%
	{"\\mbfitsansPsi",	0x1D7A7}, //mathematical sans-serif bold italic capital psi%
	{"\\mbfitsansOmega",	0x1D7A8}, //mathematical sans-serif bold italic capital omega%
	{"\\mbfitsansnabla",	0x1D7A9}, //mathematical sans-serif bold italic nabla%
	{"\\mbfitsansalpha",	0x1D7AA}, //mathematical sans-serif bold italic small alpha%
	{"\\mbfitsansbeta",	0x1D7AB}, //mathematical sans-serif bold italic small beta%
	{"\\mbfitsansgamma",	0x1D7AC}, //mathematical sans-serif bold italic small gamma%
	{"\\mbfitsansdelta",	0x1D7AD}, //mathematical sans-serif bold italic small delta%
	{"\\mbfitsansvarepsilon",	0x1D7AE}, //mathematical sans-serif bold italic small varepsilon%
	{"\\mbfitsanszeta",	0x1D7AF}, //mathematical sans-serif bold italic small zeta%
	{"\\mbfitsanseta",	0x1D7B0}, //mathematical sans-serif bold italic small eta%
	{"\\mbfitsanstheta",	0x1D7B1}, //mathematical sans-serif bold italic small theta%
	{"\\mbfitsansiota",	0x1D7B2}, //mathematical sans-serif bold italic small iota%
	{"\\mbfitsanskappa",	0x1D7B3}, //mathematical sans-serif bold italic small kappa%
	{"\\mbfitsanslambda",	0x1D7B4}, //mathematical sans-serif bold italic small lambda%
	{"\\mbfitsansmu",	0x1D7B5}, //mathematical sans-serif bold italic small mu%
	{"\\mbfitsansnu",	0x1D7B6}, //mathematical sans-serif bold italic small nu%
	{"\\mbfitsansxi",	0x1D7B7}, //mathematical sans-serif bold italic small xi%
	{"\\mbfitsansomicron",	0x1D7B8}, //mathematical sans-serif bold italic small omicron%
	{"\\mbfitsanspi",	0x1D7B9}, //mathematical sans-serif bold italic small pi%
	{"\\mbfitsansrho",	0x1D7BA}, //mathematical sans-serif bold italic small rho%
	{"\\mbfitsansvarsigma",	0x1D7BB}, //mathematical sans-serif bold italic small final sigma%
	{"\\mbfitsanssigma",	0x1D7BC}, //mathematical sans-serif bold italic small sigma%
	{"\\mbfitsanstau",	0x1D7BD}, //mathematical sans-serif bold italic small tau%
	{"\\mbfitsansupsilon",	0x1D7BE}, //mathematical sans-serif bold italic small upsilon%
	{"\\mbfitsansvarphi",	0x1D7BF}, //mathematical sans-serif bold italic small phi%
	{"\\mbfitsanschi",	0x1D7C0}, //mathematical sans-serif bold italic small chi%
	{"\\mbfitsanspsi",	0x1D7C1}, //mathematical sans-serif bold italic small psi%
	{"\\mbfitsansomega",	0x1D7C2}, //mathematical sans-serif bold italic small omega%
	{"\\mbfitsanspartial",	0x1D7C3}, //mathematical sans-serif bold italic partial differential%
	{"\\mbfitsansepsilon",	0x1D7C4}, //mathematical sans-serif bold italic varepsilon symbol%
	{"\\mbfitsansvartheta",	0x1D7C5}, //mathematical sans-serif bold italic theta symbol%
	{"\\mbfitsansvarkappa",	0x1D7C6}, //mathematical sans-serif bold italic kappa symbol%
	{"\\mbfitsansphi",	0x1D7C7}, //mathematical sans-serif bold italic phi symbol%
	{"\\mbfitsansvarrho",	0x1D7C8}, //mathematical sans-serif bold italic rho symbol%
	{"\\mbfitsansvarpi",	0x1D7C9}, //mathematical sans-serif bold italic pi symbol%
	{"\\mbfDigamma",	0x1D7CA}, //mathematical bold capital digamma%
	{"\\mbfdigamma",	0x1D7CB}, //mathematical bold small digamma%
	{"\\mbfzero",	0x1D7CE}, //mathematical bold digit 0%
	{"\\mbfone",	0x1D7CF}, //mathematical bold digit 1%
	{"\\mbftwo",	0x1D7D0}, //mathematical bold digit 2%
	{"\\mbfthree",	0x1D7D1}, //mathematical bold digit 3%
	{"\\mbffour",	0x1D7D2}, //mathematical bold digit 4%
	{"\\mbffive",	0x1D7D3}, //mathematical bold digit 5%
	{"\\mbfsix",	0x1D7D4}, //mathematical bold digit 6%
	{"\\mbfseven",	0x1D7D5}, //mathematical bold digit 7%
	{"\\mbfeight",	0x1D7D6}, //mathematical bold digit 8%
	{"\\mbfnine",	0x1D7D7}, //mathematical bold digit 9%
	{"\\Bbbzero",	0x1D7D8}, //mathematical double-struck digit 0%
	{"\\Bbbone",	0x1D7D9}, //mathematical double-struck digit 1%
	{"\\Bbbtwo",	0x1D7DA}, //mathematical double-struck digit 2%
	{"\\Bbbthree",	0x1D7DB}, //mathematical double-struck digit 3%
	{"\\Bbbfour",	0x1D7DC}, //mathematical double-struck digit 4%
	{"\\Bbbfive",	0x1D7DD}, //mathematical double-struck digit 5%
	{"\\Bbbsix",	0x1D7DE}, //mathematical double-struck digit 6%
	{"\\Bbbseven",	0x1D7DF}, //mathematical double-struck digit 7%
	{"\\Bbbeight",	0x1D7E0}, //mathematical double-struck digit 8%
	{"\\Bbbnine",	0x1D7E1}, //mathematical double-struck digit 9%
	{"\\msanszero",	0x1D7E2}, //mathematical sans-serif digit 0%
	{"\\msansone",	0x1D7E3}, //mathematical sans-serif digit 1%
	{"\\msanstwo",	0x1D7E4}, //mathematical sans-serif digit 2%
	{"\\msansthree",	0x1D7E5}, //mathematical sans-serif digit 3%
	{"\\msansfour",	0x1D7E6}, //mathematical sans-serif digit 4%
	{"\\msansfive",	0x1D7E7}, //mathematical sans-serif digit 5%
	{"\\msanssix",	0x1D7E8}, //mathematical sans-serif digit 6%
	{"\\msansseven",	0x1D7E9}, //mathematical sans-serif digit 7%
	{"\\msanseight",	0x1D7EA}, //mathematical sans-serif digit 8%
	{"\\msansnine",	0x1D7EB}, //mathematical sans-serif digit 9%
	{"\\mbfsanszero",	0x1D7EC}, //mathematical sans-serif bold digit 0%
	{"\\mbfsansone",	0x1D7ED}, //mathematical sans-serif bold digit 1%
	{"\\mbfsanstwo",	0x1D7EE}, //mathematical sans-serif bold digit 2%
	{"\\mbfsansthree",	0x1D7EF}, //mathematical sans-serif bold digit 3%
	{"\\mbfsansfour",	0x1D7F0}, //mathematical sans-serif bold digit 4%
	{"\\mbfsansfive",	0x1D7F1}, //mathematical sans-serif bold digit 5%
	{"\\mbfsanssix",	0x1D7F2}, //mathematical sans-serif bold digit 6%
	{"\\mbfsansseven",	0x1D7F3}, //mathematical sans-serif bold digit 7%
	{"\\mbfsanseight",	0x1D7F4}, //mathematical sans-serif bold digit 8%
	{"\\mbfsansnine",	0x1D7F5}, //mathematical sans-serif bold digit 9%
	{"\\mttzero",	0x1D7F6}, //mathematical monospace digit 0%
	{"\\mttone",	0x1D7F7}, //mathematical monospace digit 1%
	{"\\mtttwo",	0x1D7F8}, //mathematical monospace digit 2%
	{"\\mttthree",	0x1D7F9}, //mathematical monospace digit 3%
	{"\\mttfour",	0x1D7FA}, //mathematical monospace digit 4%
	{"\\mttfive",	0x1D7FB}, //mathematical monospace digit 5%
	{"\\mttsix",	0x1D7FC}, //mathematical monospace digit 6%
	{"\\mttseven",	0x1D7FD}, //mathematical monospace digit 7%
	{"\\mtteight",	0x1D7FE}, //mathematical monospace digit 8%
	{"\\mttnine",	0x1D7FF}, //mathematical monospace digit 9%
	{"\\arabicmaj",	0x1EEF0}, //arabic mathematical operator meem with hah with tatweel%
	{"\\arabichad",	0x1EEF1}, //arabic mathematical operator hah with dal%
	{NULL,                     0x00000},
};

const Symbol * TEXPRINTF_SYMBOLS=Symbols;

typedef struct {
	char *name;
	float scale; // wscale is always in units monospace width
} LengthUnit;

float AspectRatio=2;
// table with length units
// note that in the end all lengths will be rounded to character width and height
// I base the lengthy units on the assumptions that:
// 1 pt =0.35 mm
// 10 pt character is approx. 3.5 mm high and 1.75 mm wide
// thus 1pt =0.35/1.75=0.2 characters wide 
const LengthUnit  Lengths[] = {
	// basic units
	{"pt", 0.200},
	{"mm", 0.571},
	{"cm", 5.714},
	{"in", 14.51},
	{"ex", 1.000},
	{"em", 1.000},
	{"mu", 0.056},
	{"sp", 3.052e-6},
	{"\\nulldelimiterspace", 0.0},
	{NULL, -1} // signal no known unit (units have to be positive!)
};


