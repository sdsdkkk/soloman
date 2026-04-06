//! LLVM IR generation via Inkwell.

use std::collections::HashMap;

use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::{Linkage, Module};
use inkwell::values::{FunctionValue, IntValue, PointerValue};
use inkwell::AddressSpace;

use crate::ast::{BinOp, Expr, Program, Stmt};
use crate::typecheck::{Ty, TypeEnv};

enum GenVal<'ctx> {
    Int(IntValue<'ctx>),
    Str(PointerValue<'ctx>),
    Unit,
}

pub struct CodeGen<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    env: TypeEnv,
    vars: HashMap<String, PointerValue<'ctx>>,
    i64_t: inkwell::types::IntType<'ctx>,
    i32_t: inkwell::types::IntType<'ctx>,
    ptr_t: inkwell::types::PointerType<'ctx>,
    strlen: FunctionValue<'ctx>,
    malloc: FunctionValue<'ctx>,
    memcpy: FunctionValue<'ctx>,
    printf: FunctionValue<'ctx>,
    fprintf: FunctionValue<'ctx>,
    fflush: FunctionValue<'ctx>,
    fgets: FunctionValue<'ctx>,
    stdin: inkwell::values::GlobalValue<'ctx>,
    stderr: inkwell::values::GlobalValue<'ctx>,
    readline_buf: inkwell::values::GlobalValue<'ctx>,
    stdout: inkwell::values::GlobalValue<'ctx>,
    next_lit: u32,
}

impl<'ctx> CodeGen<'ctx> {
    pub fn new(context: &'ctx Context, env: TypeEnv) -> Self {
        let module = context.create_module("soloman");
        let builder = context.create_builder();
        let i64_t = context.i64_type();
        let i32_t = context.i32_type();
        let i8_t = context.i8_type();
        let ptr_t = i8_t.ptr_type(AddressSpace::default());

        let strlen = module.add_function(
            "strlen",
            i64_t.fn_type(&[ptr_t.into()], false),
            Some(Linkage::External),
        );
        let malloc = module.add_function(
            "malloc",
            ptr_t.fn_type(&[i64_t.into()], false),
            Some(Linkage::External),
        );
        let memcpy = module.add_function(
            "memcpy",
            ptr_t.fn_type(&[ptr_t.into(), ptr_t.into(), i64_t.into()], false),
            Some(Linkage::External),
        );
        let printf = module.add_function(
            "printf",
            i32_t.fn_type(&[ptr_t.into()], true),
            Some(Linkage::External),
        );
        let fprintf = module.add_function(
            "fprintf",
            i32_t.fn_type(&[ptr_t.into(), ptr_t.into()], true),
            Some(Linkage::External),
        );
        let fflush = module.add_function(
            "fflush",
            i32_t.fn_type(&[ptr_t.into()], false),
            Some(Linkage::External),
        );
        let fgets = module.add_function(
            "fgets",
            ptr_t.fn_type(&[ptr_t.into(), i32_t.into(), ptr_t.into()], false),
            Some(Linkage::External),
        );

        let stdin = module.add_global(ptr_t, Some(AddressSpace::default()), "stdin");
        stdin.set_linkage(Linkage::External);
        let stdout = module.add_global(ptr_t, Some(AddressSpace::default()), "stdout");
        stdout.set_linkage(Linkage::External);
        let stderr = module.add_global(ptr_t, Some(AddressSpace::default()), "stderr");
        stderr.set_linkage(Linkage::External);

        let buf_ty = i8_t.array_type(4096);
        let readline_buf = module.add_global(buf_ty, Some(AddressSpace::default()), "sol_readline_buf");
        readline_buf.set_linkage(Linkage::Internal);
        readline_buf.set_initializer(&buf_ty.const_zero());

        Self {
            context,
            module,
            builder,
            env,
            vars: HashMap::new(),
            i64_t,
            i32_t,
            ptr_t,
            strlen,
            malloc,
            memcpy,
            printf,
            fprintf,
            fflush,
            fgets,
            stdin,
            stderr,
            readline_buf,
            stdout,
            next_lit: 0,
        }
    }

