use crate::ast;
use crate::collections::HashMap;
use crate::error::CompileError;
use crate::source::Source;
use crate::token::Token;
use crate::traits::Resolve as _;
use crate::ParseAll;
use st::unit::Span;

type Result<T, E = CompileError> = std::result::Result<T, E>;

/// Flag to indicate if the expression should produce a value or not.
#[derive(Debug, Clone, Copy)]
struct NeedsValue(bool);

impl<'a> crate::ParseAll<'a, ast::File> {
    /// Encode the given object into a collection of instructions.
    pub fn compile(self) -> Result<st::Unit> {
        let ParseAll { source, item: file } = self;

        let mut unit = st::Unit::with_default_prelude();

        for import in file.imports {
            let name = resolve_path(import.path, source)?;
            unit.new_import(&name)?;
        }

        for f in file.functions {
            let name = f.name.resolve(source)?;
            let count = f.args.items.len();

            let mut assembly = unit.new_assembly();

            let mut encoder = Encoder {
                unit: &mut unit,
                instructions: &mut assembly,
                parents: Vec::new(),
                locals: Locals::new(),
                source,
                loops: Vec::new(),
                references_at: Vec::new(),
                current_block: Span::empty(),
            };

            encoder.encode_fn_decl(f)?;
            unit.new_function(&[name], count, assembly)?;
        }

        Ok(unit)
    }
}

struct Encoder<'a> {
    unit: &'a mut st::Unit,
    instructions: &'a mut st::unit::Assembly,
    parents: Vec<Locals>,
    locals: Locals,
    source: Source<'a>,
    /// The nesting of loop we are currently in.
    loops: Vec<Loop>,
    /// The current block that we are in.
    current_block: Span,
    /// Indicates that a reference was taken at the given spans.
    references_at: Vec<Span>,
}

impl<'a> Encoder<'a> {
    fn encode_fn_decl(&mut self, fn_decl: ast::FnDecl) -> Result<()> {
        let span = fn_decl.span();

        for arg in fn_decl.args.items.iter().rev() {
            let name = arg.resolve(self.source)?;
            self.locals.new_local(name, arg.token, Vec::new())?;
        }

        if fn_decl.body.exprs.is_empty() && fn_decl.body.trailing_expr.is_none() {
            self.instructions.push(st::Inst::ReturnUnit, span);
            return Ok(());
        }

        for (expr, _) in &fn_decl.body.exprs {
            self.encode_expr(expr, NeedsValue(false))?;
        }

        if let Some(expr) = &fn_decl.body.trailing_expr {
            self.references_at.clear();
            self.encode_expr(expr, NeedsValue(true))?;

            if !self.references_at.is_empty() {
                return Err(CompileError::ReturnLocalReferences {
                    block: fn_decl.body.span(),
                    span: expr.span(),
                    references_at: self.references_at.clone(),
                });
            }

            self.clean_up_locals(self.locals.var_count, span);
            self.instructions.push(st::Inst::Return, span);
        } else {
            self.pop_locals(self.locals.var_count, span);
            self.instructions.push(st::Inst::ReturnUnit, span);
        }

        Ok(())
    }

    /// Pop locals by simply popping them.
    fn pop_locals(&mut self, var_count: usize, span: Span) {
        match var_count {
            0 => (),
            1 => {
                self.instructions.push(st::Inst::Pop, span);
            }
            count => {
                self.instructions.push(st::Inst::PopN { count }, span);
            }
        }
    }

    /// Clean up local variables by preserving the value that is on top and
    /// popping the rest.
    ///
    /// The clean operation will preserve the value that is on top of the stack,
    /// and pop the values under it.
    fn clean_up_locals(&mut self, var_count: usize, span: Span) {
        match var_count {
            0 => (),
            count => {
                self.instructions.push(st::Inst::Clean { count }, span);
            }
        }
    }

