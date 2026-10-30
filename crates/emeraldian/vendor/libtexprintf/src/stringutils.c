#include <stdlib.h>
#include <stdio.h>
#include <locale.h>
#include <string.h>
#include "unicodeblocks.h"
#include "stringutils.h"
#include "error.h"

#define IsPrintable(C) iswprint((C))

int IsInSet(char c, const char *set) /* check whether char c in set */
{
	char *s;
	s=(char *)set;
	while (*s)
	{
		if ((*s)==c)
			return 1;
		s++;
	}
	return 0;
		
}
char * Unicode2Utf8(int U) /* convert unicode code point to an allocated and null teminated string (i.e. 5 bytes) */
{
	char *r, *b;
	r=calloc(5,sizeof(char));
	b=r;
	if (U<0x80)
		*b++=U;
	else if (U<0x800)
	{
		*b++=192+U/64;
		*b++=128+U%64;
	}
	else if (U-0xd800u<0x800)
	{
		fprintf(stderr, "Invalid Unicode Code Point 0x%X\n", U);
		return r;
	}
	else if (U<0x10000)
	{
		*b++=224+U/4096;
		*b++=128+U/64%64;
		*b++=128+U%64;
	}
	else if (U<0x110000)
	{
		*b++=240+U/262144;
		*b++=128+U/4096%64;
		*b++=128+U/64%64;
		*b++=128+U%64;
	}
	else 
		fprintf(stderr, "Invalid Unicode Code Point 0x%X\n", U);
	return r;
}

int NumByte(char *p) /* return the number of bytes of the multibyte char pointed to by p */
{
	int n=1;
	if (*p & 0x80) /* multibyte char */
	{
		if ((*p & 0xF0) == 0xF0)/* 4-bytes */
			n=4;
		else if ((*p & 0xE0) == 0xE0)/* 3-bytes */
			n=3;
		else if ((*p & 0xC0) == 0xC0)/* 2-bytes */
			n=2;
	}
	return n;
}
int UNumByte(int U) /* return the number of UTF-8 bytes for the unicode code point */
{
	if (U<0x80)
		return 1;
	if (U<0x800)
		return 2;
	if (U-0xd800u<0x800)
	{
		fprintf(stderr, "Invalid Unicode Code Point 0x%X\n", U);
		return 0;
	}
	if (U<0x10000)
		return 3;
	if (U<0x110000)
		return 4;
	return 0;
}

int Unicode(char *p, int *N)/* convert the UTF-8 char pointed to by p to unicode, if N is provided it will be set to the number of bytes of the character*/
{
	int u=0;
	int n;
	n=NumByte(p);
	if (N)
		*N=n;
	switch(n)
	{
			case 4:
				u=(int)(0x07&*p);
				n--;
				break;
			case 3:
				u=(int)(0x0F&*p);
				n--;
				break;
			case 2:
				u=(int)(0x1F&*p);
				n--;
				break;
			case 1:
				u=(int)(*p);
				n--;
				break;
			default:
				break;
	}
	while (n)
	{
		/* each subsequent byte contains 6 relevant bits */
		p++;
		if ((*p&0xC0)!=0x80)
			return -1;
		u=(u<<6);
		u|=((int)(0x3F&*p));
		n--;
	}
	return u;
}

int IsCombiningMark(int ch) /* check whether the unicode code point ch is a combining mark */
{
	int i=0;
	while (Combining[i].start>=0)
	{
		if ((ch>=Combining[i].start)&&(ch<=Combining[i].end))
			return 1;
		i++;
	}
	return 0;
}

int WCSPACES=2;
int FCSPACES=2;
int IsWideChar(int ch) /* check whether the unicode code point ch is a wide character */
{
	int i=0;
	while (Wide[i].start>=0)
	{
		if ((ch>=Wide[i].start)&&(ch<=Wide[i].end))
			return 1;
		i++;
	}
	return 0;
}
int IsFullChar(int ch) /* check whether the unicode code point ch is a full width character */
{
	int i=0;
	while (Full[i].start>=0)
	{
		if ((ch>=Full[i].start)&&(ch<=Full[i].end))
			return 1;
		i++;
	}
	return 0;
}


int strspaces(char *p) /* count the number of character spaces in the string where each multibyte character counts as one and combining marks as 0 */ 
{
	int l=0;
	int n;
	int u;
	while(*p)
	{
		u=Unicode(p, &n);
		if (u<0)
		{
			fprintf(stderr, "Error, string is not proper UTF-8 code\n");
			return 0;
		}
		if (!IsCombiningMark(u))
			l++;
		if (IsWideChar(u))
			l+=WCSPACES-1;
		if (IsFullChar(u)) /* we make wide characters two characters wide */
			l+=FCSPACES-1;
		p+=n;
	}
	return l;
}

