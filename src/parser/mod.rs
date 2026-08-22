use crate::core::{Command, FusionInfo, Pipeline};
use crate::engine::{Flags, StructuralRegex};
use anyhow::{Result, anyhow};

pub struct Parser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    pub fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn advance_while<F>(&mut self, condition: F)
    where
        F: Fn(char) -> bool,
    {
        while let Some(c) = self.peek() {
            if condition(c) {
                self.pos += c.len_utf8();
            } else {
                break;
            }
        }
    }

    fn skip_whitespace(&mut self) {
        self.advance_while(|c| c.is_whitespace());
    }

    /// Reads a `/regex/`-style delimited segment. The first character is taken
    /// as the delimiter, and everything up to the next unescaped delimiter is
    /// returned (without the delimiters). `\` escapes the following character.
    fn parse_pattern(&mut self) -> Result<&'a str> {
        let delimiter = self
            .peek()
            .ok_or_else(|| anyhow!("Expected delimiter, got EOF"))?;
        self.pos += delimiter.len_utf8();
        self.read_until(delimiter)
    }

    /// Reads bytes up to (and consuming) the next unescaped `delimiter`.
    fn read_until(&mut self, delimiter: char) -> Result<&'a str> {
        let start = self.pos;
        let mut escaped = false;
        while let Some(c) = self.peek() {
            self.pos += c.len_utf8();
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == delimiter {
                return Ok(&self.input[start..self.pos - delimiter.len_utf8()]);
            }
        }
        Err(anyhow!(
            "Unterminated pattern, expected delimiter '{}'",
            delimiter
        ))
    }

    /// Consumes a run of trailing flag letters glued to a closing delimiter.
    /// Only the known flags (`i`/`I`, `m`/`M`, `s`, `g`, and a `s///N` numeric
    /// occurrence selector) are consumed, so a following command (`p`, `=`, `x`,
    /// …) is left intact even with no space. `I`/`M` are GNU sed's uppercase
    /// aliases for case-insensitive / multi-line.
    fn parse_flags(&mut self) -> Flags {
        let mut f = Flags::default();
        while let Some(c) = self.peek() {
            match c {
                'i' | 'I' => f.case_insensitive = true,
                's' => f.dot_all = true,
                'g' => f.global = true,
                'm' | 'M' => {} // multi-line is the default in se
                '0'..='9' => {
                    let d = c as usize - '0' as usize;
                    f.occurrence = Some(f.occurrence.unwrap_or(0).saturating_mul(10) + d);
                }
                _ => break,
            }
            self.pos += c.len_utf8();
        }
        f
    }

    /// Reads the raw text inside an awk `@{ ... }` action with balanced-brace
    /// matching, skipping braces inside `"..."` strings and `#` comments. The
    /// opening `{` has NOT yet been consumed; on return `pos` is past the
    /// matching `}`.
    fn read_awk_program(&mut self) -> Result<&'a str> {
        self.skip_whitespace();
        if self.peek() != Some('{') {
            return Err(anyhow!("@ awk action requires a '{{ ... }}' block"));
        }
        self.pos += 1; // consume '{'
        let start = self.pos;
        let mut depth = 1usize;
        let mut in_str = false;
        let mut escaped = false;
        while let Some(c) = self.peek() {
            let clen = c.len_utf8();
            if in_str {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    in_str = false;
                }
                self.pos += clen;
                continue;
            }
            match c {
                '"' => self.pos += clen,
                '#' => {
                    // awk comment to end of line
                    while let Some(cc) = self.peek() {
                        if cc == '\n' {
                            break;
                        }
                        self.pos += cc.len_utf8();
                    }
                }
                '{' => {
                    depth += 1;
                    self.pos += clen;
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        let inner = &self.input[start..self.pos];
                        self.pos += clen; // consume closing '}'
                        return Ok(inner);
                    }
                    self.pos += clen;
                }
                _ => self.pos += clen,
            }
            if c == '"' {
                in_str = true;
            }
        }
        Err(anyhow!("@ awk action: unterminated '{{' block"))
    }

    /// Parses a `{ ... }` block, assuming the opening `{` has NOT yet been
    /// consumed. Returns the inner pipeline.
    fn parse_block(&mut self) -> Result<Pipeline> {
        let open = self.peek();
        if open != Some('{') {
            return Err(anyhow!("Expected '{{' to open a block"));
        }
        self.pos += 1;
        let inner = self.parse_pipeline()?;
        self.skip_whitespace();
        match self.peek() {
            Some('}') => {
                self.pos += 1;
                Ok(inner)
            }
            Some(other) => Err(anyhow!("Expected '}}', found '{}'", other)),
            None => Err(anyhow!("Unterminated block: expected closing '}}'")),
        }
    }

    pub fn parse_pipeline(&mut self) -> Result<Pipeline> {
        let mut pipeline = Pipeline::new();

        loop {
            self.skip_whitespace();
            while self.peek() == Some(';') {
                self.pos += 1;
                self.skip_whitespace();
            }

            if self.pos >= self.input.len() {
                break;
            }

            let c = self.peek().unwrap();

            // A closing brace ends a sub-pipeline; let the caller consume it.
            if c == '}' {
                break;
            }

            self.pos += c.len_utf8();

            match c {
                'x' | 'y' | 'z' | 'g' | 'v' => {
                    let pattern = self.parse_pattern()?;
                    let flags = self.parse_flags();
                    let re = StructuralRegex::compile_with(pattern, flags)?;
                    match c {
                        // x and z/y are all "split on the match" selectors;
                        // x keeps the matches, y/z keep the gaps between them.
                        'x' => pipeline.push(Box::new(crate::commands::ExtractCommand { re })),
                        'y' | 'z' => pipeline.push(Box::new(crate::commands::YankCommand { re })),
                        'g' => pipeline.push(Box::new(crate::commands::GlobalCommand { re })),
                        'v' => pipeline.push(Box::new(crate::commands::NotGlobalCommand { re })),
                        _ => unreachable!(),
                    }
                }
                'c' => {
                    let text = self.parse_pattern()?;
                    pipeline.push(Box::new(crate::commands::ChangeCommand {
                        replacement: unescape(text),
                    }));
                }
                'p' => pipeline.push(Box::new(crate::commands::PrintCommand)),
                '=' => pipeline.push(Box::new(crate::commands::PrintLineCommand)),
                'N' => pipeline.push(Box::new(crate::commands::NextCommand)),
                // Structural tree navigation: `+` widens to the enclosing
                // bracketed block, `-` descends into the first bracket pair.
                '+' => pipeline.push(Box::new(crate::commands::ExpandCommand)),
                '-' => pipeline.push(Box::new(crate::commands::CollapseCommand)),
                '~' => {
                    // Fuzzy selector `~k/pattern/`: keep views within edit
                    // distance k of the literal pattern.
                    let mut k = 0usize;
                    let mut saw_digit = false;
                    while let Some(d) = self.peek() {
                        if let Some(v) = d.to_digit(10) {
                            k = k * 10 + v as usize;
                            saw_digit = true;
                            self.pos += d.len_utf8();
                        } else {
                            break;
                        }
                    }
                    if !saw_digit {
                        return Err(anyhow!("~ fuzzy match needs a distance, e.g. ~2/pattern/"));
                    }
                    let pattern = self.parse_pattern()?;
                    let matcher = crate::engine::FuzzyMatcher::new(&unescape(pattern), k);
                    pipeline.push(Box::new(crate::commands::FuzzyCommand { matcher }));
                }
                ':' => {
                    // Named modifier `:name:/arg/`. Currently only `:sem:` —
                    // concept-based matching against the built-in lexicon.
                    let modifier = self.read_until(':')?;
                    match modifier {
                        "sem" => {
                            self.skip_whitespace();
                            let concept = self.parse_pattern()?;
                            let matcher = crate::engine::SemanticMatcher::new(concept)?;
                            pipeline.push(Box::new(crate::commands::SemanticCommand { matcher }));
                        }
                        other => {
                            return Err(anyhow!(
                                "unknown modifier ':{}:' (only ':sem:' is supported)",
                                other
                            ));
                        }
                    }
                }
                '@' => {
                    // Awk action `@{ program }`: each incoming view is a record.
                    let src = self.read_awk_program()?;
                    let program = crate::awk::Program::parse(src)?;
                    pipeline.push(Box::new(crate::commands::AwkProgramCommand {
                        program,
                        state: std::sync::Mutex::new(crate::awk::Interp::new()),
                    }));
                }
                '{' => {
                    self.pos -= 1; // hand the '{' to parse_block
                    let sub = self.parse_block()?;
                    pipeline.push(Box::new(crate::commands::GroupCommand { pipeline: sub }));
                }
                '/' => {
                    // Awk-style `/pattern/[flags] { ... }` (or bare `/pattern/` => print).
                    self.pos -= c.len_utf8(); // step back so parse_pattern sees the delimiter
                    let pattern = self.parse_pattern()?;
                    let flags = self.parse_flags();
                    let re = StructuralRegex::compile_with(pattern, flags)?;

                    self.skip_whitespace();
                    let action = if self.peek() == Some('{') {
                        self.parse_block()?
                    } else {
                        let mut p = Pipeline::new();
                        p.push(Box::new(crate::commands::PrintCommand));
                        p
                    };
                    pipeline.push(Box::new(crate::commands::AwkCommand {
                        re,
                        pipeline: action,
                    }));
                }
                'm' => {
                    let pattern = self.parse_pattern()?;
                    let flags = self.parse_flags();
                    let re = StructuralRegex::compile_with(pattern, flags)?;
                    self.skip_whitespace();
                    let sub = self.parse_block()?;
                    pipeline.push(Box::new(crate::commands::MapCommand { re, pipeline: sub }));
                }
                't' => {
                    self.skip_whitespace();
                    let sub = self.parse_block()?;
                    pipeline.push(Box::new(crate::commands::TestCommand { pipeline: sub }));
                }
                's' => {
                    let delimiter = self
                        .peek()
                        .ok_or_else(|| anyhow!("s/// requires a delimiter"))?;
                    self.pos += delimiter.len_utf8();
                    let pattern = self.read_until(delimiter)?;
                    let replacement = self.read_until(delimiter)?;
                    let flags = self.parse_flags();
                    if flags.occurrence == Some(0) {
                        return Err(anyhow!("s/// occurrence selector may not be zero"));
                    }
                    let re = StructuralRegex::compile_with(pattern, flags)?;
                    let template = crate::commands::ReplacementTemplate::compile(replacement, &re);
                    pipeline.push(Box::new(crate::commands::SubstituteCommand {
                        re,
                        template,
                        global: flags.global,
                        occurrence: flags.occurrence.unwrap_or(1),
                    }));
                }
                'r' => {
                    let separator = self.parse_pattern()?;
                    pipeline.push(Box::new(crate::commands::ReduceCommand {
                        separator: unescape(separator),
                    }));
                }
                _ => return Err(anyhow!("Unexpected command character: '{}'", c)),
            }
        }

        Ok(pipeline)
    }
}

