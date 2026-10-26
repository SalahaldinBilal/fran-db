use serde::Serialize;

/// Whether a statement only reads. Agents in read-only mode may only run reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StatementAccess {
    Read,
    Change { reason: String },
}

impl StatementAccess {
    pub fn change(reason: impl Into<String>) -> Self {
        Self::Change { reason: reason.into() }
    }

    pub fn is_read(&self) -> bool {
        matches!(self, Self::Read)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassifiedStatement {
    pub text: String,
    pub access: StatementAccess,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// A bare word, uppercased.
    Word(String),
    /// A quoted identifier's name, uppercased.
    Identifier(String),
    Literal,
    Symbol(char),
    /// MySQL's `/*! ... */`, which the server runs as code.
    Executable,
}

impl Token {
    fn name(&self) -> Option<&str> {
        match self {
            Self::Word(name) | Self::Identifier(name) => Some(name),
            _ => None,
        }
    }

    fn is_word(&self, word: &str) -> bool {
        matches!(self, Self::Word(found) if found == word)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Syntax {
    pub hash_comments: bool,
    pub dash_comment_needs_space: bool,
    pub nested_comments: bool,
    pub executable_comments: bool,
    pub dollar_quotes: bool,
    /// Postgres `E'...'` strings, which always take backslash escapes.
    pub escape_strings: bool,
    pub backslash_escapes: bool,
    pub double_quote_strings: bool,
    /// A double-quoted string with a backslash reads differently under MySQL's ANSI_QUOTES, so it's refused.
    pub refuse_backslash_in_double_quotes: bool,
    pub backtick_identifiers: bool,
    pub bracket_identifiers: bool,
}

pub type Inspect = fn(&[Token]) -> Option<StatementAccess>;

pub struct Rules<'a> {
    pub syntax: Syntax,
    /// Statements that read rows, checked for anything inside them that writes.
    pub queries: &'a [&'a str],
    /// Statements that describe another statement without running it.
    pub explains: &'a [&'a str],
    /// Statements that only report server state.
    pub shows: &'a [&'a str],
    pub denied_functions: &'a [&'a str],
    /// Rules of one database for statements the lists above don't cover.
    pub inspect: Option<Inspect>,
}

/// Words that never belong in a query that only reads.
const DENIED_WORDS: [&str; 17] = [
    "INSERT", "UPDATE", "DELETE", "MERGE", "UPSERT", "INTO", "TRUNCATE", "DROP", "CREATE", "ALTER", "GRANT",
    "REVOKE", "CALL", "COPY", "ATTACH", "DETACH", "VACUUM",
];

pub fn tokenize(sql: &str, syntax: Syntax) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = sql.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;

    while index < chars.len() {
        let character = chars[index];
        let next = chars.get(index + 1).copied();

        if character.is_whitespace() {
            index += 1;
        } else if character == '#' && syntax.hash_comments {
            index = line_end(&chars, index);
        } else if character == '-' && next == Some('-') {
            let after = chars.get(index + 2);
            if syntax.dash_comment_needs_space && after.is_some_and(|after| !after.is_whitespace()) {
                tokens.push(Token::Symbol('-'));
                index += 1;
            } else {
                index = line_end(&chars, index);
            }
        } else if character == '/' && next == Some('*') {
            if syntax.executable_comments && chars.get(index + 2) == Some(&'!') {
                tokens.push(Token::Executable);
            }
            index = comment_end(&chars, index, syntax.nested_comments)?;
        } else if character == '\'' {
            let escaped = syntax.backslash_escapes || (syntax.escape_strings && ends_with_escape_prefix(&tokens, &chars, index));
            if escaped && syntax.escape_strings && matches!(tokens.last(), Some(Token::Word(word)) if word == "E") {
                tokens.pop();
            }
            index = quoted_end(&chars, index, '\'', escaped)?;
            tokens.push(Token::Literal);
        } else if character == '"' && syntax.double_quote_strings {
            let end = quoted_end(&chars, index, '"', syntax.backslash_escapes)?;
            if syntax.refuse_backslash_in_double_quotes && chars[index..end].contains(&'\\') {
                return Err("A double-quoted string holds a backslash, which reads differently under ANSI_QUOTES".into());
            }
            index = end;
            tokens.push(Token::Literal);
        } else if character == '"' || (character == '`' && syntax.backtick_identifiers) {
            let end = quoted_end(&chars, index, character, false)?;
            tokens.push(Token::Identifier(unquote(&chars[index + 1..end - 1], character)));
            index = end;
        } else if character == '[' && syntax.bracket_identifiers {
            let end = chars[index..]
                .iter()
                .position(|&found| found == ']')
                .map(|position| index + position + 1)
                .ok_or("A bracketed name isn't closed")?;
            tokens.push(Token::Identifier(chars[index + 1..end - 1].iter().collect::<String>().to_uppercase()));
            index = end;
        } else if character == '$' && syntax.dollar_quotes && let Some(end) = dollar_quoted_end(&chars, index)? {
            tokens.push(Token::Literal);
            index = end;
        } else if character.is_alphabetic() || character == '_' {
            let end = word_end(&chars, index);
            tokens.push(Token::Word(chars[index..end].iter().collect::<String>().to_uppercase()));
            index = end;
        } else if character.is_ascii_digit() {
            index = word_end(&chars, index);
            while chars.get(index) == Some(&'.') || chars.get(index).is_some_and(|found| found.is_ascii_alphanumeric()) {
                index += 1;
            }
            tokens.push(Token::Literal);
        } else {
            tokens.push(Token::Symbol(character));
            index += 1;
        }
    }

    Ok(tokens)
}

