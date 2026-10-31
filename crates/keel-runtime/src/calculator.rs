//! Small arithmetic evaluator for deterministic compute verification.
//!
//! It supports `+`, `-`, `*`, `/`, `%`, parentheses, unary signs, integers and
//! decimals. It deliberately has no variables, functions or exponent syntax.

const MAX_EXPRESSION_CHARS: usize = 256;

/// Evaluates a bounded arithmetic expression.
pub fn evaluate(expression: &str) -> Result<f64, String> {
    if expression.chars().count() > MAX_EXPRESSION_CHARS {
        return Err(format!(
            "expression exceeds {MAX_EXPRESSION_CHARS} characters"
        ));
    }

    let mut parser = Parser {
        chars: expression.chars().collect(),
        index: 0,
    };
    let value = parser.expression()?;
    parser.skip_whitespace();
    if parser.index != parser.chars.len() {
        return Err(format!(
            "unexpected character `{}` at position {}",
            parser.chars[parser.index], parser.index
        ));
    }
    if !value.is_finite() {
        return Err("result is not finite".to_owned());
    }
    Ok(value)
}

struct Parser {
    chars: Vec<char>,
    index: usize,
}

impl Parser {
    fn expression(&mut self) -> Result<f64, String> {
        let mut value = self.term()?;
        loop {
            self.skip_whitespace();
            match self.peek() {
                Some('+') => {
                    self.index += 1;
                    value += self.term()?;
                }
                Some('-') => {
                    self.index += 1;
                    value -= self.term()?;
                }
                _ => return Ok(value),
            }
        }
    }

    fn term(&mut self) -> Result<f64, String> {
        let mut value = self.factor()?;
        loop {
            self.skip_whitespace();
            match self.peek() {
                Some('*') => {
                    self.index += 1;
                    value *= self.factor()?;
                }
                Some('/') => {
                    self.index += 1;
                    let divisor = self.factor()?;
                    if divisor == 0.0 {
                        return Err("division by zero".to_owned());
                    }
                    value /= divisor;
                }
                Some('%') => {
                    self.index += 1;
                    let divisor = self.factor()?;
                    if divisor == 0.0 {
                        return Err("division by zero".to_owned());
                    }
                    value %= divisor;
                }
                _ => return Ok(value),
            }
        }
    }

    fn factor(&mut self) -> Result<f64, String> {
        self.skip_whitespace();
        match self.peek() {
            Some('+') => {
                self.index += 1;
                self.factor()
            }
            Some('-') => {
                self.index += 1;
                Ok(-self.factor()?)
            }
            Some('(') => {
                self.index += 1;
                let value = self.expression()?;
                self.skip_whitespace();
                if self.peek() != Some(')') {
                    return Err("missing `)`".to_owned());
                }
                self.index += 1;
                Ok(value)
            }
            Some(character) if character.is_ascii_digit() || character == '.' => self.number(),
            Some(character) => Err(format!(
                "unexpected character `{character}` at position {}",
                self.index
            )),
            None => Err("unexpected end of expression".to_owned()),
        }
    }

    fn number(&mut self) -> Result<f64, String> {
        let start = self.index;
        let mut seen_dot = false;
        while let Some(character) = self.peek() {
            if character.is_ascii_digit() {
                self.index += 1;
            } else if character == '.' && !seen_dot {
                seen_dot = true;
                self.index += 1;
            } else {
                break;
            }
        }
        let text: String = self.chars[start..self.index].iter().collect();
        text.parse::<f64>()
            .map_err(|_| format!("invalid number `{text}`"))
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.index += 1;
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.index).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precedence_and_parentheses_work() {
        assert_eq!(evaluate("2 + 3 * 4").unwrap(), 14.0);
        assert_eq!(evaluate("(2 + 3) * 4").unwrap(), 20.0);
        assert_eq!(evaluate("10 % 4").unwrap(), 2.0);
        assert_eq!(evaluate("-2 * (3 + 1)").unwrap(), -8.0);
        assert_eq!(evaluate("1.5 + 2.25").unwrap(), 3.75);
    }

    #[test]
    fn invalid_expressions_are_rejected() {
        assert!(evaluate("1 / 0").is_err());
        assert!(evaluate("1 +").is_err());
        assert!(evaluate("(1 + 2").is_err());
        assert!(evaluate("2 ** 3").is_err());
        assert!(evaluate("abc").is_err());
        assert!(evaluate(&"1+".repeat(200)).is_err());
    }
}
