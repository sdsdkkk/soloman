//! Statically-typed checking for Soloman.
//!
//! - Variables must be declared with `let name: Ty = expr;` before use.
//! - `name = expr;` is reassignment and must match the declared type.
//! - Functions and objects are declared at module scope.

use std::collections::HashMap;

use crate::ast::{BinOp, Expr, FnDef, Item, ObjDef, Program, Stmt, Ty};

#[derive(Debug, Clone)]
pub struct ObjInfo {
    pub fields: Vec<(String, Ty)>,
    pub index: HashMap<String, usize>,
}

#[derive(Debug, Clone)]
pub struct FnSig {
    pub params: Vec<Ty>,
    pub ret: Ty,
}

#[derive(Debug, Clone)]
pub struct TypeEnv {
    pub objects: HashMap<String, ObjInfo>,
    pub functions: HashMap<String, FnSig>,
}

pub fn check_program(program: &Program) -> Result<TypeEnv, String> {
    let mut env = TypeEnv {
        objects: HashMap::new(),
        functions: builtins(),
    };

    // Collect object definitions first.
    for item in &program.items {
        if let Item::Object(o) = item {
            add_object(&mut env, o)?;
        }
    }

    // Collect function signatures.
    for item in &program.items {
        if let Item::Function(f) = item {
            add_function_sig(&mut env, f)?;
        }
    }

    // Check function bodies.
    for item in &program.items {
        if let Item::Function(f) = item {
            check_function(f, &env)?;
        }
    }

    // Check top-level statements (synthesized `main`).
    let mut scope = Scope::default();
    for stmt in &program.stmts {
        check_stmt(stmt, &env, &mut scope, Ctx::Main)?;
    }

    Ok(env)
}

fn builtins() -> HashMap<String, FnSig> {
    let mut f = HashMap::new();
    f.insert(
        "print".to_string(),
        FnSig {
            params: vec![Ty::Str], // special-cased to accept Int too
            ret: Ty::Unit,
        },
    );
    f.insert(
        "eprint".to_string(),
        FnSig {
            params: vec![Ty::Str], // special-cased to accept Int too
            ret: Ty::Unit,
        },
    );
    f.insert(
        "read_line".to_string(),
        FnSig {
            params: vec![],
            ret: Ty::Str,
        },
    );
    f.insert(
        "prompt".to_string(),
        FnSig {
            params: vec![Ty::Str],
            ret: Ty::Str,
        },
    );
    f.insert(
        "len".to_string(),
        FnSig {
            params: vec![Ty::Str],
            ret: Ty::Int,
        },
    );
    f
}

fn add_object(env: &mut TypeEnv, o: &ObjDef) -> Result<(), String> {
    if env.objects.contains_key(&o.name) {
        return Err(format!("duplicate object `{}`", o.name));
    }
    let mut index = HashMap::new();
    for (i, (f, _)) in o.fields.iter().enumerate() {
        if index.insert(f.clone(), i).is_some() {
            return Err(format!("object `{}` has duplicate field `{}`", o.name, f));
        }
    }
    env.objects.insert(
        o.name.clone(),
        ObjInfo {
            fields: o.fields.clone(),
            index,
        },
    );
    Ok(())
}

fn add_function_sig(env: &mut TypeEnv, f: &FnDef) -> Result<(), String> {
    if env.functions.contains_key(&f.name) {
        return Err(format!("duplicate function `{}`", f.name));
    }
    for p in &f.params {
        ensure_ty_known(&p.ty, env)?;
    }
    ensure_ty_known(&f.ret, env)?;
    env.functions.insert(
        f.name.clone(),
        FnSig {
            params: f.params.iter().map(|p| p.ty.clone()).collect(),
            ret: f.ret.clone(),
        },
    );
    Ok(())
}

fn ensure_ty_known(ty: &Ty, env: &TypeEnv) -> Result<(), String> {
    match ty {
        Ty::Int | Ty::Str | Ty::Unit => Ok(()),
        Ty::Object(n) => {
            if env.objects.contains_key(n) {
                Ok(())
            } else {
                Err(format!("unknown object type `{}`", n))
            }
        }
    }
}

