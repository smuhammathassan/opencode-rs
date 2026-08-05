/// From reference/packages/opencode/src/format/formatter.ts
///
/// Built-in formatter definitions. Each `enabled` check returns `Some(cmd)`
/// when the formatter applies, mirroring the reference's async
/// `Promise<string[] | false>`.
use std::collections::HashMap;
use std::sync::Arc;

use futures::FutureExt;

use crate::util::filesystem as Filesystem;
use crate::util::process::{self, RunOptions};
use crate::which::which;

pub struct Context {
    pub directory: String,
    pub worktree: String,
    pub experimental_oxfmt: bool,
}

pub type EnabledFn = Arc<
    dyn Fn(
            Context,
        )
            -> futures::future::BoxFuture<'static, Result<Option<Vec<String>>, anyhow::Error>>
        + Send
        + Sync,
>;

pub struct Info {
    pub name: &'static str,
    pub environment: Option<HashMap<String, String>>,
    pub extensions: Vec<&'static str>,
    pub enabled: EnabledFn,
}

fn bin(name: &str) -> Result<Option<Vec<String>>, anyhow::Error> {
    Ok(which(name).map(|m| vec![m.to_string_lossy().into_owned(), "$FILE".to_string()]))
}

async fn find_up_file(target: &str, context: &Context) -> Vec<String> {
    Filesystem::find_up(
        &[target.to_string()],
        &context.directory,
        Some(&context.worktree),
        false,
    )
    .await
}

fn enabled(
    f: fn(
        Context,
    ) -> futures::future::BoxFuture<'static, Result<Option<Vec<String>>, anyhow::Error>>,
) -> EnabledFn {
    Arc::new(f)
}

fn bun_env() -> HashMap<String, String> {
    HashMap::from([("BUN_BE_BUN".to_string(), "1".to_string())])
}

pub fn gofmt() -> Info {
    Info {
        name: "gofmt",
        environment: None,
        extensions: vec![".go"],
        enabled: enabled(|_ctx| {
            async move {
                match which("gofmt") {
                    Some(m) => Ok(Some(vec![
                        m.to_string_lossy().into_owned(),
                        "-w".into(),
                        "$FILE".into(),
                    ])),
                    None => Ok(None),
                }
            }
            .boxed()
        }),
    }
}

pub fn mix() -> Info {
    Info {
        name: "mix",
        environment: None,
        extensions: vec![".ex", ".exs", ".eex", ".heex", ".leex", ".neex", ".sface"],
        enabled: enabled(|_ctx| {
            async move {
                match which("mix") {
                    Some(m) => Ok(Some(vec![
                        m.to_string_lossy().into_owned(),
                        "format".into(),
                        "$FILE".into(),
                    ])),
                    None => Ok(None),
                }
            }
            .boxed()
        }),
    }
}

pub fn prettier() -> Info {
    Info {
        name: "prettier",
        environment: Some(bun_env()),
        extensions: vec![
            ".js", ".jsx", ".mjs", ".cjs", ".ts", ".tsx", ".mts", ".cts", ".html", ".htm", ".css",
            ".scss", ".sass", ".less", ".vue", ".svelte", ".json", ".jsonc", ".yaml", ".yml",
            ".toml", ".xml", ".md", ".mdx", ".graphql", ".gql",
        ],
        enabled: enabled(|ctx| {
            async move {
                for item in find_up_file("package.json", &ctx).await {
                    let json = match Filesystem::read_json(&item).await {
                        Ok(json) => json,
                        Err(_) => continue,
                    };
                    let has = |key: &str| {
                        json.get(key)
                            .and_then(|deps| deps.as_object())
                            .and_then(|deps| deps.get("prettier"))
                            .is_some()
                    };
                    if has("dependencies") || has("devDependencies") {
                        if let Some(bin) = crate::npm::which("prettier", None).await {
                            return Ok(Some(vec![bin, "--write".into(), "$FILE".into()]));
                        }
                    }
                }
                Ok(None)
            }
            .boxed()
        }),
    }
}