typedef struct {
	int point;
	int mapped;
} UniMap;
int Nmap=24;
const UniMap Mappings[] = { 
	/* some unicode codeblocks have gaps in em wile some symbols are defined in another block. 
	 * This table maps the gaps to the correct codepoint. */
	{0x1D455, 0x210E}, /* Mathematical italic h */
	{0x1D49D, 0x212C}, /* Mathematical Script B */
	{0x1D4A0, 0x2130}, /* Mathematical Script E */
	{0x1D4A1, 0x2131}, /* Mathematical Script F */
	{0x1D4A3, 0x210B}, /* Mathematical Script H */
	{0x1D4A4, 0x2110}, /* Mathematical Script I */
	{0x1D4A7, 0x2112}, /* Mathematical Script L */
	{0x1D4A8, 0x2133}, /* Mathematical Script M */
	{0x1D4AD, 0x211B}, /* Mathematical Script R */	 
	{0x1D4BA, 0x212F}, /* Mathematical Script e */	  
	{0x1D4BC, 0x210A}, /* Mathematical Script g */	  
	{0x1D4C4, 0x2134}, /* Mathematical Script o */	 
	{0x1D506, 0x212D}, /* Mathematical Fraktur B */
	{0x1D50B, 0x210C}, /* Mathematical Fraktur H*/
	{0x1D50C, 0x2111}, /* Mathematical Fraktur I*/
	{0x1D515, 0x211C}, /* Mathematical Fraktur R*/
	{0x1D51D, 0x2128}, /* Mathematical Fraktur Z*/	
	{0x1D53A, 0x2102}, /* Mathematical double stroke capital C */
	{0x1D53F, 0x210D}, /* Mathematical double stroke capital H */
	{0x1D545, 0x2115}, /* Mathematical double stroke capital N */
	{0x1D547, 0x2119}, /* Mathematical double stroke capital P */
	{0x1D548, 0x211A}, /* Mathematical double stroke capital Q */
	{0x1D549, 0x211D}, /* Mathematical double stroke capital R */
	{0x1D551, 0x2124}, /* Mathematical double stroke capital Z */
};

int MapU(int in, const UniMap M[], int N)
{
	int i, min=0, max=N-1;
	do
	{
		i=(min+max)/2;
		if (M[i].point==in)
			return M[i].mapped;
		else if (M[i].point<in)
			min=i;
		else
			max=i;
	} while (max-min>1);
	if (M[min].point==in)
		return M[min].mapped;
	if (M[max].point==in)
		return M[max].mapped;
	return in;
}

char * UnicodeMapper(char *in)
{
	char *out;
	char *p, *u;
	int na=2*strlen(in), len, pos=0;
	int Nin, Nout, U, Um;
	if (!in)
		return NULL;
	out=malloc(na*sizeof(char));
	p=in;
	len=strlen(in);
	while (*p)
	{
		U=Unicode(p, &Nin);
		p+=Nin;
		
		Um=MapU(U,Mappings,Nmap);
		if (U!=Um)
		{
			u=Unicode2Utf8(Um);
			Unicode(u, &Nout);
			len+=(Nout-Nin);
			Nin=Nout;
			if (len>na)
			{
				na=len+10;
				out=realloc(out,na*sizeof(char));
			}
		}
		else
			u=Unicode2Utf8(Um);
		strcpy(out+pos,u);
		pos+=Nin;
		free(u);
	}		
	return out;	
}


/* map sub/super scripts 
 */
typedef struct {
	const char *c; // quick search whether the character is mappable
	const int N;
	const UniMap Map[];  // map table
} MapScriptTable;

