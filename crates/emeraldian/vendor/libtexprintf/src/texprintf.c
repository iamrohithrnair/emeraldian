#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <stdarg.h>
#include "boxes.h"
#include "drawbox.h"
#include "parser.h"
#include "lexer.h"
#include "stringutils.h"
#include "error.h"

int TEXPRINTF_LW=0;
char *TEXPRINTF_FONT="text";
int TEXPRINTF_FCW=2;
int TEXPRINTF_WCW=2;

int TEXPRINTF_ERR=0;

int texprintf(const char *format, ...)
{
   	va_list ap;
   	char *str;
   	int Na=255, np;
	box root;

	ResetErrors();
	FCSPACES=TEXPRINTF_FCW;
	WCSPACES=TEXPRINTF_WCW;
	if ((WCSPACES<1)||(WCSPACES>2))
	{
		fprintf(stderr,"Error: TEXPRINTF_WCW out of range, wide characters can occupy either 1 or 2 character spaces\n");
		exit(1);
	}
	if ((FCSPACES<1)||(FCSPACES>2))
	{
		fprintf(stderr,"Error: TEXPRINTF_FCW out of range, wide characters can occupy either 1 or 2 character spaces\n");
		exit(1);
	}

   	str=malloc(Na*sizeof(char));
   	va_start (ap, format);
   	np=vsnprintf(str, Na*sizeof(char), format, ap);
   	while (np>=Na)
   	{
		Na+=255;
		str=realloc(str, Na*sizeof(char));
		va_start (ap, format);
		np=vsnprintf(str, Na*sizeof(char), format, ap);
	}

	root=ParseString(str, TEXPRINTF_LW, TEXPRINTF_FONT);
	BoxPos(&root);
	np=PrintBox(&root);
	FreeBox(&root);
	free(str);
	TEXPRINTF_ERR=ERRORSTATE;
	return np;
}

char * texstring(const char *str)
{
	box root;
	char *res;

	ResetErrors();
	FCSPACES=TEXPRINTF_FCW;
	WCSPACES=TEXPRINTF_WCW;
	if ((WCSPACES<1)||(WCSPACES>2))
	{
		fprintf(stderr,"Error: TEXPRINTF_WCW out of range, wide characters can occupy either 1 or 2 character spaces\n");
		exit(1);
	}
	if ((FCSPACES<1)||(FCSPACES>2))
	{
		fprintf(stderr,"Error: TEXPRINTF_FCW out of range, wide characters can occupy either 1 or 2 character spaces\n");
		exit(1);
	}

	root=ParseString(str, TEXPRINTF_LW, TEXPRINTF_FONT);
	BoxPos(&root);
	res=DrawBox(&root);
	FreeBox(&root);
	TEXPRINTF_ERR=ERRORSTATE;

	return res;
}


char * stexprintf(const char *format, ...)
{
   	va_list ap;
   	char *res;
   	char *str;
   	int Na=255, np;

   	str=malloc(Na*sizeof(char));
   	va_start (ap, format);
   	np=vsnprintf(str, Na*sizeof(char), format, ap);
   	while (np>=Na)
   	{
		Na+=255;
		str=realloc(str, Na*sizeof(char));
		va_start (ap, format);
		np=vsnprintf(str, Na*sizeof(char), format, ap);
	}

	res = texstring(str);
	free(str);
	return res;
}

void texfree(void *ptr)
{
    free(ptr);
}

int ftexprintf(FILE *f, const char *format, ...)
{
   	va_list ap;
   	char *res;
   	char *str;
   	int Na=255, np;
	box root;

	ResetErrors();
	FCSPACES=TEXPRINTF_FCW;
	WCSPACES=TEXPRINTF_WCW;
	if ((WCSPACES<1)||(WCSPACES>2))
	{
		fprintf(stderr,"Error: TEXPRINTF_WCW out of range, wide characters can occupy either 1 or 2 character spaces\n");
		exit(1);
	}
	if ((FCSPACES<1)||(FCSPACES>2))
	{
		fprintf(stderr,"Error: TEXPRINTF_FCW out of range, wide characters can occupy either 1 or 2 character spaces\n");
		exit(1);
	}

   	str=malloc(Na*sizeof(char));
   	va_start (ap, format);
   	np=vsnprintf(str, Na*sizeof(char), format, ap);
   	while (np>=Na)
   	{
		Na+=255;
		str=realloc(str, Na*sizeof(char));
		va_start (ap, format);
		np=vsnprintf(str, Na*sizeof(char), format, ap);
	}

	root=ParseString(str, TEXPRINTF_LW, TEXPRINTF_FONT);
	BoxPos(&root);
	res=DrawBox(&root);
	FreeBox(&root);
	free(str);
	np=strlen(res);
	fprintf(f,"%s",res);
	free(res);
	TEXPRINTF_ERR=ERRORSTATE;

	return np;
}