    /// Encode a block.
    ///
    /// Blocks are special in that they do not produce a value unless there is
    /// an item in them which does.
    fn encode_block(&mut self, block: &ast::Block, needs_value: NeedsValue) -> Result<()> {
        log::trace!("{:?}", block);

        let span = block.span();
        self.current_block = span;

        let open_var_count = self.locals.var_count;

        let parent_count = self.parents.len();
        self.parents.push(self.locals.clone());

        for (expr, _) in &block.exprs {
            // NB: terminated expressions do not need to produce a value.
            self.encode_expr(expr, NeedsValue(false))?;
        }

        if let Some(expr) = &block.trailing_expr {
            self.references_at.clear();
            self.encode_expr(expr, needs_value)?;

            if needs_value.0 && !self.references_at.is_empty() {
                return Err(CompileError::ReturnLocalReferences {
                    block: self.current_block,
                    span: expr.span(),
                    references_at: self.references_at.clone(),
                });
            }
        }

        let var_count = self.locals.var_count - open_var_count;

        if needs_value.0 {
            self.clean_up_locals(var_count, span);
        } else {
            self.pop_locals(var_count, span);
        }

        let parent = match self.parents.pop() {
            Some(parent) => parent,
            None => {
                return Err(CompileError::internal("missing parent scope", span));
            }
        };

        if self.parents.len() != parent_count {
            return Err(CompileError::internal(
                "parent scope mismatch at end of block",
                span,
            ));
        }

        self.locals = parent;
        Ok(())
    }

    /// Encode an expression.
    fn encode_expr(&mut self, expr: &ast::Expr, needs_value: NeedsValue) -> Result<()> {
        log::trace!("{:?}", expr);

        match expr {
            ast::Expr::While(while_) => {
                self.encode_while(while_, needs_value)?;
            }
            ast::Expr::Let(let_) => {
                self.encode_let(let_, needs_value)?;
            }
            ast::Expr::IndexSet(index_set) => {
                self.encode_index_set(index_set, needs_value)?;
            }
            ast::Expr::ExprGroup(expr) => {
                self.encode_expr(&*expr.expr, needs_value)?;
            }
            ast::Expr::Ident(ident) => {
                self.encode_ident(ident, needs_value)?;
            }
            ast::Expr::Path(path) => {
                self.encode_type(path, needs_value)?;
            }
            ast::Expr::CallFn(call_fn) => {
                self.encode_call_fn(call_fn, needs_value)?;
            }
            ast::Expr::CallInstanceFn(call_instance_fn) => {
                self.encode_call_instance_fn(call_instance_fn, needs_value)?;
            }
            ast::Expr::ExprUnary(expr_unary) => {
                self.encode_expr_unary(expr_unary, needs_value)?;
            }
            ast::Expr::ExprBinary(expr_binary) => {
                self.encode_expr_binary(expr_binary, needs_value)?;
            }
            ast::Expr::ExprIf(expr_if) => {
                self.encode_expr_if(expr_if, needs_value)?;
            }
            ast::Expr::UnitLiteral(unit) => {
                self.encode_unit_literal(unit)?;
            }
            ast::Expr::BoolLiteral(b) => {
                self.encode_bool_literal(b)?;
            }
            ast::Expr::NumberLiteral(number) => {
                self.encode_number_literal(number, needs_value)?;
            }
            ast::Expr::ArrayLiteral(array_literal) => {
                self.encode_array_literal(array_literal, needs_value)?;
            }
            ast::Expr::ObjectLiteral(object_literal) => {
                self.encode_object_literal(object_literal, needs_value)?;
            }
            ast::Expr::CharLiteral(string) => {
                self.encode_char_literal(string, needs_value)?;
            }
            ast::Expr::StringLiteral(string) => {
                self.encode_string_literal(string, needs_value)?;
            }
            ast::Expr::IndexGet(index_get) => {
                self.encode_index_get(index_get, needs_value)?;
            }
            ast::Expr::Break(b) => {
                self.encode_break(b, needs_value)?;
            }
        }

        Ok(())
    }

    fn encode_array_literal(
        &mut self,
        array_literal: &ast::ArrayLiteral,
        needs_value: NeedsValue,
    ) -> Result<()> {
        if !needs_value.0 && array_literal.is_all_literal() {
            // Don't encode unecessary literals.
            return Ok(());
        }

        let count = array_literal.items.len();

        for expr in array_literal.items.iter().rev() {
            self.encode_expr(expr, NeedsValue(true))?;
        }

        self.instructions
            .push(st::Inst::Array { count }, array_literal.span());
        Ok(())
    }

