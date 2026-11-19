/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use lifeguard::cache::LibraryCache;
use lifeguard::find_sources::build_source_db;
use lifeguard::find_sources::make_source_map;
use lifeguard::output::LifeGuardAnalysis;
use lifeguard::runner::analyze_library;
use lifeguard::runner::process_source_map;
use lifeguard::source_map::AstResult;
use lifeguard::source_map::ModuleName;
use lifeguard::source_map::ModuleProvider;
use lifeguard::source_map::SourceMap;
use lifeguard::source_map::Sources;
use lifeguard::test_lib::has_lazy_eligible_dep;
use lifeguard::test_lib::populate_temp_dir;
use lifeguard::test_lib::test_options;

fn whole_program(files: &[(&str, &str)]) -> LifeGuardAnalysis {
    let dir = populate_temp_dir(files);
    let options = test_options();
    let (db, _) = build_source_db(dir.path(), None, options.python_version).unwrap();
    process_source_map(make_source_map(db), dir.path(), &options).unwrap()
}

/// The map action for the package root alone, as Buck shards it.
fn package_cache(files: &[(&str, &str)]) -> LibraryCache {
    let dir = populate_temp_dir(files);
    let options = test_options();
    let (db, _) = build_source_db(dir.path(), None, options.python_version).unwrap();
    let mut sources = make_source_map(db);
    sources
        .modules
        .retain(|name, _| name.as_str() == "requests");
    let facts = analyze_library(sources, dir.path(), &options).unwrap();
    LibraryCache::build(
        &facts.safety_map,
        &facts.import_graph,
        &facts.exports,
        &facts.side_effect_imports,
    )
}

fn requires(analysis: &LifeGuardAnalysis, importer: &str, dependency: &str) {
    assert!(
        has_lazy_eligible_dep(analysis, importer, dependency),
        "{importer} must load {dependency}",
    );
}

#[test]
fn bundled_override_keeps_effects_and_source_package_shape_separate() {
    let dir = populate_temp_dir(&[("requests.py", "import provider\nexec('x = 1')\n")]);
    let name = ModuleName::from_str("requests");
    let mut map = SourceMap::default();
    map.modules.insert(name, dir.path().join("requests.py"));
    let sources = Sources::new(map, dir.path().to_path_buf());
    assert!(sources.is_stub(&name));
    assert!(sources.overrides_source(&name));
    let AstResult::Ok(effects) = sources.parse(&name).unwrap() else {
        panic!("valid source must retain its effect override");
    };
    assert!(effects.is_stub() && effects.is_init && effects.ast.body.is_empty());
    let AstResult::Ok(topology) = sources.parse_topology(&name).unwrap() else {
        panic!("valid source topology must remain parseable");
    };
    assert!(!topology.is_stub() && !topology.is_init);
    assert_eq!(
        topology.ast.body.len(),
        2,
        "topology is the source's `import provider` and `exec` call"
    );
}

#[test]
fn bundled_override_does_not_hide_source_parse_or_read_failure() {
    let dir = populate_temp_dir(&[("broken.py", "def broken(\n")]);
    let name = ModuleName::from_str("requests");
    for file in ["broken.py", "missing.py"] {
        for topology_first in [false, true] {
            let mut map = SourceMap::default();
            map.modules.insert(name, dir.path().join(file));
            let sources = Sources::new(map, dir.path().to_path_buf());
            if topology_first {
                assert!(matches!(
                    sources.parse_topology(&name),
                    Some(AstResult::ParserError(_))
                ));
            }
            assert!(matches!(
                sources.parse(&name),
                Some(AstResult::ParserError(_))
            ));
            assert!(matches!(
                sources.parse_topology(&name),
                Some(AstResult::ParserError(_))
            ));
        }
    }
}

#[test]
fn bundled_requests_preserves_provider_edges_and_whole_program_guards() {
    let whole = whole_program(&[
        ("requests/__init__.py", "from .api import get\n"),
        (
            "requests/api.py",
            "from .sessions import Session\ndef get():\n    return Session()\n",
        ),
        (
            "requests/sessions.py",
            "from .auth import AuthBase\nclass Session:\n    pass\n",
        ),
        ("requests/auth.py", "class AuthBase:\n    pass\n"),
        (
            "consumer.py",
            "import requests\nclass Signer(requests.auth.AuthBase):\n    pass\n",
        ),
    ]);
    requires(&whole, "consumer", "requests.auth");
    requires(&whole, "requests", "requests.auth");
    requires(&whole, "requests.api", "requests.auth");
    requires(&whole, "requests.sessions", "requests.auth");
}

#[test]
fn bundled_package_map_keeps_source_edges_and_private_reexports() {
    let cache = package_cache(&[
        (
            "requests/__init__.py",
            "from .api import get\nfrom .__version__ import __version__\n",
        ),
        ("requests/api.py", "def get():\n    pass\n"),
        ("requests/__version__.py", "__version__ = '1'\n"),
    ]);
    let package = cache
        .modules
        .iter()
        .find(|module| module.name == ModuleName::from_str("requests"))
        .expect("the source-backed package must keep its cache row");
    let api = ModuleName::from_str("requests.api");
    assert!(
        package.imports.contains(&api) || package.missing_imports.contains(&api),
        "the source's `from .api import get` is an edge the stub lacks",
    );
    assert!(
        cache.exports.re_exports.iter().any(|export| {
            export.exported_module == package.name
                && export.exported_attr.as_str() == "__version__"
                && export.imported_module == ModuleName::from_str("requests.__version__")
        }),
        "the source's private re-export survives the stub",
    );
}
