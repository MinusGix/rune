use rune::testing::run;
use std::sync::Arc;

fn main() -> runestick::Result<()> {
    let context = Arc::new(rune_modules::default_context()?);

    let object: (i64, i64) = run(
        &context,
        &["calc"],
        ((1, 2),),
        r#"
        fn calc(input) {
            (input.0 + 1, input.1 + 2)
        }
        "#,
    )?;

    println!("{:?}", object);
    Ok(())
}
