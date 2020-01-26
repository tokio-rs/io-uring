#[test]
fn test_build() -> anyhow::Result<()> {
    let _ring = io_uring::Builder::default()
        .dontfork()
        .build(1)?;

    Ok(())
}
