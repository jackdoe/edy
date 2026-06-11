use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Int(i64),
    Str(Rc<str>),
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{}", n),
            Value::Str(s) => write!(f, "{}", s),
        }
    }
}

#[derive(Clone)]
enum Op {
    Lit(Value),
    Call(usize),
    Prim(usize),
    Enter(usize),
    Leave,
    LocalGet(usize),
    LocalSet(usize),
    Branch(usize),
    BranchIf0(usize),
    Ret,
}

struct Word {
    name: String,
    ops: Vec<Op>,
}

pub const PRIMS: &[&str] = &[
    "+", "-", "*", "/", "mod", "negate", "=", "<>", "<", ">", "<=", ">=", "and", "or", "not",
    "dup", "drop", "swap", "over", ".", ">str", ">int", "slen", "s+", "len", "cursor@",
    "cursor!", "mark@", "mark!", "sel@", "text@", "insert", "del", "line@", "lines", "bol",
    "eol", "find", "rfind", "msg",
];

pub trait Host {
    fn len(&self) -> i64;
    fn cursor(&self) -> i64;
    fn set_cursor(&mut self, at: i64);
    fn mark(&self) -> i64;
    fn set_mark(&mut self, at: i64);
    fn selection(&self) -> (i64, i64);
    fn slice(&self, a: i64, b: i64) -> String;
    fn insert(&mut self, s: &str);
    fn delete(&mut self, a: i64, b: i64);
    fn line(&self) -> i64;
    fn lines(&self) -> i64;
    fn line_start(&self, l: i64) -> i64;
    fn line_end(&self, l: i64) -> i64;
    fn search(&self, s: &str, fwd: bool) -> i64;
    fn message(&mut self, s: &str);
}

pub fn lex(src: &str) -> Result<Vec<(usize, String)>, String> {
    let mut out = Vec::new();
    let mut it = src.char_indices().peekable();
    while let Some(&(i, c)) = it.peek() {
        if c.is_whitespace() {
            it.next();
            continue;
        }
        if c == '\\' {
            for (_, c2) in it.by_ref() {
                if c2 == '\n' {
                    break;
                }
            }
            continue;
        }
        if c == '"' {
            it.next();
            loop {
                match it.next() {
                    Some((j, '"')) => {
                        out.push((i, src[i..=j].to_string()));
                        break;
                    }
                    Some(_) => {}
                    None => return Err("unterminated string".into()),
                }
            }
            continue;
        }
        let mut end = src.len();
        while let Some(&(j, c2)) = it.peek() {
            if c2.is_whitespace() {
                end = j;
                break;
            }
            it.next();
            end = j + c2.len_utf8();
        }
        out.push((i, src[i..end].to_string()));
    }
    Ok(out)
}

#[derive(Default)]
pub struct Vm {
    words: Vec<Word>,
    stack: Vec<Value>,
}

const KIND_IF: u8 = 0;
const KIND_BEGIN: u8 = 1;
const KIND_WHILE: u8 = 2;

impl Vm {
    pub fn new() -> Vm {
        Vm::default()
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.words.iter().map(|w| w.name.as_str())
    }

    pub fn show(&self) -> String {
        let start = self.stack.len().saturating_sub(8);
        let mut s = String::from("ok (");
        if start > 0 {
            s.push_str(" …");
        }
        for v in &self.stack[start..] {
            s.push(' ');
            match v {
                Value::Int(n) => s.push_str(&n.to_string()),
                Value::Str(t) => {
                    let short: String = t.chars().take(16).collect();
                    let ell = if t.chars().count() > 16 { "…" } else { "" };
                    s.push_str(&format!("\"{}{}\"", short, ell));
                }
            }
        }
        s.push_str(" )");
        s
    }

    pub fn run(&mut self, src: &str, host: &mut dyn Host) -> Result<(), String> {
        let toks = lex(src)?;
        let saved = self.stack.clone();
        let r = self.eval(&toks, Some(host));
        if r.is_err() {
            self.stack = saved;
        }
        r
    }

    pub fn load(&mut self, src: &str) -> Result<(), String> {
        let toks = lex(src)?;
        self.eval(&toks, None)
    }

