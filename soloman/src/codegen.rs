//! LLVM IR generation via Inkwell.

use std::collections::HashMap;

use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::{Linkage, Module};
use inkwell::types::{BasicType, BasicTypeEnum, StructType};
use inkwell::values::{BasicMetadataValueEnum, FunctionValue, IntValue, PointerValue};
use inkwell::AddressSpace;

use crate::ast::{BinOp, Expr, Item, Program, Stmt, Ty};
use crate::typecheck::TypeEnv;

enum GenVal<'ctx> {
    Int(IntValue<'ctx>),
    Str(PointerValue<'ctx>),
    Obj(PointerValue<'ctx>, String), // pointer + object name
    Unit,
}

pub struct CodeGen<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    env: TypeEnv,
    i64_t: inkwell::types::IntType<'ctx>,
    i32_t: inkwell::types::IntType<'ctx>,
    i8_ptr_t: inkwell::types::PointerType<'ctx>,

    // runtime
    strlen: FunctionValue<'ctx>,
    malloc: FunctionValue<'ctx>,
    memcpy: FunctionValue<'ctx>,
    printf: FunctionValue<'ctx>,
    fprintf: FunctionValue<'ctx>,
    fflush: FunctionValue<'ctx>,
    fgets: FunctionValue<'ctx>,
    stdin: inkwell::values::GlobalValue<'ctx>,
    stdout: inkwell::values::GlobalValue<'ctx>,
    stderr: inkwell::values::GlobalValue<'ctx>,
    readline_buf: inkwell::values::GlobalValue<'ctx>,

    // types / symbols
    obj_types: HashMap<String, StructType<'ctx>>,
    fn_vals: HashMap<String, FunctionValue<'ctx>>,

    // current function locals
    locals: HashMap<String, (Ty, PointerValue<'ctx>)>,
    next_lit: u32,
}

impl<'ctx> CodeGen<'ctx> {
    pub fn new(context: &'ctx Context, env: TypeEnv) -> Self {
        let module = context.create_module("soloman");
        let builder = context.create_builder();
        let i64_t = context.i64_type();
        let i32_t = context.i32_type();
        let i8_ptr_t = context
            .i8_type()
            .ptr_type(AddressSpace::default());

        let strlen = module.add_function(
            "strlen",
            i64_t.fn_type(&[i8_ptr_t.into()], false),
            Some(Linkage::External),
        );
        let malloc = module.add_function(
            "malloc",
            i8_ptr_t.fn_type(&[i64_t.into()], false),
            Some(Linkage::External),
        );
        let memcpy = module.add_function(
            "memcpy",
            i8_ptr_t.fn_type(&[i8_ptr_t.into(), i8_ptr_t.into(), i64_t.into()], false),
            Some(Linkage::External),
        );
        let printf = module.add_function(
            "printf",
            i32_t.fn_type(&[i8_ptr_t.into()], true),
            Some(Linkage::External),
        );
        let fprintf = module.add_function(
            "fprintf",
            i32_t.fn_type(&[i8_ptr_t.into(), i8_ptr_t.into()], true),
            Some(Linkage::External),
        );
        let fflush = module.add_function(
            "fflush",
            i32_t.fn_type(&[i8_ptr_t.into()], false),
            Some(Linkage::External),
        );
        let fgets = module.add_function(
            "fgets",
            i8_ptr_t.fn_type(&[i8_ptr_t.into(), i32_t.into(), i8_ptr_t.into()], false),
            Some(Linkage::External),
        );

        let stdin = module.add_global(i8_ptr_t, Some(AddressSpace::default()), "stdin");
        stdin.set_linkage(Linkage::External);
        let stdout = module.add_global(i8_ptr_t, Some(AddressSpace::default()), "stdout");
        stdout.set_linkage(Linkage::External);
        let stderr = module.add_global(i8_ptr_t, Some(AddressSpace::default()), "stderr");
        stderr.set_linkage(Linkage::External);

        let buf_ty = context.i8_type().array_type(4096);
        let readline_buf =
            module.add_global(buf_ty, Some(AddressSpace::default()), "sol_readline_buf");
        readline_buf.set_linkage(Linkage::Internal);
        readline_buf.set_initializer(&buf_ty.const_zero());

        Self {
            context,
            module,
            builder,
            env,
            i64_t,
            i32_t,
            i8_ptr_t,
            strlen,
            malloc,
            memcpy,
            printf,
            fprintf,
            fflush,
            fgets,
            stdin,
            stdout,
            stderr,
            readline_buf,
            obj_types: HashMap::new(),
            fn_vals: HashMap::new(),
            locals: HashMap::new(),
            next_lit: 0,
        }
    }

    pub fn emit(mut self, program: &Program) -> Result<Module<'ctx>, String> {
        self.declare_object_types()?;
        self.declare_functions(program)?;
        self.define_functions(program)?;
        self.define_main(program)?;
        Ok(self.module)
    }

    fn declare_object_types(&mut self) -> Result<(), String> {
        // Create opaque structs first to allow self-references (by pointer).
        for name in self.env.objects.keys() {
            let st = self.context.opaque_struct_type(name);
            self.obj_types.insert(name.clone(), st);
        }
        // Fill bodies.
        let objs: Vec<(String, Vec<(String, Ty)>)> = self
            .env
            .objects
            .iter()
            .map(|(n, info)| (n.clone(), info.fields.clone()))
            .collect();
        for (name, field_list) in objs {
            let mut fields = Vec::new();
            for (_, ty) in &field_list {
                fields.push(self.llvm_field_ty(ty)?);
            }
            let st = self.obj_types.get(&name).unwrap();
            st.set_body(&fields, false);
        }
        Ok(())
    }

    fn declare_functions(&mut self, program: &Program) -> Result<(), String> {
        for item in &program.items {
            if let Item::Function(f) = item {
                let sig = self
                    .env
                    .functions
                    .get(&f.name)
                    .ok_or_else(|| "internal: sig".to_string())?
                    .clone();
                let mut params = Vec::new();
                for t in &sig.params {
                    params.push(self.llvm_param_ty(t)?);
                }
                let fn_ty = match sig.ret {
                    Ty::Unit => self.context.void_type().fn_type(&params, false),
                    _ => self.llvm_value_ty(&sig.ret)?.fn_type(&params, false),
                };
                let fv = self.module.add_function(&f.name, fn_ty, None);
                self.fn_vals.insert(f.name.clone(), fv);
            }
        }
        Ok(())
    }

    fn define_functions(&mut self, program: &Program) -> Result<(), String> {
        for item in &program.items {
            if let Item::Function(f) = item {
                self.define_function(f)?;
            }
        }
        Ok(())
    }

    fn define_function(&mut self, f: &crate::ast::FnDef) -> Result<(), String> {
        let fv = *self.fn_vals.get(&f.name).expect("fn val");
        let entry = self.context.append_basic_block(fv, "entry");
        self.builder.position_at_end(entry);

        self.locals.clear();

        // Allocate + store params into locals.
        for (i, p) in f.params.iter().enumerate() {
            let arg = fv.get_nth_param(i as u32).expect("arg");
            arg.set_name(&p.name);
            let slot = self.alloca_for(&p.ty, &p.name)?;
            self.builder.build_store(slot, arg).expect("store");
            self.locals.insert(p.name.clone(), (p.ty.clone(), slot));
        }

        // Pre-allocate locals from `let` statements (simple scan).
        for stmt in &f.body {
            if let Stmt::Let { name, ty, .. } = stmt {
                let slot = self.alloca_for(ty, name)?;
                self.locals.insert(name.clone(), (ty.clone(), slot));
            }
        }

        for stmt in &f.body {
            self.emit_stmt(stmt)?;
        }

        // Implicit return for Unit.
        let sig = self.env.functions.get(&f.name).expect("sig");
        if sig.ret == Ty::Unit && self.builder.get_insert_block().unwrap().get_terminator().is_none()
        {
            self.builder.build_return(None).expect("ret");
        }

        Ok(())
    }

    fn define_main(&mut self, program: &Program) -> Result<(), String> {
        let fn_type = self.i32_t.fn_type(&[], false);
        let main_fn = self.module.add_function("main", fn_type, None);
        let entry = self.context.append_basic_block(main_fn, "entry");
        self.builder.position_at_end(entry);
        self.locals.clear();

        // Pre-allocate locals from top-level lets.
        for stmt in &program.stmts {
            if let Stmt::Let { name, ty, .. } = stmt {
                let slot = self.alloca_for(ty, name)?;
                self.locals.insert(name.clone(), (ty.clone(), slot));
            }
        }

        for stmt in &program.stmts {
            self.emit_stmt(stmt)?;
        }

        let zero = self.i32_t.const_int(0, false);
        self.builder.build_return(Some(&zero)).expect("ret");
        Ok(())
    }

    fn alloca_for(&mut self, ty: &Ty, name: &str) -> Result<PointerValue<'ctx>, String> {
        let bt = match ty {
            Ty::Int => BasicTypeEnum::IntType(self.i64_t),
            Ty::Str => BasicTypeEnum::PointerType(self.i8_ptr_t),
            Ty::Unit => return Err("cannot allocate Unit".to_string()),
            Ty::Object(n) => {
                let st = self
                    .obj_types
                    .get(n)
                    .ok_or_else(|| format!("unknown object `{}`", n))?;
                BasicTypeEnum::PointerType(st.ptr_type(AddressSpace::default()))
            }
        };
        Ok(self.builder.build_alloca(bt, name).expect("alloca"))
    }

    fn llvm_value_ty(&mut self, ty: &Ty) -> Result<inkwell::types::BasicTypeEnum<'ctx>, String> {
        Ok(match ty {
            Ty::Int => self.i64_t.into(),
            Ty::Str => self.i8_ptr_t.into(),
            Ty::Unit => return Err("Unit has no value type".to_string()),
            Ty::Object(n) => {
                let st = self
                    .obj_types
                    .get(n)
                    .ok_or_else(|| format!("unknown object `{}`", n))?;
                st.ptr_type(AddressSpace::default()).into()
            }
        })
    }

    fn llvm_param_ty(&mut self, ty: &Ty) -> Result<inkwell::types::BasicMetadataTypeEnum<'ctx>, String>
    {
        Ok(self.llvm_value_ty(ty)?.into())
    }

    fn llvm_field_ty(&mut self, ty: &Ty) -> Result<inkwell::types::BasicTypeEnum<'ctx>, String> {
        // Objects are stored by pointer inside other objects.
        self.llvm_value_ty(ty)
    }

    fn emit_stmt(&mut self, stmt: &Stmt) -> Result<(), String> {
        match stmt {
            Stmt::Let { name, ty, value } => {
                let (_, slot) = self.locals.get(name).expect("slot").clone();
                let v = self.emit_expr(value)?;
                self.store_typed(slot, ty, v)?;
                Ok(())
            }
            Stmt::Assign { name, value } => {
                let (ty, slot) = self
                    .locals
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("unknown local `{}`", name))?;
                let v = self.emit_expr(value)?;
                self.store_typed(slot, &ty, v)?;
                Ok(())
            }
            Stmt::Return(e) => {
                match e {
                    None => {
                        self.builder.build_return(None).expect("ret");
                    }
                    Some(expr) => {
                        let v = self.emit_expr(expr)?;
                        match v {
                            GenVal::Int(i) => {
                                self.builder.build_return(Some(&i)).expect("ret");
                            }
                            GenVal::Str(s) => {
                                self.builder.build_return(Some(&s)).expect("ret");
                            }
                            GenVal::Obj(p, _) => {
                                self.builder.build_return(Some(&p)).expect("ret");
                            }
                            GenVal::Unit => {
                                self.builder.build_return(None).expect("ret");
                            }
                        }
                    }
                }
                Ok(())
            }
            Stmt::Expr(e) => {
                let _ = self.emit_expr(e)?;
                Ok(())
            }
        }
    }

    fn store_typed(&mut self, slot: PointerValue<'ctx>, ty: &Ty, v: GenVal<'ctx>) -> Result<(), String>
    {
        match (ty, v) {
            (Ty::Int, GenVal::Int(i)) => {
                self.builder.build_store(slot, i).expect("store");
                Ok(())
            }
            (Ty::Str, GenVal::Str(s)) => {
                self.builder.build_store(slot, s).expect("store");
                Ok(())
            }
            (Ty::Object(n), GenVal::Obj(p, on)) if n == &on => {
                self.builder.build_store(slot, p).expect("store");
                Ok(())
            }
            (Ty::Unit, GenVal::Unit) => Ok(()),
            _ => Err("internal: store type mismatch".to_string()),
        }
    }

    fn emit_expr(&mut self, expr: &Expr) -> Result<GenVal<'ctx>, String> {
        match expr {
            Expr::Int(n) => Ok(GenVal::Int(self.i64_t.const_int(*n as u64, true))),
            Expr::Str(s) => Ok(GenVal::Str(self.str_lit(s.as_str()))),
            Expr::Var(name) => {
                let (ty, slot) = self
                    .locals
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("unknown variable `{}`", name))?;
                let v = self.builder.build_load(slot, name).expect("load");
                Ok(match ty {
                    Ty::Int => GenVal::Int(v.into_int_value()),
                    Ty::Str => GenVal::Str(v.into_pointer_value()),
                    Ty::Object(n) => GenVal::Obj(v.into_pointer_value(), n),
                    Ty::Unit => GenVal::Unit,
                })
            }
            Expr::Binary { op, left, right } => {
                let l = self.emit_expr(left)?;
                let r = self.emit_expr(right)?;
                match (l, r, op) {
                    (GenVal::Int(a), GenVal::Int(b), BinOp::Add) => Ok(GenVal::Int(
                        self.builder.build_int_add(a, b, "add").expect("add"),
                    )),
                    (GenVal::Int(a), GenVal::Int(b), BinOp::Sub) => Ok(GenVal::Int(
                        self.builder.build_int_sub(a, b, "sub").expect("sub"),
                    )),
                    (GenVal::Int(a), GenVal::Int(b), BinOp::Mul) => Ok(GenVal::Int(
                        self.builder.build_int_mul(a, b, "mul").expect("mul"),
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
            Expr::ObjLit { object, fields } => self.emit_obj_lit(object, fields),
            Expr::Field { base, name } => self.emit_field(base, name),
        }
    }

    fn emit_field(&mut self, base: &Expr, field: &str) -> Result<GenVal<'ctx>, String> {
        let GenVal::Obj(ptr, obj_name) = self.emit_expr(base)? else {
            return Err("internal: field base".to_string());
        };
        let info = self
            .env
            .objects
            .get(&obj_name)
            .ok_or_else(|| "internal: obj".to_string())?;
        let idx = *info
            .index
            .get(field)
            .ok_or_else(|| "internal: field".to_string())?;
        let gep = self
            .builder
            .build_struct_gep(ptr, idx as u32, "fldp")
            .map_err(|e| e.to_string())?;
        let v = self.builder.build_load(gep, "fld").expect("load");
        let ty = &info.fields[idx].1;
        Ok(match ty {
            Ty::Int => GenVal::Int(v.into_int_value()),
            Ty::Str => GenVal::Str(v.into_pointer_value()),
            Ty::Object(n) => GenVal::Obj(v.into_pointer_value(), n.clone()),
            Ty::Unit => GenVal::Unit,
        })
    }

    fn emit_obj_lit(&mut self, object: &str, fields: &[(String, Expr)]) -> Result<GenVal<'ctx>, String>
    {
        let st = self
            .obj_types
            .get(object)
            .ok_or_else(|| format!("unknown object `{}`", object))?
            .to_owned();
        let ptr = self
            .builder
            .build_alloca(st, &format!("{}_tmp", object))
            .expect("alloca");

        let info = self.env.objects.get(object).expect("info").clone();
        for (fname, fexpr) in fields {
            let idx = *info.index.get(fname).expect("idx");
            let fptr = self
                .builder
                .build_struct_gep(ptr, idx as u32, "fptr")
                .map_err(|e| e.to_string())?;
            let v = self.emit_expr(fexpr)?;
            match (info.fields[idx].1.clone(), v) {
                (Ty::Int, GenVal::Int(i)) => {
                    self.builder.build_store(fptr, i).expect("store");
                }
                (Ty::Str, GenVal::Str(s)) => {
                    self.builder.build_store(fptr, s).expect("store");
                }
                (Ty::Object(tn), GenVal::Obj(p, on)) if tn == on => {
                    self.builder.build_store(fptr, p).expect("store");
                }
                _ => return Err("internal: objlit store mismatch".to_string()),
            }
        }

        Ok(GenVal::Obj(
            ptr,
            object.to_string(),
        ))
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
                    _ => return Err("internal".to_string()),
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
                    _ => return Err("internal".to_string()),
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
                    .build_call(self.fgets, &[buf_ptr.into(), cap.into(), sin.into()], "fg")
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
            _ => {
                let fv = *self
                    .fn_vals
                    .get(name)
                    .ok_or_else(|| format!("unknown function `{}`", name))?;
                let mut av = Vec::new();
                for a in args {
                    let gv = self.emit_expr(a)?;
                    av.push(Self::gen_to_meta(gv)?);
                }
                let cs = self.builder.build_call(fv, &av, "call").expect("call");
                if fv.get_type().get_return_type().is_none() {
                    Ok(GenVal::Unit)
                } else {
                    let v = cs.try_as_basic_value().unwrap_basic();
                    // Determine return type from type env.
                    let ret = self.env.functions.get(name).unwrap().ret.clone();
                    Ok(match ret {
                        Ty::Int => GenVal::Int(v.into_int_value()),
                        Ty::Str => GenVal::Str(v.into_pointer_value()),
                        Ty::Object(n) => GenVal::Obj(v.into_pointer_value(), n),
                        Ty::Unit => GenVal::Unit,
                    })
                }
            }
        }
    }

    fn gen_to_meta(v: GenVal<'ctx>) -> Result<BasicMetadataValueEnum<'ctx>, String> {
        Ok(match v {
            GenVal::Int(i) => i.into(),
            GenVal::Str(p) => p.into(),
            GenVal::Obj(p, _) => p.into(),
            GenVal::Unit => return Err("Unit cannot be passed as arg".to_string()),
        })
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

    fn concat_str(&mut self, a: PointerValue<'ctx>, b: PointerValue<'ctx>) -> Result<PointerValue<'ctx>, String>
    {
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
        let total = self.builder.build_int_add(la, lb, "l12").expect("add");
        let total = self.builder.build_int_add(total, one, "tot").expect("add");
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
        let p2 = unsafe { self.builder.build_gep(p, &[la], "p2").expect("gep") };
        let lb1 = self.builder.build_int_add(lb, one, "lb1").expect("add");
        self.builder
            .build_call(self.memcpy, &[p2.into(), b.into(), lb1.into()], "c2")
            .expect("memcpy");
        Ok(p)
    }
}

pub fn compile_to_ir(context: &Context, program: &Program, env: TypeEnv) -> Result<String, String> {
    let cg = CodeGen::new(context, env);
    let module = cg.emit(program)?;
    Ok(module.print_to_string().to_string())
}

#[cfg(test)]
mod tests {
    use inkwell::context::Context;

    use crate::{lexer::tokenize, parser::Parser, typecheck::check_program};

    use super::compile_to_ir;

    fn compile(src: &str) -> String {
        let toks = tokenize(src).expect("tokenize");
        let program = Parser::new(toks).parse_program().expect("parse");
        let env = check_program(&program).expect("typecheck");
        let ctx = Context::create();
        compile_to_ir(&ctx, &program, env).expect("ir")
    }

    #[test]
    fn emits_ir_for_functions_and_objects() {
        let ir = compile(
            r#"
            object Point { x: Int; y: Int; }
            fn add(a: Int, b: Int) -> Int { return a + b; }
            fn sum(p: Point) -> Int { return p.x + p.y; }
            let p: Point = Point{ x: 3, y: 4 };
            let n: Int = add(20, 22);
            print(n);
            print(sum(p));
            "#,
        );
        assert!(ir.contains("define i32 @main()"));
        assert!(ir.contains("define i64 @add"));
        assert!(ir.contains("%Point = type"));
    }

    #[test]
    fn emits_ir_for_string_concat_and_len() {
        let ir = compile(
            r#"
            fn shout(s: Str) -> Str { return s + "!"; }
            let x: Str = shout("ok");
            let n: Int = len(x);
            print(n);
            "#,
        );
        assert!(ir.contains("@malloc"));
        assert!(ir.contains("@strlen"));
    }
}

