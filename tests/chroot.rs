/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[cfg(test)]
mod tests {
    use lifeguard::output::LifeGuardAnalysis;
    use lifeguard::pyrefly::module_name::ModuleName;
    use lifeguard::test_lib::assert_paths_agree_sharded;
    use lifeguard::test_lib::run_lifeguard_analysis;

    fn loads_imports_eagerly(result: &LifeGuardAnalysis, module: &str) -> bool {
        result
            .output
            .load_imports_eagerly
            .contains(&ModuleName::from_str(module))
    }

    fn stays_lazy(result: &LifeGuardAnalysis) -> bool {
        result
            .output
            .lazy_eligible
            .contains_key(&ModuleName::from_str("tool.commands"))
    }

    fn modules_with(builder: &'static str) -> Vec<(&'static str, &'static str)> {
        vec![
            ("tool", ""),
            ("tool.builder", builder),
            ("tool.archives", "from tool import commands"),
            ("tool.commands", "import subprocess"),
        ]
    }

    /// After `os.chroot` the program's own files are outside the new root, so a deferred
    /// import first used there cannot load. Nothing in the program may defer.
    #[test]
    fn test_chroot_in_a_function_defers_nothing() {
        let builder = r#"
            import os
            from tool import archives

            def build(root):
                os.chroot(root)
                archives.commands.run()
        "#;
        let modules = modules_with(builder);
        let result = run_lifeguard_analysis(&modules);

        assert!(
            result.output.lazy_eligible.is_empty(),
            "a program that enters a chroot must not defer any import"
        );
        assert!(
            loads_imports_eagerly(&result, "tool.builder"),
            "the module calling os.chroot loads its imports eagerly"
        );
        assert_paths_agree_sharded(&modules);
    }

    #[test]
    fn test_chroot_imported_by_name_defers_nothing() {
        let builder = r#"
            from os import chroot
            from tool import archives

            def build(root):
                chroot(root)
        "#;
        let modules = modules_with(builder);
        let result = run_lifeguard_analysis(&modules);

        assert!(
            result.output.lazy_eligible.is_empty(),
            "`from os import chroot` enters a chroot too"
        );
        assert_paths_agree_sharded(&modules);
    }

    #[test]
    fn test_program_without_chroot_stays_lazy() {
        let builder = r#"
            import os
            from tool import archives

            def build(root):
                os.chdir(root)
        "#;
        let modules = modules_with(builder);
        let result = run_lifeguard_analysis(&modules);

        assert!(
            stays_lazy(&result),
            "without os.chroot, pure modules stay lazy-eligible"
        );
        assert!(!loads_imports_eagerly(&result, "tool.builder"));
    }

    #[test]
    fn test_chroot_through_a_module_alias_defers_nothing() {
        let builder = r#"
            import os as o
            from tool import archives

            def build(root):
                o.chroot(root)
        "#;
        let modules = modules_with(builder);
        let result = run_lifeguard_analysis(&modules);

        assert!(
            result.output.lazy_eligible.is_empty(),
            "`import os as o; o.chroot()` enters a chroot too"
        );
        assert_paths_agree_sharded(&modules);
    }

    #[test]
    fn test_locally_defined_chroot_stays_lazy() {
        let builder = r#"
            from tool import archives

            def chroot(root):
                return root

            def build(root):
                chroot(root)
        "#;
        let modules = modules_with(builder);
        let result = run_lifeguard_analysis(&modules);

        assert!(
            stays_lazy(&result),
            "a function of our own named chroot is not os.chroot"
        );
        assert!(!loads_imports_eagerly(&result, "tool.builder"));
        assert_paths_agree_sharded(&modules);
    }

    #[test]
    fn test_chroot_imported_from_another_module_stays_lazy() {
        let builder = r#"
            from tool import archives
            from tool.paths import chroot

            def build(root):
                chroot(root)
        "#;
        let mut modules = modules_with(builder);
        modules.push(("tool.paths", "def chroot(root):\n    return root"));
        let result = run_lifeguard_analysis(&modules);

        assert!(
            stays_lazy(&result),
            "only os.chroot and posix.chroot enter a chroot"
        );
        assert!(!loads_imports_eagerly(&result, "tool.builder"));
        assert_paths_agree_sharded(&modules);
    }

    #[test]
    fn test_chroot_through_an_assigned_alias_defers_nothing() {
        let builder = r#"
            import os
            from tool import archives

            enter = os.chroot

            def build(root):
                enter(root)
        "#;
        let modules = modules_with(builder);
        let result = run_lifeguard_analysis(&modules);

        assert!(
            result.output.lazy_eligible.is_empty(),
            "a module-level alias of os.chroot enters a chroot too"
        );
        assert_paths_agree_sharded(&modules);
    }

    #[test]
    fn test_chroot_through_a_local_alias_defers_nothing() {
        let builder = r#"
            import os
            from tool import archives

            def build(root):
                enter = os.chroot
                enter(root)
        "#;
        let modules = modules_with(builder);
        let result = run_lifeguard_analysis(&modules);

        assert!(
            result.output.lazy_eligible.is_empty(),
            "a local alias of os.chroot enters a chroot too"
        );
        assert_paths_agree_sharded(&modules);
    }
}