    fn eval(&mut self, toks: &[(usize, String)], mut host: Option<&mut dyn Host>) -> Result<(), String> {
        let mut imm: Vec<(usize, String)> = Vec::new();
        let mut i = 0;
        while i < toks.len() {
            if toks[i].1 == ":" {
                self.flush(&mut imm, &mut host)?;
                i += 1;
                let name = match toks.get(i) {
                    Some(t) if t.1 != ";" && t.1 != ":" => t.1.clone(),
                    _ => return Err(": missing name".into()),
                };
                i += 1;
                let ops = self.compile(toks, &mut i, true)?;
                self.words.push(Word { name, ops });
            } else {
                imm.push(toks[i].clone());
                i += 1;
            }
        }
        self.flush(&mut imm, &mut host)
    }

    fn flush(&mut self, imm: &mut Vec<(usize, String)>, host: &mut Option<&mut dyn Host>) -> Result<(), String> {
        if imm.is_empty() {
            return Ok(());
        }
        let h = host.as_deref_mut().ok_or_else(|| format!("only definitions allowed here: {}", imm[0].1))?;
        let mut i = 0;
        let toks = std::mem::take(imm);
        let ops = self.compile(&toks, &mut i, false)?;
        self.exec(ops, h)
    }

    fn compile(&self, toks: &[(usize, String)], i: &mut usize, until_semi: bool) -> Result<Vec<Op>, String> {
        let mut ops = vec![Op::Enter(0)];
        let mut locals: Vec<String> = Vec::new();
        let mut ctrl: Vec<(u8, usize)> = Vec::new();
        loop {
            if *i >= toks.len() {
                if until_semi {
                    return Err("missing ;".into());
                }
                break;
            }
            let t = toks[*i].1.clone();
            *i += 1;
            match t.as_str() {
                ";" if until_semi => break,
                ";" => return Err("; outside definition".into()),
                ":" => return Err(": inside definition".into()),
                "if" => {
                    ctrl.push((KIND_IF, ops.len()));
                    ops.push(Op::BranchIf0(0));
                }
                "else" => {
                    match ctrl.pop() {
                        Some((KIND_IF, p)) => {
                            ctrl.push((KIND_IF, ops.len()));
                            ops.push(Op::Branch(0));
                            let here = ops.len();
                            patch(&mut ops, p, here);
                        }
                        _ => return Err("else without if".into()),
                    }
                }
                "then" => match ctrl.pop() {
                    Some((KIND_IF, p)) => {
                        let here = ops.len();
                        patch(&mut ops, p, here);
                    }
                    _ => return Err("then without if".into()),
                },
                "begin" => ctrl.push((KIND_BEGIN, ops.len())),
                "until" => match ctrl.pop() {
                    Some((KIND_BEGIN, p)) => ops.push(Op::BranchIf0(p)),
                    _ => return Err("until without begin".into()),
                },
                "while" => match ctrl.last() {
                    Some((KIND_BEGIN, _)) => {
                        ctrl.push((KIND_WHILE, ops.len()));
                        ops.push(Op::BranchIf0(0));
                    }
                    _ => return Err("while without begin".into()),
                },
                "repeat" => match (ctrl.pop(), ctrl.pop()) {
                    (Some((KIND_WHILE, p)), Some((KIND_BEGIN, b))) => {
                        ops.push(Op::Branch(b));
                        let here = ops.len();
                        patch(&mut ops, p, here);
                    }
                    _ => return Err("repeat without begin while".into()),
                },
                _ if t.starts_with('"') => {
                    ops.push(Op::Lit(Value::Str(t[1..t.len() - 1].into())));
                }
                _ if t.len() > 1 && t.starts_with('>') && !PRIMS.contains(&t.as_str()) => {
                    let name = &t[1..];
                    let idx = match locals.iter().position(|l| l == name) {
                        Some(idx) => idx,
                        None => {
                            locals.push(name.to_string());
                            locals.len() - 1
                        }
                    };
                    ops.push(Op::LocalSet(idx));
                }
                _ => {
                    if let Some(li) = locals.iter().position(|l| *l == t) {
                        ops.push(Op::LocalGet(li));
                    } else if let Some(wi) = self.words.iter().rposition(|w| w.name == t) {
                        ops.push(Op::Call(wi));
                    } else if let Some(pi) = PRIMS.iter().position(|p| *p == t) {
                        ops.push(Op::Prim(pi));
                    } else if let Ok(n) = t.parse::<i64>() {
                        ops.push(Op::Lit(Value::Int(n)));
                    } else {
                        return Err(format!("unknown word: {}", t));
                    }
                }
            }
        }
        if !ctrl.is_empty() {
            return Err("unbalanced control flow".into());
        }
        ops[0] = Op::Enter(locals.len());
        ops.push(Op::Leave);
        ops.push(Op::Ret);
        Ok(ops)
    }