pub fn oxfmt() -> Info {
    Info {
        name: "oxfmt",
        environment: Some(bun_env()),
        extensions: vec![".js", ".jsx", ".mjs", ".cjs", ".ts", ".tsx", ".mts", ".cts"],
        enabled: enabled(|ctx| {
            async move {
                if !ctx.experimental_oxfmt {
                    return Ok(None);
                }
                for item in find_up_file("package.json", &ctx).await {
                    let json = match Filesystem::read_json(&item).await {
                        Ok(json) => json,
                        Err(_) => continue,
                    };
                    let has = |key: &str| {
                        json.get(key)
                            .and_then(|deps| deps.as_object())
                            .and_then(|deps| deps.get("oxfmt"))
                            .is_some()
                    };
                    if has("dependencies") || has("devDependencies") {
                        if let Some(bin) = crate::npm::which("oxfmt", None).await {
                            return Ok(Some(vec![bin, "$FILE".into()]));
                        }
                    }
                }
                Ok(None)
            }
            .boxed()
        }),
    }
}

pub fn biome() -> Info {
    Info {
        name: "biome",
        environment: Some(bun_env()),
        extensions: vec![
            ".js", ".jsx", ".mjs", ".cjs", ".ts", ".tsx", ".mts", ".cts", ".html", ".htm", ".css",
            ".scss", ".sass", ".less", ".vue", ".svelte", ".json", ".jsonc", ".yaml", ".yml",
            ".toml", ".xml", ".md", ".mdx", ".graphql", ".gql",
        ],
        enabled: enabled(|ctx| {
            async move {
                for config in ["biome.json", "biome.jsonc"] {
                    let found = find_up_file(config, &ctx).await;
                    if !found.is_empty() {
                        if let Some(bin) = crate::npm::which("@biomejs/biome", None).await {
                            return Ok(Some(vec![
                                bin,
                                "format".into(),
                                "--write".into(),
                                "$FILE".into(),
                            ]));
                        }
                    }
                }
                Ok(None)
            }
            .boxed()
        }),
    }
}

pub fn zig() -> Info {
    Info {
        name: "zig",
        environment: None,
        extensions: vec![".zig", ".zon"],
        enabled: enabled(|_ctx| {
            async move {
                match which("zig") {
                    Some(m) => Ok(Some(vec![
                        m.to_string_lossy().into_owned(),
                        "fmt".into(),
                        "$FILE".into(),
                    ])),
                    None => Ok(None),
                }
            }
            .boxed()
        }),
    }
}

pub fn clang() -> Info {
    Info {
        name: "clang-format",
        environment: None,
        extensions: vec![
            ".c", ".cc", ".cpp", ".cxx", ".c++", ".h", ".hh", ".hpp", ".hxx", ".h++", ".ino", ".C",
            ".H",
        ],
        enabled: enabled(|ctx| {
            async move {
                let items = find_up_file(".clang-format", &ctx).await;
                if !items.is_empty() {
                    if let Some(m) = which("clang-format") {
                        return Ok(Some(vec![
                            m.to_string_lossy().into_owned(),
                            "-i".into(),
                            "$FILE".into(),
                        ]));
                    }
                }
                Ok(None)
            }
            .boxed()
        }),
    }
}

pub fn ktlint() -> Info {
    Info {
        name: "ktlint",
        environment: None,
        extensions: vec![".kt", ".kts"],
        enabled: enabled(|_ctx| {
            async move {
                match which("ktlint") {
                    Some(m) => Ok(Some(vec![
                        m.to_string_lossy().into_owned(),
                        "-F".into(),
                        "$FILE".into(),
                    ])),
                    None => Ok(None),
                }
            }
            .boxed()
        }),
    }
}

