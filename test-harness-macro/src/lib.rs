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

#[proc_macro_attribute]
pub fn test(_attr: TokenStream, item: TokenStream) -> TokenStream {
    // Parse annotated function
    let mut function: ItemFn = parse(item).expect("Could not parse item as function");

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

    let new_body = parse::<Block>(
        quote! {
            {
                ::test_harness::internal::IN_CUSTOM_TEST_HARNESS.with(|value| {
                    value.store(true, ::std::sync::atomic::Ordering::SeqCst);
                });
                ::test_harness::internal::CARGO_TARGET_TMPDIR.with(|tmpdir| {
                    *tmpdir.borrow_mut() = Some(::std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")));
                });
                ::test_harness::internal::GHC_VERSION.with(|tmpdir| {
                    *tmpdir.borrow_mut() = #ghc_version.to_owned();
                });

                match ::tokio::task::spawn(async {
                    #(#stmts);*
                }).await {
                    Err(err) => {
                        // Copy out temp files
                        ::test_harness::internal::save_test_logs(
                            format!("{}::{}", module_path!(), #test_name)
                        );
                        ::test_harness::internal::cleanup_tempdir();

                        if err.is_panic() {
                            ::std::panic::resume_unwind(err.into_panic());
                        } else {
                            panic!("Test cancelled? {err:?}");
                        }
                    }
                    Ok(()) => {
                        ::test_harness::internal::cleanup_tempdir();
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