    fn encode_object_literal(
        &mut self,
        object_literal: &ast::ObjectLiteral,
        needs_value: NeedsValue,
    ) -> Result<()> {
        if !needs_value.0 {
            // Don't encode unecessary literals.
            return Ok(());
        }

        let count = object_literal.items.len();

        for (key, _, value) in object_literal.items.iter().rev() {
            self.encode_expr(value, NeedsValue(true))?;
            self.encode_string_literal(key, NeedsValue(true))?;
        }

        self.instructions
            .push(st::Inst::Object { count }, object_literal.span());
        Ok(())
    }

    /// Encode a char literal, like `'a'`.
    fn encode_char_literal(&mut self, c: &ast::CharLiteral, needs_value: NeedsValue) -> Result<()> {
        // NB: Elide the entire literal if it's not needed.
        if !needs_value.0 {
            return Ok(());
        }

        let resolved_char = c.resolve(self.source)?;
        self.instructions
            .push(st::Inst::Char { c: resolved_char }, c.token.span);
        Ok(())
    }

    /// Encode a string literal, like `"foo bar"`.
    fn encode_string_literal(
        &mut self,
        string: &ast::StringLiteral,
        needs_value: NeedsValue,
    ) -> Result<()> {
        // NB: Elide the entire literal if it's not needed.
        if !needs_value.0 {
            return Ok(());
        }

        let span = string.span();
        let string = string.resolve(self.source)?;
        let slot = self.unit.static_string(&*string)?;
        self.instructions.push(st::Inst::String { slot }, span);
        Ok(())
    }

    fn encode_unit_literal(&mut self, literal: &ast::UnitLiteral) -> Result<()> {
        self.instructions.push(st::Inst::Unit, literal.span());
        Ok(())
    }

    fn encode_bool_literal(&mut self, b: &ast::BoolLiteral) -> Result<()> {
        self.instructions
            .push(st::Inst::Bool { value: b.value }, b.span());
        Ok(())
    }

    fn encode_number_literal(
        &mut self,
        number: &ast::NumberLiteral,
        needs_value: NeedsValue,
    ) -> Result<()> {
        if !needs_value.0 {
            // NB: don't encode unecessary literal.
            return Ok(());
        }

        let span = number.span();
        let number = number.resolve(self.source)?;

        match number {
            ast::Number::Float(number) => {
                self.instructions.push(st::Inst::Float { number }, span);
            }
            ast::Number::Integer(number) => {
                self.instructions.push(st::Inst::Integer { number }, span);
            }
        }

        Ok(())
    }

    fn encode_while(&mut self, while_: &ast::While, needs_value: NeedsValue) -> Result<()> {
        log::trace!("{:?}", while_);

        let span = while_.span();

        let start_label = self.instructions.new_label("while_test");
        let end_label = self.instructions.new_label("while_end");

        let loop_count = self.loops.len();

        self.loops.push(Loop {
            end_label,
            var_count: self.locals.var_count,
        });

        self.instructions.label(start_label)?;
        self.encode_expr(&*while_.condition, NeedsValue(true))?;
        self.instructions.jump_if_not(end_label, span);
        self.encode_block(&*while_.body, NeedsValue(false))?;

        self.instructions.jump(start_label, span);
        self.instructions.label(end_label)?;

        // NB: If a value is needed from a while loop, encode it as a unit.
        if needs_value.0 {
            self.instructions.push(st::Inst::Unit, span);
        }

        if self.loops.pop().is_none() {
            return Err(CompileError::internal("missing parent loop", span));
        }

        if loop_count != self.loops.len() {
            return Err(CompileError::internal(
                "loop count mismatch on return",
                span,
            ));
        }

        Ok(())
    }

    fn encode_let(&mut self, let_: &ast::Let, needs_value: NeedsValue) -> Result<()> {
        log::trace!("{:?}", let_);

        let span = let_.span();

        let name = let_.name.resolve(self.source)?;

        self.references_at.clear();
        self.encode_expr(&*let_.expr, NeedsValue(true))?;

        if let Err(offset) = self
            .locals
            .decl_var(name, let_.name.token, self.references_at.clone())
        {
            // We are overloading an existing variable, so just replace it.
            self.instructions.push(st::Inst::Replace { offset }, span);
        }

        // If a value is needed for a let expression, it is evaluated as a unit.
        if needs_value.0 {
            self.instructions.push(st::Inst::Unit, span);
        }

        Ok(())
    }

