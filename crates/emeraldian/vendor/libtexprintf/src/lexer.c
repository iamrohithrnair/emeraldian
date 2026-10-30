#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <ctype.h>
#include <locale.h>
#include <math.h>
#include "lexer.h"
#include "parsedef.h"
#include "stringutils.h"
#include "error.h"
/* TODO:
	how to deal with '||' delimiters in Vmatrix and as a symbol ("\|")
*/


/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
// ERRORFLAG LEXPREMATUREEND "Premature end of string"


/* the list symbols routines list both symbols from the symbol table
 * and combining diacritical marks
 */
#define MAXSTRLEN 100000
void ListSymbols()
{
	int j=0, i, k;
	int l,lm;
	char *str, *p;
	lm=0;
	p=Unicode2Utf8(0x25CC); /*dotted circle for combining marks */
	while(Symbols[j].name)
	{
		l=strlen(Symbols[j].name);
		if (l>lm)
			lm=l;
		j++;
	}
	j=0;
	while(Symbols[j].name)
	{
		l=strlen(Symbols[j].name);
		str=Unicode2Utf8(Symbols[j].unicode);
		// symbol list should not contain combining diacritical marks
		printf("Symbol: %s"	, Symbols[j].name);
		for (i=l;i<lm+2;i++)
			printf(" ");
		printf(" %s\n", str);
		free(str);
		j++;
	}
	j=0;
	while (Combining[j].P!=PD_NONE)
	{
		str=Unicode2Utf8(Combining[j].comb);
		k=0;
		while (Keys[k].name)
		{
			if (Keys[k].P==Combining[j].P)
			{
				l=strlen(Keys[k].name);
				printf("Symbol: %s %s"	, Keys[k].name, p);
				for (i=l;i<lm+1;i++)
					printf(" ");
				printf("%s%s\n", p, str);
			}
			k++;
		}
		free(str);
		j++;
	}
	free(p);
}

char * Symbols_Str()
{
	int j=0, ln=0, s=1, k;
	size_t slen=0;
	char *str, *p;
	char *line=calloc(s, 1);
	p=Unicode2Utf8(0x25CC); /*dotted circle for combining marks */
	j=0;
	while(Symbols[j].name)
	{
		str=Unicode2Utf8(Symbols[j].unicode);
		slen = (size_t) snprintf(NULL, 0, "%s:%s;", Symbols[j].name, str);
		while (s < (ln+slen+1))
			s*=2;
		line=realloc(line, s);
		snprintf(line+ln, slen+1, "%s:%s;", Symbols[j].name, str);
		ln+=slen;
		free(str);
		j++;
	}
	j=0;
	while (Combining[j].P!=PD_NONE)
	{
		str=Unicode2Utf8(Combining[j].comb);
		k=0;
		while (Keys[k].name)
		{			
			if (Keys[k].P==Combining[j].P)
			{
				slen=(size_t)snprintf(NULL, 0, "%s %s:%s%s;", Keys[k].name, p, p, str);
				while (s < (ln+slen+1))
					s*=2;
				line=realloc(line, s);
				snprintf(line+ln, slen+1, "%s %s:%s%s;", Keys[k].name, p, p, str);
				ln+=slen;
			}
			k++;
		}
		j++;
	}	
	free(p);
	return realloc(line, ln+1);
}

Symbol IsSymbol(char *begin, const Symbol Symbols[])
{
	int j=0;
	int n;
	char *p;
	p=begin+1;
	if (IsInSet(*p, ",;"))
		p++;
	else
	{		
		if (*p)
			p++;
		
		//while ((*p)&&(!IsInSet(*p, "\\ ^_+-*/()@#$%&{},;0123456789=\n"))) /* I took out the { so I can overload symbols and scalable symbols */
		while ((*p)&&(((*p>='A')&&(*p<='Z'))||((*p>='a')&&(*p<='z'))))
			p++;
	}
		
	
	n=p-begin;
	
	while(Symbols[j].name)
	{
		if (strlen(Symbols[j].name)==n)
			if (strncmp(begin, Symbols[j].name, n) == 0)
				return Symbols[j];
		j++;
	}
	return Symbols[j];
}

int IsCommandChar(char c) // return 1 for letters, 2 for characters that are used in short commands ('\\', '\,', etc.)
{
	if ((c>='A')&&(c<='Z'))
		return 1;
	if ((c>='a')&&(c<='z'))
		return 1;
	if (c==',')
		return 2;
	if (c==';')
		return 2;
	if (c==':')
		return 2;
	if (c=='\\')
		return 2;
	if (c=='"')
		return 2;
	return 0;
}

char *CommandEnd(char *begin)
{
	char *end;
	if (!begin)
		return NULL;
	if (!(*begin))
		return NULL;
	end=begin+1;
	if (IsCommandChar(*end)==2)
		end++;
	else
	{
		while (IsCommandChar(*end)==1)
			end++;
	}
	return end;
}
	


KEYWORD LookupKey(char *begin, const KEYWORD * Keys)
{
	int j=0, l;
	char c, *p;
	p=CommandEnd(begin);
	if (!p)
	{
		KEYWORD K={NULL, PD_NONE, 0, 0};
		return K;
	}
	c=*p;
	*p='\0';
	l=strlen(begin);
	while (Keys[j].name)
	{
		if (l==strlen(Keys[j].name))
			if (strncmp(begin, Keys[j].name, l) == 0)
			{
				*p=c;			
				return Keys[j];
			}
		j++;
	}
	*p=c;	
	return Keys[j];
}

PRSDEF  LookupFont(const char *begin)
{
	int j=0;
	if (!begin)
		return PD_TEXT;
	
	while (Keys[j].name)
	{
		if (strncmp(begin, Keys[j].name+1, strlen(Keys[j].name)+1) == 0)
		{
			switch (Keys[j].P)
			{
				case PD_TEXT:
				case PD_MATHBF:
				case PD_MATHBFIT:
				case PD_MATHCAL:
				case PD_MATHSCR:
				case PD_MATHFRAK:
				case PD_MATHBB:
				case PD_MATHSF:
				case PD_MATHSFBF:
				case PD_MATHSFIT:
				case PD_MATHSFBFIT:
				case PD_MATHTT:
				case PD_MATHNORMAL:
					return Keys[j].P;
				default:
					/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
					// ERRORFLAG ERRUNKNOWNFONT  "Unknown font type, using text instead"
					AddErr(ERRUNKNOWNFONT);
					return PD_TEXT;
			}
		}
		j++;
	}
	AddErr(ERRUNKNOWNFONT);
	return PD_TEXT;
}
static char *NoDel=".";
SCALABLE_DELIMITER LookupDelimiter(char *begin, char **del)
{
	int j=0;
	
	while (DelTable[j].name)
	{
		if (strncmp(begin, DelTable[j].name, strlen(DelTable[j].name)) == 0)
		{
			if (del)
				(*del)=(char *)(DelTable[j].name);
			return DelTable[j].D;
		}
		j++;
	}
	if (del)
		(*del)=NoDel;
	return DelTable[j].D;
}


