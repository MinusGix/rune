use crate::compiling::assemble::prelude::*;

/// Compile a loop.
impl Assemble for ast::ExprLoop {
    fn assemble(&self, c: &mut Compiler<'_>, needs: Needs) -> CompileResult<()> {
        let span = self.span();
        log::trace!("ExprLoop => {:?}", c.source.source(span));

        let start_label = c.asm.new_label("loop_start");
        let end_label = c.asm.new_label("loop_end");

        let _guard = c.loops.push(Loop {
            label: self.label.map(|(label, _)| label),
            break_label: end_label,
            total_var_count: c.scopes.total_var_count(span)?,
            needs,
            drop: None,
        });

        c.asm.label(start_label)?;
        self.body.assemble(c, Needs::None)?;
        c.asm.jump(start_label, span);
        c.asm.label(end_label)?;

        Ok(())
    }
}