    /// Push reference on the stack for replacement.
    fn encode_assign_target(&mut self, expr: &ast::Expr, first_level: bool) -> Result<()> {
        match expr {
            ast::Expr::Ident(ident) => {
                self.encode_ident(ident, NeedsValue(true))?;
                return Ok(());
            }
            ast::Expr::ExprUnary(unary) => match unary.op {
                ast::UnaryOp::Deref { token } => {
                    self.encode_assign_target(&*unary.expr, false)?;

                    if !first_level {
                        self.instructions.push(st::Inst::Deref, token.span);
                    }

                    return Ok(());
                }
                _ => (),
            },
            _ => (),
        }

        Err(CompileError::UnsupportedAssignExpr { span: expr.span() })
    }

    fn encode_assign(
        &mut self,
        lhs: &ast::Expr,
        rhs: &ast::Expr,
        needs_value: NeedsValue,
    ) -> Result<()> {
        log::trace!("{:?} = {:?}", lhs, rhs);

        let span = lhs.span().join(rhs.span());

        match lhs {
            ast::Expr::Ident(ident) => {
                let name = ident.resolve(self.source)?;

                self.references_at.clear();
                self.encode_expr(rhs, NeedsValue(true))?;

                let local =
                    self.locals
                        .get_mut(name)
                        .ok_or_else(|| CompileError::MissingLocal {
                            name: name.to_owned(),
                            span,
                        })?;

                local
                    .references_at
                    .extend(self.references_at.iter().copied());

                self.instructions.push(
                    st::Inst::Replace {
                        offset: local.offset,
                    },
                    span,
                );
            }
            lhs => {
                self.encode_expr(rhs, NeedsValue(true))?;
                self.encode_assign_target(lhs, true)?;
                self.instructions.push(st::Inst::ReplaceDeref, span);
            }
        }

        if needs_value.0 {
            self.instructions.push(st::Inst::Unit, span);
        }

        Ok(())
    }

    fn encode_index_get(
        &mut self,
        index_get: &ast::IndexGet,
        needs_value: NeedsValue,
    ) -> Result<()> {
        log::trace!("{:?}", index_get);
        let span = index_get.span();

        self.encode_expr(&*index_get.index, NeedsValue(true))?;
        self.encode_expr(&*index_get.target, NeedsValue(true))?;
        self.instructions.push(st::Inst::IndexGet, span);

        // NB: we still need to perform the operation since it might have side
        // effects, but pop the result in case a value is not needed.
        if !needs_value.0 {
            self.instructions.push(st::Inst::Pop, span);
        }

        Ok(())
    }

    /// Encode a `break` expression.
    fn encode_break(&mut self, b: &ast::Break, needs_value: NeedsValue) -> Result<()> {
        let span = b.span();

        if needs_value.0 {
            return Err(CompileError::BreakDoesNotProduceValue { span });
        }

        let last_loop = match self.loops.last().copied() {
            Some(last_loop) => last_loop,
            None => {
                return Err(CompileError::BreakOutsideOfLoop { span });
            }
        };

        let vars = self
            .locals
            .var_count
            .checked_sub(last_loop.var_count)
            .ok_or_else(|| CompileError::internal("var count should be larger", span))?;

        self.pop_locals(vars, span);
        self.instructions.jump(last_loop.end_label, span);
        // NB: loops are expected to produce a value at the end of their expression.
        Ok(())
    }

    fn encode_index_set(
        &mut self,
        index_set: &ast::IndexSet,
        needs_value: NeedsValue,
    ) -> Result<()> {
        log::trace!("{:?}", index_set);
        let span = index_set.span();

        self.encode_expr(&*index_set.value, NeedsValue(true))?;
        self.encode_expr(&*index_set.index, NeedsValue(true))?;
        self.encode_expr(&*index_set.target, NeedsValue(true))?;
        self.instructions.push(st::Inst::IndexSet, span);

        // Encode a unit in case a value is needed.
        if needs_value.0 {
            self.instructions.push(st::Inst::Unit, span);
        }

        Ok(())
    }