void LookupCombining(PRSDEF P, unsigned int *comb, unsigned int *alt, unsigned int *altascii)
{
	int j=0;
	while(Combining[j].P!=PD_NONE)
	{
		if (P==Combining[j].P)
		{
			(*comb)=Combining[j].comb;
			(*alt)=Combining[j].alt;
			(*altascii)=Combining[j].altascii;
			return;
		}
		j++;
	}
	(*comb)=0;
	(*alt)=0;
	(*altascii)=0;
	return;
}
void PrintToken(TOKEN T)
{
	int i;
	printf("Token %d\n", T.P);
	for (i=0;i<T.Nopt;i++)
		printf("--> optional \'%s\'\n", T.opt[i]);
	for (i=0;i<T.Nargs;i++)
		printf("--> argument \'%s\'\n", T.args[i]);
	if (T.sub)
		printf("--> subscript \'%s\'\n", T.sub);
	if (T.super)
		printf("--> superscript \'%s\'\n", T.super);
	if (T.limits)
		printf("--> limits set\n");
}

void FreeToken(TOKEN T)
{
	int i;
	if (T.opt)
	{
		for (i=0;i<T.Nopt;i++)
			free(T.opt[i]);
		free(T.opt);
	}
	if (T.args)
	{
		for (i=0;i<T.Nargs;i++)
			free(T.args[i]);
		free(T.args);
	}
	if (T.sub)
		free(T.sub);
	if (T.super)
		free(T.super);
}

/* test whether char part of a multibyte char */
#define isutf(c) (((c)&0xC0)!=0x80)
/* increase index i to next (multibyte) char */
void u8_inc(char *s, int *i)
{
    (void)(isutf(s[++(*i)]) || isutf(s[++(*i)]) || isutf(s[++(*i)]) || ++(*i));
}

char *Script(char *begin, char **next)
{
	int brac=0;
	char *end;
	char *str, *p;
	end=begin;
	
	if ((*end!='\\')&&(*end!='{')) /* add a single character */
	{
		int i=0;
		u8_inc(end, &i);
		str=malloc((i+1)*sizeof(char));
		str[i]='\0';
		(*next)=end+i;
		while (i)
		{
			i--;
			str[i]=end[i];
		}
		return str;
	}
	
	if (*end=='\\') /* a command will follow, read till it ends */
	{
		end++;
		while ((*end)&&!IsInSet(*end, " \t+-*/&\\_^}"))
			end++;
		if(IsInSet(*end, " }")) /* ends in a bracket, we need that bracket in there to properly interpret the script */
			end++;
		(*next)=end;
	}
	else if (*end=='{')
	{
		brac=1;
		
		while ((*end)&&((brac))) /* we can blindly read till we are out of the brackets */
		{
			end++;
			if (*end=='{')
				brac++;
			else if (*end=='}')
				brac--;
		}
		if (brac)
			AddErr(LEXPREMATUREEND); /* still within brackets! */
		(*next)=end+1;
		begin++;/* strip outer brackets */
	}
		
	str=malloc((end-begin+1)*sizeof(char));
	p=str;
	while (begin<end)
	{
		*p=*begin;
		begin++;
		p++;
	}
	
	(*p)='\0';
	return str;	
}


void LexAddScripts(TOKEN *T, char *begin)
{
	char *end;
	while ((*begin=='_')||(*begin=='^'))
	{	
		if (*begin=='_')
		{
			begin++;
			if (T->sub)
			{
				
				/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
				// ERRORFLAG ERRMULTISUB  "Multiple Subscripts"
				AddErr(ERRMULTISUB);
				/* now what? */
				
			}
			T->sub=Script(begin, &end);
			begin=end;				
		}
		else
		{
			begin++;
			if (T->super)
			{
				/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
				// ERRORFLAG ERRMULTISUP  "Multiple Superscripts"
				AddErr(ERRMULTISUP);
			}
			T->super=Script(begin, &end);
			begin=end;			
		}
	}			
}

char *OptionArgument(char *begin, char **next, char open, char close)
{
	int brac=0, esc=0;
	char *end;
	char *str, *p;
	end=begin;
	
	if (*end==open)
		brac=1;
	else
	{
		(*next)=end;
		return NULL;
	}
	while ((*end)&&(brac))
	{
		end++;
		if (esc)
			esc=0;				  /* we can escape a bracket, should we ? */
		if (!esc)				  /* now it is just counting brackets */
		{
			if (*end==open)
				brac++;
			else if (*end==close)
				brac--;
			else if (*end=='\\')
				esc=1;
		}
	}
	if (!*end)
	{
		if (brac)
			AddErr(LEXPREMATUREEND); /* still within brackets! */
		(*next)=end;
	}
	else
		(*next)=end+1;
	begin++; /* strip outer brackets */
	
	str=malloc((end-begin+1)*sizeof(char));
	p=str;
	while (begin<end)
	{
		*p=*begin;
		begin++;
		p++;
	}
	(*p)='\0';
	return str;	
}

char *Option(char *begin, char **next)
{
	return OptionArgument(begin, next, '[', ']');
}


char *Argument(char *begin, char **next)
{
	char *r;
	
	while((*begin)&&isspace(*begin))
		begin++;
	
	r=OptionArgument(begin, next, '{', '}');
	if (r==NULL)
	{
		// no brackets, every char is an argument till we have some delimiter
		if (!IsInSet(*begin, "\\ ^_+-*/()@#$%&{},;\n"))
		{
			int l=NumByte(begin);
			r=malloc((l+1)*sizeof(char));
			memcpy(r,begin,l*sizeof(char));
			r[l]='\0';
			(*next)=begin+l;
		}
		else
			return NULL;
	}
	return r;
}

