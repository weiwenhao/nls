use super::common::*;
use super::lexer::Token;
use super::lexer::TokenType;
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::sync::{Arc, Mutex};

pub struct SyntaxError(usize, usize, String);

impl fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "SyntaxError: {}", self.2)
    }
}

impl fmt::Debug for SyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "SyntaxError: {}", self.2)
    }
}
impl Error for SyntaxError {}

pub fn token_to_expr_op(token: &TokenType) -> ExprOp {
    match token {
        TokenType::Plus => ExprOp::Add,
        TokenType::Minus => ExprOp::Sub,
        TokenType::Star => ExprOp::Mul,
        TokenType::Slash => ExprOp::Div,
        TokenType::Percent => ExprOp::Rem,
        TokenType::EqualEqual => ExprOp::Ee,
        TokenType::NotEqual => ExprOp::Ne,
        TokenType::GreaterEqual => ExprOp::Ge,
        TokenType::RightAngle => ExprOp::Gt,
        TokenType::LessEqual => ExprOp::Le,
        TokenType::LessThan => ExprOp::Lt,
        TokenType::AndAnd => ExprOp::AndAnd,
        TokenType::OrOr => ExprOp::OrOr,

        // 位运算
        TokenType::Tilde => ExprOp::Bnot,
        TokenType::And => ExprOp::And,
        TokenType::Or => ExprOp::Or,
        TokenType::Xor => ExprOp::Xor,
        TokenType::LeftShift => ExprOp::Lshift,
        TokenType::RightShift => ExprOp::Rshift,

        // equal 快捷运算拆解
        TokenType::PercentEqual => ExprOp::Rem,
        TokenType::MinusEqual => ExprOp::Sub,
        TokenType::PlusEqual => ExprOp::Add,
        TokenType::SlashEqual => ExprOp::Div,
        TokenType::StarEqual => ExprOp::Mul,
        TokenType::OrEqual => ExprOp::Or,
        TokenType::AndEqual => ExprOp::And,
        TokenType::XorEqual => ExprOp::Xor,
        TokenType::LeftShiftEqual => ExprOp::Lshift,
        TokenType::RightShiftEqual => ExprOp::Rshift,
        _ => ExprOp::None,
    }
}

pub fn token_to_type_kind(token: &TokenType) -> TypeKind {
    match token {
        // literal
        TokenType::True | TokenType::False => TypeKind::Bool,
        TokenType::Null => TypeKind::Null,
        TokenType::Void => TypeKind::Void,
        TokenType::FloatLiteral => TypeKind::Float,
        TokenType::IntLiteral => TypeKind::Int,
        TokenType::StringLiteral => TypeKind::String,

        // type
        TokenType::Bool => TypeKind::Bool,
        TokenType::Float => TypeKind::Float,
        TokenType::F32 => TypeKind::Float32,
        TokenType::F64 => TypeKind::Float64,
        TokenType::Int => TypeKind::Int,
        TokenType::I8 => TypeKind::Int8,
        TokenType::I16 => TypeKind::Int16,
        TokenType::I32 => TypeKind::Int32,
        TokenType::I64 => TypeKind::Int64,
        TokenType::Uint => TypeKind::Uint,
        TokenType::U8 => TypeKind::Uint8,
        TokenType::U16 => TypeKind::Uint16,
        TokenType::U32 => TypeKind::Uint32,
        TokenType::U64 => TypeKind::Uint64,
        TokenType::String => TypeKind::String,
        TokenType::Var => TypeKind::Unknown,
        _ => TypeKind::Unknown,
    }
}

#[derive(Clone, Copy)]
struct ParserRule {
    prefix: Option<fn(&mut Syntax) -> Result<Box<Expr>, SyntaxError>>,
    infix: Option<fn(&mut Syntax, Box<Expr>) -> Result<Box<Expr>, SyntaxError>>,
    infix_precedence: SyntaxPrecedence,
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
#[repr(u8)] // 指定底层表示类型
pub enum SyntaxPrecedence {
    Null, // 最低优先级
    Assign,
    Catch,
    OrOr,     // ||
    AndAnd,   // &&
    Or,       // |
    Xor,      // ^
    And,      // &
    CmpEqual, // == !=
    Compare,  // > < >= <=
    Shift,    // << >>
    Term,     // + -
    Factor,   // * / %
    TypeCast, // as/is
    Unary,    // - ! ~ * &
    Call,     // foo.bar foo["bar"] foo() foo().foo.bar
    Primary,  // 最高优先级
}

impl SyntaxPrecedence {
    fn next(self) -> Option<Self> {
        let next_value = (self as u8).checked_add(1)?;
        if next_value <= (Self::Primary as u8) {
            // 使用 unsafe 是安全的,因为我们已经确保值在枚举范围内
            Some(unsafe { std::mem::transmute(next_value) })
        } else {
            None
        }
    }
}

pub struct Syntax {
    tokens: Vec<Token>,
    current: usize, // token index

    errors: Vec<AnalyzerError>,

    // parser 阶段辅助记录当前的 type_param, 当进入到 fn body 或者 struct def 时可以准确识别当前是 type param 还是 alias, 仅仅使用到 key
    // 默认是一个空 hashmap
    type_params_table: HashMap<String, String>,

    // 部分表达式只有在 match cond 中可以使用，比如 is T, n if n xxx 等, parser_match_cond 为 true 时，表示当前处于 match cond 中
    match_cond: bool,