pub fn ruff() -> Info {
    Info {
        name: "ruff",
        environment: None,
        extensions: vec![".py", ".pyi"],
        enabled: enabled(|ctx| {
            async move {
                if which("ruff").is_none() {
                    return Ok(None);
                }
                for config in ["pyproject.toml", "ruff.toml", ".ruff.toml"] {
                    let found = find_up_file(config, &ctx).await;
                    if !found.is_empty() {
                        if config == "pyproject.toml" {
                            if let Ok(content) = Filesystem::read_text(&found[0]).await {
                                if content.contains("[tool.ruff]") {
                                    return Ok(Some(vec![
                                        "ruff".into(),
                                        "format".into(),
                                        "$FILE".into(),
                                    ]));
                                }
                            }
                        } else {
                            return Ok(Some(vec!["ruff".into(), "format".into(), "$FILE".into()]));
                        }
                    }
                }
                for dep in ["requirements.txt", "pyproject.toml", "Pipfile"] {
                    let found = find_up_file(dep, &ctx).await;
                    if !found.is_empty() {
                        if let Ok(content) = Filesystem::read_text(&found[0]).await {
                            if content.contains("ruff") {
                                return Ok(Some(vec![
                                    "ruff".into(),
                                    "format".into(),
                                    "$FILE".into(),
                                ]));
                            }
                        }
                    }
                }
                Ok(None)
            }
            .boxed()
        }),
    }
}

pub fn rlang() -> Info {
    Info {
        name: "air",
        environment: None,
        extensions: vec![".R"],
        enabled: enabled(|_ctx| {
            async move {
                let Some(air) = which("air") else {
                    return Ok(None);
                };
                let air = air.to_string_lossy().into_owned();
                let out = process::text(
                    &[air.clone(), "--help".to_string()],
                    &RunOptions {
                        nothrow: true,
                        ..Default::default()
                    },
                )
                .await;
                let Ok(out) = out else { return Ok(None) };
                let first_line = out.text.split('\n').next().unwrap_or("");
                let has_r = first_line.contains("R language");
                let has_formatter = first_line.contains("formatter");
                if out.code == 0 && has_r && has_formatter {
                    Ok(Some(vec![air, "format".into(), "$FILE".into()]))
                } else {
                    Ok(None)
                }
            }
            .boxed()
        }),
    }
}

pub fn uvformat() -> Info {
    Info {
        name: "uv",
        environment: None,
        extensions: vec![".py", ".pyi"],
        enabled: enabled(|ctx| {
            async move {
                if (ruff().enabled)(ctx).await?.is_some() {
                    return Ok(None);
                }
                let Some(uv) = which("uv") else {
                    return Ok(None);
                };
                let uv = uv.to_string_lossy().into_owned();
                let out = process::run(
                    &[uv.clone(), "format".to_string(), "--help".to_string()],
                    &RunOptions {
                        nothrow: true,
                        ..Default::default()
                    },
                )
                .await;
                if out.map(|o| o.code).unwrap_or(1) == 0 {
                    Ok(Some(vec![uv, "format".into(), "--".into(), "$FILE".into()]))
                } else {
                    Ok(None)
                }
            }
            .boxed()
        }),
    }
}

pub fn rubocop() -> Info {
    Info {
        name: "rubocop",
        environment: None,
        extensions: vec![".rb", ".rake", ".gemspec", ".ru"],
        enabled: enabled(|_ctx| {
            async move {
                match which("rubocop") {
                    Some(m) => Ok(Some(vec![
                        m.to_string_lossy().into_owned(),
                        "--autocorrect".into(),
                        "$FILE".into(),
                    ])),
                    None => Ok(None),
                }
            }
            .boxed()
        }),
    }
}

pub fn standardrb() -> Info {
    Info {
        name: "standardrb",
        environment: None,
        extensions: vec![".rb", ".rake", ".gemspec", ".ru"],
        enabled: enabled(|_ctx| {
            async move {
                match which("standardrb") {
                    Some(m) => Ok(Some(vec![
                        m.to_string_lossy().into_owned(),
                        "--fix".into(),
                        "$FILE".into(),
                    ])),
                    None => Ok(None),
                }
            }
            .boxed()
        }),
    }
}

pub fn htmlbeautifier() -> Info {
    Info {
        name: "htmlbeautifier",
        environment: None,
        extensions: vec![".erb", ".html.erb"],
        enabled: enabled(|_ctx| async move { bin("htmlbeautifier") }.boxed()),
    }
}

