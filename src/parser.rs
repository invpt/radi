use crate::{
    char_reader::CharReader,
    errors::{CompilationErrorKind, ErrorStream},
    tokenizer::{Intern, Span, Token, TokenKind, TokenizationError, Tokens},
};

mod ast;
mod preds;
pub mod utils;

pub use ast::*;
use preds::*;

#[derive(Debug)]
pub struct ParseError<'s> {
    pub kind: ParseErrorKind<'s>,
    pub span: Option<Span>,
}

#[derive(Debug)]
pub enum ParseErrorKind<'s> {
    Unexpected(Option<Token<'s>>),
    TokenizationError(TokenizationError),
}

impl<'s> From<TokenizationError> for ParseError<'s> {
    fn from(err: TokenizationError) -> Self {
        ParseError {
            span: err.span,
            kind: ParseErrorKind::TokenizationError(err),
        }
    }
}

type Result<'s, T> = std::result::Result<T, ParseError<'s>>;

pub fn parse<'s>(
    tokens: Tokens<'s, impl CharReader>,
    errors: &'s ErrorStream<'s>,
) -> Result<'s, Expr<'s>> {
    Parser { tokens, errors }.parse()
}

struct Parser<'s, R> {
    tokens: Tokens<'s, R>,
    errors: &'s ErrorStream<'s>,
}

impl<'s, R: CharReader> Parser<'s, R> {
    fn parse(mut self) -> Result<'s, Expr<'s>> {
        let definitions = self.object_body(vpred!())?;

        if let (Some(first), Some(last)) = (definitions.first(), definitions.last()) {
            let span = Span {
                start: first.span.start,
                end: last.span.end,
            };

            Ok(Expr {
                kind: ExprKind::Object { definitions },
                span,
            })
        } else {
            Ok(Expr {
                kind: ExprKind::Object {
                    definitions: Box::new([]),
                },
                span: Span { start: 0, end: 0 },
            })
        }
    }

    fn object_body(
        &mut self,
        end_pred: impl Fn(&Token<'s>) -> Option<()>,
    ) -> Result<'s, Box<[Def<'s>]>> {
        let mut defs = Vec::new();

        while let Some(None) = self.tokens.peek()?.map(&end_pred) {
            defs.push(self.def()?)
        }

        Ok(defs.into())
    }

    fn def(&mut self) -> Result<'s, Def<'s>> {
        let Token {
            span: Span { start, .. },
            ..
        } = self.require(tpred!(TokenKind::Def))?;
        let name = self.require(vpred!(TokenKind::Name(n) => n))?;
        let (value, needs_semi) = self.def_block()?;
        let end = if needs_semi {
            self.require(vpred!(:t: TokenKind::Semicolon => t.span.end))?
        } else {
            value.span.end
        };

        Ok(Def {
            name,
            value: Box::new(value),
            span: Span { start, end },
        })
    }

