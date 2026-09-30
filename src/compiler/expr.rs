//! Expressions - the small language used in conditions, assignments and
//! `{...}` placeholders in text.
//!
//! ```text
//! affection >= 5 and not met_rival
//! score + 10 * bonus
//! "Hello, " + name
//! ```
//!
//! Literals: numbers, "strings" or 'strings', true, false, null.
//! Variables: identifiers, optionally dotted (`persistent.endings`).
//! Operators by increasing precedence:
//! `or`, `and`, `not`, comparisons (`== != < <= > >=`), `+ -`, `* / %`, unary `-`.
//! `&&`, `||` and `!` are accepted as aliases. `and`/`or` short-circuit and,
//! like Python, return one of their operands.

use serde_json::Value as JsonValue;

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Literal(JsonValue),
    Var(String),
    Unary(UnaryOp, Box<Expr>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl BinaryOp {
    /// Operator as written in expressions
    pub fn symbol(self) -> &'static str {
        match self {
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Mod => "%",
            BinaryOp::Eq => "==",
            BinaryOp::Ne => "!=",
            BinaryOp::Lt => "<",
            BinaryOp::Le => "<=",
            BinaryOp::Gt => ">",
            BinaryOp::Ge => ">=",
        }
    }
}

impl Expr {
    /// Names of all variables the expression reads
    pub fn variables(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.collect_variables(&mut out);
        out
    }

    fn collect_variables(&self, out: &mut Vec<String>) {
        match self {
            Expr::Literal(_) => {}
            Expr::Var(name) => {
                if !out.contains(name) {
                    out.push(name.clone());
                }
            }
            Expr::Unary(_, e) => e.collect_variables(out),
            Expr::Binary(_, a, b) | Expr::And(a, b) | Expr::Or(a, b) => {
                a.collect_variables(out);
                b.collect_variables(out);
            }
        }
    }
}

/// A piece of interpolated text
#[derive(Debug, Clone, PartialEq)]
pub enum TemplatePart {
    Text(String),
    Expr(Expr),
}

// --- tokenizer -----------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(JsonValue),
    Str(String),
    Ident(String),
    Op(&'static str),
    LParen,
    RParen,
}

const OPERATORS: [&str; 16] = [
    "==", "!=", "<=", ">=", "&&", "||", "<", ">", "+", "-", "*", "/", "%", "!", "=", "&",
];

fn tokenize(src: &str) -> Result<Vec<(Token, usize)>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        let start = i;
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit()
            || (c == '.' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit()))
        {
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            let value = if text.contains('.') {
                text.parse::<f64>()
                    .ok()
                    .and_then(serde_json::Number::from_f64)
                    .map(JsonValue::Number)
            } else {
                text.parse::<i64>().ok().map(JsonValue::from)
            };
            let value = value.ok_or_else(|| format!("invalid number '{}' at {}", text, start))?;
            tokens.push((Token::Number(value), start));
        } else if c == '"' || c == '\'' {
            let quote = c;
            let mut text = String::new();
            i += 1;
            loop {
                match chars.get(i) {
                    None => return Err(format!("unterminated string starting at {}", start)),
                    Some(&ch) if ch == quote => {
                        i += 1;
                        break;
                    }
                    Some('\\') => {
                        let escaped = match chars.get(i + 1) {
                            Some('n') => '\n',
                            Some('t') => '\t',
                            Some(&other) => other,
                            None => return Err(format!("unterminated string starting at {}", start)),
                        };
                        text.push(escaped);
                        i += 2;
                    }
                    Some(&ch) => {
                        text.push(ch);
                        i += 1;
                    }
                }
            }
            tokens.push((Token::Str(text), start));
        } else if c.is_alphabetic() || c == '_' {
            while i < chars.len()
                && (chars[i].is_alphanumeric()
                    || chars[i] == '_'
                    || (chars[i] == '.'
                        && chars.get(i + 1).is_some_and(|n| n.is_alphabetic() || *n == '_')))
            {
                i += 1;
            }
            tokens.push((Token::Ident(chars[start..i].iter().collect()), start));
        } else if c == '(' {
            tokens.push((Token::LParen, start));
            i += 1;
        } else if c == ')' {
            tokens.push((Token::RParen, start));
            i += 1;
        } else {
            let rest: String = chars[i..chars.len().min(i + 2)].iter().collect();
            let op = OPERATORS
                .iter()
                .find(|op| rest.starts_with(**op))
                .ok_or_else(|| format!("unexpected character '{}' at {}", c, start))?;
            match *op {
                "=" => return Err(format!("use '==' to compare (at {})", start)),
                "&" => return Err(format!("use 'and' or '&&' (at {})", start)),
                _ => {}
            }
            tokens.push((Token::Op(op), start));
            i += op.len();
        }
    }
    Ok(tokens)
}

