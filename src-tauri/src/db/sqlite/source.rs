use super::splitter::{closing_quote, comment_end, is_identifier_char, is_word_start, quoted_end, word_end};

#[derive(Debug, PartialEq)]
enum Token {
    Word(String),
    Quoted(String),
    Symbol(char),
    Literal,
}

const CLAUSES: [&str; 5] = ["WHERE", "GROUP", "HAVING", "ORDER", "LIMIT"];

const RESERVED: [&str; 21] = [
    "WHERE", "GROUP", "HAVING", "WINDOW", "ORDER", "LIMIT", "JOIN", "LEFT", "RIGHT", "FULL", "INNER", "CROSS",
    "NATURAL", "OUTER", "UNION", "INTERSECT", "EXCEPT", "INDEXED", "NOT", "ON", "USING",
];

const COMPOUND: [&str; 3] = ["UNION", "INTERSECT", "EXCEPT"];

fn tokens(sql: &str) -> Vec<Token> {
    let chars: Vec<char> = sql.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;

    while index < chars.len() {
        let character = chars[index];
        if character.is_whitespace() {
            index += 1;
        } else if let Some(end) = comment_end(&chars, index) {
            index = end;
        } else if character == '\'' {
            index = quoted_end(&chars, index, '\'');
            tokens.push(Token::Literal);
        } else if let Some(close) = closing_quote(character) {
            let end = quoted_end(&chars, index, close);
            let inner_end = if end > index + 1 && chars[end - 1] == close { end - 1 } else { end };
            let inner: String = chars[index + 1..inner_end].iter().collect();
            let name = if close == ']' { inner } else { inner.replace(&format!("{close}{close}"), &close.to_string()) };
            tokens.push(Token::Quoted(name));
            index = end;
        } else if is_word_start(character) {
            let end = word_end(&chars, index);
            tokens.push(Token::Word(chars[index..end].iter().collect()));
            index = end;
        } else if character.is_ascii_digit() {
            while index < chars.len() && (is_identifier_char(chars[index]) || chars[index] == '.') {
                index += 1;
            }
            tokens.push(Token::Literal);
        } else {
            tokens.push(Token::Symbol(character));
            index += 1;
        }
    }

    tokens
}

fn name(token: &Token) -> Option<String> {
    match token {
        Token::Word(word) | Token::Quoted(word) => Some(word.clone()),
        _ => None,
    }
}

fn is_keyword(token: &Token, keyword: &str) -> bool {
    matches!(token, Token::Word(word) if word.eq_ignore_ascii_case(keyword))
}

fn is_any(token: &Token, keywords: &[&str]) -> bool {
    keywords.iter().any(|keyword| is_keyword(token, keyword))
}

fn after_keyword<'a>(tokens: &'a [Token], keyword: &str) -> Option<&'a [Token]> {
    match tokens {
        [first, rest @ ..] if is_keyword(first, keyword) => Some(rest),
        _ => None,
    }
}

/// The table a plain single-table SELECT reads, when every result column is `*` or a bare column name.
pub fn single_table(sql: &str) -> Option<(String, String)> {
    let tokens = tokens(sql);
    let mut rest = after_keyword(&tokens, "SELECT")?;
    rest = after_keyword(rest, "DISTINCT").or_else(|| after_keyword(rest, "ALL")).unwrap_or(rest);

    let mut qualifiers = Vec::new();
    loop {
        rest = match rest {
            [Token::Symbol('*'), tail @ ..] => tail,
            [qualifier, Token::Symbol('.'), Token::Symbol('*'), tail @ ..] => {
                qualifiers.push(name(qualifier)?);
                tail
            }
            [qualifier, Token::Symbol('.'), column, tail @ ..] => {
                qualifiers.push(name(qualifier)?);
                name(column)?;
                tail
            }
            [column, tail @ ..] if !is_keyword(column, "FROM") => {
                name(column)?;
                tail
            }
            _ => return None,
        };
        match rest {
            [Token::Symbol(','), tail @ ..] => rest = tail,
            _ => break,
        }
    }

    rest = after_keyword(rest, "FROM")?;
    let (schema, table) = match rest {
        [schema, Token::Symbol('.'), table, tail @ ..] => {
            rest = tail;
            (name(schema)?, name(table)?)
        }
        [table, tail @ ..] => {
            rest = tail;
            ("main".to_string(), name(table)?)
        }
        [] => return None,
    };

    let mut alias = None;
    if let Some(tail) = after_keyword(rest, "AS") {
        let (first, tail) = tail.split_first()?;
        alias = Some(name(first)?);
        rest = tail;
    } else if let [first, tail @ ..] = rest {
        if !is_any(first, &RESERVED) {
            if let Some(found) = name(first) {
                alias = Some(found);
                rest = tail;
            }
        }
    }

    if rest.first().is_some_and(|token| !is_any(token, &CLAUSES)) {
        return None;
    }

    let mut depth = 0usize;
    for token in rest {
        match token {
            Token::Symbol('(') => depth += 1,
            Token::Symbol(')') => depth = depth.saturating_sub(1),
            token if depth == 0 && is_any(token, &COMPOUND) => return None,
            _ => {}
        }
    }

    let known = |qualifier: &String| {
        qualifier.eq_ignore_ascii_case(&table) || alias.as_ref().is_some_and(|alias| qualifier.eq_ignore_ascii_case(alias))
    };
    if !qualifiers.iter().all(known) {
        return None;
    }

    Some((schema, table))
}

#[cfg(test)]
mod tests {
    use super::single_table;

    fn in_main(table: &str) -> Option<(String, String)> {
        Some(("main".to_string(), table.to_string()))
    }

    #[test]
    fn finds_plain_selects() {
        assert_eq!(single_table("SELECT * FROM users"), in_main("users"));
        assert_eq!(single_table("select id, email from users where id > 3 order by id limit 10"), in_main("users"));
        assert_eq!(single_table("SELECT DISTINCT * FROM \"my \"\"table\"\"\""), in_main("my \"table\""));
        assert_eq!(single_table("SELECT [a b], `c` FROM [t]"), in_main("t"));
        assert_eq!(single_table("SELECT u.id, u.* FROM users AS u"), in_main("users"));
        assert_eq!(
            single_table("SELECT users.id FROM users u WHERE u.id IN (SELECT 1 UNION SELECT 2)"),
            in_main("users")
        );
        assert_eq!(
            single_table("SELECT * FROM other.\"t\" -- note\n WHERE name = 'x FROM y'"),
            Some(("other".to_string(), "t".to_string()))
        );
    }

    #[test]
    fn refuses_anything_else() {
        for sql in [
            "SELECT id AS key FROM users",
            "SELECT id key FROM users",
            "SELECT count(*) FROM users",
            "SELECT 1 FROM users",
            "SELECT * FROM users JOIN posts ON 1",
            "SELECT * FROM users u LEFT JOIN posts p ON 1",
            "SELECT * FROM users, posts",
            "SELECT * FROM (SELECT * FROM users)",
            "SELECT * FROM pragma_table_info('users')",
            "SELECT * FROM a UNION SELECT * FROM b",
            "SELECT p.id FROM users u",
            "WITH x AS (SELECT 1) SELECT * FROM x",
            "UPDATE users SET id = 1",
            "SELECT 1",
        ] {
            assert_eq!(single_table(sql), None, "{sql}");
        }
    }
}