/// Expands the small set of C-style escapes that are useful in replacement
/// and separator text (`\n`, `\t`, `\r`, `\0`, `\\`, and `\<delim>`). Unknown
/// escapes keep the following character verbatim.
fn unescape(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            let next = bytes[i + 1];
            match next {
                b'n' => out.push(b'\n'),
                b't' => out.push(b'\t'),
                b'r' => out.push(b'\r'),
                b'0' => out.push(0),
                other => out.push(other),
            }
            i += 2;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

pub fn parse(program: &str) -> Result<Pipeline> {
    let mut parser = Parser::new(program);
    let pipeline = parser.parse_pipeline()?;
    // Reject trailing garbage such as an unmatched closing brace.
    parser.skip_whitespace();
    if parser.pos < parser.input.len() {
        return Err(anyhow!(
            "Unexpected '{}' at end of program",
            &parser.input[parser.pos..]
        ));
    }
    Ok(pipeline)
}

/// Peephole optimizer: fuse an adjacent line splitter and containment filter —
/// `x/.*\n/ g/lit/` or `x/.*\n/ v/lit/` — into one search-then-extend
/// [`LineFilterCommand`](crate::commands::LineFilterCommand). This is the
/// grep-beating fast path: instead of materializing one view per line and
/// filtering it, the filter literal is located with NEON and each hit is
/// extended to its line.
///
/// It only fires when the filter compiled to a newline-free NEON literal, so the
/// rewrite is always behaviour-preserving (`require_newline` is carried over
/// from the splitter so trailing-line handling is identical). Runs on the
/// top-level pipeline only; nested blocks are left as written.
pub fn optimize(pipeline: Pipeline) -> Pipeline {
    let infos: Vec<FusionInfo> = pipeline.commands.iter().map(|c| c.fusion_info()).collect();
    let mut slots: Vec<Option<Box<dyn Command>>> =
        pipeline.commands.into_iter().map(Some).collect();
    let mut out: Vec<Box<dyn Command>> = Vec::with_capacity(slots.len());

    let mut i = 0;
    while i < infos.len() {
        if let FusionInfo::LineSplit { require_newline } = &infos[i] {
            let require_newline = *require_newline;
            match infos.get(i + 1) {
                // `x/.*\n/ g|v/lit/` → one LineFilterCommand replacing both. Safe
                // for any position: a line filter selects, exactly like `g`/`v`.
                Some(FusionInfo::LineFilter { re, invert }) if line_safe_literal(re) => {
                    out.push(Box::new(crate::commands::LineFilterCommand {
                        matcher: re.as_literal().unwrap(),
                        invert: *invert,
                        require_newline,
                    }));
                    i += 2;
                    continue;
                }
                // `x/.*\n/ g/lit/i` (case-insensitive literal → meta engine, so no
                // NEON matcher): still a newline-free literal, so run one whole-
                // buffer search and extend to lines instead of matching per line.
                // Non-invert only; `v/lit/i` stays on the per-line path.
                Some(FusionInfo::LineFilter { re, invert })
                    if !*invert && re.as_literal().is_none() && re.is_plain_literal() =>
                {
                    out.push(Box::new(crate::commands::RegexLineFilterCommand {
                        re: re.clone(),
                        require_newline,
                    }));
                    i += 2;
                    continue;
                }
                // `/lit/ { p }` (or bare `/lit/`) → LineFilterCommand + Print, but
                // only when the awk binder is the *last* command: it passes every
                // view through, so fusing it to a selector is only equivalent when
                // nothing downstream consumes that pass-through stream.
                Some(FusionInfo::AwkPrint { re })
                    if i + 1 == infos.len() - 1 && line_safe_literal(re) =>
                {
                    out.push(Box::new(crate::commands::LineFilterCommand {
                        matcher: re.as_literal().unwrap(),
                        invert: false,
                        require_newline,
                    }));
                    out.push(Box::new(crate::commands::PrintCommand));
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        out.push(slots[i].take().expect("each slot consumed at most once"));
        i += 1;
    }

    Pipeline { commands: out }
}

/// A filter is fusible only when it compiled to a NEON literal whose needle is
/// itself newline-free, so search-then-extend cannot stray across a line.
fn line_safe_literal(re: &StructuralRegex) -> bool {
    re.as_literal()
        .is_some_and(|m| !m.as_bytes().contains(&b'\n'))
}
