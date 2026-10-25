use crate::db::types::StatementRange;

/// How many leading words of a statement are kept to tell a trigger definition apart.
const HEADER_WORDS: usize = 3;

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

        if let Some(close) = closing_quote(character) {
            index = quoted_end(&chars, index, close);
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

fn closing_quote(character: char) -> Option<char> {
    match character {
        '\'' | '"' | '`' => Some(character),
        '[' => Some(']'),
        _ => None,
    }
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

/// A doubled closing quote stands for itself, except inside brackets, which can't hold `]`.
fn quoted_end(chars: &[char], start: usize, close: char) -> usize {
    let mut index = start + 1;

    while index < chars.len() {
        if chars[index] == close {
            if close != ']' && chars.get(index + 1) == Some(&close) {
                index += 2;
                continue;
            }
            return index + 1;
        }
        index += 1;
    }

    chars.len()
}

fn line_end(chars: &[char], start: usize) -> usize {
    chars[start..]
        .iter()
        .position(|&character| character == '\n')
        .map_or(chars.len(), |position| start + position)
}

/// SQLite block comments don't nest.
fn block_comment_end(chars: &[char], start: usize) -> usize {
    let mut index = start + 2;
    while index + 1 < chars.len() {
        if chars[index] == '*' && chars[index + 1] == '/' {
            return index + 2;
        }
        index += 1;
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

fn is_trigger_definition(words: &[String]) -> bool {
    let Some((first, rest)) = words.split_first() else {
        return false;
    };
    if first != "CREATE" {
        return false;
    }
    let rest = match rest {
        [temp, rest @ ..] if temp == "TEMP" || temp == "TEMPORARY" => rest,
        rest => rest,
    };
    rest.first().map(String::as_str) == Some("TRIGGER")
}

/// Trigger bodies (`BEGIN ... END`) hold semicolons, and CASE inside them ends with END too.
fn track_word(words: &mut Vec<String>, block_depth: &mut usize, word: &str) {
    if words.len() < HEADER_WORDS {
        words.push(word.to_string());
        return;
    }
    if !is_trigger_definition(words) {
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
        let sql = "select ';', \"a;b\", `c;d`, [e;f]; -- note; here\nselect 2 /* x; */; select 3";
        assert_eq!(
            texts(sql),
            vec!["select ';', \"a;b\", `c;d`, [e;f]", "select 2", "select 3"]
        );
    }

    #[test]
    fn block_comments_do_not_nest() {
        assert_eq!(texts("select 1 /* /* */; select 2"), vec!["select 1", "select 2"]);
    }

    #[test]
    fn backslashes_do_not_escape() {
        assert_eq!(texts(r"select 'a\'; select 2"), vec![r"select 'a\'", "select 2"]);
    }

    #[test]
    fn keeps_trigger_bodies_together() {
        let sql = "CREATE TEMP TRIGGER t AFTER INSERT ON a BEGIN \
                   UPDATE b SET x = CASE WHEN new.y THEN 1 END; DELETE FROM c; END; select 3";
        assert_eq!(
            texts(sql),
            vec![
                "CREATE TEMP TRIGGER t AFTER INSERT ON a BEGIN \
                 UPDATE b SET x = CASE WHEN new.y THEN 1 END; DELETE FROM c; END",
                "select 3",
            ]
        );
    }

    #[test]
    fn transaction_blocks_still_split() {
        assert_eq!(
            texts("BEGIN TRANSACTION; select 1; END;"),
            vec!["BEGIN TRANSACTION", "select 1", "END"]
        );
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
