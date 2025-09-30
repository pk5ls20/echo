use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{format_ident, quote};
use syn::{
    Attribute, Data, DeriveInput, Error as SynError, Fields, Generics, Ident, ItemStruct, LitBool,
    LitInt, LitStr, Result as SynResult, Token,
    parse::{Parse, ParseStream},
    parse_macro_input, parse_quote,
};

trait ArgKeys {
    const KEYS: &'static [&'static str];
}

macro_rules! define_args {
    (
        struct $name:ident { $( $field:ident : $ty:ty ),* $(,)? }
    ) => {
        struct $name { $( $field : $ty ),* }

        impl ArgKeys for $name {
            const KEYS: &'static [&'static str] = &[$( stringify!($field) ),*];
        }
    };
}

macro_rules! set_once {
    ($slot:expr, $key_ident:expr, $name:literal, $value:expr) => {{
        if $slot.is_some() {
            return Err(SynError::new(
                $key_ident.span(),
                concat!("duplicate `", $name, "`"),
            ));
        }
        $slot = Some($value);
    }};
}

macro_rules! set_required {
    ($slot:ident) => {{
        $slot.ok_or_else(|| {
            SynError::new(
                Span::call_site(),
                concat!("`", stringify!($slot), "` is required"),
            )
        })
    }};
}

macro_rules! bail {
    ($unknown_key:expr, $bind_struct:ty) => {{
        let other = $unknown_key.to_string();
        let expected = <$bind_struct as ArgKeys>::KEYS.join("`, `");
        return Err(SynError::new(
            $unknown_key.span(),
            format!("unknown key `{}` (expected {})", other, expected),
        ));
    }};
    ($unknown_key:expr, $msg:expr) => {{
        return Err(SynError::new($unknown_key.span(), $msg));
    }};
}

struct U32Pair(pub u32, pub u32);

impl Parse for U32Pair {
    fn parse(input: ParseStream) -> SynResult<Self> {
        let first: LitInt = input.parse()?;
        input.parse::<Token![,]>()?;
        let second: LitInt = input.parse()?;
        Ok(U32Pair(first.base10_parse()?, second.base10_parse()?))
    }
}

define_args! {
    struct EchoExtArgs {
        id: u32,
        desc: Option<LitStr>,
        side_effect: Option<bool>,
        fuzz_hw: U32Pair,
    }
}

impl Parse for EchoExtArgs {
    fn parse(input: ParseStream) -> SynResult<Self> {
        let mut id: Option<u32> = None;
        let mut desc: Option<LitStr> = None;
        let mut side_effect: Option<bool> = None;
        let mut fuzz_hw: Option<U32Pair> = None;
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            match &*key.to_string() {
                "id" => set_once!(id, key, "id", input.parse::<LitInt>()?.base10_parse()?),
                "desc" => set_once!(desc, key, "desc", input.parse::<LitStr>()?),
                "side_effect" => set_once!(
                    side_effect,
                    key,
                    "side_effect",
                    input.parse::<LitBool>()?.value
                ),
                "fuzz_hw" => set_once!(fuzz_hw, key, "fuzz_hw", input.parse::<U32Pair>()?),
                _ => bail!(key, EchoExtArgs),
            }
            input.peek(Token![,]).then(|| input.parse::<Token![,]>());
        }
        let id = set_required!(id)?;
        let side_effect = side_effect.or(Some(false));
        let fuzz_hw = fuzz_hw.unwrap_or(U32Pair(200, 300));
        Ok(Self {
            id,
            desc,
            side_effect,
            fuzz_hw,
        })
    }
}