/* some commands work on arguments before they are issued, we peek ahead to see if we need to do something */
void PeekAhead(TOKEN *T, char *begin)
{
	char *end, *bb;	
	int reset=1;
	KEYWORD K;
	
	/* we need to handle spaces between
	 * so we skip spaces but restore the spaces if peekahead does not find 
	 * anything relevant
	 */
	bb=begin; // backup begin
	while (*begin==' ') // skip whitespace
		begin++;
	K=LookupKey(begin, Keys); /* see if we have a keyword */
	switch (K.P)
	{
		case PD_LIMITS: /* set the limits option (puts sub and superscripts vertically above the argument */
			begin+=strlen(K.name);
			T->limits=1;
			reset=0;
			break;
		case PD_NOLIMITS: /* unset the limits option */
			begin+=strlen(K.name);
			T->limits=0;
			reset=0;
			break;
		case PD_OVER: /* same as \frac but then after the fact. BUG: in tex this command is greedy but this implementation is not! */
		{
			char *str, *p, *b;
			reset=0;
			T->P=PD_FRAC;
			/* get the two arguments */
			/* the first argument was already lexed! */
			if (!T->args)
				T->args=malloc(2*sizeof(char *));
			else
			{
				int i;
				for (i=0;i<T->Nargs;i++)
					free(T->args[i]);					
				T->args=realloc(T->args, 2*sizeof(char *));
			}
			T->Nargs=2;
			
			str=malloc((begin-T->self+1)*sizeof(char));
			p=str;
			b=T->self;
			while (b<begin)
			{
				*p=*b;
				b++;
				p++;
			}
			(*p)='\0';
			T->args[0]=str;
			
			 /* move begin to skip '\over' */
			begin+=5;
			fflush(stdout);
			p=begin;
			while (*p && ((*p==' ')||(*p=='\t')))
				p++;
			if (*p=='{')
			{			
				str=Argument(p, &end);	
				begin=end;
			}
			else
			{
				/* read till space */
				begin=p;
				if (*p=='\\')
				{
					/*we need to parse the command with all its arguments ... */
					/*let's just say you need to put the argument betwene curly brackets then */
					str=NULL;
				}
				else
				{
					while ((*p)&&(!IsInSet(*p, "\\ \t{")))
						p++;
					if (p-begin==0)
						str=NULL;
					else
					{
						str=malloc((p-begin+1)*sizeof(char));
						end=p;
						p=str;
						while (begin<end)
						{
							*p=*begin;
							begin++;
							p++;
						}
						(*p)='\0';
					}
				}
			}
			if (!str)
			{
				AddErr(ERRTOOFEWMANDARG);
				T->P=PD_NONE;
				return;
			}
			T->args[1]=str;
			/* recurse, do not set next string yet (an \over, _, or ^ may follow) */
			PeekAhead(T, begin);
			return;
		}
		case PD_CHOOSE: /* same as \frac but then after the fact.  BUG: in tex this command is greedy but this implementation is not! */
		{
			char *str, *p, *b;
			reset=0;
			T->P=PD_BINOM;
			/* get the two arguments */
			/* the first argument was already lexed! */
			if (!T->args)
				T->args=malloc(2*sizeof(char *));
			else
			{
				int i;
				for (i=0;i<T->Nargs;i++)
					free(T->args[i]);					
				T->args=realloc(T->args, 2*sizeof(char *));
			}
			T->Nargs=2;
			
			str=malloc((begin-T->self+1)*sizeof(char));
			p=str;
			b=T->self;
			while (b<begin)
			{
				*p=*b;
				b++;
				p++;
			}
			(*p)='\0';
			T->args[0]=str;
			
			 /* move begin to skip '\choose' */
			begin+=7;
			fflush(stdout);
			p=begin;
			while (*p && ((*p==' ')||(*p=='\t')))
				p++;
			if (*p=='{')
			{			
				str=Argument(p, &end);	
				begin=end;
			}
			else
			{
				/* read till space */
				begin=p;
				if (*p=='\\')
				{
					/*we need to parse the command with all its arguments ... */
					/*let's just say you need to put the argument betwene curly brackets then */
					str=NULL;
				}
				else
				{
					while ((*p)&&(!IsInSet(*p, "\\ \t{")))
						p++;
					if (p-begin==0)
						str=NULL;
					else
					{
						str=malloc((p-begin+1)*sizeof(char));
						end=p;
						p=str;
						while (begin<end)
						{
							*p=*begin;
							begin++;
							p++;
						}
						(*p)='\0';
					}
				}
			}
			if (!str)
			{
				AddErr(ERRTOOFEWMANDARG);
				T->P=PD_NONE;
				return;
			}
			T->args[1]=str;
			/* recurse, do not set next string yet (an \over, _, or ^ may follow) */
			PeekAhead(T, begin);
			return;
		}
		default:
			break;
	}
	while ((*begin=='_')||(*begin=='^'))
	{	
		reset=0;
		if (*begin=='_')
		{
			begin++;
			if (T->sub)
				AddErr(ERRMULTISUB);
			T->sub=Script(begin, &end);
			begin=end;				
		}
		else
		{
			begin++;
			if (T->super)
				AddErr(ERRMULTISUP);
			T->super=Script(begin, &end);
			begin=end;			
		}
	}			
	if (reset)
		T->next=bb; // nothing found, restore any skipped spaces
	else
		T->next=begin;
}


/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
// ERRORFLAG INVALIDDELIMITER  "Invalid Delimiter"
void LeftMiddleRight(char *begin, char **next, char **arg1, char **arg2, char **open, char **middle, char **close)
{
	int brac=1;
	char *end;
	char *str, *p;
	SCALABLE_DELIMITER D;
	
	(*open)=NULL;
	(*middle)=NULL;
	(*close)=NULL;
	(*arg1)=NULL;
	(*arg2)=NULL;
	
	while (*begin==' ') // skip whitespace
		begin++;
	end=begin;
	D=LookupDelimiter(end, open);
	if (D==DEL_NONE)
		AddErr(INVALIDDELIMITER);
	else
		begin+=strlen(*open);
	end=begin;
	while ((*end)&&(brac))
	{
		if (*end=='\\')/* check for a \right */
		{
			if ((strncmp(end, "\\right", 6))==0)
				brac--;
			if ((strncmp(end, "\\left", 5))==0)
				brac++;
			if ((brac==1)&&((strncmp(end, "\\middle", 7))==0))
			{
				/* set a middle bracket */
				/* two body arguments! */
				if (*end)
				{
					str=malloc((end-begin+1)*sizeof(char));
					p=str;
					while (begin<end)
					{
						*p=*begin;
						begin++;
						p++;
					}
					(*p)='\0';
					end+=7;
					(*arg1)=str;
					
					while (*end==' ') // skip whitespace
						end++;
					D=LookupDelimiter(end, middle);
					if (D==DEL_NONE)	
						AddErr(INVALIDDELIMITER);
					else
						begin=end+strlen(*middle);	
				}		
			}
		}
		end++;
	}
	end--;
	if (*end)
	{
		str=malloc((end-begin+1)*sizeof(char));
		p=str;
		while (begin<end)
		{
			*p=*begin;
			begin++;
			p++;
		}
		(*p)='\0';
		end+=6;
		(*arg2)=str;
		
		while (*end==' ') // skip whitespace
			end++;
		D=LookupDelimiter(end, close);
		if (D==DEL_NONE)	
			AddErr(INVALIDDELIMITER);
		else
			end+=strlen(*close);
	}
	else
	{
		/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
		// ERRORFLAG NORIGHTBRAC  "Premature end, no \\right found"
		AddErr(NORIGHTBRAC);
		/* we insert one, the show must go on */
		(*close)=NoDel;
		(*next)=end; /* make next point to an empty string, we are out ocf characters */
		return;
		
	}
	(*next)=end;
}


