/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use pyrefly_python::module_name::ModuleName;

use crate::traits::ModuleNameExt;

/// The module a dotted name is defined in, and the name within it: the longest
/// prefix `is_module` accepts, paired with the remainder.
///
/// Yields `None` for a name with no dot, or with no prefix satisfying is_module.
pub(crate) fn enclosing_module<'a>(
    name: &'a str,
    is_module: impl Fn(&ModuleName) -> bool,
) -> Option<(ModuleName, &'a str)> {
    ModuleName::from_str(name)
        .iter_parents()
        .find(|(parent, _)| is_module(parent))
        .map(|(module, dot_pos)| (module, &name[dot_pos + 1..]))
}

/// As [`enclosing_module`], but testing candidate ancestors as string slices.
///
/// `ModuleName::from_str` interns, so probing with it pays the global interner for
/// every ancestor tried, and leaves a permanent entry behind for each one that is
/// not a module. A caller splitting many names against one module set should build
/// a `&str -> ModuleName` view of that set once and probe it instead; nothing is interned.
pub(crate) fn enclosing_module_str<'a>(
    name: &'a str,
    module_of: impl Fn(&str) -> Option<ModuleName>,
) -> Option<(ModuleName, &'a str)> {
    let mut end = name.len();
    while let Some(pos) = name[..end].rfind('.') {
        end = pos;
        if let Some(module) = module_of(&name[..pos]) {
            return Some((module, &name[pos + 1..]));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enclosing_module_lookups_split_at_the_longest_module_prefix() {
        let modules = ["pkg", "pkg.mod", "paqueté", "paqueté.módulo"];
        let cases = [
            ("pkg.mod.Class.method", Some(("pkg.mod", "Class.method"))),
            (
                "pkg.missing.Class.method",
                Some(("pkg", "missing.Class.method")),
            ),
            ("pkg.mod", Some(("pkg", "mod"))),
            ("pkg", None),
            ("pkg_extra.mod.member", None),
            ("unrelated.mod.member", None),
            ("", None),
            (
                "paqueté.módulo.Cláss.方法",
                Some(("paqueté.módulo", "Cláss.方法")),
            ),
            ("paqueté.Cláss.方法", Some(("paqueté", "Cláss.方法"))),
        ];

        for (name, expected) in cases {
            let expected = expected.map(|(module, suffix)| (ModuleName::from_str(module), suffix));
            assert_eq!(
                enclosing_module(name, |module| modules.contains(&module.as_str())),
                expected,
                "interned lookup: {name}",
            );
            assert_eq!(
                enclosing_module_str(name, |prefix| {
                    modules
                        .iter()
                        .find(|&&module| module == prefix)
                        .map(|module| ModuleName::from_str(module))
                }),
                expected,
                "string lookup: {name}",
            );
        }
    }
}
