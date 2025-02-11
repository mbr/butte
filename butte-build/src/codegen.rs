use crate::types::*;

#[cfg(test)]
use crate::{field, table};

use butte::VOffsetT;
use heck::{ShoutySnakeCase, SnakeCase};
use itertools::Itertools;
use proc_macro2::TokenStream;
use quote::{format_ident, quote, ToTokens};
use std::{convert::TryInto, fmt::Display};
use syn::spanned::Spanned;

#[cfg(test)]
fn to_code(value: impl ToTokens) -> String {
    format!("{}", value.to_token_stream())
}

#[cfg(test)]
mod constant_tests {
    use super::*;

    #[test]
    fn test_visit_floating_constant() {
        let result = to_code(1.0);
        let expected = "1f64";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_visit_integer_constant() {
        let result = to_code(1);
        let expected = "1i32";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_visit_string_constant() {
        let result = to_code("abc");
        let expected = "\"abc\"";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_visit_bool_constant() {
        let result = to_code(true);
        let expected = "true";
        assert_eq!(result, expected);

        let result = to_code(false);
        let expected = "false";
        assert_eq!(result, expected);
    }
}

impl ToTokens for Ident<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        format_ident!("{}", self.raw).to_tokens(tokens)
    }
}

#[cfg(test)]
mod ident_tests {
    use super::*;

    #[test]
    fn test_visit_ident() {
        let result = to_code(Ident::from("foo"));
        let expected = "foo";
        assert_eq!(result, expected);
    }
}

impl ToTokens for Scalar {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            Scalar::Integer(i) => i.to_tokens(tokens),
            Scalar::Float(f) => f.to_tokens(tokens),
            Scalar::Boolean(b) => b.to_tokens(tokens),
        }
    }
}