char *LeftRightBlock(char *begin)
{
	int brac=1;
	char *end;
	char *open=NULL, *close=NULL;
	SCALABLE_DELIMITER D;
	
	
	while (*begin==' ') // skip whitespace
		begin++;
	end=begin;
	D=LookupDelimiter(end, &open);
	if (D!=DEL_NONE)
		begin+=strlen(open);
	end=begin;
	while ((*end)&&(brac))
	{
		end++;
		if (*end=='\\')/* check for a \right */
		{
			if ((strncmp(end, "\\right", 6))==0)
				brac--;
			if ((strncmp(end, "\\left", 5))==0)
				brac++;
		}
	}
	
	if (*end)
	{
		end+=6;
		while (*end==' ') // skip whitespace
			end++;
		D=LookupDelimiter(end, &close);
		if (D!=DEL_NONE)
			end+=strlen(close);
	}
	
	return end;
}
/* somebody shoot me, this code is horror ... the worst part is that I wrote it */
/* tableread parses a latex table to an arry of strings, one for each cell, and info like \hlines number of columns, etc. */
// I should rewrite from scratch
char ** TableRead(char *begin, char **end, int *Nc, int *N, char **hsep, int *Nhsep)
{
	int go=1, i=0, j=0, nc=0, nr=0, nnc=-1, b=1, row=0, line=0;
	char **res;
	int Na=10, na=10, nasep=10;
	KEYWORD K;
	res=calloc(Na,sizeof(char *));
	res[i]=malloc(na*sizeof(char));
	res[i][0]='\0';
	*hsep=malloc(nasep*sizeof(char));
	
	(*hsep)[row]='c';
	while (*begin&&go)
	{
	    K=LookupKey(begin, Envs);
	    if (K.P==PD_BEGIN)
	    {
			b++;
			/* read a block */
			res[i][j]=(*begin);
			j++;
			if (j==na)
			{
				na+=10;
				res[i]=realloc(res[i],na*sizeof(char));
			}
			while ((*begin)&&(b>1))
			{
				begin++;
				K=LookupKey(begin, Envs);
				if (K.P==PD_END)
				{
					int k;
					b--;
					res[i][j]=(*begin);
					j++;
					if (j==na)
					{
						na+=10;
						res[i]=realloc(res[i],na*sizeof(char));
					}
					for (k=0;k<3;k++)
					{
						begin++;
						res[i][j]=(*begin);
						j++;
						if (j==na)
						{
							na+=10;
							res[i]=realloc(res[i],na*sizeof(char));
						}
					}
					if (*begin)
					{
						do
						{
							begin++;
							res[i][j]=(*begin);
							j++;
							if (j==na)
							{
								na+=10;
								res[i]=realloc(res[i],na*sizeof(char));
							}
						} while((begin)&&(*begin!='}'));
					}
					begin++;
					K.P=PD_NONE;
				} 
				else
				{
					if (K.P==PD_BEGIN)
						b++;
					res[i][j]=(*begin);
					j++;
					if (j==na)
					{
						na+=10;
						res[i]=realloc(res[i],na*sizeof(char));
					}
				}
			}
			line=1;
		}
		else if (K.P==PD_END)
		{
			b--;
			begin+=4;
		}
		else if (K.P==PD_HLINE)
		{
			if (nc==0) /* hline before row */
			{
				if (row>0)
				{
					if ((*hsep)[row-1]=='-')
					{
						/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
						// ERRORFLAG ERRDOUBLEHLINE  "Double \\hline"
						AddErr(ERRDOUBLEHLINE);
						/* ignore it by decreasing row */
						row--;					
					}
				}
				(*hsep)[row]='-';
				row++;
				if (row==nasep)
				{
					nasep+=10;
					*hsep=realloc(*hsep, nasep*sizeof(char));						
				}
				(*hsep)[row]='c';
				
			}
			else if ((nc==nnc)||(nnc<0))/* hline after row */
			{
				if (nnc<0)
					nnc=nc;
				row++;
				if (row==nasep)
				{
					nasep+=10;
					*hsep=realloc(*hsep, nasep*sizeof(char));						
				}
				(*hsep)[row]='-';
			}
			else
			{	
				/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
				// ERRORFLAG ERRUNEXPHLINE  "unexpected \\hline in the middle of a row"
				AddErr(ERRUNEXPHLINE);
				/* just ignore */	
			}
			begin+=6;
		}
		else 
		{
			if (b==0)
				go=0;
			else
			{		
				if (strncmp(begin,"\\left",5)==0)
				{
					char *pp;
					pp=LeftRightBlock(begin);
					while (begin<pp)
					{
						res[i][j]=(*begin);
						j++;
						begin++;
						if (j==na)
						{
							na+=10;
							res[i]=realloc(res[i],na*sizeof(char));
						}
					}
					begin--;// begin is incremented later					
				}
				else
				{
					switch (*begin)
					{
						case '&': /* next column */
							res[i][j]='\0';
							nc++;
							i++;
							if (i==Na)
							{
								Na+=10;
								res=realloc(res,Na*sizeof(char*));
							}
							na=10;
							res[i]=calloc(na,sizeof(char));
							j=0;
							line=1;
							break;
						case '\\': 
							if (begin[1]=='\\')/* next row */
							{
								line=0;
								begin++;
								res[i][j]='\0';
								row++;
								if (row==nasep)
								{
									nasep+=10;
									*hsep=realloc(*hsep, nasep*sizeof(char));						
								}
								(*hsep)[row]='c';
								
								nr++;
								if (nnc<0)
									nnc=nc;
								if (nc!=nnc)
								{
									int skip=(nc-nnc);
									if (nnc<nc)
										skip=(nc-nnc);
									else
									{																		
										/* error is Fatal */	
										/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
										// ERRORFLAG ERRNUMCOLMATCH  "Unequal number of columns in different rows"
										AddErr(ERRNUMCOLMATCH);
										skip=0;
									}
									if (i+skip+1>=Na)
									{
										Na+=10;
										res=realloc(res,Na*sizeof(char*));
									}
									na=10;
									i++;
									for (;skip>0;skip--)
										res[i++]=NULL;
									na=10;
									res[i]=calloc(na,sizeof(char));	
								}
								else
								{
									i++;
									if (i==Na)
									{
										Na+=10;
										res=realloc(res,Na*sizeof(char*));
									}
									na=10;
									res[i]=calloc(na,sizeof(char));	
								}
								j=0;
								nc=0;
							}
							else
							{
								// escaped character?
								line=1;
								res[i][j]=(*begin);
								j++;
								if (j==na)
								{
									na+=10;
									res[i]=realloc(res[i],na*sizeof(char));
								}
							}
							break;
						case '{':
						{ /* read a block */
							int br=1, esc=0;
							res[i][j]=(*begin);
							j++;
							if (j==na)
							{
								na+=10;
								res[i]=realloc(res[i],na*sizeof(char));
							}
							line=1;
							while ((*begin)&&(br))
							{
								begin++;
								if (!esc)
								{
									if (*begin=='{')
										br++;
									else if (*begin=='}')
										br--;
									else if (*begin=='\\')
										esc=1;
								}
								else
									esc=0;
								res[i][j]=(*begin);
								j++;
								if (j==na)
								{
									na+=10;
									res[i]=realloc(res[i],na*sizeof(char));
								}
							}
							line=1;
							break;
						}
						default:
							res[i][j]=(*begin);
							j++;
							if (j==na)
							{
								na+=10;
								res[i]=realloc(res[i],na*sizeof(char));
							}
							if (!isspace(*begin))
								line=1;
							break;
					}
				}
				begin++;
			}
		}
	}
	if (!line)
		free(res[i]);
	else
		res[i][j]='\0';
	*end=begin;
	if (nnc<0)
		nnc=nc;
	if ((line) && (nc!=nnc))
	{
		int skip=(nc-nnc);
		if (nnc<nc)
			skip=(nc-nnc);
		else
		{			
			AddErr(ERRNUMCOLMATCH);
			skip=0;
		}
		if (i+skip+1>=Na)
		{
			Na+=10;
			res=realloc(res,Na*sizeof(char*));
		}
		na=10;
		i++;
		for (;skip>0;skip--)
			res[i++]=NULL;
	}
	*Nc=nnc+1;
	*N=i+line;
	*Nhsep=row+(line);
	(*hsep)[row+(line)]='\0';
	return res;
}

