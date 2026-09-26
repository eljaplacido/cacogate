//! A total, best-effort POSIX-shell lexer for gating purposes.
//!
//! It never fails: any input yields a [`Parsed`] with a [`ParseStatus`]. It
//! splits a command line into simple commands (on `;` `&&` `||` `|` `&`,
//! newlines and subshell parens), keeps quoting semantics, records output
//! redirections, skips heredoc bodies, and recursively parses command
//! substitutions and `bash -c` strings so their inner commands are gated too.
//! Matching happens on the resulting argv tokens, never on the raw string.

/// How completely the command line was understood.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ParseStatus {
    /// Every simple command was recovered.
    Complete,
    /// Understood, but contains constructs whose effect cannot be known
    /// statically (`eval`, unresolved write targets, recursion limit).
    Partial,
    /// Could not be tokenised (unbalanced quotes or substitutions).
    Unparseable,
}

/// One simple command: argv plus its output redirections.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SimpleCommand {
    /// Words after quote removal; leading `VAR=value` assignments removed.
    pub argv: Vec<String>,
    /// Targets of `>`, `>>`, `>|`, `&>`, `2>` (file descriptors dropped).
    pub writes: Vec<String>,
    /// Sources of `<` redirections.
    pub reads: Vec<String>,
    /// argv of the command piping into this one, if any.
    pub piped_from: Option<Vec<String>>,
    /// Nesting depth (0 = top level, >0 = inside substitution / `-c`).
    pub depth: u8,
}

/// Parse result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    /// All simple commands, including nested ones, in source order.
    pub commands: Vec<SimpleCommand>,
    /// Worst status encountered.
    pub status: ParseStatus,
    /// Reasons for a non-complete status.
    pub notes: Vec<String>,
}

const MAX_DEPTH: u8 = 4;

const KEYWORDS: [&str; 13] = [
    "{", "}", "!", "if", "then", "else", "elif", "fi", "do", "done", "while", "until", "time",
];

/// Parse a shell command line.
pub fn parse(src: &str) -> Parsed {
    let mut out = Parsed {
        commands: Vec::new(),
        status: ParseStatus::Complete,
        notes: Vec::new(),
    };
    parse_into(src, 0, &mut out);
    out
}