// --- parser --------------------------------------------------------------

struct Parser {
    tokens: Vec<(Token, usize)>,
    pos: usize,
    len: usize,
}

const PREC_OR: u8 = 1;
const PREC_AND: u8 = 2;
const PREC_NOT: u8 = 3;
const PREC_CMP: u8 = 4;
const PREC_ADD: u8 = 5;
const PREC_MUL: u8 = 6;
const PREC_UNARY: u8 = 7;

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos).map(|(t, _)| t)
    }

    fn position(&self) -> usize {
        self.tokens.get(self.pos).map_or(self.len, |(_, p)| *p)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos).map(|(t, _)| t.clone());
        self.pos += 1;
        token
    }

    /// Binary operator at the cursor with its precedence
    fn infix(&self) -> Option<(&'static str, u8)> {
        match self.peek()? {
            Token::Ident(w) if w == "or" => Some(("or", PREC_OR)),
            Token::Ident(w) if w == "and" => Some(("and", PREC_AND)),
            Token::Op("||") => Some(("or", PREC_OR)),
            Token::Op("&&") => Some(("and", PREC_AND)),
            Token::Op(op) if matches!(*op, "==" | "!=" | "<" | "<=" | ">" | ">=") => {
                Some((*op, PREC_CMP))
            }
            Token::Op(op) if matches!(*op, "+" | "-") => Some((*op, PREC_ADD)),
            Token::Op(op) if matches!(*op, "*" | "/" | "%") => Some((*op, PREC_MUL)),
            _ => None,
        }
    }

    fn expression(&mut self, min_prec: u8) -> Result<Expr, String> {
        let mut left = self.prefix()?;
        while let Some((op, prec)) = self.infix() {
            if prec < min_prec {
                break;
            }
            self.pos += 1;
            // Left-associative: the right side binds tighter
            let right = self.expression(prec + 1)?;
            left = match op {
                "or" => Expr::Or(Box::new(left), Box::new(right)),
                "and" => Expr::And(Box::new(left), Box::new(right)),
                _ => Expr::Binary(binary_op(op), Box::new(left), Box::new(right)),
            };
        }
        Ok(left)
    }

    fn prefix(&mut self) -> Result<Expr, String> {
        let at = self.position();
        match self.next() {
            Some(Token::Number(n)) => Ok(Expr::Literal(n)),
            Some(Token::Str(s)) => Ok(Expr::Literal(JsonValue::String(s))),
            Some(Token::Ident(word)) => match word.as_str() {
                "true" => Ok(Expr::Literal(JsonValue::Bool(true))),
                "false" => Ok(Expr::Literal(JsonValue::Bool(false))),
                "null" => Ok(Expr::Literal(JsonValue::Null)),
                "not" => Ok(Expr::Unary(UnaryOp::Not, Box::new(self.expression(PREC_NOT)?))),
                "and" | "or" => Err(format!("unexpected '{}' at {}", word, at)),
                _ => Ok(Expr::Var(word)),
            },
            Some(Token::Op("!")) => {
                Ok(Expr::Unary(UnaryOp::Not, Box::new(self.expression(PREC_NOT)?)))
            }
            Some(Token::Op("-")) => {
                Ok(Expr::Unary(UnaryOp::Neg, Box::new(self.expression(PREC_UNARY)?)))
            }
            Some(Token::LParen) => {
                let inner = self.expression(0)?;
                match self.next() {
                    Some(Token::RParen) => Ok(inner),
                    _ => Err(format!("missing ')' for '(' at {}", at)),
                }
            }
            Some(Token::RParen) => Err(format!("unexpected ')' at {}", at)),
            Some(Token::Op(op)) => Err(format!("unexpected '{}' at {}", op, at)),
            None => Err("unexpected end of expression".to_string()),
        }
    }
}

fn binary_op(op: &str) -> BinaryOp {
    match op {
        "+" => BinaryOp::Add,
        "-" => BinaryOp::Sub,
        "*" => BinaryOp::Mul,
        "/" => BinaryOp::Div,
        "%" => BinaryOp::Mod,
        "==" => BinaryOp::Eq,
        "!=" => BinaryOp::Ne,
        "<" => BinaryOp::Lt,
        "<=" => BinaryOp::Le,
        ">" => BinaryOp::Gt,
        _ => BinaryOp::Ge,
    }
}

/// Parse an expression
pub fn parse_expr(src: &str) -> Result<Expr, String> {
    let tokens = tokenize(src)?;
    if tokens.is_empty() {
        return Err("empty expression".to_string());
    }
    let mut parser = Parser { tokens, pos: 0, len: src.chars().count() };
    let expr = parser.expression(0)?;
    if parser.pos < parser.tokens.len() {
        return Err(format!("unexpected input at {}", parser.position()));
    }
    Ok(expr)
}