    fn exec(&mut self, entry: Vec<Op>, host: &mut dyn Host) -> Result<(), String> {
        let mut locals: Vec<Value> = Vec::new();
        let mut bases: Vec<usize> = Vec::new();
        let mut rstack: Vec<(isize, usize)> = Vec::new();
        let mut wi: isize = -1;
        let mut ip = 0usize;
        let mut steps = 0u32;
        loop {
            steps += 1;
            if steps > 1_000_000 {
                return Err("step budget exceeded".into());
            }
            let ops = if wi < 0 { &entry } else { &self.words[wi as usize].ops };
            let op = ops[ip].clone();
            ip += 1;
            match op {
                Op::Lit(v) => self.push(v)?,
                Op::Prim(p) => self.prim(p, host)?,
                Op::Call(w) => {
                    if rstack.len() >= 256 {
                        return Err("call depth exceeded".into());
                    }
                    rstack.push((wi, ip));
                    wi = w as isize;
                    ip = 0;
                }
                Op::Enter(n) => {
                    if locals.len() + n > 4096 {
                        return Err("locals overflow".into());
                    }
                    bases.push(locals.len());
                    locals.resize(locals.len() + n, Value::Int(0));
                }
                Op::Leave => {
                    let b = bases.pop().ok_or("frame underflow")?;
                    locals.truncate(b);
                }
                Op::LocalGet(idx) => {
                    let b = *bases.last().ok_or("no frame")?;
                    let v = locals.get(b + idx).cloned().ok_or("bad local")?;
                    self.push(v)?;
                }
                Op::LocalSet(idx) => {
                    let b = *bases.last().ok_or("no frame")?;
                    let v = self.pop()?;
                    *locals.get_mut(b + idx).ok_or("bad local")? = v;
                }
                Op::Branch(t) => ip = t,
                Op::BranchIf0(t) => {
                    if self.pop_int()? == 0 {
                        ip = t;
                    }
                }
                Op::Ret => match rstack.pop() {
                    Some((w, p)) => {
                        wi = w;
                        ip = p;
                    }
                    None => return Ok(()),
                },
            }
        }
    }

    fn push(&mut self, v: Value) -> Result<(), String> {
        if self.stack.len() >= 1024 {
            return Err("stack overflow".into());
        }
        self.stack.push(v);
        Ok(())
    }

    fn pop(&mut self) -> Result<Value, String> {
        self.stack.pop().ok_or_else(|| "stack underflow".into())
    }

    fn pop_int(&mut self) -> Result<i64, String> {
        match self.pop()? {
            Value::Int(n) => Ok(n),
            Value::Str(s) => Err(format!("expected number, got \"{}\"", s)),
        }
    }

    fn pop_str(&mut self) -> Result<Rc<str>, String> {
        match self.pop()? {
            Value::Str(s) => Ok(s),
            Value::Int(n) => Err(format!("expected string, got {}", n)),
        }
    }

    fn pop2(&mut self) -> Result<(i64, i64), String> {
        let b = self.pop_int()?;
        let a = self.pop_int()?;
        Ok((a, b))
    }