/// Convert a `types::Type` to a type with the supplied wrapper for reference types
fn to_type(ty: &Type, lifetime: impl ToTokens, wrap_refs_types: impl ToTokens) -> impl ToTokens {
    match ty {
        Type::String => {
            let wrap_tokens = wrap_refs_types.into_token_stream();
            if wrap_tokens.is_empty() {
                quote!(&#lifetime str)
            } else {
                quote!(#wrap_tokens::<&#lifetime str>)
            }
        }
        // TODO other reference types?
        _ => quote!(#ty),
    }
}

fn offset_id(field: &Field) -> impl ToTokens {
    format_ident!("VT_{}", field.id.as_ref().to_shouty_snake_case())
}

impl ToTokens for Table<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let Self {
            id: struct_id,
            fields,
            doc,
            ..
        } = self;

        let raw_struct_name = struct_id.raw;

        let builder_add_calls = fields.iter().map(|Field { id: field_id, .. }| {
            let raw_field_name = field_id.raw;
            let add_field_method = format_ident!("add_{}", raw_field_name);
            quote!(builder.#add_field_method(args.#field_id);)
        });

        let args = format_ident!("{}Args", raw_struct_name);
        let args_fields = fields.iter().map(
            |Field {
                 id: field_id,
                 ty,
                 scalar,
                 ..
             }| {
                let arg_ty = to_type(ty, quote!('a), quote!(butte::WIPOffset));
                // Scalar fields can have a default value
                let default = if let Some(default_value) = scalar {
                    quote!(#[default = #default_value])
                } else {
                    quote!()
                };
                quote! {
                    #default
                    pub #field_id: #arg_ty
                }
            },
        );

        let builder_type = format_ident!("{}Builder", struct_id.raw);

        // TODO: field accessors
        // TODO: check the impl of offset generation
        // TODO: unions
        // TODO: testing this is going to be fun
        let field_nested_flatbuffers = fields.iter().filter_map(|Field { id: field_id, metadata, .. }| {
            let method_name = format_ident!("{}_nested_flatbuffer", field_id.raw);
            if let Some(metadata) = metadata {
                if metadata.values.contains_key(&Ident::from("nested_flatbuffer")) {
                    Some(quote! {
                        pub fn #method_name(&self) -> Option<Self> {
                            self.#field_id.map(|data| <butte::ForwardsUOffset<Self>>::follow(data, 0))
                        }
                    })
                } else {
                    None
                }
            } else {
                None
            }
        });

        let builder_field_methods = fields.iter().map(|field| {
            let Field {
                id: field_id,
                ty,
                scalar,
                ..
            } = field;
            let add_method_name = format_ident!("add_{}", field_id.raw);
            let offset = offset_id(&field);
            let field_offset = quote!(#struct_id::#offset);
            let arg_ty = to_type(ty, quote!('_), quote!(butte::WIPOffset));
            let body = if ty.is_scalar() {
                if let Some(default_value) = scalar {
                    quote!(self.fbb.push_slot<#arg_ty>(#field_offset, #field_id, #default_value))
                } else {
                    quote!(self.fbb.push_slot_always::<#arg_ty>(#field_offset, #field_id))
                }
            } else {
                quote!(self.fbb.push_slot_always::<#arg_ty>(#field_offset, #field_id))
            };
            let arg_ty = to_type(ty, quote!('b), quote!(butte::WIPOffset));
            quote! {
                #[inline]
                fn #add_method_name(&mut self, #field_id: #arg_ty) {
                    #body;
                }
            }
        });

        let field_offset_constants = fields.iter().enumerate().map(|(index, field)| {
            let offset_name = offset_id(&field);
            let offset_value = butte::field_index_to_field_offset(index as VOffsetT);
            quote! {
                pub const #offset_name: butte::VOffsetT = #offset_value;
            }
        });

        let field_accessors = fields.iter().map(|field| {
            let snake_name = format_ident!("{}", field.id.as_ref().to_snake_case());
            let offset_name = offset_id(&field);
            let ty = &field.ty;
            let ty_simple_lifetime = to_type(ty, quote!('a), quote!());
            let ty_wrapped = to_type(ty, quote!(), quote!(butte::ForwardsUOffset));

            quote! {
                #[inline]
                pub fn #snake_name(&self) -> Option<#ty_simple_lifetime> {
                    self.table
                        .get::<#ty_wrapped>(#struct_id::#offset_name, None)
                }
            }
        });

        let struct_offset_enum_name = format_ident!("{}Offset", struct_id.raw);

        let required_fields = fields.iter().map(|field| {
            let snake_name = field.id.as_ref().to_snake_case();
            let offset_name = offset_id(field);
            quote! {
                self.fbb.required(o, #struct_id::#offset_name, #snake_name);
            }
        });

        (quote! {
            pub enum #struct_offset_enum_name {}

            #[derive(Copy, Clone, Debug, PartialEq)]
            #doc
            pub struct #struct_id<'a> {
                table: butte::Table<'a>,
            }

            impl<'a> From<butte::Table<'a>> for #struct_id<'a> {
                fn from(table: butte::Table<'a>) -> Self {
                    Self { table }
                }
            }

            impl<'a> #struct_id<'a> {
                pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr>(
                    fbb: &'mut_bldr mut butte::FlatBufferBuilder<'bldr>,
                    args: &'args #args<'args>
                ) -> butte::WIPOffset<#struct_id<'bldr>> {
                    let mut builder = #builder_type::new(fbb);
                    #(#builder_add_calls)*
                    builder.finish()
                }

                // field offset constants
                #(#field_offset_constants)*

                // fields access
                #(#field_accessors)*

                // nested flatbuffers if applicable
                #(#field_nested_flatbuffers)*
            }

            impl<'a> butte::Follow<'a> for #struct_id<'a> {
                type Inner = Self;

                #[inline]
                fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
                    let table = butte::Table { buf, loc };
                    Self { table }
                }
            }

            // Builder Args
            // TODO: Can't use this because we can mix fields that are
            // default-able with those that are not
            pub struct #args<'a> {
                #(#args_fields),*
            }

            //// builder
            pub struct #builder_type<'a, 'b> {
                fbb: &'b mut butte::FlatBufferBuilder<'a>,
                start: butte::WIPOffset<butte::TableUnfinishedWIPOffset>,
            }

            impl<'a: 'b, 'b> #builder_type<'a, 'b> {
                #(#builder_field_methods)*

                #[inline]
                pub fn new(fbb: &'b mut butte::FlatBufferBuilder<'a>) -> Self {
                    let start = fbb.start_table();
                    #builder_type {
                        fbb, start
                    }
                }

                #[inline]
                pub fn finish(self) -> butte::WIPOffset<#struct_id<'a>> {
                    let o = self.fbb.end_table(self.start);
                    #(#required_fields)*
                    butte::WIPOffset::new(o.value())
                }
            }
        })
        .to_tokens(tokens)
    }
}

#[cfg(test)]
mod product_type_tests {
    use super::*;

    #[test]
    fn test_visit_product_type_table() {
        let table = table!(
            MyMessage,
            [field!(message, String), field!(foo, Float64 = 2.0)]
        );
        let result = to_code(table);
        assert!(!result.is_empty());
    }
}

impl ToTokens for Type<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            Type::Bool => quote!(bool),
            Type::Byte => quote!(i8),
            Type::UByte => quote!(u8),
            Type::Short => quote!(i16),
            Type::UShort => quote!(u16),
            Type::Int => quote!(i32),
            Type::UInt => quote!(u32),
            Type::Float => quote!(f32),
            Type::Long => quote!(i64),
            Type::ULong => quote!(u64),
            Type::Double => quote!(f64),
            Type::Int8 => quote!(i8),
            Type::UInt8 => quote!(u8),
            Type::Int16 => quote!(i16),
            Type::UInt16 => quote!(u16),
            Type::Int32 => quote!(i32),
            Type::UInt32 => quote!(u32),
            Type::Int64 => quote!(i64),
            Type::UInt64 => quote!(u64),
            Type::Float32 => quote!(f32),
            Type::Float64 => quote!(f64),
            Type::String => quote!(String),
            Type::Array(ty) => quote!(Vec<#ty>),
            Type::Ident(id) => quote!(#id),
        }
        .to_tokens(tokens)
    }
}

#[cfg(test)]
mod type_tests {
    use super::*;

    #[test]
    fn test_visit_type() {
        let result = to_code(Type::Bool);
        let expected = "bool";
        assert_eq!(result, expected);

        let result = to_code(Type::Array(Box::new(Type::String)));
        let expected = "Vec < String >";
        assert_eq!(result, expected);

        let result = to_code(Type::Ident(DottedIdent::from(vec!["MyType".into()])));
        let expected = "MyType";
        assert_eq!(result, expected);
    }
}

// TODO: Properly implement this.
// We only generate a trait method right now.
// TODO: Figure out how this will integrate into tonic.
impl ToTokens for RpcMethod<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let Self {
            id,
            request_type,
            response_type,
            doc,
            ..
        } = self;
        let snake_name = format_ident!("{}", id.raw.to_snake_case());
        (quote! {
            #doc
            fn #snake_name(request: #request_type) -> #response_type;
        })
        .to_tokens(tokens)
    }
}