fn ends_with_escape_prefix(tokens: &[Token], chars: &[char], quote: usize) -> bool {
    quote > 0 && matches!(chars[quote - 1], 'E' | 'e') && matches!(tokens.last(), Some(Token::Word(word)) if word == "E")
}

fn unquote(body: &[char], quote: char) -> String {
    let doubled: String = [quote, quote].iter().collect();
    body.iter().collect::<String>().replace(&doubled, &quote.to_string()).to_uppercase()
}

fn line_end(chars: &[char], start: usize) -> usize {
    chars[start..]
        .iter()
        .position(|&character| character == '\n')
        .map_or(chars.len(), |position| start + position)
}

fn comment_end(chars: &[char], start: usize, nested: bool) -> Result<usize, String> {
    let mut depth = 0usize;
    let mut index = start;
    while index + 1 < chars.len() {
        if chars[index] == '/' && chars[index + 1] == '*' && (nested || depth == 0) {
            depth += 1;
            index += 2;
        } else if chars[index] == '*' && chars[index + 1] == '/' {
            depth -= 1;
            index += 2;
            if depth == 0 {
                return Ok(index);
            }
        } else {
            index += 1;
        }
    }
    Err("A comment isn't closed".into())
}

fn quoted_end(chars: &[char], start: usize, quote: char, backslash_escapes: bool) -> Result<usize, String> {
    let mut index = start + 1;
    while index < chars.len() {
        let character = chars[index];
        if backslash_escapes && character == '\\' {
            index += 2;
            continue;
        }
        if character == quote {
            if chars.get(index + 1) == Some(&quote) {
                index += 2;
                continue;
            }
            return Ok(index + 1);
        }
        index += 1;
    }
    Err("A quoted string or name isn't closed".into())
}

/// None when the `$` doesn't open a `$tag$` string, as in `$1` or `a$b`.
fn dollar_quoted_end(chars: &[char], start: usize) -> Result<Option<usize>, String> {
    if start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
        return Ok(None);
    }
    let mut tag_end = start + 1;
    while tag_end < chars.len() && (chars[tag_end].is_alphanumeric() || chars[tag_end] == '_') {
        tag_end += 1;
    }
    if chars.get(tag_end) != Some(&'$') || (tag_end > start + 1 && chars[start + 1].is_ascii_digit()) {
        return Ok(None);
    }

    let tag = &chars[start..=tag_end];
    let mut search = tag_end + 1;
    while search + tag.len() <= chars.len() {
        if chars[search..search + tag.len()] == *tag {
            return Ok(Some(search + tag.len()));
        }
        search += 1;
    }
    Err("A dollar-quoted string isn't closed".into())
}

fn word_end(chars: &[char], start: usize) -> usize {
    let mut index = start;
    while index < chars.len() && (chars[index].is_alphanumeric() || chars[index] == '_' || chars[index] == '$') {
        index += 1;
    }
    index
}

