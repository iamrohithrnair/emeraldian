

typedef enum boxtype {
	B_UNIT,		/* Single boxes: No children, only contents */
	B_ARRAY,	/* Aligned multi box, contents specifies alignment  */
	B_POS,		/* Positioned multi box, contents specifies x and y coordinates of the boxes  */
	B_DUMMY,	/* empty box with fixed size  */
	B_LINE,		/* Box which *tries* to fit a certain line length and may choose to put a line break breaks between its children */
	B_ENDLINE,	/* zero-sized empty box indicating a line break in a line box  */
} boxtype;

typedef enum boxstate {INIT, SIZEKNOWN,RELPOSKNOWN,ABSPOSKNOWN} boxstate;
typedef enum boxalign {MIN, CENTER, MAX, FIX} boxalign;
/* states:
	INIT: 		freshly alloctaed box
	SIZEKNOWN:	box w,h,xc,yc are computed
	RELPOSKNOWN	rx and ry are set
	ABSPOSKNOWN	ax and ay are set
*/
typedef struct box {
	struct box *parent;
	struct box *child;
	int Nc;
	boxstate S;	/* indicates the state of the box */
	boxtype T;
	void *content;
	boxalign X;
	boxalign Y;	
	int rx,ry;	/* position w.r.t the parent */
	int ax,ay;	/* absolute position */
	int w,h;	/* width and height of the box */
	int xc,yc;	/* center of alignment w.r.t the current box */
} box;

box InitBox(box * parent, boxtype T, void *content); 		/* initializes a box */
void AddChild(box * parent, boxtype T, void *content);
void FreeBox(box *b); 			/* frees a box and its children */
int BoxInBox(box *b, boxtype T, void *content);
boxstate StateBoxtree(box * b, box **minstate);	/* recursively determines the minimal state of the box tree */
box *FindBoxAtPos(box *b, int x, int y);

int BoxSize_children(box *b);
int BoxSize(box *b);
void BoxPos(box *b);
void BoxSetState(box *b, boxstate S);