fn parse_echo_ext_args(attrs: &[Attribute]) -> SynResult<EchoExtArgs> {
    let mut out: Option<EchoExtArgs> = None;
    attrs
        .iter()
        .find(|attr| attr.path().is_ident("echo_ext"))
        .map(|attr| {
            let parsed = attr.parse_args::<EchoExtArgs>()?;
            if out.replace(parsed).is_some() {
                return Err(SynError::new_spanned(
                    attr,
                    "duplicate `#[echo_ext(...)]` attribute",
                ));
            }
            Ok(())
        })
        .transpose()?;
    out.ok_or_else(|| SynError::new(Span::call_site(), "missing `#[echo_ext(id = ...)]`"))
}

define_args! {
    struct EchoFieldArgs {
        desc: Option<LitStr>,
        example: Option<LitStr>,
    }
}

impl Parse for EchoFieldArgs {
    fn parse(input: ParseStream) -> SynResult<Self> {
        let mut desc: Option<LitStr> = None;
        let mut example: Option<LitStr> = None;
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            match &*key.to_string() {
                "desc" => set_once!(desc, key, "desc", input.parse::<LitStr>()?),
                "example" => set_once!(example, key, "example", input.parse::<LitStr>()?),
                _ => bail!(key, EchoFieldArgs),
            }
            input.peek(Token![,]).then(|| input.parse::<Token![,]>());
        }
        Ok(Self { desc, example })
    }
}

