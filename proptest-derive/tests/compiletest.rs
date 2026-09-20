// Copyright 2018 The proptest developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

extern crate compiletest_rs as ct;

use std::{
    env, fs,
    path::{Path, PathBuf},
};

/// Builds the flags compiletest needs to invoke rustc without Cargo.
///
/// Cargo normally passes its dependencies to rustc. Nightly now stores those
/// artifacts in per-package directories rather than a shared `deps` directory,
/// so the compile-fail fixtures must receive explicit paths to `proptest` and
/// the `proptest_derive` proc macro, plus paths to their dependencies.
fn rustcflags() -> String {
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/debug");
    let build = target.join("build");
    let artifact = |directory: &str, crate_name: &str, extension: &str| {
        fs::read_dir(build.join(directory))
            .expect("missing Cargo build artifacts")
            .filter_map(Result::ok)
            .map(|entry| entry.path().join("out"))
            .filter_map(|dir| fs::read_dir(dir).ok())
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.extension().is_some_and(|ext| ext == extension)
                    && path.file_name().is_some_and(|name| {
                        name.to_string_lossy()
                            .starts_with(&format!("lib{crate_name}-"))
                    })
            })
            .expect("missing Cargo crate artifact")
    };

    let mut flags = format!(
        "--extern proptest={} --extern proptest_derive={} --edition=2021",
        // `rmeta` is enough to type-check against the library crate.
        artifact("proptest", "proptest", "rmeta").display(),
        // Proc macros must be loaded as platform-specific dynamic libraries
        // `env::consts::DLL_EXTENSION` allows this to work on Mac or Linux.
        artifact(
            "proptest-derive",
            "proptest_derive",
            env::consts::DLL_EXTENSION,
        )
        .display(),
    );
    for package in fs::read_dir(&build).expect("missing Cargo build artifacts")
    {
        for artifact in
            fs::read_dir(package.expect("invalid Cargo build artifact").path())
                .expect("invalid Cargo build artifacts")
        {
            let out = artifact
                .expect("invalid Cargo build artifact")
                .path()
                .join("out");
            if out.is_dir() {
                flags.push_str(&format!(" -L {}", out.display()));
            }
        }
    }
    flags
}

fn run_mode(src: &'static str, mode: &'static str) {
    let mut config = ct::Config::default();

    config.mode = mode.parse().expect("invalid mode");
    config.rustc_path = env::var_os("RUSTC")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("rustc"));
    config.target_rustcflags = Some(rustcflags());
    if let Ok(name) = env::var("TESTNAME") {
        config.filters = vec![name];
    }
    config.src_base = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(src);
    config.clean_rmeta();

    ct::run_tests(&config);
}

#[test]
fn compile_test() {
    run_mode("compile-fail", "compile-fail");
}
