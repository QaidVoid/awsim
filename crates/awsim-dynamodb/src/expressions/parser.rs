use std::collections::HashMap;

use awsim_core::AwsError;

/// A parsed condition/filter expression node.
#[derive(Debug, Clone)]
pub enum ConditionExpr {
    /// attribute = :val
    Comparison {
        left: Operand,
        op: CompareOp,
        right: Operand,
    },
    /// BETWEEN x AND y
    Between {
        operand: Operand,
        low: Operand,
        high: Operand,
    },
    /// IN (a, b, c)
    In {
        operand: Operand,
        values: Vec<Operand>,
    },
    /// AND / OR
    Logical {
        op: LogicalOp,
        children: Vec<ConditionExpr>,
    },
    /// NOT
    Not(Box<ConditionExpr>),
    /// attribute_exists(path)
    AttributeExists(String),
    /// attribute_not_exists(path)
    AttributeNotExists(String),
    /// attribute_type(path, :val)
    AttributeType(String, Operand),
    /// begins_with(path, :val)
    BeginsWith(Operand, Operand),
    /// contains(path, :val)
    Contains(Operand, Operand),
    /// size(path) <op> :val
    SizeComparison {
        path: String,
        op: CompareOp,
        right: Operand,
    },
}

#[derive(Debug, Clone)]
pub enum Operand {
    Path(String),
    Value(String), // placeholder name, e.g. ":val"
}

#[derive(Debug, Clone, PartialEq)]
pub enum CompareOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone)]
pub enum LogicalOp {
    And,
    Or,
}

// ─── Tokenizer ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Word(String),
    Colon,
    Hash,
    Dot,
    LParen,
    RParen,
    Comma,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Eof,
}

struct Lexer {
    input: Vec<char>,
    pos: usize,
}

impl Lexer {
    fn new(s: &str) -> Self {
        Self {
            input: s.chars().collect(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.input.get(self.pos).copied();
        self.pos += 1;
        c
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(|c| c.is_whitespace()) {
            self.advance();
        }
    }

    fn tokenize(&mut self) -> Vec<Token> {
        let mut tokens = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                None => {
                    tokens.push(Token::Eof);
                    break;
                }
                Some(c) => match c {
                    ':' => {
                        self.advance();
                        tokens.push(Token::Colon);
                    }
                    '#' => {
                        self.advance();
                        tokens.push(Token::Hash);
                    }
                    '.' => {
                        self.advance();
                        tokens.push(Token::Dot);
                    }
                    '(' => {
                        self.advance();
                        tokens.push(Token::LParen);
                    }
                    ')' => {
                        self.advance();
                        tokens.push(Token::RParen);
                    }
                    ',' => {
                        self.advance();
                        tokens.push(Token::Comma);
                    }
                    '=' => {
                        self.advance();
                        tokens.push(Token::Eq);
                    }
                    '<' => {
                        self.advance();
                        if self.peek() == Some('>') {
                            self.advance();
                            tokens.push(Token::Ne);
                        } else if self.peek() == Some('=') {
                            self.advance();
                            tokens.push(Token::Le);
                        } else {
                            tokens.push(Token::Lt);
                        }
                    }
                    '>' => {
                        self.advance();
                        if self.peek() == Some('=') {
                            self.advance();
                            tokens.push(Token::Ge);
                        } else {
                            tokens.push(Token::Gt);
                        }
                    }
                    _ if c.is_alphanumeric() || c == '_' => {
                        let mut word = String::new();
                        while self
                            .peek()
                            .is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
                        {
                            word.push(self.advance().unwrap());
                        }
                        tokens.push(Token::Word(word));
                    }
                    _ => {
                        self.advance();
                    }
                },
            }
        }
        tokens
    }
}

