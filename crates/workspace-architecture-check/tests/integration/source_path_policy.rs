//! Inspect Rust tokens so comments, literals, cfg_attr and macro groups stay distinct.
use super::{CargoPackage, PolicyResult, normalize_path};
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use syn::{Expr, Lit, Meta, Token, parse::Parser, punctuated::Punctuated};

const EXCEPTIONS: [(&str, &str); 2] = [
    (
        "crates/collaboration-central-tokio/tests/integration/binding_attachment.rs",
        "crates/collaboration-central-cloudflare/src/binding_attachment.rs",
    ),
    (
        "crates/collaboration-central-tokio/tests/integration/object_identity.rs",
        "crates/collaboration-central-cloudflare/src/object_location.rs",
    ),
];

pub(super) fn check_source_paths(root: &Path, packages: &[&CargoPackage]) -> PolicyResult {
    let mut observed_exceptions = BTreeSet::new();
    let root = root.canonicalize()?;
    for package in packages {
        let home = package
            .manifest_path
            .parent()
            .ok_or("package home missing")?
            .canonicalize()?;
        let mut sources = Vec::new();
        collect_rust_sources(&home, &home, &mut sources)?;
        sources.sort();
        for source in sources {
            let tokens: TokenStream = fs::read_to_string(&source)?.parse().map_err(|error| {
                format!("{}: Rust token scan failed: {error}", source.display())
            })?;
            for declared in declared_paths(tokens)? {
                let parent = source.parent().ok_or("source parent missing")?;
                let target = normalize_path(&parent.join(declared));
                // Local inline-module paths need Rust's virtual module base. They
                // cannot cross this package boundary and are not this check's job.
                if target.starts_with(&home) {
                    continue;
                }
                let source_relative = source.strip_prefix(&root)?;
                let target_relative = target.strip_prefix(&root).map_err(|_| {
                    format!(
                        "{} includes outside workspace: {}",
                        source.display(),
                        target.display()
                    )
                })?;
                let exception = EXCEPTIONS.iter().position(|(from, to)| {
                    source_relative == Path::new(from) && target_relative == Path::new(to)
                });
                let Some(exception) = exception else {
                    return Err(format!(
                        "cross-crate source path forbidden: {} -> {}",
                        source_relative.display(),
                        target_relative.display()
                    )
                    .into());
                };
                if !target.is_file() || !source.is_file() {
                    return Err(format!("stale source-path exception: {}", target.display()).into());
                }
                // Canonical targets also reject symlink escapes.
                if target.canonicalize()? != target {
                    return Err(format!(
                        "source-path exception target moved: {}",
                        target.display()
                    )
                    .into());
                }
                observed_exceptions.insert(exception);
                eprintln!(
                    "allowed source-path exception: {} -> {}",
                    source_relative.display(),
                    target_relative.display()
                );
            }
        }
    }
    for (index, (source, target)) in EXCEPTIONS.iter().enumerate() {
        if !observed_exceptions.contains(&index)
            || !root.join(source).is_file()
            || !root.join(target).is_file()
        {
            return Err(format!("stale source-path exception: {source} -> {target}").into());
        }
    }
    Ok(())
}

fn collect_rust_sources(home: &Path, directory: &Path, files: &mut Vec<PathBuf>) -> PolicyResult {
    if directory != home && directory.join("Cargo.toml").is_file() {
        return Ok(());
    }
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() {
            if !matches!(
                entry.file_name().to_str(),
                Some("target" | "node_modules" | ".git" | "tmp")
            ) {
                collect_rust_sources(home, &path, files)?;
            }
        } else if file_type.is_file() && path.extension().is_some_and(|value| value == "rs") {
            files.push(path);
        }
    }
    Ok(())
}

pub(super) fn declared_paths(
    tokens: TokenStream,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut paths = Vec::new();
    let mut tokens = tokens.into_iter().peekable();
    while let Some(token) = tokens.next() {
        match token {
            TokenTree::Punct(pound) if pound.as_char() == '#' => {
                if matches!(tokens.peek(), Some(TokenTree::Punct(mark)) if mark.as_char() == '!') {
                    tokens.next();
                }
                if let Some(TokenTree::Group(group)) = tokens.peek()
                    && group.delimiter() == Delimiter::Bracket
                {
                    let group = group.clone();
                    tokens.next();
                    let first = group.stream().into_iter().next();
                    if matches!(first, Some(TokenTree::Ident(name)) if name == "path" || name == "cfg_attr")
                    {
                        collect_meta_paths(syn::parse2(group.stream())?, &mut paths)?;
                    }
                }
            }
            TokenTree::Group(group) => paths.extend(declared_paths(group.stream())?),
            _ => {}
        }
    }
    Ok(paths)
}

fn collect_meta_paths(meta: Meta, paths: &mut Vec<String>) -> PolicyResult {
    match meta {
        Meta::NameValue(value) if value.path.is_ident("path") => {
            if let Expr::Lit(literal) = value.value
                && let Lit::Str(path) = literal.lit
            {
                paths.push(path.value());
            } else {
                return Err("source path attribute must contain a literal path".into());
            }
        }
        Meta::List(list) if list.path.is_ident("cfg_attr") => {
            let nested = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(list.tokens)?;
            for meta in nested.into_iter().skip(1) {
                collect_meta_paths(meta, paths)?;
            }
        }
        _ => {}
    }
    Ok(())
}