impl ToTokens for Comment<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let doc = self.lines.iter().rev().fold(quote!(), |docs, line| {
            quote! {
                #[doc = #line]
                #docs
            }
        });
        doc.to_tokens(tokens)
    }
}

impl ToTokens for DottedIdent<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let parts = &self.parts;
        debug_assert!(!self.parts.is_empty());
        let code = parts.iter().map(|e| e.raw).join("::");
        let num_parts = parts.len();
        let path_string = if num_parts > 1 {
            format!(
                "{}::{}",
                std::iter::repeat("super").take(num_parts - 1).join("::"),
                code
            )
        } else {
            code
        };
        syn::parse_str::<syn::Path>(&path_string)
            .expect("Cannot parse path")
            .to_tokens(tokens)
    }
}

// TODO: This is woefully incomplete
impl ToTokens for Rpc<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let Self {
            id: Ident { raw },
            methods,
            doc,
        } = self;
        let service_name = format_ident!("{}Service", raw);
        (quote! {
            #doc
            pub trait #service_name {
                #(#methods)*
            }
        })
        .to_tokens(tokens)
    }
}

fn lit_int(value: impl Display, base_type: impl Spanned + Display) -> impl ToTokens {
    let stringified_int = format!("{}_{}", value, base_type);
    syn::LitInt::new(&stringified_int, base_type.span())
}

