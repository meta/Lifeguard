/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use lifeguard::source_map::AstResult;
use lifeguard::source_map::ModuleName;
use lifeguard::source_map::ModuleProvider;
use lifeguard::source_map::SourceMap;
use lifeguard::source_map::Sources;
use lifeguard::test_lib::populate_temp_dir;

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
