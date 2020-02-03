// © 2020, ETH Zurich
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

import * as vscode from 'vscode';
import * as path from 'path';

const USER_HOME_DIR = require('os').homedir();
const RUST_LIFE_HOME_DIR = ".rust-life";

/**
 * Gives the home dir of RustLife This might depend on the context of the extension in the future, e.g. using the
 * globalStoragePath. But for now it is hard-coded to ~/.rust-life, e.g the folder .rust-life in the users home.
 * @param context 
 */
export function rustLifeHome(context: vscode.ExtensionContext): string {
	return path.join(USER_HOME_DIR, RUST_LIFE_HOME_DIR);
}

const RUST_LIFE_EXE_NAME = "extract-error";

export function rustLifeExe(context: vscode.ExtensionContext): string {
	return path.join(rustLifeHome(context), RUST_LIFE_EXE_NAME);
}

const RUST_VERSION = "nightly-2019-05-21-x86_64-unknown-linux-gnu";

/**
 * This function returns the rust version that must be used to run this edition of rust-life. More exactly, it gives the
 * name of the directory that contains this toolchain in the ~/.rustup/toolchains folder of the system. This toolchain
 * must be installed on the system in order to allow this extension/plug-in to work.
 * WARNING: This function, and all functions that are based on it (obviously) only work on gnu/linux x86_64 systems.
 */
export function rustVersion(): string {
	return RUST_VERSION;
}

/**
 * This function gives the path to the root directory of the rust version/toolchain that must be used for
 * rust-life (compiler mod, extract-error). It must be passed as --sysroot argument when running it.
 */
export function rustCompilerPath(): string {
	return path.join(USER_HOME_DIR, ".rustup/toolchains", rustVersion());
}

/**
 * This function gives the path to the lib (library) directory of the rust version/toolchain that must be used for
 * rust-life (compiler mod, extract-error). It must be exported as LD_LIBRARY_PATH before running it.
 */
export function rustLibPath(): string {
	return path.join(rustCompilerPath(), "lib");
}