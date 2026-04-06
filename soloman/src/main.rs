//! Soloman compiler driver: `.sol` → LLVM IR, optional run via clang.

mod ast;
mod codegen;
mod lexer;
mod parser;
mod typecheck;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use inkwell::context::Context;

fn main() {
    let mut args = env::args().skip(1);
    let cmd = args.next().unwrap_or_else(|| {
        eprintln!(
            "usage: soloman emit-ll <file.sol>   write LLVM IR to stdout\n       soloman run <file.sol>        compile and run (requires clang on PATH)"
        );
        std::process::exit(1);
    });

    let path = args.next().unwrap_or_else(|| {
        eprintln!("error: missing file path");
        std::process::exit(1);
    });

    if let Err(e) = drive(&cmd, Path::new(&path)) {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

fn drive(cmd: &str, path: &Path) -> Result<(), String> {
    if path.extension().and_then(|s| s.to_str()) != Some("sol") {
        return Err("source file should use the .sol extension".to_string());
    }

    let program = load_with_imports(path)?;
    let env = typecheck::check_program(&program)?;

    let context = Context::create();
    match cmd {
        "emit-ll" => {
            let ir = codegen::compile_to_ir(&context, &program, env)?;
            print!("{}", ir);
            Ok(())
        }
        "run" => {
            let ir = codegen::compile_to_ir(&context, &program, env)?;
            run_via_clang(&ir)
        }
        _ => Err(format!(
            "unknown command `{}` (try `emit-ll` or `run`)",
            cmd
        )),
    }
}

fn load_with_imports(entry: &Path) -> Result<ast::Program, String> {
    let mut seen = std::collections::HashSet::<PathBuf>::new();
    let mut items = Vec::new();
    let mut stmts = Vec::new();
    let entry_abs = fs::canonicalize(entry).map_err(|e| e.to_string())?;
    load_file_recursive(&entry_abs, &entry_abs, &mut seen, &mut items, &mut stmts)?;
    Ok(ast::Program { items, stmts })
}

fn load_file_recursive(
    entry_abs: &Path,
    path: &Path,
    seen: &mut std::collections::HashSet<PathBuf>,
    items_out: &mut Vec<ast::Item>,
    stmts_out: &mut Vec<ast::Stmt>,
) -> Result<(), String> {
    let abs = fs::canonicalize(path).map_err(|e| e.to_string())?;
    if !seen.insert(abs.clone()) {
        return Ok(());
    }

    let src = fs::read_to_string(&abs).map_err(|e| e.to_string())?;
    let tokens = lexer::tokenize(&src)?;
    let program = parser::Parser::new(tokens).parse_program()?;

    // Recurse into imports.
    for item in &program.items {
        if let ast::Item::Import(rel) = item {
            let child = abs
                .parent()
                .unwrap_or(entry_abs)
                .join(rel);
            load_file_recursive(entry_abs, &child, seen, items_out, stmts_out)?;
        }
    }

    // Add all non-import items to the output.
    for item in program.items {
        match item {
            ast::Item::Import(_) => {}
            other => items_out.push(other),
        }
    }

    // Only the entry file contributes top-level executable statements.
    if abs == entry_abs {
        stmts_out.extend(program.stmts);
    }

    Ok(())
}

fn run_via_clang(ir: &str) -> Result<(), String> {
    let out = std::env::temp_dir().join(format!(
        "soloman_run_{}",
        std::process::id()
    ));
    let ll = out.with_extension("ll");
    fs::write(&ll, ir).map_err(|e| e.to_string())?;
    let bin = out.with_extension("bin");
    let status = Command::new("clang")
        .arg("-O0")
        .arg(&ll)
        .arg("-o")
        .arg(&bin)
        .status()
        .map_err(|e| {
            format!(
                "failed to spawn clang (install clang and ensure it is on PATH): {}",
                e
            )
        })?;
    if !status.success() {
        return Err("clang failed".to_string());
    }
    let st = Command::new(&bin)
        .status()
        .map_err(|e| e.to_string())?;
    let _ = fs::remove_file(&ll);
    let _ = fs::remove_file(&bin);
    if !st.success() {
        return Err(format!("program exited with {:?}", st.code()));
    }
    Ok(())
}