    fn def_block(&mut self) -> Result<'s, (Expr<'s>, bool)> {
        if let Some(open) = self.eat(tpred!(TokenKind::OpenBrace))? {
            Ok((
                self.scope(open.span.start, tpred!(TokenKind::CloseBrace))?,
                false,
            ))
        } else if let Some(open) = self.eat(tpred!(TokenKind::DotOpenBrace))? {
            let body_defs = self.object_body(bpred!(TokenKind::CloseBrace))?;
            let close = self.require(tpred!(TokenKind::CloseBrace))?;

            Ok((
                Expr {
                    span: Span {
                        start: open.span.start,
                        end: close.span.end,
                    },
                    kind: ExprKind::Object {
                        definitions: body_defs,
                    },
                },
                false,
            ))
        } else {
            Ok(self.def_expr()?)
        }
    }

    fn block(&mut self) -> Result<'s, Expr<'s>> {
        if let Some(open) = self.eat(tpred!(TokenKind::OpenBrace))? {
            self.scope(open.span.start, tpred!(TokenKind::CloseBrace))
        } else if let Some(open) = self.eat(tpred!(TokenKind::DotOpenBrace))? {
            let body_defs = self.object_body(bpred!(TokenKind::CloseBrace))?;
            let close = self.require(tpred!(TokenKind::CloseBrace))?;

            Ok(Expr {
                span: Span {
                    start: open.span.start,
                    end: close.span.end,
                },
                kind: ExprKind::Object {
                    definitions: body_defs,
                },
            })
        } else {
            self.expr()
        }
    }

    fn scope(
        &mut self,
        start: usize,
        end_pred: impl Fn(&Token<'s>) -> Option<Token<'s>>,
    ) -> Result<'s, Expr<'s>> {
        if let Some(close) = self.eat(&end_pred)? {
            let span = Span {
                start,
                end: close.span.end,
            };

            return Ok(Expr {
                kind: ExprKind::Tuple {
                    exprs: Box::new([]),
                },
                span,
            });
        }

        let mut body;
        if !self.has_peek(bpred!(TokenKind::Def))? {
            let first = self.tuple()?;

            if self.eat(&end_pred)?.is_some() {
                return Ok(first);
            } else {
                body = vec![Item::Expr(first)];

                self.require(bpred!(TokenKind::Semicolon))?;
            }
        } else {
            body = Vec::new();
        }

        let mut semi = true;
        while let Some(None) = self.tokens.peek()?.map(&end_pred) {
            if self.has_peek(to_bpred(&end_pred))? {
                break;
            } else if self.has_peek(bpred!(TokenKind::Def))? {
                body.push(Item::Def(self.def()?))
            } else {
                let expr = self.tuple()?;
                body.push(Item::Expr(expr));

                if self.eat(bpred!(TokenKind::Semicolon))?.is_none() {
                    semi = false;
                    break;
                } else {
                    semi = true;
                }
            }
        }

        if semi {
            body.push(Item::Empty);
        }

        let close = self.require(&end_pred)?;

        let span = Span {
            start,
            end: close.span.end,
        };

        Ok(Expr {
            kind: ExprKind::Scope { body: body.into() },
            span,
        })
    }

    fn tuple(&mut self) -> Result<'s, Expr<'s>> {
        let first = self.block()?;

        if self.has_peek(bpred!(TokenKind::Comma))? {
            let mut items = vec![first];

            while self.eat(bpred!(TokenKind::Comma))?.is_some() {
                let item = self.block()?;

                items.push(item);
            }

            let span = Span {
                start: items.first().unwrap().span.start,
                end: items.last().unwrap().span.end,
            };

            Ok(Expr {
                kind: ExprKind::Tuple {
                    exprs: items.into(),
                },
                span,
            })
        } else {
            Ok(first)
        }
    }

    fn def_expr(&mut self) -> Result<'s, (Expr<'s>, bool)> {
        let mut a = (self.logical()?, true);

        if let Some(open) = self.eat(tpred!(TokenKind::OpenBrace))? {
            let body = self.scope(open.span.start, tpred!(TokenKind::CloseBrace))?;

            a = (
                Expr {
                    span: Span {
                        start: a.0.span.start,
                        end: body.span.end,
                    },
                    kind: ExprKind::Lambda {
                        arg: Box::new(a.0),
                        body: Box::new(body),
                    },
                },
                false,
            );
        } else if let Some(open) = self.eat(tpred!(TokenKind::DotOpenBrace))? {
            let body_defs = self.object_body(bpred!(TokenKind::CloseBrace))?;
            let close = self.require(tpred!(TokenKind::CloseBrace))?;

            let body = Box::new(Expr {
                span: Span {
                    start: open.span.start,
                    end: close.span.end,
                },
                kind: ExprKind::Object {
                    definitions: body_defs,
                },
            });

            a = (
                Expr {
                    span: Span {
                        start: a.0.span.start,
                        end: body.span.end,
                    },
                    kind: ExprKind::Lambda {
                        arg: Box::new(a.0),
                        body,
                    },
                },
                false,
            );
        }

        if self.eat(bpred!(TokenKind::ColonColon))?.is_some() {
            let b = self.logical()?;

            a = (
                Expr {
                    span: Span {
                        start: a.0.span.start,
                        end: b.span.end,
                    },
                    kind: ExprKind::TypeAssertion {
                        a: Box::new(a.0),
                        b: Box::new(b),
                    },
                },
                true,
            );
        }

        Ok(a)
    }

    fn expr(&mut self) -> Result<'s, Expr<'s>> {
        let mut a = self.logical()?;

        if let Some(open) = self.eat(tpred!(TokenKind::OpenBrace))? {
            let body = self.scope(open.span.start, tpred!(TokenKind::CloseBrace))?;

            a = Expr {
                span: Span {
                    start: a.span.start,
                    end: body.span.end,
                },
                kind: ExprKind::Lambda {
                    arg: Box::new(a),
                    body: Box::new(body),
                },
            };
        }

        if self.eat(bpred!(TokenKind::ColonColon))?.is_some() {
            let b = self.logical()?;

            a = Expr {
                span: Span {
                    start: a.span.start,
                    end: b.span.end,
                },
                kind: ExprKind::TypeAssertion {
                    a: Box::new(a),
                    b: Box::new(b),
                },
            };
        } else if let Some(open) = self.eat(tpred!(TokenKind::DotOpenBrace))? {
            let body_defs = self.object_body(bpred!(TokenKind::CloseBrace))?;
            let close = self.require(tpred!(TokenKind::CloseBrace))?;

            let body = Box::new(Expr {
                span: Span {
                    start: open.span.start,
                    end: close.span.end,
                },
                kind: ExprKind::Object {
                    definitions: body_defs,
                },
            });

            a = Expr {
                span: Span {
                    start: a.span.start,
                    end: body.span.end,
                },
                kind: ExprKind::Lambda {
                    arg: Box::new(a),
                    body,
                },
            }
        }

        Ok(a)
    }

    fn logical(&mut self) -> Result<'s, Expr<'s>> {
        self.bin_op(
            Self::equal,
            vpred! {
                TokenKind::AmpAmp => BinOp::And,
                TokenKind::PipePipe => BinOp::Or,
            },
        )
    }

    fn equal(&mut self) -> Result<'s, Expr<'s>> {
        self.bin_op(
            Self::cmp,
            vpred! {
                TokenKind::Equal => BinOp::Equal,
                TokenKind::NotEqual => BinOp::NotEqual,
            },
        )
    }

    fn cmp(&mut self) -> Result<'s, Expr<'s>> {
        self.bin_op(
            Self::terms,
            vpred! {
                TokenKind::Gt => BinOp::Gt,
                TokenKind::GtEq => BinOp::GtEq,
                TokenKind::Lt => BinOp::Lt,
                TokenKind::LtEq => BinOp::LtEq,
            },
        )
    }

    fn terms(&mut self) -> Result<'s, Expr<'s>> {
        self.bin_op(
            Self::factors,
            vpred! {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
            },
        )
    }

    fn factors(&mut self) -> Result<'s, Expr<'s>> {
        self.bin_op(
            Self::prefix,
            vpred! {
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                TokenKind::Percent => BinOp::Mod,
            },
        )
    }

    fn prefix(&mut self) -> Result<'s, Expr<'s>> {
        if let Some((op_span, op)) = self.eat(vpred! {
            :t: TokenKind::Bang => (t.span, UnOp::Not),
            :t: TokenKind::Set => (t.span, UnOp::Set),
            :t: TokenKind::Val => (t.span, UnOp::Val),
            :t: TokenKind::Caret => (t.span, UnOp::Ref),
        })? {
            let a = self.prefix()?;

            let span = Span {
                start: op_span.start,
                end: a.span.end,
            };

            Ok(Expr {
                kind: ExprKind::UnOp {
                    op,
                    arg: Box::new(a),
                },
                span,
            })
        } else {
            self.suffix()
        }
    }

    fn suffix(&mut self) -> Result<'s, Expr<'s>> {
        let Some(mut a) = self.maybe_atom()? else {
            return Err(ParseError {
                kind: ParseErrorKind::Unexpected(self.tokens.peek()?.cloned()),
                span: None,
            })
        };

        loop {
            if let Some(caret) = self.eat(tpred!(TokenKind::Caret))? {
                let span = Span {
                    start: a.span.start,
                    end: caret.span.end,
                };

                a = Expr {
                    kind: ExprKind::UnOp {
                        op: UnOp::Deref,
                        arg: Box::new(a),
                    },
                    span,
                }
            } else if self.eat(tpred!(TokenKind::Dot))?.is_some() {
                let (prop_span, prop) =
                    self.require(vpred!(:t: TokenKind::Name(n) => (t.span, n)))?;

                let span = Span {
                    start: a.span.start,
                    end: prop_span.end,
                };

                a = Expr {
                    kind: ExprKind::Access {
                        expr: Box::new(a),
                        prop,
                    },
                    span,
                }
            } else if let Some(arg) = self.maybe_atom()? {
                a = Expr {
                    span: Span {
                        start: a.span.start,
                        end: arg.span.end,
                    },
                    kind: ExprKind::Apply {
                        a: Box::new(a),
                        b: Box::new(arg),
                    },
                }
            } else {
                break;
            }
        }

        Ok(a)
    }

    fn maybe_atom(&mut self) -> Result<'s, Option<Expr<'s>>> {
        if let Some(open) = self.eat(tpred!(TokenKind::OpenParen))? {
            Ok(Some(
                self.scope(open.span.start, tpred!(TokenKind::CloseParen))?,
            ))
        } else if self.has_peek(bpred!(TokenKind::Pipe))? {
            Ok(Some(self.variant()?))
        } else if let Some((span, kind)) = self.eat(vpred! {
            :t: TokenKind::Float(f) => (t.span, ExprKind::Literal(Literal::Float(f))),
            :t: TokenKind::Integer(i) => (t.span, ExprKind::Literal(Literal::Integer(i))),
            :t: TokenKind::String(s) => (t.span, ExprKind::Literal(Literal::String(s))),
            :t: TokenKind::Name(n) => (t.span, ExprKind::Ident(n)),
        })? {
            Ok(Some(Expr { span, kind }))
        } else {
            Ok(None)
        }
    }

    fn variant(&mut self) -> Result<'s, Expr<'s>> {
        let mut items = Vec::with_capacity(1);
        while let Some(pipe) = self.eat(tpred!(TokenKind::Pipe))? {
            let start = pipe.span.start;
            let (name_span, name) = self.require(vpred!(:t: TokenKind::Name(n) => (t.span, n)))?;
            let value;
            let end;
            if self.eat(bpred!(TokenKind::Colon))?.is_some() {
                let expr = self.expr()?;
                end = expr.span.end;
                value = Some(expr);
            } else {
                end = name_span.end;
                value = None;
            }

            items.push(VariantItem {
                name,
                value,
                span: Span { start, end },
            })
        }

        Ok(Expr {
            span: Span {
                start: items.first().unwrap().span.start,
                end: items.last().unwrap().span.end,
            },
            kind: ExprKind::Variant(items.into()),
        })
    }

    fn bin_op(
        &mut self,
        next: impl Fn(&mut Self) -> Result<'s, Expr<'s>>,
        pred: impl Fn(&Token<'s>) -> Option<BinOp>,
    ) -> Result<'s, Expr<'s>> {
        let mut a = next(self)?;

        while let Some(op) = self.eat(&pred)? {
            let b = next(self)?;

            let span = Span {
                start: a.span.start,
                end: a.span.end,
            };

            a = Expr {
                kind: ExprKind::BinOp {
                    op,
                    lhs: Box::new(a),
                    rhs: Box::new(b),
                },
                span,
            }
        }

        Ok(a)
    }

    /// Returns `true` if the current token peek satisfies `pred`.
    fn has_peek(&mut self, pred: impl Fn(&Token<'s>) -> Option<()>) -> Result<'s, bool> {
        if let Some(token) = self.tokens.peek()? {
            if pred(token).is_some() {
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            Ok(false)
        }
    }

    /// Requires that the next token exists and satisfies `pred` and errors otherwise.
    ///
    /// Does not consume the token if it does not satisfy `pred`.
    fn require<T>(&mut self, pred: impl Fn(&Token<'s>) -> Option<T>) -> Result<'s, T> {
        match self.maybe_require(pred) {
            Ok(Some(t)) => Ok(t),
            Ok(None) => Err(ParseError {
                kind: ParseErrorKind::Unexpected(None),
                span: None,
            }),
            Err(e) => Err(e),
        }
    }

    /// Requires that the next token (if one exists) satisfies `pred` and errors otherwise.
    ///
    /// Does not consume the token if it does not satisfy `pred`.
    fn maybe_require<T>(
        &mut self,
        pred: impl Fn(&Token<'s>) -> Option<T>,
    ) -> Result<'s, Option<T>> {
        if let Some(token) = self.tokens.peek()? {
            if let Some(t) = pred(token) {
                self.tokens.next()?;
                Ok(Some(t))
            } else {
                Err(ParseError {
                    span: Some(token.span),
                    kind: ParseErrorKind::Unexpected(Some(token.clone())),
                })
            }
        } else {
            Ok(None)
        }
    }

    /// Checks if the next token (if one exists) satisfies `pred` and returns None otherwise.
    ///
    /// Does not consume the token if it does not satisfy `pred`.
    fn eat<T>(&mut self, pred: impl Fn(&Token<'s>) -> Option<T>) -> Result<'s, Option<T>> {
        if let Some(token) = self.tokens.peek()? {
            if let Some(t) = pred(token) {
                self.tokens.next()?;
                Ok(Some(t))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }
}
