use crate::compiling::assemble::prelude::*;

/// Compile a literal byte such as `b'a'`.
impl Assemble for ast::LitByte {
    fn assemble(&self, c: &mut Compiler<'_>, needs: Needs) -> CompileResult<()> {
        let span = self.span();
        log::trace!("LitByte => {:?}", c.source.source(span));

        // NB: Elide the entire literal if it's not needed.
        if !needs.value() {
            c.warnings.not_used(c.source_id, span, c.context());
            return Ok(());
        }

        let b = self.resolve(&c.storage, &*c.source)?;
        c.asm.push(Inst::byte(b), span);
        Ok(())
    }
}
