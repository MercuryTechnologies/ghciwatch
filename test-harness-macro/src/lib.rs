use proc_macro::TokenStream;

use quote::quote;
use quote::ToTokens;
use syn::parse;
use syn::parse::Parse;
use syn::parse::ParseStream;
use syn::Attribute;
use syn::Block;
use syn::Ident;
use syn::ItemFn;

/// Runs a test asynchronously in the `tokio` current-thread runtime with `tracing` enabled.
///
/// One test is generated for each GHC version listed in the `$GHC_VERSIONS` environment variable
/// at compile-time.
///
/// This is designed to be used with [`test_harness::GhcidNg`].
#[proc_macro_attribute]
pub fn test(_attr: TokenStream, item: TokenStream) -> TokenStream {
    // Parse annotated function
    let mut function: ItemFn = parse(item).expect("Could not parse item as function");

    // Add attributes to run the test in the `tokio` current-thread runtime and enable tracing.
    function.attrs.extend(
        parse::<Attributes>(
            quote! {
                #[tokio::test]
                #[tracing_test::traced_test]
                #[allow(non_snake_case)]
            }
            .into(),
        )
        .expect("Could not parse quoted attributes #[tokio::test] and #[tracing_test::traced_test]")
        .0,
    );

    let ghc_versions = match option_env!("GHC_VERSIONS") {
        None => {
            panic!("`$GHC_VERSIONS` should be set to a list of GHC versions to run tests under, separated by spaces, like `9.0.2 9.2.8 9.4.6 9.6.2`.");
        }
        Some(versions) => versions.split_ascii_whitespace().collect::<Vec<_>>(),
    };

    // Generate functions for each GHC version we want to test.
    let mut ret = TokenStream::new();
    for ghc_version in ghc_versions {
        ret.extend::<TokenStream>(
            make_test_fn(function.clone(), ghc_version)
                .to_token_stream()
                .into(),
        );
    }
    ret
}

struct Attributes(Vec<Attribute>);

impl Parse for Attributes {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(Self(input.call(Attribute::parse_outer)?))
    }
}

fn make_test_fn(mut function: ItemFn, ghc_version: &str) -> ItemFn {
    let ghc_version_ident = ghc_version.replace('.', "");
    let stmts = function.block.stmts;
    let test_name_base = function.sig.ident.to_string();
    let test_name = format!("{test_name_base}_{ghc_version_ident}");
    function.sig.ident = Ident::new(&test_name, function.sig.ident.span());

    // Wrap the test code in startup/cleanup code.
    //
    // Before the user test code, we set the thread-local storage to test-local data so that when
    // we construct a `test_harness::GhcidNg` it can use the correct GHC version.
    //
    // Then we run the user test code. If it errors, we save the logs to `CARGO_TARGET_TMPDIR`.
    //
    // Finally, we clean up the temporary directory `GhcidNg` created.
    let new_body = parse::<Block>(
        quote! {
            {
                ::test_harness::internal::GHC_VERSION.with(|tmpdir| {
                    *tmpdir.borrow_mut() = #ghc_version.to_owned();
                });

                match ::tokio::task::spawn(async {
                    #(#stmts);*
                }).await {
                    Err(err) => {
                        // Copy out temp files
                        ::test_harness::internal::save_test_logs(
                            format!("{}::{}", module_path!(), #test_name),
                            ::std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
                        );
                        ::test_harness::internal::cleanup().await;

                        if err.is_panic() {
                            ::std::panic::resume_unwind(err.into_panic());
                        } else {
                            panic!("Test cancelled? {err:?}");
                        }
                    }
                    Ok(()) => {
                        ::test_harness::internal::cleanup().await;
                    }
                };
            }
        }
        .into(),
    )
    .expect("Could not parse function body");

    // Replace function body
    *function.block = new_body;

    function
}