pub fn dart() -> Info {
    Info {
        name: "dart",
        environment: None,
        extensions: vec![".dart"],
        enabled: enabled(|_ctx| {
            async move {
                match which("dart") {
                    Some(m) => Ok(Some(vec![
                        m.to_string_lossy().into_owned(),
                        "format".into(),
                        "$FILE".into(),
                    ])),
                    None => Ok(None),
                }
            }
            .boxed()
        }),
    }
}

pub fn ocamlformat() -> Info {
    Info {
        name: "ocamlformat",
        environment: None,
        extensions: vec![".ml", ".mli"],
        enabled: enabled(|ctx| {
            async move {
                if which("ocamlformat").is_none() {
                    return Ok(None);
                }
                let items = find_up_file(".ocamlformat", &ctx).await;
                if items.is_empty() {
                    return Ok(None);
                }
                Ok(Some(vec![
                    "ocamlformat".into(),
                    "-i".into(),
                    "$FILE".into(),
                ]))
            }
            .boxed()
        }),
    }
}

pub fn terraform() -> Info {
    Info {
        name: "terraform",
        environment: None,
        extensions: vec![".tf", ".tfvars"],
        enabled: enabled(|_ctx| {
            async move {
                match which("terraform") {
                    Some(m) => Ok(Some(vec![
                        m.to_string_lossy().into_owned(),
                        "fmt".into(),
                        "$FILE".into(),
                    ])),
                    None => Ok(None),
                }
            }
            .boxed()
        }),
    }
}

pub fn latexindent() -> Info {
    Info {
        name: "latexindent",
        environment: None,
        extensions: vec![".tex"],
        enabled: enabled(|_ctx| {
            async move {
                match which("latexindent") {
                    Some(m) => Ok(Some(vec![
                        m.to_string_lossy().into_owned(),
                        "-w".into(),
                        "-s".into(),
                        "$FILE".into(),
                    ])),
                    None => Ok(None),
                }
            }
            .boxed()
        }),
    }
}

pub fn gleam() -> Info {
    Info {
        name: "gleam",
        environment: None,
        extensions: vec![".gleam"],
        enabled: enabled(|_ctx| {
            async move {
                match which("gleam") {
                    Some(m) => Ok(Some(vec![
                        m.to_string_lossy().into_owned(),
                        "format".into(),
                        "$FILE".into(),
                    ])),
                    None => Ok(None),
                }
            }
            .boxed()
        }),
    }
}

pub fn shfmt() -> Info {
    Info {
        name: "shfmt",
        environment: None,
        extensions: vec![".sh", ".bash"],
        enabled: enabled(|_ctx| {
            async move {
                match which("shfmt") {
                    Some(m) => Ok(Some(vec![
                        m.to_string_lossy().into_owned(),
                        "-w".into(),
                        "$FILE".into(),
                    ])),
                    None => Ok(None),
                }
            }
            .boxed()
        }),
    }
}

pub fn nixfmt() -> Info {
    Info {
        name: "nixfmt",
        environment: None,
        extensions: vec![".nix"],
        enabled: enabled(|_ctx| async move { bin("nixfmt") }.boxed()),
    }
}

pub fn rustfmt() -> Info {
    Info {
        name: "rustfmt",
        environment: None,
        extensions: vec![".rs"],
        enabled: enabled(|_ctx| async move { bin("rustfmt") }.boxed()),
    }
}

pub fn pint() -> Info {
    Info {
        name: "pint",
        environment: None,
        extensions: vec![".php"],
        enabled: enabled(|ctx| {
            async move {
                for item in find_up_file("composer.json", &ctx).await {
                    let json = match Filesystem::read_json(&item).await {
                        Ok(json) => json,
                        Err(_) => continue,
                    };
                    let has = |key: &str| {
                        json.get(key)
                            .and_then(|deps| deps.as_object())
                            .and_then(|deps| deps.get("laravel/pint"))
                            .is_some()
                    };
                    if has("require") || has("require-dev") {
                        return Ok(Some(vec!["./vendor/bin/pint".into(), "$FILE".into()]));
                    }
                }
                Ok(None)
            }
            .boxed()
        }),
    }
}