// ─── Parser ──────────────────────────────────────────────────────────────────

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) -> &Token {
        let t = self.tokens.get(self.pos).unwrap_or(&Token::Eof);
        self.pos += 1;
        t
    }

    fn expect_word(&mut self) -> Result<String, AwsError> {
        match self.advance().clone() {
            Token::Word(w) => Ok(w),
            other => Err(AwsError::validation(format!(
                "Expected identifier, got {:?}",
                other
            ))),
        }
    }

    /// Parse OR-level expression (lowest precedence).
    fn parse_or(&mut self) -> Result<ConditionExpr, AwsError> {
        let mut left = self.parse_and()?;
        loop {
            if let Token::Word(w) = self.peek().clone()
                && w.eq_ignore_ascii_case("OR")
            {
                self.advance();
                let right = self.parse_and()?;
                left = ConditionExpr::Logical {
                    op: LogicalOp::Or,
                    children: vec![left, right],
                };
                continue;
            }
            break;
        }
        Ok(left)
    }

    /// Parse AND-level expression.
    fn parse_and(&mut self) -> Result<ConditionExpr, AwsError> {
        let mut left = self.parse_not()?;
        loop {
            if let Token::Word(w) = self.peek().clone()
                && w.eq_ignore_ascii_case("AND")
            {
                self.advance();
                let right = self.parse_not()?;
                left = ConditionExpr::Logical {
                    op: LogicalOp::And,
                    children: vec![left, right],
                };
                continue;
            }
            break;
        }
        Ok(left)
    }

    /// Parse NOT expression.
    fn parse_not(&mut self) -> Result<ConditionExpr, AwsError> {
        if let Token::Word(w) = self.peek().clone()
            && w.eq_ignore_ascii_case("NOT")
        {
            self.advance();
            let inner = self.parse_primary()?;
            return Ok(ConditionExpr::Not(Box::new(inner)));
        }
        self.parse_primary()
    }

    /// Parse a primary expression (atom).
    fn parse_primary(&mut self) -> Result<ConditionExpr, AwsError> {
        // Parenthesized group
        if self.peek() == &Token::LParen {
            self.advance();
            let expr = self.parse_or()?;
            // consume )
            if self.peek() == &Token::RParen {
                self.advance();
            }
            return Ok(expr);
        }

        // Read first operand (path or function call)
        let operand = self.parse_operand()?;

        // Check if it's a function call
        if let Operand::Path(ref name) = operand {
            let name_lc = name.to_lowercase();

            if name_lc == "attribute_exists" && self.peek() == &Token::LParen {
                self.advance();
                let path = self.read_path()?;
                if self.peek() == &Token::RParen {
                    self.advance();
                }
                return Ok(ConditionExpr::AttributeExists(path));
            }
            if name_lc == "attribute_not_exists" && self.peek() == &Token::LParen {
                self.advance();
                let path = self.read_path()?;
                if self.peek() == &Token::RParen {
                    self.advance();
                }
                return Ok(ConditionExpr::AttributeNotExists(path));
            }
            if name_lc == "attribute_type" && self.peek() == &Token::LParen {
                self.advance();
                let path = self.read_path()?;
                if self.peek() == &Token::Comma {
                    self.advance();
                }
                let type_val = self.parse_operand()?;
                if self.peek() == &Token::RParen {
                    self.advance();
                }
                return Ok(ConditionExpr::AttributeType(path, type_val));
            }
            if name_lc == "begins_with" && self.peek() == &Token::LParen {
                self.advance();
                let path = self.parse_operand()?;
                if self.peek() == &Token::Comma {
                    self.advance();
                }
                let val = self.parse_operand()?;
                if self.peek() == &Token::RParen {
                    self.advance();
                }
                return Ok(ConditionExpr::BeginsWith(path, val));
            }
            if name_lc == "contains" && self.peek() == &Token::LParen {
                self.advance();
                let path = self.parse_operand()?;
                if self.peek() == &Token::Comma {
                    self.advance();
                }
                let val = self.parse_operand()?;
                if self.peek() == &Token::RParen {
                    self.advance();
                }
                return Ok(ConditionExpr::Contains(path, val));
            }
            if name_lc == "size" && self.peek() == &Token::LParen {
                self.advance();
                let path = self.read_path()?;
                if self.peek() == &Token::RParen {
                    self.advance();
                }
                // Expect comparison op
                let op = self.parse_compare_op()?;
                let right = self.parse_operand()?;
                return Ok(ConditionExpr::SizeComparison { path, op, right });
            }
        }

        // Comparison: operand op operand
        // Check for BETWEEN
        if let Token::Word(w) = self.peek().clone() {
            if w.eq_ignore_ascii_case("BETWEEN") {
                self.advance();
                let low = self.parse_operand()?;
                // consume AND
                if let Token::Word(w2) = self.peek().clone()
                    && w2.eq_ignore_ascii_case("AND")
                {
                    self.advance();
                }
                let high = self.parse_operand()?;
                return Ok(ConditionExpr::Between { operand, low, high });
            }
            if w.eq_ignore_ascii_case("IN") {
                self.advance();
                // consume (
                if self.peek() == &Token::LParen {
                    self.advance();
                }
                let mut values = Vec::new();
                loop {
                    values.push(self.parse_operand()?);
                    if self.peek() == &Token::Comma {
                        self.advance();
                    } else {
                        break;
                    }
                }
                if self.peek() == &Token::RParen {
                    self.advance();
                }
                return Ok(ConditionExpr::In { operand, values });
            }
        }

        // Regular comparison
        if let Ok(op) = self.parse_compare_op() {
            let right = self.parse_operand()?;
            return Ok(ConditionExpr::Comparison {
                left: operand,
                op,
                right,
            });
        }

        // Fallback: attribute_exists check if no operator
        if let Operand::Path(p) = operand {
            Ok(ConditionExpr::AttributeExists(p))
        } else {
            Err(AwsError::validation("Unexpected expression format"))
        }
    }

    fn parse_compare_op(&mut self) -> Result<CompareOp, AwsError> {
        let op = match self.peek().clone() {
            Token::Eq => CompareOp::Eq,
            Token::Ne => CompareOp::Ne,
            Token::Lt => CompareOp::Lt,
            Token::Le => CompareOp::Le,
            Token::Gt => CompareOp::Gt,
            Token::Ge => CompareOp::Ge,
            other => {
                return Err(AwsError::validation(format!(
                    "Expected comparison operator, got {:?}",
                    other
                )));
            }
        };
        self.advance();
        Ok(op)
    }

    /// Parse an operand: either a value placeholder (:name) or a path (#name or name.subfield).
    fn parse_operand(&mut self) -> Result<Operand, AwsError> {
        if self.peek() == &Token::Colon {
            self.advance();
            let name = self.expect_word()?;
            return Ok(Operand::Value(format!(":{name}")));
        }
        let path = self.read_path()?;
        Ok(Operand::Path(path))
    }

    /// Read a dot-separated path, possibly starting with # tokens.
    fn read_path(&mut self) -> Result<String, AwsError> {
        let mut parts = Vec::new();
        // First segment
        parts.push(self.read_path_segment()?);
        // Dot-separated rest
        while self.peek() == &Token::Dot {
            self.advance();
            parts.push(self.read_path_segment()?);
        }
        Ok(parts.join("."))
    }

    fn read_path_segment(&mut self) -> Result<String, AwsError> {
        if self.peek() == &Token::Hash {
            self.advance();
            let name = self.expect_word()?;
            Ok(format!("#{name}"))
        } else {
            self.expect_word()
        }
    }
}

