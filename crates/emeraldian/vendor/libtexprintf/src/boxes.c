#include <stdlib.h>
#include <stdio.h>
#include "boxes.h"
#include "stringutils.h"
#include "error.h"

box InitBox(box *parent, boxtype T, void *content)
{
	box b;
	b.parent=parent;
	if (parent)
		parent->Nc++;
	
	b.child=NULL;
	b.Nc=0;
	b.X=CENTER;
	b.Y=CENTER;
	b.S=INIT;
	b.T=T;
	b.content=content;
	b.rx=0;
	b.ry=0;
	b.ax=0;
	b.ay=0;
	b.w=0;
	b.h=0;
	b.xc=0;
	b.yc=0;
	if (T==B_DUMMY)
	{
		b.w=((int *)content)[0];
		b.h=((int *)content)[1];
		b.S=SIZEKNOWN;		
	}
	if (T==B_ENDLINE)
	{
		b.w=0;
		b.h=0;
		b.S=SIZEKNOWN;		
	}
	return b;
}

void AddChild(box * parent, boxtype T, void *content)
{
	if (!parent->child)
		parent->child=malloc(sizeof(box));
	else
		parent->child=realloc(parent->child, (parent->Nc+1)*sizeof(box));
	parent->child[parent->Nc]=InitBox(parent, T, content);
}
	
void FreeBox(box *b)
{
	int i;
	if (b->Nc)
	{
		for (i=0;i<b->Nc;i++)
			FreeBox(b->child+i);
		free(b->child);
	}
	
	free(b->content);
}


int BoxInBox(box *b, boxtype T, void *content)
{
/* wraps the box b (not the root) into another box os specified type and content */
/* i.e. after running box in box a new box is created where b used to be and b becomes the (single) child of this box */
	box c;
	int i;
	
	if (!b->parent)
	{
		/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
		// ERRORFLAG ERRBOXINBOX  "BoxInBox cannot take the root box as an agument"
		AddErr(ERRBOXINBOX);
		return 1;
	}
	c=*b;
	b->Nc=1;
	b->child=malloc(sizeof(box));
	
	c.parent=b;
	b->child[0]=c;
	for (i=0;i<c.Nc;i++)
		b->child[0].child[i].parent=b->child;
	
	b->S=INIT;
	b->X=CENTER;
	b->Y=CENTER;
	b->T=T;
	b->content=content;
	b->rx=0;
	b->ry=0;
	b->ax=0;
	b->ay=0;
	b->w=0;
	b->h=0;
	b->xc=0;
	b->yc=0;
	return 0;
}

boxstate StateBoxtree(box *b, box **minstate)
{
	int i;
	box *ms=b;
	boxstate S;
	boxstate SUB;
	S=b->S;
	if (minstate)
		(*minstate)=b;
	for (i=0;i<b->Nc;i++)
	{
		SUB=StateBoxtree(b->child+i, &ms);
		if (SUB<=S)
		{
			S=SUB;
			if (minstate)
				(*minstate)=ms;
		}
	}
	return S;

}

box *FindBoxAtPos(box *b, int x, int y)
{
	if (b->S==ABSPOSKNOWN)
	{
		/* go to level where the coordinate is contained in the box */
		while ((b->parent)&&((x<b->ax)||(x>=b->ax+b->w)||(y<b->ay)||(y>=b->ay+b->h)))
			b=b->parent;
		
		if ((x<b->ax)||(x>=b->ax+b->w)||(y<b->ay)||(y>=b->ay+b->h))
			return NULL;
			
		while (b->Nc) /* as long as we have children */
		{
			int i=0, f=0;
			while (i<b->Nc)
			{
				if (!((x<b->child[i].ax)||(x>=b->child[i].ax+b->child[i].w)||(y<b->child[i].ay)||(y>=b->child[i].ay+b->child[i].h)))
				{
					b=b->child+i;
					if (b->T!=B_DUMMY) /* a dummy box is like no box */
						f=1;
					break; /* found a box containing the coordinate */
				}
				i++;
			}
			if (f==0)
				return NULL; /* no unit box containing this coordinate */
		}
		return b;
	
	}
	else
	{
		/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
		// ERRORFLAG ERRBOXATPOS  "Box positions unknown in FindBoxAtPos"
		AddErr(ERRBOXATPOS);
		return NULL;
	}
}



