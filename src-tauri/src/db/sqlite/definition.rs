#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Word(String),
    Identifier(String),
    Text,
    Symbol(char),
}

#[derive(Debug, Clone)]
struct Spanned {
    token: Token,
    start: usize,
    end: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColumnClauses {
    pub name: String,
    pub collation: Option<String>,
    pub generated: Option<String>,
    pub autoincrement: bool,
}

fn tokenize(sql: &str) -> Vec<Spanned> {
    let chars: Vec<(usize, char)> = sql.char_indices().collect();
    let byte_at = |index: usize| chars.get(index).map_or(sql.len(), |(byte, _)| *byte);
    let mut tokens = Vec::new();
    let mut index = 0;

    while index < chars.len() {
        let (_, character) = chars[index];
        let next = chars.get(index + 1).map(|(_, next)| *next);
        let start = index;

        if character.is_whitespace() {
            index += 1;
            continue;
        }
        if character == '-' && next == Some('-') {
            while index < chars.len() && chars[index].1 != '\n' {
                index += 1;
            }
            continue;
        }
        if character == '/' && next == Some('*') {
            index += 2;
            while index + 1 < chars.len() && !(chars[index].1 == '*' && chars[index + 1].1 == '/') {
                index += 1;
            }
            index = (index + 2).min(chars.len());
            continue;
        }

        let token = match character {
            '\'' | '"' | '`' | '[' => {
                let close = if character == '[' { ']' } else { character };
                let mut value = String::new();
                index += 1;
                while index < chars.len() {
                    let current = chars[index].1;
                    if current == close {
                        if close != ']' && chars.get(index + 1).map(|(_, next)| *next) == Some(close) {
                            value.push(close);
                            index += 2;
                            continue;
                        }
                        index += 1;
                        break;
                    }
                    value.push(current);
                    index += 1;
                }
                if character == '\'' { Token::Text } else { Token::Identifier(value) }
            }
            _ if character.is_alphanumeric() || character == '_' || character == '$' => {
                let mut word = String::new();
                while index < chars.len() && (chars[index].1.is_alphanumeric() || matches!(chars[index].1, '_' | '$')) {
                    word.push(chars[index].1);
                    index += 1;
                }
                Token::Word(word)
            }
            _ => {
                index += 1;
                Token::Symbol(character)
            }
        };
        tokens.push(Spanned {
            token,
            start: byte_at(start),
            end: byte_at(index),
        });
    }

    tokens
}

fn is_word(token: &Token, expected: &str) -> bool {
    matches!(token, Token::Word(word) if word.eq_ignore_ascii_case(expected))
}

fn name_of(token: &Token) -> Option<String> {
    match token {
        Token::Word(word) | Token::Identifier(word) => Some(word.clone()),
        _ => None,
    }
}

/// The index just past the parenthesis that closes the one at `open`.
fn closing(tokens: &[Spanned], open: usize) -> usize {
    let mut depth = 0usize;
    for (index, spanned) in tokens.iter().enumerate().skip(open) {
        match spanned.token {
            Token::Symbol('(') => depth += 1,
            Token::Symbol(')') => {
                depth -= 1;
                if depth == 0 {
                    return index + 1;
                }
            }
            _ => {}
        }
    }
    tokens.len()
}

fn body_items(tokens: &[Spanned]) -> Vec<&[Spanned]> {
    let Some(open) = tokens.iter().position(|spanned| {
        spanned.token == Token::Symbol('(') || is_word(&spanned.token, "AS")
    }) else {
        return Vec::new();
    };
    if tokens[open].token != Token::Symbol('(') {
        return Vec::new();
    }
    let close = closing(tokens, open).saturating_sub(1);

    let mut items = Vec::new();
    let mut depth = 0usize;
    let mut item_start = open + 1;
    for index in open + 1..close {
        match tokens[index].token {
            Token::Symbol('(') => depth += 1,
            Token::Symbol(')') => depth = depth.saturating_sub(1),
            Token::Symbol(',') if depth == 0 => {
                items.push(&tokens[item_start..index]);
                item_start = index + 1;
            }
            _ => {}
        }
    }
    items.push(&tokens[item_start..close]);
    items.retain(|item| !item.is_empty());
    items
}

const TABLE_CONSTRAINTS: [&str; 5] = ["CONSTRAINT", "PRIMARY", "UNIQUE", "CHECK", "FOREIGN"];

pub fn column_clauses(create_sql: &str) -> Vec<ColumnClauses> {
    let tokens = tokenize(create_sql);
    let mut columns = Vec::new();

    for item in body_items(&tokens) {
        let first = &item[0].token;
        if matches!(first, Token::Word(_)) && TABLE_CONSTRAINTS.iter().any(|keyword| is_word(first, keyword)) {
            continue;
        }
        let Some(name) = name_of(first) else {
            continue;
        };

        let mut clauses = ColumnClauses {
            name,
            ..Default::default()
        };
        for (index, spanned) in item.iter().enumerate() {
            if is_word(&spanned.token, "COLLATE") {
                clauses.collation = item.get(index + 1).and_then(|next| name_of(&next.token));
            } else if is_word(&spanned.token, "AUTOINCREMENT") {
                clauses.autoincrement = true;
            } else if is_word(&spanned.token, "AS")
                && let Some(open) = item.get(index + 1).filter(|next| next.token == Token::Symbol('('))
            {
                let close = closing(item, index + 1);
                if close > index + 2 {
                    let last = &item[close - 1];
                    clauses.generated = Some(create_sql[open.end..last.start].trim().to_string());
                }
            }
        }
        columns.push(clauses);
    }

    columns
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_column_clauses() {
        let sql = r#"CREATE TABLE "my table" (
            id INTEGER PRIMARY KEY AUTOINCREMENT, -- the key, (really)
            [first name] TEXT COLLATE NOCASE NOT NULL DEFAULT 'a,b',
            total REAL GENERATED ALWAYS AS (price * (1 + tax)) STORED,
            price REAL, tax REAL AS (0.2),
            CONSTRAINT pk UNIQUE (price, tax),
            FOREIGN KEY (id) REFERENCES other(id)
        )"#;
        let columns = column_clauses(sql);
        assert_eq!(columns.len(), 5);
        assert!(columns[0].autoincrement);
        assert_eq!(columns[1].name, "first name");
        assert_eq!(columns[1].collation.as_deref(), Some("NOCASE"));
        assert_eq!(columns[2].generated.as_deref(), Some("price * (1 + tax)"));
        assert_eq!(columns[3].name, "price");
        assert_eq!(columns[4].generated.as_deref(), Some("0.2"));
    }

    #[test]
    fn ignores_create_as_select() {
        assert!(column_clauses("CREATE TABLE t AS SELECT (1) AS a").is_empty());
    }
}