TOKEN BeginEnv(TOKEN T)
{
	TOKEN R;
	KEYWORD K;
	char *begin;
	char *end;
	
	R.Nargs=0;
	R.Nopt=0;
	R.args=NULL;
	R.opt=NULL;
	R.sub=NULL;
	R.super=NULL;
	R.limits=0;
	R.P=PD_NONE;
	K=LookupKey(T.args[0], Envs);
	/* lex environment */
	switch(K.P)
	{
		case PD_ALIGN:
		case PD_ARRAY:
		{
			int Nc, Nha, n, nv;
			char *valign, *halign, *hsep, *p;
			if (K.P==PD_ARRAY)
			{
				/* fetch alignment infos from TOKEN T */
				halign=Option(T.next, &begin);	
				valign=Argument(begin, &end);
				if ((!valign)||(*valign=='\0'))
				{
					/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
					// ERRORFLAG ERRVALIGHN  "\\begin{array} requires column-wise alignment info"
					AddErr(ERRVALIGHN);
					return R;
				}
				begin=end;	
			}
			else
			{
				/* K.P=PD_ALIGN */
				/* align implemented as a table with two columns, left column is right aligned 
				 * right column is left aligned
				 */
				halign=NULL;
				valign=malloc(3*sizeof(char));
				valign[0]='r';
				valign[1]='l';
				valign[2]='\0';
				begin=T.next;
			}
						
			/* fetch body till \end{array} and put it in arguments or R */
			R.args=TableRead(begin, &R.next, &Nc, &R.Nargs, &hsep, &Nha);
			if (QueryErr(ERRNUMCOLMATCH))
				return R;
			
			if (strncmp(R.next+1, T.args[0], strlen(T.args[0]))!=0)
			{
				/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
				// ERRORFLAG ERRNOMATCHINEND  "\\begin does not match closed with \\end"
				AddErr(ERRNOMATCHINEND);
				return R;
			}
			else
				R.next+=strlen(T.args[0])+2; /* move beyond the \end{env-name} */
			
			/* in the option we put Nc, halign and valign */
			R.opt=malloc(3*sizeof(char *));
			R.opt[0]=malloc(16*sizeof(char));
			snprintf(R.opt[0],16, "%d", Nc); /* Nc in option 0 */
			
			p=valign;
			n=0;
			nv=0;
			while(*p)
			{
				if (IsInSet(*p, "clr"))
					n++;
				else if (!IsInSet(*p, "|"))
				{
					/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
					// ERRORFLAG ERRNOVALIDALIGNC  "Illegal character in alignment info"
					AddErr(ERRNOVALIDALIGNC);
					*p='c'; /* set it to center as default */
					n++;
				}
				else
					nv++;
				p++;
			}
			if (n!=Nc)
			{
				int Nl, j=0, k=0;
				/* fix alignment data */
				p=calloc((2*Nc+1),sizeof(char));
				Nl=strlen(valign);
				while (k<Nc)
				{
					p[j]=valign[k%Nl];
					if (IsInSet(p[j], "clr"))
						k++;
					j++;					
				}
				if (IsInSet(valign[k%Nl], "|"))
					p[j]=valign[k%Nl];
				free(valign);
				valign=p;
			}							
			R.opt[1]=valign;
			
			
			if (halign)
			{
					int Nl, j=0, k=0;
					Nl=strlen(halign);
					while (halign[j]&&(hsep[k]))
					{
						while (hsep[k]=='-')
							k++;
						hsep[k]=halign[j%Nl];
						j++;
						k++;
					}
					while (hsep[k]=='-')
						k++;
					if ((halign [k])||(halign[j]))
					{
						/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
						// ERRORFLAG ALROWSMATCH  "\\number of rows does not match the alignment inf"
						AddErr(ALROWSMATCH);
						return R;					
					}
					free(halign);
			}
			R.opt[2]=hsep;
			
			R.Nopt=3;	
			R.P=PD_ARRAY;
			break;
		}
		case PD_CASES:
		case PD_MATRIX:
		case PD_VMATRIX:
		case PD_VVMATRIX:
		case PD_BBMATRIX:
		case PD_BMATRIX:
		case PD_PMATRIX:
		{
			int Nc, Nha, i,j;
			char *hsep, *valign, va='c';
			
			valign=Option(T.next, &begin);	
			
			/* fetch body till \end{array} and put it in arguments or R */
			R.args=TableRead(begin, &R.next, &Nc, &R.Nargs, &hsep, &Nha);
			if (strncmp(R.next+1, T.args[0], strlen(T.args[0]))!=0)
			{
				AddErr(ERRNOMATCHINEND);
				return R;
			}
			else
				R.next+=strlen(T.args[0])+2; /* move beyond the \end{env-name} */
			
			/* in the option we put Nc, halign and valign */
			R.opt=malloc(3*sizeof(char *));
			R.opt[0]=malloc(16*sizeof(char));
			snprintf(R.opt[0],16, "%d", Nc); /* Nc in option 0 */
			
			if (valign)
				va=valign[0];
			free(valign);
			if (!IsInSet(va, "lrc"))
			{
				AddErr(ERRNOVALIDALIGNC);
				va='c'; /* set it to center as default */
			}
							
			R.opt[1]=malloc((Nc+1)*sizeof(char));
			for (i=0;i<Nc;i++)
				R.opt[1][i]=va;
			R.opt[1][Nc]='\0';
			j=0;
			for (i=0;i<Nha;i++)
			{
				hsep[j]=hsep[i];
				if (hsep[i]=='-')
				{
					/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
					// ERRORFLAG ERRHLINESINMATRIX  "no \\hline's allowed in the matrix environment"
					AddErr(ERRHLINESINMATRIX);	
				}
				else
					j++;
			}
			R.opt[2]=hsep;
			
			R.Nopt=3;	
			R.P=K.P;
			
			break;
			
		}
		default:
			/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
			// ERRORFLAG ERRUNKNOWNENV  "Unknown environment"
			AddErr(ERRUNKNOWNENV);	
			return R;
	}
	/* add sub and superscripts to the environment */
	begin=R.next;
	PeekAhead(&R, begin);
	FreeToken(T);
	return R;
}


char *GetNumPart(char *str, float *f)
{
	char *p, c;
	p=str;
	(*f)=1.0;
	if (IsInSet(*p, "+-"))
		p++;
	while(IsInSet(*p, "0123456789"))
		p++;
	if (IsInSet(*p, "."))
		p++;
	while(IsInSet(*p, "0123456789"))
		p++;
	if (p==str)
		return p;
	c=(*p);
	*p='\0';
	if (strncmp(p,"-",2)==0)// only a - means -1;
		(*f)=-1.0;
	else
		(*f)=atof(str);
	*p=c;
	return p;
}

float LookupUnit(char *str)
{
	int i=0;
	while (Lengths[i].name)
	{
		if (strncmp(str,Lengths[i].name,255)==0)
			return Lengths[i].scale;
		i++;
	}
	return Lengths[i].scale;
}
int ReadLengthWidth(char *str)
{ // return length in character width
	char *p;
	float l, u;
	p=GetNumPart(str, &l);
	u=LookupUnit(p);
	if (u>=0)
		l*=u;
	return (int)round(l);
}

