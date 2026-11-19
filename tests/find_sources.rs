/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::collections::BTreeSet;

    use lifeguard::find_sources::build_source_db;
    use lifeguard::runner::default_python_version;
    use lifeguard::test_lib::populate_temp_dir;

    fn keys(build_map: &BTreeMap<String, String>) -> BTreeSet<&str> {
        build_map.keys().map(|s| s.as_str()).collect()
    }

    #[test]
    fn test_seeds_py_files_from_input_dir() {
        let tmp = populate_temp_dir(&[("a.py", ""), ("pkg/__init__.py", ""), ("pkg/b.py", "")]);

        let (build_map, seed_count) =
            build_source_db(tmp.path(), None, default_python_version()).unwrap();
        assert_eq!(seed_count, 3, "all three .py files are seeded");
        assert_eq!(
            keys(&build_map),
            BTreeSet::from(["a.py", "pkg/__init__.py", "pkg/b.py"]),
        );
    }

    #[test]
    fn test_skips_non_identifier_names() {
        let tmp = populate_temp_dir(&[
            ("good.py", ""),
            // Dir whose name is not a valid identifier: skipped wholesale.
            (".venv/bad.py", ""),
            // File whose stem is not a valid identifier: skipped.
            ("2024-migration.py", ""),
        ]);

        let (build_map, _) = build_source_db(tmp.path(), None, default_python_version()).unwrap();
        assert_eq!(keys(&build_map), BTreeSet::from(["good.py"]));
    }

    #[test]
    fn test_follows_imports_into_site_packages() {
        let tmp = populate_temp_dir(&[
            ("proj/main.py", "import foo\n"),
            ("sp/foo/__init__.py", ""),
            // Unreachable from main.py's imports — must not be pulled in.
            ("sp/foo/helper.py", ""),
            ("sp/unused/__init__.py", ""),
        ]);
        let proj = tmp.path().join("proj");
        let sp = tmp.path().join("sp");

        let (build_map, seed_count) =
            build_source_db(&proj, Some(&sp), default_python_version()).unwrap();
        assert_eq!(seed_count, 1, "only main.py is seeded from the project");
        assert_eq!(
            keys(&build_map),
            BTreeSet::from(["main.py", "foo/__init__.py"]),
        );
    }

    #[test]
    fn test_follows_imports_nested_in_blocks_and_definitions() {
        let main = r#"
try:
    import in_try
except ImportError:
    import in_handler
if True:
    from in_if import name
else:
    import in_else
with open(__file__):
    import in_with
for _ in ():
    import in_for

def f():
    import in_def

class C:
    import in_class
"#;
        let tmp = populate_temp_dir(&[
            ("proj/main.py", main),
            ("proj/uses_pkg.py", "if True:\n    import pkg\n"),
            ("sp/in_try.py", ""),
            ("sp/in_handler.py", ""),
            ("sp/in_if.py", ""),
            ("sp/in_else.py", ""),
            ("sp/in_with.py", ""),
            ("sp/in_for.py", ""),
            ("sp/in_def.py", ""),
            ("sp/in_class.py", ""),
            ("sp/pkg/__init__.py", "import pkg.outer\n"),
            ("sp/pkg/outer.py", "def load():\n    from . import inner\n"),
            ("sp/pkg/inner.py", ""),
            ("sp/unused.py", ""),
        ]);
        let proj = tmp.path().join("proj");
        let sp = tmp.path().join("sp");

        let (build_map, _) = build_source_db(&proj, Some(&sp), default_python_version()).unwrap();
        assert_eq!(
            keys(&build_map),
            BTreeSet::from([
                "main.py",
                "uses_pkg.py",
                "in_try.py",
                "in_handler.py",
                "in_if.py",
                "in_else.py",
                "in_with.py",
                "in_for.py",
                "in_def.py",
                "in_class.py",
                "pkg/__init__.py",
                "pkg/outer.py",
                "pkg/inner.py",
            ]),
        );
    }

    #[test]
    fn test_follows_relative_imports_in_site_packages() {
        // pkg/__init__.py uses a relative import to pull in its sibling.
        // Without package-aware relative-import resolution, `helper` would
        // never make it into the build map.
        let tmp = populate_temp_dir(&[
            ("proj/main.py", "import pkg\n"),
            ("sp/pkg/__init__.py", "from . import helper\n"),
            ("sp/pkg/helper.py", ""),
        ]);
        let proj = tmp.path().join("proj");
        let sp = tmp.path().join("sp");

        let (build_map, _) = build_source_db(&proj, Some(&sp), default_python_version()).unwrap();
        assert_eq!(
            keys(&build_map),
            BTreeSet::from(["main.py", "pkg/__init__.py", "pkg/helper.py"]),
        );
    }
}
