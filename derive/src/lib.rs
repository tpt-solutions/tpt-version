//! `#[derive(Entity)]` for tpt-version.
//!
//! Every named field is stored under its own name, so the struct round-trips
//! through its `UirNode` form. Exactly one field must be marked
//! `#[entity(id)]`. It has to be an `EntityId`, and it is also stored as a
//! field, so the stored form reads back with the same id.
//!
//! ```ignore
//! use tpt_version::prelude::*;
//!
//! #[derive(Entity)]
//! struct Vertex {
//!     #[entity(id)]
//!     id: EntityId,
//!     position: [f64; 3],
//!     label: String,
//! }
//! ```
//!
//! Each field type must implement `tpt_version::UirValue`. That covers the
//! scalars, `EntityId`, `Option`, `Vec` and fixed-size arrays.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Data, DeriveInput, Error, Fields, Ident, LitStr, parse_macro_input};

/// Derives `tpt_version::Entity` for a struct with named fields.
#[proc_macro_derive(Entity, attributes(entity))]
pub fn derive_entity(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(&input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

fn expand(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let Data::Struct(data) = &input.data else {
        return Err(Error::new_spanned(
            &input.ident,
            "Entity can only be derived for a struct",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(Error::new_spanned(
            &input.ident,
            "Entity needs a struct with named fields",
        ));
    };

    let mut id_field: Option<Ident> = None;
    let mut idents = Vec::new();
    for field in &fields.named {
        let ident = field.ident.clone().expect("named fields have identifiers");
        for attribute in field.attrs.iter().filter(|a| a.path().is_ident("entity")) {
            attribute.parse_nested_meta(|meta| {
                if !meta.path.is_ident("id") {
                    return Err(meta.error("unknown entity attribute, expected `id`"));
                }
                if id_field.is_some() {
                    return Err(meta.error("only one field can be #[entity(id)]"));
                }
                id_field = Some(ident.clone());
                Ok(())
            })?;
        }
        idents.push(ident);
    }

    let Some(id) = id_field else {
        return Err(Error::new_spanned(
            &input.ident,
            "Entity needs one field marked #[entity(id)]",
        ));
    };
    let names: Vec<LitStr> = idents
        .iter()
        .map(|ident| LitStr::new(&ident.to_string(), ident.span()))
        .collect();

    let name = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics ::tpt_version::Entity for #name #type_generics #where_clause {
            fn id(&self) -> ::tpt_version::EntityId {
                self.#id
            }

            fn to_uir(&self) -> ::tpt_version::UirNode {
                ::tpt_version::entity::__map([
                    #( (#names, ::tpt_version::UirValue::to_uir_value(&self.#idents)), )*
                ])
            }

            fn from_uir(
                node: &::tpt_version::UirNode,
            ) -> ::core::result::Result<Self, ::tpt_version::ObjectError> {
                ::core::result::Result::Ok(Self {
                    #( #idents: ::tpt_version::UirValue::from_uir_value(
                        ::tpt_version::entity::__field(node, #names)?,
                    )?, )*
                })
            }
        }
    })
}