int UnitBoxSize(box *b)
{
	if (b->T!=B_UNIT)
	{
		/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
		// ERRORFLAG ERRUBOXSIZE  "Call of UnitBoxSize on something not a unit box"
		AddErr(ERRUBOXSIZE);
		return 1;
	}
	if (b->S<SIZEKNOWN)
	{
		char *str;
		str=(char *) b->content;
		b->w=strspaces(str);
		b->h=1;
		switch(b->X)
		{
			case MAX:
				b->xc=b->w;
				break;
			case MIN:
				b->xc=0;
				break;
			case CENTER:
				b->xc=(b->w-1)/2;
				break;
			default:
				break;
		}
		b->yc=0;
		b->S=SIZEKNOWN;
	}
	return 0;
}


int ArrayBoxSize(box *b)
{
	int i, r, c, Nc, Nr, *h, *w, *xc, *yc, *rx, *ry;
	int *content;
	
	if (b->T!=B_ARRAY)
	{
		/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
		// ERRORFLAG ERRABOXSIZE  "Call of ArrayBoxSize on something not an array box"
		AddErr(ERRABOXSIZE);
		return 1;
	}
	/* first compute all sizes of all children boxes */
	if (BoxSize_children(b))
		return 1;
	
	/* Sizes of all children are known, now align the children */
	/* fetch number of columns from the content blob */
	content=(int *)b->content;
	Nc=content[0];
	
	if (Nc<=0)
	{
		Nr=1;
		Nc=b->Nc;
	}
	else /* number of rows: */
		Nr=b->Nc/Nc+(b->Nc%Nc>0);
	
	/* allocate row and column heights, widths respectively */
	h=calloc(Nr,sizeof(int));
	yc=calloc(Nr,sizeof(int));
	w=calloc(Nc,sizeof(int));
	xc=calloc(Nc,sizeof(int));
	ry=calloc(Nr,sizeof(int));
	rx=calloc(Nc,sizeof(int));
	
	
	for (i=0;i<b->Nc;i++)
	{
		int m;
		c=i%Nc;
		r=i/Nc;
		
		/* upper half of the box */
		m=(b->child[i].h-b->child[i].yc);
		if (m>h[r]-yc[r])
			h[r]+=m-h[r]+yc[r];
			
			
		/* lower half of the box */
		m=(b->child[i].yc);
		if (m>yc[r])
		{
			/* height increases along with the baseline */
			h[r]+=m-yc[r];
			yc[r]+=m-yc[r];
		}
		
		/* right half of the box */
		m=(b->child[i].w-b->child[i].xc);
		if (m>w[c]-xc[c])
			w[c]+=m-w[c]+xc[c];
			
			
		/* left half of the box */
		m=(b->child[i].xc);
		if (m>xc[c])
		{
			/* height increases along with the baseline */
			w[c]+=m-xc[c];
			xc[c]+=m-xc[c];
		}
	}
	for (i=1;i<Nc;i++)
		rx[i]=rx[i-1]+w[i-1];
	for (i=Nr-2;i>=0;i--)
		ry[i]=ry[i+1]+h[i+1];
	b->w=rx[Nc-1]+w[Nc-1];
	b->h=ry[0]+h[0];
	b->S=SIZEKNOWN;
	switch(b->X)
	{
		case MAX:
			b->xc=b->w;
			break;
		case MIN:
			b->xc=0;
			break;
		case CENTER:
			b->xc=(b->w-1)/2;
			break;
		default:
			break;
	}
	switch(b->Y)
	{
		case MAX:
			b->yc=b->h;
			break;
		case MIN:
			b->yc=0;
			break;
		case CENTER:
			b->yc=(b->h-1)/2;
			break;
		default:
			break;
	}
	
	/* propagate the x an y positions of the boxes */
	
	for (i=0;i<b->Nc;i++)
	{
		c=i%Nc;
		r=i/Nc;
		b->child[i].ry=ry[r]+(yc[r]-b->child[i].yc);
		b->child[i].rx=rx[c]+(xc[c]-b->child[i].xc);
		b->child[i].S=RELPOSKNOWN;
	}
	
	free(h);
	free(w);
	free(yc);
	free(xc);
	free(ry);
	free(rx);
	return 0;
}