#[derive(Default)]
struct Scope {
    vars: HashMap<String, Ty>,
}

#[derive(Clone)]
enum Ctx {
    Main,
    Function { ret: Ty },
}

fn check_function(f: &FnDef, env: &TypeEnv) -> Result<(), String> {
    let sig = env
        .functions
        .get(&f.name)
        .ok_or_else(|| "internal: fn sig".to_string())?
        .clone();
    let mut scope = Scope::default();
    for (p, ty) in f.params.iter().zip(sig.params.iter()) {
        scope.vars.insert(p.name.clone(), ty.clone());
    }

    let mut saw_return = false;
    for stmt in &f.body {
        if matches!(stmt, Stmt::Return(_)) {
            saw_return = true;
        }
        check_stmt(stmt, env, &mut scope, Ctx::Function { ret: sig.ret.clone() })?;
    }

    if sig.ret != Ty::Unit && !saw_return {
        return Err(format!("function `{}` must return {:?}", f.name, sig.ret));
    }
    Ok(())
}

fn check_stmt(stmt: &Stmt, env: &TypeEnv, scope: &mut Scope, ctx: Ctx) -> Result<(), String> {
    match stmt {
        Stmt::Let { name, ty, value } => {
            ensure_ty_known(ty, env)?;
            let vt = check_expr(value, env, scope)?;
            if vt == Ty::Unit {
                return Err("cannot assign from a Unit-valued expression".to_string());
            }
            if &vt != ty {
                return Err(format!("`let {}: {:?}` assigned value of type {:?}", name, ty, vt));
            }
            if scope.vars.contains_key(name) {
                return Err(format!("duplicate variable `{}`", name));
            }
            scope.vars.insert(name.clone(), ty.clone());
            Ok(())
        }
        Stmt::Assign { name, value } => {
            let Some(existing) = scope.vars.get(name).cloned() else {
                return Err(format!("assignment to undeclared variable `{}` (use let)", name));
            };
            let vt = check_expr(value, env, scope)?;
            if vt == Ty::Unit {
                return Err("cannot assign from a Unit-valued expression".to_string());
            }
            if vt != existing {
                return Err(format!(
                    "variable `{}` is {:?}, cannot assign {:?}",
                    name, existing, vt
                ));
            }
            Ok(())
        }
        Stmt::Return(e) => match ctx {
            Ctx::Main => Err("`return` is only valid inside functions".to_string()),
            Ctx::Function { ret } => {
                let got = match e {
                    None => Ty::Unit,
                    Some(expr) => check_expr(expr, env, scope)?,
                };
                if got != ret {
                    return Err(format!("return type mismatch: expected {:?}, got {:?}", ret, got));
                }
                Ok(())
            }
        },
        Stmt::Expr(e) => {
            let _ = check_expr(e, env, scope)?;
            Ok(())
        }
    }
}

