//! Simple monomorphic type checking (int vs string).

use std::collections::HashMap;

use crate::ast::{BinOp, Expr, Program, Stmt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ty {
    Int,
    Str,
    /// Result of `print` / `eprint`; cannot be stored or nested in other expressions.
    Unit,
}

pub struct TypeEnv {
    pub vars: HashMap<String, Ty>,
}

pub fn check_program(program: &Program) -> Result<TypeEnv, String> {
    let mut env = TypeEnv {
        vars: HashMap::new(),
    };
    for stmt in &program.stmts {
        check_stmt(stmt, &mut env)?;
    }
    Ok(env)
}

fn check_stmt(stmt: &Stmt, env: &mut TypeEnv) -> Result<(), String> {
    match stmt {
        Stmt::Assign { name, value } => {
            let t = check_expr(value, env)?;
            if t == Ty::Unit {
                return Err("cannot assign from `print` / `eprint` (they do not produce a value)".to_string());
            }
            if let Some(existing) = env.vars.get(name) {
                if *existing != t {
                    return Err(format!(
                        "variable `{}` was {:?}, cannot assign {:?}",
                        name, existing, t
                    ));
                }
            } else {
                env.vars.insert(name.clone(), t);
            }
        }
        Stmt::Expr(e) => {
            let _ = check_expr(e, env)?;
        }
    }
    Ok(())
}

fn check_expr(expr: &Expr, env: &TypeEnv) -> Result<Ty, String> {
    match expr {
        Expr::Int(_) => Ok(Ty::Int),
        Expr::Str(_) => Ok(Ty::Str),
        Expr::Var(name) => env
            .vars
            .get(name)
            .copied()
            .ok_or_else(|| format!("undefined variable `{}`", name)),
        Expr::Binary { op, left, right } => {
            let lt = check_expr(left, env)?;
            let rt = check_expr(right, env)?;
            if lt == Ty::Unit || rt == Ty::Unit {
                return Err("cannot use `print` / `eprint` inside an expression".to_string());
            }
            match op {
                BinOp::Add => match (lt, rt) {
                    (Ty::Int, Ty::Int) => Ok(Ty::Int),
                    (Ty::Str, Ty::Str) => Ok(Ty::Str),
                    _ => Err("addition requires two integers or two strings".to_string()),
                },
                BinOp::Sub | BinOp::Mul | BinOp::Div => {
                    if lt == Ty::Int && rt == Ty::Int {
                        Ok(Ty::Int)
                    } else {
                        Err("arithmetic requires integers".to_string())
                    }
                }
            }
        }
        Expr::Call { name, args } => check_call(name, args, env),
    }
}

fn check_call(name: &str, args: &[Expr], env: &TypeEnv) -> Result<Ty, String> {
    match name {
        "print" | "eprint" => {
            if args.len() != 1 {
                return Err(format!("`{}` expects 1 argument", name));
            }
            let t = check_expr(&args[0], env)?;
            if t != Ty::Int && t != Ty::Str {
                return Err("internal: print type".to_string());
            }
            Ok(Ty::Unit)
        }
        "read_line" => {
            if !args.is_empty() {
                return Err("`read_line` expects no arguments".to_string());
            }
            Ok(Ty::Str)
        }
        "prompt" => {
            if args.len() != 1 {
                return Err("`prompt` expects 1 string argument".to_string());
            }
            if check_expr(&args[0], env)? != Ty::Str {
                return Err("`prompt` message must be a string".to_string());
            }
            Ok(Ty::Str)
        }
        "len" => {
            if args.len() != 1 {
                return Err("`len` expects 1 string argument".to_string());
            }
            if check_expr(&args[0], env)? != Ty::Str {
                return Err("`len` expects a string".to_string());
            }
            Ok(Ty::Int)
        }
        _ => Err(format!("unknown function `{}`", name)),
    }
}
