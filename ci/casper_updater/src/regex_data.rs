#![allow(clippy::wildcard_imports)]

use once_cell::sync::Lazy;
use regex::Regex;

use crate::dependent_file::DependentFile;

pub static MANIFEST_NAME_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?m)(^name = )"([^"]+)"#).unwrap());
pub static MANIFEST_VERSION_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?m)(^version = )"([^"]+)"#).unwrap());
pub static PACKAGE_JSON_NAME_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?m)(^  "name": )"([^"]+)"#).unwrap());
pub static PACKAGE_JSON_VERSION_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?m)(^  "version": )"([^"]+)"#).unwrap());
pub static TYPES_VERSION_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?m)(^casper-types = \{[^\}]*version = )"(?:[^"]+)"#).unwrap());
pub static CASPER_STORAGE_VERSION_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?m)(^casper-storage = \{[^\}]*version = )"(?:[^"]+)"#).unwrap());

fn replacement(updated_version: &str) -> String {
    format!(r#"$1"{}"#, updated_version)
}

fn replacement_with_slash(updated_version: &str) -> String {
    format!(r#"$1/{}"#, updated_version)
}

pub mod types {
    use super::*;

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
        vec![
            DependentFile::new(
                "storage/Cargo.toml",
                TYPES_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "execution_engine/Cargo.toml",
                TYPES_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "execution_engine_testing/test_support/Cargo.toml",
                TYPES_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new("node/Cargo.toml", TYPES_VERSION_REGEX.clone(), replacement),
            DependentFile::new(
                "binary_port/Cargo.toml",
                TYPES_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "smart_contracts/contract/Cargo.toml",
                TYPES_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "types/Cargo.toml",
                MANIFEST_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "types/src/lib.rs",
                Regex::new(
                    r#"(?m)(#!\[doc\(html_root_url = "https://docs.rs/casper-types)/(?:[^"]+)"#,
                )
                .unwrap(),
                replacement_with_slash,
            ),
        ]
    });
}

pub mod binary_port {
    use super::*;

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
        vec![DependentFile::new(
            "binary_port/Cargo.toml",
            MANIFEST_VERSION_REGEX.clone(),
            replacement,
        )]
    });
}
pub mod storage {
    use super::*;

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
        vec![
            DependentFile::new(
                "execution_engine/Cargo.toml",
                CASPER_STORAGE_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "execution_engine_testing/test_support/Cargo.toml",
                CASPER_STORAGE_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "node/Cargo.toml",
                CASPER_STORAGE_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "storage/Cargo.toml",
                MANIFEST_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "storage/src/lib.rs",
                Regex::new(
                    r#"(?m)(#!\[doc\(html_root_url = "https://docs.rs/casper-storage)/(?:[^"]+)"#,
                )
                .unwrap(),
                replacement_with_slash,
            ),
        ]
    });
}

pub mod execution_engine {
    use super::*;

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
        vec![
                DependentFile::new(
                    "execution_engine_testing/test_support/Cargo.toml",
                    Regex::new(r#"(?m)(^casper-execution-engine = \{[^\}]*version = )"(?:[^"]+)"#)
                        .unwrap(),
                    replacement,
                ),
                DependentFile::new(
                    "node/Cargo.toml",
                    Regex::new(r#"(?m)(^casper-execution-engine = \{[^\}]*version = )"(?:[^"]+)"#)
                        .unwrap(),
                    replacement,
                ),
                DependentFile::new(
                    "execution_engine/Cargo.toml",
                    MANIFEST_VERSION_REGEX.clone(),
                    replacement,
                ),
                DependentFile::new(
                    "execution_engine/src/lib.rs",
                    Regex::new(r#"(?m)(#!\[doc\(html_root_url = "https://docs.rs/casper-execution-engine)/(?:[^"]+)"#).unwrap(),
                    replacement_with_slash,
                ),
            ]
    });
}

pub mod node {
    use super::*;

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
        vec![
            DependentFile::new(
                "node/Cargo.toml",
                MANIFEST_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "node/src/lib.rs",
                Regex::new(
                    r#"(?m)(#!\[doc\(html_root_url = "https://docs.rs/casper-node)/(?:[^"]+)"#,
                )
                .unwrap(),
                replacement_with_slash,
            ),
        ]
    });
}

pub mod smart_contracts_contract {
    use super::*;

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
        vec![
            DependentFile::new(
                "smart_contracts/contract/Cargo.toml",
                MANIFEST_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "smart_contracts/contract/src/lib.rs",
                Regex::new(
                    r#"(?m)(#!\[doc\(html_root_url = "https://docs.rs/casper-contract)/(?:[^"]+)"#,
                )
                .unwrap(),
                replacement_with_slash,
            ),
        ]
    });
}

pub mod smart_contracts_contract_as {
    use super::*;

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
        vec![
            DependentFile::new(
                "smart_contracts/contract_as/package.json",
                PACKAGE_JSON_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "smart_contracts/contract_as/package-lock.json",
                PACKAGE_JSON_VERSION_REGEX.clone(),
                replacement,
            ),
        ]
    });
}

pub mod execution_engine_testing_test_support {
    use super::*;

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
        vec![
                DependentFile::new(
                    "execution_engine_testing/test_support/Cargo.toml",
                    MANIFEST_VERSION_REGEX.clone(),
                    replacement,
                ),
                DependentFile::new(
                    "execution_engine_testing/test_support/src/lib.rs",
                    Regex::new(r#"(?m)(#!\[doc\(html_root_url = "https://docs.rs/casper-engine-test-support)/(?:[^"]+)"#).unwrap(),
                    replacement_with_slash,
                ),
            ]
    });
}
