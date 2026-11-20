//! `#[derive(Replicate)]`: one `replicate_field` call per field, in
//! declaration order, with the quantization its `#[replicate(..)]` asks for.

use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Expr, Fields, Ident, LitInt};

/// Bits of an angle when `#[replicate(angle)]` doesn't say.
const ANGLE_BITS: u8 = 12;

pub(crate) fn replicate(input: DeriveInput) -> syn::Result<TokenStream2> {
    let net = net_path();
    let name = &input.ident;
    let fields = match &input.data {
        Data::Struct(s) => &s.fields,
        Data::Enum(e) => {
            return Err(syn::Error::new(
                e.enum_token.span(),
                "`#[derive(Replicate)]` works on structs; for an enum field, implement `ReplicateField` for the enum",
            ));
        }
        Data::Union(u) => {
            return Err(syn::Error::new(
                u.union_token.span(),
                "`#[derive(Replicate)]` works on structs",
            ));
        }
    };
    let mut calls = Vec::new();
    let members: Vec<(syn::Member, String, &syn::Field)> = match fields {
        Fields::Named(named) => named
            .named
            .iter()
            .map(|f| {
                let ident = f.ident.clone().expect("named fields have names");
                let text = ident.to_string();
                (syn::Member::Named(ident), text.trim_start_matches("r#").to_string(), f)
            })
            .collect(),
        Fields::Unnamed(unnamed) => unnamed
            .unnamed
            .iter()
            .enumerate()
            .map(|(i, f)| (syn::Member::Unnamed(i.into()), i.to_string(), f))
            .collect(),
        Fields::Unit => Vec::new(),
    };
    for (member, field_name, field) in members {
        let Some(quantize) = options(field, &net)? else {
            continue; // skipped
        };
        let span = field.ty.span();
        calls.push(quote_spanned! {span=>
            #net::ReplicateField::replicate_field(&mut self.#member, #field_name, #quantize, fields);
        });
    }
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics #net::Replicate for #name #ty_generics #where_clause {
            fn fields(&mut self, fields: &mut #net::Fields<'_>) {
                #(#calls)*
            }
        }
    })
}

/// A field's quantization from its `#[replicate(..)]`, or `None` for `skip`.
fn options(field: &syn::Field, net: &TokenStream2) -> syn::Result<Option<TokenStream2>> {
    let mut range: [Option<Expr>; 3] = [None, None, None];
    let mut angle = false;
    let mut bits: Option<LitInt> = None;
    let mut skip = false;
    let mut first: Option<Span> = None;
    for attr in field.attrs.iter().filter(|a| a.path().is_ident("replicate")) {
        first.get_or_insert(attr.span());
        attr.parse_nested_meta(|meta| {
            let path = &meta.path;
            let slot = if path.is_ident("min") {
                Some(0)
            } else if path.is_ident("max") {
                Some(1)
            } else if path.is_ident("step") {
                Some(2)
            } else {
                None
            };
            if let Some(i) = slot {
                range[i] = Some(meta.value()?.parse::<Expr>()?);
            } else if path.is_ident("angle") {
                angle = true;
            } else if path.is_ident("bits") {
                bits = Some(meta.value()?.parse::<LitInt>()?);
            } else if path.is_ident("skip") {
                skip = true;
            } else {
                let option = quote!(#path).to_string().replace(' ', "");
                return Err(meta.error(format!(
                    "unknown replicate option `{option}`: use `min`, `max` and `step`, `angle`, `bits` or `skip`"
                )));
            }
            Ok(())
        })?;
    }
    let span = first.unwrap_or_else(|| field.span());
    let ranged = range.iter().any(Option::is_some);
    if skip {
        if ranged || angle || bits.is_some() {
            return Err(syn::Error::new(span, "`skip` takes no other replicate options"));
        }
        return Ok(None);
    }
    if ranged {
        let [Some(min), Some(max), Some(step)] = range else {
            return Err(syn::Error::new(
                span,
                "a range needs all of `min`, `max` and `step`, e.g. `#[replicate(min = -100.0, max = 100.0, step = 0.01)]`",
            ));
        };
        if angle || bits.is_some() {
            return Err(syn::Error::new(
                span,
                "a range (`min`, `max`, `step`) takes no `angle` or `bits`",
            ));
        }
        return Ok(Some(quote! {
            #net::Quantize::Range { min: (#min) as f32, max: (#max) as f32, step: (#step) as f32 }
        }));
    }
    let bits = match bits {
        Some(lit) => Some(
            lit.base10_parse::<u8>()
                .map_err(|_| syn::Error::new(lit.span(), "`bits` is a number of bits, 1 to 32"))?,
        ),
        None => None,
    };
    Ok(Some(match (angle, bits) {
        (true, bits) => {
            let bits = bits.unwrap_or(ANGLE_BITS);
            quote!(#net::Quantize::Angle { bits: #bits })
        }
        (false, Some(bits)) => quote!(#net::Quantize::Bits(#bits)),
        (false, None) => quote!(#net::Quantize::Exact),
    }))
}

/// The engine's `net` module as the user's crate sees it (see
/// `component_trait` for how the name is found).
fn net_path() -> TokenStream2 {
    let name = match crate_name("ponko") {
        Ok(FoundCrate::Itself) => "ponko".to_string(),
        Ok(FoundCrate::Name(name)) => name,
        Err(_) => "ponko".to_string(),
    };
    let name = Ident::new(&name, Span::call_site());
    quote!(::#name::net)
}
