// Regression tests for the syn::Error backport; upstream MIT license applies.
use proc_macro2::TokenStream;
use quote::quote;

fn assert_error(result: syn::Result<TokenStream>, expected: &str) {
    let error = result.expect_err("invalid macro input must be rejected");
    assert!(error.to_string().contains(expected), "{error}");
    assert!(error
        .into_compile_error()
        .to_string()
        .contains("compile_error"));
}

#[test]
fn derives_report_invalid_input_without_panicking() {
    let structure = syn::parse_quote!(
        struct Invalid;
    );
    assert_error(
        crate::boxed_derive::impl_boxed(&structure),
        "requires #[boxed_type",
    );
    assert_error(
        crate::enum_derive::impl_enum(&structure),
        "only supports enums",
    );
    assert_error(
        crate::error_domain_derive::impl_error_domain(&structure),
        "only supports enums",
    );
    assert_error(
        crate::shared_boxed_derive::impl_shared_boxed(&structure),
        "requires struct",
    );
    let enumeration = syn::parse_quote!(
        enum Invalid {
            Value,
        }
    );
    assert_error(
        crate::enum_derive::impl_enum(&enumeration),
        "requires #[enum_type",
    );
    assert_error(
        crate::error_domain_derive::impl_error_domain(&enumeration),
        "requires #[error_domain",
    );
    let shared = syn::parse_quote!(
        struct Shared(std::sync::Arc<String>);
    );
    assert_error(
        crate::shared_boxed_derive::impl_shared_boxed(&shared),
        "requires #[shared_boxed_type",
    );
}

#[test]
fn attribute_macros_reject_inherent_impls_and_non_enum_flags() {
    let inherent = syn::parse_quote!(impl Invalid {});
    assert_error(
        crate::object_subclass_attribute::impl_object_subclass(&inherent),
        "ObjectSubclass",
    );
    assert_error(
        crate::object_interface_attribute::impl_object_interface(&inherent),
        "ObjectInterface",
    );
    assert_error(
        crate::derived_properties_attribute::impl_derived_properties(&inherent),
        "ObjectImpl",
    );
    let flags = crate::flags_attribute::impl_flags(
        crate::flags_attribute::AttrInput {
            enum_name: syn::parse_quote!("Invalid"),
        },
        &syn::parse_quote!(
            struct Invalid;
        ),
    );
    assert!(flags.to_string().contains("compile_error"));
    assert!(flags.to_string().contains("only supports enums"));
}

#[test]
fn variant_errors_are_diagnostics() {
    assert_error(
        crate::variant_derive::impl_variant(syn::parse_quote!(union Invalid { x: u32 })),
        "unions",
    );
    assert_error(
        crate::variant_derive::impl_variant(syn::parse_quote!(
            #[variant_enum(unknown)]
            enum Invalid {
                Value,
            }
        )),
        "unknown type",
    );
    assert_error(
        crate::variant_derive::impl_variant(syn::parse_quote!(
            #[variant_enum(repr)]
            enum Invalid {
                Value,
            }
        )),
        "Must have #[repr]",
    );
    assert_error(
        crate::variant_derive::impl_variant(syn::parse_quote!(
            #[variant_enum(enum)]
            enum Invalid {
                Unit,
                Value(String),
            }
        )),
        "only allowed with C-style enums",
    );
}

#[test]
fn valid_derives_still_generate_rust_items() {
    let outputs = [
        crate::boxed_derive::impl_boxed(&syn::parse_quote!(
            #[boxed_type(name = "Boxed")]
            struct Boxed(String);
        )),
        crate::enum_derive::impl_enum(&syn::parse_quote!(
            #[enum_type(name = "Enum")]
            enum Enum {
                Value,
            }
        )),
        crate::error_domain_derive::impl_error_domain(&syn::parse_quote!(
            #[error_domain(name = "error")]
            enum Error {
                Value,
            }
        )),
        crate::shared_boxed_derive::impl_shared_boxed(&syn::parse_quote!(
            #[shared_boxed_type(name = "Shared")]
            struct Shared(std::sync::Arc<String>);
        )),
        crate::variant_derive::impl_variant(syn::parse_quote!(
            struct Variant {
                value: String,
            }
        )),
        crate::object_subclass_attribute::impl_object_subclass(
            &syn::parse_quote!(impl ObjectSubclass for Object {}),
        ),
        crate::object_interface_attribute::impl_object_interface(&syn::parse_quote!(
            unsafe impl ObjectInterface for Interface {}
        )),
        crate::derived_properties_attribute::impl_derived_properties(
            &syn::parse_quote!(impl ObjectImpl for Object {}),
        ),
    ];
    for output in outputs {
        let output = output.unwrap();
        assert!(!output.is_empty());
        assert!(!output.to_string().contains("compile_error"));
        syn::parse2::<syn::File>(output).unwrap();
    }
    let flags = crate::flags_attribute::impl_flags(
        crate::flags_attribute::AttrInput {
            enum_name: syn::parse_quote!("Flags"),
        },
        &syn::parse2(quote!(
            enum Flags {
                A = 1,
                B = 2,
            }
        ))
        .unwrap(),
    );
    syn::parse2::<syn::File>(flags).unwrap();
}