    fn prim(&mut self, p: usize, host: &mut dyn Host) -> Result<(), String> {
        match PRIMS[p] {
            "+" => {
                let (a, b) = self.pop2()?;
                self.push(Value::Int(a.wrapping_add(b)))
            }
            "-" => {
                let (a, b) = self.pop2()?;
                self.push(Value::Int(a.wrapping_sub(b)))
            }
            "*" => {
                let (a, b) = self.pop2()?;
                self.push(Value::Int(a.wrapping_mul(b)))
            }
            "/" => {
                let (a, b) = self.pop2()?;
                if b == 0 {
                    return Err("division by zero".into());
                }
                self.push(Value::Int(a.wrapping_div(b)))
            }
            "mod" => {
                let (a, b) = self.pop2()?;
                if b == 0 {
                    return Err("division by zero".into());
                }
                self.push(Value::Int(a.wrapping_rem(b)))
            }
            "negate" => {
                let a = self.pop_int()?;
                self.push(Value::Int(a.wrapping_neg()))
            }
            "=" => {
                let b = self.pop()?;
                let a = self.pop()?;
                self.push(Value::Int((a == b) as i64))
            }
            "<>" => {
                let b = self.pop()?;
                let a = self.pop()?;
                self.push(Value::Int((a != b) as i64))
            }
            "<" => {
                let (a, b) = self.pop2()?;
                self.push(Value::Int((a < b) as i64))
            }
            ">" => {
                let (a, b) = self.pop2()?;
                self.push(Value::Int((a > b) as i64))
            }
            "<=" => {
                let (a, b) = self.pop2()?;
                self.push(Value::Int((a <= b) as i64))
            }
            ">=" => {
                let (a, b) = self.pop2()?;
                self.push(Value::Int((a >= b) as i64))
            }
            "and" => {
                let (a, b) = self.pop2()?;
                self.push(Value::Int((a != 0 && b != 0) as i64))
            }
            "or" => {
                let (a, b) = self.pop2()?;
                self.push(Value::Int((a != 0 || b != 0) as i64))
            }
            "not" => {
                let a = self.pop_int()?;
                self.push(Value::Int((a == 0) as i64))
            }
            "dup" => {
                let v = self.stack.last().cloned().ok_or("stack underflow")?;
                self.push(v)
            }
            "drop" => self.pop().map(|_| ()),
            "swap" => {
                let b = self.pop()?;
                let a = self.pop()?;
                self.push(b)?;
                self.push(a)
            }
            "over" => {
                let n = self.stack.len();
                let v = self.stack.get(n.wrapping_sub(2)).cloned().ok_or("stack underflow")?;
                self.push(v)
            }
            "." => {
                let v = self.pop()?;
                host.message(&v.to_string());
                Ok(())
            }
            ">str" => {
                let v = self.pop()?;
                self.push(Value::Str(v.to_string().into()))
            }
            ">int" => {
                let s = self.pop_str()?;
                let n = s.trim().parse::<i64>().map_err(|_| format!("not a number: \"{}\"", s))?;
                self.push(Value::Int(n))
            }
            "slen" => {
                let s = self.pop_str()?;
                self.push(Value::Int(s.len() as i64))
            }
            "s+" => {
                let b = self.pop_str()?;
                let a = self.pop_str()?;
                self.push(Value::Str(format!("{}{}", a, b).into()))
            }
            "len" => self.push(Value::Int(host.len())),
            "cursor@" => self.push(Value::Int(host.cursor())),
            "cursor!" => {
                let n = self.pop_int()?;
                host.set_cursor(n);
                Ok(())
            }
            "mark@" => self.push(Value::Int(host.mark())),
            "mark!" => {
                let n = self.pop_int()?;
                host.set_mark(n);
                Ok(())
            }
            "sel@" => {
                let (a, b) = host.selection();
                self.push(Value::Int(a))?;
                self.push(Value::Int(b))
            }
            "text@" => {
                let (a, b) = self.pop2()?;
                self.push(Value::Str(host.slice(a, b).into()))
            }
            "insert" => {
                let s = self.pop_str()?;
                host.insert(&s);
                Ok(())
            }
            "del" => {
                let (a, b) = self.pop2()?;
                host.delete(a, b);
                Ok(())
            }
            "line@" => self.push(Value::Int(host.line())),
            "lines" => self.push(Value::Int(host.lines())),
            "bol" => {
                let l = self.pop_int()?;
                self.push(Value::Int(host.line_start(l)))
            }
            "eol" => {
                let l = self.pop_int()?;
                self.push(Value::Int(host.line_end(l)))
            }
            "find" => {
                let s = self.pop_str()?;
                self.push(Value::Int(host.search(&s, true)))
            }
            "rfind" => {
                let s = self.pop_str()?;
                self.push(Value::Int(host.search(&s, false)))
            }
            "msg" => {
                let s = self.pop_str()?;
                host.message(&s);
                Ok(())
            }
            _ => unreachable!(),
        }
    }
}

