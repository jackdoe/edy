# edy

A text editor for modern terminals. Zero dependencies, emacs keybindings, a
piece table underneath, and a small Forth on top.

Written entirely — code, specs, and this README — by Claude (Anthropic's
Claude Fable 5, via Claude Code), pair-designed with a human who made the
calls.

## What it is

- **Zero dependencies.** Only `std`. The kernel is reached through a
  hand-written `extern "C"` block (termios, ioctl, poll, read) — libc is the
  platform, not a dependency. ~3,800 lines of Rust including its 61 tests.
- **Emacs keybindings.** The classic set your fingers already know: movement,
  kill ring, mark and region, incremental search, query-replace, undo,
  multiple buffers. No windows, no splits — one buffer visible at a time.
- **Piece table.** The file you opened is never mutated in memory; edits are
  pieces over an immutable original plus an append-only add buffer. Undo is
  emacs-true: undos are themselves undoable, no separate redo machinery.
- **Modern tty only.** UTF-8 and xterm CSI sequences assumed, never queried.
  No terminfo. Alternate screen, synchronized output (`?2026`) for flickerless
  full-frame paints, reverse video for the modeline and the active region —
  and no other styling. No syntax highlighting, deliberately.
- **Forth, not lisp.** The extension language is a uxn-flavored partial
  Forth with named locals. No `here`, no `does>`, no memory model. The VM's
  entire universe is the current buffer: it has no I/O words at all.

## Build

```
cargo build --release
./target/release/edy file.txt
```

macOS and Linux. Refuses to start if stdin/stdout is not a terminal.

## Keys

| Keys | |
|---|---|
| `C-f` `C-b` `C-n` `C-p` / arrows | move by char/line |
| `M-f` `M-b` | move by word |
| `C-a` `C-e` `M-<` `M->` | line / buffer ends |
| `C-v` `M-v` `C-l` `M-g g` | page, recenter, goto line |
| `C-spc` `C-x C-x` | set mark, exchange (region shows in reverse video) |
| `C-w` `M-w` `C-k` `M-d` `M-DEL` | kill region / copy / kill line / kill word |
| `C-y` `M-y` | yank, yank-pop (kill ring, 16 entries) |
| `C-s` `C-r` | incremental search |
| `M-%` | query-replace (`y` `n` `!` `q`) |
| `C-/` or `C-_` | undo |
| `C-u` | numeric argument |
| `C-x C-f` `C-x C-s` `C-x C-w` | find file, save, write as (TAB completes paths) |
| `C-x b` `C-x k` `C-x C-b` | switch / kill / list buffers (TAB completes names) |
| `C-x C-c` | quit |
| `M-x` | Forth command line (TAB completes words) |
| `M-;` | eval the form at point |
| `C-g` | quit anything |

## Forth

`M-x` opens a command line; the echo area answers with the data stack
(`ok ( 3 "ab" )`) or the error. The stack and dictionary persist for the
session. `M-;` evaluates the form at point: inside a `: … ;` definition it
defines the word, otherwise the token under the cursor runs — any buffer is
a REPL.

The signature feature is **named locals**. `>x` pops into a local; a bare
`x` pushes it back. This replaces the `dup nip over rot` juggling entirely:

```forth
: abc >x >y 5 x + y * ;     \ 3 4 abc  leaves 27
```

Locals live in real frames (`enter`/`leave` ops, `local_get`/`local_set` by
index) and work at the top level too, scoped to the line.

Control flow: `if/else/then`, `begin/until`, `begin/while/repeat`.
Values are integers and `"strings"` (a literal may contain real newlines —
there are no escapes). `\` comments to end of line.

Vocabulary — core:

```
+ - * / mod negate   = <> < > <= >=   and or not
dup drop swap over .   >str >int slen s+
```

and editor (integers are byte offsets, lines 0-based):

```
len  cursor@ cursor!  mark@ mark!  sel@  text@  insert  del
line@ lines bol eol  find rfind  msg
```

So beginning-of-line is `line@ bol cursor!`, and duplicating the current
line — as the starter `.edy.f` defines it, building on its own `eol!` and
`nl` words — is:

```forth
: dup-line  line@ >l  l bol l eol text@ >s  eol!  nl s s+ insert ;
```

The VM is sandboxed by construction: no I/O words, every offset clamped and
snapped to char boundaries, a step budget (1M) that turns infinite loops
into errors, capped stacks. On error the data stack rolls back; edits a word
already made remain as a single undo group, so `C-/` reverts them.

### `~/.edy.f`

At startup edy reads one optional file, `~/.edy.f`, in **definitions-only
mode** — it can define words but nothing in it executes. A starter
dictionary ships in this repo (`.edy.f`): line duplication, selection
wrapping, a TODO jumper, replace-all, buffer stats. `cp .edy.f ~/` and
they're on `M-x`.

## Hardened I/O

Saves are atomic: write to an `O_EXCL` temp sibling, fsync, match the
original's permissions (0600 for new files), rename over, fsync the
directory. Paths canonicalize at open so saving through a symlink never
replaces the link. No backup files, no autosave, no lockfiles, no swap —
the only bytes edy writes to disk are explicit saves, and the only file it
ever reads unasked is `~/.edy.f`, which cannot run anything. It never
shells out and spawns nothing. Stated trade-off: no crash recovery.

## Architecture

```
main.rs    event loop                              sys.rs   ALL unsafe: termios,
ui.rs      frame compose, wcwidth, $-truncation             ioctl, poll, read FFI
editor.rs  buffers, commands, modes, undo, forth glue
forth.rs   the VM — lexer, compiler, interpreter   term.rs  RawMode RAII guard,
text.rs    piece table, pure, no IO                         Key decoder
width.rs   display-column math                     file.rs  atomic save, completion
```

Dependencies point one way. `text`, `editor`, and `forth` never touch the
terminal, so the whole command set and the VM test headless — bytes in,
state out. `unsafe` exists only in `sys.rs`. There are no comments in the
code; names and structure carry the meaning. Design documents live in
`docs/superpowers/specs/`.

## What it refuses to do

Wrap long lines (truncates with `$` and horizontal scroll), open non-UTF-8
files, recover from crashes, split windows, highlight syntax, read
configuration beyond `~/.edy.f`, or execute anything, ever.