// TODO: Unions. Though see structs, because those overlap with unions in a way
// that isn't clear to me ATM. It seems that unions can be enum-like or
// struct-like depending on the element types.
impl ToTokens for Enum<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let Self {
            id: enum_id,
            values,
            base_type,
            doc,
            ..
        } = self;

        // generate enum variant name => string name of the variant for use in
        // a match statement
        let names_to_strings = values.iter().map(|EnumVal { id: key, .. }| {
            let raw_key = key.raw;
            quote! {
                #enum_id::#key => #raw_key
            }
        });

        // assign a value to the key if one was given, otherwise give it the
        // enumerated index's value
        let fields = values
            .iter()
            .enumerate()
            .map(|(i, EnumVal { id: key, value })| {
                // format the value with the correct type, i.e., base_type
                let scalar_value = lit_int(
                    if let Some(constant) = *value {
                        constant
                    } else {
                        i.try_into().expect("invalid conversion to enum base type")
                    },
                    base_type.to_token_stream(),
                );
                quote! {
                    #key = #scalar_value
                }
            });

        let raw_snake_enum_name = enum_id.raw.to_snake_case();
        let enum_id_fn_name = format_ident!("enum_name_{}", raw_snake_enum_name);

        // TODO: Maybe separate these pieces to avoid variables that used far
        // away from their definition.
        (quote! {
            // force a C-style enum
            #[repr(#base_type)]
            #[allow(non_camel_case_types)]
            #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
            #doc
            pub enum #enum_id {
                #(#fields),*
            }

            impl<'a> butte::Follow<'a> for #enum_id {
                type Inner = Self;

                fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
                    butte::read_scalar_at::<Self>(buf, loc)
                }
            }

            impl butte::EndianScalar for #enum_id {
                #[inline]
                fn to_little_endian(self) -> Self {
                    let n = #base_type::to_le(self as #base_type);
                    let p = &n as *const #base_type as *const Self;
                    unsafe { *p }
                }

                #[inline]
                fn from_little_endian(self) -> Self {
                    let n = #base_type::from_le(self as #base_type);
                    let p = &n as *const #base_type as *const Self;
                    unsafe { *p }
                }
            }

            impl butte::Push for #enum_id {
                type Output = Self;

                #[inline]
                fn push(&self, dst: &mut [u8], _rest: &[u8]) {
                    butte::emplace_scalar::<Self>(dst, *self);
                }
            }

            pub fn #enum_id_fn_name(e: #enum_id) -> &'static str {
                match e {
                    #(#names_to_strings),*
                }
            }
        })
        .to_tokens(tokens)
    }
}

// TODO: better error messages for things that aren't implemented
impl ToTokens for Element<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        // the following constructs are (or should be) handled at the file
        // level:
        // * Namespaces
        // * Root types
        // * File extensions
        // * File identifiers
        //
        // Additionally, attributes do not have corresponding concrete code
        // generated, they are used to *affect* codegen of other items.
        match self {
            Element::Table(t) => t.to_tokens(tokens),
            Element::Struct(_) => unimplemented!(),
            Element::Enum(e) => e.to_tokens(tokens),

            Element::Root(_) => unimplemented!(),
            Element::FileExtension(_) => unimplemented!(),
            Element::FileIdentifier(_) => unimplemented!(),
            Element::Attribute(_) => unimplemented!(),
            Element::Rpc(rpc) => rpc.to_tokens(tokens),
            Element::Object(_) => unimplemented!(),
            element => panic!("{:?}", element),
        }
    }
}

// TODO: actually open up a file
// this will need some notion of a default include path to search for fbs
// files.
impl ToTokens for Include<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let Self { stem, doc, .. } = self;
        let id = format_ident!("{}", stem);
        (quote! {
            #doc
            use #id::*;
        })
        .to_tokens(tokens)
    }
}

// TODO:
// * Root types
// * File identifiers
// * File extensions
// * Parsing included files
impl ToTokens for Schema<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let Self { includes, elements } = self;

        // namespaces precede all of their contents, so track the current namespace and accumulate
        // its change into the key of a map of namespace -> elements contained within that
        // namespace.
        let code = elements
            .iter()
            .scan((None, None), |ns_item_pair, element| {
                *ns_item_pair = if element.is_namespace() {
                    (element.namespace(), None)
                } else {
                    (ns_item_pair.0, Some(element))
                };
                Some(*ns_item_pair)
            })
            .filter(|(_, element)| element.is_some())
            .into_group_map()
            .into_iter()
            // for each non-None namespace and elements contained within:
            // reverse fold over the namespace pieces to generate nested modules:
            // if namespace parts is [a, b, c]
            // and body is struct Foo { ... }
            // then
            // reverse:
            //   [c, b, a]
            // fold:
            //    init: struct Foo { ... }
            //    next: pub mod c { struct Foo { ... } }
            //    next: pub mod b { pub mod c { struct Foo { ... } } }
            //   final: pub mod a { pub mod b { pub mod c { struct Foo { ... } } } }
            .map(|(namespace, elements)| {
                let base_body = quote! { #(#elements)* };
                if let Some(Namespace { ident, doc }) = namespace {
                    let nested =
                        ident
                            .parts
                            .iter()
                            .rev()
                            .fold(base_body, |module_body, module_name| {
                                quote! {
                                    pub mod #module_name {
                                        #module_body
                                    }
                                }
                            });
                    quote! {
                        #doc
                        #nested
                    }
                } else {
                    base_body
                }
            });

        (quote! {
            #(#includes)*
            #(#code)*
        })
        .to_tokens(tokens)
    }
}

impl ToTokens for File<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let Self { schema, .. } = self;
        schema.to_tokens(tokens)
    }
}
