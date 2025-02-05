use object::read::pe::{PeFile32, PeFile64};
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, LitStr};

const FORWARD_ATTR_LACK_MESSAGE: &str =
    r#"你需要添加 #[forward(target = "path/of/target_dll.dll")]"#;
const FORWARD_ATTR_INVALID_MESSAGE: &str = r#"#[forward()] 的参数格式错误，正确格式如 #[forward(target = "C:\Windows\System32\version.dll")]"#;

/// ForwardModule 派生宏。用于读取 DLL 的导出表，生成用于转发的导出函数。
///
/// # 使用方式：
///
/// ```rust,ignore
/// use forward_dll::ForwardModule;
///
/// #[derive(ForwardModule)]
/// #[forward(target = "C:\\Windows\\System32\\version.dll")]
/// struct VersionModule;
/// ```
#[proc_macro_derive(ForwardModule, attributes(forward))]
pub fn derive_forward_module(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as syn::DeriveInput);
    let forward_attr = input
        .attrs
        .iter()
        .find(|i| i.path().is_ident("forward"))
        .expect(FORWARD_ATTR_LACK_MESSAGE);

    // 解析 #[forward(target = "")] 的参数。
    let mut dll_path: Option<LitStr> = None;
    forward_attr
        .parse_nested_meta(|meta| {
            let path = &meta.path;
            if path.is_ident("target") {
                let value = meta.value().expect(FORWARD_ATTR_INVALID_MESSAGE);
                dll_path = Some(value.parse().expect(FORWARD_ATTR_INVALID_MESSAGE));
            } else {
                return Err(meta.error(FORWARD_ATTR_INVALID_MESSAGE));
            }
            Ok(())
        })
        .expect(FORWARD_ATTR_INVALID_MESSAGE);

    let dll_path = dll_path.expect(FORWARD_ATTR_INVALID_MESSAGE);
    let exports = get_dll_export_names(dll_path.value().as_str())
        .expect("指定的 DLL 可能是一个无效的 PE 文件");

    let export_names: Vec<_> = exports.iter().map(|(_, fn_name)| fn_name).collect();
    let export_idents: Vec<_> = exports
        .iter()
        .map(|(_, fn_name)| format_ident!("{}", fn_name.replace("@", "_")))
        .collect();
    let export_count = exports.len();
    let struct_name = input.ident;
    let impl_code = quote! {
        const _ : () = {
            extern crate forward_dll as _forward_dll;

            static mut _FORWARDER: _forward_dll::DllForwarder<#export_count> = _forward_dll::DllForwarder {
                initialized: false,
                module_handle: 0,
                lib_name: #dll_path,
                target_functions_address: [0; #export_count],
                target_function_names: [#(#export_names),*],
            };

            _forward_dll::define_function!(#dll_path, _FORWARDER, 0, #(#export_idents)*);

            impl _forward_dll::ForwardModule for #struct_name {
                fn init(&self) -> _forward_dll::ForwardResult<()> {
                    unsafe { _FORWARDER.forward_all() }
                }
            }
        };
    };
    impl_code.into()
}

fn get_dll_export_names(dll_path: &str) -> Result<Vec<(u32, String)>, String> {
    let dll_file = std::fs::read(dll_path).map_err(|err| format!("Failed to read file: {err}"))?;
    let in_data = dll_file.as_slice();

    let kind = object::FileKind::parse(in_data).map_err(|err| format!("Invalid file: {err}"))?;
    let exports = match kind {
        object::FileKind::Pe32 => PeFile32::parse(in_data)
            .map_err(|err| format!("Invalid pe file: {err}"))?
            .export_table()
            .map_err(|err| format!("Invalid pe file: {err}"))?
            .ok_or_else(|| "No export table".to_string())?
            .exports(),
        object::FileKind::Pe64 => PeFile64::parse(in_data)
            .map_err(|err| format!("Invalid pe file: {err}"))?
            .export_table()
            .map_err(|err| format!("Invalid pe file: {err}"))?
            .ok_or_else(|| "No export table".to_string())?
            .exports(),
        _ => return Err("Invalid file".to_string()),
    }
    .map_err(|err| format!("Invalid file: {err}"))?;

    let mut names = Vec::new();
    for export_item in exports {
        names.push((
            export_item.ordinal,
            export_item
                .name
                .map(String::from_utf8_lossy)
                .map(String::from)
                .unwrap_or_default(),
        ));
    }
    Ok(names)
}