int ReadLengthHeight(char *str)
{ // return length in character width
	char *p;
	float l, u;
	p=GetNumPart(str, &l);
	u=LookupUnit(p);
	if (u>=0)
		l=l*u/AspectRatio; // if no unit is found it is directly character height
	return (int)round(l);
}

/* TODO
 * make things like 
 * \kern-\nulldelimiterspace work
 * to this end we must extract the whole length spec
 * as a unit. Right now it only works when we do \kern{} (which is not even proper tex)
 * we have to hanmdle an open ended str
 */




int IsTexConstruct(char *string)
{
	char *p;
	p=string;
	while (*p)
	{
		if(IsInSet(*p,"\\_^"))
			return 1;
		p++;
	}
	return 0;
}

TOKEN SubLexer(char *begin, FONT F)
{
	/* fetch a new token and its arguments */
	int n, j,i,k, nsp;
	char *end;
	KEYWORD K;
	char c, *p;
	
	TOKEN R;
	R.Nargs=0;
	R.Nopt=0;
	R.args=NULL;
	R.opt=NULL;
	R.sub=NULL;
	R.super=NULL;
	R.self=begin;
	R.limits=0;
	R.P=PD_NONE;
	R.F=F; /* the current fonts */
	if (*begin=='\\')
	{
		/* we have something of the form \comm[]..{}.. */
		K=LookupKey(begin, Keys);
		if (K.P==PD_LEFTRIGHT)
		{
			/* create a token with three arguments, the open, middle and closing bracket and units inbetween */
			char *arg1, *arg2;
			char *open, *middle, *close;
			R.P=K.P;
			begin+=strlen(K.name);
			LeftMiddleRight(begin, &end, &arg1, &arg2, &open, &middle, &close);			
			R.args=malloc(5*sizeof(char *));
			if (arg1)
				R.args[R.Nargs]=arg1;
			else
			{
				R.args[R.Nargs]=malloc(sizeof(char));
				R.args[R.Nargs][0]='\0';
			}
			R.Nargs++;
			
			
			if (arg2)
				R.args[R.Nargs]=arg2;
			else
			{
				R.args[R.Nargs]=malloc(sizeof(char));
				R.args[R.Nargs][0]='\0';
			}
			R.Nargs++;	
			
			
			R.args[R.Nargs]=malloc((strlen(open)+1)*sizeof(char)); 
			strncpy(R.args[R.Nargs],open,(strlen(open)+1));			
			R.Nargs++;
			
			if (middle)
			{
				R.args[R.Nargs]=malloc((strlen(middle)+1)*sizeof(char));
				strncpy(R.args[R.Nargs],middle,(strlen(middle)+1));
			}
			else
			{
				/* missing middle makes it equivalent to \leftX\middle. ... \rightY */
				R.args[R.Nargs]=malloc(2*sizeof(char));
				R.args[R.Nargs][0]='.';
				R.args[R.Nargs][1]='\0';
			}			
			R.Nargs++;	
			
			R.args[R.Nargs]=malloc((strlen(close)+1)*sizeof(char));
			strncpy(R.args[R.Nargs],close,(strlen(close)+1));
			R.Nargs++;	
			begin=end;
			PeekAhead(&R, begin);
			return R;
		}
		else if ((K.P>=PD_BIG1)&&(K.P<=PD_BIG4))
		{
			SCALABLE_DELIMITER D;
			/* create a token with three arguments, the open, middle and closing bracket and units inbetween */
			char *brac;
			R.P=K.P;
			begin+=strlen(K.name);
					
			while (*begin==' ') // skip whitespace
				begin++;
			end=begin;
			D=LookupDelimiter(end, &brac);
			if (D==DEL_NONE)
			{	
				AddErr(INVALIDDELIMITER);
			}
			begin+=strlen(brac);
					
			R.args=malloc(1*sizeof(char *));
			R.args[0]=calloc(16,sizeof(char));
			strncpy(R.args[0],brac,15);
			
			PeekAhead(&R, begin);
			return R;
		}
		else if ((K.P==PD_FUNCTION)||(K.P==PD_LIM))
		{
			R.P=PD_TEXT;
			R.args=malloc(2*sizeof(char *));
			R.Nargs=1;
			p=begin+1;
			while ((*p)&&(!IsInSet(*p, "\\_^/*{ ,;(")))
				p++;
			c=*p;
			*p='\0';
			j=strlen(begin+1);
			R.args[0]=malloc((j+1)*sizeof(char));
			strncpy(R.args[0], begin+1, j+1);
			*p=c;
			begin=p;
			if (*begin == ' ')
				begin++;
			if (K.P==PD_LIM)
				R.limits=1;/* here the sub/super scripts are placed vertically */
			PeekAhead(&R, begin);
			return R;
		}
		else if ((K.P==PD_SETITALIC)||(K.P==PD_SETBOLD)||(K.P==PD_SETROMAN))
		{
			int l;
			begin+=3;
			switch(K.P)
			{
				case PD_SETITALIC:
					R.F=F_ITALIC;
					R.P=PD_MATHSFIT;
					break;
				case PD_SETBOLD:				
					R.F=F_BOLD;
					R.P=PD_MATHBF;
					break;
				case PD_SETROMAN:				
					R.F=F_ROMAN;
					R.P=PD_TEXT;
					break;
				default:
					break;
			}
			if (*begin==' ')
				begin++;
			R.args=malloc(2*sizeof(char *));
			l=strlen(begin);
			if (l>MAXSTRLEN)
			{
				/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
				// ERRORFLAG ERRLINETOOLONG  "Input string is too long, truncated input"
				AddErr(ERRLINETOOLONG);
				l=MAXSTRLEN;
			}
			R.args[0]=malloc((l+1)*sizeof(char));
			R.Nargs=1;
			strncpy(R.args[0], begin, l+1);
			R.args[0][l+1]='\0';
			begin+=l;
			R.next=begin;
			return R;					
		}
		else if (K.P==PD_ENDLINE)
		{
			R.P=K.P;
			R.next=begin+2;
			return R;
		}
		else if (K.P==PD_KERN)
		{
			char *p, *e, c;
			int l;
			float f;
			R.P=K.P;
			R.args=malloc(sizeof(char *));
			// we do everything twice ...
			begin+=5;
			p=GetNumPart(begin, &f);
			e=CommandEnd(p);
			c=*e;
			*e='\0';
			f=LookupUnit(p);
			if (f>=0)
			{
				l=strlen(begin);
				R.args[0]=malloc((l+1)*sizeof(char));
				R.Nargs=1;
				strncpy(R.args[0], begin, l+1);
				R.args[0][l+1]='\0';
				begin+=l+1;
				*e=c;
				R.next=begin;
			}
			else
			{ // only a number?				
				*e=c;
				if (p>begin)
				{
					c=(*p);
					*p='\0';
					l=strlen(begin);
					R.args[0]=malloc((l+1)*sizeof(char));
					R.Nargs=1;
					strncpy(R.args[0], begin, l+1);
					R.args[0][l+1]='\0';
					begin+=l;
					*p=c;
					R.next=begin;
				}
				else
				{
					/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
					// ERRORFLAG ERRTOOFEWMANDARG  "Too few mandatory arguments to command"
					AddErr(ERRTOOFEWMANDARG);
					R.P=PD_NONE; /* cancel this token, it is not working */
				}
			}
			return R;
		}
		else if (K.name!=NULL)
		{
			char *str;
			/* it is a command */
			/* move pointer to the arguments */
			R.P=K.P;
			begin+=strlen(K.name);
			n=K.Nopt;
			/* re for optional arguments */
			
			while ((str=Option(begin, &end))&&n)
			{
				/* we have an option */
				if (!R.opt)
					R.opt=malloc(K.Nopt*sizeof(char *));
				R.opt[R.Nopt]=str;
				/* move begin forward (to beyond the brackets) */	
				begin=end;
				R.Nopt++;
				n--;			
			}
			str=Option(begin, &end);
			if (str)
			{
				/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
				// ERRORFLAG ERRTOOMANYOPTARG  "Too many optional arguments to command, excess ignored"
				AddErr(ERRTOOMANYOPTARG);
				free(str);
				while ((str=Option(begin, &end))) /*skip excess optional arguments*/
				{
					free(str);
					begin=end;
				}
			}
			
			n=K.Nargs;
			/* when we have an environment we have somthing like \begin{env}[]..{}.. 
			 * Here we only process the first argument to begin and are not upset if more arguments follow
			 * */
			while (n>0) 
			{
				/* we have an argument */
				if ((str=Argument(begin, &end))!=NULL)
				{
					if (!R.args)
						R.args=malloc(K.Nargs*sizeof(char *));
					R.args[R.Nargs]=str;	
					begin=end;
					R.Nargs++;
					n--;
				}
				else
					n=-1;
			}
			if (!(n==0))
			{
				AddErr(ERRTOOFEWMANDARG);
				R.P=PD_NONE; /* cancel this token, it is not working */
				return R;
			}
			PeekAhead(&R, begin);
			return R;
		}
		else
		{
			/* interpret as escape sequence */
			/* we cannot escape characters in the range a-z and A-Z */
			begin++;
			if (!IsCommandChar(*begin))
			{
				if (R.P==PD_NONE)
					R.P=PD_SYMBOL;
				R.args=malloc(2*sizeof(char *));
				R.Nargs=1;
				R.args[0]=malloc(2*sizeof(char));
				R.args[0][0]=*begin;
				R.args[0][1]='\0';
				begin++;
				
				PeekAhead(&R, begin);
				return R;
			}
			else
			{
				// ERRORFLAG ERRUNKNOWNCOMM  "Unknown command"
				AddErr(ERRUNKNOWNCOMM);
				R.P=PD_NONE;
				return R;
			}
		}
	}
	else if (*begin=='$')
	{
		/* we have a "rootfont" block */
		
		int i;
		char *str;
		R.P=PD_ROOTFONT;
		str=begin+1;
		while ((*str!='$')&&(*str))
			str++;
		if (*str)
			str++;
		else
		{
			/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
			// ERRORFLAG ERRUNMATCHDOLLAR  "Missing $ inserted"
			AddErr(ERRUNMATCHDOLLAR);
		}
		
		R.args=malloc(2*sizeof(char *));
		R.args[0]=malloc((str-begin+1)*sizeof(char));
		i=0;
		begin++;
		while (begin<str-1)
		{
			R.args[0][i++]=*begin;
			begin++;
		}
		R.args[0][i]='\0';
		R.Nargs=1;	
		begin=str;
		PeekAhead(&R, begin);
		return R;
	}
	else if (*begin=='{')
	{
		/* we have a block */
		int brac=1,i, esc=0;
		char *str;
		R.P=PD_BLOCK;
		str=begin+1;
		while ((brac>0)&&(*str))
		{
			if (!esc)
			{
				if (*str=='}')
					brac--;
				else if (*str=='{')
					brac++;
				else if (*str=='\\')
					esc=1;
			}
			else
				esc=0;
			str++;
		}
		if (brac)
		{
			/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
			// ERRORFLAG ERRUNMATCHBRAC  "Missing } inserted"
			AddErr(ERRUNMATCHBRAC);
		}
		
		R.args=malloc(2*sizeof(char *));
		R.args[0]=malloc((str-begin+1)*sizeof(char));
		i=0;
		begin++;
		while (begin<str-1)
		{
			R.args[0][i++]=*begin;
			begin++;
		}
		R.args[0][i]='\0';
		R.Nargs=1;	
		begin=str;
		PeekAhead(&R, begin);
		return R;
	}
	else if (*begin=='\'') // primes are handled in the lexer, i.e. they do not occur in parsedef!
	{
		R.P=PD_PRIME;
		j=1;
		p=begin+1;
		while ((j<255)&&(*p=='\''))
		{
			j++;
			p++;
		}
		if (j==255)
		{
			/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
			// ERRORFLAG ERRTOOMANYPRIMES  "Too many primes"
			AddErr(ERRTOOMANYPRIMES);
		}
		R.args=malloc(2*sizeof(char *));
		R.Nargs=1;
		R.args[0]=malloc(2*sizeof(char));
		R.args[0][0]=(char)j;
		R.args[0][1]='\0';
			
		begin=p;
		PeekAhead(&R, begin);
		return R;		
	}
	else if ((*begin=='^')||(*begin=='_'))
	{
		// make a 0x1 box token with sub/superscripts
		R.P=PD_BOX;
		R.args=malloc(2*sizeof(char *));
		R.Nargs=2;
		R.args[0]=malloc(2*sizeof(char));
		R.args[0][0]='0';
		R.args[0][1]='\0';
		R.args[1]=malloc(2*sizeof(char));
		R.args[1][0]='1';
		R.args[1][1]='\0';
		PeekAhead(&R, begin);
		return R;		
	}
	/* we have a plain character */
	if (R.P==PD_NONE)
		R.P=PD_SYMBOL;
	R.args=malloc(2*sizeof(char *));
	R.Nargs=1;
		
	p=begin+1;
	while ((*p)&&(!IsInSet(*p, "\\_^/*{ +-\'")))
		p++;
	while (*p==' ')
		p++;
	c=*p;
	*p='\0';
	j=strlen(begin);
	R.args[0]=malloc((j+1)*sizeof(char));
	strncpy(R.args[0], begin, j+1);
	*p=c;
	// strip multiple spaces
	k=0;
	nsp=0;
	for (i=0;i<=j;i++)
	{
		R.args[0][k]=R.args[0][i];
		
		if (IsInSet(R.args[0][i], " \t"))
		{
			if (nsp==0)
				k++;
			nsp=1;
		}
		else
		{
			nsp=0;
			k++;
		}
	}
	
	begin=p;
	
	PeekAhead(&R, begin);
	return R;
	
	
}