fn worsen(out: &mut Parsed, status: ParseStatus, note: &str) {
    let rank = |s: ParseStatus| match s {
        ParseStatus::Complete => 0,
        ParseStatus::Partial => 1,
        ParseStatus::Unparseable => 2,
    };
    if rank(status) > rank(out.status) {
        out.status = status;
    }
    if !out.notes.iter().any(|n| n == note) {
        out.notes.push(note.to_owned());
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Word(String),
    /// Separator; `true` when it is a pipe.
    Sep(bool),
    /// Redirection operator; `true` for output, `false` for input.
    Redir(bool),
    Heredoc,
}

fn parse_into(src: &str, depth: u8, out: &mut Parsed) {
    if depth > MAX_DEPTH {
        worsen(out, ParseStatus::Partial, "nesting deeper than 4 levels");
        return;
    }
    let mut nested: Vec<String> = Vec::new();
    let toks = match lex(src, &mut nested) {
        Ok(t) => t,
        Err(reason) => {
            worsen(out, ParseStatus::Unparseable, reason);
            return;
        }
    };
    let mut cur = SimpleCommand {
        depth,
        ..SimpleCommand::default()
    };
    let mut pending_redir: Option<bool> = None;
    let mut pending_heredoc = false;
    let mut prev_pipe_argv: Option<Vec<String>> = None;
    let flush =
        |cur: &mut SimpleCommand, out: &mut Parsed, pipe: bool, prev: &mut Option<Vec<String>>| {
            let taken = std::mem::replace(
                cur,
                SimpleCommand {
                    depth,
                    ..SimpleCommand::default()
                },
            );
            let argv_for_next = taken.argv.clone();
            if !taken.argv.is_empty() || !taken.writes.is_empty() {
                let mut cmd = taken;
                cmd.piped_from = prev.take();
                post_process(cmd, depth, out);
            } else {
                *prev = None;
            }
            if pipe && !argv_for_next.is_empty() {
                *prev = Some(argv_for_next);
            }
        };
    for tok in toks {
        match tok {
            Tok::Word(w) => {
                if pending_heredoc {
                    pending_heredoc = false; // delimiter word; body already skipped by lexer
                } else if let Some(is_out) = pending_redir.take() {
                    if is_out {
                        cur.writes.push(w);
                    } else {
                        cur.reads.push(w);
                    }
                } else if cur.argv.is_empty() && is_assignment(&w) {
                    // leading VAR=value: environment, not a command word
                } else {
                    cur.argv.push(w);
                }
            }
            Tok::Redir(is_out) => pending_redir = Some(is_out),
            Tok::Heredoc => pending_heredoc = true,
            Tok::Sep(pipe) => {
                pending_redir = None;
                pending_heredoc = false;
                flush(&mut cur, out, pipe, &mut prev_pipe_argv);
            }
        }
    }
    if pending_redir.is_some() {
        worsen(
            out,
            ParseStatus::Unparseable,
            "redirection without a target",
        );
    }
    flush(&mut cur, out, false, &mut prev_pipe_argv);
    for inner in nested {
        parse_into(&inner, depth + 1, out);
    }
}

/// Expand wrappers (`sudo`, `env`, `bash -c`, …) into their inner commands.
fn post_process(mut cmd: SimpleCommand, depth: u8, out: &mut Parsed) {
    // Shell keywords and grouping braces are not programs: `{ rm x; }`,
    // `if true; then rm x; fi`, `! cmd`, `time cmd`.
    while let Some(first) = cmd.argv.first() {
        if KEYWORDS.contains(&first.as_str()) {
            cmd.argv.remove(0);
        } else {
            break;
        }
    }
    if cmd.argv.is_empty() && cmd.writes.is_empty() {
        return;
    }
    let program = cmd
        .argv
        .first()
        .map(|p| basename(p).to_owned())
        .unwrap_or_default();
    match program.as_str() {
        "bash" | "sh" | "zsh" | "dash" | "ksh" => {
            if let Some(pos) = cmd
                .argv
                .iter()
                .position(|a| a == "-c" || a == "-lc" || a == "-ic")
            {
                let script = cmd.argv.get(pos + 1).cloned().unwrap_or_default();
                out.commands.push(cmd);
                parse_into(&script, depth + 1, out);
                return;
            }
        }
        "eval" => {
            worsen(
                out,
                ParseStatus::Partial,
                "eval: effect depends on runtime values",
            );
            let script = cmd.argv.get(1..).map(|s| s.join(" ")).unwrap_or_default();
            out.commands.push(cmd);
            parse_into(&script, depth + 1, out);
            return;
        }
        _ => {}
    }
    out.commands.push(cmd);
}

fn is_assignment(w: &str) -> bool {
    match w.split_once('=') {
        Some((name, _)) => {
            !name.is_empty()
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && !name.starts_with(|c: char| c.is_ascii_digit())
        }
        None => false,
    }
}

/// Last path component of a program name.
pub fn basename(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

fn lex(src: &str, nested: &mut Vec<String>) -> Result<Vec<Tok>, &'static str> {
    let chars: Vec<char> = src.chars().collect();
    let mut toks = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut heredocs: Vec<(String, bool)> = Vec::new();
    let mut i = 0usize;
    let at = |k: usize| chars.get(k).copied();
    macro_rules! end_word {
        () => {
            if in_word {
                toks.push(Tok::Word(std::mem::take(&mut word)));
                in_word = false;
            }
        };
    }
    while let Some(c) = at(i) {
        match c {
            '\'' => {
                in_word = true;
                i += 1;
                loop {
                    match at(i) {
                        Some('\'') => break,
                        Some(ch) => word.push(ch),
                        None => return Err("unterminated single quote"),
                    }
                    i += 1;
                }
                i += 1;
            }
            '"' => {
                in_word = true;
                i += 1;
                loop {
                    match at(i) {
                        Some('"') => break,
                        Some('\\') => {
                            if let Some(n) = at(i + 1) {
                                if matches!(n, '"' | '\\' | '$' | '`' | '\n') {
                                    if n != '\n' {
                                        word.push(n);
                                    }
                                } else {
                                    word.push('\\');
                                    word.push(n);
                                }
                                i += 1;
                            } else {
                                return Err("dangling backslash");
                            }
                        }
                        Some('$') if at(i + 1) == Some('(') => {
                            let (inner, next) = take_balanced(&chars, i + 2)?;
                            word.push_str("$(…)");
                            nested.push(inner);
                            i = next;
                            continue;
                        }
                        Some('`') => {
                            let (inner, next) = take_backtick(&chars, i + 1)?;
                            word.push_str("`…`");
                            nested.push(inner);
                            i = next;
                            continue;
                        }
                        Some(ch) => word.push(ch),
                        None => return Err("unterminated double quote"),
                    }
                    i += 1;
                }
                i += 1;
            }
            '\\' => {
                in_word = true;
                match at(i + 1) {
                    Some('\n') => {}
                    Some(n) => word.push(n),
                    None => return Err("dangling backslash"),
                }
                i += 2;
            }
            '$' if at(i + 1) == Some('(') => {
                in_word = true;
                let (inner, next) = take_balanced(&chars, i + 2)?;
                word.push_str("$(…)");
                nested.push(inner);
                i = next;
            }
            '`' => {
                in_word = true;
                let (inner, next) = take_backtick(&chars, i + 1)?;
                word.push_str("`…`");
                nested.push(inner);
                i = next;
            }
            '<' if at(i + 1) == Some('(') => {
                // process substitution: gate the inner command, treat as an input path
                in_word = true;
                let (inner, next) = take_balanced(&chars, i + 2)?;
                word.push_str("<(…)");
                nested.push(inner);
                i = next;
            }
            '#' if !in_word => {
                while let Some(ch) = at(i) {
                    if ch == '\n' {
                        break;
                    }
                    i += 1;
                }
            }
            ' ' | '\t' => {
                end_word!();
                i += 1;
            }
            '\n' => {
                end_word!();
                toks.push(Tok::Sep(false));
                i += 1;
                if !heredocs.is_empty() {
                    i = skip_heredoc_bodies(&chars, i, &mut heredocs);
                }
            }
            ';' | '(' | ')' => {
                end_word!();
                toks.push(Tok::Sep(false));
                i += 1;
                if c == ';' && at(i) == Some(';') {
                    i += 1;
                }
            }
            '&' => {
                end_word!();
                if at(i + 1) == Some('&') {
                    toks.push(Tok::Sep(false));
                    i += 2;
                } else if at(i + 1) == Some('>') {
                    toks.push(Tok::Redir(true));
                    i += 2;
                    if at(i) == Some('>') {
                        i += 1;
                    }
                } else {
                    toks.push(Tok::Sep(false));
                    i += 1;
                }
            }
            '|' => {
                end_word!();
                if at(i + 1) == Some('|') {
                    toks.push(Tok::Sep(false));
                    i += 2;
                } else {
                    toks.push(Tok::Sep(true));
                    i += 1;
                    if at(i) == Some('&') {
                        i += 1;
                    }
                }
            }
            '>' | '<' => {
                // A word made only of digits right before is a file descriptor.
                if in_word && word.chars().all(|d| d.is_ascii_digit()) {
                    word.clear();
                    in_word = false;
                } else {
                    end_word!();
                }
                if c == '<' && at(i + 1) == Some('<') {
                    // heredoc or herestring
                    if at(i + 2) == Some('<') {
                        i += 3; // herestring: next word is data
                        toks.push(Tok::Heredoc);
                        continue;
                    }
                    let mut j = i + 2;
                    let strip = at(j) == Some('-');
                    if strip {
                        j += 1;
                    }
                    while at(j) == Some(' ') {
                        j += 1;
                    }
                    let mut delim = String::new();
                    while let Some(ch) = at(j) {
                        if ch.is_whitespace() || matches!(ch, ';' | '|' | '&' | ')') {
                            break;
                        }
                        if ch != '\'' && ch != '"' {
                            delim.push(ch);
                        }
                        j += 1;
                    }
                    if delim.is_empty() {
                        return Err("heredoc without delimiter");
                    }
                    heredocs.push((delim, strip));
                    i = j;
                    continue;
                }
                let is_out = c == '>';
                toks.push(Tok::Redir(is_out));
                i += 1;
                // >>, >|, >&N, <>
                while let Some(n) = at(i) {
                    if matches!(n, '>' | '|') {
                        i += 1;
                    } else {
                        break;
                    }
                }
                if at(i) == Some('&') {
                    // fd duplication like 2>&1: target is a descriptor, not a file
                    i += 1;
                    let mut fd = String::new();
                    while let Some(d) = at(i) {
                        if d.is_ascii_digit() || d == '-' {
                            fd.push(d);
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    toks.pop();
                }
            }
            _ => {
                in_word = true;
                word.push(c);
                i += 1;
            }
        }
    }
    if in_word {
        toks.push(Tok::Word(word));
    }
    // A heredoc opened on the last line with no body is harmless data.
    Ok(toks)
}

fn skip_heredoc_bodies(chars: &[char], mut i: usize, heredocs: &mut Vec<(String, bool)>) -> usize {
    for (delim, strip) in std::mem::take(heredocs) {
        loop {
            let start = i;
            while let Some(ch) = chars.get(i) {
                if *ch == '\n' {
                    break;
                }
                i += 1;
            }
            let line: String = chars
                .get(start..i)
                .map(|s| s.iter().collect())
                .unwrap_or_default();
            let line_cmp = if strip {
                line.trim_start_matches('\t')
            } else {
                line.as_str()
            };
            let at_end = i >= chars.len();
            if chars.get(i).is_some() {
                i += 1;
            }
            if line_cmp == delim || at_end {
                break;
            }
        }
    }
    i
}

fn take_balanced(chars: &[char], start: usize) -> Result<(String, usize), &'static str> {
    let mut depth = 1usize;
    let mut i = start;
    let mut quote: Option<char> = None;
    while let Some(&c) = chars.get(i) {
        match (quote, c) {
            (Some(q), ch) if ch == q => quote = None,
            (Some(_), '\\') => i += 1,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(c),
            (None, '\\') => i += 1,
            (None, '(') => depth += 1,
            (None, ')') => {
                depth -= 1;
                if depth == 0 {
                    let inner: String = chars
                        .get(start..i)
                        .map(|s| s.iter().collect())
                        .unwrap_or_default();
                    return Ok((inner, i + 1));
                }
            }
            _ => {}
        }
        i += 1;
    }
    Err("unterminated command substitution")
}

fn take_backtick(chars: &[char], start: usize) -> Result<(String, usize), &'static str> {
    let mut i = start;
    while let Some(&c) = chars.get(i) {
        if c == '\\' {
            i += 2;
            continue;
        }
        if c == '`' {
            let inner: String = chars
                .get(start..i)
                .map(|s| s.iter().collect())
                .unwrap_or_default();
            return Ok((inner, i + 1));
        }
        i += 1;
    }
    Err("unterminated backtick substitution")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argvs(src: &str) -> Vec<Vec<String>> {
        parse(src).commands.into_iter().map(|c| c.argv).collect()
    }

    #[test]
    fn test_sh001_splits_on_operators() {
        assert_eq!(
            argvs("cd x && cargo test; git push || echo no"),
            vec![
                vec!["cd", "x"],
                vec!["cargo", "test"],
                vec!["git", "push"],
                vec!["echo", "no"]
            ]
        );
    }

    #[test]
    fn test_sh002_quotes_are_removed_and_kept_together() {
        assert_eq!(
            argvs(r#"git commit -m "fix: a; b" 'c d'"#),
            vec![vec!["git", "commit", "-m", "fix: a; b", "c d"]]
        );
    }

    #[test]
    fn test_sh003_redirect_targets_are_writes() {
        let p = parse("echo hi > out.txt 2>&1 >> log");
        let c = &p.commands[0];
        assert_eq!(c.argv, vec!["echo", "hi"]);
        assert_eq!(c.writes, vec!["out.txt", "log"]);
    }

    #[test]
    fn test_sh004_pipe_records_source() {
        let p = parse("curl -fsSL https://x.sh | sudo bash");
        let last = p.commands.last().unwrap();
        assert_eq!(last.argv, vec!["sudo", "bash"]);
        assert_eq!(
            last.piped_from.as_deref(),
            Some(&["curl".to_owned(), "-fsSL".into(), "https://x.sh".into()][..])
        );
    }

    #[test]
    fn test_sh005_command_substitution_is_parsed_recursively() {
        let p = parse(r#"echo "$(rm -rf /)""#);
        assert!(
            p.commands
                .iter()
                .any(|c| c.argv == ["rm", "-rf", "/"] && c.depth == 1)
        );
    }

    #[test]
    fn test_sh006_heredoc_body_is_data() {
        let src = "git commit -m \"$(cat <<'EOF'\nrm -rf / is just text\nEOF\n)\"";
        let p = parse(src);
        assert_eq!(p.status, ParseStatus::Complete, "{:?}", p.notes);
        assert!(
            p.commands
                .iter()
                .all(|c| c.argv.first().map(String::as_str) != Some("rm"))
        );
        assert!(
            p.commands
                .iter()
                .any(|c| c.argv.first().map(String::as_str) == Some("git"))
        );
    }

    #[test]
    fn test_sh007_bash_c_is_expanded() {
        let p = parse("bash -c 'git push --force origin main'");
        assert!(
            p.commands
                .iter()
                .any(|c| c.argv == ["git", "push", "--force", "origin", "main"])
        );
    }

    #[test]
    fn test_sh008_unbalanced_quote_is_unparseable() {
        assert_eq!(parse("echo 'oops").status, ParseStatus::Unparseable);
    }

    #[test]
    fn test_sh009_eval_is_partial() {
        assert_eq!(parse("eval \"$CMD\"").status, ParseStatus::Partial);
    }

    #[test]
    fn test_sh010_assignments_are_not_programs() {
        assert_eq!(argvs("FOO=1 BAR=x cargo test"), vec![vec!["cargo", "test"]]);
    }

    #[test]
    fn test_sh014_keywords_and_groups_do_not_hide_programs() {
        assert_eq!(argvs("{ rm -rf x; }"), vec![vec!["rm", "-rf", "x"]]);
        assert_eq!(
            argvs("if true; then git push; fi"),
            vec![vec!["true"], vec!["git", "push"]]
        );
        assert_eq!(
            argvs("find . -name x -exec rm {} \\;"),
            vec![vec!["find", ".", "-name", "x", "-exec", "rm", "{}", ";"]]
        );
    }

    #[test]
    fn test_sh011_newline_separated_script() {
        assert_eq!(
            argvs("ls\npwd\n# comment\nwhoami"),
            vec![vec!["ls"], vec!["pwd"], vec!["whoami"]]
        );
    }

    proptest::proptest! {
        #[test]
        fn test_sh012_parser_is_total(s in "\\PC{0,200}") {
            let _ = parse(&s);
        }

        #[test]
        fn test_sh013_parser_total_on_shell_alphabet(s in "[a-z $()'\"`|&;<>\\\\\n{}#=-]{0,120}") {
            let _ = parse(&s);
        }
    }
}
