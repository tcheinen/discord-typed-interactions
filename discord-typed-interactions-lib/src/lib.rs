use heck::{CamelCase, SnakeCase};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CommandOption {
    // TODO: ensure zero is caught as an illegal type for subcommands at runtime,
    // or make a top-level struct without a `type` field
    #[serde(default)]
    r#type: u8,
    name: String,
    options: Option<Vec<CommandOption>>,
}

impl CommandOption {
    fn print_kind(&self) -> &'static str {
        match self.r#type {
            4 => "u64",
            5 => "bool",
            3 | 6 | 7 | 8 | 9 => "String",
            invalid => panic!("invalid CommandOption kind {}", invalid),
        }
    }
}

pub fn generate_deserialize_impl(opts: &[CommandOption]) -> TokenStream {
    if opts.is_empty() {
        return quote! {
            fn parse_property<'de, D>(deserializer: D) -> Result<Options, D::Error>
                where
                    D: Deserializer<'de>,
                {
                    struct PropertyParser;
                    impl<'de> Visitor<'de> for PropertyParser {
                        type Value = Options;

                        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                            // TODO actually write this lol
                            formatter.write_str("aaa")
                        }

                        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                            Ok(Options {})
                        }
                    }
                    deserializer.deserialize_any(PropertyParser {})
                }
        };
    }
    let enum_fields = opts.iter().map(|opt| {
        let ident_snake_case = opt.name.to_snake_case();
        let ident_camel_case = mk_ident(&opt.name.to_camel_case());
        let type_ident = mk_ident(opt.print_kind());
        quote! {
            #[serde(rename = #ident_snake_case)]
            #ident_camel_case(#type_ident)
        }
    });

    let match_fields = opts.iter().map(|opt| {
        let ident_snake_case = mk_ident(&opt.name.to_snake_case());
        let ident_camel_case = mk_ident(&opt.name.to_camel_case());
        quote! {
            Property::#ident_camel_case(v) => prop.#ident_snake_case = v
        }
    });

    quote! {
        fn parse_property<'de, D>(deserializer: D) -> Result<Options, D::Error>
            where
                D: Deserializer<'de>,
            {
                #[derive(serde::Serialize, serde::Deserialize, Debug)]
                #[serde(tag = "name", content = "value")]
                enum Property {
                    #(#enum_fields),*,
                }

                struct PropertyParser;
                impl<'de> Visitor<'de> for PropertyParser {
                    type Value = Options;

                    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                        // TODO actually write this lol
                        formatter.write_str("aaa")
                    }

                    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                        let mut prop = Options {
                            ..Default::default()
                        };
                        while let Some(tmp) = seq.next_element::<Property>()? {
                            match tmp {
                                #(#match_fields),*,
                            }
                        }
                        Ok(prop)
                    }
                }
                deserializer.deserialize_any(PropertyParser {})
            }
    }
}

pub fn structify_data(input: &CommandOption) -> Option<TokenStream> {
    let opts = input.options.as_ref()?;
    let name = mk_ident(&input.name.to_camel_case());
    let fields = opts.iter().map(|x| {
        let kind = mk_ident(x.print_kind());
        let name = mk_ident(&x.name);
        quote! {
           pub #name: #kind
        }
    });
    let deser_impl = generate_deserialize_impl(&opts);

    let mod_ident = mk_ident(&input.name.to_snake_case());
    Some(quote! {
        pub mod #mod_ident {
            use serde::de::{SeqAccess, Visitor};
            use serde::Deserializer;
            use std::fmt;
            use std::fmt::Write;
            #[derive(serde::Serialize, serde::Deserialize, Debug)]
            pub struct #name {
                pub name: String,
                #[serde(deserialize_with = "parse_property")]
                pub options: Options,
            }
            #[derive(serde::Serialize, Debug, Default)]
            pub struct Options {
                #(#fields),*
            }
            #deser_impl
        }
    })
}
pub fn extract_modules(
    schema: &CommandOption,
) -> (Vec<&CommandOption>, HashMap<&str, Vec<&CommandOption>>) {
    fn recurse<'schema>(
        next: &'schema CommandOption,
        path: &mut Vec<&'schema str>,
        root: &mut Vec<&'schema CommandOption>,
        modules: &mut HashMap<&'schema str, Vec<&'schema CommandOption>>,
    ) {
        if let Some(arr) = next.options.as_ref() {
            if arr.iter().all(|x| x.options.is_none()) {
                if path.len() == 1 {
                    root.push(next);
                } else {
                    modules
                        .entry(path[1])
                        .and_modify(|v| v.push(next))
                        .or_insert_with(|| vec![next]);
                }
            }
            path.push(&next.name);
            for i in arr.iter() {
                recurse(i, path, root, modules);
            }
            path.pop();
        }
    }
    let mut root = Vec::new();
    let mut modules = HashMap::new();
    recurse(schema, &mut Vec::new(), &mut root, &mut modules);
    (root, modules)
}