TOKEN Lexer(char *begin, FONT F)
{
	TOKEN T;
	T=SubLexer(begin, F);
	
	if (T.P==PD_BEGIN)
		return BeginEnv(T);
	return T;
}

/* the tex operators '\over' and '\choose' work very different from most operators 
 * as the arguments are not after the command, like, e.g. \frac{a}{b}.
 * I treat these commands like I treat '^' and '_', which are also inbetween the 
 * arguments they work on. However, unlike '^' and '_', \over and \choose are greedy,
 * that is:
 * a + b \over c
 * means (a+b)/c and not a + b/c
 *
 * I use the following function to make operators greedy. It processes the input to 
 * change
 * a + b\over c
 * into
 * {a + b}\over {c}
 * 
 * Note that this approach is not very efficient but given I only know of \over 
 * and \choose to work this way I figure we're good.
 * 
 * Note, the followingt is ambiguous
 * 
 * a \choose b + c\choose d
 * 
 * do you want {a \choose b + c}\choose d:
 * 
 * ⎛⎛  a  ⎞⎞
 * ⎜⎜     ⎟⎟
 * ⎜⎝b + c⎠⎟
 * ⎜       ⎟
 * ⎝   d   ⎠
 * 
 * or a \choose {b + c\choose d}:
 * 
 * ⎛   a   ⎞
 * ⎜       ⎟
 * ⎜⎛b + c⎞⎟
 * ⎜⎜     ⎟⎟
 * ⎝⎝  d  ⎠⎠
 * 
 * Latex throws an error
 * my greedy routine is not that smart and as a resuylt I simply have an order of presedence
 * where \over precedes over \choose (always) and the first appearance over any subsequent.
 * i.e.
 * 
 * a \choose b + c\choose d
 * is processed as:
 * {a }\choose {{b + c}\choose {d}}:
 * and 
 * a \choose b + c\over d
 * as
 * {{a }\choose {b + c}}\over {d}:
 * 
 * The routine reallocates the input string to make space for inserted braces
 */
