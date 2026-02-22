use inkwell::context::Context;
use inkwell::OptimizationLevel;
use std::env;
use std::fs;

// =====================
// AST
// =====================

#[derive(Debug)]
enum Expr {
    Number(i64),
    Add(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
}

#[derive(Debug)]
enum Stmt {
    Print(Expr),
}

#[derive(Debug)]
struct Program {
    statements: Vec<Stmt>,
}

// =====================
// VERY SIMPLE PARSER
// (for demonstration)
// =====================

fn parse(source: &str) -> Program {

    let mut statements = Vec::new();

    for line in source.lines() {

        let line = line.trim();

        if line.starts_with("print ") {

            let expr_str = line
                .strip_prefix("print ")
                .unwrap()
                .strip_suffix(";")
                .unwrap();

            let expr = parse_expr(expr_str);

            statements.push(Stmt::Print(expr));
        }
    }

    Program { statements }
}

fn parse_expr(s: &str) -> Expr {

    if let Some(pos) = s.find('+') {

        return Expr::Add(
            Box::new(parse_expr(&s[..pos])),
            Box::new(parse_expr(&s[pos+1..]))
        );
    }

    if let Some(pos) = s.find('*') {

        return Expr::Mul(
            Box::new(parse_expr(&s[..pos])),
            Box::new(parse_expr(&s[pos+1..]))
        );
    }

    Expr::Number(s.trim().parse().unwrap())
}

// =====================
// LLVM CODEGEN
// =====================

fn compile(program: Program, output: &str) {

    let context = Context::create();
    let module = context.create_module("soloman");
    let builder = context.create_builder();

    let i64_type = context.i64_type();

    // main function
    let main_type = i64_type.fn_type(&[], false);
    let main_fn = module.add_function("main", main_type, None);

    let basic_block = context.append_basic_block(main_fn, "entry");
    builder.position_at_end(basic_block);

    // printf
    let i8ptr = context.i8_type().ptr_type(Default::default());

    let printf_type =
        context.i32_type().fn_type(&[i8ptr.into()], true);

    let printf =
        module.add_function("printf", printf_type, None);

    // format string
    let format_str =
        builder.build_global_string_ptr("%ld\n", "fmt");

    for stmt in program.statements {

        match stmt {

            Stmt::Print(expr) => {

                let value =
                    compile_expr(&context, &builder, expr);

                builder.build_call(
                    printf,
                    &[
                        format_str.as_pointer_value().into(),
                        value.into()
                    ],
                    "printf"
                );
            }
        }
    }

    builder.build_return(Some(&i64_type.const_int(0, false)));

    module.print_to_file("out.ll").unwrap();

    // Compile to executable
    std::process::Command::new("clang")
        .args(&["out.ll", "-o", output])
        .status()
        .unwrap();
}

fn compile_expr<'ctx>(
    context: &'ctx Context,
    builder: &inkwell::builder::Builder<'ctx>,
    expr: Expr,
) -> inkwell::values::IntValue<'ctx> {

    let i64_type = context.i64_type();

    match expr {

        Expr::Number(n) =>
            i64_type.const_int(n as u64, false),

        Expr::Add(a, b) => {

            let left = compile_expr(context, builder, *a);
            let right = compile_expr(context, builder, *b);

            builder.build_int_add(left, right, "addtmp")
        }

        Expr::Mul(a, b) => {

            let left = compile_expr(context, builder, *a);
            let right = compile_expr(context, builder, *b);

            builder.build_int_mul(left, right, "multmp")
        }
    }
}

// =====================
// MAIN
// =====================

fn main() {

    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {

        println!("Usage: soloman <file>");
        return;
    }

    let source =
        fs::read_to_string(&args[1]).unwrap();

    let program = parse(&source);

    compile(program, "out");

    println!("Soloman compiled successfully.");
}