#[proc_macro_derive(EchoExt, attributes(echo_ext, field, eval, skip))]
pub fn echo_ext(input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as ItemStruct);
    let EchoExtArgs {
        id,
        desc,
        side_effect,
        fuzz_hw: U32Pair(fuzz_h, fuzz_w),
    } = match parse_echo_ext_args(&ast.attrs) {
        Ok(v) => v,
        Err(e) => return e.to_compile_error().into(),
    };

    let desc_tokens = match desc {
        Some(lit) => quote!(Some(#lit)),
        None => quote!(None),
    };

    let (name, generics) = (&ast.ident, &ast.generics);
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let fields = match &ast.fields {
        Fields::Named(named) => &named.named,
        _ => {
            return SynError::new_spanned(
                &ast.fields,
                "#[derive(EchoExt)] only supports structs with named fields",
            )
            .to_compile_error()
            .into();
        }
    };

    let field_count = fields.len();
    let (mut meta, mut eval_keys) = (
        Vec::with_capacity(field_count),
        Vec::with_capacity(field_count),
    );

    'f: for field in fields {
        let name = field.ident.as_ref().unwrap().to_string().replace('_', "-");
        'a: for a in &field.attrs {
            match a.path() {
                p if p.is_ident("skip") => continue 'a,
                p if p.is_ident("eval") => {
                    eval_keys.push(name);
                    continue 'f;
                }
                _ => {
                    match a.parse_args::<EchoFieldArgs>() {
                        Ok(field_args) => {
                            let ty = &field.ty;
                            let ty_lit = LitStr::new(&quote!(#ty).to_string(), Span::call_site());
                            let key_lit = LitStr::new(&name, Span::call_site());
                            let desc_tokens = match field_args.desc {
                                Some(s) => quote!(Some(#s)),
                                None => quote!(None),
                            };
                            let example_tokens = match field_args.example {
                                Some(s) => quote!(Some(#s)),
                                None => quote!(None),
                            };
                            meta.push(quote! {
                                #key_lit => EchoExtMetaFieldCommonVal {
                                    typ: #ty_lit,
                                    desc: #desc_tokens,
                                    example: #example_tokens,
                                }
                            });
                        }
                        Err(e) => return e.to_compile_error().into(),
                    };
                    continue 'f;
                }
            }
        }
    }

    let meta_tokens = match meta.is_empty() {
        true => quote! { None },
        false => {
            quote! {
                Some(::phf::phf_map! {
                    #(#meta),*
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
        #[cfg_attr(target_vendor = "apple", unsafe(link_section = "__DATA,.echo_ext.ids"))]
        #[cfg_attr(not(target_vendor = "apple"), unsafe(link_section = ".echo_ext.ids"))]
        #[unsafe(export_name = #guard_export)]
        static #guard_ident: [u8; 0] = [];

        #[automatically_derived]
        impl #impl_generics EchoExtMeta for #name #ty_generics #where_clause {
            const ID: u32 = #id;
            const DESC: Option<&'static str> = #desc_tokens;
            const SIDE_EFFECT: bool = #side_effect;
            const FUZZ_HW: (u32, u32) = (#fuzz_h, #fuzz_w);
            const META: Option<::phf::Map<&'static str, EchoExtMetaFieldCommonVal>> = #meta_tokens;
            const EVALUATE_KEY: Option<::phf::Set<&'static str>> = #eval_tokens;
        }
    };

    expanded.into()
}

struct CodeAttr {
    value: u32,
}

impl Parse for CodeAttr {
    fn parse(input: ParseStream) -> SynResult<Self> {
        let lit: LitInt = input.parse()?;
        Ok(Self {
            value: lit.base10_parse()?,
        })
    }
}

struct VariantInfo {
    ident: Ident,
    fields: Fields,
    code: Option<u32>,
}

struct EnumInput {
    ident: Ident,
    generics: Generics,
    variants: Vec<VariantInfo>,
}

impl Parse for EnumInput {
    fn parse(input: ParseStream) -> SynResult<Self> {
        let di: DeriveInput = input.parse()?;
        let ident = di.ident;
        let generics = di.generics;
        let data = match di.data {
            Data::Enum(e) => e,
            _ => {
                return Err(SynError::new(
                    Span::call_site(),
                    "EchoErrCode can only be derived for enums",
                ));
            }
        };
        let mut variants = Vec::new();
        for v in data.variants {
            let code = v
                .attrs
                .iter()
                .find(|a| a.path().is_ident("code"))
                .and_then(|a| a.parse_args::<CodeAttr>().ok())
                .map(|c| c.value);
            variants.push(VariantInfo {
                ident: v.ident,
                fields: v.fields,
                code,
            });
        }
        Ok(Self {
            ident,
            generics,
            variants,
        })
    }
}

#[proc_macro_derive(EchoBusinessError, attributes(code))]
pub fn derive_echo_err_code(input: TokenStream) -> TokenStream {
    let EnumInput {
        ident,
        generics,
        variants,
    } = parse_macro_input!(input as EnumInput);
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let mut some_arms = Vec::new();
    let mut guards = Vec::new();

    for v in variants.iter() {
        if let Some(code) = v.code {
            let v_ident = &v.ident;
            let pat = match &v.fields {
                Fields::Unit => quote!(Self::#v_ident),
                Fields::Unnamed(_) => quote!(Self::#v_ident(..)),
                Fields::Named(_) => quote!(Self::#v_ident { .. }),
            };
            some_arms.push(quote!(#pat => Some(#code),));
            let guard_ident =
                format_ident!("__ECHO_ERR_CODE_GUARD__{}_{}_{}", ident, v_ident, code);
            let guard_export_name =
                LitStr::new(&format!("__echo_err_code__{}", code), Span::call_site());
            guards.push(quote! {
                #[doc(hidden)]
                #[used]
                #[unsafe(no_mangle)]
                #[unsafe(export_name = #guard_export_name)]
                #[cfg_attr(target_vendor = "apple", unsafe(link_section = "__DATA,.echo_err_code"))]
                #[cfg_attr(not(target_vendor = "apple"), unsafe(link_section = ".echo_err_code"))]
                static #guard_ident: [u8; 0] = [];
            });
        }
    }

    let biz_trait: syn::Path = parse_quote!(crate::errors::EchoBusinessErrCode);
    let expanded = quote! {
        #[automatically_derived]
        impl #impl_generics #biz_trait for #ident #ty_generics #where_clause {
            fn code(&self) -> Option<u32> {
                match self {
                    #(#some_arms)*
                    _ => None
                }
            }
        }
        #(#guards)*
    };
    TokenStream::from(expanded)
}