// ─── Public parse functions ───────────────────────────────────────────────────

/// Parse a condition/filter expression string into a ConditionExpr AST.
pub fn parse_condition(expr: &str) -> Result<ConditionExpr, AwsError> {
    let tokens = Lexer::new(expr).tokenize();
    let mut parser = Parser::new(tokens);
    parser.parse_or()
}

/// Parse a projection expression into a list of path strings.
/// e.g. "#n, address.city" → ["#n", "address.city"]
pub fn parse_projection(expr: &str) -> Vec<String> {
    expr.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Resolve a path that may contain expression attribute name placeholders (#name).
pub fn resolve_path(path: &str, expr_attr_names: &HashMap<String, String>) -> String {
    path.split('.')
        .map(|seg| {
            if let Some(stripped) = seg.strip_prefix('#') {
                expr_attr_names
                    .get(&format!("#{stripped}"))
                    .cloned()
                    .unwrap_or_else(|| seg.to_string())
            } else {
                seg.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// Resolve a value placeholder (":name") using ExpressionAttributeValues.
pub fn resolve_value<'a>(
    placeholder: &str,
    expr_attr_values: &'a serde_json::Map<String, serde_json::Value>,
) -> Option<&'a serde_json::Value> {
    expr_attr_values.get(placeholder)
}
