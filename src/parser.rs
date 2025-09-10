use crate::ast::*;
use crate::lexer::Token;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("Unexpected token: expected {expected}, found {found:?}")]
    UnexpectedToken { expected: String, found: Token },
    #[error("Invalid expression")]
    InvalidExpression,
}

pub struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            position: 0,
        }
    }

    fn current(&self) -> &Token {
        self.tokens.get(self.position).unwrap_or(&Token::Eof)
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.position + 1).unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) {
        if self.position < self.tokens.len() {
            self.position += 1;
        }
    }

    fn expect(&mut self, expected: Token) -> Result<(), ParseError> {
        if self.current() == &expected {
            self.advance();
            Ok(())
        } else {
            // Debug: show context around the error
            if expected == Token::LeftBrace && self.current() == &Token::If {
                eprintln!(
                    "Debug: Expecting LeftBrace but found If at position {}",
                    self.position
                );
                eprintln!(
                    "Previous tokens: {:?}",
                    self.tokens
                        .get(self.position.saturating_sub(5)..self.position)
                );
                eprintln!(
                    "Next tokens: {:?}",
                    self.tokens
                        .get(self.position..self.position.saturating_add(5).min(self.tokens.len()))
                );
            }
            {
                // Debug for new error
                if (expected == Token::RightBrace || expected == Token::LeftBrace)
                    && (self.current() == &Token::ColonColon || self.current() == &Token::DotDot)
                {
                    eprintln!(
                        "Debug: Expecting {:?} but found {:?} at position {}",
                        expected,
                        self.current(),
                        self.position
                    );
                    eprintln!(
                        "Previous 5 tokens: {:?}",
                        self.tokens
                            .get(self.position.saturating_sub(5)..self.position)
                    );
                }
                {
                    if expected == Token::Colon && self.current() == &Token::Dot {
                        eprintln!(
                            "Debug: Expecting Colon but found Dot at position {}",
                            self.position
                        );
                        eprintln!(
                            "Previous 5 tokens: {:?}",
                            self.tokens
                                .get(self.position.saturating_sub(5)..self.position)
                        );
                    }
                    Err(ParseError::UnexpectedToken {
                        expected: format!("{:?}", expected),
                        found: self.current().clone(),
                    })
                }
            }
        }
    }

    fn parse_program(&mut self) -> Result<Program, ParseError> {
        let mut imports = Vec::new();
        let mut items = Vec::new();

        // Parse use statements first
        while self.current() == &Token::Use {
            imports.push(self.parse_use_statement()?);
        }

        // Then parse items
        while self.current() != &Token::Eof {
            items.push(self.parse_item()?);
        }

        Ok(Program { imports, items })
    }

    fn parse_use_statement(&mut self) -> Result<UseStatement, ParseError> {
        self.expect(Token::Use)?;

        // Parse the path (e.g., crate::module::submodule or module)
        let mut path = Vec::new();

        // Handle special path prefixes
        if self.current() == &Token::Crate {
            path.push("crate".to_string());
            self.advance();
            self.expect(Token::ColonColon)?;
        } else if self.current() == &Token::Super {
            path.push("super".to_string());
            self.advance();
            self.expect(Token::ColonColon)?;
        }

        // Parse the module path
        path.push(self.parse_ident()?);

        while self.current() == &Token::ColonColon {
            self.advance();

            // Check for glob (*) or list ({...})
            if self.current() == &Token::Star {
                self.advance();
                self.expect(Token::Semicolon)?;
                return Ok(UseStatement {
                    path,
                    items: UseTree::Glob,
                });
            } else if self.current() == &Token::LeftBrace {
                let items = self.parse_use_tree_list()?;
                self.expect(Token::Semicolon)?;
                return Ok(UseStatement {
                    path,
                    items: UseTree::List(items),
                });
            } else {
                path.push(self.parse_ident()?);
            }
        }

        // Check for alias (as)
        if self.current() == &Token::As {
            self.advance();
            let alias = self.parse_ident()?;
            let original = path.pop().unwrap();
            self.expect(Token::Semicolon)?;
            return Ok(UseStatement {
                path,
                items: UseTree::Alias(original, alias),
            });
        }

        // Simple import
        let item = path.pop().unwrap();
        self.expect(Token::Semicolon)?;
        Ok(UseStatement {
            path,
            items: UseTree::Simple(item),
        })
    }

    fn parse_use_tree_list(&mut self) -> Result<Vec<UseTree>, ParseError> {
        self.expect(Token::LeftBrace)?;
        let mut items = Vec::new();

        loop {
            if self.current() == &Token::RightBrace {
                break;
            }

            let name = self.parse_ident()?;

            // Check for alias
            if self.current() == &Token::As {
                self.advance();
                let alias = self.parse_ident()?;
                items.push(UseTree::Alias(name, alias));
            } else {
                items.push(UseTree::Simple(name));
            }

            if self.current() == &Token::Comma {
                self.advance();
            } else {
                break;
            }
        }

        self.expect(Token::RightBrace)?;
        Ok(items)
    }

    fn parse_item(&mut self) -> Result<Item, ParseError> {
        match self.current() {
            Token::Struct => self.parse_struct().map(Item::Struct),
            Token::Enum => self.parse_enum().map(Item::Enum),
            Token::Trait => self.parse_trait().map(Item::Trait),
            Token::Impl => self.parse_impl().map(Item::Impl),
            Token::Fn => self.parse_function().map(Item::Function),
            Token::Const => self.parse_const().map(Item::Const),
            Token::Let => {
                // Parse global variable as a statement wrapped in Item::Global
                let let_stmt = self.parse_let_statement()?;
                Ok(Item::Global(Statement::Let(let_stmt)))
            }
            _ => Err(ParseError::UnexpectedToken {
                expected: "item".to_string(),
                found: self.current().clone(),
            }),
        }
    }

    fn parse_struct(&mut self) -> Result<Struct, ParseError> {
        self.expect(Token::Struct)?;
        let name = self.parse_ident()?;
        let generics = self.parse_generics()?;
        self.expect(Token::LeftBrace)?;

        let mut fields = Vec::new();
        while self.current() != &Token::RightBrace {
            let field_name = self.parse_ident()?;
            self.expect(Token::Colon)?;
            let ty = self.parse_type()?;
            fields.push(Field {
                name: field_name,
                ty,
            });

            if self.current() == &Token::Comma {
                self.advance();
            } else {
                break;
            }
        }

        self.expect(Token::RightBrace)?;

        Ok(Struct {
            name,
            generics,
            fields,
        })
    }

    fn parse_enum(&mut self) -> Result<Enum, ParseError> {
        self.expect(Token::Enum)?;
        let name = self.parse_ident()?;
        let generics = self.parse_generics()?;
        self.expect(Token::LeftBrace)?;

        let mut variants = Vec::new();
        while self.current() != &Token::RightBrace {
            let variant_name = self.parse_ident()?;
            let fields = if self.current() == &Token::LeftParen {
                self.advance();
                let mut tuple_fields = Vec::new();
                while self.current() != &Token::RightParen {
                    tuple_fields.push(self.parse_type()?);
                    if self.current() == &Token::Comma {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(Token::RightParen)?;
                VariantFields::Tuple(tuple_fields)
            } else if self.current() == &Token::LeftBrace {
                self.advance();
                let mut named_fields = Vec::new();
                while self.current() != &Token::RightBrace {
                    let field_name = self.parse_ident()?;
                    self.expect(Token::Colon)?;
                    let ty = self.parse_type()?;
                    named_fields.push(Field {
                        name: field_name,
                        ty,
                    });
                    if self.current() == &Token::Comma {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(Token::RightBrace)?;
                VariantFields::Named(named_fields)
            } else {
                VariantFields::Unit
            };

            variants.push(Variant {
                name: variant_name,
                fields,
            });

            if self.current() == &Token::Comma {
                self.advance();
            } else {
                break;
            }
        }

        self.expect(Token::RightBrace)?;

        Ok(Enum {
            name,
            generics,
            variants,
        })
    }

    fn parse_trait(&mut self) -> Result<Trait, ParseError> {
        self.expect(Token::Trait)?;
        let name = self.parse_ident()?;
        let generics = self.parse_generics()?;
        self.expect(Token::LeftBrace)?;

        let mut methods = Vec::new();
        while self.current() != &Token::RightBrace {
            self.expect(Token::Fn)?;
            let method_name = self.parse_ident()?;
            self.expect(Token::LeftParen)?;
            let params = self.parse_params()?;
            self.expect(Token::RightParen)?;

            let return_type = if self.current() == &Token::Arrow {
                self.advance();
                Some(self.parse_type()?)
            } else {
                None
            };

            let body = if self.current() == &Token::LeftBrace {
                Some(self.parse_block()?)
            } else {
                self.expect(Token::Semicolon)?;
                None
            };

            methods.push(TraitMethod {
                name: method_name,
                params,
                return_type,
                body,
            });
        }

        self.expect(Token::RightBrace)?;

        Ok(Trait {
            name,
            generics,
            methods,
        })
    }

    fn parse_impl(&mut self) -> Result<Impl, ParseError> {
        self.expect(Token::Impl)?;
        let generics = self.parse_generics()?;

        let (trait_name, target_type) = if self.peek() == &Token::For {
            let trait_name = self.parse_ident()?;
            self.expect(Token::For)?;
            let target_type = self.parse_type()?;
            (Some(trait_name), target_type)
        } else {
            let target_type = self.parse_type()?;
            (None, target_type)
        };

        self.expect(Token::LeftBrace)?;

        let mut methods = Vec::new();
        while self.current() != &Token::RightBrace {
            methods.push(self.parse_function()?);
        }

        self.expect(Token::RightBrace)?;

        Ok(Impl {
            generics,
            trait_name,
            target_type,
            methods,
        })
    }

    fn parse_function(&mut self) -> Result<Function, ParseError> {
        self.expect(Token::Fn)?;
        let name = self.parse_ident()?;
        let generics = self.parse_generics()?;
        self.expect(Token::LeftParen)?;
        let params = self.parse_params()?;
        self.expect(Token::RightParen)?;

        let return_type = if self.current() == &Token::Arrow {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };

        let body = self.parse_block()?;

        Ok(Function {
            name,
            generics,
            params,
            return_type,
            body,
        })
    }

    fn parse_const(&mut self) -> Result<Const, ParseError> {
        self.expect(Token::Const)?;
        let name = self.parse_ident()?;
        self.expect(Token::Colon)?;
        let ty = self.parse_type()?;
        self.expect(Token::Eq)?;
        let value = self.parse_expr()?;
        self.expect(Token::Semicolon)?;

        Ok(Const { name, ty, value })
    }

    fn parse_generics(&mut self) -> Result<Vec<String>, ParseError> {
        if self.current() != &Token::Lt {
            return Ok(Vec::new());
        }

        self.advance();
        let mut generics = Vec::new();

        while self.current() != &Token::Gt {
            generics.push(self.parse_ident()?);
            if self.current() == &Token::Comma {
                self.advance();
            } else {
                break;
            }
        }

        self.expect(Token::Gt)?;
        Ok(generics)
    }

    fn parse_params(&mut self) -> Result<Vec<Param>, ParseError> {
        let mut params = Vec::new();

        while self.current() != &Token::RightParen {
            let (name, is_self, is_mut) = if self.current() == &Token::Self_ {
                self.advance();
                ("self".to_string(), true, false)
            } else if self.current() == &Token::Ampersand {
                self.advance();
                let is_mut = if self.current() == &Token::Mut {
                    self.advance();
                    true
                } else {
                    false
                };
                self.expect(Token::Self_)?;
                ("self".to_string(), true, is_mut)
            } else {
                let is_mut = if self.current() == &Token::Mut {
                    self.advance();
                    true
                } else {
                    false
                };
                let name = self.parse_ident()?;
                (name, false, is_mut)
            };

            let ty = if is_self && self.current() != &Token::Colon {
                Type::Path("Self".to_string())
            } else {
                self.expect(Token::Colon)?;
                self.parse_type()?
            };

            params.push(Param {
                name,
                ty,
                is_self,
                is_mut,
            });

            if self.current() == &Token::Comma {
                self.advance();
            } else {
                break;
            }
        }

        Ok(params)
    }

    fn parse_type(&mut self) -> Result<Type, ParseError> {
        if self.current() == &Token::Ampersand {
            self.advance();
            let is_mut = if self.current() == &Token::Mut {
                self.advance();
                true
            } else {
                false
            };
            let inner = self.parse_type()?;
            return Ok(Type::Reference(Box::new(inner), is_mut));
        }

        if self.current() == &Token::LeftParen {
            self.advance();
            let mut types = Vec::new();
            while self.current() != &Token::RightParen {
                types.push(self.parse_type()?);
                if self.current() == &Token::Comma {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect(Token::RightParen)?;
            return Ok(Type::Tuple(types));
        }

        if self.current() == &Token::LeftBracket {
            self.advance();
            let elem_type = self.parse_type()?;
            self.expect(Token::Semicolon)?;
            let size = match self.current() {
                Token::IntLiteral(n) => *n as usize,
                _ => {
                    return Err(ParseError::UnexpectedToken {
                        expected: "array size".to_string(),
                        found: self.current().clone(),
                    })
                }
            };
            self.advance();
            self.expect(Token::RightBracket)?;
            return Ok(Type::Array(Box::new(elem_type), size));
        }

        let name = self.parse_ident()?;

        if self.current() == &Token::Lt {
            self.advance();
            let mut args = Vec::new();
            while self.current() != &Token::Gt {
                args.push(self.parse_type()?);
                if self.current() == &Token::Comma {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect(Token::Gt)?;
            Ok(Type::Generic(name, args))
        } else {
            Ok(Type::Path(name))
        }
    }

    fn parse_block(&mut self) -> Result<Block, ParseError> {
        self.expect(Token::LeftBrace)?;
        let mut statements = Vec::new();

        while self.current() != &Token::RightBrace {
            statements.push(self.parse_statement()?);
        }

        self.expect(Token::RightBrace)?;
        Ok(Block { statements })
    }

    fn parse_statement(&mut self) -> Result<Statement, ParseError> {
        match self.current() {
            Token::Let => self.parse_let_statement().map(Statement::Let),
            Token::Return => {
                self.advance();
                let expr = if self.current() == &Token::Semicolon {
                    None
                } else {
                    Some(self.parse_expr()?)
                };
                self.expect(Token::Semicolon)?;
                Ok(Statement::Return(expr))
            }
            Token::If => self.parse_if_statement().map(Statement::If),
            Token::While => self.parse_while_statement().map(Statement::While),
            Token::For => self.parse_for_statement().map(Statement::For),
            Token::Match => self.parse_match_statement().map(Statement::Match),
            _ => {
                // Try to parse assignment or expression statement.
                let expr = self.parse_expr()?;

                // Check if this is an assignment.
                if self.current() == &Token::Eq {
                    self.advance();
                    let rhs = self.parse_expr()?;
                    if self.current() == &Token::Semicolon {
                        self.advance();
                    }
                    Ok(Statement::Assign(expr, rhs))
                } else {
                    if self.current() == &Token::Semicolon {
                        self.advance();
                    }
                    Ok(Statement::Expr(expr))
                }
            }
        }
    }

    fn parse_let_statement(&mut self) -> Result<LetStatement, ParseError> {
        self.expect(Token::Let)?;
        let is_mut = if self.current() == &Token::Mut {
            self.advance();
            true
        } else {
            false
        };

        let name = self.parse_ident()?;

        let ty = if self.current() == &Token::Colon {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };

        let value = if self.current() == &Token::Eq {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };

        self.expect(Token::Semicolon)?;

        Ok(LetStatement {
            name,
            ty,
            value,
            is_mut,
        })
    }

    fn parse_if_statement(&mut self) -> Result<IfStatement, ParseError> {
        self.expect(Token::If)?;
        let condition = self.parse_expr()?;
        let then_branch = self.parse_block()?;

        let else_branch = if self.current() == &Token::Else {
            self.advance();
            if self.current() == &Token::If {
                // Handle 'else if' by parsing another if statement
                let if_stmt = self.parse_if_statement()?;
                // Wrap the if statement in a block with a single statement
                Some(Block {
                    statements: vec![Statement::If(if_stmt)],
                })
            } else {
                Some(self.parse_block()?)
            }
        } else {
            None
        };

        Ok(IfStatement {
            condition,
            then_branch,
            else_branch,
        })
    }

    fn parse_while_statement(&mut self) -> Result<WhileStatement, ParseError> {
        self.expect(Token::While)?;
        let condition = self.parse_expr()?;
        let body = self.parse_block()?;

        Ok(WhileStatement { condition, body })
    }

    fn parse_for_statement(&mut self) -> Result<ForStatement, ParseError> {
        self.expect(Token::For)?;
        let var = self.parse_ident()?;
        self.expect(Token::In)?;
        let iter = self.parse_expr()?;
        let body = self.parse_block()?;

        Ok(ForStatement { var, iter, body })
    }

    fn parse_match_statement(&mut self) -> Result<MatchStatement, ParseError> {
        self.expect(Token::Match)?;
        let expr = self.parse_expr()?;
        self.expect(Token::LeftBrace)?;

        let mut arms = Vec::new();
        while self.current() != &Token::RightBrace {
            let pattern = self.parse_pattern()?;
            self.expect(Token::FatArrow)?;
            let body = self.parse_expr()?;
            arms.push(MatchArm { pattern, body });

            if self.current() == &Token::Comma {
                self.advance();
            }
        }

        self.expect(Token::RightBrace)?;

        Ok(MatchStatement { expr, arms })
    }

    fn parse_pattern(&mut self) -> Result<Pattern, ParseError> {
        match self.current() {
            Token::Underscore => {
                self.advance();
                Ok(Pattern::Wildcard)
            }
            Token::IntLiteral(n) => {
                let n = *n;
                self.advance();
                Ok(Pattern::Literal(Literal::Int(n)))
            }
            Token::FloatLiteral(f) => {
                let f = *f;
                self.advance();
                Ok(Pattern::Literal(Literal::Float(f)))
            }
            Token::BoolLiteral(b) => {
                let b = *b;
                self.advance();
                Ok(Pattern::Literal(Literal::Bool(b)))
            }
            Token::StringLiteral(s) => {
                let s = s.clone();
                self.advance();
                Ok(Pattern::Literal(Literal::String(s)))
            }
            Token::CharLiteral(c) => {
                let c = *c;
                self.advance();
                Ok(Pattern::Literal(Literal::Char(c)))
            }
            Token::LeftParen => {
                self.advance();
                let mut patterns = Vec::new();
                while self.current() != &Token::RightParen {
                    patterns.push(self.parse_pattern()?);
                    if self.current() == &Token::Comma {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(Token::RightParen)?;
                Ok(Pattern::Tuple(patterns))
            }
            Token::Ident(_) => {
                let name = self.parse_ident()?;

                if self.current() == &Token::ColonColon {
                    self.advance();
                    let variant = self.parse_ident()?;
                    let inner = if self.current() == &Token::LeftParen {
                        self.advance();
                        let pattern = self.parse_pattern()?;
                        self.expect(Token::RightParen)?;
                        Some(Box::new(pattern))
                    } else {
                        None
                    };
                    Ok(Pattern::Enum(name, variant, inner))
                } else if self.current() == &Token::LeftBrace {
                    self.advance();
                    let mut fields = Vec::new();
                    while self.current() != &Token::RightBrace {
                        let field_name = self.parse_ident()?;
                        self.expect(Token::Colon)?;
                        let pattern = self.parse_pattern()?;
                        fields.push((field_name, pattern));
                        if self.current() == &Token::Comma {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    self.expect(Token::RightBrace)?;
                    Ok(Pattern::Struct(name, fields))
                } else {
                    Ok(Pattern::Ident(name))
                }
            }
            _ => Err(ParseError::UnexpectedToken {
                expected: "pattern".to_string(),
                found: self.current().clone(),
            }),
        }
    }

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_or_expr()
    }

    fn parse_or_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_and_expr()?;

        while self.current() == &Token::OrOr {
            self.advance();
            let right = self.parse_and_expr()?;
            left = Expr::Binary(BinaryOp::Or, Box::new(left), Box::new(right));
        }

        Ok(left)
    }

    fn parse_and_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_bitwise_or_expr()?;

        while self.current() == &Token::AndAnd {
            self.advance();
            let right = self.parse_bitwise_or_expr()?;
            left = Expr::Binary(BinaryOp::And, Box::new(left), Box::new(right));
        }

        Ok(left)
    }

    fn parse_bitwise_or_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_bitwise_xor_expr()?;

        while self.current() == &Token::Pipe {
            self.advance();
            let right = self.parse_bitwise_xor_expr()?;
            left = Expr::Binary(BinaryOp::BitOr, Box::new(left), Box::new(right));
        }

        Ok(left)
    }

    fn parse_bitwise_xor_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_bitwise_and_expr()?;

        while self.current() == &Token::Caret {
            self.advance();
            let right = self.parse_bitwise_and_expr()?;
            left = Expr::Binary(BinaryOp::BitXor, Box::new(left), Box::new(right));
        }

        Ok(left)
    }

    fn parse_bitwise_and_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_equality_expr()?;

        while self.current() == &Token::Ampersand {
            self.advance();
            let right = self.parse_equality_expr()?;
            left = Expr::Binary(BinaryOp::BitAnd, Box::new(left), Box::new(right));
        }

        Ok(left)
    }

    fn parse_equality_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_relational_expr()?;

        loop {
            let op = match self.current() {
                Token::EqEq => BinaryOp::Eq,
                Token::Ne => BinaryOp::Ne,
                _ => break,
            };
            self.advance();
            let right = self.parse_relational_expr()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }

        Ok(left)
    }

    fn parse_relational_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_shift_expr()?;

        loop {
            let op = match self.current() {
                Token::Lt => BinaryOp::Lt,
                Token::Le => BinaryOp::Le,
                Token::Gt => BinaryOp::Gt,
                Token::Ge => BinaryOp::Ge,
                _ => break,
            };
            self.advance();
            let right = self.parse_shift_expr()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }

        Ok(left)
    }

    fn parse_shift_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_additive_expr()?;

        loop {
            let op = match self.current() {
                Token::Shl => BinaryOp::Shl,
                Token::Shr => BinaryOp::Shr,
                _ => break,
            };
            self.advance();
            let right = self.parse_additive_expr()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }

        Ok(left)
    }

    fn parse_additive_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_multiplicative_expr()?;

        loop {
            let op = match self.current() {
                Token::Plus => BinaryOp::Add,
                Token::Minus => BinaryOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplicative_expr()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }

        Ok(left)
    }

    fn parse_multiplicative_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_unary_expr()?;

        loop {
            let op = match self.current() {
                Token::Star => BinaryOp::Mul,
                Token::Slash => BinaryOp::Div,
                Token::Percent => BinaryOp::Mod,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary_expr()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }

        Ok(left)
    }

    fn parse_unary_expr(&mut self) -> Result<Expr, ParseError> {
        match self.current() {
            Token::Bang => {
                self.advance();
                let expr = self.parse_unary_expr()?;
                Ok(Expr::Unary(UnaryOp::Not, Box::new(expr)))
            }
            Token::Minus => {
                self.advance();
                let expr = self.parse_unary_expr()?;
                Ok(Expr::Unary(UnaryOp::Neg, Box::new(expr)))
            }
            Token::Ampersand => {
                self.advance();
                let expr = self.parse_unary_expr()?;
                // For now, treat &expr as just expr (Pico-8 Lua doesn't have references)
                // In a full implementation, we'd have a Ref unary operator
                Ok(expr)
            }
            _ => self.parse_postfix_expr(),
        }
    }

    fn parse_postfix_expr(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_primary_expr()?;

        loop {
            match self.current() {
                Token::Dot => {
                    self.advance();
                    if self.peek() == &Token::LeftParen {
                        let method = self.parse_ident()?;
                        self.expect(Token::LeftParen)?;
                        let args = self.parse_args()?;
                        self.expect(Token::RightParen)?;
                        expr = Expr::MethodCall(Box::new(expr), method, args);
                    } else {
                        let field = self.parse_ident()?;
                        expr = Expr::Field(Box::new(expr), field);
                    }
                }
                Token::LeftBracket => {
                    self.advance();
                    let index = self.parse_expr()?;
                    self.expect(Token::RightBracket)?;
                    expr = Expr::Index(Box::new(expr), Box::new(index));
                }
                Token::LeftParen => {
                    self.advance();
                    let args = self.parse_args()?;
                    self.expect(Token::RightParen)?;
                    expr = Expr::Call(Box::new(expr), args);
                }
                Token::As => {
                    self.advance();
                    // Parse the target type
                    let _target_type = self.parse_type()?;
                    // For now, just return the expression unchanged
                    // Pico-8 Lua doesn't have type casting anyway
                    // In a full implementation, we'd have a Cast expression type
                }
                _ => break,
            }
        }

        Ok(expr)
    }

    fn parse_primary_expr(&mut self) -> Result<Expr, ParseError> {
        match self.current() {
            Token::IntLiteral(n) => {
                let n = *n;
                self.advance();

                // Check for range (e.g., 0..16)
                if self.current() == &Token::DotDot {
                    self.advance();
                    if matches!(
                        self.current(),
                        Token::IntLiteral(_) | Token::Ident(_) | Token::LeftParen
                    ) {
                        let end = self.parse_expr()?;
                        Ok(Expr::Range(
                            Some(Box::new(Expr::Literal(Literal::Int(n)))),
                            Some(Box::new(end)),
                        ))
                    } else {
                        Ok(Expr::Range(
                            Some(Box::new(Expr::Literal(Literal::Int(n)))),
                            None,
                        ))
                    }
                } else {
                    Ok(Expr::Literal(Literal::Int(n)))
                }
            }
            Token::FloatLiteral(f) => {
                let f = *f;
                self.advance();
                Ok(Expr::Literal(Literal::Float(f)))
            }
            Token::BoolLiteral(b) => {
                let b = *b;
                self.advance();
                Ok(Expr::Literal(Literal::Bool(b)))
            }
            Token::StringLiteral(s) => {
                let s = s.clone();
                self.advance();
                Ok(Expr::Literal(Literal::String(s)))
            }
            Token::CharLiteral(c) => {
                let c = *c;
                self.advance();
                Ok(Expr::Literal(Literal::Char(c)))
            }
            Token::LeftParen => {
                self.advance();
                let mut exprs = Vec::new();
                while self.current() != &Token::RightParen {
                    exprs.push(self.parse_expr()?);
                    if self.current() == &Token::Comma {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(Token::RightParen)?;
                if exprs.len() == 1 {
                    Ok(exprs.into_iter().next().unwrap())
                } else {
                    Ok(Expr::Tuple(exprs))
                }
            }
            Token::LeftBracket => {
                self.advance();
                let mut elements = Vec::new();
                while self.current() != &Token::RightBracket {
                    elements.push(self.parse_expr()?);
                    if self.current() == &Token::Comma {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(Token::RightBracket)?;
                Ok(Expr::Array(elements))
            }
            Token::LeftBrace => {
                let block = self.parse_block()?;
                Ok(Expr::Block(block))
            }
            Token::If => {
                let if_stmt = self.parse_if_statement()?;
                Ok(Expr::If(Box::new(if_stmt)))
            }
            Token::Match => {
                let match_stmt = self.parse_match_statement()?;
                Ok(Expr::Match(Box::new(match_stmt)))
            }
            Token::Self_ => {
                self.advance();
                Ok(Expr::Ident("self".to_string()))
            }
            Token::Ident(name) => {
                let name = name.clone();
                self.advance();

                // Check for Option types
                if name == "None" {
                    return Ok(Expr::None);
                } else if name == "Some" {
                    self.expect(Token::LeftParen)?;
                    let value = self.parse_expr()?;
                    self.expect(Token::RightParen)?;
                    return Ok(Expr::Some(Box::new(value)));
                }

                // Check for path (e.g., GameState::Title)
                if self.current() == &Token::ColonColon {
                    self.advance();
                    let variant = self.parse_ident()?;
                    // For now, treat EnumName::Variant as a simple identifier
                    // In a full implementation, we'd have a Path expression type
                    return Ok(Expr::Ident(format!("{}::{}", name, variant)));
                }

                if self.current() == &Token::LeftBrace {
                    // Look ahead to see if this is really a struct literal
                    // Struct literals have the pattern: Name { field: value, ... }
                    // We need to distinguish from other uses of { after an identifier

                    // Save position in case we need to backtrack
                    let saved_pos = self.position;
                    self.advance(); // consume {

                    // Check if the next tokens look like a struct literal
                    // If we see "ident :" it's likely a struct literal
                    // If we see something else, it's not
                    let is_struct_literal = if let Token::Ident(_) = self.current() {
                        let next_pos = self.position;
                        self.advance();
                        let has_colon = self.current() == &Token::Colon;
                        self.position = next_pos; // restore position
                        has_colon
                    } else {
                        false
                    };

                    if is_struct_literal {
                        let mut fields = Vec::new();
                        while self.current() != &Token::RightBrace {
                            let field_name = self.parse_ident()?;
                            self.expect(Token::Colon)?;
                            let value = self.parse_expr()?;
                            fields.push((field_name, value));
                            if self.current() == &Token::Comma {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                        self.expect(Token::RightBrace)?;
                        Ok(Expr::Struct(name, fields))
                    } else {
                        // Not a struct literal, backtrack
                        self.position = saved_pos;
                        Ok(Expr::Ident(name))
                    }
                } else if self.current() == &Token::DotDot {
                    self.advance();
                    if matches!(
                        self.current(),
                        Token::IntLiteral(_) | Token::Ident(_) | Token::LeftParen
                    ) {
                        let end = self.parse_expr()?;
                        Ok(Expr::Range(
                            Some(Box::new(Expr::Ident(name))),
                            Some(Box::new(end)),
                        ))
                    } else {
                        Ok(Expr::Range(Some(Box::new(Expr::Ident(name))), None))
                    }
                } else {
                    Ok(Expr::Ident(name))
                }
            }
            Token::DotDot => {
                self.advance();
                if matches!(
                    self.current(),
                    Token::IntLiteral(_) | Token::Ident(_) | Token::LeftParen
                ) {
                    let end = self.parse_expr()?;
                    Ok(Expr::Range(None, Some(Box::new(end))))
                } else {
                    Ok(Expr::Range(None, None))
                }
            }
            _ => {
                eprintln!(
                    "Invalid expression: unexpected token {:?} at position {}",
                    self.current(),
                    self.position
                );
                Err(ParseError::InvalidExpression)
            }
        }
    }

    fn parse_args(&mut self) -> Result<Vec<Expr>, ParseError> {
        let mut args = Vec::new();

        while self.current() != &Token::RightParen {
            args.push(self.parse_expr()?);
            if self.current() == &Token::Comma {
                self.advance();
            } else {
                break;
            }
        }

        Ok(args)
    }

    fn parse_ident(&mut self) -> Result<String, ParseError> {
        match self.current() {
            Token::Ident(name) => {
                let name = name.clone();
                self.advance();
                Ok(name)
            }
            _ => Err(ParseError::UnexpectedToken {
                expected: "identifier".to_string(),
                found: self.current().clone(),
            }),
        }
    }
}

pub fn parse(tokens: Vec<Token>) -> Result<Program, ParseError> {
    let mut parser = Parser::new(tokens);
    parser.parse_program()
}
