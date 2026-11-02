/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::path::PathBuf;

    use anyhow::Result;
    use lifeguard::module_parser::ParsedModule;
    use lifeguard::module_parser::read_and_parse_source;
    use lifeguard::pyrefly::module_name::ModuleName;
    use tempfile::NamedTempFile;

    fn parse(code: &[u8]) -> Result<ParsedModule> {
        let mut temp_file = NamedTempFile::with_suffix(".py")?;
        temp_file.write_all(code)?;
        temp_file.flush()?;

        let path = PathBuf::from(temp_file.path());
        let module_name = ModuleName::from_str("test");
        read_and_parse_source(&path, module_name, false)
    }

    fn check_string(code: &[u8], expected: &str) {
        let module = parse(code).unwrap();
        assert_eq!(module.first_syntax_error(), None);
        assert_eq!(module.ast.body.len(), 1);
        let assignment = module.ast.body[0].as_assign_stmt().unwrap();
        let value = assignment.value.as_string_literal_expr().unwrap();
        assert_eq!(value.value.to_str(), expected);
    }

    #[test]
    fn test_latin1_encoding() {
        // The byte 0xa6 (¦ broken bar) is valid in Latin-1 but invalid in UTF-8.
        let code: &[u8] = b"# -*- coding: latin-1 -*-\nx = \"hello \xa6 world\"\n";
        check_string(code, "hello � world");
    }

    #[test]
    fn test_utf8_encoding() {
        let code: &[u8] = b"# -*- coding: utf-8 -*-\nx = \"hello world\"\n";
        check_string(code, "hello world");
    }

    #[test]
    fn test_utf8_with_unicode() {
        let code = "# -*- coding: utf-8 -*-\nx = \"hello 世界 🌍\"\n";
        check_string(code.as_bytes(), "hello 世界 🌍");
    }

    #[test]
    fn test_windows_1252_encoding() {
        // Windows-1252 smart quotes (0x93, 0x94) need ASCII string delimiters.
        let code: &[u8] = b"# -*- coding: cp1252 -*-\nx = \"\x93hello\x94\"\n";
        check_string(code, "�hello�");
    }

    #[test]
    fn test_non_utf8_syntax_recovery() {
        let code: &[u8] = b"# -*- coding: cp1252 -*-\nx = \x93hello\x94\n";
        let module = parse(code).unwrap();
        assert!(module.first_syntax_error().is_some());
    }

    #[test]
    fn test_utf8_syntax_error_rejected() {
        let code = "# -*- coding: cp1252 -*-\nx = �hello�\n";
        assert!(parse(code.as_bytes()).is_err());
    }
}
