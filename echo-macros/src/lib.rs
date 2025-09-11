use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{format_ident, quote};
use syn::{
    Attribute, Fields, Ident, ItemStruct, LitInt, LitStr, Meta, Result as SynResult, Token,
    parse::{Parse, ParseStream},
    parse_macro_input,
};

struct EchoExtArgs {
    id: u32,
}

impl Parse for EchoExtArgs {
    fn parse(input: ParseStream) -> SynResult<Self> {
        let key: Ident = input.parse()?;
        if key != "id" {
            return Err(syn::Error::new(key.span(), "expected `id`"));
        }
        input.parse::<Token![=]>()?;
        let lit: LitInt = input.parse()?;
        let _ = input.parse::<Token![,]>();
        Ok(Self {
            id: lit.base10_parse()?,
        })
    }
}

fn parse_echo_ext_args(attrs: &[Attribute]) -> SynResult<EchoExtArgs> {
    let mut out: Option<EchoExtArgs> = None;
    attrs
        .iter()
        .find(|attr| attr.path().is_ident("echo_ext"))
        .map(|attr| {
            let parsed: EchoExtArgs = attr.parse_args()?;
            if out.replace(parsed).is_some() {
                return Err(syn::Error::new_spanned(
                    attr,
                    "duplicate `#[echo_ext(...)]` attribute",
                ));
            }
            Ok(())
        })
        .transpose()?;
    out.ok_or_else(|| syn::Error::new(Span::call_site(), "missing `#[echo_ext(id = ...)]`"))
}

#[proc_macro_derive(EchoExt, attributes(echo_ext, eval, skip))]
pub fn echo_ext(input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as ItemStruct);
    let EchoExtArgs { id } = match parse_echo_ext_args(&ast.attrs) {
        Ok(v) => v,
        Err(e) => return e.to_compile_error().into(),
    };

    let (name, generics) = (&ast.ident, &ast.generics);
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let fields = match &ast.fields {
        Fields::Named(named) => &named.named,
        _ => {
            return syn::Error::new_spanned(
                &ast.fields,
                "#[derive(EchoExt)] only supports structs with named fields",
            )
            .to_compile_error()
            .into();
        }
    };

    let field_count = fields.len();
    let (mut meta_keys, mut eval_keys) = (
        Vec::with_capacity(field_count),
        Vec::with_capacity(field_count),
    );

    fields.iter().for_each(|f| {
        let ident = f.ident.as_ref().unwrap();
        let field_name = ident.to_string();
        let has_skip = f
            .attrs
            .iter()
            .any(|a| matches!(&a.meta, Meta::Path(p) if p.is_ident("skip")));
        let has_eval = f
            .attrs
            .iter()
            .any(|a| matches!(&a.meta, Meta::Path(p) if p.is_ident("eval")));
        match (has_skip, has_eval) {
            (false, true) => eval_keys.push(field_name),
            (false, false) => meta_keys.push(field_name),
            _ => {}
        }
    });

    let meta_tokens = match meta_keys.is_empty() {
        true => quote! { None },
        false => {
            let meta_key_str = meta_keys.iter().map(|k| k.as_str());
            quote! {
                Some(::phf::phf_set! {
                    #(#meta_key_str),*
                })
            }
        }
    };

    let eval_tokens = match eval_keys.is_empty() {
        true => quote! { None },
        false => {
            let eval_key_str = eval_keys.iter().map(|k| k.as_str());
            quote! {
                Some(::phf::phf_set! {
                    #(#eval_key_str),*
                })
            }
        }
    };

    let guard_export = LitStr::new(&format!("__echo_ext_{}", id), Span::call_site());
    let guard_ident = format_ident!("__ECHO_EXT_ID_GUARD_{}_{}", name, id);

    let expanded = quote! {
        #[used]
        #[doc(hidden)]
        #[unsafe(no_mangle)]
        #[unsafe(link_section = ".echo_ext.ids")]
        #[unsafe(export_name = #guard_export)]
        static #guard_ident: [u8; 0] = [];

        #[automatically_derived]
        impl #impl_generics EchoExtMeta for #name #ty_generics #where_clause {
            const ID: u32 = #id;
            const META_KEY: Option<::phf::Set<&'static str>> = #meta_tokens;
            const EVALUATE_KEY: Option<::phf::Set<&'static str>> = #eval_tokens;
        }
    };

    expanded.into()
}