MapScriptTable SuperScriptMap = {
	"231hjrwylsxABDEGHIJKLMNOPRTUWabdegkmoptuvcfz0i456789+-=()nV! ", // note we add a space
	60,
	{
		{0x00021,0x0A71D},
		{0x00028,0x0207D},
		{0x00029,0x0207E},
		{0x0002B,0x0207A},
		{0x0002D,0x0207B},
		{0x00030,0x02070},
		{0x00031,0x000B9},					// 1 -> ¹
		{0x00032,0x000B2},					// 2 -> ²
		{0x00033,0x000B3},					// 3 -> ³
		{0x00034,0x02074},
		{0x00035,0x02075},
		{0x00036,0x02076},
		{0x00037,0x02077},
		{0x00038,0x02078},
		{0x00039,0x02079},
		{0x0003D,0x0207C},
		{0x00041,0x01D2C},
		{0x00042,0x01D2E},
		{0x00044,0x01D30},
		{0x00045,0x01D31},
		{0x00047,0x01D33},
		{0x00048,0x01D34},
		{0x00049,0x01D35},
		{0x0004A,0x01D36},
		{0x0004B,0x01D37},
		{0x0004C,0x01D38},
		{0x0004D,0x01D39},
		{0x0004E,0x01D3A},
		{0x0004F,0x01D3C},
		{0x00050,0x01D3E},
		{0x00052,0x01D3F},
		{0x00054,0x01D40},
		{0x00055,0x01D41},
		{0x00056,0x02C7D},
		{0x00057,0x01D42},
		{0x00061,0x01D43},
		{0x00062,0x01D47},
		{0x00063,0x01D9C},
		{0x00064,0x01D48},
		{0x00065,0x01D49},
		{0x00066,0x01DA0},
		{0x00067,0x01D4D},
		{0x00068,0x002B0},
		{0x00069,0x02071},
		{0x0006A,0x002B2},
		{0x0006B,0x01D4F},
		{0x0006C,0x002E1},
		{0x0006D,0x01D50},
		{0x0006E,0x0207F},
		{0x0006F,0x01D52},
		{0x00070,0x01D56},
		{0x00072,0x002B3},
		{0x00073,0x002E2},
		{0x00074,0x01D57},
		{0x00075,0x01D58},
		{0x00076,0x01D5B},
		{0x00077,0x002B7},
		{0x00078,0x002E3},
		{0x00079,0x002B8},
		{0x0007A,0x01DBB},
	}
};

MapScriptTable SubScriptMap = {
	"iruv0123456789+-=()aeoxhklmnpstj ",// note we add a space
	32,
	{
		{0x00028,0x0208D},
		{0x00029,0x0208E},
		{0x0002B,0x0208A},
		{0x0002D,0x0208B},
		{0x00030,0x02080},
		{0x00031,0x02081},
		{0x00032,0x02082},
		{0x00033,0x02083},
		{0x00034,0x02084},
		{0x00035,0x02085},
		{0x00036,0x02086},
		{0x00037,0x02087},
		{0x00038,0x02088},
		{0x00039,0x02089},
		{0x0003D,0x0208C},
		{0x00061,0x02090},
		{0x00065,0x02091},
		{0x00068,0x02095},
		{0x00069,0x01D62},
		{0x0006A,0x02C7C},
		{0x0006B,0x02096},
		{0x0006C,0x02097},
		{0x0006D,0x02098},
		{0x0006E,0x02099},
		{0x0006F,0x02092},
		{0x00070,0x0209A},
		{0x00072,0x01D63},
		{0x00073,0x0209B},
		{0x00074,0x0209C},
		{0x00075,0x01D64},
		{0x00076,0x01D65},
		{0x00078,0x02093},
	}
};

int MappableScript(char *script, MapScriptTable *S)
{
	char *p;
	int r=1;
	p=script;
	while ((*p)&&(r))
	{
		r*=IsInSet(*p,S->c);
		p++;
	}
	return r;
}

char *MapScript(char *script, MapScriptTable *S)
{
	char *out;
	char *p, *u;
	int na=2*strlen(script), len, pos=0;
	int Nin, Nout, U, Um;
	out=malloc((na+1)*sizeof(char));
	p=script;
	len=strlen(script);
	while (*p)
	{
		U=Unicode(p, &Nin);
		p+=Nin;
		
		Um=MapU(U,S->Map,S->N);
		if (U!=Um)
		{
			u=Unicode2Utf8(Um);
			Unicode(u, &Nout);
			len+=(Nout-Nin);
			Nin=Nout;
			if (len>na)
			{
				na=len+10;
				out=realloc(out,(na+1)*sizeof(char));
			}
		}
		else
			u=Unicode2Utf8(Um);
		strcpy(out+pos,u);
		pos+=Nin;
		free(u);
	}		
	return out;
}

int MappableSuper(char *super)
{
	return MappableScript(super, &SuperScriptMap);
}
char *MapSuperScript(char *super)
{
	return MapScript(super, &SuperScriptMap);
}
int MappableSub(char *sub)
{
	return MappableScript(sub, &SubScriptMap);
}
char *MapSubScript(char *sub)
{
	return MapScript(sub, &SubScriptMap);
}
