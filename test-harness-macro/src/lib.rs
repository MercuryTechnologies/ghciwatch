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
        .expect("Could not parse quoted attributes")
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
    let new_body = parse::<Block>(
        quote! {
            {
                ::test_harness::internal::wrap_test(
                    async {
                        #(#stmts);*
                    },
                    #ghc_version,
                    #test_name,
                    env!("CARGO_TARGET_TMPDIR"),
                ).await;
            }
        }
        .into(),
    )
    .expect("Could not parse function body");

    // Replace function body
    *function.block = new_body;

    function
}
