#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include "stringutils.h"
#include "boxes.h"
#include "error.h"

char *RemoveLineTrailingWhitespace(char *str)
{
	// search for whitespace ending with endline
	char *p, *q;
	p=str;
	q=str;
	while (*p)
	{
		if (*p==' ')
		{
			char *z;
			z=p;
			while(*z==' ')
				z++;
			if ((*z=='\n')||(*z=='\0'))
				p=z;
			else
			{
				while(p<z)
				{
					*q=*p;
					p++;
					q++;
				}				
			}
		}
		*q=*p;
		p++;
		q++;
	}
	*q='\0';
	return str;
}
		
	


char * DrawBox(box *b)
{
	char *out;
	int k=0;
	out=calloc(1,sizeof(char));
	if (b->S==ABSPOSKNOWN)
	{
		char *buffer;
		int i, w=0;
		int x, y, off=0, Na;
		box *bb;
		if ((b->ax!=0)||(b->ay!=0))
		{
			/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
			// ERRORFLAG ERRDRAWBOXNOROOT "Drawbox needs a rootbox as input"
			AddErr(ERRDRAWBOXNOROOT);
			out[0]='\0';	/* return empty string */
			return out;
		}
		buffer=malloc((b->w+1)*sizeof(char));
		w=b->w;
		Na=w+1;
		for (y=b->h-1;y>=0;y--)
		{
			off=0; /* offset from multibyte and combinational characters */
			
			for (x=0;x<b->w;x++)
			{
				bb=FindBoxAtPos(b, x, y);
				if (bb)
				{
					if (bb->ax==x)
					{
						/* insert unit box bb's string in our buffer */
						char *str;
						str=UnicodeMapper((char *) bb->content);
						i=x+off;
						off+=strlen(str)-strspaces(str); /* update offset */
						if (i+strlen(str)>=Na-1)
						{
							Na+=strlen(str)+2;
							buffer=realloc(buffer, Na*sizeof(char));
						}
						sprintf(buffer+i, "%s", str);
						i+=strlen(str);
						free(str);
					}
				}
				else
				{
					/* nothing there, fill up with whitespace */
					if (x+off+1>=Na-1)
					{
						Na+=2;
						buffer=realloc(buffer, Na*sizeof(char));
					}
					buffer[x+off]=' ';
					
				}
			}
			out=realloc(out,(k+w+off+2)*sizeof(char));
			for (i=0;i<w+off;i++)
			{
				out[k]=buffer[i];
				k++;
			}
			out[k]='\n';
			k++;
			out[k]='\0';
		}
		
		free(buffer);
	}
	else
	{
		/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
		// ERRORFLAG ERRABSPOSUNKNOWN "DrawBox cannot draw box, box positions not absolute"
		AddErr(ERRABSPOSUNKNOWN);
	}
	if (k)
		out[k-1]='\0';
	else
		out[k]='\0';
	return RemoveLineTrailingWhitespace(out);
}

int PrintBox(box *b)
{
	char *eq;
	int N;
	eq=DrawBox(b);

	N=printf("%s\n",eq);
	free(eq);
	return N;
}
void Indent(int indent)
{
	int i;
	for (i=0;i<indent;i++)
		printf(" ");
}
#define NIDENT 2
void DrawBoxTreeRec(box *b, int indent)
{
	int i;
	Indent(indent);
	printf("Box:\n");
	Indent(indent);
	printf("State: %d\n", b->S);
	Indent(indent);
	printf("Pos:\n");
	Indent(indent+NIDENT);
	if (b->S==ABSPOSKNOWN)
		printf("(x,y)=(%d,%d)\n", b->ax, b->ay);
	else
		printf("(x,y)=(?,?)\n");
	
	Indent(indent+NIDENT);
	if (b->S>=RELPOSKNOWN)
		printf("(rx,ry)=(%d,%d)\n", b->rx, b->ry);
		else
			printf("(rx,ry)=(?,?)\n");	
	if (b->S>=SIZEKNOWN)
	{				
		Indent(indent+NIDENT);
		printf("(xc,yc)=(%d,%d)\n", b->xc, b->yc);
		Indent(indent+NIDENT);
		printf("(X,Y)=(%d,%d)\n", b->X, b->Y);
		Indent(indent+NIDENT);
		printf("(w,h)=(%d,%d)\n", b->w, b->h);
	}
	else
	{				
		Indent(indent+NIDENT);
		printf("(xc,yc)=(?,?)\n");
		Indent(indent+NIDENT);
		printf("(X,Y)=(?,?)\n");
		Indent(indent+NIDENT);
		printf("(w,h)=(?,?)\n");
	}
	Indent(indent);
	printf("Type: ");		
	switch(b->T)
	{
		case B_UNIT:
		{
			char *str;
			printf("UNIT\n");
			str=UnicodeMapper((char *) b->content);
			Indent(indent+NIDENT);
			printf("Content: %s\n",str);
			free(str);
			break;
		}
		case B_ARRAY:
			printf("ARRAY\n");
			Indent(indent+NIDENT);
			printf("Nc=%d\n", b->Nc);
			for (i=0;i<b->Nc;i++)
				DrawBoxTreeRec(b->child+i, indent+NIDENT);
			break;
		case B_POS:	
			printf("POS\n");
			Indent(indent+NIDENT);
			printf("Nc=%d\n", b->Nc);
			for (i=0;i<b->Nc;i++)
				DrawBoxTreeRec(b->child+i, indent+NIDENT);
			break;
		case B_DUMMY:	
			printf("DUMMY\n");
			break;	
		case B_LINE:
			printf("LINE\n");
			Indent(indent+NIDENT);
			printf("Nc=%d\n", b->Nc);
			for (i=0;i<b->Nc;i++)
				DrawBoxTreeRec(b->child+i, indent+NIDENT);
			break;			
		case B_ENDLINE:	
			printf("ENDLINE\n");
			break;
		default:
			break;
	}
	
}
void DrawBoxTree(box *b)
{
	DrawBoxTreeRec(b, 0);
}
