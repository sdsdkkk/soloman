//! Soloman compiler driver: `.sol` → LLVM IR, optional run via clang.

mod ast;
mod codegen;
mod lexer;
mod parser;
mod typecheck;

use std::env;
use std::fs;
use std::path::Path;
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
    let src = fs::read_to_string(path).map_err(|e| e.to_string())?;
    if path.extension().and_then(|s| s.to_str()) != Some("sol") {
        return Err("source file should use the .sol extension".to_string());
    }

    let tokens = lexer::tokenize(&src)?;
    let program = parser::Parser::new(tokens).parse_program()?;
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
