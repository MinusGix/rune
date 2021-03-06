use rune::testing::*;

#[test]
fn test_bad_attributes() {
    assert_compile_error! {
        r#"pub fn main() { #[foo] #[bar] let x = 1; }"#,
        span, CompileErrorKind::Custom { message } => {
            assert_eq!(message, "attributes are not supported");
            assert_eq!(span, Span::new(16, 29));
        }
    };
}