pub fn ormolu() -> Info {
    Info {
        name: "ormolu",
        environment: None,
        extensions: vec![".hs"],
        enabled: enabled(|_ctx| {
            async move {
                match which("ormolu") {
                    Some(m) => Ok(Some(vec![
                        m.to_string_lossy().into_owned(),
                        "-i".into(),
                        "$FILE".into(),
                    ])),
                    None => Ok(None),
                }
            }
            .boxed()
        }),
    }
}

pub fn cljfmt() -> Info {
    Info {
        name: "cljfmt",
        environment: None,
        extensions: vec![".clj", ".cljs", ".cljc", ".edn"],
        enabled: enabled(|_ctx| {
            async move {
                match which("cljfmt") {
                    Some(m) => Ok(Some(vec![
                        m.to_string_lossy().into_owned(),
                        "fix".into(),
                        "--quiet".into(),
                        "$FILE".into(),
                    ])),
                    None => Ok(None),
                }
            }
            .boxed()
        }),
    }
}

pub fn dfmt() -> Info {
    Info {
        name: "dfmt",
        environment: None,
        extensions: vec![".d"],
        enabled: enabled(|_ctx| {
            async move {
                match which("dfmt") {
                    Some(m) => Ok(Some(vec![
                        m.to_string_lossy().into_owned(),
                        "-i".into(),
                        "$FILE".into(),
                    ])),
                    None => Ok(None),
                }
            }
            .boxed()
        }),
    }
}

/// All built-in formatters in source order, mirroring `Object.values(Formatter)`.
pub fn all() -> Vec<Info> {
    vec![
        gofmt(),
        mix(),
        prettier(),
        oxfmt(),
        biome(),
        zig(),
        clang(),
        ktlint(),
        ruff(),
        rlang(),
        uvformat(),
        rubocop(),
        standardrb(),
        htmlbeautifier(),
        dart(),
        ocamlformat(),
        terraform(),
        latexindent(),
        gleam(),
        shfmt(),
        nixfmt(),
        rustfmt(),
        pint(),
        ormolu(),
        cljfmt(),
        dfmt(),
    ]
}

pub fn by_name(name: &str) -> Option<Info> {
    all().into_iter().find(|info| info.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(dir: &str) -> Context {
        Context {
            directory: dir.to_string(),
            worktree: dir.to_string(),
            experimental_oxfmt: false,
        }
    }

    #[test]
    fn all_formatters_have_distinct_names() {
        let all = all();
        let mut names: Vec<&str> = all.iter().map(|info| info.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), all.len());
    }

    #[test]
    fn by_name_lookup() {
        assert_eq!(by_name("gofmt").unwrap().name, "gofmt");
        assert!(by_name("nonexistent").is_none());
    }

    #[tokio::test]
    async fn gofmt_disabled_without_binary() {
        if which("gofmt").is_some() {
            return;
        }
        let result = (gofmt().enabled)(ctx("/tmp")).await.unwrap();
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn pint_enabled_when_composer_declares_it() {
        let dir = std::env::temp_dir().join(format!("oc-util-fmt-pint-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("composer.json"),
            r#"{"require": {"laravel/pint": "^1.0"}}"#,
        )
        .unwrap();
        let result = (pint().enabled)(ctx(dir.to_str().unwrap())).await.unwrap();
        assert_eq!(
            result,
            Some(vec!["./vendor/bin/pint".to_string(), "$FILE".to_string()])
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn pint_disabled_without_composer_dependency() {
        let dir = std::env::temp_dir().join(format!("oc-util-fmt-pint2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("composer.json"), r#"{"require": {"php": ">=8"}}"#).unwrap();
        let result = (pint().enabled)(ctx(dir.to_str().unwrap())).await.unwrap();
        assert_eq!(result, None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