    /// Encode a local copy.
    fn encode_ident(&mut self, ident: &ast::Ident, needs_value: NeedsValue) -> Result<()> {
        log::trace!("encode local: {:?}", ident);

        // NB: avoid the encode completely if it is not needed.
        if !needs_value.0 {
            return Ok(());
        }

        let target = ident.resolve(self.source)?;

        let local = match self.locals.get(target) {
            Some(offset) => offset,
            None => {
                // Something imported is automatically a type.
                if let Some(path) = self.unit.lookup_import_by_name(target) {
                    let hash = st::Hash::of_type(path);
                    self.instructions
                        .push(st::Inst::Type { hash }, ident.span());
                    return Ok(());
                }

                return Err(CompileError::MissingLocal {
                    name: target.to_owned(),
                    span: ident.token.span,
                });
            }
        };

        self.references_at
            .extend(local.references_at.iter().copied());
        self.instructions.push(
            st::Inst::Copy {
                offset: local.offset,
            },
            ident.span(),
        );
        Ok(())
    }

    /// Decode a path into a call destination based on its hashes.
    fn decode_call_dest(&self, path: &ast::Path) -> Result<st::Hash> {
        let local = path.first.resolve(self.source)?;

        let imported = match self.unit.lookup_import_by_name(local).cloned() {
            Some(path) => path,
            None => st::Item::of(&[local]),
        };

        let mut rest = Vec::new();

        for (_, part) in &path.rest {
            rest.push(part.resolve(self.source)?);
        }

        let it = imported
            .into_iter()
            .map(String::as_str)
            .chain(rest.into_iter());

        Ok(st::Hash::function(it))
    }

    /// Encode the given type.
    fn encode_type(&mut self, path: &ast::Path, needs_value: NeedsValue) -> Result<()> {
        log::trace!("{:?}", path);

        // NB: do nothing if we don't need a value.
        if !needs_value.0 {
            return Ok(());
        }

        let mut parts = Vec::new();
        parts.push(path.first.resolve(self.source)?);

        for (_, part) in &path.rest {
            parts.push(part.resolve(self.source)?);
        }

        let hash = st::Hash::of_type(&parts);
        self.instructions.push(st::Inst::Type { hash }, path.span());
        Ok(())
    }

    fn encode_call_fn(&mut self, call_fn: &ast::CallFn, needs_value: NeedsValue) -> Result<()> {
        log::trace!("{:?}", call_fn);

        let span = call_fn.span();
        let args = call_fn.args.items.len();

        for expr in call_fn.args.items.iter().rev() {
            self.encode_expr(expr, NeedsValue(true))?;
        }

        let hash = self.decode_call_dest(&call_fn.name)?;
        self.instructions.push(st::Inst::Call { hash, args }, span);

        // NB: we put it here to preserve the call in case it has side effects.
        // But if we don't need the value, then pop it from the stack.
        if !needs_value.0 {
            self.instructions.push(st::Inst::Pop, span);
        }

        Ok(())
    }

    fn encode_call_instance_fn(
        &mut self,
        call_instance_fn: &ast::CallInstanceFn,
        needs_value: NeedsValue,
    ) -> Result<()> {
        log::trace!("{:?}", call_instance_fn);

        let span = call_instance_fn.span();
        let args = call_instance_fn.args.items.len();

        for expr in call_instance_fn.args.items.iter().rev() {
            self.encode_expr(expr, NeedsValue(true))?;
        }

        self.encode_expr(&*call_instance_fn.instance, NeedsValue(true))?;

        let name = call_instance_fn.name.resolve(self.source)?;
        let hash = st::Hash::of(name);
        self.instructions
            .push(st::Inst::CallInstance { hash, args }, span);

        // NB: we put it here to preserve the call in case it has side effects.
        // But if we don't need the value, then pop it from the stack.
        if !needs_value.0 {
            self.instructions.push(st::Inst::Pop, span);
        }

        Ok(())
    }