/// Split text with `{expression}` placeholders. `{{` and `}}` are literal braces.
pub fn parse_template(src: &str) -> Result<Vec<TemplatePart>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut parts = Vec::new();
    let mut text = String::new();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            '{' if chars.get(i + 1) == Some(&'{') => {
                text.push('{');
                i += 2;
            }
            '}' if chars.get(i + 1) == Some(&'}') => {
                text.push('}');
                i += 2;
            }
            '{' => {
                // Find the closing brace, skipping braces inside string literals
                let start = i + 1;
                let mut j = start;
                let mut quote: Option<char> = None;
                while j < chars.len() {
                    match (quote, chars[j]) {
                        (Some(_), '\\') => j += 1,
                        (Some(q), c) if c == q => quote = None,
                        (None, '"' | '\'') => quote = Some(chars[j]),
                        (None, '}') => break,
                        _ => {}
                    }
                    j += 1;
                }
                if j >= chars.len() {
                    return Err(format!("unclosed '{{' at {}", i));
                }
                let source: String = chars[start..j].iter().collect();
                let expr = parse_expr(&source).map_err(|e| format!("in {{{}}}: {}", source, e))?;
                if !text.is_empty() {
                    parts.push(TemplatePart::Text(std::mem::take(&mut text)));
                }
                parts.push(TemplatePart::Expr(expr));
                i = j + 1;
            }
            c => {
                text.push(c);
                i += 1;
            }
        }
    }
    if !text.is_empty() || parts.is_empty() {
        parts.push(TemplatePart::Text(text));
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn var(name: &str) -> Box<Expr> {
        Box::new(Expr::Var(name.into()))
    }

    fn lit(v: JsonValue) -> Box<Expr> {
        Box::new(Expr::Literal(v))
    }

    #[test]
    fn precedence() {
        assert_eq!(
            parse_expr("a + 2 * b").unwrap(),
            Expr::Binary(
                BinaryOp::Add,
                var("a"),
                Box::new(Expr::Binary(BinaryOp::Mul, lit(json!(2)), var("b")))
            )
        );
        assert_eq!(
            parse_expr("not x == 1 and y").unwrap(),
            Expr::And(
                Box::new(Expr::Unary(
                    UnaryOp::Not,
                    Box::new(Expr::Binary(BinaryOp::Eq, var("x"), lit(json!(1))))
                )),
                var("y")
            )
        );
        assert_eq!(
            parse_expr("a - b - c").unwrap(),
            Expr::Binary(
                BinaryOp::Sub,
                Box::new(Expr::Binary(BinaryOp::Sub, var("a"), var("b"))),
                var("c")
            )
        );
    }

    #[test]
    fn literals_and_aliases() {
        assert_eq!(parse_expr("1.5").unwrap(), Expr::Literal(json!(1.5)));
        assert_eq!(parse_expr("'it\\'s'").unwrap(), Expr::Literal(json!("it's")));
        assert_eq!(parse_expr("a && !b").unwrap(), parse_expr("a and not b").unwrap());
        assert_eq!(parse_expr("persistent.ending").unwrap(), Expr::Var("persistent.ending".into()));
        assert_eq!(parse_expr("-(x)").unwrap(), Expr::Unary(UnaryOp::Neg, var("x")));
    }

    #[test]
    fn errors_point_at_the_problem() {
        assert!(parse_expr("a = 1").unwrap_err().contains("'=='"));
        assert!(parse_expr("(a + 1").unwrap_err().contains("missing ')'"));
        assert!(parse_expr("a +").unwrap_err().contains("end of expression"));
        assert!(parse_expr("a b").unwrap_err().contains("unexpected input at 2"));
        assert!(parse_expr("").is_err());
        assert!(parse_expr("\"open").unwrap_err().contains("unterminated"));
    }

    #[test]
    fn variables_are_collected_once() {
        assert_eq!(parse_expr("a + b * a").unwrap().variables(), vec!["a", "b"]);
    }

    #[test]
    fn templates() {
        assert_eq!(parse_template("plain").unwrap(), vec![TemplatePart::Text("plain".into())]);
        assert_eq!(parse_template("").unwrap(), vec![TemplatePart::Text("".into())]);
        assert_eq!(
            parse_template("Hi, {name}! {{x}}").unwrap(),
            vec![
                TemplatePart::Text("Hi, ".into()),
                TemplatePart::Expr(Expr::Var("name".into())),
                TemplatePart::Text("! {x}".into()),
            ]
        );
        assert_eq!(
            parse_template("{'}'}").unwrap(),
            vec![TemplatePart::Expr(Expr::Literal(json!("}")))]
        );
        assert!(parse_template("{oops").unwrap_err().contains("unclosed"));
        assert!(parse_template("{1 +}").unwrap_err().contains("in {1 +}"));
    }
}