    fn str_lit(&mut self, s: &str) -> PointerValue<'ctx> {
        self.next_lit += 1;
        let name = format!("str_{}", self.next_lit);
        self.builder
            .build_global_string_ptr(s, &name)
            .expect("global string")
            .as_pointer_value()
    }

    fn c_str_lit(&mut self, s: &str) -> PointerValue<'ctx> {
        self.next_lit += 1;
        let name = format!("fmt_{}", self.next_lit);
        self.builder
            .build_global_string_ptr(s, &name)
            .expect("fmt")
            .as_pointer_value()
    }

    pub fn emit(mut self, program: &Program) -> Result<Module<'ctx>, String> {
        let fn_type = self.i32_t.fn_type(&[], false);
        let main_fn = self.module.add_function("main", fn_type, None);
        let entry = self.context.append_basic_block(main_fn, "entry");
        self.builder.position_at_end(entry);

        for (name, ty) in &self.env.vars {
            let slot = match ty {
                Ty::Int => self
                    .builder
                    .build_alloca(self.i64_t, name)
                    .expect("alloca"),
                Ty::Str => self
                    .builder
                    .build_alloca(self.ptr_t, name)
                    .expect("alloca"),
                Ty::Unit => continue,
            };
            self.vars.insert(name.clone(), slot);
        }

        for stmt in &program.stmts {
            self.emit_stmt(stmt)?;
        }

        let zero = self.i32_t.const_int(0, false);
        self.builder.build_return(Some(&zero)).expect("ret");

        Ok(self.module)
    }

    fn emit_stmt(&mut self, stmt: &Stmt) -> Result<(), String> {
        match stmt {
            Stmt::Assign { name, value } => {
                let v = self.emit_expr(value)?;
                let slot = self.vars.get(name).expect("slot");
                match v {
                    GenVal::Int(iv) => {
                        self.builder.build_store(*slot, iv).expect("store");
                    }
                    GenVal::Str(pv) => {
                        self.builder.build_store(*slot, pv).expect("store");
                    }
                    GenVal::Unit => return Err("internal: unit assign".to_string()),
                }
            }
            Stmt::Expr(e) => {
                self.emit_expr(e)?;
            }
        }
        Ok(())
    }

    fn emit_expr(&mut self, expr: &Expr) -> Result<GenVal<'ctx>, String> {
        match expr {
            Expr::Int(n) => Ok(GenVal::Int(self.i64_t.const_int(*n as u64, true))),
            Expr::Str(s) => Ok(GenVal::Str(self.str_lit(s.as_str()))),
            Expr::Var(name) => {
                let slot = self
                    .vars
                    .get(name)
                    .ok_or_else(|| format!("unknown var `{}`", name))?;
                let ty = self.env.vars.get(name).expect("ty");
                let v = self.builder.build_load(*slot, name).expect("load");
                match ty {
                    Ty::Int => Ok(GenVal::Int(v.into_int_value())),
                    Ty::Str => Ok(GenVal::Str(v.into_pointer_value())),
                    Ty::Unit => Err("internal".to_string()),
                }
            }
            Expr::Binary { op, left, right } => {
                let l = self.emit_expr(left)?;
                let r = self.emit_expr(right)?;
                match (l, r, op) {
                    (GenVal::Int(a), GenVal::Int(b), BinOp::Add) => Ok(GenVal::Int(
                        self.builder
                            .build_int_add(a, b, "add")
                            .expect("add"),
                    )),
                    (GenVal::Int(a), GenVal::Int(b), BinOp::Sub) => Ok(GenVal::Int(
                        self.builder
                            .build_int_sub(a, b, "sub")
                            .expect("sub"),
                    )),
                    (GenVal::Int(a), GenVal::Int(b), BinOp::Mul) => Ok(GenVal::Int(
                        self.builder
                            .build_int_mul(a, b, "mul")
                            .expect("mul"),
                    )),
                    (GenVal::Int(a), GenVal::Int(b), BinOp::Div) => Ok(GenVal::Int(
                        self.builder
                            .build_int_signed_div(a, b, "div")
                            .expect("div"),
                    )),
                    (GenVal::Str(a), GenVal::Str(b), BinOp::Add) => {
                        Ok(GenVal::Str(self.concat_str(a, b)?))
                    }
                    _ => Err("internal: bad binary".to_string()),
                }
            }
            Expr::Call { name, args } => self.emit_call(name, args),
        }
    }

    fn concat_str(
        &mut self,
        a: PointerValue<'ctx>,
        b: PointerValue<'ctx>,
    ) -> Result<PointerValue<'ctx>, String> {
        let la = self
            .builder
            .build_call(self.strlen, &[a.into()], "la")
            .expect("strlen")
            .try_as_basic_value()
            .unwrap_basic()
            .into_int_value();
        let lb = self
            .builder
            .build_call(self.strlen, &[b.into()], "lb")
            .expect("strlen")
            .try_as_basic_value()
            .unwrap_basic()
            .into_int_value();
        let one = self.i64_t.const_int(1, false);
        let total = self
            .builder
            .build_int_add(la, lb, "l12")
            .expect("add");
        let total = self
            .builder
            .build_int_add(total, one, "tot")
            .expect("add");
        let p = self
            .builder
            .build_call(self.malloc, &[total.into()], "mp")
            .expect("malloc")
            .try_as_basic_value()
            .unwrap_basic()
            .into_pointer_value();
        self.builder
            .build_call(self.memcpy, &[p.into(), a.into(), la.into()], "c1")
            .expect("memcpy");
        let p2 = unsafe {
            self.builder
                .build_gep(p, &[la], "p2")
                .expect("gep")
        };
        let lb1 = self
            .builder
            .build_int_add(lb, one, "lb1")
            .expect("add");
        self.builder
            .build_call(self.memcpy, &[p2.into(), b.into(), lb1.into()], "c2")
            .expect("memcpy");
        Ok(p)
    }

    fn emit_call(&mut self, name: &str, args: &[Expr]) -> Result<GenVal<'ctx>, String> {
        match name {
            "print" => {
                let a = self.emit_expr(&args[0])?;
                match a {
                    GenVal::Int(v) => {
                        let fmt = self.c_str_lit("%lld\n");
                        self.builder
                            .build_call(self.printf, &[fmt.into(), v.into()], "p")
                            .expect("printf");
                    }
                    GenVal::Str(v) => {
                        let fmt = self.c_str_lit("%s\n");
                        self.builder
                            .build_call(self.printf, &[fmt.into(), v.into()], "p")
                            .expect("printf");
                    }
                    GenVal::Unit => return Err("internal".to_string()),
                }
                Ok(GenVal::Unit)
            }
            "eprint" => {
                let a = self.emit_expr(&args[0])?;
                let err = self
                    .builder
                    .build_load(self.stderr.as_pointer_value(), "stderr")
                    .expect("load")
                    .into_pointer_value();
                match a {
                    GenVal::Int(v) => {
                        let fmt = self.c_str_lit("%lld\n");
                        self.builder
                            .build_call(self.fprintf, &[err.into(), fmt.into(), v.into()], "e")
                            .expect("fprintf");
                    }
                    GenVal::Str(v) => {
                        let fmt = self.c_str_lit("%s\n");
                        self.builder
                            .build_call(self.fprintf, &[err.into(), fmt.into(), v.into()], "e")
                            .expect("fprintf");
                    }
                    GenVal::Unit => return Err("internal".to_string()),
                }
                Ok(GenVal::Unit)
            }
            "read_line" => {
                let z = self.i32_t.const_int(0, false);
                let buf_ptr = unsafe {
                    self.builder
                        .build_gep(
                            self.readline_buf.as_pointer_value(),
                            &[z, z],
                            "readbuf",
                        )
                        .expect("gep")
                };
                let cap = self.i32_t.const_int(4096, false);
                let sin = self
                    .builder
                    .build_load(self.stdin.as_pointer_value(), "stdin")
                    .expect("load")
                    .into_pointer_value();
                self.builder
                    .build_call(
                        self.fgets,
                        &[buf_ptr.into(), cap.into(), sin.into()],
                        "fg",
                    )
                    .expect("fgets");
                Ok(GenVal::Str(buf_ptr))
            }
            "prompt" => {
                let msg = self.emit_expr(&args[0])?;
                let GenVal::Str(msg) = msg else {
                    return Err("internal".to_string());
                };
                let fmt = self.c_str_lit("%s");
                self.builder
                    .build_call(self.printf, &[fmt.into(), msg.into()], "pm")
                    .expect("printf");
                let out = self
                    .builder
                    .build_load(self.stdout.as_pointer_value(), "stdout")
                    .expect("load")
                    .into_pointer_value();
                self.builder
                    .build_call(self.fflush, &[out.into()], "fl")
                    .expect("fflush");
                self.emit_call("read_line", &[])
            }
            "len" => {
                let s = self.emit_expr(&args[0])?;
                let GenVal::Str(s) = s else {
                    return Err("internal".to_string());
                };
                let n = self
                    .builder
                    .build_call(self.strlen, &[s.into()], "ln")
                    .expect("strlen")
                    .try_as_basic_value()
                    .unwrap_basic()
                    .into_int_value();
                Ok(GenVal::Int(n))
            }
            _ => Err(format!("unknown call `{}`", name)),
        }
    }
}

pub fn compile_to_ir(
    context: &Context,
    program: &Program,
    env: TypeEnv,
) -> Result<String, String> {
    let cg = CodeGen::new(context, env);
    let module = cg.emit(program)?;
    Ok(module.print_to_string().to_string())
}

