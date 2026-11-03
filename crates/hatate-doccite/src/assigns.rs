//! Where test code gives a settings key a value (DAN-2602, the settings ratchet).
//!
//! [`key_assignments`] parses one Rust file and lists every `x.key = value;` and every
//! `Section { key: value }` literal whose key is in the set asked for, with the literal on the right
//! (when it is one) and the named item it sits in. It does not decide what counts as evidence; the
//! ratchet test in `hatate-gtk` does, from this list. A `json!({"key": ..})` is a macro body and is
//! not seen.

use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::{FnSpan, fn_spans};

/// What stands on the right of the assignment.
#[derive(Debug, Clone, PartialEq)]
pub enum Rhs {
    Bool(bool),
    Num(f64),
    Str(String),
    /// Anything that is not one of the three literals above: a variable, a call, a collection.
    Other,
}

/// One assignment of a settings key.
#[derive(Debug, Clone, PartialEq)]
pub struct Assign {
    /// The field name.
    pub key: String,
    /// For `a.mcp.key = ..` the `mcp`; for `McpSettings { key: .. }` the type name. `None` for a bare `a.key`.
    pub section: Option<String>,
    /// 1-based line of the assignment.
    pub line: usize,
    pub rhs: Rhs,
    /// The innermost named item the line is in.
    pub in_fn: Option<String>,
}

fn rhs_of(e: &syn::Expr) -> Rhs {
    match e {
        syn::Expr::Lit(l) => match &l.lit {
            syn::Lit::Bool(b) => Rhs::Bool(b.value),
            syn::Lit::Int(i) => i.base10_parse::<f64>().map_or(Rhs::Other, Rhs::Num),
            syn::Lit::Float(f) => f.base10_parse::<f64>().map_or(Rhs::Other, Rhs::Num),
            syn::Lit::Str(s) => Rhs::Str(s.value()),
            _ => Rhs::Other,
        },
        syn::Expr::Unary(u) if matches!(u.op, syn::UnOp::Neg(_)) => match rhs_of(&u.expr) {
            Rhs::Num(n) => Rhs::Num(-n),
            _ => Rhs::Other,
        },
        syn::Expr::Paren(p) => rhs_of(&p.expr),
        _ => Rhs::Other,
    }
}

struct Walk<'k> {
    keys: &'k [&'k str],
    out: Vec<(String, Option<String>, usize, Rhs)>,
}

impl Walk<'_> {
    fn note(&mut self, key: &syn::Ident, section: Option<String>, whole: &impl Spanned, rhs: Rhs) {
        let k = key.to_string();
        if self.keys.contains(&k.as_str()) {
            self.out.push((k, section, whole.span().start().line, rhs));
        }
    }
}

impl<'a> Visit<'a> for Walk<'_> {
    fn visit_expr_assign(&mut self, a: &'a syn::ExprAssign) {
        if let syn::Expr::Field(f) = &*a.left
            && let syn::Member::Named(key) = &f.member
        {
            let section = match &*f.base {
                syn::Expr::Field(b) => match &b.member {
                    syn::Member::Named(s) => Some(s.to_string()),
                    _ => None,
                },
                _ => None,
            };
            self.note(key, section, a, rhs_of(&a.right));
        }
        syn::visit::visit_expr_assign(self, a);
    }

    fn visit_expr_struct(&mut self, s: &'a syn::ExprStruct) {
        let ty = s.path.segments.last().map(|p| p.ident.to_string());
        for f in &s.fields {
            if let syn::Member::Named(key) = &f.member {
                self.note(key, ty.clone(), f, rhs_of(&f.expr));
            }
        }
        syn::visit::visit_expr_struct(self, s);
    }
}

/// Every assignment in `src` of one of `keys`, in source order.
pub fn key_assignments(src: &str, keys: &[&str]) -> Result<Vec<Assign>, String> {
    let file = syn::parse_file(src).map_err(|e| format!("does not parse: {e}"))?;
    let mut w = Walk {
        keys,
        out: Vec::new(),
    };
    w.visit_file(&file);
    let spans = fn_spans(src)?;
    let mut out: Vec<Assign> = w
        .out
        .into_iter()
        .map(|(key, section, line, rhs)| {
            let in_fn = spans
                .iter()
                .filter(|s: &&FnSpan| s.kind == "fn" && s.start <= line && line <= s.end)
                .min_by_key(|s| s.end - s.start)
                .map(|s| s.name.clone());
            Assign {
                key,
                section,
                line,
                rhs,
                in_fn,
            }
        })
        .collect();
    out.sort_by_key(|a| a.line);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_field_assignments_closures_and_struct_literals() {
        let src = r#"
fn a() {
    s.lazy_thumbnails = false;
    s.mcp.audit_log_max_bytes = 100;
    let _ = McpSettings { audit_log_max_bytes: -5, port: 1 };
    let f = |s: &mut S| s.hash_workers = n;
    other.unrelated = 1;
}
"#;
        let got = key_assignments(
            src,
            &["lazy_thumbnails", "audit_log_max_bytes", "hash_workers"],
        )
        .unwrap();
        let shape: Vec<_> = got
            .iter()
            .map(|a| {
                (
                    a.key.as_str(),
                    a.section.as_deref(),
                    a.rhs.clone(),
                    a.in_fn.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            shape,
            vec![
                ("lazy_thumbnails", None, Rhs::Bool(false), Some("a")),
                (
                    "audit_log_max_bytes",
                    Some("mcp"),
                    Rhs::Num(100.0),
                    Some("a")
                ),
                (
                    "audit_log_max_bytes",
                    Some("McpSettings"),
                    Rhs::Num(-5.0),
                    Some("a")
                ),
                ("hash_workers", None, Rhs::Other, Some("a")),
            ]
        );
    }
}