pub fn classify_statement(sql: &str, rules: &Rules) -> StatementAccess {
    let tokens = match tokenize(sql, rules.syntax) {
        Ok(tokens) => tokens,
        Err(reason) => return StatementAccess::change(reason),
    };

    if tokens.contains(&Token::Executable) {
        return StatementAccess::change("Executable comments (/*! */) can run anything");
    }
    if tokens.contains(&Token::Symbol(';')) {
        return StatementAccess::change("It holds more than one statement");
    }

    let Some(first) = tokens.iter().find(|token| **token != Token::Symbol('(')) else {
        return StatementAccess::change("It's empty");
    };
    let Token::Word(first) = first else {
        return StatementAccess::change("It doesn't start with a keyword");
    };

    if let Some(access) = rules.inspect.and_then(|inspect| inspect(&tokens)) {
        return access;
    }

    let first = first.as_str();
    if rules.shows.contains(&first) {
        return StatementAccess::Read;
    }
    if rules.explains.contains(&first) {
        if tokens.iter().any(|token| token.is_word("ANALYZE") || token.is_word("ANALYSE")) {
            return StatementAccess::change(format!("{first} ANALYZE runs the statement"));
        }
        return denied_function(&tokens, rules).unwrap_or(StatementAccess::Read);
    }
    if !rules.queries.contains(&first) {
        return StatementAccess::change(format!("{first} statements can change data or settings"));
    }

    denied_word(&tokens).or_else(|| denied_function(&tokens, rules)).unwrap_or(StatementAccess::Read)
}

fn denied_word(tokens: &[Token]) -> Option<StatementAccess> {
    for (index, token) in tokens.iter().enumerate() {
        let Token::Word(word) = token else { continue };
        let next = tokens.get(index + 1);

        if DENIED_WORDS.contains(&word.as_str()) {
            return Some(StatementAccess::change(format!("It uses {word}, which writes or locks")));
        }
        let pair = next.and_then(Token::name).map(|next| (word.as_str(), next));
        let locks = matches!(pair, Some(("FOR", "SHARE" | "KEY")) | Some(("LOCK", "IN")));
        if locks {
            return Some(StatementAccess::change("It locks the rows it reads"));
        }
        if matches!(pair, Some(("NEXT", "VALUE"))) {
            return Some(StatementAccess::change("NEXT VALUE FOR advances a sequence"));
        }
        if word == "REPLACE" && next != Some(&Token::Symbol('(')) {
            return Some(StatementAccess::change("It uses REPLACE, which writes"));
        }
    }
    None
}

fn denied_function(tokens: &[Token], rules: &Rules) -> Option<StatementAccess> {
    tokens.windows(2).find_map(|pair| {
        let name = pair[0].name()?;
        (pair[1] == Token::Symbol('(') && rules.denied_functions.contains(&name))
            .then(|| StatementAccess::change(format!("It calls {}(), which can change things", name.to_lowercase())))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: Rules = Rules {
        syntax: Syntax {
            hash_comments: false,
            dash_comment_needs_space: false,
            nested_comments: true,
            executable_comments: false,
            dollar_quotes: true,
            escape_strings: true,
            backslash_escapes: false,
            double_quote_strings: false,
            refuse_backslash_in_double_quotes: false,
            backtick_identifiers: false,
            bracket_identifiers: false,
        },
        queries: &["SELECT", "WITH", "VALUES", "TABLE"],
        explains: &["EXPLAIN"],
        shows: &["SHOW"],
        denied_functions: &["SET_CONFIG"],
        inspect: None,
    };

    fn read(sql: &str) -> bool {
        classify_statement(sql, &RULES).is_read()
    }

    #[test]
    fn allows_plain_reads() {
        assert!(read("select * from users where name = 'delete' and \"update\" = 1"));
        assert!(read("(select 1) union (select 2)"));
        assert!(read("with recent as (select * from t) select count(*) from recent"));
        assert!(read("select replace(name, 'a', 'b'), deleted_at from t -- insert\n"));
        assert!(read("show search_path"));
        assert!(read("explain delete from t"));
        assert!(read("select $body$ drop table t; $body$, $1"));
    }

    #[test]
    fn refuses_anything_that_writes() {
        assert!(!read("delete from t"));
        assert!(!read("with gone as (delete from t returning *) select * from gone"));
        assert!(!read("select * into copy from t"));
        assert!(!read("select * from t for update"));
        assert!(!read("select * from t for share"));
        assert!(!read("select set_config('default_transaction_read_only', 'off', false)"));
        assert!(!read("select \"set_config\"('a', 'b', false)"));
        assert!(!read("explain analyze select 1"));
        assert!(!read("explain (analyze) select 1"));
        assert!(!read("select 1; delete from t"));
        assert!(!read("set default_transaction_read_only = off"));
        assert!(!read("select 'unclosed"));
        assert!(!read(""));
    }

    #[test]
    fn backslashes_only_escape_in_e_strings() {
        assert!(read(r"select E'a\'; delete from t; --'"));
        assert!(!read(r"select 'a\'; delete from t"));
    }
}
