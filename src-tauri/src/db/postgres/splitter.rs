use crate::db::types::StatementRange;

/// How many leading words of a statement are kept to tell a routine definition apart.
const HEADER_WORDS: usize = 4;

pub fn split(sql: &str) -> Vec<StatementRange> {
    let chars: Vec<char> = sql.chars().collect();
    let total = chars.len();

    let mut utf16_offsets = Vec::with_capacity(total + 1);
    let mut offset = 0;
    utf16_offsets.push(0);
    for character in &chars {
        offset += character.len_utf16();
        utf16_offsets.push(offset);
    }

    let mut statements = Vec::new();
    let mut start: Option<usize> = None;
    let mut code_end = 0;
    let mut index = 0;
    let mut words: Vec<String> = Vec::new();
    let mut block_depth = 0usize;

    let finish = |start: usize, code_end: usize, statements: &mut Vec<StatementRange>| {
        if code_end > start {
            statements.push(StatementRange {
                start: utf16_offsets[start],
                end: utf16_offsets[code_end],
                text: chars[start..code_end].iter().collect(),
            });
        }
    };

    while index < total {
        let character = chars[index];

        if start.is_none() {
            if character.is_whitespace() {
                index += 1;
                continue;
            }
            if let Some(end) = comment_end(&chars, index) {
                index = end;
                continue;
            }
            start = Some(index);
            code_end = index;
            words.clear();
            block_depth = 0;
        }

        if character == '\'' {
            index = quoted_end(&chars, index, '\'', is_escape_string(&chars, index));
            code_end = index;
        } else if character == '"' {
            index = quoted_end(&chars, index, '"', false);
            code_end = index;
        } else if character == '$' {
            index = dollar_quoted_end(&chars, index).unwrap_or(index + 1);
            code_end = index;
        } else if character == ';' && block_depth == 0 {
            if let Some(statement_start) = start.take() {
                finish(statement_start, code_end, &mut statements);
            }
            index += 1;
        } else if let Some(end) = comment_end(&chars, index) {
            index = end;
        } else if is_word_start(character) && !(index > 0 && is_identifier_char(chars[index - 1])) {
            let end = word_end(&chars, index);
            let word: String = chars[index..end].iter().collect::<String>().to_ascii_uppercase();
            track_word(&mut words, &mut block_depth, &word);
            index = end;
            code_end = index;
        } else {
            if !character.is_whitespace() {
                code_end = index + 1;
            }
            index += 1;
        }
    }

    if let Some(statement_start) = start {
        finish(statement_start, code_end, &mut statements);
    }

    statements
}

fn is_word_start(character: char) -> bool {
    character.is_alphabetic() || character == '_'
}

fn is_identifier_char(character: char) -> bool {
    character.is_alphanumeric() || character == '_' || character == '$'
}

fn word_end(chars: &[char], start: usize) -> usize {
    let mut index = start;
    while index < chars.len() && is_identifier_char(chars[index]) {
        index += 1;
    }
    index
}

/// `E'...'` strings take backslash escapes. The `E` must stand alone, not end a longer word.
fn is_escape_string(chars: &[char], quote: usize) -> bool {
    quote > 0
        && matches!(chars[quote - 1], 'E' | 'e')
        && (quote < 2 || !is_identifier_char(chars[quote - 2]))
}

fn quoted_end(chars: &[char], start: usize, quote: char, backslash_escapes: bool) -> usize {
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
            return index + 1;
        }
        index += 1;
    }

    chars.len()
}

/// The end of a `$tag$ ... $tag$` string starting at `start`, or None when the `$` doesn't open one,
/// as in the parameter `$1` or the identifier `a$b`.
fn dollar_quoted_end(chars: &[char], start: usize) -> Option<usize> {
    if start > 0 && is_identifier_char(chars[start - 1]) {
        return None;
    }

    let mut tag_end = start + 1;
    while tag_end < chars.len() && (chars[tag_end].is_alphanumeric() || chars[tag_end] == '_') {
        tag_end += 1;
    }
    if chars.get(tag_end) != Some(&'$') {
        return None;
    }
    if tag_end > start + 1 && chars[start + 1].is_ascii_digit() {
        return None;
    }

    let tag = &chars[start..=tag_end];
    let mut search = tag_end + 1;
    while search + tag.len() <= chars.len() {
        if chars[search..search + tag.len()] == *tag {
            return Some(search + tag.len());
        }
        search += 1;
    }
    Some(chars.len())
}

