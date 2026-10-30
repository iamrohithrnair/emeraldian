libtexprintf & utftex
=====================
[![CI](https://github.com/bartp5/libtexprintf/workflows/CI/badge.svg)](https://github.com/bartp5/libtexprintf/actions?query=workflow%3ACI)

The utftex program and underlying libtexprintf library provide tools to
pretty print math in mono-space fonts, using a tex-like syntax. It 
produces UTF-8 encoded text. The program was inspired by 
[asciiTeX](http://asciitex.sourceforge.net/), and the improved 
[asciiTeX fork](https://github.com/larseggert/asciiTeX). However, 
utftex supports much more TeX syntax and contains extensive Unicode 
tables to map latex commands to Unicode symbols. Use 
libtexprintf/utftex to unlock the math capabilities of Unicode in 
mono-space text applications.

Note that how the equations look depends strongly on the font you use. 
Naturally, one needs a monospace font with good Unicode support for the 
symbols you use. A good monospace font for math is, for example,  
[JuliaMono](https://juliamono.netlify.app/).

libtexprintf
------------
The core library to pretty print math in mono-space font. Use it to 
enable your c or c++ code to print out formatted math. The calls are 
modeled after the printf family to produce formatted output.

utftex
------
The utftex program uses libtexprinf to implement a command-line utility 
to format math. For example:  

    > utftex '\frac{\alpha}{\beta+x}'  
gives
 
     α   
    ───  
    β+x  


The texprintf library provides a fairly full featured tex-like syntax
with arrays, brackets, many symbols, and even font style (note that 
texprintf outputs UTF-8 and as such cannot change the fonts. However,
Unicode does provide several font-styles like mathematical calligraphic
style, or italic). For example:

    > utftex -F mathnormal '\frac{1}{a+x}' 
gives  

     1
    ───
    𝑎+𝑥

It also supports some diacritical combining marks, to do things like

    utftex '\vec x' 
to get
 
    x⃗.

A bit more elaborate example is this small table of Laplace transforms:

    > utftex '\begin{array}{|l|l|}  
    \hline  
    f(t) & \mathscr{L}[f(t)]=F(s) \\\hline  
    1 & \frac{1}{s} \\\hline  
    e^{at}f(t) & F(s-a) \\\hline  
    \delta\left(t\right) & 1 \\\hline  
    \delta\left(t-t_0\right) & e^{-st_0} \\\hline  
    \int_0^t f(x)g(t-x)dx & F(s)G(s)\hline  
    \end{array}'  

    ┌────────────────┬──────────────┐ 
    │f(t)            │ 𝓛[f(t)]=F(s) │  
    ├────────────────┼──────────────┤  
    │                │ 1            │  
    │1               │ ─            │  
    │                │ s            │  
    ├────────────────┼──────────────┤  
    │ at             │              │  
    │e  f(t)         │ F(s-a)       │  
    ├────────────────┼──────────────┤  
    │δ(t)            │ 1            │  
    ├────────────────┼──────────────┤  
    │                │  -st         │  
    │                │     0        │  
    │δ⎛t-t ⎞         │ e            │  
    │ ⎝   0⎠         │              │  
    ├────────────────┼──────────────┤  
    │ t              │              │  
    │⌠               │              │  
    │⎮  f(x)g(t-x)dx │ F(s)G(s)     │  
    │⌡               │              │  
    │ 0              │              │  
    └────────────────┴──────────────┘  


Note that if the table borders do not properly align you are not viewing 
the table in monospace fonts. 

utfstringinfo
-------------
Utility to analyze UTF-8 strings. The utfstringinfo  program reads 
arguments or, if no arguments are provided, stdin and for every single 
character it finds it shows  the  UTF-8  byte sequence, the Unicode 
code-point and, if applicable, the utftex command that can be used to 
generate the symbol.  

Software using libtexprintf/utftex
----------------------------------
Several projects use libtexprintf or utftex. Some projects that I am aware of are (in arbitrary order):

* [LibTeXPrintf](https://github.com/JuliaStrings/LibTeXPrintf.jl): a 
wrapper around libtexprintf to use the library directly from the 
julia programming language
* [kramdown-rfc](https://github.com/cabo/kramdown-rfc): (a backend for
[kramdown](https://kramdown.gettalong.org/)) uses utftex to format 
latex equations
* [utftex-gui](https://github.com/wpeaton/utftex-gui/): a simple 
graphical user interface to utftex (much like the gui that came with 
asciiTeX)
* [utftexbot](https://github.com/LRDPRDX/utftexbot): a telegram bot to
pretty print LaTeX
* [render-markdown.nvim](https://github.com/MeanderingProgrammer/render-markdown.nvim): 
a markdown plugin for Neovim which can use utftex to format LaTeX 
equations
* [euporie](https://euporie.readthedocs.io/en/stable/index.html): a 
terminal based interactive computing environment for Jupyter.


Node.js binding to libtexprintf
-------------------------------

an npmjs package is available which links libtexprintf to Node.js. To install do

```sh
npm install libtexprintf
```

Usage:
---------------
After installation

```js
import { loadInstance, createRender } from "libtexprintf";

const instance = await loadInstance();
const render = createRender(instance);
console.log(render("\\frac{\\alpha}{\\beta+x}"));
```
Further details can be found in the npmjs directory of this repository.


Questions One Might Ask (QOMA)
------------------------------

* Why is my equation all misaligned?
	- Make sure you are using mono-space fonts.
	
* I am using mono-space fonts, why are my equations still poorly aligned?
	- Most fonts do not support all Unicode characters. Your program 
	   may use some variable-width fallback fonts.
	   
* Why are some symbols are not displayed (properly)?
	- Most fonts do not support all Unicode characters. Perhaps your 
	   font engine just gave up? This also often happens when using 
	   combining diacritical marks. Find the right set of font engine 
	   and fonts to use.
	   
* Can you recommend a good mono-space symbol font?
	- Best is if the monospace font you use supports lots of symbols by
	  itself. There are various fonts that support a great deal of 
	  Unicode. See for example 
	  [this discussion](https://stackoverflow.com/a/73313342/3662120).
	  As mentioned before I recommend 
	  [JuliaMono](https://juliamono.netlify.app/).
	- Make your own out of your favorite (variable-space) fonts! The 
	   great "monospacifier" will do that for you. Get it at 
	   https://github.com/cpitclaudel/monospacifier

* How do I specify the fallback fonts in <insert some program/system>?
	- This will depend on your system and/or program you are using. If 
	   your system uses fontconfig you may try that to set up an 
	   appropriate mono-space fallback font.

* How do I specify the fallback fonts in with fontconfig?
	- Let us assume you use some mono-space font called "MONO" and we 
	   have some symbol mono-space fallback font for it "SYMBOLMONO". 
	   Here are the steps:
	   1. Install all fonts (e.g. copy font files to 
	      ~/.local/share/fonts and run "fc-cache -f -v")
	   2. Then create a fontconfig file in 
	      ~/.config/fontconfig/fonts.conf
	      It could look something like (replace the font names with the 
	      appropriate values):
	      
```
<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "fonts.dtd">
<fontconfig>
 <alias>
   <family>monospace</family>
   <prefer>
     <family>MONO</family>
     <family>SYMBOLMONO</family>
   </prefer>
 </alias>
</fontconfig>	   
```	     
