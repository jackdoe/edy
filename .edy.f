\ ~/.edy.f — edy forth dictionary, loaded at startup
\ definitions only: nothing here runs until you invoke a word
\ run a word with M-x <name> RET, or put the cursor on it and press M-;

\ -- building blocks ----------------------------------------------------

: nl "
" ;                                          \ ( -- s )  newline string

: bol!  line@ bol cursor! ;                  \ ( -- )  go to line start
: eol!  line@ eol cursor! ;                  \ ( -- )  go to line end
: mid   len 2 / cursor! ;                    \ ( -- )  jump to buffer middle

\ -- lines --------------------------------------------------------------

\ duplicate the current line below itself
: dup-line  line@ >l  l bol l eol text@ >s  eol!  nl s s+ insert ;

\ delete the whole current line, newline included
: zap-line  line@ >l  l bol  l eol 1 +  del ;

\ -- selection ----------------------------------------------------------

\ wrap the selection in parens (with no selection: insert () at point)
: parens  sel@ >e >a  e cursor! ")" insert  a cursor! "(" insert ;

\ say how big the selection is
: sel-count  sel@ swap - >str " bytes selected" s+ msg ;

\ -- loops and locals ---------------------------------------------------

\ insert n dashes, e.g.: 40 dashes
: dashes  >n ""  begin n 0 > while  "-" s+  n 1 - >n  repeat  insert ;

\ horizontal rule on its own line
: hr  40 dashes nl insert ;

\ -- search -------------------------------------------------------------

\ jump past the next TODO; run again for the one after
: next-todo  "TODO" find >p  p 0 < if "no TODO" msg else p 4 + cursor! then ;

\ -- replace ------------------------------------------------------------

\ replace every a with b from point onward, e.g.: "cat" "owl" replace-all
\ (M-< first to do the whole buffer; one C-/ undoes the lot)
: replace-all  >b >a  0 >n
  a slen 0 > if
    begin  a find >p  p 0 >= while
      p cursor!  p  p a slen +  del  b insert
      n 1 + >n
    repeat
  then
  n >str " replaced" s+ msg ;

\ -- buffer info --------------------------------------------------------

: stats  len >str " bytes  " s+  lines >str s+  " lines" s+  msg ;