fn mk_enum_field(
    input: &str,
    root_name: &Ident,
    subenum: Option<&Ident>,
    submodule: Option<&Ident>,
) -> TokenStream {
    let snake_case_ident = mk_ident(&input.to_snake_case());
    let camel_case_ident = mk_ident(&input.to_camel_case());
    let qualified_type_elements = [
        Some(root_name),
        submodule,
        Some(&snake_case_ident),
        subenum,
        Some(&camel_case_ident),
    ];
    let qualified_type_elements = <[_; 5]>::into_iter(qualified_type_elements).flat_map(|x| x);
    quote! {
        #camel_case_ident(crate::#(#qualified_type_elements)::*)
    }
}

fn mk_ident(input: &str) -> Ident {
    Ident::new(input, Span::call_site())
}

pub fn structify(input: &str) -> TokenStream {
    let schema: CommandOption = serde_json::from_str(input).unwrap();

    let (root, modules) = extract_modules(&schema);

    let root_name_camelcase = mk_ident(&schema.name.to_camel_case());
    let root_name = mk_ident(&schema.name);
    let subcommand_struct_tokens = modules.iter().map(|(k, v)| {
        let mod_ident = mk_ident(k);
        let enum_ident = mk_ident(&k.to_camel_case());
        let fields = v.iter().flat_map(|x| structify_data(x));
        // is k necessarily snake_case?
        let enum_tokens = v
            .iter()
            .map(|x| mk_enum_field(&x.name, &root_name, None, Some(&mk_ident(k))));
        quote! {
            pub mod #mod_ident {
                use serde::de::{SeqAccess, Visitor};
                use serde::Deserializer;
                use std::fmt;
                use std::fmt::Write;
                #(#fields)*
                pub mod cmd {
                    #[derive(serde::Serialize, serde::Deserialize, Debug)]
                    #[serde(untagged)]
                    pub enum #enum_ident {
                        #(#enum_tokens),*,
                    }
                }
            }
        }
    });
    let root_enum_tokens = root
        .iter()
        .map(|x| mk_enum_field(&x.name, &root_name, None, None));
    let root_module_tokens = modules
        .keys()
        .map(|x| mk_enum_field(x, &root_name, Some(&mk_ident("cmd")), None));
    let root_struct_tokens = root.iter().flat_map(|x| structify_data(x));
    let token = quote! {
        pub mod #root_name {
            #(#root_struct_tokens)*
            pub mod cmd {
                #[derive(serde::Serialize, serde::Deserialize, Debug)]
                pub struct #root_name_camelcase {
                    id: String,
                    name: String,
                    options: Vec<Options>
                }

                #[derive(serde::Serialize, serde::Deserialize, Debug)]
                #[serde(untagged)]
                pub enum Options {
                    #(#root_enum_tokens),*,
                    #(#root_module_tokens),*,
                }
            }
            #(#subcommand_struct_tokens)*
        }
    };
    token
}

#[cfg(test)]
mod tests {
    use crate::{generate_deserialize_impl, structify, structify_data, CommandOption};
    use quote::quote;
    use serde_json::json;
    use std::fmt;
    use std::io::Write;
    use std::process::{Command, Stdio};

    #[derive(PartialEq)]
    struct DisplayString(String);