fn check_expr(expr: &Expr, env: &TypeEnv, scope: &Scope) -> Result<Ty, String> {
    match expr {
        Expr::Int(_) => Ok(Ty::Int),
        Expr::Str(_) => Ok(Ty::Str),
        Expr::Var(name) => scope
            .vars
            .get(name)
            .cloned()
            .ok_or_else(|| format!("undefined variable `{}`", name)),
        Expr::Field { base, name } => {
            let bt = check_expr(base, env, scope)?;
            let Ty::Object(obj) = bt else {
                return Err("field access requires an object value".to_string());
            };
            let info = env
                .objects
                .get(&obj)
                .ok_or_else(|| format!("unknown object `{}`", obj))?;
            let idx = info
                .index
                .get(name)
                .copied()
                .ok_or_else(|| format!("object `{}` has no field `{}`", obj, name))?;
            Ok(info.fields[idx].1.clone())
        }
        Expr::Binary { op, left, right } => {
            let lt = check_expr(left, env, scope)?;
            let rt = check_expr(right, env, scope)?;
            if lt == Ty::Unit || rt == Ty::Unit {
                return Err("cannot use Unit-valued expressions in binary ops".to_string());
            }
            match op {
                BinOp::Add => match (&lt, &rt) {
                    (Ty::Int, Ty::Int) => Ok(Ty::Int),
                    (Ty::Str, Ty::Str) => Ok(Ty::Str),
                    _ => Err("`+` requires two Int or two Str".to_string()),
                },
                BinOp::Sub | BinOp::Mul | BinOp::Div => {
                    if lt == Ty::Int && rt == Ty::Int {
                        Ok(Ty::Int)
                    } else {
                        Err("arithmetic requires Int".to_string())
                    }
                }
            }
        }
        Expr::Call { name, args } => check_call(name, args, env, scope),
        Expr::ObjLit { object, fields } => {
            let info = env
                .objects
                .get(object)
                .ok_or_else(|| format!("unknown object `{}`", object))?;
            if fields.len() != info.fields.len() {
                return Err(format!(
                    "`{}` literal must specify {} fields",
                    object,
                    info.fields.len()
                ));
            }
            let mut seen = HashMap::<String, ()>::new();
            for (fname, fexpr) in fields {
                if !info.index.contains_key(fname) {
                    return Err(format!("object `{}` has no field `{}`", object, fname));
                }
                if seen.insert(fname.clone(), ()).is_some() {
                    return Err(format!("duplicate field `{}` in `{}` literal", fname, object));
                }
                let expect = info.fields[*info.index.get(fname).unwrap()].1.clone();
                let got = check_expr(fexpr, env, scope)?;
                if got != expect {
                    return Err(format!(
                        "field `{}` of `{}` expects {:?}, got {:?}",
                        fname, object, expect, got
                    ));
                }
            }
            Ok(Ty::Object(object.clone()))
        }
    }
}

fn check_call(name: &str, args: &[Expr], env: &TypeEnv, scope: &Scope) -> Result<Ty, String> {
    match name {
        "print" | "eprint" => {
            if args.len() != 1 {
                return Err(format!("`{}` expects 1 argument", name));
            }
            let t = check_expr(&args[0], env, scope)?;
            if t != Ty::Int && t != Ty::Str {
                return Err(format!("`{}` expects Int or Str", name));
            }
            Ok(Ty::Unit)
        }
        _ => {
            let sig = env
                .functions
                .get(name)
                .ok_or_else(|| format!("unknown function `{}`", name))?;
            if sig.params.len() != args.len() {
                return Err(format!(
                    "`{}` expects {} args, got {}",
                    name,
                    sig.params.len(),
                    args.len()
                ));
            }
            for (i, (arg, pty)) in args.iter().zip(sig.params.iter()).enumerate() {
                let at = check_expr(arg, env, scope)?;
                if at != *pty {
                    return Err(format!(
                        "arg {} of `{}` expects {:?}, got {:?}",
                        i, name, pty, at
                    ));
                }
            }
            Ok(sig.ret.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{lexer::tokenize, parser::Parser};

    use super::check_program;

    fn parse(src: &str) -> crate::ast::Program {
        let toks = tokenize(src).expect("tokenize");
        Parser::new(toks).parse_program().expect("parse")
    }

    #[test]
    fn typecheck_accepts_valid_program() {
        let p = parse(
            r#"
            object Point { x: Int; y: Int; }
            fn sum(p: Point) -> Int { return p.x + p.y; }
            fn hello(name: Str) -> Str { return name + "!"; }
            let p: Point = Point{ x: 2, y: 3 };
            let n: Int = sum(p);
            print(n);
            "#,
        );
        let env = check_program(&p).expect("typecheck");
        assert!(env.objects.contains_key("Point"));
        assert!(env.functions.contains_key("sum"));
    }

    #[test]
    fn typecheck_rejects_assignment_type_mismatch() {
        let p = parse("let n: Int = 1; n = \"x\";");
        let err = check_program(&p).unwrap_err();
        assert!(err.contains("cannot assign"));
    }

    #[test]
    fn typecheck_rejects_bad_return_type() {
        let p = parse("fn f() -> Int { return \"x\"; }");
        let err = check_program(&p).unwrap_err();
        assert!(err.contains("return type mismatch"));
    }

    #[test]
    fn typecheck_rejects_unknown_object_type() {
        let p = parse("fn f(x: Missing) -> Int { return 1; }");
        let err = check_program(&p).unwrap_err();
        assert!(err.contains("unknown object type"));
    }
}