fn patch(ops: &mut [Op], at: usize, target: usize) {
    match &mut ops[at] {
        Op::Branch(t) | Op::BranchIf0(t) => *t = target,
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeHost {
        text: String,
        cursor: usize,
        mark: Option<usize>,
        msg: String,
    }

    impl FakeHost {
        fn new(text: &str) -> FakeHost {
            FakeHost { text: text.into(), cursor: 0, mark: None, msg: String::new() }
        }

        fn at(&self, n: i64) -> usize {
            let mut i = n.clamp(0, self.text.len() as i64) as usize;
            while !self.text.is_char_boundary(i) {
                i -= 1;
            }
            i
        }

        fn range(&self, a: i64, b: i64) -> (usize, usize) {
            let (a, b) = (self.at(a), self.at(b));
            (a.min(b), a.max(b))
        }
    }

    impl Host for FakeHost {
        fn len(&self) -> i64 {
            self.text.len() as i64
        }

        fn cursor(&self) -> i64 {
            self.cursor as i64
        }

        fn set_cursor(&mut self, at: i64) {
            self.cursor = self.at(at);
        }

        fn mark(&self) -> i64 {
            self.mark.map_or(-1, |m| m as i64)
        }

        fn set_mark(&mut self, at: i64) {
            self.mark = if at < 0 { None } else { Some(self.at(at)) };
        }

        fn selection(&self) -> (i64, i64) {
            match self.mark {
                Some(m) => (m.min(self.cursor) as i64, m.max(self.cursor) as i64),
                None => (self.cursor as i64, self.cursor as i64),
            }
        }

        fn slice(&self, a: i64, b: i64) -> String {
            let (a, b) = self.range(a, b);
            self.text[a..b].into()
        }

        fn insert(&mut self, s: &str) {
            self.text.insert_str(self.cursor, s);
            self.cursor += s.len();
        }

        fn delete(&mut self, a: i64, b: i64) {
            let (a, b) = self.range(a, b);
            self.text.replace_range(a..b, "");
            self.cursor = self.at(self.cursor.min(self.text.len()) as i64);
        }

        fn line(&self) -> i64 {
            self.text[..self.cursor].matches('\n').count() as i64
        }

        fn lines(&self) -> i64 {
            self.text.matches('\n').count() as i64 + 1
        }

        fn line_start(&self, l: i64) -> i64 {
            let mut off = 0;
            for _ in 0..l.max(0) {
                match self.text[off..].find('\n') {
                    Some(i) => off += i + 1,
                    None => return self.text.len() as i64,
                }
            }
            off as i64
        }

        fn line_end(&self, l: i64) -> i64 {
            let s = self.line_start(l) as usize;
            self.text[s..].find('\n').map_or(self.text.len(), |i| s + i) as i64
        }

        fn search(&self, s: &str, fwd: bool) -> i64 {
            if fwd {
                self.text[self.cursor..].find(s).map_or(-1, |i| (self.cursor + i) as i64)
            } else {
                self.text[..self.cursor].rfind(s).map_or(-1, |i| i as i64)
            }
        }

        fn message(&mut self, s: &str) {
            self.msg = s.into();
        }
    }

    fn run(vm: &mut Vm, src: &str) -> Result<FakeHost, String> {
        let mut h = FakeHost::new("");
        vm.run(src, &mut h)?;
        Ok(h)
    }

    fn ints(vm: &Vm) -> Vec<i64> {
        vm.stack
            .iter()
            .map(|v| match v {
                Value::Int(n) => *n,
                Value::Str(_) => panic!("string on stack"),
            })
            .collect()
    }

    #[test]
    fn locals() {
        let mut vm = Vm::new();
        run(&mut vm, ": abc >x >y 5 x + y * ; 3 4 abc").unwrap();
        assert_eq!(ints(&vm), vec![27]);
    }

    #[test]
    fn locals_at_top_level() {
        let mut vm = Vm::new();
        run(&mut vm, "10 >a a a +").unwrap();
        assert_eq!(ints(&vm), vec![20]);
    }

    #[test]
    fn if_else_then() {
        let mut vm = Vm::new();
        run(&mut vm, ": sign 0 < if -1 else 1 then ; 5 sign -3 sign").unwrap();
        assert_eq!(ints(&vm), vec![1, -1]);
    }

    #[test]
    fn begin_until() {
        let mut vm = Vm::new();
        run(&mut vm, ": count >n 0 begin 1 + dup n = until ; 5 count").unwrap();
        assert_eq!(ints(&vm), vec![5]);
    }

    #[test]
    fn begin_while_repeat() {
        let mut vm = Vm::new();
        run(&mut vm, ": w >n 0 begin dup n < while 1 + repeat ; 3 w").unwrap();
        assert_eq!(ints(&vm), vec![3]);
    }

    #[test]
    fn strings() {
        let mut vm = Vm::new();
        run(&mut vm, "\"ab\" \"cd\" s+ slen").unwrap();
        assert_eq!(ints(&vm), vec![4]);
    }

    #[test]
    fn shadowing() {
        let mut vm = Vm::new();
        run(&mut vm, ": f 1 ; : f 2 ; f").unwrap();
        assert_eq!(ints(&vm), vec![2]);
    }

    #[test]
    fn step_budget() {
        let mut vm = Vm::new();
        let e = run(&mut vm, ": spin begin 0 until ; spin").err().unwrap();
        assert!(e.contains("budget"));
    }

    #[test]
    fn errors_restore_stack() {
        let mut vm = Vm::new();
        run(&mut vm, "1 2").unwrap();
        assert!(run(&mut vm, "3 nosuch").is_err());
        assert_eq!(ints(&vm), vec![1, 2]);
        assert!(run(&mut vm, "drop drop drop").is_err());
        assert_eq!(ints(&vm), vec![1, 2]);
    }

    #[test]
    fn compile_errors() {
        let mut vm = Vm::new();
        assert!(run(&mut vm, ": f if 1 ;").is_err());
        assert!(run(&mut vm, "else").is_err());
        assert!(run(&mut vm, ": f 1").is_err());
        assert!(run(&mut vm, "\"open").is_err());
    }

    #[test]
    fn defs_only_load() {
        let mut vm = Vm::new();
        vm.load(": a 1 ; \\ comment\n: b a a + ;").unwrap();
        assert!(vm.load("1 2 +").is_err());
        let mut h = FakeHost::new("");
        vm.run("b", &mut h).unwrap();
        assert_eq!(ints(&vm), vec![2]);
    }

    #[test]
    fn editor_words() {
        let mut vm = Vm::new();
        let mut h = FakeHost::new("hello\nworld");
        vm.run("len", &mut h).unwrap();
        assert_eq!(ints(&vm), vec![11]);
        vm.run("drop 0 eol cursor! \" there\" insert", &mut h).unwrap();
        assert_eq!(h.text, "hello there\nworld");
        vm.run("\"world\" find", &mut h).unwrap();
        assert_eq!(ints(&vm), vec![12]);
        vm.run("drop 0 5 del", &mut h).unwrap();
        assert_eq!(h.text, " there\nworld");
        vm.run("\"done\" msg", &mut h).unwrap();
        assert_eq!(h.msg, "done");
    }

    #[test]
    fn dot_and_conversions() {
        let mut vm = Vm::new();
        let mut h = FakeHost::new("");
        vm.run("6 7 * .", &mut h).unwrap();
        assert_eq!(h.msg, "42");
        vm.run("\"12\" >int 1 + >str", &mut h).unwrap();
        assert_eq!(vm.stack, vec![Value::Str("13".into())]);
    }
}