#define WIDTH b->w
#define HEIGHT b->h
int PosBoxSize(box *b)
{
	int i, *xy;
	void *content;
	
	
	if (b->T!=B_POS)
	{
		/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
		// ERRORFLAG ERRPBOXSIZE  "Call of PosBoxSize on something not a pos box"
		AddErr(ERRPBOXSIZE);
		return 1;
	}
	/* first compute all sizes of all children boxes */
	if (BoxSize_children(b))
		return 1;
	
	/* Sizes of all children are known, now align the children */
	/* fetch number of columns from the content blob */
	content=b->content;
	xy=(int *)content;
	
	WIDTH=0;
	HEIGHT=0;
	for (i=0;i<b->Nc;i++)
	{
		if ((xy[2*i]<0)||(xy[2*i+1]<0))
		{
			/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
			// ERRORFLAG ERRNEGRELPOS  "Relative positions may not be negative in PosBoxSize"
			AddErr(ERRNEGRELPOS);
			return 1;
		}
		b->child[i].rx=xy[2*i];
		b->child[i].ry=xy[2*i+1];
		b->child[i].S=RELPOSKNOWN;
		if (b->child[i].rx+b->child[i].w>WIDTH)
			WIDTH=b->child[i].rx+b->child[i].w;
		if (b->child[i].ry+b->child[i].h>HEIGHT)
			HEIGHT=b->child[i].ry+b->child[i].h;
	}
	b->S=SIZEKNOWN;
	switch(b->X)
	{
		case MAX:
			b->xc=WIDTH;
			break;
		case MIN:
			b->xc=0;
			break;
		case CENTER:
			b->xc=(WIDTH-1)/2;
			break;
		default:
			break;
	}
	switch(b->Y)
	{
		case MAX:
			b->yc=HEIGHT;
			break;
		case MIN:
			b->yc=0;
			break;
		case CENTER:
			b->yc=(HEIGHT-1)/2;
			break;
		default:
			break;
	}
	return 0;
}


int DummyBoxSize(box *b)
{
	if (b->T!=B_DUMMY)
	{
		/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
		// ERRORFLAG ERRDBOXSIZE  "Call of DummyBoxSize on something not a dummy box"
		AddErr(ERRDBOXSIZE);
		return 1;
	}	
	if (b->S<SIZEKNOWN)
		b->S=SIZEKNOWN;
	return 0;
}

int EndlineBoxSize(box *b)
{
	if (b->T!=B_ENDLINE)
	{
		/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
		// ERRORFLAG ERRELBOXSIZE  "Call of EndlineBoxSize on something not a endline box"
		AddErr(ERRELBOXSIZE);
		return 1;
	}
	b->h=0;	
	b->w=0;	
	b->yc=0;	
	b->xc=0;	
	if (b->S<SIZEKNOWN)
		b->S=SIZEKNOWN;
	return 0;
}


int LineBoxSize(box *b)
{
	int i, j, LW, X, *line, nl=0, H=0, Yc=0, W=0, *y, *yc, nra=2;
	int *content;
	
	if (b->T!=B_LINE)
	{
		/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
		// ERRORFLAG ERRLBOXSIZE "LineBoxSize can only be used on line boxes"
		AddErr(ERRLBOXSIZE);
		return 1;
	}
	/* first compute all sizes of all children boxes */
	if (BoxSize_children(b))
		return 1;
	
	/* fetch the line width from the binary blob */
	content=(int *)b->content;
	LW=content[0];
	line=malloc(b->Nc*sizeof(int));
	y=malloc(nra*sizeof(int));
	yc=malloc(nra*sizeof(int));
	
	if (LW<0)
		LW=0;
	
	
	X=0;
	for (i=0;i<b->Nc;i++)
	{
		/* we first set the X positions for each box */
		if (((LW>0)&&(X+b->child[i].w>LW)&&(X>0))||(b->child[i].T==B_ENDLINE))/* if X = 0 -> w>LW, we stay on this line */
		{
			for (j=0;j<nl;j++)
				y[j]+=H;
			y[nl]=H;	/* store current line height */
			yc[nl]=Yc;	/* store current baseline */
			H=0;
			Yc=0;
			nl++;
			X=0;
			if (nl==nra)
			{
				nra+=2;
				y=realloc(y,nra*sizeof(int));
				yc=realloc(yc,nra*sizeof(int));
			}
		}
		b->child[i].rx=X;
		X+=b->child[i].w;
		if (X>W)	/* compute maximum line width */
			W=X;
		line[i]=nl;
		/* compute height and baseline of the current line */
		if (H-Yc<b->child[i].h-b->child[i].yc)
			H+=(b->child[i].h-b->child[i].yc)-(H-Yc);
		if (Yc<b->child[i].yc)
		{
			H+=b->child[i].yc-Yc;
			Yc=b->child[i].yc;
		}
	}
	for (j=0;j<nl;j++)
		y[j]+=H;
	y[nl]=H;	/* store current line height */
	
	yc[nl]=Yc;	/* store current baseline */
	H=y[0];
	for (j=0;j<nl;j++)
		y[j]=y[j+1];
	y[nl]=0;
	
	
	for (i=b->Nc-1;i>=0;i--)
	{
		/* set relative y coordinates */
		b->child[i].ry=y[line[i]]+(yc[line[i]]-b->child[i].yc);
		b->child[i].S=RELPOSKNOWN;
	}
	b->h=H;
	b->w=W;
	b->S=SIZEKNOWN;
	
	switch(b->X)
	{
		case MAX:
			b->xc=WIDTH;
			break;
		case MIN:
			b->xc=0;
			break;
		case CENTER:
			b->xc=(WIDTH-1)/2;
			break;
		default:
			break;
	}
	if (nl==0)
		b->yc=yc[0]; /* if we do not have a linebox spanning several lines we must preserve the baseline */
	else
	{
		switch(b->Y)
		{
			case MAX:
				b->yc=HEIGHT;
				break;
			case MIN:
				b->yc=0;
				break;
			case CENTER:
				b->yc=(HEIGHT-1)/2;
				break;
			default:
				break;
		}
	}

	
	free(y);
	free(yc);
	free(line);
	return 0;
}