    fn encode_expr_unary(
        &mut self,
        expr_unary: &ast::ExprUnary,
        needs_value: NeedsValue,
    ) -> Result<()> {
        log::trace!("{:?}", expr_unary);
        let span = expr_unary.span();

        // NB: special unary expressions.
        match expr_unary.op {
            ast::UnaryOp::Ref { .. } => {
                self.encode_ref(&*expr_unary.expr, expr_unary.span())?;
                return Ok(());
            }
            _ => (),
        }

        self.encode_expr(&*expr_unary.expr, NeedsValue(true))?;

        match expr_unary.op {
            ast::UnaryOp::Not { .. } => {
                self.instructions.push(st::Inst::Not, span);
            }
            ast::UnaryOp::Deref { .. } => {
                self.instructions.push(st::Inst::Deref, span);
            }
            op => {
                return Err(CompileError::UnsupportedUnaryOp { span, op });
            }
        }

        // NB: we put it here to preserve the call in case it has side effects.
        // But if we don't need the value, then pop it from the stack.
        if !needs_value.0 {
            self.instructions.push(st::Inst::Pop, span);
        }

        Ok(())
    }

    /// Encode a ref `&<expr>` value.
    fn encode_ref(&mut self, expr: &ast::Expr, span: Span) -> Result<()> {
        match expr {
            ast::Expr::Ident(ident) => {
                let target = ident.resolve(self.source)?;

                let local = match self.locals.get(target) {
                    Some(offset) => offset,
                    None => {
                        return Err(CompileError::MissingLocal {
                            name: target.to_owned(),
                            span: ident.token.span,
                        });
                    }
                };

                self.references_at.push(span);
                self.instructions.push(
                    st::Inst::Ptr {
                        offset: local.offset,
                    },
                    span,
                );
            }
            _ => {
                return Err(CompileError::UnsupportedRef { span });
            }
        }

        Ok(())
    }

    fn encode_expr_binary(
        &mut self,
        expr_binary: &ast::ExprBinary,
        needs_value: NeedsValue,
    ) -> Result<()> {
        log::trace!("{:?}", expr_binary);

        // Special expressions which operates on the stack in special ways.
        match expr_binary.op {
            ast::BinOp::Assign { .. } => {
                self.encode_assign(&*expr_binary.lhs, &*expr_binary.rhs, needs_value)?;
                return Ok(());
            }
            _ => (),
        }

        let span = expr_binary.span();

        self.encode_expr(&*expr_binary.lhs, NeedsValue(true))?;
        self.encode_expr(&*expr_binary.rhs, NeedsValue(true))?;

        match expr_binary.op {
            ast::BinOp::Add { .. } => {
                self.instructions.push(st::Inst::Add, span);
            }
            ast::BinOp::Sub { .. } => {
                self.instructions.push(st::Inst::Sub, span);
            }
            ast::BinOp::Div { .. } => {
                self.instructions.push(st::Inst::Div, span);
            }
            ast::BinOp::Mul { .. } => {
                self.instructions.push(st::Inst::Mul, span);
            }
            ast::BinOp::Eq { .. } => {
                self.instructions.push(st::Inst::Eq, span);
            }
            ast::BinOp::Neq { .. } => {
                self.instructions.push(st::Inst::Neq, span);
            }
            ast::BinOp::Lt { .. } => {
                self.instructions.push(st::Inst::Lt, span);
            }
            ast::BinOp::Gt { .. } => {
                self.instructions.push(st::Inst::Gt, span);
            }
            ast::BinOp::Lte { .. } => {
                self.instructions.push(st::Inst::Lte, span);
            }
            ast::BinOp::Gte { .. } => {
                self.instructions.push(st::Inst::Gte, span);
            }
            ast::BinOp::Is { .. } => {
                self.instructions.push(st::Inst::Is, span);
            }
            op => {
                return Err(CompileError::UnsupportedBinaryOp { span, op });
            }
        }

        // NB: we put it here to preserve the call in case it has side effects.
        // But if we don't need the value, then pop it from the stack.
        if !needs_value.0 {
            self.instructions.push(st::Inst::Pop, span);
        }

        Ok(())
    }

