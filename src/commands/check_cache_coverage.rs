/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Check that the per-library caches a real incremental build deploys cover
//! every module in the same target's complete source DB.
//!
//! `compare-paths` maps the whole source DB as a single library, so both of its
//! paths see every module by construction and it cannot detect a library whose
//! cache the build never produced. That is a live failure mode: a macro that
//! skips the incremental attributes leaves its libraries without a
//! `[lazy-import-cache]`, their modules absent from the reduce, and the
//! resulting policy quietly different from whole-program.

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use pyrefly_python::module_name::ModuleName;
use rayon::prelude::*;

use crate::cache::LibraryCache;
use crate::hasher::AHashSet;
use crate::source_map;
use crate::tracing::time;

#[derive(Parser)]
pub struct CheckCacheCoverageArgs {
    /// Path to the target's complete source DB (its `dbg-db.json`).
    pub db_path: PathBuf,

    /// Manifest listing the per-library cache paths the build deploys, one per
    /// line. `scripts/lazy_imports_caches.bxl` produces this set for a target.
    #[arg(long = "cache-manifest")]
    pub cache_manifest: PathBuf,

    /// Fail (exit code 1) when more than this many source-DB modules have no
    /// covering cache. Defaults to 0 (require complete coverage).
    #[arg(long = "max-uncovered", default_value_t = 0)]
    pub max_uncovered: usize,

    /// Max example module names to print.
    #[arg(long = "examples", default_value_t = 20)]
    pub examples: usize,
}

pub fn run(args: CheckCacheCoverageArgs) -> Result<()> {
    let source_map = time("Loading source db", || {
        source_map::load_source_map(&args.db_path)
    })?;

    let mut cache_paths: Vec<PathBuf> = std::fs::read_to_string(&args.cache_manifest)?
        .lines()
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect();
    let mut seen = AHashSet::default();
    cache_paths.retain(|path| seen.insert(path.clone()));
    anyhow::ensure!(!cache_paths.is_empty(), "no cache paths provided");

    let covered: AHashSet<ModuleName> = time("Loading caches", || {
        cache_paths
            .par_iter()
            .map(|p| LibraryCache::read_from_file(p))
            .collect::<Result<Vec<_>>>()
    })?
    .iter()
    .flat_map(|cache| cache.modules.iter().map(|m| m.name))
    .collect();

    let mut uncovered: Vec<ModuleName> = source_map
        .modules
        .keys()
        .filter(|name| !covered.contains(name))
        .copied()
        .collect();
    uncovered.sort_by(|a, b| a.as_str().cmp(b.as_str()));

    println!("=== Lifeguard per-library cache coverage ===");
    println!("DB: {}", args.db_path.display());
    println!(
        "caches: {}, modules covered: {}, modules in source DB: {}",
        cache_paths.len(),
        covered.len(),
        source_map.modules.len(),
    );
    println!("--- uncovered modules ---");
    for name in uncovered.iter().take(args.examples) {
        println!("  {}", name.as_str());
    }
    if uncovered.len() > args.examples {
        println!("  ... and {} more", uncovered.len() - args.examples);
    }
    println!("--- summary ---");
    println!("total uncovered modules: {}", uncovered.len());
    println!("budget (max-uncovered): {}", args.max_uncovered);

    if uncovered.len() > args.max_uncovered {
        anyhow::bail!(
            "{} source-DB modules have no covering library cache, over the budget of {}",
            uncovered.len(),
            args.max_uncovered,
        );
    }
    println!("OK: coverage within budget.");
    Ok(())
}