int BoxSize_children(box *b) /* computes size of the children */
{
	int i;
	boxstate S;
	box *ms=b;
	int err=0;
	
	for (i=0;i<b->Nc;i++)
	{
		while((S=StateBoxtree(b->child+i, &ms))==INIT)
		{
			switch(ms->T)
			{
				case B_UNIT:
					err+=UnitBoxSize(ms);
					break;
				case B_ARRAY:
					err+=ArrayBoxSize(ms);
					break;
				case B_POS:
					err+=PosBoxSize(ms);
					break;
				case B_DUMMY:
					ms->S=SIZEKNOWN;
					break;
				case B_LINE:
					err+=LineBoxSize(ms);
					break;
				case B_ENDLINE:
					err+=EndlineBoxSize(ms);
					break;
				default:
					/* The following comment line lets the gen_errorflags.sh script generate appropriate error flags and messages */
					// ERRORFLAG ERRUNKNOWNBOX "Unknown box type in BoxSize"
					AddErr(ERRUNKNOWNBOX);
					return 1;
			}
		}
	}
	if (err)
		return 1;
	return 0;
}

int BoxSize(box *b) /* computes size of box b */
{
	if (b->S==INIT)
	{
		switch(b->T)
		{
			case B_UNIT:
				return UnitBoxSize(b);
				break;
			case B_ARRAY:
				return ArrayBoxSize(b);
				break;
			case B_POS:
				return PosBoxSize(b);
				break;
			case B_DUMMY:
				return 0; /* dummy box size is known from the start */
				break;
			case B_LINE:
				return LineBoxSize(b);
				break;
			case B_ENDLINE:
				return EndlineBoxSize(b);
				break;
			default:
				AddErr(ERRUNKNOWNBOX); /* reuse previously defined unknown box type error */
				return 1;
		}		
	}
	return 0; /* if not init, size must be known already */
}

void BoxPos_recursive(box *b)
{
	int i;
	for (i=0;i<b->Nc;i++)
	{
		b->child[i].ax=b->ax+b->child[i].rx;
		b->child[i].ay=b->ay+b->child[i].ry;
		b->child[i].S=ABSPOSKNOWN;
		BoxPos_recursive(b->child+i);
	}
	
}
void BoxPos(box *b)
{
	if (b->S<SIZEKNOWN)
		BoxSize(b);
		
	/* we treat the b that is passed as the root node */
	b->ax=0;
	b->ay=0;
	b->S=ABSPOSKNOWN;
	BoxPos_recursive(b);
}

void BoxSetState_recursive(box *b, boxstate S)
{
	int i;
	for (i=0;i<b->Nc;i++)
	{
		if (b->child[i].S>S)
			b->child[i].S=S;
		BoxSetState_recursive(b->child+i, S);
	}
	
}
void BoxSetState(box *b, boxstate S)
{
	if (b->S>S)
		b->S=S;
	BoxSetState_recursive(b, S);
}