    fn encode_expr_if(&mut self, expr_if: &ast::ExprIf, needs_value: NeedsValue) -> Result<()> {
        let span = expr_if.span();

        log::trace!("{:?}", expr_if);

        let then_label = self.instructions.new_label("if_then");
        let end_label = self.instructions.new_label("if_end");

        let mut branch_labels = Vec::new();

        self.encode_expr(&*expr_if.condition, NeedsValue(true))?;
        self.instructions.jump_if(then_label, span);

        for branch in &expr_if.expr_else_ifs {
            let label = self.instructions.new_label("if_branch");
            branch_labels.push(label);

            self.encode_expr(&*branch.condition, needs_value)?;
            self.instructions.jump_if(label, branch.span());
        }

        // use fallback as fall through.
        if let Some(fallback) = &expr_if.expr_else {
            self.encode_block(&*fallback.block, needs_value)?;
        } else {
            // NB: if we must produce a value and there is no fallback branch,
            // encode the result of the statement as a unit.
            if needs_value.0 {
                self.instructions.push(st::Inst::Unit, span);
            }
        }

        self.instructions.jump(end_label, span);

        self.instructions.label(then_label)?;
        self.encode_block(&*expr_if.block, needs_value)?;

        if !expr_if.expr_else_ifs.is_empty() {
            self.instructions.jump(end_label, span);
        }

        let mut it = expr_if
            .expr_else_ifs
            .iter()
            .zip(branch_labels.iter().copied())
            .peekable();

        if let Some((branch, label)) = it.next() {
            let span = branch.span();
            self.instructions.label(label)?;
            self.encode_block(&*branch.block, needs_value)?;

            if it.peek().is_some() {
                self.instructions.jump(end_label, span);
            }
        }

        self.instructions.label(end_label)?;
        Ok(())
    }
}

/// Decode the specified path.
fn resolve_path<'a>(path: ast::Path, source: Source<'a>) -> Result<Vec<&'a str>> {
    let mut output = Vec::new();

    output.push(path.first.resolve(source)?);

    for (_, ident) in path.rest {
        output.push(ident.resolve(source)?);
    }

    Ok(output)
}

/// A locally declared variable.
#[derive(Debug, Clone)]
struct Local {
    /// Slot offset from the current stack frame.
    offset: usize,
    /// Name of the variable.
    name: String,
    /// Token assocaited with the variable.
    token: Token,
    /// Local references used by local expression.
    references_at: Vec<Span>,
}

#[derive(Debug, Clone)]
struct Locals {
    locals: HashMap<String, Local>,
    var_count: usize,
}

impl Locals {
    /// Construct a new locals handlers.
    pub fn new() -> Locals {
        Self {
            locals: HashMap::new(),
            var_count: 0,
        }
    }

    /// Insert a new local, and return the old one if there's a conflict.
    pub fn new_local(&mut self, name: &str, token: Token, references_at: Vec<Span>) -> Result<()> {
        let local = Local {
            offset: self.var_count,
            name: name.to_owned(),
            token,
            references_at,
        };

        self.var_count += 1;

        if let Some(old) = self.locals.insert(name.to_owned(), local) {
            return Err(CompileError::VariableConflict {
                name: name.to_owned(),
                span: token.span,
                existing_span: old.token.span,
            });
        }

        Ok(())
    }

    /// Insert a new local, and return the old one if there's a conflict.
    pub fn decl_var(
        &mut self,
        name: &str,
        token: Token,
        references_at: Vec<Span>,
    ) -> Result<(), usize> {
        if let Some(old) = self.locals.get(name) {
            return Err(old.offset);
        }

        self.locals.insert(
            name.to_owned(),
            Local {
                offset: self.var_count,
                name: name.to_owned(),
                token,
                references_at,
            },
        );

        self.var_count += 1;
        Ok(())
    }

    /// Access the local with the given name.
    pub fn get(&self, name: &str) -> Option<&Local> {
        if let Some(local) = self.locals.get(name) {
            return Some(local);
        }

        None
    }

    /// Access the local with the given name.
    pub fn get_mut(&mut self, name: &str) -> Option<&mut Local> {
        if let Some(local) = self.locals.get_mut(name) {
            return Some(local);
        }

        None
    }
}

/// Loops we are inside.
#[derive(Clone, Copy)]
struct Loop {
    /// The end label of the loop.
    end_label: st::unit::Label,
    /// The number of variables observed at the start of the loop.
    var_count: usize,
}