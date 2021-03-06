use std::collections::BTreeMap;
use std::ops::{Index, IndexMut};

use cranelift_entity::{entity_impl, PrimaryMap};

use crate::{Function, FunctionIdent};
use libeir_diagnostics::SourceSpan;
use libeir_intern::{Ident, Symbol};

pub struct FunctionDefinition {
    index: FunctionIndex,
    fun: Function,
}
impl FunctionDefinition {
    pub fn index(&self) -> FunctionIndex {
        self.index
    }

    pub fn function(&self) -> &Function {
        &self.fun
    }

    pub fn function_mut(&mut self) -> &mut Function {
        &mut self.fun
    }
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FunctionIndex(u32);
entity_impl!(FunctionIndex, "function_index");

pub struct Module {
    name: Ident,
    span: SourceSpan,
    functions: PrimaryMap<FunctionIndex, FunctionDefinition>,
    name_map: BTreeMap<(Symbol, usize), FunctionIndex>,
}
impl Module {
    pub fn new(name: Ident) -> Self {
        Self {
            name,
            span: SourceSpan::UNKNOWN,
            functions: PrimaryMap::new(),
            name_map: BTreeMap::new(),
        }
    }

    pub fn new_with_span(name: Ident, span: SourceSpan) -> Self {
        Self {
            name,
            span,
            functions: PrimaryMap::new(),
            name_map: BTreeMap::new(),
        }
    }

    pub fn name(&self) -> Ident {
        self.name
    }

    pub fn span(&self) -> SourceSpan {
        self.span
    }

    pub fn add_function(
        &mut self,
        span: SourceSpan,
        name: Ident,
        arity: usize,
    ) -> &mut FunctionDefinition {
        let ident = FunctionIdent {
            module: self.name,
            name,
            arity,
        };
        assert!(!self.name_map.contains_key(&(name.name, arity)));

        let fun = Function::new(span, ident);
        let def = FunctionDefinition {
            index: FunctionIndex(0),
            fun,
        };

        let index = self.functions.push(def);
        self.name_map.insert((name.name, arity), index);

        let def_mut = self.functions.get_mut(index).unwrap();
        def_mut.index = index;
        def_mut
    }

    pub fn ident_index(&self, ident: &FunctionIdent) -> Option<FunctionIndex> {
        self.name_map.get(&(ident.name.name, ident.arity)).cloned()
    }
    pub fn name_arity_index(&self, name: Symbol, arity: usize) -> Option<FunctionIndex> {
        self.name_map.get(&(name, arity)).cloned()
    }

    pub fn function_iter(&self) -> impl Iterator<Item = &FunctionDefinition> {
        self.functions.values()
    }
    pub fn function_iter_mut(&mut self) -> impl Iterator<Item = &mut FunctionDefinition> {
        self.functions.values_mut()
    }

    pub fn index_iter(&self) -> impl Iterator<Item = FunctionIndex> {
        self.functions.keys()
    }
}
impl Clone for Module {
    fn clone(&self) -> Self {
        let mut functions: PrimaryMap<FunctionIndex, FunctionDefinition> = PrimaryMap::new();
        let mut name_map = BTreeMap::new();
        for def in self.function_iter() {
            let fun = def.function();
            let ident = fun.ident();
            let def = FunctionDefinition {
                index: FunctionIndex(0),
                fun: fun.clone(),
            };
            let index = functions.push(def);
            name_map.insert((ident.name.name, ident.arity), index);
        }
        Self {
            name: self.name.clone(),
            span: self.span,
            functions,
            name_map,
        }
    }
}

impl Index<FunctionIndex> for Module {
    type Output = FunctionDefinition;
    fn index(&self, idx: FunctionIndex) -> &FunctionDefinition {
        &self.functions[idx]
    }
}
impl IndexMut<FunctionIndex> for Module {
    fn index_mut(&mut self, idx: FunctionIndex) -> &mut FunctionDefinition {
        &mut self.functions[idx]
    }
}

impl Index<&FunctionIdent> for Module {
    type Output = FunctionDefinition;
    fn index(&self, ident: &FunctionIdent) -> &FunctionDefinition {
        let idx = self
            .ident_index(ident)
            .expect("function ident not in module");
        &self.functions[idx]
    }
}