fn line_end(chars: &[char], start: usize) -> usize {
    chars[start..]
        .iter()
        .position(|&character| character == '\n')
        .map_or(chars.len(), |position| start + position)
}

fn block_comment_end(chars: &[char], start: usize) -> usize {
    let mut depth = 0usize;
    let mut index = start;

    while index + 1 < chars.len() {
        if chars[index] == '/' && chars[index + 1] == '*' {
            depth += 1;
            index += 2;
        } else if chars[index] == '*' && chars[index + 1] == '/' {
            depth -= 1;
            index += 2;
            if depth == 0 {
                return index;
            }
        } else {
            index += 1;
        }
    }

    chars.len()
}

fn comment_end(chars: &[char], index: usize) -> Option<usize> {
    match (chars[index], chars.get(index + 1)) {
        ('-', Some('-')) => Some(line_end(chars, index)),
        ('/', Some('*')) => Some(block_comment_end(chars, index)),
        _ => None,
    }
}

fn is_routine_definition(words: &[String]) -> bool {
    let Some((first, rest)) = words.split_first() else {
        return false;
    };
    if first != "CREATE" {
        return false;
    }
    let rest = match rest {
        [or, replace, rest @ ..] if or == "OR" && replace == "REPLACE" => rest,
        rest => rest,
    };
    matches!(rest.first().map(String::as_str), Some("FUNCTION" | "PROCEDURE"))
}

/// SQL-standard routine bodies (`BEGIN ATOMIC ... END`) hold semicolons, and CASE inside them ends with END too.
fn track_word(words: &mut Vec<String>, block_depth: &mut usize, word: &str) {
    if words.len() < HEADER_WORDS {
        words.push(word.to_string());
        return;
    }
    if !is_routine_definition(words) {
        return;
    }

    match word {
        "BEGIN" => *block_depth += 1,
        "CASE" if *block_depth > 0 => *block_depth += 1,
        "END" if *block_depth > 0 => *block_depth -= 1,
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::split;

    fn texts(sql: &str) -> Vec<String> {
        split(sql).into_iter().map(|statement| statement.text).collect()
    }

    #[test]
    fn splits_on_semicolons() {
        assert_eq!(texts("select 1; select 2"), vec!["select 1", "select 2"]);
    }

    #[test]
    fn ignores_semicolons_in_quotes_and_comments() {
        let sql = "select ';', \"a;b\"; -- note; here\nselect 2 /* x; /* nested; */ y; */; select 3";
        assert_eq!(texts(sql), vec!["select ';', \"a;b\"", "select 2", "select 3"]);
    }

    #[test]
    fn handles_dollar_quotes() {
        let sql = "create function f() returns int as $$ select 1; $$ language sql; \
                   select $1, $body$ a;b $body$, a$b; select 3";
        assert_eq!(
            texts(sql),
            vec![
                "create function f() returns int as $$ select 1; $$ language sql",
                "select $1, $body$ a;b $body$, a$b",
                "select 3",
            ]
        );
    }

    #[test]
    fn backslashes_only_escape_in_e_strings() {
        assert_eq!(
            texts(r"select E'a\';b'; select 'c\'; select 3"),
            vec![r"select E'a\';b'", r"select 'c\'", "select 3"]
        );
    }

    #[test]
    fn keeps_begin_atomic_bodies_together() {
        let sql = "CREATE OR REPLACE FUNCTION f() RETURNS int LANGUAGE sql BEGIN ATOMIC \
                   SELECT CASE WHEN true THEN 1 END; SELECT 2; END; select 3";
        assert_eq!(
            texts(sql),
            vec![
                "CREATE OR REPLACE FUNCTION f() RETURNS int LANGUAGE sql BEGIN ATOMIC \
                 SELECT CASE WHEN true THEN 1 END; SELECT 2; END",
                "select 3",
            ]
        );
    }

    #[test]
    fn transaction_blocks_still_split() {
        assert_eq!(texts("BEGIN; select 1; END;"), vec!["BEGIN", "select 1", "END"]);
    }

    #[test]
    fn skips_comment_only_statements() {
        assert_eq!(texts("-- just a note\n;\n/* x */"), Vec::<String>::new());
    }

    #[test]
    fn reports_utf16_offsets() {
        let statements = split("select '😀'; select 2");
        assert_eq!(statements[0].start, 0);
        assert_eq!(statements[0].end, 11);
        assert_eq!(statements[1].start, 13);
    }
}
