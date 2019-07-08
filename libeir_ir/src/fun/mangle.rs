use std::collections::{ HashMap, HashSet };

use crate::{ Function, FunctionBuilder };
use crate::{ Value, Block, ValueType, OpKind };

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct EntryArg(usize);

/// Utility struct implementing the lambda mangling primitive.
/// Supports both mangling within a single function container, and
/// across function containers, implemented by `run` and `run_across`
/// respectively.
pub struct Mangler {
    entry: Option<Block>,
    num_args: usize,

    map: HashMap<RenameSource, RenameDest>,

    source_buf: Vec<RenameSource>,
    value_buf: Vec<Value>,
    mapped_buf: Vec<MappedValue>,

    scope: Option<HashSet<Block>>,
    to_walk: Vec<Block>,
}

impl Mangler {

    pub fn new() -> Self {
        Mangler {
            entry: None,
            num_args: 0,

            map: HashMap::new(),

            source_buf: Vec::new(),
            value_buf: Vec::new(),
            mapped_buf: Vec::new(),

            scope: Some(HashSet::new()),
            to_walk: Vec::new(),
        }
    }

    /// Clears the mangler for a new use.
    /// This is automatically done by the `start` method.
    fn clear(&mut self) {
        self.entry = None;
        self.num_args = 0;

        self.map.clear();

        self.source_buf.clear();
        self.value_buf.clear();
        self.mapped_buf.clear();

        self.scope.as_mut().unwrap().clear();
        self.to_walk.clear();
    }

    /// Clears the mangler of all existing data, and starts a new transaction
    /// for the given entry block.
    pub fn start(&mut self, block: Block) {
        self.clear();
        self.entry = Some(block);
    }

    /// Add an argument on the new entry block.
    /// The return value can be used to make renames using the `add_rename` method.
    pub fn add_argument(&mut self) -> EntryArg {
        let arg = EntryArg(self.num_args);
        self.num_args += 1;
        arg
    }

    pub fn add_rename<S, D>(&mut self, old: S, new: D)
    where
        S: Into<RenameSource>,
        D: Into<RenameDest>,
    {
        self.map.insert(old.into(), new.into());
    }

    /// Runs lambda mangling on a single function container
    pub fn run(&mut self, fun: &mut FunctionBuilder) -> Block {
        let mut recv = SingleMangleReceiver {
            fun,
        };
        self.run_inner(&mut recv)
    }

    // Runs lambda mangling while copying across function containers
    pub fn run_across(&mut self, from: &Function, to: &mut FunctionBuilder) -> Block {
        let mut recv = CopyMangleReceiver {
            from,
            to,
        };
        self.run_inner(&mut recv)
    }

