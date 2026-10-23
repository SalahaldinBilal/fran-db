use crate::db::types::StatementRange;

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
    let mut delimiter: Vec<char> = vec![';'];
    let mut start: Option<usize> = None;
    let mut code_end = 0;
    let mut index = 0;

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
            if let Some(end) = plain_comment_end(&chars, index) {
                index = end;
                continue;
            }
            if let Some((end, new_delimiter)) = delimiter_command(&chars, index) {
                if !new_delimiter.is_empty() {
                    delimiter = new_delimiter;
                }
                index = end;
                continue;
            }
            start = Some(index);
            code_end = index;
        }

        if matches!(character, '\'' | '"' | '`') {
            index = quoted_end(&chars, index);
            code_end = index;
        } else if matches_at(&chars, index, &delimiter) {
            if let Some(statement_start) = start.take() {
                finish(statement_start, code_end, &mut statements);
            }
            index += delimiter.len();
        } else if is_executable_comment(&chars, index) {
            index = block_comment_end(&chars, index);
            code_end = index;
        } else if let Some(end) = plain_comment_end(&chars, index) {
            index = end;
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

fn matches_at(chars: &[char], index: usize, pattern: &[char]) -> bool {
    chars.len() >= index + pattern.len() && chars[index..index + pattern.len()] == *pattern
}

fn quoted_end(chars: &[char], start: usize) -> usize {
    let quote = chars[start];
    let mut index = start + 1;

    while index < chars.len() {
        let character = chars[index];
        if character == '\\' && quote != '`' {
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

fn is_executable_comment(chars: &[char], index: usize) -> bool {
    matches_at(chars, index, &['/', '*', '!']) || matches_at(chars, index, &['/', '*', '+'])
}

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

fn line_end(chars: &[char], start: usize) -> usize {
    chars[start..]
        .iter()
        .position(|&character| character == '\n')
        .map_or(chars.len(), |position| start + position)
}

/// Comments the server ignores. `/*!` and `/*+` carry code, so they don't count.
fn plain_comment_end(chars: &[char], index: usize) -> Option<usize> {
    if chars[index] == '#' {
        return Some(line_end(chars, index));
    }

    let is_dash_comment = matches_at(chars, index, &['-', '-'])
        && chars.get(index + 2).is_none_or(|next| next.is_whitespace());
    if is_dash_comment {
        return Some(line_end(chars, index));
    }

    if matches_at(chars, index, &['/', '*']) && !is_executable_comment(chars, index) {
        return Some(block_comment_end(chars, index));
    }

    None
}

/// The client-side `DELIMITER <token>` command, only valid where a statement would start.
fn delimiter_command(chars: &[char], index: usize) -> Option<(usize, Vec<char>)> {
    const KEYWORD: &str = "delimiter";
    let keyword_len = KEYWORD.len();

    if chars.len() < index + keyword_len {
        return None;
    }

    let word: String = chars[index..index + keyword_len].iter().collect();
    if !word.eq_ignore_ascii_case(KEYWORD) {
        return None;
    }

    let after = chars.get(index + keyword_len)?;
    if !matches!(after, ' ' | '\t') {
        return None;
    }

    let end = line_end(chars, index);
    let token: Vec<char> = chars[index + keyword_len..end]
        .iter()
        .copied()
        .skip_while(|character| character.is_whitespace())
        .take_while(|character| !character.is_whitespace())
        .collect();

    Some((end, token))
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
    fn ignores_delimiters_in_quotes_and_comments() {
        let sql = "select ';', \"a;b\", `c;d`; -- note; here\nselect 'it''s' # x;\n; /* a; b */ select 3";
        assert_eq!(
            texts(sql),
            vec!["select ';', \"a;b\", `c;d`", "select 'it''s'", "select 3"]
        );
    }

    #[test]
    fn handles_backslash_escapes() {
        assert_eq!(texts(r"select 'a\';b'; select 2"), vec![r"select 'a\';b'", "select 2"]);
    }

    #[test]
    fn supports_delimiter_command() {
        let sql = "DELIMITER $$\nCREATE PROCEDURE p() BEGIN SELECT 1; END $$\nDELIMITER ;\nSELECT 3;";
        assert_eq!(
            texts(sql),
            vec!["CREATE PROCEDURE p() BEGIN SELECT 1; END", "SELECT 3"]
        );
    }

    #[test]
    fn keeps_executable_comments() {
        assert_eq!(
            texts("/*!40101 SET NAMES utf8 */;\nselect 1"),
            vec!["/*!40101 SET NAMES utf8 */", "select 1"]
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

    #[test]
    fn double_dash_needs_whitespace() {
        assert_eq!(texts("select 1--1; select 2"), vec!["select 1--1", "select 2"]);
    }
}