    impl fmt::Debug for DisplayString {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(&self.0)
        }
    }

    fn fmt(input: &str) -> Option<String> {
        let mut proc = Command::new("rustfmt")
            .arg("--emit=stdout")
            .arg("--edition=2018")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let stdin = proc.stdin.as_mut()?;
        stdin.write_all(input.as_bytes()).ok()?;
        let output = proc.wait_with_output().ok()?;

        if output.status.success() {
            String::from_utf8(output.stdout).ok()
        } else {
            None
        }
    }

    // this is a macro to preserve line information on failure; also,
    // this should only be used on strings that contain Rust code
    macro_rules! assert_eq {
        ($a:expr, $b:expr) => {
            if let (Some(a), Some(b)) = (fmt(&$a), fmt(&$b)) {
                let a = DisplayString(a);
                let b = DisplayString(b);
                pretty_assertions::assert_eq!(a, b);
            } else {
                pretty_assertions::assert_eq!($a, $b);
            }
        };
    }

    #[test]
    fn command_data_no_options() {
        let experimental = structify_data(
            &serde_json::from_value(json!({
                "name": "test",
                "description": "test",
                "options": []
            }))
            .unwrap(),
        )
        .unwrap()
        .to_string();
        println!("{}", &experimental);

        let actual = quote! {
            pub mod test {
                use serde::de::{SeqAccess, Visitor};
                use serde::Deserializer;
                use std::fmt;
                use std::fmt::Write;
                #[derive(serde :: Serialize, serde :: Deserialize, Debug)]
                pub struct Test {
                    pub name: String,
                    #[serde(deserialize_with = "parse_property")]
                    pub options: Options,
                }
                #[derive(serde :: Serialize, Debug, Default)]
                pub struct Options {}
                fn parse_property<'de, D>(deserializer: D) -> Result<Options, D::Error>
                where
                    D: Deserializer<'de>,
                {
                    struct PropertyParser;
                    impl<'de> Visitor<'de> for PropertyParser {
                        type Value = Options;
                        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                            formatter.write_str("aaa")
                        }
                        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                            Ok(Options {})
                        }
                    }
                    deserializer.deserialize_any(PropertyParser {})
                }
            }
        }
        .to_string();
        assert_eq!(experimental, actual);
    }

    #[test]
    fn command_data_no_subcommand() {
        let experimental = structify_data(
            &serde_json::from_value(json!({
                "name": "test",
                "description": "test",
                "options": [
                    {
                        "name": "opt",
                        "description": "opt1",
                        "type": 3,
                        "required": true
                    }
                ]
            }))
            .unwrap(),
        )
        .unwrap()
        .to_string();
        let actual = quote! {
            pub mod test {
                use serde::de::{SeqAccess, Visitor};
                use serde::Deserializer;
                use std::fmt;
                use std::fmt::Write;
                #[derive(serde :: Serialize, serde :: Deserialize, Debug)]
                pub struct Test {
                    pub name: String,
                    #[serde(deserialize_with = "parse_property")]
                    pub options: Options,
                }
                #[derive(serde :: Serialize, Debug, Default)]
                pub struct Options {
                    pub opt: String,
                }
                fn parse_property<'de, D>(deserializer: D) -> Result<Options, D::Error>
                where
                    D: Deserializer<'de>,
                {
                    #[derive(serde :: Serialize, serde :: Deserialize, Debug)]
                    #[serde(tag = "name", content = "value")]
                    enum Property {
                        #[serde(rename = "opt")]
                        Opt(String),
                    }
                    struct PropertyParser;
                    impl<'de> Visitor<'de> for PropertyParser {
                        type Value = Options;
                        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                            formatter.write_str("aaa")
                        }
                        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                            let mut prop = Options {
                                ..Default::default()
                            };
                            while let Some(tmp) = seq.next_element::<Property>()? {
                                match tmp {
                                    Property::Opt(v) => prop.opt = v,
                                }
                            }
                            Ok(prop)
                        }
                    }
                    deserializer.deserialize_any(PropertyParser {})
                }
            }
        }
        .to_string();
        assert_eq!(experimental, actual);
    }

    #[test]
    fn real_life() {
        let experimental = structify(
            &json!({
             "name": "ctf",
              "description": "placeholder",
              "options": [
                {
                  "type": 1,
                  "name": "play",
                  "description": "placeholder",
                  "options": [
                    {
                      "type": 3,
                      "name": "name",
                      "description": "placeholder",
                      "required": true
                    }
                  ]
                },
                {
                  "type": 1,
                  "name": "archive",
                  "description": "placeholder",
                  "options": [
                    {
                      "type": 7,
                      "name": "channel",
                      "description": "placeholder"
                    }
                  ]
                },
                {
                  "type": 1,
                  "name": "chall",
                  "description": "placeholder",
                  "options": [
                    {
                      "type": 3,
                      "name": "name",
                      "description": "placeholder",
                      "required": true
                    }
                  ]
                },
                {
                  "type": 1,
                  "name": "solve",
                  "description": "placeholder",
                  "options": [
                    {
                      "type": 3,
                      "name": "flag",
                      "description": "placeholder",
                      "required": true
                    },
                    {
                      "type": 7,
                      "name": "channel",
                      "description": "placeholder"
                    },
                    {
                      "type": 4,
                      "name": "points",
                      "description": "placeholder"
                    }
                  ]
                },
                {
                  "type": 2,
                  "name": "players",
                  "description": "placeholder",
                  "options": [
                    {
                      "type": 1,
                      "name": "add",
                      "description": "placeholder",
                      "options": [
                        {
                          "type": 9,
                          "name": "name",
                          "description": "placeholder",
                          "required": true
                        }
                      ]
                    },
                    {
                      "type": 1,
                      "name": "remove",
                      "description": "placeholder",
                      "options": [
                        {
                          "type": 9,
                          "name": "name",
                          "description": "placeholder",
                          "required": true
                        }
                      ]
                    }
                  ]
                }
              ]
            }
            )
            .to_string(),
        )
        .to_string();
        let actual = quote! {
            pub mod ctf {
                pub mod play {
                    use serde::de::{SeqAccess, Visitor};
                    use serde::Deserializer;
                    use std::fmt;
                    use std::fmt::Write;
                    #[derive(serde :: Serialize, serde :: Deserialize, Debug)]
                    pub struct Play {
                        pub name: String,
                        #[serde(deserialize_with = "parse_property")]
                        pub options: Options,
                    }
                    #[derive(serde :: Serialize, Debug, Default)]
                    pub struct Options {
                        pub name: String,
                    }
                    fn parse_property<'de, D>(deserializer: D) -> Result<Options, D::Error>
                    where
                        D: Deserializer<'de>,
                    {
                        #[derive(serde :: Serialize, serde :: Deserialize, Debug)]
                        #[serde(tag = "name", content = "value")]
                        enum Property {
                            #[serde(rename = "name")]
                            Name(String),
                        }
                        struct PropertyParser;
                        impl<'de> Visitor<'de> for PropertyParser {
                            type Value = Options;
                            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                                formatter.write_str("aaa")
                            }
                            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                                let mut prop = Options {
                                    ..Default::default()
                                };
                                while let Some(tmp) = seq.next_element::<Property>()? {
                                    match tmp {
                                        Property::Name(v) => prop.name = v,
                                    }
                                }
                                Ok(prop)
                            }
                        }
                        deserializer.deserialize_any(PropertyParser {})
                    }
                }
                pub mod archive {
                    use serde::de::{SeqAccess, Visitor};
                    use serde::Deserializer;
                    use std::fmt;
                    use std::fmt::Write;
                    #[derive(serde :: Serialize, serde :: Deserialize, Debug)]
                    pub struct Archive {
                        pub name: String,
                        #[serde(deserialize_with = "parse_property")]
                        pub options: Options,
                    }
                    #[derive(serde :: Serialize, Debug, Default)]
                    pub struct Options {
                        pub channel: String,
                    }
                    fn parse_property<'de, D>(deserializer: D) -> Result<Options, D::Error>
                    where
                        D: Deserializer<'de>,
                    {
                        #[derive(serde :: Serialize, serde :: Deserialize, Debug)]
                        #[serde(tag = "name", content = "value")]
                        enum Property {
                            #[serde(rename = "channel")]
                            Channel(String),
                        }
                        struct PropertyParser;
                        impl<'de> Visitor<'de> for PropertyParser {
                            type Value = Options;
                            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                                formatter.write_str("aaa")
                            }
                            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                                let mut prop = Options {
                                    ..Default::default()
                                };
                                while let Some(tmp) = seq.next_element::<Property>()? {
                                    match tmp {
                                        Property::Channel(v) => prop.channel = v,
                                    }
                                }
                                Ok(prop)
                            }
                        }
                        deserializer.deserialize_any(PropertyParser {})
                    }
                }
                pub mod chall {
                    use serde::de::{SeqAccess, Visitor};
                    use serde::Deserializer;
                    use std::fmt;
                    use std::fmt::Write;
                    #[derive(serde :: Serialize, serde :: Deserialize, Debug)]
                    pub struct Chall {
                        pub name: String,
                        #[serde(deserialize_with = "parse_property")]
                        pub options: Options,
                    }
                    #[derive(serde :: Serialize, Debug, Default)]
                    pub struct Options {
                        pub name: String,
                    }
                    fn parse_property<'de, D>(deserializer: D) -> Result<Options, D::Error>
                    where
                        D: Deserializer<'de>,
                    {
                        #[derive(serde :: Serialize, serde :: Deserialize, Debug)]
                        #[serde(tag = "name", content = "value")]
                        enum Property {
                            #[serde(rename = "name")]
                            Name(String),
                        }
                        struct PropertyParser;
                        impl<'de> Visitor<'de> for PropertyParser {
                            type Value = Options;
                            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                                formatter.write_str("aaa")
                            }
                            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                                let mut prop = Options {
                                    ..Default::default()
                                };
                                while let Some(tmp) = seq.next_element::<Property>()? {
                                    match tmp {
                                        Property::Name(v) => prop.name = v,
                                    }
                                }
                                Ok(prop)
                            }
                        }
                        deserializer.deserialize_any(PropertyParser {})
                    }
                }
                pub mod solve {
                    use serde::de::{SeqAccess, Visitor};
                    use serde::Deserializer;
                    use std::fmt;
                    use std::fmt::Write;
                    #[derive(serde :: Serialize, serde :: Deserialize, Debug)]
                    pub struct Solve {
                        pub name: String,
                        #[serde(deserialize_with = "parse_property")]
                        pub options: Options,
                    }
                    #[derive(serde :: Serialize, Debug, Default)]
                    pub struct Options {
                        pub flag: String,
                        pub channel: String,
                        pub points: u64,
                    }
                    fn parse_property<'de, D>(deserializer: D) -> Result<Options, D::Error>
                    where
                        D: Deserializer<'de>,
                    {
                        #[derive(serde :: Serialize, serde :: Deserialize, Debug)]
                        #[serde(tag = "name", content = "value")]
                        enum Property {
                            #[serde(rename = "flag")]
                            Flag(String),
                            #[serde(rename = "channel")]
                            Channel(String),
                            #[serde(rename = "points")]
                            Points(u64),
                        }
                        struct PropertyParser;
                        impl<'de> Visitor<'de> for PropertyParser {
                            type Value = Options;
                            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                                formatter.write_str("aaa")
                            }
                            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                                let mut prop = Options {
                                    ..Default::default()
                                };
                                while let Some(tmp) = seq.next_element::<Property>()? {
                                    match tmp {
                                        Property::Flag(v) => prop.flag = v,
                                        Property::Channel(v) => prop.channel = v,
                                        Property::Points(v) => prop.points = v,
                                    }
                                }
                                Ok(prop)
                            }
                        }
                        deserializer.deserialize_any(PropertyParser {})
                    }
                }
                pub mod cmd {
                    #[derive(serde :: Serialize, serde :: Deserialize, Debug)]
                    pub struct Ctf {
                        id: String,
                        name: String,
                        options: Vec<Options>,
                    }
                    #[derive(serde :: Serialize, serde :: Deserialize, Debug)]
                    #[serde(untagged)]
                    pub enum Options {
                        Play(crate::ctf::play::Play),
                        Archive(crate::ctf::archive::Archive),
                        Chall(crate::ctf::chall::Chall),
                        Solve(crate::ctf::solve::Solve),
                        Players(crate::ctf::players::cmd::Players),
                    }
                }
                pub mod players {
                    use serde::de::{SeqAccess, Visitor};
                    use serde::Deserializer;
                    use std::fmt;
                    use std::fmt::Write;
                    pub mod add {
                        use serde::de::{SeqAccess, Visitor};
                        use serde::Deserializer;
                        use std::fmt;
                        use std::fmt::Write;
                        #[derive(serde :: Serialize, serde :: Deserialize, Debug)]
                        pub struct Add {
                            pub name: String,
                            #[serde(deserialize_with = "parse_property")]
                            pub options: Options,
                        }
                        #[derive(serde :: Serialize, Debug, Default)]
                        pub struct Options {
                            pub name: String,
                        }
                        fn parse_property<'de, D>(deserializer: D) -> Result<Options, D::Error>
                        where
                            D: Deserializer<'de>,
                        {
                            #[derive(serde :: Serialize, serde :: Deserialize, Debug)]
                            #[serde(tag = "name", content = "value")]
                            enum Property {
                                #[serde(rename = "name")]
                                Name(String),
                            }
                            struct PropertyParser;
                            impl<'de> Visitor<'de> for PropertyParser {
                                type Value = Options;
                                fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                                    formatter.write_str("aaa")
                                }
                                fn visit_seq<A: SeqAccess<'de>>(
                                    self,
                                    mut seq: A,
                                ) -> Result<Self::Value, A::Error> {
                                    let mut prop = Options {
                                        ..Default::default()
                                    };
                                    while let Some(tmp) = seq.next_element::<Property>()? {
                                        match tmp {
                                            Property::Name(v) => prop.name = v,
                                        }
                                    }
                                    Ok(prop)
                                }
                            }
                            deserializer.deserialize_any(PropertyParser {})
                        }
                    }
                    pub mod remove {
                        use serde::de::{SeqAccess, Visitor};
                        use serde::Deserializer;
                        use std::fmt;
                        use std::fmt::Write;
                        #[derive(serde :: Serialize, serde :: Deserialize, Debug)]
                        pub struct Remove {
                            pub name: String,
                            #[serde(deserialize_with = "parse_property")]
                            pub options: Options,
                        }
                        #[derive(serde :: Serialize, Debug, Default)]
                        pub struct Options {
                            pub name: String,
                        }
                        fn parse_property<'de, D>(deserializer: D) -> Result<Options, D::Error>
                        where
                            D: Deserializer<'de>,
                        {
                            #[derive(serde :: Serialize, serde :: Deserialize, Debug)]
                            #[serde(tag = "name", content = "value")]
                            enum Property {
                                #[serde(rename = "name")]
                                Name(String),
                            }
                            struct PropertyParser;
                            impl<'de> Visitor<'de> for PropertyParser {
                                type Value = Options;
                                fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                                    formatter.write_str("aaa")
                                }
                                fn visit_seq<A: SeqAccess<'de>>(
                                    self,
                                    mut seq: A,
                                ) -> Result<Self::Value, A::Error> {
                                    let mut prop = Options {
                                        ..Default::default()
                                    };
                                    while let Some(tmp) = seq.next_element::<Property>()? {
                                        match tmp {
                                            Property::Name(v) => prop.name = v,
                                        }
                                    }
                                    Ok(prop)
                                }
                            }
                            deserializer.deserialize_any(PropertyParser {})
                        }
                    }
                    pub mod cmd {
                        #[derive(serde :: Serialize, serde :: Deserialize, Debug)]
                        #[serde(untagged)]
                        pub enum Players {
                            Add(crate::ctf::players::add::Add),
                            Remove(crate::ctf::players::remove::Remove),
                        }
                    }
                }
            }
        }
        .to_string();
        assert_eq!(experimental, actual);
    }

    #[test]
    fn deser_impl() {
        let arr = [
            CommandOption {
                r#type: 3,
                name: "Abc".to_string(),
                options: None,
            },
            CommandOption {
                r#type: 3,
                name: "Def".to_string(),
                options: None,
            },
            CommandOption {
                r#type: 4,
                name: "Ghi".to_string(),
                options: None,
            },
        ];
        let experimental = generate_deserialize_impl(&arr).to_string();
        let actual = quote! {
            fn parse_property<'de, D>(deserializer: D) -> Result<Options, D::Error>
            where
                D: Deserializer<'de>,
            {
                #[derive(serde::Serialize, serde::Deserialize, Debug)]
                #[serde(tag = "name", content = "value")]
                enum Property {
                    #[serde(rename = "abc")]
                    Abc(String),
                    #[serde(rename = "def")]
                    Def(String),
                    #[serde(rename = "ghi")]
                    Ghi(u64),
                }

                struct PropertyParser;
                impl<'de> Visitor<'de> for PropertyParser {
                    type Value = Options;

                    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                        formatter.write_str("aaa")
                    }

                    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                        let mut prop = Options {
                            ..Default::default()
                        };
                        while let Some(tmp) = seq.next_element::<Property>()? {
                            match tmp {
                                Property::Abc(v) => prop.abc = v,
                                Property::Def(v) => prop.def = v,
                                Property::Ghi(v) => prop.ghi = v,
                            }
                        }
                        Ok(prop)
                    }
                }
                deserializer.deserialize_any(PropertyParser {})
            }
        }.to_string();
        assert_eq!(experimental, actual)
    }
}