    // match 表达式中 subject 的解析
    match_subject: bool,
}

impl Syntax {
    // static method new, Syntax::new(tokens)
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens: tokens,
            current: 0,
            type_params_table: HashMap::new(),
            match_cond: false,
            match_subject: false,
            errors: Vec::new(),
        }
    }

    fn advance(&mut self) -> &Token {
        // assert!(
        //     self.current + 1 < self.tokens.len(),
        //     "Syntax::advance: current index out of range"
        // );
        let token = &self.tokens[self.current];

        self.current += 1;
        return token;
    }

    fn peek(&self) -> &Token {
        if self.current >= self.tokens.len() {
            panic!("syntax::peek: current index out of range");
        }
        return &self.tokens[self.current];
    }

    fn prev(&self) -> Option<&Token> {
        if self.current == 0 {
            return None;
        }

        return Some(&self.tokens[self.current - 1]);
    }

    fn is(&self, token_type: TokenType) -> bool {
        return self.peek().token_type == token_type;
    }

    fn consume(&mut self, token_type: TokenType) -> bool {
        if self.is(token_type) {
            self.advance();
            return true;
        }
        return false;
    }

    fn must(&mut self, expect: TokenType) -> Result<&Token, SyntaxError> {
        let token = self.peek().clone(); // 对 self 进行了不可变借用, clone 让借用立刻结束
        self.advance();

        if token.token_type != expect {
            let message = format!("expected '{}'", expect.to_string());

            return Err(SyntaxError(token.start, token.end, message));
        }

        return Ok(self.prev().unwrap());
    }

    // 对应 parser_next
    fn next(&self, step: usize) -> Option<&Token> {
        if self.current + step >= self.tokens.len() {
            return None;
        }
        Some(&self.tokens[self.current + step])
    }

    // 对应 parser_next_is
    fn next_is(&self, step: usize, expect: TokenType) -> bool {
        match self.next(step) {
            Some(token) => token.token_type == expect,
            None => false,
        }
    }

    fn is_stmt_eof(&self) -> bool {
        self.is(TokenType::StmtEof) || self.is(TokenType::Eof)
    }

    fn stmt_new(&self) -> Box<Stmt> {
        Box::new(Stmt {
            start: self.peek().start,
            end: self.peek().end,
            node: AstNode::None,
        })
    }

    fn expr_new(&self) -> Box<Expr> {
        Box::new(Expr {
            start: self.peek().start,
            end: self.peek().end,
            type_: Type::default(),
            target_type: Type::default(),
            node: AstNode::None,
        })
    }

    fn fake_new(&self, expr: Box<Expr>) -> Box<Stmt> {
        let mut stmt = self.stmt_new();
        stmt.node = AstNode::Fake(expr);

        return stmt;
    }

    fn find_rule(&self, token_type: TokenType) -> ParserRule {
        use TokenType::*;
        match token_type {
            LeftParen => ParserRule {
                prefix: Some(Self::parser_left_paren_expr),
                infix: Some(Self::parser_call_expr),
                infix_precedence: SyntaxPrecedence::Call,
            },
            LeftSquare => ParserRule {
                prefix: Some(Self::parser_vec_new),
                infix: Some(Self::parser_access),
                infix_precedence: SyntaxPrecedence::Call,
            },
            LeftCurly => ParserRule {
                prefix: Some(Self::parser_left_curly_expr),
                infix: None,
                infix_precedence: SyntaxPrecedence::Null,
            },
            LessThan => ParserRule {
                prefix: None,
                infix: Some(Self::parser_binary),
                infix_precedence: SyntaxPrecedence::Compare,
            },
            LeftAngle => ParserRule {
                prefix: None,
                infix: Some(Self::parser_type_args_expr),
                infix_precedence: SyntaxPrecedence::Call,
            },
            MacroIdent => ParserRule {
                prefix: Some(Self::parser_macro_call),
                infix: None,
                infix_precedence: SyntaxPrecedence::Null,
            },
            Dot => ParserRule {
                prefix: None,
                infix: Some(Self::parser_select),
                infix_precedence: SyntaxPrecedence::Call,
            },
            Minus => ParserRule {
                prefix: Some(Self::parser_unary),
                infix: Some(Self::parser_binary),
                infix_precedence: SyntaxPrecedence::Term,
            },
            Plus => ParserRule {
                prefix: None,
                infix: Some(Self::parser_binary),
                infix_precedence: SyntaxPrecedence::Term,
            },
            Not => ParserRule {
                prefix: Some(Self::parser_unary),
                infix: None,
                infix_precedence: SyntaxPrecedence::Unary,
            },
            Tilde => ParserRule {
                prefix: Some(Self::parser_unary),
                infix: None,
                infix_precedence: SyntaxPrecedence::Unary,
            },
            And => ParserRule {
                prefix: Some(Self::parser_unary),
                infix: Some(Self::parser_binary),
                infix_precedence: SyntaxPrecedence::And,
            },
            Or => ParserRule {
                prefix: None,
                infix: Some(Self::parser_binary),
                infix_precedence: SyntaxPrecedence::Or,
            },
            Xor => ParserRule {
                prefix: None,
                infix: Some(Self::parser_binary),
                infix_precedence: SyntaxPrecedence::Xor,
            },
            LeftShift => ParserRule {
                prefix: None,
                infix: Some(Self::parser_binary),
                infix_precedence: SyntaxPrecedence::Shift,
            },
            Star => ParserRule {
                prefix: Some(Self::parser_unary),
                infix: Some(Self::parser_binary),
                infix_precedence: SyntaxPrecedence::Factor,
            },
            Slash => ParserRule {
                prefix: None,
                infix: Some(Self::parser_binary),
                infix_precedence: SyntaxPrecedence::Factor,
            },
            OrOr => ParserRule {
                prefix: None,
                infix: Some(Self::parser_binary),
                infix_precedence: SyntaxPrecedence::OrOr,
            },
            AndAnd => ParserRule {
                prefix: None,
                infix: Some(Self::parser_binary),
                infix_precedence: SyntaxPrecedence::AndAnd,
            },
            NotEqual | EqualEqual => ParserRule {
                prefix: None,
                infix: Some(Self::parser_binary),
                infix_precedence: SyntaxPrecedence::CmpEqual,
            },
            RightShift => ParserRule {
                prefix: None,
                infix: Some(Self::parser_binary),
                infix_precedence: SyntaxPrecedence::Shift,
            },

            RightAngle | GreaterEqual | LessEqual => ParserRule {
                prefix: None,
                infix: Some(Self::parser_binary),
                infix_precedence: SyntaxPrecedence::Compare,
            },
            StringLiteral | IntLiteral | FloatLiteral | True | False | Null => ParserRule {
                prefix: Some(Self::parser_literal),
                infix: None,
                infix_precedence: SyntaxPrecedence::Null,
            },
            As => ParserRule {
                prefix: None,
                infix: Some(Self::parser_as_expr),
                infix_precedence: SyntaxPrecedence::TypeCast,
            },
            Is => ParserRule {
                prefix: Some(Self::parser_match_is_expr),
                infix: Some(Self::parser_is_expr),
                infix_precedence: SyntaxPrecedence::TypeCast,
            },
            Catch => ParserRule {
                prefix: None,
                infix: Some(Self::parser_catch_expr),
                infix_precedence: SyntaxPrecedence::Catch,
            },
            Ident => ParserRule {
                prefix: Some(Self::parser_ident_expr),
                infix: None,
                infix_precedence: SyntaxPrecedence::Null,
            },
            _ => ParserRule {
                prefix: None,
                infix: None,
                infix_precedence: SyntaxPrecedence::Null,
            },
        }
    }

    // 处理中缀表达式的 token
    fn parser_infix_token(&mut self, expr: &Box<Expr>) -> TokenType {
        let mut infix_token = self.peek().token_type.clone();

        // 处理 < 的歧义
        if infix_token == TokenType::LeftAngle && !self.parser_left_angle_is_type_args(expr) {
            infix_token = TokenType::LessThan;
        }

        // 处理连续的 >> 合并
        if infix_token == TokenType::RightAngle && self.next_is(1, TokenType::RightAngle) {
            self.advance();
            infix_token = TokenType::RightShift;
        }

        infix_token
    }

    fn must_stmt_end(&mut self) -> Result<(), SyntaxError> {
        if self.is(TokenType::Eof) || self.is(TokenType::RightCurly) {
            return Ok(());
        }

        // ; (scanner 时主动添加)
        if self.is(TokenType::StmtEof) {
            self.advance();
            return Ok(());
        }

        let prev_token = self.prev().unwrap();
        // stmt eof 失败。报告错误，并返回 false 即可
        // 获取前一个 token 的位置用于错误报告
        return Err(SyntaxError(
            prev_token.start,
            prev_token.end,
            "expected ';' or '}' at end of statement".to_string(),
        ));
    }

    fn is_basic_type(&self) -> bool {
        matches!(
            self.peek().token_type,
            TokenType::Var
                | TokenType::Null
                | TokenType::Void
                | TokenType::Int
                | TokenType::I8
                | TokenType::I16
                | TokenType::I32
                | TokenType::I64
                | TokenType::Uint
                | TokenType::U8
                | TokenType::U16
                | TokenType::U32
                | TokenType::U64
                | TokenType::Float
                | TokenType::F32
                | TokenType::F64
                | TokenType::Bool
                | TokenType::String
        )
    }

    pub fn parser(&mut self) -> (Vec<Box<Stmt>>, Vec<AnalyzerError>) {
        self.current = 0;

        let mut stmt_list = Vec::new();

        while !self.is(TokenType::Eof) {
            match self.parser_stmt() {
                Ok(stmt) => stmt_list.push(stmt),
                Err(e) => {
                    self.errors.push(AnalyzerError {
                        start: e.0,
                        end: e.1,
                        message: e.2,
                    });

                    // 查找到下一个同步点
                    let found = self.synchronize(0);
                    if !found {
                        // 当前字符无法被表达式解析，且 sync 查找下一个可用同步点失败，直接跳过当前字符
                        self.advance();
                    }
                }
            }
        }

        return (stmt_list, self.errors.clone());
    }

    fn parser_body(&mut self) -> Result<Vec<Box<Stmt>>, SyntaxError> {
        let mut stmt_list = Vec::new();
        self.must(TokenType::LeftCurly)?;

        while !self.is(TokenType::RightCurly) && !self.is(TokenType::Eof) {
            match self.parser_stmt() {
                Ok(stmt) => stmt_list.push(stmt),
                Err(e) => {
                    self.errors.push(AnalyzerError {
                        start: e.0,
                        end: e.1,
                        message: e.2,
                    });

                    self.synchronize(1);
                }
            }
        }
        self.must(TokenType::RightCurly)?;

        return Ok(stmt_list);
    }

    // fn advance_line(&mut self) {
    //     let current_line = self.peek().line;

    //     while !self.is(TokenType::Eof) && self.peek().line == current_line {
    //         self.advance();
    //     }
    // }

    fn synchronize(&mut self, current_brace_level: isize) -> bool {
        let mut brace_level = current_brace_level;

        loop {
            let token = self.peek().token_type.clone();

            // 提前返回的情况
            match token {
                TokenType::Eof => return false,

                // 在当前层级遇到语句结束符
                TokenType::StmtEof if brace_level == current_brace_level => {
                    self.advance();
                    return true;
                }

                // 在当前层级遇到关键字或基本类型
                _ if brace_level == current_brace_level => {
                    if matches!(
                        token,
                        TokenType::Fn
                            | TokenType::Var
                            | TokenType::Return
                            | TokenType::If
                            | TokenType::For
                            | TokenType::Match
                            | TokenType::Try
                            | TokenType::Catch
                            | TokenType::Continue
                            | TokenType::Break
                            | TokenType::Import
                            | TokenType::Type
                    ) || self.is_basic_type()
                    {
                        return true;
                    }
                }
                _ => {}
            }

            // 处理花括号层级
            match token {
                TokenType::LeftCurly => brace_level += 1,
                TokenType::RightCurly => {
                    brace_level -= 1;
                    if brace_level < current_brace_level {
                        return false;
                    }
                }
                _ => {}
            }

            self.advance();
        }
    }

    fn parser_single_type(&mut self) -> Result<Type, SyntaxError> {
        let mut t = Type::default();
        t.status = ReductionStatus::Undo;
        t.start = self.peek().start;

        // union type
        if self.consume(TokenType::Any) {
            t.kind = TypeKind::Union(true, Vec::new());
            t.end = self.prev().unwrap().end;
            return Ok(t);
        }

        // 基本类型 int/float/bool/string/void/var
        if self.is_basic_type() {
            let type_token = self.advance();
            t.kind = token_to_type_kind(&type_token.token_type);
            t.impl_ident = Some(t.kind.to_string());

            if matches!(
                type_token.token_type,
                TokenType::Int | TokenType::Uint | TokenType::Float
            ) {
                t.origin_ident = Some(type_token.literal.clone());
                t.origin_type_kind = t.kind.clone();
            }

            t.end = self.prev().unwrap().end;
            return Ok(t);
        }

        // ptr<type>
        if self.consume(TokenType::Ptr) {
            self.must(TokenType::LeftAngle)?;
            let value_type = self.parser_type()?;
            self.must(TokenType::RightAngle)?;

            t.kind = TypeKind::Ptr(Box::new(value_type));
            t.end = self.prev().unwrap().end;
            return Ok(t);
        }

        // [type]
        if self.consume(TokenType::LeftSquare) {
            let element_type = self.parser_type()?;
            self.must(TokenType::RightSquare)?;

            t.kind = TypeKind::Vec(Box::new(element_type));
            t.end = self.prev().unwrap().end;
            return Ok(t);
        }

        // vec<type>
        if self.consume(TokenType::Vec) {
            self.must(TokenType::LeftAngle)?;
            let element_type = self.parser_type()?;
            self.must(TokenType::RightAngle)?;

            t.kind = TypeKind::Vec(Box::new(element_type));
            t.end = self.prev().unwrap().end;
            return Ok(t);
        }

        // map<type,type>
        if self.consume(TokenType::Map) {
            self.must(TokenType::LeftAngle)?;
            let key_type = self.parser_type()?;
            self.must(TokenType::Comma)?;
            let value_type = self.parser_type()?;
            self.must(TokenType::RightAngle)?;

            t.kind = TypeKind::Map(Box::new(key_type), Box::new(value_type));
            t.end = self.prev().unwrap().end;
            return Ok(t);
        }

        // set<type>
        if self.consume(TokenType::Set) {
            self.must(TokenType::LeftAngle)?;
            let element_type = self.parser_type()?;
            self.must(TokenType::RightAngle)?;

            t.kind = TypeKind::Set(Box::new(element_type));
            t.end = self.prev().unwrap().end;
            return Ok(t);
        }

        // tup<type, type, ...>
        if self.consume(TokenType::Tup) {
            self.must(TokenType::LeftAngle)?;
            let mut elements = Vec::new();

            loop {
                let element_type = self.parser_type()?;
                elements.push(element_type);

                if !self.consume(TokenType::Comma) {
                    break;
                }
            }
            self.must(TokenType::RightAngle)?;

            t.kind = TypeKind::Tuple(elements, 0);
            t.end = self.prev().unwrap().end;
            return Ok(t);
        }

        // chan<type>
        if self.consume(TokenType::Chan) {
            self.must(TokenType::LeftAngle)?;
            let element_type = self.parser_type()?;
            self.must(TokenType::RightAngle)?;

            t.kind = TypeKind::Chan(Box::new(element_type));
            t.end = self.prev().unwrap().end;
            return Ok(t);
        }

        // arr<type,length>
        if self.consume(TokenType::Arr) {
            self.must(TokenType::LeftAngle)?;
            let element_type = self.parser_type()?;
            self.must(TokenType::Comma)?;
            let length_token = self.must(TokenType::IntLiteral)?;

            let length = length_token.literal.parse::<u64>().map_err(|_| {
                SyntaxError(
                    length_token.start,
                    length_token.end,
                    "array length must be a valid integer".to_string(),
                )
            })?;

            if length == 0 {
                return Err(SyntaxError(
                    length_token.start,
                    length_token.end,
                    "array length must be greater than 0".to_string(),
                ));
            }
            self.must(TokenType::RightAngle)?;

            t.kind = TypeKind::Arr(length, Box::new(element_type));
            t.end = self.prev().unwrap().end;
            return Ok(t);
        }

        // tuple (type, type)
        if self.consume(TokenType::LeftParen) {
            let mut elements = Vec::new();
            loop {
                let element_type = self.parser_type()?;
                elements.push(element_type);
                if !self.consume(TokenType::Comma) {
                    break;
                }
            }
            self.must(TokenType::RightParen)?;

            t.kind = TypeKind::Tuple(elements, 0);
            t.end = self.prev().unwrap().end;
            return Ok(t);
        }

        // {Type:Type} or {Type}
        if self.consume(TokenType::LeftCurly) {
            let key_type = self.parser_type()?;

            if self.consume(TokenType::Colon) {
                // map 类型
                let value_type = self.parser_type()?;
                self.must(TokenType::RightCurly)?;

                t.kind = TypeKind::Map(Box::new(key_type), Box::new(value_type));

                t.end = self.prev().unwrap().end;
                return Ok(t);
            } else {
                // set 类型
                self.must(TokenType::RightCurly)?;

                t.kind = TypeKind::Set(Box::new(key_type));

                t.end = self.prev().unwrap().end;
                return Ok(t);
            }
        }

        // struct { field_type field_name = default_value }
        if self.consume(TokenType::Struct) {
            self.must(TokenType::LeftCurly)?;

            let mut properties = Vec::new();

            while !self.is(TokenType::RightCurly) {
                let field_type = self.parser_type()?;
                let field_name = self.advance().literal.clone();

                let mut default_value = None;

                // 默认值支持
                if self.consume(TokenType::Equal) {
                    let expr = self.parser_expr()?;

                    // 不允许是函数定义
                    if let AstNode::FnDef(_) = expr.node {
                        return Err(SyntaxError(
                            expr.start,
                            expr.end,
                            "struct field default value cannot be a function definition".to_string(),
                        ));
                    }

                    default_value = Some(expr);
                }

                properties.push(TypeStructProperty {
                    type_: field_type,
                    key: field_name,
                    value: default_value,
                });

                self.must_stmt_end()?;
            }

            self.must(TokenType::RightCurly)?;

            t.kind = TypeKind::Struct("".to_string(), 0, properties);
            t.end = self.prev().unwrap().end;
            return Ok(t);
        }

        // fn(Type, Type, ...):ReturnType
        if self.consume(TokenType::Fn) {
            self.must(TokenType::LeftParen)?;
            let mut param_types = Vec::new();

            if !self.consume(TokenType::RightParen) {
                loop {
                    let param_type = self.parser_type()?;
                    param_types.push(param_type);

                    if !self.consume(TokenType::Comma) {
                        break;
                    }
                }
                self.must(TokenType::RightParen)?;
            }

            let return_type = if self.consume(TokenType::Colon) {
                self.parser_type()?
            } else {
                Type::new(TypeKind::Void)
            };

            t.kind = TypeKind::Fn(Box::new(TypeFn {
                name: None,
                param_types,
                return_type,
                rest: false,
                tpl: false,
            }));
            t.end = self.prev().unwrap().end;
            return Ok(t);
        }

        // ident foo = 12
        if self.is(TokenType::Ident) {
            let first = self.advance().clone();

            // handle param
            if !self.type_params_table.is_empty() && self.type_params_table.contains_key(&first.literal) {
                t.kind = TypeKind::Param(first.literal.clone());
                t.origin_ident = Some(first.literal.clone());
                t.origin_type_kind = t.kind.clone();
                return Ok(t);
            }

            // handle alias (package.ident)
            let mut second = None;
            if self.consume(TokenType::Dot) {
                second = Some(self.advance());
            }

            let ident = if let Some(second_token) = second {
                second_token.clone()
            } else {
                first.clone()
            };
            let mut alias = TypeAlias {
                ident: ident.literal,
                import_as: if second.is_some() {
                    Some(first.literal.clone())
                } else {
                    None
                },

                args: None,
            };
            t.origin_ident = Some(alias.ident.clone());
            if let Some(import_as) = &alias.import_as {
                t.origin_ident = Some(format!("{}.{}", import_as, alias.ident.clone()));
            }

            // alias<arg1, arg2, ...>
            if self.consume(TokenType::LeftAngle) {
                let mut args = Vec::new();
                loop {
                    args.push(self.parser_single_type()?);
                    if !self.consume(TokenType::Comma) {
                        break;
                    }
                }
                self.must(TokenType::RightAngle)?;
                alias.args = Some(args);
            }

            t.kind = TypeKind::Alias(Box::new(alias));
            t.end = self.prev().unwrap().end;
            return Ok(t);
        }

        return Err(SyntaxError(
            self.peek().start,
            self.peek().end,
            "Type definition exception".to_string(),
        ));
    }

    fn parser_type(&mut self) -> Result<Type, SyntaxError> {
        let t = self.parser_single_type()?;

        // Type|Type or Type?
        if !self.is(TokenType::Or) && !self.is(TokenType::Question) {
            return Ok(t);
        }

        // handle union type
        let mut union_t = Type::default();
        union_t.status = ReductionStatus::Undo;
        union_t.start = self.peek().start;

        let mut elements = Vec::new();

        elements.push(t);

        if self.consume(TokenType::Question) {
            let t2 = Type::new(TypeKind::Null);
            elements.push(t2);

            union_t.kind = TypeKind::Union(false, elements);
            union_t.end = self.prev().unwrap().end;
            return Ok(union_t);
        }

        // T|E
        self.must(TokenType::Or)?;
        loop {
            let t2 = self.parser_single_type()?;
            elements.push(t2);

            if !self.consume(TokenType::Or) {
                break;
            }
        }

        union_t.kind = TypeKind::Union(false, elements);
        union_t.end = self.prev().unwrap().end;
        return Ok(union_t);
    }

    fn parser_type_alias_stmt(&mut self) -> Result<Box<Stmt>, SyntaxError> {
        let mut stmt = self.stmt_new();

        self.must(TokenType::Type)?;
        let ident_token = self.must(TokenType::Ident)?;
        let alias_ident = ident_token.clone();

        // T<arg1, arg2>
        let mut alias_args = Vec::new();
        if self.consume(TokenType::LeftAngle) {
            if self.is(TokenType::RightAngle) {
                return Err(SyntaxError(
                    self.peek().start,
                    self.peek().end,
                    "type alias params cannot be empty".to_string(),
                ));
            }

            // 临时保存当前的 type_params_table
            self.type_params_table = HashMap::new();

            loop {
                let ident = self.advance().literal.clone();
                let mut param = GenericsParam::new(ident.clone());

                // 可选的泛型类型约束 <T:t1|t2, U:t1|t2>
                if self.consume(TokenType::Colon) {
                    param.constraints.0 = false;
                    loop {
                        let t = self.parser_single_type()?;

                        param.constraints.1.push(t);
                        if !self.consume(TokenType::Or) {
                            break;
                        }
                    }
                }

                alias_args.push(param);

                self.type_params_table.insert(ident.clone(), ident.clone());

                if !self.consume(TokenType::Comma) {
                    break;
                }
            }

            self.must(TokenType::RightAngle)?;
        }

        self.must(TokenType::Equal)?;

        let alias_type = self.parser_type()?;

        // 恢复之前的 type_params_table
        self.type_params_table = HashMap::new();

        stmt.node = AstNode::TypeAlias(Arc::new(Mutex::new(TypeAliasStmt {
            ident: alias_ident.literal,
            symbol_start: alias_ident.start,
            symbol_end: alias_ident.end,
            params: if alias_args.is_empty() { None } else { Some(alias_args) },
            type_: alias_type,
        })));

        Ok(stmt)
    }

    fn expr_to_type_alias(&self, left_expr: &Expr, generics_args: Option<Vec<Type>>) -> Type {
        let mut t = Type::default();
        t.status = ReductionStatus::Undo;
        t.start = self.peek().start;
        t.end = self.peek().end;

        // 根据左值表达式类型构造 TypeAlias
        let alias = match &left_expr.node {
            // 简单标识符: foo
            AstNode::Ident(ident) => {
                t.origin_ident = Some(ident.clone());
                t.origin_type_kind = TypeKind::Alias(Box::new(TypeAlias::default()));

                TypeAlias {
                    ident: ident.clone(),
                    import_as: None,
                    args: generics_args,
                }
            }

            // 包选择器: pkg.foo
            AstNode::Select(left, key) => {
                if let AstNode::Ident(left_ident) = &left.node {
                    t.origin_ident = Some(key.clone());
                    t.origin_type_kind = TypeKind::Alias(Box::new(TypeAlias::default()));

                    TypeAlias {
                        ident: key.clone(),
                        import_as: Some(left_ident.clone()),
                        args: generics_args,
                    }
                } else {
                    panic!("struct new left type exception");
                }
            }
            _ => panic!("struct new left type exception"),
        };

        t.kind = TypeKind::Alias(Box::new(alias));
        t
    }

    // 解析变量声明
    fn parser_var_decl(&mut self) -> Result<Arc<Mutex<VarDeclExpr>>, SyntaxError> {
        let var_type = self.parser_type()?;

        // 变量名必须是标识符
        let var_ident = self.must(TokenType::Ident)?;

        Ok(Arc::new(Mutex::new(VarDeclExpr {
            type_: var_type,
            ident: var_ident.literal.clone(),
            symbol_start: var_ident.start,
            symbol_end: var_ident.end,
            be_capture: false,
            heap_ident: None,
        })))
    }

    // 解析函数参数
    fn parser_params(&mut self, fn_decl: &mut AstFnDef) -> Result<(), SyntaxError> {
        self.must(TokenType::LeftParen)?;

        if self.consume(TokenType::RightParen) {
            return Ok(());
        }

        loop {
            if self.consume(TokenType::Ellipsis) {
                fn_decl.rest_param = true;
            }

            let param = self.parser_var_decl()?;
            fn_decl.params.push(param);

            // 可变参数必须是最后一个参数
            if fn_decl.rest_param && !self.is(TokenType::RightParen) {
                return Err(SyntaxError(
                    self.peek().start,
                    self.peek().end,
                    "can only use '...' as the final argument in the list".to_string(),
                ));
            }

            if !self.consume(TokenType::Comma) {
                break;
            }
        }

        self.must(TokenType::RightParen)?;
        Ok(())
    }

    // 解析二元表达式
    fn parser_binary(&mut self, left: Box<Expr>) -> Result<Box<Expr>, SyntaxError> {
        let mut expr = self.expr_new();

        let operator_token = self.advance().clone();

        // 获取运算符优先级
        let precedence = self.find_rule(operator_token.token_type.clone()).infix_precedence;
        let right = self.parser_precedence_expr(precedence.next().unwrap(), TokenType::Unknown)?;

        expr.node = AstNode::Binary(token_to_expr_op(&operator_token.token_type), left, right);

        Ok(expr)
    }

    fn parser_left_angle_is_type_args(&mut self, left: &Box<Expr>) -> bool {
        // 保存当前解析位置, 为后面的错误恢复做准备
        let current_pos = self.current;

        // 必须是标识符或选择器表达式
        match &left.node {
            AstNode::Ident(_) => (),
            AstNode::Select(left, _) => {
                // 选择器的左侧必须是标识符
                if !matches!(left.node, AstNode::Ident(_)) {
                    return false;
                }
            }
            _ => return false,
        }

        // 跳过 <
        self.advance();

        // 尝试解析第一个类型
        if let Err(_) = self.parser_type() {
            // 类型解析存在错误
            self.current = current_pos;
            return false;
        }

        // 检查是否直接以 > 结束 (大多数情况)
        if self.is(TokenType::RightAngle) {
            self.current = current_pos;
            return true;
        }

        if self.consume(TokenType::Comma) {
            // 处理多个类型参数的情况
            loop {
                if let Err(_) = self.parser_type() {
                    self.current = current_pos;
                    return false;
                }

                if !self.consume(TokenType::Comma) {
                    break;
                }
            }

            if !self.is(TokenType::RightAngle) {
                self.current = current_pos;
                return false;
            }

            // type args 后面不能紧跟 { 或 (, 这两者通常是 generics params
            if !self.next_is(1, TokenType::LeftCurly) && !self.next_is(1, TokenType::LeftParen) {
                self.current = current_pos;
                return false;
            }

            self.current = current_pos;
            return true;
        }

        self.current = current_pos;
        return false;
    }

    fn parser_type_args_expr(&mut self, left: Box<Expr>) -> Result<Box<Expr>, SyntaxError> {
        assert!(self.is(TokenType::LeftAngle));

        let mut expr = self.expr_new();

        // 解析泛型参数
        let mut generics_args = Vec::new();
        if self.consume(TokenType::LeftAngle) {
            loop {
                let t = self.parser_type()?;
                generics_args.push(t);

                if !self.consume(TokenType::Comma) {
                    break;
                }
            }
            self.must(TokenType::RightAngle)?;
        }

        // 判断下一个符号
        if self.is(TokenType::LeftParen) {
            // 函数调用
            let mut call = AstCall {
                return_type: Type::default(),
                left,
                generics_args,
                args: Vec::new(),
                spread: false,
            };

            call.args = self.parser_args(&mut call)?;

            expr.node = AstNode::Call(call);
            return Ok(expr);
        }

        // 结构体初始化
        assert!(self.is(TokenType::LeftCurly));
        let t = self.expr_to_type_alias(&left, Some(generics_args));

        self.parser_struct_new(t)
    }

    fn parser_struct_new(&mut self, type_: Type) -> Result<Box<Expr>, SyntaxError> {
        let mut expr = self.expr_new();
        let mut properties = Vec::new();

        self.must(TokenType::LeftCurly)?;

        if !self.consume(TokenType::RightCurly) {
            loop {
                let key = self.must(TokenType::Ident)?.literal.clone();

                self.must(TokenType::Equal)?;

                let value = self.parser_expr()?;

                properties.push(StructNewProperty {
                    type_: Type::default(), // 类型会在语义分析阶段填充
                    key,
                    value,
                });

                if !self.consume(TokenType::Comma) {
                    break;
                }
            }

            self.consume(TokenType::StmtEof);
            self.must(TokenType::RightCurly)?;
        }

        expr.node = AstNode::StructNew(String::new(), type_, properties);

        Ok(expr)
    }

    fn parser_unary(&mut self) -> Result<Box<Expr>, SyntaxError> {
        let mut expr = self.expr_new();
        let operator_token = self.advance();

        let operator = match operator_token.token_type {
            TokenType::Not => ExprOp::Not,
            TokenType::Minus => {
                // 检查是否可以直接合并成字面量
                if self.is(TokenType::IntLiteral) {
                    let int_token = self.advance();
                    expr.node = AstNode::Literal(TypeKind::Int, format!("-{}", int_token.literal));
                    return Ok(expr);
                }

                if self.is(TokenType::FloatLiteral) {
                    let float_token = self.advance();
                    expr.node = AstNode::Literal(TypeKind::Float, format!("-{}", float_token.literal));
                    return Ok(expr);
                }

                ExprOp::Neg
            }
            TokenType::Tilde => ExprOp::Bnot,
            TokenType::And => ExprOp::La,
            TokenType::Star => ExprOp::Ia,
            _ => {
                return Err(SyntaxError(
                    operator_token.start,
                    operator_token.end,
                    format!("unknown unary operator '{}'", operator_token.literal),
                ));
            }
        };

        let operand = self.parser_precedence_expr(SyntaxPrecedence::Unary, TokenType::Unknown)?;
        expr.node = AstNode::Unary(operator, operand);

        Ok(expr)
    }

    fn parser_catch_expr(&mut self, left: Box<Expr>) -> Result<Box<Expr>, SyntaxError> {
        let mut expr = self.expr_new();
        self.must(TokenType::Catch)?;

        let error_ident = self.must(TokenType::Ident)?;

        let catch_err = VarDeclExpr {
            ident: error_ident.literal.clone(),
            symbol_start: error_ident.start,
            symbol_end: error_ident.end,
            type_: Type::new(TypeKind::Unknown), // 实际上就是 error type
            be_capture: false,
            heap_ident: None,
        };

        let catch_body = self.parser_body()?;

        expr.node = AstNode::Catch(left, catch_err, catch_body);

        Ok(expr)
    }

    fn parser_as_expr(&mut self, left: Box<Expr>) -> Result<Box<Expr>, SyntaxError> {
        let mut expr = self.expr_new();
        self.must(TokenType::As)?;

        let target_type = self.parser_single_type()?;

        expr.node = AstNode::As(target_type, left);

        Ok(expr)
    }

    fn parser_match_is_expr(&mut self) -> Result<Box<Expr>, SyntaxError> {
        let mut expr = self.expr_new();
        self.must(TokenType::Is)?;

        // 确保在 match 表达式中使用 is
        if !self.match_cond {
            return Err(SyntaxError(
                self.peek().start,
                self.peek().end,
                "is type must be specified in the match expression".to_string(),
            ));
        }

        let target_type = self.parser_single_type()?;

        expr.node = AstNode::MatchIs(target_type);

        Ok(expr)
    }

    fn parser_is_expr(&mut self, left: Box<Expr>) -> Result<Box<Expr>, SyntaxError> {
        let mut expr = self.expr_new();
        self.must(TokenType::Is)?;

        let target_type = self.parser_single_type()?;

        expr.node = AstNode::Is(target_type, left);

        Ok(expr)
    }

    fn parser_left_paren_expr(&mut self) -> Result<Box<Expr>, SyntaxError> {
        self.must(TokenType::LeftParen)?;

        // 先尝试解析为普通表达式
        let expr = self.parser_expr()?;

        // 如果直接遇到右括号,说明是普通的括号表达式
        if self.consume(TokenType::RightParen) {
            return Ok(expr);
        }

        // 否则应该是元组表达式
        self.must(TokenType::Comma)?;

        let mut elements = Vec::new();
        elements.push(expr);

        // 继续解析剩余的元素
        loop {
            let element = self.parser_expr()?;
            elements.push(element);

            if !self.consume(TokenType::Comma) {
                break;
            }
        }

        self.must(TokenType::RightParen)?;

        let mut tuple_expr = self.expr_new();
        tuple_expr.node = AstNode::TupleNew(elements);

        Ok(tuple_expr)
    }

    fn parser_literal(&mut self) -> Result<Box<Expr>, SyntaxError> {
        let mut expr = self.expr_new();
        let literal_token = self.advance();

        let kind = token_to_type_kind(&literal_token.token_type);

        expr.node = AstNode::Literal(kind, literal_token.literal.clone());

        Ok(expr)
    }

    fn parser_is_tuple_typedecl(&self, current: usize) -> bool {
        let t = &self.tokens[current];
        assert_eq!(t.token_type, TokenType::LeftParen, "tuple type decl start left param");

        // param is left paren, so close + 1 = 1,
        let mut close = 1;
        let mut pos = current;

        while t.token_type != TokenType::Eof {
            pos += 1;
            let t = &self.tokens[pos];

            if t.token_type == TokenType::LeftParen {
                close += 1;
            }

            if t.token_type == TokenType::RightParen {
                close -= 1;
                if close == 0 {
                    break;
                }
            }
        }

        if close > 0 {
            return false;
        }

        // (...) ident; ) 的 下一符号如果是 ident 就表示 (...) 里面是 tuple typedecl
        let t = &self.tokens[pos + 1];
        if t.token_type != TokenType::Ident {
            return false;
        }

        return true;
    }

    fn parser_ident_expr(&mut self) -> Result<Box<Expr>, SyntaxError> {
        let mut expr = self.expr_new();
        let ident_token = self.must(TokenType::Ident)?;

        expr.node = AstNode::Ident(ident_token.literal.clone());

        Ok(expr)
    }

    fn parser_access(&mut self, left: Box<Expr>) -> Result<Box<Expr>, SyntaxError> {
        let mut expr = self.expr_new();

        self.must(TokenType::LeftSquare)?;
        let key = self.parser_expr()?;
        self.must(TokenType::RightSquare)?;

        expr.node = AstNode::Access(left, key);

        Ok(expr)
    }

    fn parser_select(&mut self, left: Box<Expr>) -> Result<Box<Expr>, SyntaxError> {
        let mut expr = self.expr_new();

        self.must(TokenType::Dot)?;

        let property_token = self.must(TokenType::Ident)?;
        expr.node = AstNode::Select(left, property_token.literal.clone());

        Ok(expr)
    }

    fn parser_args(&mut self, call: &mut AstCall) -> Result<Vec<Box<Expr>>, SyntaxError> {
        self.must(TokenType::LeftParen)?;
        let mut args = Vec::new();

        // 无调用参数
        if self.consume(TokenType::RightParen) {
            return Ok(args);
        }

        loop {
            if self.consume(TokenType::Ellipsis) {
                call.spread = true;
            }

            let expr = self.parser_expr()?;
            args.push(expr);

            // 可变参数必须是最后一个参数
            if call.spread && !self.is(TokenType::RightParen) {
                return Err(SyntaxError(
                    self.peek().start,
                    self.peek().end,
                    "can only use '...' as the final argument in the list".to_string(),
                ));
            }

            if !self.consume(TokenType::Comma) {
                break;
            }
        }

        self.must(TokenType::RightParen)?;
        Ok(args)
    }

    fn parser_call_expr(&mut self, left: Box<Expr>) -> Result<Box<Expr>, SyntaxError> {
        let mut expr = self.expr_new();

        let mut call = AstCall {
            return_type: Type::default(),
            left,
            args: Vec::new(),
            generics_args: Vec::new(),
            spread: false,
        };

        call.args = self.parser_args(&mut call)?;

        expr.node = AstNode::Call(call);
        Ok(expr)
    }

    fn parser_else_if(&mut self) -> Result<Vec<Box<Stmt>>, SyntaxError> {
        let mut stmt_list = Vec::new();
        stmt_list.push(self.parser_if_stmt()?);
        Ok(stmt_list)
    }

    fn parser_if_stmt(&mut self) -> Result<Box<Stmt>, SyntaxError> {
        let mut stmt = self.stmt_new();
        self.must(TokenType::If)?;

        let condition = self.parser_expr_with_precedence()?;
        let consequent = self.parser_body()?;

        let alternate = if self.consume(TokenType::Else) {
            if self.is(TokenType::If) {
                self.parser_else_if()?
            } else {
                self.parser_body()?
            }
        } else {
            Vec::new()
        };

        stmt.node = AstNode::If(
            condition,
            consequent,
            if alternate.is_empty() { None } else { Some(alternate) },
        );

        Ok(stmt)
    }

    fn is_for_tradition_stmt(&self) -> Result<bool, SyntaxError> {
        let mut semicolon_count = 0;
        let mut close = 0;
        let mut pos = self.current;
        let current_line = self.tokens[pos].line;

        while pos < self.tokens.len() {
            let t = &self.tokens[pos];

            if t.token_type == TokenType::Eof {
                return Err(SyntaxError(
                    self.peek().start,
                    self.peek().end,
                    "unexpected end of file".to_string(),
                ));
            }

            if close == 0 && t.token_type == TokenType::StmtEof {
                semicolon_count += 1;
            }

            if t.token_type == TokenType::LeftCurly {
                close += 1;
            }

            if t.token_type == TokenType::RightCurly {
                close -= 1;
            }

            if t.line != current_line {
                break;
            }

            pos += 1;
        }

        if semicolon_count != 0 && semicolon_count != 2 {
            return Err(SyntaxError(
                self.peek().start,
                self.peek().end,
                "for statement must have two semicolons".to_string(),
            ));
        }

        Ok(semicolon_count == 2)
    }

    fn is_type_begin_stmt(&mut self) -> bool {
        // var/any/int/float/bool/string
        if self.is_basic_type() {
            return true;
        }

        if self.is(TokenType::Any) {
            return true;
        }

        // {int}/{int:int} 或 [int]
        if self.is(TokenType::LeftCurly) || self.is(TokenType::LeftSquare) {
            return true;
        }

        if self.is(TokenType::Ptr) {
            return true;
        }

        // 内置复合类型
        if matches!(
            self.peek().token_type,
            TokenType::Arr | TokenType::Map | TokenType::Tup | TokenType::Vec | TokenType::Set | TokenType::Chan
        ) {
            return true;
        }

        // fndef type (stmt 维度禁止了匿名 fndef, 所以这里一定是 fndef type)
        if self.is(TokenType::Fn) && self.next_is(1, TokenType::LeftParen) {
            return true;
        }

        // person a 连续两个 ident， 第一个 ident 一定是类型 ident
        if self.is(TokenType::Ident) && self.next_is(1, TokenType::Ident) {
            return true;
        }

        // package.ident foo = xxx
        if self.is(TokenType::Ident)
            && self.next_is(1, TokenType::Dot)
            && self.next_is(2, TokenType::Ident)
            && self.next_is(3, TokenType::Ident)
        {
            return true;
        }

        // person|i8 a
        if self.is(TokenType::Ident) && self.next_is(1, TokenType::Or) {
            return true;
        }

        // package.ident|i8 foo = xxx
        if self.is(TokenType::Ident)
            && self.next_is(1, TokenType::Dot)
            && self.next_is(2, TokenType::Ident)
            && self.next_is(3, TokenType::Or)
        {
            return true;
        }

        // person<[i8]> foo
        if self.is(TokenType::Ident) && self.next_is(1, TokenType::LeftAngle) {
            return true;
        }

        // person.foo<[i8]>
        if self.is(TokenType::Ident)
            && self.next_is(1, TokenType::Dot)
            && self.next_is(2, TokenType::Ident)
            && self.next_is(3, TokenType::LeftAngle)
        {
            return true;
        }

        // (var_a, var_b) = (1, 2)
        // (custom, int, int, (int, int), map) a = xxx
        if self.is(TokenType::LeftParen) && self.parser_is_tuple_typedecl(self.current) {
            return true;
        }

        false
    }

    fn parser_for_stmt(&mut self) -> Result<Box<Stmt>, SyntaxError> {
        self.advance();
        let mut stmt = self.stmt_new();

        // 通过找 ; 号的形式判断, 必须要有两个 ; 才会是 tradition
        // for int i = 1; i <= 10; i+=1
        if self.is_for_tradition_stmt()? {
            let init = self.parser_stmt()?;
            self.must(TokenType::StmtEof)?;

            let cond = self.parser_expr_with_precedence()?;
            self.must(TokenType::StmtEof)?;

            let update = self.parser_stmt()?;

            let body = self.parser_body()?;

            stmt.node = AstNode::ForTradition(init, cond, update, body);

            return Ok(stmt);
        }

        // for k,v in map {}
        if self.is(TokenType::Ident) && (self.next_is(1, TokenType::Comma) || self.next_is(1, TokenType::In)) {
            let first_ident = self.must(TokenType::Ident)?;
            let first = VarDeclExpr {
                type_: Type::new(TypeKind::Unknown),
                ident: first_ident.literal.clone(),
                symbol_start: first_ident.start,
                symbol_end: first_ident.end,
                be_capture: false,
                heap_ident: None,
            };

            let second = if self.consume(TokenType::Comma) {
                let second_ident = self.must(TokenType::Ident)?;
                Some(VarDeclExpr {
                    type_: Type::new(TypeKind::Unknown),
                    ident: second_ident.literal.clone(),
                    symbol_start: second_ident.start,
                    symbol_end: second_ident.end,
                    be_capture: false,
                    heap_ident: None,
                })
            } else {
                None
            };

            self.must(TokenType::In)?;
            let iterate = self.parser_precedence_expr(SyntaxPrecedence::TypeCast, TokenType::Unknown)?;
            let body = self.parser_body()?;

            stmt.node = AstNode::ForIterator(iterate, first, second, body);

            return Ok(stmt);
        }

        // for (condition) {}
        let condition = self.parser_expr_with_precedence()?;
        let body = self.parser_body()?;

        stmt.node = AstNode::ForCond(condition, body);

        Ok(stmt)
    }

    fn parser_assign(&mut self, left: Box<Expr>) -> Result<Box<Stmt>, SyntaxError> {
        let mut stmt = self.stmt_new();

        // 简单赋值
        if self.consume(TokenType::Equal) {
            let right = self.parser_expr()?;

            stmt.node = AstNode::Assign(left, right);

            return Ok(stmt);
        }

        // 复合赋值
        let t = self.advance().clone();
        if !t.is_complex_assign() {
            return Err(SyntaxError(
                t.start,
                t.end,
                format!("assign={} token exception", t.token_type),
            ));
        }

        let mut right = self.expr_new();
        right.node = AstNode::Binary(
            token_to_expr_op(&t.token_type),
            left.clone(),
            self.parser_expr_with_precedence()?,
        );

        stmt.node = AstNode::Assign(left, right);

        Ok(stmt)
    }

    fn parser_expr_begin_stmt(&mut self) -> Result<Box<Stmt>, SyntaxError> {
        let left = self.parser_expr()?;

        // 处理函数调用语句
        if let AstNode::Call(call) = left.node {
            if self.is(TokenType::Equal) {
                return Err(SyntaxError(
                    self.peek().start,
                    self.peek().end,
                    "call expr cannot assign".to_string(),
                ));
            }

            let mut stmt = self.stmt_new();
            stmt.node = AstNode::Call(call);
            return Ok(stmt);
        }

        // 处理 catch 语句
        if let AstNode::Catch(try_expr, catch_err, catch_body) = left.node {
            if self.is(TokenType::Equal) || self.is(TokenType::Catch) {
                return Err(SyntaxError(
                    self.peek().start,
                    self.peek().end,
                    "catch expr cannot assign or immediately next catch".to_string(),
                ));
            }

            let mut stmt = self.stmt_new();
            stmt.node = AstNode::Catch(try_expr, catch_err, catch_body);
            return Ok(stmt);
        }

        // 检查表达式完整性
        if self.is_stmt_eof() {
            return Err(SyntaxError(
                self.peek().start,
                self.peek().end,
                "expr incompleteness".to_string(),
            ));
        }

        // 处理赋值语句
        self.parser_assign(left)
    }

    fn parser_break_stmt(&mut self) -> Result<Box<Stmt>, SyntaxError> {
        let mut stmt = self.stmt_new();
        self.must(TokenType::Break)?;

        let expr = if !self.is_stmt_eof() && !self.is(TokenType::RightCurly) {
            Some(self.parser_expr()?)
        } else {
            None
        };

        stmt.node = AstNode::Break(expr);
        Ok(stmt)
    }

    fn parser_continue_stmt(&mut self) -> Result<Box<Stmt>, SyntaxError> {
        let mut stmt = self.stmt_new();
        self.must(TokenType::Continue)?;

        stmt.node = AstNode::Continue;
        Ok(stmt)
    }

    fn parser_return_stmt(&mut self) -> Result<Box<Stmt>, SyntaxError> {
        let mut stmt = self.stmt_new();
        self.advance();

        let expr = if !self.is_stmt_eof() && !self.is(TokenType::RightCurly) {
            Some(self.parser_expr()?)
        } else {
            None
        };

        stmt.node = AstNode::Return(expr);
        Ok(stmt)
    }

    fn parser_import_stmt(&mut self) -> Result<Box<Stmt>, SyntaxError> {
        let mut stmt = self.stmt_new();
        self.advance();

        let token = self.advance();
        let (file, ast_package) = if token.token_type == TokenType::StringLiteral {
            (Some(token.literal.clone()), None)
        } else if token.token_type == TokenType::Ident {
            let mut package = vec![token.literal.clone()];
            while self.consume(TokenType::Dot) {
                let ident = self.must(TokenType::Ident)?;
                package.push(ident.literal.clone());
            }
            (None, Some(package))
        } else {
            return Err(SyntaxError(
                token.start,
                token.end,
                "import token must be string or ident".to_string(),
            ));
        };

        let as_name = if self.consume(TokenType::As) {
            let t = self.advance();
            if !matches!(t.token_type, TokenType::Ident | TokenType::ImportStar) {
                return Err(SyntaxError(
                    t.start,
                    t.end,
                    "import as token must be ident or *".to_string(),
                ));
            }
            Some(t.literal.clone())
        } else {
            None
        };

        stmt.node = AstNode::Import(ImportStmt {
            file,
            ast_package,
            as_name,
            module_type: 0,
            full_path: String::new(),
            package_conf: None,
            package_dir: String::new(),
            use_links: false,
            module_ident: String::new(),
        });

        Ok(stmt)
    }

    fn parser_vec_new(&mut self) -> Result<Box<Expr>, SyntaxError> {
        let mut expr = self.expr_new();
        self.must(TokenType::LeftSquare)?;

        let mut elements = Vec::new();
        if !self.consume(TokenType::RightSquare) {
            loop {
                let element = self.parser_expr()?;
                elements.push(element);

                if !self.consume(TokenType::Comma) {
                    break;
                }
            }
            self.must(TokenType::RightSquare)?;
        }

        expr.node = AstNode::VecNew(elements, None, None);

        Ok(expr)
    }

    fn parser_left_curly_expr(&mut self) -> Result<Box<Expr>, SyntaxError> {
        let mut expr = self.expr_new();

        // parse empty curly
        self.must(TokenType::LeftCurly)?;
        if self.consume(TokenType::RightCurly) {
            expr.node = AstNode::EmptyCurlyNew;
            return Ok(expr);
        }

        // parse first expr
        let key_expr = self.parser_expr()?;

        // if colon, parse map
        if self.consume(TokenType::Colon) {
            let mut elements = Vec::new();
            let value = self.parser_expr()?;

            elements.push(MapElement { key: key_expr, value });

            while self.consume(TokenType::Comma) {
                let key = self.parser_expr()?;
                self.must(TokenType::Colon)?;
                let value = self.parser_expr()?;
                elements.push(MapElement { key, value });
            }

            // skip stmt eof
            self.consume(TokenType::StmtEof);
            self.must(TokenType::RightCurly)?;

            expr.node = AstNode::MapNew(elements);
            return Ok(expr);
        }

        // else is set
        let mut elements = Vec::new();
        elements.push(key_expr);

        while self.consume(TokenType::Comma) {
            let element = self.parser_expr()?;
            elements.push(element);
        }

        self.must(TokenType::RightCurly)?;
        expr.node = AstNode::SetNew(elements);

        Ok(expr)
    }

    fn parser_fndef_expr(&mut self) -> Result<Box<Expr>, SyntaxError> {
        let mut expr = self.expr_new();
        let start = self.peek().start;
        let end = self.peek().end;

        self.must(TokenType::Fn)?;

        let mut fndef = AstFnDef::default();
        fndef.start = start;
        fndef.end = end;

        // parse ident
        if self.is(TokenType::Ident) {
            let name = self.advance().literal.clone();
            fndef.symbol_name = name.clone();
            fndef.fn_name = Some(name);
        }

        self.parser_params(&mut fndef)?;

        // parse return type
        if self.consume(TokenType::Colon) {
            fndef.return_type = self.parser_type()?;
        } else {
            fndef.return_type = Type::new(TypeKind::Void);
        }

        fndef.body = self.parser_body()?;
        expr.node = AstNode::FnDef(Arc::new(Mutex::new(fndef)));

        // parse immediately call fn expr
        if self.is(TokenType::LeftParen) {
            let mut call = AstCall {
                return_type: Type::default(),
                left: expr,
                args: Vec::new(),
                generics_args: Vec::new(),
                spread: false,
            };
            call.args = self.parser_args(&mut call)?;

            let mut call_expr = self.expr_new();
            call_expr.node = AstNode::Call(call);
            return Ok(call_expr);
        }

        Ok(expr)
    }

    fn parser_new_expr(&mut self) -> Result<Box<Expr>, SyntaxError> {
        let mut expr = self.expr_new();
        self.must(TokenType::New)?;

        expr.node = AstNode::New(self.parser_type()?, Vec::new());

        Ok(expr)
    }

    fn parser_tuple_destr(&mut self) -> Result<TupleDestrExpr, SyntaxError> {
        self.must(TokenType::LeftParen)?;

        let mut elements = Vec::new();
        loop {
            let element = if self.is(TokenType::LeftParen) {
                let mut expr = self.expr_new();
                expr.node = AstNode::TupleDestr(self.parser_tuple_destr()?.elements);
                expr
            } else {
                let expr = self.parser_expr()?;

                // 检查表达式是否可赋值
                if !expr.node.can_assign() {
                    return Err(SyntaxError(
                        self.peek().start,
                        self.peek().end,
                        "tuple destr src operand assign failed".to_string(),
                    ));
                }
                expr
            };

            elements.push(element);

            if !self.consume(TokenType::Comma) {
                break;
            }
        }

        self.must(TokenType::RightParen)?;

        Ok(TupleDestrExpr { elements })
    }

    fn parser_var_tuple_destr(&mut self) -> Result<TupleDestrExpr, SyntaxError> {
        self.must(TokenType::LeftParen)?;

        let mut elements = Vec::new();
        loop {
            let element = if self.is(TokenType::LeftParen) {
                let mut expr = self.expr_new();
                expr.node = AstNode::TupleDestr(self.parser_var_tuple_destr()?.elements);
                expr
            } else {
                let ident = self.must(TokenType::Ident)?.literal.clone();
                let mut expr = self.expr_new();

                expr.node = AstNode::VarDecl(ident, Type::new(TypeKind::Unknown), false, None);
                expr
            };

            elements.push(element);

            if !self.consume(TokenType::Comma) {
                break;
            }
        }

        self.must(TokenType::RightParen)?;

        Ok(TupleDestrExpr { elements })
    }

    fn parser_var_begin_stmt(&mut self) -> Result<Box<Stmt>, SyntaxError> {
        let mut stmt = self.stmt_new();
        let type_decl = self.parser_type()?;

        // 处理 var (a, b) 形式
        if self.is(TokenType::LeftParen) {
            let tuple_destr = self.parser_var_tuple_destr()?;
            self.must(TokenType::Equal)?;
            let right = self.parser_expr()?;

            stmt.node = AstNode::VarTupleDestr(Box::new(tuple_destr), right);
            return Ok(stmt);
        }

        // 处理 var a = 1 形式
        let ident = self.must(TokenType::Ident)?.clone();
        self.must(TokenType::Equal)?;

        stmt.node = AstNode::VarDef(
            Arc::new(Mutex::new(VarDeclExpr {
                type_: type_decl,
                ident: ident.literal,
                symbol_start: ident.start,
                symbol_end: ident.end,
                be_capture: false,
                heap_ident: None,
            })),
            self.parser_expr()?,
        );

        Ok(stmt)
    }

    fn parser_type_begin_stmt(&mut self) -> Result<Box<Stmt>, SyntaxError> {
        let mut stmt = self.stmt_new();
        let type_decl = self.parser_type()?;
        let ident = self.must(TokenType::Ident)?.clone();

        // 仅 var 支持元组解构
        if self.is(TokenType::LeftParen) {
            return Err(SyntaxError(
                self.peek().start,
                self.peek().end,
                "type begin stmt not support tuple destr".to_string(),
            ));
        }

        // 声明必须赋值
        self.must(TokenType::Equal)?;

        stmt.node = AstNode::VarDef(
            Arc::new(Mutex::new(VarDeclExpr {
                type_: type_decl,
                ident: ident.literal,
                symbol_start: ident.start,
                symbol_end: ident.end,
                be_capture: false,
                heap_ident: None,
            })),
            self.parser_expr()?,
        );

        Ok(stmt)
    }

    fn is_impl_fn(&self) -> bool {
        if self.is_basic_type() {
            return true;
        }

        if self.is(TokenType::Vec) || self.is(TokenType::Map) || self.is(TokenType::Set) {
            return true;
        }

        if self.is(TokenType::Chan) {
            return true;
        }

        if self.is(TokenType::Ident) && self.next_is(1, TokenType::Dot) {
            return true;
        }

        if self.is(TokenType::Ident) && self.next_is(1, TokenType::LeftParen) {
            return false;
        }

        if self.is(TokenType::Ident) && self.next_is(1, TokenType::LeftAngle) {
            let mut close = 1;
            let mut pos = self.current + 1;
            let current_line = self.tokens[pos].line;

            while pos < self.tokens.len() {
                let t = &self.tokens[pos];

                if t.token_type == TokenType::Eof || t.token_type == TokenType::StmtEof || t.line != current_line {
                    break;
                }

                if t.token_type == TokenType::LeftAngle {
                    close += 1;
                }

                if t.token_type == TokenType::RightAngle {
                    close -= 1;
                    if close == 0 {
                        break;
                    }
                }

                pos += 1;
            }

            if close > 0 {
                return false;
            }

            let next = &self.tokens[pos + 1];
            if next.token_type == TokenType::Dot {
                return true;
            }

            if next.token_type == TokenType::LeftParen {
                return false;
            }
        }

        false
    }

    fn is_impl_type(&mut self, kind: &TypeKind) -> bool {
        matches!(
            kind,
            TypeKind::String
                | TypeKind::Bool
                | TypeKind::Int
                | TypeKind::Uint
                | TypeKind::Int8
                | TypeKind::Int16
                | TypeKind::Int32
                | TypeKind::Int64
                | TypeKind::Uint8
                | TypeKind::Uint16
                | TypeKind::Uint32
                | TypeKind::Uint64
                | TypeKind::Float
                | TypeKind::Float32
                | TypeKind::Float64
                | TypeKind::Chan(..)
                | TypeKind::Vec(..)
                | TypeKind::Map(..)
                | TypeKind::Set(..)
                | TypeKind::Tuple(..)
                | TypeKind::Alias(..)
        )
    }

    fn parser_fndef_stmt(&mut self, mut fndef: AstFnDef) -> Result<Box<Stmt>, SyntaxError> {
        let mut stmt = self.stmt_new();
        fndef.start = self.peek().start;
        self.must(TokenType::Fn)?;

        // 检查是否是类型实现函数
        let is_impl_type = if self.is_impl_fn() {
            let temp_current = self.current; // 回退位置

            let first_token = self.advance().clone();

            // 处理泛型参数
            if self.consume(TokenType::LeftAngle) {
                self.type_params_table = HashMap::new();
                fndef.generics_params = Some(Vec::new());

                loop {
                    let ident = self.advance().clone();

                    let mut param = GenericsParam::new(ident.literal.clone());

                    // 处理泛型约束 <T:t1|t2, U:t1|t2>
                    if self.consume(TokenType::Colon) {
                        param.constraints.0 = false;
                        loop {
                            let t = self.parser_single_type()?;
                            param.constraints.1.push(t);
                            if !self.consume(TokenType::Or) {
                                break;
                            }
                        }
                    }

                    if let Some(params) = &mut fndef.generics_params {
                        params.push(param);
                    }

                    self.type_params_table
                        .insert(ident.literal.clone(), ident.literal.clone());

                    if !self.consume(TokenType::Comma) {
                        break;
                    }
                }

                self.must(TokenType::RightAngle)?;
            }

            self.current = temp_current;

            // 解析实现类型
            let impl_type = if first_token.token_type == TokenType::Ident {
                let mut t = Type::default();
                t.kind = TypeKind::Alias(Box::new(TypeAlias {
                    import_as: None,
                    ident: first_token.literal.clone(),
                    args: None,
                }));
                t.impl_ident = Some(self.must(TokenType::Ident)?.literal.clone());

                if fndef.generics_params.is_some() {
                    self.must(TokenType::LeftAngle)?;
                    let mut args = Vec::new();

                    loop {
                        let param_type = self.parser_single_type()?;
                        assert!(matches!(param_type.kind, TypeKind::Param(_)));

                        if self.consume(TokenType::Colon) {
                            loop {
                                self.parser_single_type()?;
                                if !self.consume(TokenType::Or) {
                                    break;
                                }
                            }
                        }
                        args.push(param_type);

                        if !self.consume(TokenType::Comma) {
                            break;
                        }
                    }

                    self.must(TokenType::RightAngle)?;

                    if let TypeKind::Alias(alias) = &mut t.kind {
                        alias.args = Some(args);
                    }
                }
                t
            } else {
                let mut t = self.parser_single_type()?;
                t.impl_ident = Some(first_token.literal.clone());
                t
            };

            // 类型检查
            if !self.is_impl_type(&impl_type.kind) {
                return Err(SyntaxError(
                    self.peek().start,
                    self.peek().end,
                    format!("type '{}' cannot impl fn", impl_type.kind),
                ));
            }

            fndef.impl_type = impl_type;
            self.must(TokenType::Dot)?;

            true
        } else {
            false
        };

        // 处理函数名
        let ident = self.must(TokenType::Ident)?;
        fndef.symbol_name = ident.literal.clone();
        fndef.fn_name = Some(ident.literal.clone());

        // 处理非实现类型的泛型参数
        if !is_impl_type && self.consume(TokenType::LeftAngle) {
            self.type_params_table = HashMap::new();
            fndef.generics_params = Some(Vec::new());

            loop {
                let ident = self.advance().literal.clone();
                let mut param = GenericsParam::new(ident.clone());

                if self.consume(TokenType::Colon) {
                    param.constraints.0 = false;
                    loop {
                        let t = self.parser_single_type()?;
                        param.constraints.1.push(t);
                        if !self.consume(TokenType::Or) {
                            break;
                        }
                    }
                }

                if let Some(params) = &mut fndef.generics_params {
                    params.push(param);
                }

                self.type_params_table.insert(ident.clone(), ident.clone());

                if !self.consume(TokenType::Comma) {
                    break;
                }
            }

            self.must(TokenType::RightAngle)?;
        }

        self.parser_params(&mut fndef)?;

        // 处理返回类型
        if self.consume(TokenType::Colon) {
            fndef.return_type = self.parser_type()?;
        } else {
            fndef.return_type = Type::new(TypeKind::Void);
            fndef.return_type.start = self.peek().start;
            fndef.return_type.end = self.peek().end;
        }

        // tpl fn not body;
        if self.is_stmt_eof() {
            fndef.is_tpl = true;
            stmt.node = AstNode::FnDef(Arc::new(Mutex::new(fndef)));
            return Ok(stmt);
        }

        fndef.body = self.parser_body()?;

        fndef.end = if let Some(prev) = self.prev() {
            prev.end
        } else {
            self.peek().end
        };

        self.type_params_table = HashMap::new();

        stmt.node = AstNode::FnDef(Arc::new(Mutex::new(fndef)));
        Ok(stmt)
    }

    fn parser_fn_label(&mut self) -> Result<Box<Stmt>, SyntaxError> {
        let mut fndef = AstFnDef::default();

        while self.is(TokenType::FnLabel) {
            let token = self.must(TokenType::FnLabel)?;

            if token.literal == "linkid" {
                if self.is(TokenType::Ident) {
                    let linkto = self.must(TokenType::Ident)?;
                    fndef.linkid = Some(linkto.literal.clone());
                } else {
                    let literal = self.must(TokenType::StringLiteral)?;
                    fndef.linkid = Some(literal.literal.clone());
                }
            } else if token.literal == "local" {
                fndef.is_private = true;
            } else {
                return Err(SyntaxError(
                    token.start,
                    token.end,
                    format!("unknown fn label '{}'", token.literal),
                ));
            }
        }

        self.must(TokenType::StmtEof)?;

        self.parser_fndef_stmt(fndef)
    }

    fn parser_let_stmt(&mut self) -> Result<Box<Stmt>, SyntaxError> {
        let mut stmt = self.stmt_new();
        self.must(TokenType::Let)?;

        let expr = self.parser_expr()?;

        // 确保是 as 表达式
        if !matches!(expr.node, AstNode::As(..)) {
            return Err(SyntaxError(expr.start, expr.end, "must be 'as' expr".to_string()));
        }

        stmt.node = AstNode::Let(expr);
        Ok(stmt)
    }

    fn parser_throw_stmt(&mut self) -> Result<Box<Stmt>, SyntaxError> {
        let mut stmt = self.stmt_new();
        self.must(TokenType::Throw)?;

        stmt.node = AstNode::Throw(self.parser_expr()?);
        Ok(stmt)
    }

    fn parser_left_paren_begin_stmt(&mut self) -> Result<Box<Stmt>, SyntaxError> {
        // 保存当前位置以便回退
        let current_pos = self.current;

        // 尝试解析元组解构
        self.must(TokenType::LeftParen)?;
        let _ = self.parser_expr()?;
        let is_comma = self.is(TokenType::Comma);

        // 回退到开始位置
        self.current = current_pos;

        if is_comma {
            // 元组解构赋值语句
            let mut stmt = self.stmt_new();
            let mut left = self.expr_new();
            left.node = AstNode::TupleDestr(self.parser_tuple_destr()?.elements);

            self.must(TokenType::Equal)?;
            let right = self.parser_expr()?;

            stmt.node = AstNode::Assign(left, right);
            Ok(stmt)
        } else {
            // 普通表达式语句
            self.parser_expr_begin_stmt()
        }
    }

    fn parser_stmt(&mut self) -> Result<Box<Stmt>, SyntaxError> {
        let stmt = match self.peek().token_type {
            TokenType::Var => self.parser_var_begin_stmt()?,
            TokenType::LeftParen => self.parser_left_paren_begin_stmt()?,
            TokenType::Throw => self.parser_throw_stmt()?,
            TokenType::Let => self.parser_let_stmt()?,
            TokenType::FnLabel => self.parser_fn_label()?,
            TokenType::Ident => self.parser_expr_begin_stmt()?,
            TokenType::Fn => self.parser_fndef_stmt(AstFnDef::default())?,
            TokenType::If => self.parser_if_stmt()?,
            TokenType::For => self.parser_for_stmt()?,
            TokenType::Return => self.parser_return_stmt()?,
            TokenType::Import => self.parser_import_stmt()?,
            TokenType::Type => self.parser_type_alias_stmt()?,
            TokenType::Continue => self.parser_continue_stmt()?,
            TokenType::Break => self.parser_break_stmt()?,
            TokenType::Go => {
                let expr = self.parser_go_expr()?;
                self.fake_new(expr)
            }
            TokenType::Match => {
                let expr = self.parser_match_expr()?;
                self.fake_new(expr)
            }
            TokenType::MacroIdent => {
                let expr = self.parser_expr_with_precedence()?;
                self.fake_new(expr)
            }
            _ => {
                if self.is_type_begin_stmt() {
                    self.parser_type_begin_stmt()?
                } else {
                    return Err(SyntaxError(
                        self.peek().start,
                        self.peek().end,
                        format!("statement cannot start with '{}'", self.peek().literal),
                    ));
                }
            }
        };

        self.must_stmt_end()?;

        Ok(stmt)
    }

    fn parser_precedence_expr(
        &mut self,
        precedence: SyntaxPrecedence,
        exclude: TokenType,
    ) -> Result<Box<Expr>, SyntaxError> {
        // 读取表达式前缀
        let rule = self.find_rule(self.peek().token_type.clone());

        let prefix_fn = rule.prefix.ok_or_else(|| {
            SyntaxError(
                self.peek().start,
                self.peek().end,
                format!("<expr> expected, found '{}'", self.peek().literal),
            )
        })?;

        let mut expr = prefix_fn(self)?;

        // 前缀表达式已经处理完成，判断是否有中缀表达式
        let mut token_type = self.parser_infix_token(&expr);
        if exclude != TokenType::Eof && token_type == exclude {
            return Ok(expr);
        }

        let mut infix_rule = self.find_rule(token_type);

        while infix_rule.infix_precedence >= precedence {
            let infix_fn = if let Some(infix) = infix_rule.infix {
                infix
            } else {
                panic!("invalid infix expression");
            };

            expr = infix_fn(self, expr)?;

            token_type = self.parser_infix_token(&expr);
            if exclude != TokenType::Eof && token_type == exclude {
                return Ok(expr);
            }

            infix_rule = self.find_rule(token_type);
        }

        Ok(expr)
    }

    fn is_struct_param_new_prefix(&self, current: usize) -> bool {
        let t = &self.tokens[current];
        if t.token_type != TokenType::LeftAngle {
            return false;
        }

        let mut close = 1;
        let mut pos = current;

        while pos < self.tokens.len() {
            pos += 1;
            let t = &self.tokens[pos];

            if t.token_type == TokenType::LeftAngle {
                close += 1;
            }

            if t.token_type == TokenType::RightAngle {
                close -= 1;
                if close == 0 {
                    break;
                }
            }

            if t.token_type == TokenType::Eof {
                return false;
            }
        }

        if close > 0 {
            return false;
        }

        // next is '{' ?
        if pos + 1 >= self.tokens.len() {
            return false;
        }

        self.tokens[pos + 1].token_type == TokenType::LeftCurly
    }

    fn parser_is_struct_new_expr(&self) -> bool {
        // foo {}
        if self.is(TokenType::Ident) && self.next_is(1, TokenType::LeftCurly) {
            return true;
        }

        // foo.bar {}
        if self.is(TokenType::Ident)
            && self.next_is(1, TokenType::Dot)
            && self.next_is(2, TokenType::Ident)
            && self.next_is(3, TokenType::LeftCurly)
        {
            return true;
        }

        // foo<a, b> {}
        if self.is(TokenType::Ident) && self.next_is(1, TokenType::LeftAngle) {
            if self.is_struct_param_new_prefix(self.current + 1) {
                return true;
            }
        }

        // foo.bar<a, b> {}
        if self.is(TokenType::Ident)
            && self.next_is(1, TokenType::Dot)
            && self.next_is(2, TokenType::Ident)
            && self.next_is(3, TokenType::LeftAngle)
        {
            if self.is_struct_param_new_prefix(self.current + 3) {
                return true;
            }
        }

        false
    }

    fn parser_struct_new_expr(&mut self) -> Result<Box<Expr>, SyntaxError> {
        let t = self.parser_type()?;
        self.parser_struct_new(t)
    }

    fn parser_expr_with_precedence(&mut self) -> Result<Box<Expr>, SyntaxError> {
        self.parser_precedence_expr(SyntaxPrecedence::Assign, TokenType::Unknown)
    }

    fn parser_macro_default_expr(&mut self) -> Result<Box<Expr>, SyntaxError> {
        let mut expr = self.expr_new();
        self.must(TokenType::LeftParen)?;
        self.must(TokenType::RightParen)?;

        expr.node = AstNode::MacroDefault;
        Ok(expr)
    }

    fn parser_macro_sizeof(&mut self) -> Result<Box<Expr>, SyntaxError> {
        let mut expr = self.expr_new();
        self.must(TokenType::LeftParen)?;

        let target_type = self.parser_single_type()?;
        self.must(TokenType::RightParen)?;

        expr.node = AstNode::MacroSizeof(target_type);
        Ok(expr)
    }

    fn parser_macro_reflect_hash(&mut self) -> Result<Box<Expr>, SyntaxError> {
        let mut expr = self.expr_new();
        self.must(TokenType::LeftParen)?;

        let target_type = self.parser_single_type()?;
        self.must(TokenType::RightParen)?;

        expr.node = AstNode::MacroReflectHash(target_type);
        Ok(expr)
    }

    fn coroutine_fn_closure(&mut self, call_expr: &Box<Expr>) -> AstFnDef {
        let mut fndef = AstFnDef::default();
        fndef.is_co_async = true;
        fndef.params = Vec::new();
        fndef.return_type = Type::new(TypeKind::Void);

        let mut stmt_list = Vec::new();

        // var a = call(x, x, x)
        let mut vardef_stmt = self.stmt_new();
        vardef_stmt.node = AstNode::VarDef(
            Arc::new(Mutex::new(VarDeclExpr {
                type_: Type::new(TypeKind::Unknown),
                ident: "result".to_string(),
                symbol_start: 0,
                symbol_end: 0,
                be_capture: false,
                heap_ident: None,
            })),
            call_expr.clone(),
        );

        // co_return(&result)
        let mut call_stmt = self.stmt_new();
        let call = AstCall {
            return_type: Type::default(),
            left: Box::new(Expr::ident(fndef.start, fndef.end, "co_return".to_string())),
            args: vec![Box::new(Expr {
                node: AstNode::Unary(
                    ExprOp::La,
                    Box::new(Expr::ident(fndef.start, fndef.end, "result".to_string())),
                ),
                ..Default::default()
            })],
            generics_args: Vec::new(),
            spread: false,
        };
        call_stmt.node = AstNode::Call(call);

        stmt_list.push(vardef_stmt);
        stmt_list.push(call_stmt);
        fndef.body = stmt_list;

        fndef
    }

    fn coroutine_fn_void_closure(&mut self, call_expr: &Box<Expr>) -> AstFnDef {
        let mut fndef = AstFnDef::default();
        fndef.is_co_async = true;
        fndef.params = Vec::new();
        fndef.return_type = Type::new(TypeKind::Void);

        let mut stmt_list = Vec::new();

        // call(x, x, x)
        let mut call_stmt = self.stmt_new();
        if let AstNode::Call(call) = &call_expr.node {
            call_stmt.node = AstNode::Call(call.clone());
        }
        stmt_list.push(call_stmt);
        fndef.body = stmt_list;
        fndef
    }

    fn parser_match_expr(&mut self) -> Result<Box<Expr>, SyntaxError> {
        self.must(TokenType::Match)?;
        let mut expr = self.expr_new();
        let mut subject = None;
        let mut cases = Vec::new();

        // match ({a, b, c}) {}
        if !self.is(TokenType::LeftCurly) {
            subject = Some(self.parser_expr_with_precedence()?);
            self.match_subject = true;
        }

        self.must(TokenType::LeftCurly)?;

        while !self.consume(TokenType::RightCurly) {
            self.match_cond = true;

            let mut cond_list = Vec::new();

            if subject.is_some() {
                loop {
                    let expr = self.parser_precedence_expr(SyntaxPrecedence::Assign, TokenType::Or)?;
                    cond_list.push(expr);
                    if !self.consume(TokenType::Or) {
                        break;
                    }
                }
            } else {
                cond_list.push(self.parser_expr()?);
            }

            self.must(TokenType::RightArrow)?;
            self.match_cond = false;

            let (exec_expr, exec_body) = if self.is(TokenType::LeftCurly) {
                (None, Some(self.parser_body()?))
            } else {
                (Some(self.parser_expr()?), None)
            };

            self.must_stmt_end()?;

            cases.push(MatchCase {
                cond_list,
                handle_body: exec_body,
                handle_expr: exec_expr,
                is_default: false,
            });
        }

        self.match_subject = false;
        expr.node = AstNode::Match(subject, cases);
        Ok(expr)
    }

    fn parser_go_expr(&mut self) -> Result<Box<Expr>, SyntaxError> {
        self.must(TokenType::Go)?;
        let call_expr = self.parser_expr()?;

        // expr 的 type 必须是 call
        if !matches!(call_expr.node, AstNode::Call(_)) {
            return Err(SyntaxError(
                call_expr.start,
                call_expr.end,
                "go expr must be call".to_string(),
            ));
        }

        let mut expr = self.expr_new();
        expr.node = AstNode::MacroCoAsync(MacroCoAsyncExpr {
            origin_call: if let AstNode::Call(call) = &call_expr.node {
                Box::new(call.clone())
            } else {
                panic!("go expr must be call")
            },
            closure_fn: Box::new(self.coroutine_fn_closure(&call_expr)),
            closure_fn_void: Box::new(self.coroutine_fn_void_closure(&call_expr)),
            flag_expr: None,
            return_type: Type::new(TypeKind::Void),
        });

        Ok(expr)
    }

    fn parser_macro_co_async_expr(&mut self) -> Result<Box<Expr>, SyntaxError> {
        let mut expr = self.expr_new();
        self.must(TokenType::LeftParen)?;

        let call_expr = self.parser_expr()?;
        let mut co_async = MacroCoAsyncExpr {
            origin_call: if let AstNode::Call(call) = &call_expr.node {
                Box::new(call.clone())
            } else {
                panic!("co_async expr must be call")
            },
            closure_fn: Box::new(self.coroutine_fn_closure(&call_expr)),
            closure_fn_void: Box::new(self.coroutine_fn_void_closure(&call_expr)),
            flag_expr: None,
            return_type: Type::new(TypeKind::Void),
        };

        if self.consume(TokenType::Comma) {
            co_async.flag_expr = Some(self.parser_expr()?);
        }
        self.must(TokenType::RightParen)?;

        expr.node = AstNode::MacroCoAsync(co_async);
        Ok(expr)
    }

    fn parser_macro_ula_expr(&mut self) -> Result<Box<Expr>, SyntaxError> {
        let mut expr = self.expr_new();
        self.must(TokenType::LeftParen)?;

        let src = self.parser_expr()?;
        self.must(TokenType::RightParen)?;

        expr.node = AstNode::MacroUla(src);
        Ok(expr)
    }

    fn parser_macro_call(&mut self) -> Result<Box<Expr>, SyntaxError> {
        let token = self.must(TokenType::MacroIdent)?;

        // 根据宏名称选择对应的解析器
        match token.literal.as_str() {
            "sizeof" => self.parser_macro_sizeof(),
            "reflect_hash" => self.parser_macro_reflect_hash(),
            "default" => self.parser_macro_default_expr(),
            "co_async" => self.parser_macro_co_async_expr(),
            "ula" => self.parser_macro_ula_expr(),
            _ => Err(SyntaxError(
                token.start,
                token.end,
                format!("macro '{}' not defined", token.literal),
            )),
        }
    }

    fn parser_expr(&mut self) -> Result<Box<Expr>, SyntaxError> {
        // 根据当前 token 类型选择对应的解析器
        if self.parser_is_struct_new_expr() {
            self.parser_struct_new_expr()
        } else if self.is(TokenType::Go) {
            self.parser_go_expr()
        } else if self.is(TokenType::Match) {
            self.parser_match_expr()
        } else if self.is(TokenType::Fn) {
            self.parser_fndef_expr()
        } else if self.is(TokenType::New) {
            self.parser_new_expr()
        } else {
            self.parser_expr_with_precedence()
        }
    }
}
