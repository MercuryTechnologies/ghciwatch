use expect_test::expect_file;

#[test]
fn test_example_complex_app() {
    mod complex_app {
        include!("../examples/complex_app.rs");
    }

    let expected = expect_file!["../examples/complex_app.md"];

    expected.assert_eq(&clap_markdown::help_markdown::<complex_app::Opts>());
}
