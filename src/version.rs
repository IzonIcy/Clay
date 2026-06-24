use std::cmp::Ordering;

pub fn compare_versions(left: &str, right: &str) -> Ordering {
    let left_parts = tokenize(left);
    let right_parts = tokenize(right);

    for (left, right) in left_parts.iter().zip(right_parts.iter()) {
        let ordering = match (left, right) {
            (Token::Number(left), Token::Number(right)) => compare_numeric(left, right),
            (Token::Text(left), Token::Text(right)) => {
                left.to_lowercase().cmp(&right.to_lowercase())
            }
            (Token::Number(_), Token::Text(_)) => Ordering::Greater,
            (Token::Text(_), Token::Number(_)) => Ordering::Less,
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }

    left_parts.len().cmp(&right_parts.len())
}

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Number(String),
    Text(String),
}

fn tokenize(version: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut current_is_digit = None;

    for ch in version.chars() {
        let is_digit = ch.is_ascii_digit();
        if ch == '.' || ch == '-' || ch == '_' || ch == '+' {
            push_token(&mut tokens, &mut current, current_is_digit);
            current_is_digit = None;
            continue;
        }

        match current_is_digit {
            Some(kind) if kind == is_digit => current.push(ch),
            Some(kind) => {
                push_token(&mut tokens, &mut current, Some(kind));
                current.push(ch);
                current_is_digit = Some(is_digit);
            }
            None => {
                current.push(ch);
                current_is_digit = Some(is_digit);
            }
        }
    }

    push_token(&mut tokens, &mut current, current_is_digit);
    tokens
}

fn push_token(tokens: &mut Vec<Token>, current: &mut String, is_digit: Option<bool>) {
    if current.is_empty() {
        return;
    }

    let value = std::mem::take(current);
    if is_digit.unwrap_or(false) {
        tokens.push(Token::Number(value));
    } else {
        tokens.push(Token::Text(value));
    }
}

fn compare_numeric(left: &str, right: &str) -> Ordering {
    let left = left.trim_start_matches('0');
    let right = right.trim_start_matches('0');
    let left = if left.is_empty() { "0" } else { left };
    let right = if right.is_empty() { "0" } else { right };

    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}

#[cfg(test)]
mod tests {
    use super::compare_versions;
    use std::cmp::Ordering;

    #[test]
    fn compares_numeric_segments_naturally() {
        assert_eq!(compare_versions("1.10", "1.9"), Ordering::Greater);
        assert_eq!(compare_versions("2.0", "10.0"), Ordering::Less);
        assert_eq!(compare_versions("01.002", "1.2"), Ordering::Equal);
    }

    #[test]
    fn compares_mixed_text_and_numbers() {
        assert_eq!(compare_versions("openssl@3", "openssl@11"), Ordering::Less);
        assert_eq!(compare_versions("1.0_2", "1.0_10"), Ordering::Less);
    }
}