void texboxtree(const char *format, ...)
{
   	va_list ap;
   	char *str;
   	int Na=255, np;
	box root;

	ResetErrors();
	FCSPACES=TEXPRINTF_FCW;
	WCSPACES=TEXPRINTF_WCW;
	if ((WCSPACES<1)||(WCSPACES>2))
	{
		fprintf(stderr,"Error: TEXPRINTF_WCW out of range, wide characters can occupy either 1 or 2 character spaces\n");
		exit(1);
	}
	if ((FCSPACES<1)||(FCSPACES>2))
	{
		fprintf(stderr,"Error: TEXPRINTF_FCW out of range, wide characters can occupy either 1 or 2 character spaces\n");
		exit(1);
	}

   	str=malloc(Na*sizeof(char));
   	va_start (ap, format);
   	np=vsnprintf(str, Na*sizeof(char), format, ap);
   	while (np>=Na)
   	{
		Na+=255;
		str=realloc(str, Na*sizeof(char));
		va_start (ap, format);
		np=vsnprintf(str, Na*sizeof(char), format, ap);
	}

	root=ParseString(str, TEXPRINTF_LW, TEXPRINTF_FONT);
	BoxPos(&root);
	DrawBoxTree(&root);
	FreeBox(&root);
	free(str);
	TEXPRINTF_ERR=ERRORSTATE;

}

void texlistsymbols()
{
	ListSymbols();								/* link to lexer.o */
}

char *texsymbols_str()
{
	return Symbols_Str();								/* link to lexer.o */
}

void texerrors()
{
	ERRORSTATE=TEXPRINTF_ERR;
	E_Messages();						/* link to error.o */
	ResetErrors();
}

/* Like texerror(), but return "; "-delimited string instead of
   writing to stderr.  Caller should free return value with texfree. */
char *texerrors_str()
{
	char *s;
	ERRORSTATE=TEXPRINTF_ERR;
	s = E_Messages_str();						/* link to error.o */
	ResetErrors();
	return s;
}

int texerror_state()
{
	return TEXPRINTF_ERR;
}

void SetStyleASCII()
{
	style=&STYLE_ASC;
}
void SetStyleUNICODE()
{
	style=&STYLE_UNI;
}
void ToggleMapSuperSub()
{
	style->mapsupersub=!style->mapsupersub;
}
void ToggleAvoidCombining()
{
	style->avoidcombining=!style->avoidcombining;
}

void SetRootFont(const char *font)
{
	PRSDEF P;
	P=(int)LookupFont(font);
	switch(P)
	{
		case PD_TEXT:
			TEXPRINTF_FONT="text";
			break;		
		case PD_MATHBF:
			TEXPRINTF_FONT="mathbf";
			break;		
		case PD_MATHBFIT:
			TEXPRINTF_FONT="mathbfit";
			break;		
		case PD_MATHCAL:
			TEXPRINTF_FONT="mathcal";
			break;		
		case PD_MATHSCR:
			TEXPRINTF_FONT="mathscr";
			break;		
		case PD_MATHFRAK:
			TEXPRINTF_FONT="mathfrak";
			break;		
		case PD_MATHBB:
			TEXPRINTF_FONT="mathbb";
			break;		
		case PD_MATHSF:
			TEXPRINTF_FONT="mathsf";
			break;		
		case PD_MATHSFBF:
			TEXPRINTF_FONT="mathsfbf";
			break;		
		case PD_MATHSFIT:
			TEXPRINTF_FONT="mathsfit";
			break;		
		case PD_MATHSFBFIT:
			TEXPRINTF_FONT="mathsfbfit";
			break;		
		case PD_MATHTT:
			TEXPRINTF_FONT="mathtt";
			break;		
		case PD_MATHNORMAL:
			TEXPRINTF_FONT="mathnormal";
			break;		
		default:
			TEXPRINTF_FONT="unknown"; // will throw an error later
	}
}


