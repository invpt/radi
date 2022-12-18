use super::*;

pub fn ast_size(expr: &Expr) -> usize {
    std::mem::size_of::<Expr>()
        + match &expr.kind {
            ExprKind::Object { definitions } => definitions.iter().map(def_size).sum(),
            ExprKind::Scope { body, .. } => {
                body.iter()
                    .map(|it| match it {
                        Item::Expr(e) => ast_size(e) + 8,
                        Item::Def(d) => def_size(d) + 8,
                        Item::Empty => 8,
                    })
                    .sum()
                //0//defs.iter().map(def_size).sum::<usize>() + body.iter().map(ast_size).sum::<usize>()
            }
            ExprKind::Lambda { arg, body } => ast_size(arg) + ast_size(body),
            ExprKind::SqLambda { arg, expr } => ast_size(expr) + ast_size(arg),
            ExprKind::BinOp { lhs, rhs, .. } => ast_size(lhs) + ast_size(rhs),
            ExprKind::UnOp { arg, .. } => ast_size(arg),
            ExprKind::Access { expr, prop } => ast_size(expr) + prop.0.len(),
            ExprKind::Case {
                cond,
                on_true,
                on_false,
            } => ast_size(cond) + ast_size(on_true) + ast_size(on_false),
            ExprKind::Tuple { exprs } => exprs.iter().map(ast_size).sum(),
            ExprKind::Apply { a, b } => ast_size(a) + ast_size(b),
            ExprKind::TypeAssertion { a, b } => ast_size(a) + ast_size(b),
            ExprKind::Variant(its) => its.iter().map(varit_size).sum(),
            ExprKind::Ident(i) => i.0.len(),
            ExprKind::Literal(Literal::String(i)) => i.0.len(),
            _ => 0,
        }
}

fn def_size(def: &Def) -> usize {
    std::mem::size_of::<Def>() + def.name.0.len() + ast_size(&def.value)
}

fn varit_size(varit: &VariantItem) -> usize {
    std::mem::size_of::<VariantItem>() + varit.value.as_ref().map(ast_size).unwrap_or(0)
}