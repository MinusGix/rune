use crate::ast;
use crate::{ParseError, Parser, Spanned, ToTokens};

/// A `while` loop: `while [expr] { ... }`.
///
/// # Examples
///
/// ```rust
/// use rune::{testing, ast};
///
/// testing::roundtrip::<ast::ExprWhile>("while x {}");
/// testing::roundtrip::<ast::ExprWhile>("'label: while x {}");
/// testing::roundtrip::<ast::ExprWhile>("#[attr] 'label: while x {}");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, ToTokens, Spanned)]
pub struct ExprWhile {
    /// The attributes for the `while` loop
    #[rune(iter)]
    pub attributes: Vec<ast::Attribute>,
    /// A label for the while loop.
    #[rune(iter)]
    pub label: Option<(ast::Label, ast::Colon)>,
    /// The `while` keyword.
    pub while_token: ast::While,
    /// The name of the binding.
    pub condition: ast::Condition,
    /// The body of the while loop.
    pub body: Box<ast::Block>,
}

impl ExprWhile {
    /// Parse the `while` with the given attributes and label.
    pub(crate) fn parse_with_meta(
        parser: &mut Parser<'_>,
        attributes: Vec<ast::Attribute>,
        label: Option<(ast::Label, ast::Colon)>,
    ) -> Result<Self, ParseError> {
        Ok(ExprWhile {
            attributes,
            label,
            while_token: parser.parse()?,
            condition: parser.parse()?,
            body: parser.parse()?,
        })
    }
}

expr_parse!(ExprWhile, "while expression");
