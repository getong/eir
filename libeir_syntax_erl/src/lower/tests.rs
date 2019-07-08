use crate::ast::*;
use crate::*;

use crate::lower::lower_module;

use libeir_ir::{ FunctionIdent, Module as IrModule };
use libeir_diagnostics::{ColorChoice, Emitter, StandardStreamEmitter};

use std::path::{ Path, PathBuf };

fn parse<T>(input: &str, config: ParseConfig) -> (T, Parser)
where
    T: Parse<T>,
{
    let parser = Parser::new(config);
    let errs = match parser.parse_string::<&str, T>(input) {
        Ok(ast) => return (ast, parser),
        Err(errs) => errs,
    };
    let emitter = StandardStreamEmitter::new(ColorChoice::Auto)
        .set_codemap(parser.config.codemap.clone());
    for err in errs.iter() {
        emitter.diagnostic(&err.to_diagnostic()).unwrap();
    }
    panic!("parse failed");
}

fn parse_file<T, P>(path: P, config: ParseConfig) -> (T, Parser)
where
    T: Parse<T>,
    P: AsRef<Path>,
{
    let parser = Parser::new(config);
    let errs = match parser.parse_file::<_, T>(path) {
        Ok(ast) => return (ast, parser),
        Err(errs) => errs,
    };
    let emitter = StandardStreamEmitter::new(ColorChoice::Auto)
        .set_codemap(parser.config.codemap.clone());
    for err in errs.iter() {
        emitter.diagnostic(&err.to_diagnostic()).unwrap();
    }
    panic!("parse failed");
}

fn lower_file<P>(path: P, config: ParseConfig) -> Result<IrModule, ()>
where
    P: AsRef<Path>
{
    let (parsed, parser): (Module, _) = parse_file(path, config);
    let (res, messages) = lower_module(&parsed);

    let emitter = StandardStreamEmitter::new(ColorChoice::Auto)
        .set_codemap(parser.config.codemap.clone());
    for err in messages.iter() {
        emitter.diagnostic(&err.to_diagnostic()).unwrap();
    }

    res
}

fn lower(input: &str, config: ParseConfig) -> Result<IrModule, ()> {
    let (parsed, parser): (Module, _) = parse(input, config);
    let (res, messages) = lower_module(&parsed);

    let emitter = StandardStreamEmitter::new(ColorChoice::Auto)
        .set_codemap(parser.config.codemap.clone());
    for err in messages.iter() {
        emitter.diagnostic(&err.to_diagnostic()).unwrap();
    }

    res
}

#[test]
fn fib_lower() {
    let result = lower(
        "-module(fib).

fib(X) when X < 2 -> 1;
fib(X) -> fib(X - 1) + fib(X-2).
",
        ParseConfig::default()
    ).unwrap();


}

#[test]
fn pat_1_lower() {
    let fun = lower(
        "-module(pat).
pat(A, A) -> 1.
",
        ParseConfig::default()
    ).unwrap();

    print!("{}", fun.to_text());
}

//#[test]
fn compiler_lower() {
    let mut config = ParseConfig::default();

    config.include_paths.push_front(PathBuf::from("../otp/lib/compiler/src/"));
    config.include_paths.push_front(PathBuf::from("../otp/bootstrap/lib/stdlib/include/"));

    let ir = lower_file("../otp/lib/compiler/src/compile.erl", config).unwrap();

    let ident = FunctionIdent {
        module: Ident::from_str("compile"),
        name: Ident::from_str("iofile"),
        arity: 1,
    };

    let mut errors = Vec::new();
    for fun in ir.functions.values() {
        //if fun.ident() == &ident {
            println!("{:?}", fun.ident());
        fun.validate(&mut errors);
        //}
    }
    println!("{:#?}", errors);

    let fun = &ir.functions[&ident];

    print!("{}", fun.to_text());

    let mut dot = Vec::<u8>::new();
    libeir_ir::text::dot_printer::function_to_dot(fun, &mut dot).unwrap();
    let dot_text = std::str::from_utf8(&dot).unwrap();
    print!("{}", dot_text);

}