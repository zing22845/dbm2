//! A lightweight SQL tokenizer producing semantic tokens with depth tracking.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Word,
    QuotedIdentifier,
    String,
    Comment,
    Punctuation,
    Number,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticToken {
    pub kind: TokenKind,
    pub text: String,
    pub normalized: String,
    pub span: Span,
    pub depth: u32,
}

pub fn tokenize_sql(input: &str) -> Vec<SemanticToken> {
    let mut tokens = Vec::new();
    let mut index = 0usize;
    let mut depth = 0u32;
    let bytes = input.as_bytes();

    while index < bytes.len() {
        let start = index;
        let ch = bytes[index];
        let next = bytes.get(index + 1).copied();

        if ch.is_ascii_whitespace() {
            index += 1;
            continue;
        }

        if ch == b'-' && next == Some(b'-') {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' && bytes[index] != b'\r' {
                index += 1;
            }
            push_token(&mut tokens, TokenKind::Comment, input, start, index, depth);
            continue;
        }

        if ch == b'/' && next == Some(b'*') {
            index += 2;
            while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/') {
                index += 1;
            }
            if index + 1 < bytes.len() {
                index += 2;
            }
            push_token(&mut tokens, TokenKind::Comment, input, start, index, depth);
            continue;
        }

        if ch == b'\'' {
            index = read_quoted(bytes, index + 1, b'\'');
            push_token(&mut tokens, TokenKind::String, input, start, index, depth);
            continue;
        }

        if ch == b'"' {
            index = read_quoted(bytes, index + 1, b'"');
            push_token(
                &mut tokens,
                TokenKind::QuotedIdentifier,
                input,
                start,
                index,
                depth,
            );
            continue;
        }

        if ch.is_ascii_digit() {
            index += 1;
            while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'.') {
                index += 1;
            }
            push_token(&mut tokens, TokenKind::Number, input, start, index, depth);
            continue;
        }

        if let Some(ch) = input[index..].chars().next()
            && super::ident::is_ident_start(ch)
        {
            let start = index;
            index += ch.len_utf8();
            while let Some(next) = input[index..].chars().next() {
                if !super::ident::is_ident_part(next) {
                    break;
                }
                index += next.len_utf8();
            }
            push_token(&mut tokens, TokenKind::Word, input, start, index, depth);
            continue;
        }

        if matches!(ch, b'(' | b')' | b',' | b'.' | b';' | b'*') {
            if ch == b')' {
                depth = depth.saturating_sub(1);
            }
            push_token(
                &mut tokens,
                TokenKind::Punctuation,
                input,
                start,
                index + 1,
                depth,
            );
            if ch == b'(' {
                depth += 1;
            }
            index += 1;
            continue;
        }

        if let Some(ch) = input[index..].chars().next()
            && matches!(ch, '；' | '，' | '（' | '）')
        {
            if ch == '）' {
                depth = depth.saturating_sub(1);
            }
            let end = index + ch.len_utf8();
            push_token(
                &mut tokens,
                TokenKind::Punctuation,
                input,
                index,
                end,
                depth,
            );
            if ch == '（' {
                depth += 1;
            }
            index = end;
            continue;
        }

        if let Some(ch) = input[index..].chars().next() {
            index += ch.len_utf8();
        } else {
            index += 1;
        }
    }

    tokens
}

pub fn is_suppressed_at_cursor(tokens: &[SemanticToken], cursor: usize) -> bool {
    for token in tokens {
        if token.span.start >= cursor {
            break;
        }
        if token.span.end <= cursor {
            continue;
        }
        return matches!(token.kind, TokenKind::String | TokenKind::Comment);
    }
    false
}

pub fn significant_tokens(tokens: &[SemanticToken]) -> Vec<&SemanticToken> {
    tokens
        .iter()
        .filter(|t| !matches!(t.kind, TokenKind::Comment))
        .collect()
}

fn push_token(
    tokens: &mut Vec<SemanticToken>,
    kind: TokenKind,
    input: &str,
    start: usize,
    end: usize,
    depth: u32,
) {
    let text = input[start..end].to_string();
    let normalized = if kind == TokenKind::Word {
        text.to_ascii_lowercase()
    } else {
        text.clone()
    };
    tokens.push(SemanticToken {
        kind,
        text,
        normalized,
        span: Span { start, end },
        depth,
    });
}

fn read_quoted(bytes: &[u8], mut index: usize, quote: u8) -> usize {
    while index < bytes.len() {
        if bytes[index] == quote {
            if bytes.get(index + 1) == Some(&quote) {
                index += 2;
                continue;
            }
            return index + 1;
        }
        index += 1;
    }
    bytes.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_words_and_punctuation() {
        let tokens = tokenize_sql("SELECT * FROM users");
        let words: Vec<_> = tokens
            .iter()
            .filter(|t| t.kind == TokenKind::Word)
            .map(|t| t.normalized.as_str())
            .collect();
        assert_eq!(words, vec!["select", "from", "users"]);
    }

    #[test]
    fn skips_whitespace() {
        let tokens = tokenize_sql("SELECT  \n  1");
        assert!(
            tokens
                .iter()
                .any(|t| t.kind == TokenKind::Word && t.text == "SELECT")
        );
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Number));
    }

    #[test]
    fn tokenizes_unicode_identifiers() {
        let tokens = tokenize_sql("SELECT * FROM 测试");
        let words: Vec<_> = tokens
            .iter()
            .filter(|t| t.kind == TokenKind::Word)
            .map(|t| t.text.as_str())
            .collect();
        assert_eq!(words, vec!["SELECT", "FROM", "测试"]);
    }

    #[test]
    fn tokenizes_fullwidth_semicolon() {
        let tokens = tokenize_sql("select ；");
        assert!(tokens.iter().any(|t| t.text == "；"));
    }
}