    fn run_inner<'a, R>(&mut self, recv: &mut R) -> Block where R: MangleReceiver<'a> {
        let mut scope = self.scope.take().unwrap();

        // Insert new entry block
        let entry = self.entry.unwrap();
        let new_entry;
        {
            let to = recv.to();
            new_entry = to.block_insert();
            for _ in 0..self.num_args {
                to.block_arg_insert(new_entry);
            }
        }

        // Make sure no added rename sources are blocks
        {
            let from = recv.from();

            self.source_buf.clear();
            self.source_buf.extend(self.map.keys().cloned());

            for key in self.source_buf.drain(..) {
                let dest = self.map[&key];
                if let RenameSource::Value(val) = key {
                    if let Some(block) = from.value_block(val) {
                        let new_source = RenameSource::Block(block);
                        if let Some(existing) = self.map.get(&new_source) {
                            assert!(*existing == dest);
                        }
                        self.map.insert(new_source, dest);
                        self.map.remove(&RenameSource::Value(val));
                    }
                }
            }
        }

        // Walk scope
        {
            self.to_walk.push(entry);

            let from = recv.from();
            let graph = from.block_graph();

            while let Some(block) = self.to_walk.pop() {
                if scope.contains(&block) { continue; }

                if self.map.contains_key(&RenameSource::Block(block)) { continue; }
                scope.insert(block);

                for out in graph.outgoing(block) {
                    self.to_walk.push(out);
                }
            }
        }

        // Insert new blocks
        {
            for block in scope.iter().cloned() {
                assert!(!self.map.contains_key(&RenameSource::Block(block)));
                let new_block = recv.to().block_insert();
                self.map.insert(RenameSource::Block(block), RenameDest::Block(new_block));

                self.value_buf.clear();
                self.value_buf.extend(recv.from().block_args(block).iter().cloned());

                // Insert new arguments
                let to = recv.to();
                for arg in self.value_buf.iter().cloned() {
                    let new_arg = to.block_arg_insert(new_block);
                    if !self.map.contains_key(&arg.into()) {
                        self.map.insert(RenameSource::Value(arg), RenameDest::Value(new_arg));
                    }
                }
            }
        }

        let map_value = |mang: &Mangler, recv: &mut R, orig_value: Value| {
            let dest = {
                let from = recv.from();
                match *from.value_kind(orig_value) {
                    ValueType::Arg(_r_block) => {
                        if let Some(new) = mang.map.get(&RenameSource::Value(orig_value)) {
                            MappedValue::RenameDest(*new)
                        } else {
                            MappedValue::MapValue(orig_value)
                        }
                    }
                    ValueType::Constant(_) => {
                        MappedValue::MapConst(orig_value)
                    }
                    ValueType::Block(block) => {
                        MappedValue::RenameDest(*mang.map.get(
                            &RenameSource::Block(block)).unwrap())
                    }
                    _ => unreachable!(),
                }
            };

            match dest {
                MappedValue::MapConst(val) => recv.map_const(val),
                MappedValue::MapValue(val) => recv.map_free_value(val),
                MappedValue::RenameDest(RenameDest::Value(value)) => value,
                MappedValue::RenameDest(RenameDest::EntryArg(idx)) => {
                    recv.to().block_args(new_entry)[idx.0]
                },
                MappedValue::RenameDest(RenameDest::Block(block)) => {
                    recv.to().value(block)
                }
            }
        };

        let copy_body = |mang: &mut Mangler, recv: &mut R, from_block: Block, to_block: Block| {
            let to_op = recv.map_block_op(from_block);
            let span = recv.from().block_span(from_block);

            // Get and map reads to new values
            mang.value_buf.clear();
            mang.value_buf.extend(recv.from().block_reads(from_block).iter().cloned());
            for n in 0..mang.value_buf.len() {
                let mapped = map_value(mang, recv, mang.value_buf[n]);
                mang.value_buf[n] = mapped;
            }

            // Update the new block with kind, span and reads
            let to = recv.to();
            let to_fun = to.fun_mut();
            let data = &mut to_fun.blocks[to_block];

            data.op = Some(to_op);
            data.span = span;

            for read in mang.value_buf.iter().cloned() {
                data.reads.push(read, &mut to_fun.value_pool);
            }
        };

        // Set entry body
        copy_body(self, recv, entry, new_entry);

        // Set new bodies
        for block in scope.iter().cloned() {
            let new_block = self.map.get(&RenameSource::Block(block)).unwrap().block();
            copy_body(self, recv, block, new_block);
        }

        self.scope = Some(scope);
        self.clear();

        new_entry
    }

}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum RenameDest {
    Value(Value),
    Block(Block),
    EntryArg(EntryArg),
}
impl From<Value> for RenameDest {
    fn from(val: Value) -> Self {
        RenameDest::Value(val)
    }
}
impl From<Block> for RenameDest {
    fn from(val: Block) -> Self {
        RenameDest::Block(val)
    }
}
impl From<EntryArg> for RenameDest {
    fn from(val: EntryArg) -> Self {
        RenameDest::EntryArg(val)
    }
}
impl RenameDest {
    fn block(&self) -> Block {
        match self {
            RenameDest::Block(block) => *block,
            _ => panic!(),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum RenameSource {
    Value(Value),
    Block(Block),
}
impl From<Value> for RenameSource {
    fn from(val: Value) -> Self {
        RenameSource::Value(val)
    }
}
impl From<Block> for RenameSource {
    fn from(val: Block) -> Self {
        RenameSource::Block(val)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
enum MappedValue {
    RenameDest(RenameDest),
    MapConst(Value),
    MapValue(Value),
}

trait MangleReceiver<'b> {
    fn from<'a>(&'a mut self) -> &'a Function;
    fn to<'a>(&'a mut self) -> &'a mut FunctionBuilder<'b>;
    fn map_const(&mut self, val: Value) -> Value;
    fn map_free_value(&mut self, val: Value) -> Value;
    fn map_block_op(&mut self, block: Block) -> OpKind;
}

struct SingleMangleReceiver<'a, 'b> {
    fun: &'a mut FunctionBuilder<'b>,
}
impl<'b, 'c> MangleReceiver<'b> for SingleMangleReceiver<'c, 'b> {
    fn from<'a>(&'a mut self) -> &'a Function {
        self.fun.fun()
    }
    fn to<'a>(&'a mut self) -> &'a mut FunctionBuilder<'b> {
        self.fun
    }
    fn map_const(&mut self, val: Value) -> Value {
        val
    }
    fn map_free_value(&mut self, val: Value) -> Value {
        val
    }
    fn map_block_op(&mut self, block: Block) -> OpKind {
        self.fun.fun().block_kind(block).unwrap().clone()
    }
}

