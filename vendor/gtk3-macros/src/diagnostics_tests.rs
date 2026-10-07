// Regression tests for the upstream diagnostics backport; MIT license applies.

#[test]
fn missing_or_invalid_templates_emit_compile_errors() {
    for (input, expected) in [
        (
            syn::parse_quote!(
                struct Widget;
            ),
            "requires #[template",
        ),
        (
            syn::parse_quote!(
                #[template(invalid = "value")]
                struct Widget;
            ),
            "requires #[template",
        ),
        (
            syn::parse_quote!(
                #[template(string = "<interface/>")]
                enum Widget {
                    Value,
                }
            ),
            "only supports structs",
        ),
        (
            syn::parse_quote!(
                #[template(string = "<interface/>")]
                struct Widget {
                    #[template_child(invalid)]
                    child: Child,
                }
            ),
            "compile_error",
        ),
    ] {
        let output = crate::composite_template_derive::impl_composite_template(&input).to_string();
        assert!(output.contains("compile_error"), "{output}");
        assert!(output.contains(expected), "{output}");
    }
}

#[test]
fn valid_template_still_generates_rust_items() {
    let input = syn::parse_quote!(
        #[template(string = "<interface/>")]
        struct Widget {
            #[template_child(id = "label")]
            child: Child,
        }
    );
    let output = crate::composite_template_derive::impl_composite_template(&input);
    assert!(!output.to_string().contains("compile_error"));
    syn::parse2::<syn::File>(output).unwrap();
}
