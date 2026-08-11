use quote::{format_ident, quote, ToTokens};
use std::{env, fs, path::PathBuf};
use syn::{ForeignItem, FnArg, Item, ReturnType};

const MODULES: &[&str] = &[
    "lib",
    "offline_asr",
    "offline_speaker_diarization",
    "online_asr",
    "resampler",
    "speaker_embedding",
    "vad",
];

fn main() {
    println!("cargo:rerun-if-changed=src");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));

    for module in MODULES {
        let source = fs::read_to_string(format!("src/{module}.rs")).expect("read sys module");
        let file = syn::parse_file(&source).expect("parse sys module");
        let mut wrappers = Vec::new();

        for item in file.items {
            let Item::ForeignMod(foreign) = item else { continue };
            for item in foreign.items {
                let ForeignItem::Fn(function) = item else { continue };
                let mut signature = function.sig;
                signature.unsafety = Some(syn::parse_quote!(unsafe));
                let name = &signature.ident;
                let argument_names = signature
                    .inputs
                    .iter()
                    .filter_map(|argument| match argument {
                        FnArg::Typed(argument) => Some(argument.pat.to_token_stream()),
                        FnArg::Receiver(_) => None,
                    })
                    .collect::<Vec<_>>();
                let argument_types = signature
                    .inputs
                    .iter()
                    .filter_map(|argument| match argument {
                        FnArg::Typed(argument) => Some(argument.ty.to_token_stream()),
                        FnArg::Receiver(_) => None,
                    })
                    .collect::<Vec<_>>();
                let return_type = match &signature.output {
                    ReturnType::Default => quote!(()),
                    ReturnType::Type(_, ty) => ty.to_token_stream(),
                };
                let fallback = match &signature.output {
                    ReturnType::Default => quote!(()),
                    ReturnType::Type(_, ty) => {
                        let rendered = ty.to_token_stream().to_string();
                        if rendered.starts_with("* const") {
                            quote!(std::ptr::null())
                        } else if rendered.starts_with("* mut") {
                            quote!(std::ptr::null_mut())
                        } else {
                            quote!(Default::default())
                        }
                    }
                };
                let function_type = format_ident!("__{}_fn", name);
                let symbol = format!("{}\0", name);

                wrappers.push(quote! {
                    #[allow(non_camel_case_types)]
                    type #function_type = unsafe extern "C" fn(#(#argument_types),*) -> #return_type;

                    #[allow(non_snake_case)]
                    pub #signature {
                        match crate::dynamic::symbol::<#function_type>(#symbol.as_bytes()) {
                            Ok(function) => unsafe { function(#(#argument_names),*) },
                            Err(_) => #fallback,
                        }
                    }
                });
            }
        }

        fs::write(output.join(format!("{module}_dynamic.rs")), quote!(#(#wrappers)*) .to_string())
            .expect("write dynamic wrappers");
    }
}