char *PreProcessGreedyOverLikeOperator(char *string, const char *op, int nop)
{
	int p,q,end,e,s;
	int na;
	na=strlen(string)+1;	
	p=0;
	end=na-1;

	while (string[p])
	{
		if (strncmp(string+p, op,nop)==0)
		{
			if (((string[p+nop]>='A')&&(string[p+nop]<='Z'))||((string[p+nop]>='a')&&(string[p+nop]<='z')))
				break;
			// check for closing bracket before
			q=p-1;
			if (string[q]!='}')
			{
				int brac=1;
				// insert brackets to make over greedy
				e=p; // end				
				s=q; // start
				while ((s>0)&&(brac>0))
				{
					s--;
					if (string[s]=='{')
						brac--;
					if (string[s]=='}')
						brac++;
				}
				
				// add two characters
				na+=2;
				string=realloc(string,na*sizeof(char));
				q=end;
				end+=2;
				p+=2; // increment ref position
				while (q>=e)
				{
					string[q+2]=string[q];
					q--;
				}
				string[q+2]='}';
				while (q>=s)
				{
					string[q+1]=string[q];
					q--;
				}
				string[s]='{';
			}
				
			// check for opening bracket after
			q=p+nop;
			if ((string[q]==' ')&&(q<end))
				q++;
					
			if (string[q]!='{')
			{
				int brac=1;
				// insert brackets to make over greedy
				s=q; // start
				
				e=q;
				while ((e<end)&&(brac>0))
				{
					e++;
					if (string[e]=='}')
						brac--;
					if (string[e]=='{')
						brac++;
				}
				na+=2;
				string=realloc(string,na*sizeof(char));
				q=end;
				end+=2;
				while (q>=e)
				{
					string[q+2]=string[q];
					q--;
				}
				string[q+2]='}';
				while (q>=s)
				{
					string[q+1]=string[q];
					q--;
				}
				string[s]='{';
			}	
		}
		p++;
	}
	return string;
}


/* substitutes in the input string all symbols found in the Symbols table
 * This also includes combining diacritical marks
 * The routine creates a new allocated string with a modified copy.
 */
char * PreProcessorSymb(char *string)
{
	char *res, *str, *c;
	int na, p=0;
	int Ncomb=0, *comb, Nacomb=4;
	
	Symbol S;
	KEYWORD K;
	na=strlen(string)+1;
	res=malloc(na*sizeof(char));
	comb=malloc(Nacomb*sizeof(int));
	
	while (*string)
	{
		if (*string=='\\')
		{
			/* we have something of the form \comm[]..{}.. */
			K=LookupKey(string, Keys);
			if (K.name)
			{
				c=K.name;
				while(*c)
				{
					res[p]=*c;
					string++;
					c++;
					p++;
					if (p==na)
					{
						na+=16;
						res=realloc(res,na*sizeof(char));
					}
				}
					
				continue;
			}
			
			
			S=IsSymbol(string, Symbols);
			if (S.name!=NULL)
			{
				str=Unicode2Utf8(S.unicode);
				if(IsCombiningMark(S.unicode))
				{
						/* we need to swap order, i.e. combining diacritrical mark follows whatever it should be combined with, put it on the stack */
						comb[Ncomb]=S.unicode;
						Ncomb++;
						if (Ncomb==Nacomb)
						{
							Nacomb+=4;
							comb=realloc(comb,Nacomb*sizeof(int));								
						}
						free(str);
				}
				else
				{
					c=str;
					while(*c)
					{
						res[p]=*c;
						p++;
						c++;
						if (p==na)
						{
							na+=16;
							res=realloc(res,na*sizeof(char));
						}
					}
					free(str);
					while (Ncomb)
					{
						str=Unicode2Utf8(comb[Ncomb-1]);
						
						c=str;
						while(*c)
						{
							res[p]=*c;
							p++;
							c++;
							if (p==na)
							{
								na+=16;
								res=realloc(res,na*sizeof(char));
							}
						}
						free(str);
						Ncomb--;
					}
				}
				string+=strlen(S.name);
				if (IsInSet(*string, " "))
					string++;
				
			}
			else
			{
				res[p]=*string;
				p++;
				string++;
				
				while (Ncomb)
				{
					str=Unicode2Utf8(comb[Ncomb-1]);
					
					c=str;
					while(*c)
					{
						res[p]=*c;
						p++;
						c++;
						if (p==na)
						{
							na+=16;
							res=realloc(res,na*sizeof(char));
						}
					}
					free(str);
					Ncomb--;
				}
			}
		}
		else if (IsInSet(*string, "\n"))
			string++;
		else
		{
			res[p]=*string;
			p++;
			string++;
			while (Ncomb)
			{
				str=Unicode2Utf8(comb[Ncomb-1]);
				
				c=str;
				while(*c)
				{
					res[p]=*c;
					p++;
					c++;
					if (p==na)
					{
						na+=16;
						res=realloc(res,na*sizeof(char));
					}
				}
				free(str);
				Ncomb--;
			}
			
		}
		if (p==na)
		{
			na+=16;
			res=realloc(res,na*sizeof(char));
		}
	}
	res[p]='\0';
	free(comb);
	return res;
}

char * PreProcessor(char *string)
{
	char *res;
	res=PreProcessorSymb(string);// allocates a new string res
	res=PreProcessGreedyOverLikeOperator(res, "\\over", 5); // reallocates res if needed
	res=PreProcessGreedyOverLikeOperator(res, "\\choose", 7);
	return res;
}