struct CopyMangleReceiver<'a, 'b> {
    from: &'a Function,
    to: &'a mut FunctionBuilder<'b>,
}
impl<'b, 'c> MangleReceiver<'b> for CopyMangleReceiver<'c, 'b> {
    fn from<'a>(&'a mut self) -> &'a Function {
        self.from
    }
    fn to<'a>(&'a mut self) -> &'a mut FunctionBuilder<'b> {
        self.to
    }
    fn map_const(&mut self, _val: Value) -> Value {
        unimplemented!()
    }
    fn map_free_value(&mut self, _val: Value) -> Value {
        panic!()
    }
    fn map_block_op(&mut self, _block: Block) -> OpKind {
        unimplemented!()
    }
}


#[cfg(test)]
mod tests {

    use crate::{ FunctionIdent, Function, FunctionBuilder };
    use crate::NilTerm;
    use libeir_intern::Ident;
    use super::Mangler;

    #[test]
    fn test_simple_mangle() {

        let ident = FunctionIdent {
            module: Ident::from_str("woo"),
            name: Ident::from_str("woo"),
            arity: 1,
        };
        let mut fun = Function::new(ident);
        let mut b = FunctionBuilder::new(&mut fun);

        let b1 = b.block_insert();
        let b1_ret = b.block_arg_insert(b1);
        let b1_arg = b.block_arg_insert(b1);

        let b2 = b.block_insert();

        b.op_call(b1, b2, &[]);
        b.op_call(b2, b1_ret, &[b1_arg]);

        let mut mangler = Mangler::new();

        let nil_term = b.value(NilTerm);
        mangler.start(b1);
        let ret_narg = mangler.add_argument();
        mangler.add_rename(b1_ret, ret_narg);
        mangler.add_rename(b1_arg, nil_term);
        let new_b1 = mangler.run(&mut b);

        {
            let b1_args = b.block_args(new_b1);
            assert!(b1_args.len() == 1);

            let b1_reads = b.block_reads(new_b1);
            assert!(b1_reads.len() == 1);
            let new_b2 = b.fun().value_block(b1_reads[0]).unwrap();
            assert!(new_b2 != b2);

            let b2_args = b.block_args(new_b2);
            assert!(b2_args.len() == 0);

            let b2_reads = b.block_reads(new_b2);
            assert!(b2_reads.len() == 2);
            assert!(b2_reads[0] == b1_args[0]);
            assert!(b2_reads[1] == nil_term);
        }

    }

}